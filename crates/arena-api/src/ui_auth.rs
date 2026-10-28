//! Staff sign-in for the local Hub UI, on the loopback server (127.0.0.1, same origin guard as
//! `/ui/snapshot`):
//!
//! - `POST /ui/auth/sign-in {login, password}` answers [`SignInOk`], or `AUTH_FAILED` (401) with
//!   one fixed message for every cause. Five consecutive failures for a login (compared without
//!   case) pause it for 60 s: every attempt in that window, right or wrong, gets `AUTH_LOCKED`
//!   (429) with `retry_after_s`. A success resets the count. The lockout is in memory, keyed
//!   by login, and timed by the injected [`Clock`].
//! - `POST /ui/auth/sign-out` (bearer token) ends the session.
//! - `GET /ui/me` (bearer token) answers the session's [`SessionStaff`] and whether it is
//!   locked ([`UiMe`]), or 401.
//! - `POST /ui/elevate {items: [{intent_kind, key?}], pin}` (bearer token) checks a manager's
//!   PIN for exactly the listed intents (1–10; `settings.set` items name their setting key),
//!   which the signed-in member cannot do alone. A follower refuses first (`NOT_PRIMARY`), an
//!   unhandled kind is `UNSUPPORTED`, and a member who already holds every permission gets
//!   `VALIDATION_FAILED` on `items`. The PIN is checked with argon2id (at most 2 checks at once)
//!   against the active staff (never the session's own member) whose role allows every
//!   permission the items need, at most 50. A match answers [`ElevateOk`]: a ticket of 128
//!   random bits, held in memory for this session, redeemable once per approved item within
//!   60 s. Anything else is `PIN_REJECTED` (403) with one fixed message that names no one. Five
//!   failures in a row pause elevation for the requesting staff member (whatever the session)
//!   for 60 s (`AUTH_LOCKED`, 429, with `retry_after_s`; the fifth failure already answers it).
//!   The PIN is never logged, stored or echoed.
//! - `POST /ui/elevate {items: [{permission}], pin}` (bearer token) is a read elevation: one
//!   item naming a read permission ([`Permission::READ_ELEVATION`]: `audit.view`,
//!   `report.view`, `player.view`), alone. The same PIN check, throttle and refusals apply (a
//!   follower may grant one: nothing is written). The ticket it answers is bound to the session,
//!   the member and that permission, allows unlimited reads for 10 min ([`READ_TICKET_TTL_MS`])
//!   and is sent in the `X-Arena-Elevation` header ([`crate::ui_audit`]); it never redeems an
//!   intent. A lock, a re-bind or signing out drops it like any ticket.
//! - `POST /ui/elevate/drop {ticket}` (bearer token) gives up a ticket's unused approvals (204);
//!   signing out drops the session's tickets.
//! - `POST /ui/intent {intent, elevation_ticket?}` (bearer token) submits the intent with the
//!   session's `actor{staff, staff_id}`, whatever actor the client sent, and drops any
//!   client-supplied `elevated_by`. A ticket is redeemed for one unused approved item matching
//!   the intent (kind, and the key of a `settings.set`; else `PIN_REQUIRED`) and becomes
//!   `elevated_by{pin, staff_id, hub, verifier_version}`, which `apply()` re-checks. A refusal
//!   (nothing applied) gives the item back.
//! - `POST /ui/lock` (bearer token) idle-locks the session (204): until it is unlocked,
//!   `/ui/intent`, `/ui/elevate` and the `/ui/audit*` reads answer `SESSION_LOCKED` (423) and
//!   the session's tickets are dropped. Nothing else stops: `/ui/events` keeps streaming and the Hub keeps applying.
//! - `POST /ui/unlock {pin}` (bearer token) only unlocks a locked session (otherwise
//!   `VALIDATION_FAILED` on `session`, 409, before any PIN check). It checks the PIN against
//!   the session's own member first, then the other active staff with a PIN (at most 50 checks,
//!   argon2id). The own PIN unlocks the session as it was; another member's unlocks it and
//!   re-binds it to them (so every later intent carries their actor, and tickets issued before
//!   are refused), and [`UnlockOk`] says so. No match is `PIN_REJECTED` with one fixed message
//!   ("that PIN can't unlock this screen"). Five failures in a row, counted both per staff member and per token, pause
//!   unlocking: 60 s, doubling with each further pause (at most 15 min) until a successful
//!   unlock (`AUTH_LOCKED` with `retry_after_s`); signing out still works. Locking and
//!   unlocking push `session.locked` / `session.unlocked` to `/ui/events` streams of the same
//!   token. `ui.locked` / `ui.unlocked` are logged with staff ids only, never the PIN.
//! - `GET /ui/staff` (bearer token, `user.manage`) lists the active and invited staff with the//!   `users` limit in effect and how many count against it. `POST /ui/staff/deactivate//!   {staff_id, pin, idempotency_key}` (an Owner) checks the caller's **own** PIN under the//!   elevation throttle and applies `staff.deactivate`; the generic `/ui/intent` refuses that kind.
//! - `GET /ui/settings` (bearer token) lists the branch settings with their `rev`, so an edit
//!   can carry `base_rev`.
//!
//! A token is 32 random bytes in base64url, held only in this process's session map: sessions
//! end when the Hub restarts, 12 h after sign-in, or as soon as the member is deactivated or
//! removed (every authenticated request re-reads the staff row, which also refreshes the name
//! and role). A member holds at most 5 sessions; signing in again ends the oldest. No token, or
//! an unknown one, is `AUTH_FAILED` (401); signing out an unknown token is still 204.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arena_core::staff::{
    AUTH_FAILED_MESSAGE, new_elevation_ticket, new_session_token, pin_rejected, pin_required,
};
use arena_core::{Clock, SnapshotReader, StaffDirectory, WriterHandle};
use arena_proto::{
    ActorKind, AppError, ElevateDrop, ElevateOk, ElevateRequest, ElevatedBy, ElevationItem,
    ElevationKind, ElevationMode, ErrorCode, IntentKind, MAX_TICKET_USES, Permission, Requirement,
    SessionStaff, SetupCodeRequest, SignInOk, SignInRequest, UiError, UiIntentRequest,
    UiIntentResult, UiMe, UiPlayerRequest, UnlockOk, UnlockRequest,
};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

/// Consecutive failures that pause a login.
pub const LOCKOUT_FAILURES: u32 = 5;
/// How long a paused login stays paused.
pub const LOCKOUT_MS: i64 = 60_000;
/// Lockout entries kept before the ones with nothing to remember are dropped.
const MAX_LOCKOUT_ENTRIES: usize = 4096;
/// How long a session lives after sign-in, whatever its use.
pub const SESSION_TTL_MS: i64 = 12 * 60 * 60 * 1000;
/// Sessions one staff member may hold; signing in again ends the oldest.
pub const MAX_SESSIONS_PER_STAFF: usize = 5;
/// How long an elevation ticket lives.
pub const TICKET_TTL_MS: i64 = 60_000;
/// How long a read-elevation ticket lives (unlimited reads meanwhile).
pub const READ_TICKET_TTL_MS: i64 = 10 * 60_000;
/// Unexpired tickets one session may hold; a new one replaces the oldest.
const MAX_TICKETS_PER_SESSION: usize = 4;

/// PIN checks (each up to 50 argon2id verifications) that may run at once.
const MAX_CONCURRENT_PIN_CHECKS: usize = 2;
const MAX_SETUP_FIELD_BYTES: usize = 64;

/// A granted elevation, held only in memory.
#[derive(Debug, Clone)]
struct Ticket {
    /// The session token it was issued to.
    session: String,
    /// The staff member the session acted as when it was issued (a re-bound session cannot
    /// redeem it).
    requester: String,
    /// The approving staff member.
    staff_id: String,
    verifier_version: i64,
    expires_at: i64,
    /// The approved items: intents, each redeemed once (`true` once used), or one read
    /// permission, never redeemed by an intent.
    items: Vec<(ElevationItem, bool)>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Entry {
    failures: u32,
    /// Attempts reserved by [`Lockout::begin`] and not yet finished.
    in_flight: u32,
    locked_until: Option<i64>,
}

impl Entry {
    fn locked_at(&self, now_ms: i64) -> bool {
        self.locked_until.is_some_and(|until| until > now_ms)
    }

    /// Nothing worth keeping: no failures, no attempt in flight, no live pause.
    fn idle(&self, now_ms: i64) -> bool {
        self.failures == 0 && self.in_flight == 0 && !self.locked_at(now_ms)
    }
}

/// Sign-in attempts per login (lower-cased), in memory. An attempt is reserved before the
/// password is checked ([`begin`](Self::begin)) and settled after ([`finish`](Self::finish)),
/// so attempts in flight count toward the limit: concurrent attempts cannot all slip past it.
#[derive(Debug, Default)]
pub struct Lockout {
    entries: HashMap<String, Entry>,
    /// Keys are used as given (staff ids), not trimmed and lower-cased like logins.
    exact: bool,
}

fn lockout_key(login: &str) -> String {
    login.trim().to_lowercase()
}

fn retry_after(until: i64, now_ms: i64) -> u32 {
    u32::try_from((until - now_ms + 999) / 1000).unwrap_or(u32::MAX)
}

impl Lockout {
    /// A lockout whose keys are compared exactly (the PIN elevation throttle, keyed by staff id).
    pub fn exact() -> Self {
        Self {
            entries: HashMap::new(),
            exact: true,
        }
    }

    fn key(&self, login: &str) -> String {
        if self.exact {
            login.to_owned()
        } else {
            lockout_key(login)
        }
    }

    /// Releases an attempt reserved by [`begin`](Self::begin) without settling it: it counts
    /// neither as a failure nor as a success (the check itself could not run).
    pub fn release(&mut self, login: &str, now_ms: i64) {
        let key = self.key(login);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.in_flight = entry.in_flight.saturating_sub(1);
            if entry.idle(now_ms) {
                self.entries.remove(&key);
            }
        }
    }
    /// Reserves one attempt for `login`, or `Err(retry_after_s)` while it is paused, or while
    /// failures plus attempts in flight already reach the limit.
    pub fn begin(&mut self, login: &str, now_ms: i64) -> Result<(), u32> {
        if self.entries.len() >= MAX_LOCKOUT_ENTRIES {
            // Live failure counts and pauses are never dropped: a spray of junk logins cannot
            // reset a real login's count.
            self.entries.retain(|_, e| !e.idle(now_ms));
        }
        let key = self.key(login);
        let entry = self.entries.entry(key).or_default();
        match entry.locked_until {
            Some(until) if until > now_ms => return Err(retry_after(until, now_ms)),
            Some(_) => {
                entry.locked_until = None;
                entry.failures = 0;
            }
            None => {}
        }
        if entry.failures + entry.in_flight >= LOCKOUT_FAILURES {
            return Err(retry_after(now_ms + LOCKOUT_MS, now_ms));
        }
        entry.in_flight += 1;
        Ok(())
    }

    /// How long `login` stays paused, if it is paused now (reserves nothing).
    pub fn paused_for(&self, login: &str, now_ms: i64) -> Option<u32> {
        self.entries
            .get(&self.key(login))
            .and_then(|e| e.locked_until)
            .filter(|until| *until > now_ms)
            .map(|until| retry_after(until, now_ms))
    }

    /// Settles an attempt reserved by [`begin`](Self::begin): a success resets the count; the
    /// fifth failure in a row pauses the login for [`LOCKOUT_MS`].
    pub fn finish(&mut self, login: &str, now_ms: i64, success: bool) {
        let key = self.key(login);
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        entry.in_flight = entry.in_flight.saturating_sub(1);
        if success {
            entry.failures = 0;
            entry.locked_until = None;
        } else {
            entry.failures += 1;
            if entry.failures >= LOCKOUT_FAILURES {
                entry.failures = 0;
                entry.locked_until = Some(now_ms + LOCKOUT_MS);
            }
        }
        if entry.idle(now_ms) {
            self.entries.remove(&key);
        }
    }
}

/// The first unlock pause; each further consecutive pause doubles it.
pub const UNLOCK_PAUSE_MS: i64 = 60_000;
/// The longest unlock pause.
pub const UNLOCK_PAUSE_MAX_MS: i64 = 15 * 60_000;

#[derive(Debug, Clone, Copy, Default)]
struct UnlockEntry {
    failures: u32,
    in_flight: u32,
    locked_until: Option<i64>,
    /// Pauses in a row since the last successful unlock.
    pauses: u32,
}

impl UnlockEntry {
    fn idle(&self, now_ms: i64) -> bool {
        self.failures == 0
            && self.in_flight == 0
            && self.pauses == 0
            && self.locked_until.is_none_or(|u| u <= now_ms)
    }
}

/// Unlock attempts, counted under several keys at once (the staff member and the token): any
/// key at its limit refuses. Each pause doubles the previous one (60 s, 120 s, … 15 min); only
/// a successful unlock resets the doubling.
#[derive(Debug, Default)]
pub struct UnlockThrottle {
    entries: HashMap<String, UnlockEntry>,
}

impl UnlockThrottle {
    /// Reserves one attempt under every key, or `Err(retry_after_s)` (nothing reserved).
    pub fn begin(&mut self, keys: &[String], now_ms: i64) -> Result<(), u32> {
        if self.entries.len() >= MAX_LOCKOUT_ENTRIES {
            self.entries.retain(|_, e| !e.idle(now_ms));
        }
        let mut wait: Option<u32> = None;
        for key in keys {
            let e = self.entries.entry(key.clone()).or_default();
            match e.locked_until {
                Some(until) if until > now_ms => {
                    wait = wait.max(Some(retry_after(until, now_ms)));
                }
                Some(_) => {
                    e.locked_until = None;
                    e.failures = 0;
                }
                None => {}
            }
            if e.failures + e.in_flight >= LOCKOUT_FAILURES {
                wait = wait.max(Some(retry_after(now_ms + UNLOCK_PAUSE_MS, now_ms)));
            }
        }
        if let Some(w) = wait {
            return Err(w);
        }
        for key in keys {
            if let Some(e) = self.entries.get_mut(key) {
                e.in_flight += 1;
            }
        }
        Ok(())
    }

    /// Releases a reservation without counting it either way.
    pub fn release(&mut self, keys: &[String], now_ms: i64) {
        for key in keys {
            if let Some(e) = self.entries.get_mut(key) {
                e.in_flight = e.in_flight.saturating_sub(1);
                if e.idle(now_ms) {
                    self.entries.remove(key);
                }
            }
        }
    }

    /// Settles a reservation. A success resets every key; a failure counts, and the fifth in a
    /// row pauses that key (answers the longest pause now in force, if any, and the pauses).
    pub fn finish(&mut self, keys: &[String], now_ms: i64, success: bool) -> Option<(u32, u32)> {
        let mut paused: Option<(u32, u32)> = None;
        for key in keys {
            let Some(e) = self.entries.get_mut(key) else {
                continue;
            };
            e.in_flight = e.in_flight.saturating_sub(1);
            if success {
                *e = UnlockEntry {
                    in_flight: e.in_flight,
                    ..UnlockEntry::default()
                };
            } else {
                e.failures += 1;
                if e.failures >= LOCKOUT_FAILURES {
                    e.failures = 0;
                    e.pauses = e.pauses.saturating_add(1);
                    let factor = 1i64 << e.pauses.saturating_sub(1).min(10);
                    let pause = UNLOCK_PAUSE_MS
                        .saturating_mul(factor)
                        .min(UNLOCK_PAUSE_MAX_MS);
                    e.locked_until = Some(now_ms + pause);
                }
                if let Some(until) = e.locked_until.filter(|u| *u > now_ms) {
                    let r = retry_after(until, now_ms);
                    if paused.is_none_or(|(w, _)| r > w) {
                        paused = Some((r, e.pauses));
                    }
                }
            }
            if e.idle(now_ms) {
                self.entries.remove(key);
            }
        }
        paused
    }

    /// Forgets `key` (a token whose session ended).
    pub fn forget(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

fn unlock_keys(staff_id: &str, token: &str) -> [String; 2] {
    [format!("staff:{staff_id}"), format!("token:{token}")]
}

#[derive(Debug, Clone)]
struct Session {
    staff: SessionStaff,
    created_at: i64,
    /// Idle-locked: intents and elevation are refused until a PIN unlocks it.
    locked: bool,
}

/// The state behind the auth routes.
pub struct UiAuth {
    staff: Arc<StaffDirectory>,
    reader: Arc<SnapshotReader>,
    writer: Option<WriterHandle>,
    clock: Arc<dyn Clock>,
    lockout: Mutex<Lockout>,
    setup_checks: Arc<tokio::sync::Semaphore>,
    setup_lockout: Mutex<Lockout>,
    /// PIN elevation failures, keyed by the requesting staff id (so a new session does not
    /// reset them).
    elevate_lockout: Mutex<Lockout>,
    /// Unlock failures, keyed by the locked session's staff id and by token.
    unlock_lockout: Mutex<UnlockThrottle>,
    /// Tests: an unlock that matched a PIN signals the first and waits for the second.
    #[cfg(test)]
    unlock_pause: Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    /// Bounds the argon2id PIN checks running at once.
    pin_checks: Arc<tokio::sync::Semaphore>,
    /// Elevation tickets, keyed by ticket.
    tickets: Mutex<HashMap<String, Ticket>>,
    sessions: Mutex<HashMap<String, Session>>,
    /// Bumped whenever a session ends, so open `/ui/events` streams re-check theirs.
    ended: tokio::sync::watch::Sender<u64>,
    /// The Hub's role: `/ui/events` streams are reset when it changes.
    status: Option<tokio::sync::watch::Receiver<arena_core::HubStatus>>,
    events_config: crate::ui_events::EventsConfig,
    /// The shared `/ui/events` fan-out, started by the first stream.
    events: std::sync::OnceLock<Arc<crate::ui_events::EventsHub>>,
    /// The audit viewer's read-only connection, opened on first use.
    audit: std::sync::OnceLock<Arc<arena_core::AuditReader>>,
    /// Device presence on the LAN: `/ui/events` pushes `devices.changed` when it changes.
    presence: crate::lan::Presence,
    backup_probe: BackupProbe,
    backup_probe_timeout: std::time::Duration,
    backup_folder: Option<Arc<dyn arena_core::setup::BackupFolder>>,
    printer: Option<Arc<dyn arena_core::printer::Printer>>,
    printer_find: tokio::sync::Mutex<()>,
    printer_jobs: Arc<arena_core::printer::PrinterJobs>,
    pairing: Option<Arc<crate::pairing::Pairing>>,
    streams: Mutex<HashMap<String, usize>>,
    pair_gate: Option<arena_core::PairGate>,
    kek: Option<Arc<dyn arena_core::players::Kek>>,
    player_index: Arc<Mutex<arena_core::players::PlayerIndex>>,
}

pub type BackupProbe =
    Arc<dyn Fn(&std::path::Path) -> Result<(), arena_proto::BackupDirProblem> + Send + Sync>;

pub const BACKUP_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl std::fmt::Debug for UiAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiAuth").finish_non_exhaustive()
    }
}

impl UiAuth {
    /// `writer` is `None` on a follower: intents then fail `NOT_PRIMARY`. Lockout and session
    /// timing come from `clock`.
    pub fn new(
        staff: Arc<StaffDirectory>,
        reader: Arc<SnapshotReader>,
        writer: Option<WriterHandle>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            staff,
            reader,
            writer,
            clock,
            lockout: Mutex::new(Lockout::default()),
            setup_checks: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PIN_CHECKS)),
            setup_lockout: Mutex::new(Lockout::default()),
            elevate_lockout: Mutex::new(Lockout::exact()),
            unlock_lockout: Mutex::new(UnlockThrottle::default()),
            #[cfg(test)]
            unlock_pause: Mutex::new(None),
            pin_checks: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PIN_CHECKS)),
            tickets: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            ended: tokio::sync::watch::channel(0).0,
            status: None,
            events_config: crate::ui_events::EventsConfig::default(),
            events: std::sync::OnceLock::new(),
            audit: std::sync::OnceLock::new(),
            presence: crate::lan::Presence::none(),
            backup_probe: Arc::new(arena_core::setup::probe_backup_dir),
            backup_probe_timeout: BACKUP_PROBE_TIMEOUT,
            backup_folder: None,
            printer: None,
            printer_find: tokio::sync::Mutex::new(()),
            printer_jobs: Arc::default(),
            pairing: None,
            pair_gate: None,
            kek: None,
            player_index: Arc::default(),
            streams: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_player_kek(mut self, kek: Arc<dyn arena_core::players::Kek>) -> Self {
        self.kek = Some(kek);
        self
    }

    pub fn with_pair_gate(mut self, gate: arena_core::PairGate) -> Self {
        self.pair_gate = Some(gate);
        self
    }

    pub fn with_printer(mut self, printer: Arc<dyn arena_core::printer::Printer>) -> Self {
        self.printer = Some(printer);
        self
    }

    pub(crate) fn printer(&self) -> Option<&Arc<dyn arena_core::printer::Printer>> {
        self.printer.as_ref()
    }

    pub(crate) fn printer_find_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.printer_find
    }

    pub(crate) fn printer_job_lock(&self) -> &tokio::sync::Mutex<()> {
        self.printer_jobs.lock()
    }

    /// Shares the printer job lock and last failure with the Hub's print task.
    pub fn with_printer_jobs(mut self, jobs: Arc<arena_core::printer::PrinterJobs>) -> Self {
        self.printer_jobs = jobs;
        self
    }

    /// The last print job that failed, until a job succeeds.
    pub(crate) fn printer_jobs(&self) -> &arena_core::printer::PrinterJobs {
        &self.printer_jobs
    }

    pub fn with_pairing(mut self, pairing: Arc<crate::pairing::Pairing>) -> Self {
        self.pairing = Some(pairing);
        self
    }

    pub(crate) fn pairing(&self) -> Option<&Arc<crate::pairing::Pairing>> {
        self.pairing.as_ref()
    }

    pub(crate) fn stream_opened(&self, token: &str) {
        if let Ok(mut streams) = self.streams.lock() {
            *streams.entry(token.to_owned()).or_default() += 1;
        }
    }

    pub(crate) fn stream_closed(&self, token: &str) {
        if let Ok(mut streams) = self.streams.lock()
            && let Some(n) = streams.get_mut(token)
        {
            *n = n.saturating_sub(1);
            if *n == 0 {
                streams.remove(token);
            }
        }
    }

    pub(crate) fn has_stream(&self, token: &str) -> bool {
        self.streams
            .lock()
            .is_ok_and(|s| s.get(token).is_some_and(|n| *n > 0))
    }

    /// Pushes `devices.changed` to `/ui/events` streams whenever a device comes online or
    /// goes offline on the LAN.
    pub fn with_presence(mut self, presence: crate::lan::Presence) -> Self {
        self.presence = presence;
        self
    }

    pub fn with_backup_probe(mut self, probe: BackupProbe, timeout: std::time::Duration) -> Self {
        self.backup_probe = probe;
        self.backup_probe_timeout = timeout;
        self
    }

    /// Checks a folder chosen through the platform picker (a tree URI) with `folder` instead of the
    /// disk; without it such a location is refused.
    pub fn with_backup_folder(mut self, folder: Arc<dyn arena_core::setup::BackupFolder>) -> Self {
        self.backup_folder = Some(folder);
        self
    }

    async fn probe_backup_dir(
        &self,
        intent: &arena_proto::Intent,
        authorized: bool,
    ) -> Result<(), UiError> {
        let arena_proto::IntentPayload::SettingsSet(set) = &intent.payload else {
            return Ok(());
        };
        if set.key != arena_proto::KEY_BACKUP_DIR
            || !authorized
            || intent.kind != intent.payload.kind()
            || !arena_proto::is_backup_location(&set.value)
        {
            return Ok(());
        }
        let reader = Arc::clone(&self.reader);
        let key = intent.idempotency_key.clone();
        let (rev, replay) = tokio::task::spawn_blocking(move || {
            reader.setting_rev_and_replay(arena_proto::KEY_BACKUP_DIR, &key)
        })
        .await
        .map_err(|_| internal())?
        .map_err(ui_error)?;
        if replay || rev.is_none() || rev != intent.base_rev {
            return Ok(());
        }
        let job = if arena_proto::is_backup_tree_uri(&set.value) {
            let folder = self.backup_folder.clone();
            let location = set.value.clone();
            tokio::task::spawn_blocking(move || {
                arena_core::setup::probe_location(folder.as_deref(), &location)
            })
        } else {
            let probe = Arc::clone(&self.backup_probe);
            let path = std::path::PathBuf::from(&set.value);
            tokio::task::spawn_blocking(move || probe(&path))
        };
        match tokio::time::timeout(self.backup_probe_timeout, job).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(problem))) => Err(ui_error(arena_core::setup::backup_dir_error(problem))),
            Ok(Err(_)) => Err(internal()),
            Err(_) => {
                tracing::warn!("the backup location probe timed out");
                Err(ui_error(arena_core::setup::backup_dir_error(
                    arena_proto::BackupDirProblem::NotWritable,
                )))
            }
        }
    }

    pub(crate) fn presence(&self) -> &crate::lan::Presence {
        &self.presence
    }

    /// The audit viewer's reader, opened on the snapshot reader's database the first time.
    pub(crate) fn audit_reader(&self) -> Result<Arc<arena_core::AuditReader>, UiError> {
        if let Some(r) = self.audit.get() {
            return Ok(Arc::clone(r));
        }
        let path = self.reader.db_path().map_err(ui_error)?;
        let opened = Arc::new(arena_core::AuditReader::open(path).map_err(ui_error)?);
        // A concurrent first use may have won: keep whichever was stored.
        Ok(Arc::clone(self.audit.get_or_init(|| opened)))
    }

    /// Resets `/ui/events` streams whenever this Hub's role changes (promotion or demotion), so
    /// clients reopen them against the new writer.
    pub fn with_status(
        mut self,
        status: tokio::sync::watch::Receiver<arena_core::HubStatus>,
    ) -> Self {
        self.status = Some(status);
        self
    }

    /// Overrides the `/ui/events` timings and buffer (tests).
    pub fn with_events_config(mut self, config: crate::ui_events::EventsConfig) -> Self {
        self.events_config = config;
        self
    }

    pub(crate) fn events_config(&self) -> &crate::ui_events::EventsConfig {
        &self.events_config
    }

    pub(crate) fn status(&self) -> Option<&tokio::sync::watch::Receiver<arena_core::HubStatus>> {
        self.status.as_ref()
    }

    pub(crate) fn events_hub(&self) -> &std::sync::OnceLock<Arc<crate::ui_events::EventsHub>> {
        &self.events
    }

    /// The cheap per-event check: the session is still in the map and within its 12 h (no
    /// database read). An expired session is dropped.
    pub(crate) fn session_live(&self, token: &str) -> bool {
        let now = self.clock.now_ms();
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        match sessions.get(token) {
            Some(s) if now - s.created_at < SESSION_TTL_MS => true,
            Some(_) => {
                sessions.remove(token);
                drop(sessions);
                self.session_ended();
                false
            }
            None => false,
        }
    }

    pub(crate) fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub(crate) fn reader(&self) -> &Arc<SnapshotReader> {
        &self.reader
    }

    pub(crate) fn writer(&self) -> Option<&WriterHandle> {
        self.writer.as_ref()
    }

    /// A receiver that changes whenever a session ends.
    pub(crate) fn session_ends(&self) -> tokio::sync::watch::Receiver<u64> {
        self.ended.subscribe()
    }

    /// The session behind `token` when it may act: `SESSION_LOCKED` while it is locked.
    pub(crate) async fn acting_session(&self, token: &str) -> Result<SessionStaff, UiError> {
        let (me, locked) = self.session_state(token).await?;
        if locked {
            return Err(session_locked());
        }
        Ok(me)
    }

    /// Pushes a `session.*` event to the `/ui/events` streams of `token` (if any are open).
    fn push_session(&self, token: &str, name: &'static str, data: String) {
        if let Some(hub) = self.events.get() {
            hub.push_session(token, name, data);
        }
    }

    fn session_ended(&self) {
        self.ended.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// The session behind the bearer token. Each call re-reads the staff row: a session past
    /// [`SESSION_TTL_MS`], or whose member is gone or inactive, is dropped (`AUTH_FAILED`);
    /// otherwise its name and role are refreshed.
    async fn session(&self, headers: &HeaderMap) -> Result<SessionStaff, UiError> {
        let token = bearer(headers).ok_or_else(auth_failed)?.to_owned();
        self.session_for(&token).await
    }

    /// Like [`session`](Self::session), for a token given directly (`/ui/events?token=`).
    pub(crate) async fn session_for(&self, token: &str) -> Result<SessionStaff, UiError> {
        self.session_state(token).await.map(|(staff, _)| staff)
    }

    /// The session's staff and whether it is locked, both read under one lock.
    async fn session_state(&self, token: &str) -> Result<(SessionStaff, bool), UiError> {
        let token = token.to_owned();
        let now = self.clock.now_ms();
        let cached = {
            let mut sessions = self.sessions.lock().map_err(|_| internal())?;
            let session = sessions.get(&token).cloned().ok_or_else(auth_failed)?;
            if now - session.created_at >= SESSION_TTL_MS {
                sessions.remove(&token);
                drop(sessions);
                self.session_ended();
                return Err(auth_failed());
            }
            session
        };
        let staff = Arc::clone(&self.staff);
        let id = cached.staff.staff_id.clone();
        let current = tokio::task::spawn_blocking(move || staff.current(&id))
            .await
            .map_err(|_| internal())?
            .map_err(ui_error)?;
        self.store_refresh(&token, &cached.staff.staff_id, current)
    }

    /// Writes back the staff row read for `read_id`. A session re-bound meanwhile (an unlock
    /// with another member's PIN) is left as it is and answered as it is now.
    fn store_refresh(
        &self,
        token: &str,
        read_id: &str,
        current: Option<SessionStaff>,
    ) -> Result<(SessionStaff, bool), UiError> {
        let mut sessions = self.sessions.lock().map_err(|_| internal())?;
        let Some(s) = sessions.get_mut(token) else {
            return Err(auth_failed());
        };
        if s.staff.staff_id != read_id {
            return Ok((s.staff.clone(), s.locked));
        }
        match current {
            Some(fresh) => {
                s.staff = fresh.clone();
                Ok((fresh, s.locked))
            }
            None => {
                sessions.remove(token);
                drop(sessions);
                self.session_ended();
                Err(auth_failed())
            }
        }
    }
}

pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

pub(crate) fn auth_failed() -> UiError {
    UiError {
        code: ErrorCode::AuthFailed,
        message: AUTH_FAILED_MESSAGE.into(),
        field: None,
        retry_after_s: None,
    }
}

fn session_locked() -> UiError {
    UiError {
        code: ErrorCode::SessionLocked,
        message: "this screen is locked; unlock it with a PIN".into(),
        field: None,
        retry_after_s: None,
    }
}

pub(crate) fn internal() -> UiError {
    UiError {
        code: ErrorCode::Internal,
        message: "the hub could not complete this request".into(),
        field: None,
        retry_after_s: None,
    }
}

pub(crate) fn bad_frame(message: &str) -> UiError {
    UiError {
        code: ErrorCode::BadFrame,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

/// The wire form of an [`AppError`]. An internal error's message may carry database details,
/// so only its code is sent.
pub(crate) fn ui_error(e: AppError) -> UiError {
    if e.code == ErrorCode::Internal {
        tracing::warn!(detail = %e.message, "ui request failed");
        return internal();
    }
    UiError {
        code: e.code,
        message: e.message,
        field: e.field,
        retry_after_s: None,
    }
}

/// The HTTP status for an error code.
pub fn status_of(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::AuthFailed => StatusCode::UNAUTHORIZED,
        ErrorCode::AuthLocked | ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::OtpInvalid | ErrorCode::OtpExpired | ErrorCode::TotpInvalid => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::PasswordTooShort | ErrorCode::PasswordCommon => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::AlreadySet
        | ErrorCode::BranchCodeTaken
        | ErrorCode::BillingDisabled
        | ErrorCode::OneFreeBranch
        | ErrorCode::StepNotReady
        | ErrorCode::HubNotPaired => StatusCode::CONFLICT,
        ErrorCode::PairCodeInvalid | ErrorCode::PairCodeExpired | ErrorCode::SetupCodeInvalid => {
            StatusCode::UNAUTHORIZED
        }
        ErrorCode::FeatureNotEntitled
        | ErrorCode::TierLocked
        | ErrorCode::DeviceLimit
        | ErrorCode::UserLimit => StatusCode::FORBIDDEN,
        ErrorCode::CloudUnreachable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::PasswordRequired | ErrorCode::SetupIncomplete => StatusCode::FORBIDDEN,
        ErrorCode::PermissionDenied
        | ErrorCode::Forbidden
        | ErrorCode::PinRequired
        | ErrorCode::PinRejected => StatusCode::FORBIDDEN,
        ErrorCode::ReasonRequired => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::SessionLocked => StatusCode::LOCKED,
        ErrorCode::BadFrame | ErrorCode::Unsupported | ErrorCode::BadRequest => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::NotFound | ErrorCode::InvoiceNotFound => StatusCode::NOT_FOUND,
        ErrorCode::ValidationFailed
        | ErrorCode::Expired
        | ErrorCode::HsnRequired
        | ErrorCode::ReferenceRequired
        | ErrorCode::ConsentRequired
        | ErrorCode::CouponInvalid => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::Stale
        | ErrorCode::OrderSettled
        | ErrorCode::RefundExceedsPaid
        | ErrorCode::AlreadyBootstrapped
        | ErrorCode::BranchCodeLocked
        | ErrorCode::IdempotencyConflict
        | ErrorCode::GroupNotEmpty
        | ErrorCode::GroupKindMismatch
        | ErrorCode::ZoneNotEmpty
        | ErrorCode::CellTaken
        | ErrorCode::BarcodeTaken
        | ErrorCode::LoginTaken
        | ErrorCode::UsernameTaken
        | ErrorCode::PhoneTaken
        | ErrorCode::PcBusy
        | ErrorCode::PcUnavailable
        | ErrorCode::NoRate
        | ErrorCode::PaymentShort
        | ErrorCode::UndoNotAllowed
        | ErrorCode::InsufficientBalance
        | ErrorCode::CreditLimit
        | ErrorCode::CreditBelowOutstanding
        | ErrorCode::OverrideUsedToday
        | ErrorCode::CreditPaused
        | ErrorCode::PassAlreadyActive
        | ErrorCode::PackExpired
        | ErrorCode::PlanNotForGroup
        | ErrorCode::PassOutsideWindow
        | ErrorCode::PastClosing
        | ErrorCode::OutOfStock
        | ErrorCode::ShiftAlreadyOpen
        | ErrorCode::ShiftNotOpen
        | ErrorCode::DrawerShort
        | ErrorCode::VarianceOverThreshold
        | ErrorCode::BundleShort
        | ErrorCode::ControllersUnchecked
        | ErrorCode::DeviceUnassigned => StatusCode::CONFLICT,
        ErrorCode::NotPrimary | ErrorCode::HubSlow | ErrorCode::NoHub => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ErrorCode::HubIncompatible => StatusCode::CONFLICT,
        ErrorCode::PairPinPaused => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => StatusCode::UNAUTHORIZED,
        ErrorCode::SchemaTooNew | ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn fail(e: UiError) -> Response {
    (status_of(e.code), Json(e)).into_response()
}

impl UiAuth {
    fn open_session(&self, me: &SessionStaff) -> Result<String, UiError> {
        let token = new_session_token().map_err(ui_error)?;
        let now = self.clock.now_ms();
        match self.sessions.lock() {
            Ok(mut s) => {
                s.retain(|_, v| now - v.created_at < SESSION_TTL_MS);
                // At most MAX_SESSIONS_PER_STAFF per member: end the oldest.
                loop {
                    let mine: Vec<(&String, i64)> = s
                        .iter()
                        .filter(|(_, v)| v.staff.staff_id == me.staff_id)
                        .map(|(k, v)| (k, v.created_at))
                        .collect();
                    if mine.len() < MAX_SESSIONS_PER_STAFF {
                        break;
                    }
                    let oldest = mine
                        .into_iter()
                        .min_by_key(|(_, at)| *at)
                        .map(|(k, _)| k.clone());
                    if let Some(k) = oldest {
                        s.remove(&k);
                        self.session_ended();
                    }
                }
                s.insert(
                    token.clone(),
                    Session {
                        staff: me.clone(),
                        created_at: now,
                        locked: false,
                    },
                );
            }
            Err(_) => return Err(internal()),
        }
        Ok(token)
    }
}

async fn sign_in(State(auth): State<Arc<UiAuth>>, body: axum::body::Bytes) -> Response {
    let Ok(req) = serde_json::from_slice::<SignInRequest>(&body) else {
        return fail(bad_frame("the body must be {\"login\", \"password\"}"));
    };
    let reserved = match auth.lockout.lock() {
        Ok(mut l) => l.begin(&req.login, auth.clock.now_ms()),
        Err(_) => return fail(internal()),
    };
    if let Err(retry_after_s) = reserved {
        return fail(UiError {
            code: ErrorCode::AuthLocked,
            message: "too many failed sign-ins; try again later".into(),
            field: None,
            retry_after_s: Some(retry_after_s),
        });
    }
    let staff = Arc::clone(&auth.staff);
    let (login, password) = (req.login.clone(), req.password);
    let verified = tokio::task::spawn_blocking(move || staff.sign_in(&login, &password)).await;
    let settle = |success: bool| {
        if let Ok(mut l) = auth.lockout.lock() {
            l.finish(&req.login, auth.clock.now_ms(), success);
        }
    };
    let me = match verified {
        Ok(Ok(me)) => {
            settle(true);
            me
        }
        Ok(Err(e)) if e.code == ErrorCode::AuthFailed => {
            settle(false);
            return fail(auth_failed());
        }
        Ok(Err(e)) => {
            // Not a wrong password: release the reservation without counting it.
            if let Ok(mut l) = auth.lockout.lock() {
                l.finish(&req.login, auth.clock.now_ms(), true);
            }
            return fail(ui_error(e));
        }
        Err(e) => {
            settle(true);
            tracing::warn!(detail = %e, "sign-in task failed");
            return fail(internal());
        }
    };
    let token = match auth.open_session(&me) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    tracing::info!(staff_id = %me.staff_id, "staff signed in");
    Json(SignInOk { token, staff: me }).into_response()
}

fn setup_code_invalid() -> UiError {
    ui_error(arena_core::staff::setup_code_invalid())
}

async fn setup_code(State(auth): State<Arc<UiAuth>>, body: axum::body::Bytes) -> Response {
    let Some(writer) = auth.writer.clone() else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    };
    let Ok(req) = serde_json::from_slice::<SetupCodeRequest>(&body) else {
        return fail(bad_frame(
            "the body must be {\"login\", \"code\", \"password\", \"pin\"}",
        ));
    };
    if req.login.len() > MAX_SETUP_FIELD_BYTES || req.code.len() > MAX_SETUP_FIELD_BYTES {
        return fail(setup_code_invalid());
    }
    let reserved = match auth.setup_lockout.lock() {
        Ok(mut l) => l.begin(&req.login, auth.clock.now_ms()),
        Err(_) => return fail(internal()),
    };
    if let Err(retry_after_s) = reserved {
        return fail(UiError {
            code: ErrorCode::AuthLocked,
            message: "too many failed setup codes; try again later".into(),
            field: None,
            retry_after_s: Some(retry_after_s),
        });
    }
    tokio::spawn(setup_code_run(auth, writer, req))
        .await
        .unwrap_or_else(|_| fail(internal()))
}

enum Spent {
    Failure,
    Success,
    Neither,
}

async fn setup_code_run(
    auth: Arc<UiAuth>,
    writer: WriterHandle,
    req: SetupCodeRequest,
) -> Response {
    let settle = |spent: Spent| {
        if let Ok(mut l) = auth.setup_lockout.lock() {
            let now = auth.clock.now_ms();
            match spent {
                Spent::Failure => l.finish(&req.login, now, false),
                Spent::Success => l.finish(&req.login, now, true),
                Spent::Neither => l.release(&req.login, now),
            }
        }
    };
    let trusted_now = |auth: &UiAuth| {
        let wall = auth.clock.now_ms();
        auth.status
            .as_ref()
            .map_or(wall, |s| s.borrow().trusted_now(wall))
    };
    let Ok(permit) = Arc::clone(&auth.setup_checks).acquire_owned().await else {
        settle(Spent::Neither);
        return fail(internal());
    };
    let staff = Arc::clone(&auth.staff);
    let (login, code, checked_at) = (req.login.clone(), req.code.clone(), trusted_now(&auth));
    let checked =
        tokio::task::spawn_blocking(move || staff.check_setup_code(&login, &code, checked_at))
            .await;
    drop(permit);
    let (staff_id, setup_code_hash) = match checked {
        Ok(Ok(Some(found))) => found,
        Ok(Ok(None)) => {
            settle(Spent::Failure);
            return fail(setup_code_invalid());
        }
        Ok(Err(e)) => {
            settle(Spent::Neither);
            return fail(ui_error(e));
        }
        Err(_) => {
            settle(Spent::Neither);
            return fail(internal());
        }
    };
    if let Err(e) = arena_core::staff::check_new_credentials(&req.password, &req.pin) {
        settle(Spent::Success);
        return fail(ui_error(e));
    }
    let (password, pin) = (req.password.clone(), req.pin.clone());
    let hashed = tokio::task::spawn_blocking(move || {
        Ok::<_, AppError>((
            arena_core::staff::hash_secret(&password)?,
            arena_core::staff::hash_secret(&pin)?,
        ))
    })
    .await;
    let (password_hash, pin_hash) = match hashed {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            settle(Spent::Neither);
            return fail(ui_error(e));
        }
        Err(_) => {
            settle(Spent::Neither);
            return fail(internal());
        }
    };
    let intent = arena_proto::Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::StaffSetCredentials,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: auth.clock.now_ms(),
        base_rev: None,
        actor: arena_proto::Actor {
            kind: ActorKind::Staff,
            id: staff_id.clone(),
        },
        elevated_by: None,
        reason: None,
        payload: arena_proto::IntentPayload::StaffSetCredentials(
            arena_proto::StaffSetCredentials {
                staff_id: staff_id.clone(),
                setup_code_hash,
                trusted_now_ms: trusted_now(&auth),
                password_hash,
                pin_hash,
            },
        ),
    };
    if let Err(e) = writer.submit_intent(intent).await {
        settle(if e.code == ErrorCode::SetupCodeInvalid {
            Spent::Failure
        } else {
            Spent::Neither
        });
        return fail(ui_error(e));
    }
    settle(Spent::Success);
    let me = match auth.current_staff(staff_id.clone()).await {
        Ok(Some(me)) => me,
        Ok(None) => return fail(auth_failed()),
        Err(e) => return fail(e),
    };
    let token = match auth.open_session(&me) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    tracing::info!(staff_id = %me.staff_id, "staff set their credentials with a setup code");
    Json(SignInOk { token, staff: me }).into_response()
}

async fn sign_out(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(auth_failed());
    };
    // An unknown or already-ended token is fine: the session is gone either way.
    match auth.sessions.lock() {
        Ok(mut s) => {
            let removed = s.remove(token).is_some();
            drop(s);
            auth.drop_tickets(token);
            if let Ok(mut l) = auth.unlock_lockout.lock() {
                l.forget(&format!("token:{token}"));
            }
            if removed {
                auth.session_ended();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => fail(internal()),
    }
}

async fn me(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let (staff, locked) = match auth.session_state(&token).await {
        Ok(state) => state,
        Err(e) => return fail(e),
    };
    let dir = Arc::clone(&auth.staff);
    let id = staff.staff_id.clone();
    match tokio::task::spawn_blocking(move || dir.has_pin(&id)).await {
        Ok(Ok(has_pin)) => Json(UiMe {
            staff,
            locked,
            has_pin,
        })
        .into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/lock`: idle-locks the session and drops its tickets (204). Locking a locked
/// session changes nothing.
async fn lock(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.session_for(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let newly = match auth.sessions.lock() {
        Ok(mut sessions) => match sessions.get_mut(&token) {
            Some(s) => !std::mem::replace(&mut s.locked, true),
            None => return fail(auth_failed()),
        },
        Err(_) => return fail(internal()),
    };
    auth.drop_tickets(&token);
    if newly {
        tracing::info!(staff_id = %me.staff_id, at = auth.clock.now_ms(), "ui.locked");
        auth.push_session(&token, "session.locked", "{}".into());
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The one message for a PIN that unlocks nothing. It never names anyone.
pub const UNLOCK_REJECTED_MESSAGE: &str = "that PIN can't unlock this screen";

fn unlock_rejected() -> UiError {
    UiError {
        code: ErrorCode::PinRejected,
        message: UNLOCK_REJECTED_MESSAGE.into(),
        field: None,
        retry_after_s: None,
    }
}

fn not_locked() -> Response {
    let e = UiError {
        code: ErrorCode::ValidationFailed,
        message: "this session is not locked".into(),
        field: Some("session".into()),
        retry_after_s: None,
    };
    (StatusCode::CONFLICT, Json(e)).into_response()
}

/// `POST /ui/unlock {pin}`: see the module docs.
async fn unlock(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let (me, is_locked) = match auth.session_state(&token).await {
        Ok(state) => state,
        Err(e) => return fail(e),
    };
    let Ok(UnlockRequest { pin }) = serde_json::from_slice::<UnlockRequest>(&body) else {
        return fail(bad_frame("the body must be {\"pin\"}"));
    };
    // Unlocking is no way to switch members on a live session (that is elevation's job).
    if !is_locked {
        return not_locked();
    }
    let current = me.staff_id.clone();
    let keys = unlock_keys(&current, &token);
    let reserved = match auth.unlock_lockout.lock() {
        Ok(mut l) => l.begin(&keys, auth.clock.now_ms()),
        Err(_) => return fail(internal()),
    };
    if let Err(retry_after_s) = reserved {
        tracing::warn!(staff_id = %current, "ui unlock refused: paused");
        return fail(locked(retry_after_s));
    }
    let release = || {
        if let Ok(mut l) = auth.unlock_lockout.lock() {
            l.release(&keys, auth.clock.now_ms());
        }
    };
    let failed = || -> Response {
        let now = auth.clock.now_ms();
        let paused = auth
            .unlock_lockout
            .lock()
            .ok()
            .and_then(|mut l| l.finish(&keys, now, false));
        tracing::warn!(staff_id = %current, "ui unlock refused");
        match paused {
            Some((retry, pauses)) => {
                tracing::warn!(staff_id = %current, pauses, retry_after_s = retry, "ui unlock paused");
                fail(locked(retry))
            }
            None => fail(unlock_rejected()),
        }
    };
    let matched = {
        let Ok(_permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
            release();
            return fail(internal());
        };
        let staff = Arc::clone(&auth.staff);
        let first = current.clone();
        tokio::task::spawn_blocking(move || staff.match_unlock_pin(&first, &pin)).await
    };
    let matched_id = match matched {
        Ok(Ok(Some(id))) => id,
        Ok(Ok(None)) => return failed(),
        Ok(Err(e)) => {
            release();
            return fail(ui_error(e));
        }
        Err(e) => {
            release();
            tracing::warn!(detail = %e, "unlock pin check task failed");
            return fail(internal());
        }
    };
    #[cfg(test)]
    {
        let pause = auth.unlock_pause.lock().ok().and_then(|p| p.clone());
        if let Some((reached, resume)) = pause {
            reached.notify_one();
            resume.notified().await;
        }
    }
    let switched = matched_id != current;
    let staff = if switched {
        match auth.current_staff(matched_id).await {
            Ok(Some(fresh)) => fresh,
            // Deactivated since the PIN matched: it unlocks nothing.
            Ok(None) => return failed(),
            Err(e) => {
                release();
                return fail(e);
            }
        }
    } else {
        me
    };
    match auth.sessions.lock() {
        Ok(mut sessions) => match sessions.get_mut(&token) {
            // Still locked by the same member (a concurrent unlock did not win first).
            Some(s) if s.locked && s.staff.staff_id == current => {
                s.staff = staff.clone();
                s.locked = false;
            }
            Some(_) => {
                drop(sessions);
                release();
                return not_locked();
            }
            None => {
                drop(sessions);
                release();
                return fail(auth_failed());
            }
        },
        Err(_) => {
            release();
            return fail(internal());
        }
    }
    if let Ok(mut l) = auth.unlock_lockout.lock() {
        l.finish(&keys, auth.clock.now_ms(), true);
    }
    let at = auth.clock.now_ms();
    if switched {
        // Approvals granted to the previous member never carry over.
        auth.drop_tickets(&token);
        tracing::info!(staff_id = %staff.staff_id, previous_staff_id = %current, at, "ui.unlocked");
    } else {
        tracing::info!(staff_id = %staff.staff_id, at, "ui.unlocked");
    }
    let ok = UnlockOk { staff, switched };
    auth.push_session(
        &token,
        "session.unlocked",
        serde_json::to_string(&ok).unwrap_or_default(),
    );
    Json(ok).into_response()
}

fn locked(retry_after_s: u32) -> UiError {
    UiError {
        code: ErrorCode::AuthLocked,
        message: "too many wrong PINs; try again later".into(),
        field: None,
        retry_after_s: Some(retry_after_s),
    }
}

fn invalid_items(message: &str) -> UiError {
    UiError {
        code: ErrorCode::ValidationFailed,
        message: message.into(),
        field: Some("items".into()),
        retry_after_s: None,
    }
}

/// What a list of elevation items asks for.
struct Needs {
    /// The permissions an approver must hold (catalogue order, each once), or `None` when one
    /// of the intents is not a permission (it can never be elevated).
    perms: Option<Vec<Permission>>,
}

/// What `items` need from an approver. Checks the items' shape first: 1–[`MAX_TICKET_USES`]
/// intent items (`settings.set` naming its key, others none), or exactly one read item.
fn item_permissions(items: &[ElevationItem]) -> Result<Needs, UiError> {
    if items.is_empty() || items.len() > MAX_TICKET_USES {
        return Err(invalid_items(&format!(
            "an elevation approves 1–{MAX_TICKET_USES} intents"
        )));
    }
    if items.iter().any(|i| i.permission.is_some()) {
        let [item] = items else {
            return Err(invalid_items(
                "a read approval names one permission, on its own",
            ));
        };
        return match (item.permission, item.intent_kind, &item.key) {
            (Some(p), None, None) if p.is_read_elevation() => Ok(Needs {
                perms: Some(vec![p]),
            }),
            (Some(_), None, None) => Err(invalid_items(
                "only audit.view, report.view and player.view can be approved for reading",
            )),
            _ => Err(invalid_items(
                "a read approval names its permission and nothing else",
            )),
        };
    }
    let mut perms = Vec::new();
    let mut all_perms = true;
    for item in items {
        let Some(kind) = item.intent_kind else {
            return Err(invalid_items(
                "each item names an intent_kind or a permission",
            ));
        };
        if !kind.is_handled() {
            return Err(ui_error(AppError::new(
                ErrorCode::Unsupported,
                format!("{} has no handler yet", kind.as_str()),
            )));
        }
        let keyed = kind == IntentKind::SettingsSet;
        match &item.key {
            Some(k) if keyed && !k.trim().is_empty() => {}
            None if !keyed => {}
            _ => {
                return Err(invalid_items(
                    "settings.set items name their setting key; other kinds take none",
                ));
            }
        }
        if item.discount == Some(true)
            && !matches!(
                kind,
                IntentKind::SessionSettle | IntentKind::PosSale | IntentKind::PlanSell
            )
        {
            return Err(invalid_items(
                "only a settle, a counter sale or a plan sale takes a discount",
            ));
        }
        match kind.requirement() {
            Requirement::Perm(p) => {
                // A counter sale is only ever approved to override short stock, which also
                // needs `stock.override`, a discount needs `discount.manual`, and a close is only
                // ever approved for a difference over the threshold, which needs
                // `shift.override_variance` (`apply()` checks each on the approver).
                let wanted = [
                    Some(p),
                    matches!(kind, IntentKind::PosSale | IntentKind::OrderPlace)
                        .then_some(Permission::StockOverride),
                    (kind == IntentKind::ShiftClose).then_some(Permission::ShiftOverrideVariance),
                    (item.discount == Some(true)).then_some(Permission::DiscountManual),
                ];
                for p in wanted.into_iter().flatten() {
                    if !perms.contains(&p) {
                        perms.push(p);
                    }
                }
            }
            _ => all_perms = false,
        }
    }
    perms.sort_by_key(|p| Permission::ALL.iter().position(|q| q == p));
    Ok(Needs {
        perms: all_perms.then_some(perms),
    })
}

/// How long a ticket for `items` lives: a read elevation 10 min, intents 60 s.
fn ticket_ttl_ms(items: &[ElevationItem]) -> i64 {
    if items.iter().any(|i| i.permission.is_some()) {
        READ_TICKET_TTL_MS
    } else {
        TICKET_TTL_MS
    }
}

/// What an item is about, for the logs: its intent kind or its read permission.
fn item_label(item: &ElevationItem) -> &'static str {
    match (item.intent_kind, item.permission) {
        (Some(k), _) => k.as_str(),
        (None, Some(p)) => p.as_str(),
        (None, None) => "?",
    }
}

async fn elevate(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(ElevateRequest { items, pin }) = serde_json::from_slice::<ElevateRequest>(&body) else {
        return fail(bad_frame(
            "the body must be {\"items\": [{\"intent_kind\", \"key\"?} | {\"permission\"}], \"pin\"}",
        ));
    };
    let read = items.iter().any(|i| i.permission.is_some());
    // A follower could never apply what it approved: refuse before any PIN is checked. A read
    // elevation writes nothing, so a follower may grant it.
    if auth.writer.is_none() && !read {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    let Needs { perms } = match item_permissions(&items) {
        Ok(n) => n,
        Err(e) => return fail(e),
    };
    if let Some(perms) = &perms
        && perms.iter().all(|p| me.permissions.contains(p))
    {
        return fail(invalid_items("you can do this without approval"));
    }
    let kinds: Vec<&str> = items.iter().map(item_label).collect();
    let requester = me.staff_id.clone();
    let reserved = match auth.elevate_lockout.lock() {
        Ok(mut l) => l.begin(&requester, auth.clock.now_ms()),
        Err(_) => return fail(internal()),
    };
    if let Err(retry_after_s) = reserved {
        tracing::warn!(staff_id = %requester, kinds = ?kinds, "pin elevation refused: paused");
        return fail(locked(retry_after_s));
    }
    /// How the reserved attempt ends.
    enum Settle {
        Matched,
        Failed,
        /// The check could not run: neither a failure nor a success.
        Release,
    }
    let settle = |how: Settle| -> Option<u32> {
        let now = auth.clock.now_ms();
        let mut l = auth.elevate_lockout.lock().ok()?;
        match how {
            Settle::Matched => l.finish(&requester, now, true),
            Settle::Release => l.release(&requester, now),
            Settle::Failed => {
                l.finish(&requester, now, false);
                // The failure that starts the pause already says so, with its countdown.
                return l.paused_for(&requester, now);
            }
        }
        None
    };
    let matched = match perms {
        Some(perms) => {
            let Ok(_permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
                settle(Settle::Release);
                return fail(internal());
            };
            let staff = Arc::clone(&auth.staff);
            let exclude = me.staff_id.clone();
            tokio::task::spawn_blocking(move || staff.match_pin(&perms, &exclude, &pin)).await
        }
        None => Ok(Ok(None)),
    };
    let (approver, verifier_version) = match matched {
        Ok(Ok(Some(m))) => {
            settle(Settle::Matched);
            m
        }
        Ok(Ok(None)) => {
            let paused = settle(Settle::Failed);
            tracing::warn!(staff_id = %requester, kinds = ?kinds, "pin elevation refused");
            return match paused {
                Some(retry) => fail(locked(retry)),
                None => fail(ui_error(pin_rejected())),
            };
        }
        Ok(Err(e)) => {
            settle(Settle::Release);
            tracing::warn!(staff_id = %requester, kinds = ?kinds, "pin elevation failed");
            return fail(ui_error(e));
        }
        Err(e) => {
            settle(Settle::Release);
            tracing::warn!(staff_id = %requester, kinds = ?kinds, detail = %e, "pin check task failed");
            return fail(internal());
        }
    };
    let ticket = match new_elevation_ticket() {
        Ok(t) => t,
        Err(e) => return fail(ui_error(e)),
    };
    if let Err(e) = auth.issue_ticket(
        &ticket,
        &token,
        &me.staff_id,
        &approver,
        verifier_version,
        &items,
    ) {
        return fail(e);
    }
    // A read grant is audited before it is handed out; if that cannot be recorded, the
    // ticket is withdrawn.
    if let Some(permission) = items.iter().find_map(|i| i.permission)
        && let Err(e) = auth
            .record_read_grant(permission, &me.staff_id, &approver, verifier_version)
            .await
    {
        if let Ok(mut tickets) = auth.tickets.lock() {
            tickets.remove(&ticket);
        }
        tracing::warn!(staff_id = %me.staff_id, "read elevation not recorded; withdrawn");
        return fail(e);
    }
    tracing::info!(
        staff_id = %me.staff_id,
        approver = %approver,
        kinds = ?kinds,
        "pin elevation granted"
    );
    let expires_in_s = u32::try_from(ticket_ttl_ms(&items) / 1000).unwrap_or(u32::MAX);
    Json(ElevateOk {
        ticket,
        items,
        expires_in_s,
    })
    .into_response()
}

/// `POST /ui/elevate/drop {ticket}`: gives up the session's ticket and its unused approvals.
/// 204 whether or not it still existed.
async fn elevate_drop(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.session_for(&token).await {
        return fail(e);
    }
    let Ok(ElevateDrop { ticket }) = serde_json::from_slice::<ElevateDrop>(&body) else {
        return fail(bad_frame("the body must be {\"ticket\"}"));
    };
    match auth.tickets.lock() {
        Ok(mut tickets) => {
            if tickets.get(&ticket).is_some_and(|t| t.session == token) {
                tickets.remove(&ticket);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => fail(internal()),
    }
}

/// The settings key an intent writes, for matching a `settings.set` approval.
fn item_key(intent: &arena_proto::Intent) -> Option<&str> {
    match &intent.payload {
        arena_proto::IntentPayload::SettingsSet(s) => Some(s.key.as_str()),
        _ => None,
    }
}

impl UiAuth {
    /// Redeems the first unused approved item of `ticket` that matches `intent` (its kind and,
    /// for `settings.set`, its key) in the session `token`: the `elevated_by` it stands for and
    /// the item's index (so the use can be given back, [`refund`](Self::refund)). Unknown,
    /// expired, another session's, or no matching unused item: `PIN_REQUIRED`.
    fn redeem(
        &self,
        ticket: &str,
        token: &str,
        intent: &arena_proto::Intent,
    ) -> Result<(ElevatedBy, usize), UiError> {
        let now = self.clock.now_ms();
        let kind = intent.payload.kind();
        let key = item_key(intent);
        let mut tickets = self.tickets.lock().map_err(|_| internal())?;
        let Some(t) = tickets.get_mut(ticket) else {
            return Err(ui_error(pin_required()));
        };
        if t.expires_at <= now {
            tickets.remove(ticket);
            return Err(ui_error(pin_required()));
        }
        if t.session != token || t.requester != intent.actor.id {
            return Err(ui_error(pin_required()));
        }
        let Some(index) = t.items.iter().position(|(item, used)| {
            !used && item.intent_kind == Some(kind) && item.key.as_deref() == key
        }) else {
            return Err(ui_error(pin_required()));
        };
        t.items[index].1 = true;
        Ok((
            ElevatedBy {
                kind: ElevationKind::Pin,
                id: t.staff_id.clone(),
                mode: ElevationMode::Hub,
                verifier_version: t.verifier_version,
            },
            index,
        ))
    }

    /// Gives back the use an intent took when nothing was applied. A ticket that has expired
    /// or been dropped or evicted since stays gone.
    fn refund(&self, ticket: &str, index: usize) {
        let now = self.clock.now_ms();
        let Ok(mut tickets) = self.tickets.lock() else {
            return;
        };
        if let Some(t) = tickets.get_mut(ticket).filter(|t| t.expires_at > now)
            && let Some(item) = t.items.get_mut(index)
        {
            item.1 = false;
        }
    }

    /// Stores a granted ticket for the session `token`, unless the session has been locked or
    /// re-bound since the request was checked (`SESSION_LOCKED`): checked under the tickets
    /// lock, so a lock's ticket drop cannot run between the check and the insert.
    fn issue_ticket(
        &self,
        ticket: &str,
        token: &str,
        requester: &str,
        approver: &str,
        verifier_version: i64,
        items: &[ElevationItem],
    ) -> Result<(), UiError> {
        let now = self.clock.now_ms();
        let mut tickets = self.tickets.lock().map_err(|_| internal())?;
        let still_acting = {
            let sessions = self.sessions.lock().map_err(|_| internal())?;
            sessions
                .get(token)
                .is_some_and(|s| !s.locked && s.staff.staff_id == requester)
        };
        if !still_acting {
            return Err(session_locked());
        }
        tickets.retain(|_, t| t.expires_at > now);
        // At most MAX_TICKETS_PER_SESSION per session: the oldest goes.
        loop {
            let mine: Vec<(&String, i64)> = tickets
                .iter()
                .filter(|(_, t)| t.session == token)
                .map(|(k, t)| (k, t.expires_at))
                .collect();
            if mine.len() < MAX_TICKETS_PER_SESSION {
                break;
            }
            let oldest = mine
                .into_iter()
                .min_by_key(|(_, at)| *at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                tickets.remove(&k);
            }
        }
        tickets.insert(
            ticket.to_owned(),
            Ticket {
                session: token.to_owned(),
                requester: requester.to_owned(),
                staff_id: approver.to_owned(),
                verifier_version,
                expires_at: now + ticket_ttl_ms(items),
                items: items.iter().cloned().map(|i| (i, false)).collect(),
            },
        );
        Ok(())
    }

    /// The staff member `id` as a session shows them now (`None` when gone or inactive).
    async fn current_staff(&self, id: String) -> Result<Option<SessionStaff>, UiError> {
        let dir = Arc::clone(&self.staff);
        tokio::task::spawn_blocking(move || dir.current(&id))
            .await
            .map_err(|_| internal())?
            .map_err(ui_error)
    }

    /// Whether `ticket` is an unexpired read elevation of `permission` issued to the session
    /// `token` while it acted as `staff_id`, and its approver still holds it: active, their
    /// role still allowing `permission`, their PIN not rotated since (re-read on every use).
    /// An approval that no longer holds drops the ticket. Reading spends nothing.
    pub(crate) async fn read_granted(
        &self,
        ticket: &str,
        token: &str,
        staff_id: &str,
        permission: Permission,
    ) -> Result<bool, UiError> {
        let now = self.clock.now_ms();
        let approval = {
            let tickets = self.tickets.lock().map_err(|_| internal())?;
            tickets
                .get(ticket)
                .filter(|t| {
                    t.expires_at > now
                        && t.session == token
                        && t.requester == staff_id
                        && t.items.iter().any(|(item, _)| {
                            item.intent_kind.is_none() && item.permission == Some(permission)
                        })
                })
                .map(|t| (t.staff_id.clone(), t.verifier_version))
        };
        let Some((approver, version)) = approval else {
            return Ok(false);
        };
        let dir = Arc::clone(&self.staff);
        let id = approver.clone();
        let holds =
            tokio::task::spawn_blocking(move || dir.approval_holds(&id, permission, version))
                .await
                .map_err(|_| internal())?
                .map_err(ui_error)?;
        if !holds {
            tracing::info!(staff_id, approver = %approver, "read elevation ended: approval no longer holds");
            if let Ok(mut tickets) = self.tickets.lock() {
                tickets.remove(ticket);
            }
        }
        Ok(holds)
    }

    /// Records a granted read elevation as the system intent `audit.read_granted`, so it
    /// shows in the audit log. A follower cannot write: it logs the grant at info instead.
    async fn record_read_grant(
        &self,
        permission: Permission,
        requester: &str,
        approver: &str,
        verifier_version: i64,
    ) -> Result<(), UiError> {
        let Some(writer) = &self.writer else {
            tracing::info!(
                permission = permission.as_str(),
                staff_id = requester,
                approver,
                verifier_version,
                "read elevation granted on a follower (not in the change log)"
            );
            return Ok(());
        };
        let intent = arena_proto::Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::AuditReadGranted,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: self.clock.now_ms(),
            base_rev: None,
            actor: arena_proto::Actor {
                kind: ActorKind::System,
                id: "hub".into(),
            },
            elevated_by: None,
            reason: None,
            payload: arena_proto::IntentPayload::AuditReadGranted(arena_proto::AuditReadGranted {
                permission,
                requester_staff_id: requester.to_owned(),
                approver_staff_id: approver.to_owned(),
                verifier_version,
            }),
        };
        writer
            .submit_intent(intent)
            .await
            .map(|_| ())
            .map_err(ui_error)
    }

    /// Pauses `player_id`'s credit when it is 15+ days overdue (the lookup half of Story 3.20's
    /// check). A refusal or a follower leaves the credit as it was.
    async fn pause_overdue_credit(&self, player_id: Option<String>) {
        let Some(writer) = &self.writer else {
            return;
        };
        match writer.credit_auto_pause(player_id).await {
            Ok(_) => {}
            Err(e) if e.code == ErrorCode::NotPrimary => {}
            Err(e) => tracing::warn!(code = e.code.as_str(), "credit auto-pause failed"),
        }
    }

    /// Ends every ticket of the session `token`.
    fn drop_tickets(&self, token: &str) {
        if let Ok(mut tickets) = self.tickets.lock() {
            tickets.retain(|_, t| t.session != token);
        }
    }
}

async fn intent(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(UiIntentRequest {
        mut intent,
        elevation_ticket,
    }) = serde_json::from_slice::<UiIntentRequest>(&body)
    else {
        return fail(bad_frame(
            "the body must be {\"intent\": Intent, \"elevation_ticket\"?}",
        ));
    };
    // The session decides who acts, never the client, and only a redeemed ticket elevates.
    intent.actor = arena_proto::Actor {
        kind: ActorKind::Staff,
        id: me.staff_id.clone(),
    };
    intent.elevated_by = None;
    if matches!(
        intent.payload,
        arena_proto::IntentPayload::StaffSetCredentials(_)
    ) {
        return fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "credentials are set with a setup code",
        )));
    }
    if matches!(
        intent.payload,
        arena_proto::IntentPayload::PlayerCreate(_)
            | arena_proto::IntentPayload::PlayerUpdate(_)
            | arena_proto::IntentPayload::PlayerPasswordReset(_)
    ) {
        return fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "profiles are changed from the profile forms",
        )));
    }
    if matches!(
        &intent.payload,
        arena_proto::IntentPayload::SessionAssignPlayer(a) if a.create.is_some()
    ) {
        return fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "a profile is created from the convert form",
        )));
    }
    if matches!(
        intent.payload,
        arena_proto::IntentPayload::StaffDeactivate(_)
    ) {
        return fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "staff are deactivated from Settings → Staff, with your PIN",
        )));
    }
    if matches!(intent.payload, arena_proto::IntentPayload::PrinterStatus(_)) {
        return fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "the printer status is recorded by a test print or by skipping the printer",
        )));
    }
    run_intent(&auth, &token, &me, intent, elevation_ticket).await
}

async fn run_intent(
    auth: &Arc<UiAuth>,
    token: &str,
    me: &SessionStaff,
    mut intent: arena_proto::Intent,
    elevation_ticket: Option<String>,
) -> Response {
    // A follower spends no ticket use.
    let Some(writer) = &auth.writer else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    };
    // A remote unlock needs the kiosk on the LAN now: refused at once otherwise, so no exit is
    // recorded that nobody could grant.
    let target = match &intent.payload {
        arena_proto::IntentPayload::DeviceAdminUnlock(u) => Some(&u.device_id),
        arena_proto::IntentPayload::DeviceTestSessionStart(s) => Some(&s.device_id),
        arena_proto::IntentPayload::DeviceScreen(s) => Some(&s.device_id),
        arena_proto::IntentPayload::DevicePower(p) => Some(&p.device_id),
        _ => None,
    };
    if target.is_some_and(|id| !auth.presence.online(id)) {
        return fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "device_id",
            "the device is offline",
        )));
    }
    let seated = match &intent.payload {
        arena_proto::IntentPayload::SessionStart(s) => Some(s.device_id.clone()),
        arena_proto::IntentPayload::SessionTransfer(t) => Some(t.device_id.clone()),
        _ => None,
    };
    if let Some(id) = seated
        && !auth.presence.online(&id)
    {
        let reader = Arc::clone(&auth.reader);
        match tokio::task::spawn_blocking(move || reader.paired_device(&id)).await {
            Ok(Ok(true)) => {
                return fail(ui_error(AppError::with_field(
                    ErrorCode::PcUnavailable,
                    "device_id",
                    "This PC is offline.",
                )));
            }
            Ok(Ok(false)) => {}
            Ok(Err(e)) => return fail(ui_error(e)),
            Err(_) => return fail(internal()),
        }
    }
    let can_edit = me.permissions.contains(&Permission::SettingsEdit);
    let mut redeemed = None;
    if let Some(ticket) = elevation_ticket {
        if intent.kind != intent.payload.kind() {
            return fail(ui_error(pin_required()));
        }
        match auth.redeem(&ticket, token, &intent) {
            Ok((elevated, index)) => {
                intent.elevated_by = Some(elevated);
                redeemed = Some((ticket, index));
            }
            Err(e) => return fail(e),
        }
    }
    if let Err(e) = auth
        .probe_backup_dir(&intent, can_edit || redeemed.is_some())
        .await
    {
        if let Some((ticket, index)) = redeemed {
            auth.refund(&ticket, index);
        }
        return fail(e);
    }
    let command = match &intent.payload {
        arena_proto::IntentPayload::DeviceScreen(s) => Some((
            s.device_id.clone(),
            s.action,
            s.text.as_deref().map(|t| t.trim().to_owned()),
        )),
        arena_proto::IntentPayload::DevicePower(p) => Some((p.device_id.clone(), p.action, None)),
        _ => None,
    };
    let started = match &intent.payload {
        arena_proto::IntentPayload::SessionStart(s) => Some(s.device_id.clone()),
        _ => None,
    };
    let transferring = match &intent.payload {
        arena_proto::IntentPayload::SessionTransfer(t) => {
            Some((t.session_id.clone(), t.device_id.clone()))
        }
        _ => None,
    };
    let ended = matches!(
        intent.payload,
        arena_proto::IntentPayload::SessionEnd(_) | arena_proto::IntentPayload::SessionForceEnd(_)
    );
    let forced = matches!(
        intent.payload,
        arena_proto::IntentPayload::SessionForceEnd(_)
    );
    let ending = match &intent.payload {
        arena_proto::IntentPayload::SessionEnd(e) => Some(e.session_id.clone()),
        arena_proto::IntentPayload::SessionForceEnd(e) => Some(e.session_id.clone()),
        _ => None,
    };
    let reopening = matches!(
        intent.payload,
        arena_proto::IntentPayload::SessionEndReverted(_)
            | arena_proto::IntentPayload::SessionForceEndReverted(_)
    );
    let topping_up = matches!(
        intent.payload,
        arena_proto::IntentPayload::WalletTopup(_)
            | arena_proto::IntentPayload::CreditCollect(_)
            | arena_proto::IntentPayload::PlanSell(_)
            | arena_proto::IntentPayload::PosSale(_)
    );
    let settling = match &intent.payload {
        arena_proto::IntentPayload::SessionSettle(s) => Some(s.order_id.clone()),
        _ => None,
    };
    let pausing = match &intent.payload {
        arena_proto::IntentPayload::SessionPause(p) => Some((p.session_id.clone(), true)),
        arena_proto::IntentPayload::SessionResume(r) => Some((r.session_id.clone(), false)),
        arena_proto::IntentPayload::SessionExtend(e) => Some((e.session_id.clone(), false)),
        _ => None,
    };
    let placing = match &intent.payload {
        arena_proto::IntentPayload::OrderPlace(p) => Some(p.session_id.clone()),
        _ => None,
    };
    // An Undo of a placement changes what the kiosk lists as ordered, too.
    let unplacing = match &intent.payload {
        arena_proto::IntentPayload::OrderPlaceReverted(r) => Some(r.order_id.clone()),
        _ => None,
    };
    // A cancelled line may take its order with it: read its session first, so the kiosk's
    // "Ordered: n items" can follow.
    // A voided order leaves its session, so the same applies.
    let cancelling = match &intent.payload {
        arena_proto::IntentPayload::OrderLineCancel(c) => {
            let (reader, id) = (Arc::clone(&auth.reader), c.line_id.clone());
            tokio::task::spawn_blocking(move || reader.line_session(&id))
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
        }
        arena_proto::IntentPayload::OrderVoid(v) => {
            let (reader, id) = (Arc::clone(&auth.reader), v.order_id.clone());
            tokio::task::spawn_blocking(move || reader.order_session(&id))
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
        }
        _ => None,
    };
    let cmd_id = intent.idempotency_key.clone();
    match writer.submit_intent(intent).await {
        Ok(applied) => {
            if let Some(session_id) = cancelling {
                let reader = Arc::clone(&auth.reader);
                let id = session_id.clone();
                let paused = tokio::task::spawn_blocking(move || reader.session_paused(&id))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or(true);
                if !paused {
                    push_pause_state(auth, &session_id, false, &cmd_id).await;
                }
            }
            if let Some(order_id) = unplacing {
                let reader = Arc::clone(&auth.reader);
                let session = tokio::task::spawn_blocking(move || reader.order_session(&order_id))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten();
                if let Some(session_id) = session {
                    let reader = Arc::clone(&auth.reader);
                    let id = session_id.clone();
                    let paused = tokio::task::spawn_blocking(move || reader.session_paused(&id))
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or(true);
                    if !paused {
                        push_pause_state(auth, &session_id, false, &cmd_id).await;
                    }
                }
            }
            if let Some((session_id, paused)) = pausing {
                push_pause_state(auth, &session_id, paused, &cmd_id).await;
            }
            if let Some((session_id, to_device)) = transferring {
                push_transfer(auth, &session_id, &to_device, &applied, &cmd_id).await;
            }
            if ended && let Some(c) = applied.changes.iter().find(|c| c.tbl == "devices_meta") {
                let sent = auth.presence.push_pc_command(
                    &c.row_id,
                    &cmd_id,
                    arena_proto::PcAction::Lock,
                    None,
                    Some(if forced {
                        arena_proto::LockReason::EndedByStaff
                    } else {
                        arena_proto::LockReason::PayAtCounter
                    }),
                );
                tracing::info!(device_id = %c.row_id, cmd_id = %cmd_id, forced, sessions = sent, "end lock pushed");
            }
            let started = started.or_else(|| {
                reopening
                    .then(|| applied.changes.iter().find(|c| c.tbl == "devices_meta"))
                    .flatten()
                    .map(|c| c.row_id.clone())
            });
            if let Some(device_id) = started {
                let (reader, now) = (Arc::clone(&auth.reader), auth.clock.now_ms());
                let screens =
                    tokio::task::spawn_blocking(move || reader.session_screens(now)).await;
                match screens {
                    Ok(Ok(screens)) => {
                        if let Some((_, screen)) =
                            screens.into_iter().find(|(d, _)| *d == device_id)
                        {
                            let sent = auth
                                .presence
                                .push_session_started(&device_id, &cmd_id, screen);
                            tracing::info!(device_id = %device_id, cmd_id = %cmd_id, sessions = sent, "session start pushed");
                        } else {
                            tracing::warn!(device_id = %device_id, cmd_id = %cmd_id, "no live session to push");
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(device_id = %device_id, error = %e, "could not read the session screen");
                    }
                    Err(e) => {
                        tracing::warn!(device_id = %device_id, error = %e, "the session screen read failed");
                    }
                }
            }
            if let Some((device_id, action, text)) = command {
                let sent = auth.presence.push_pc_command(
                    &device_id,
                    &cmd_id,
                    action,
                    text,
                    (action == arena_proto::PcAction::Lock)
                        .then_some(arena_proto::LockReason::LockedByStaff),
                );
                tracing::info!(device_id = %device_id, cmd_id = %cmd_id, action = action.as_str(), sessions = sent, "pc command pushed");
            }
            let settling = settling.or_else(|| {
                topping_up
                    .then(|| {
                        applied
                            .changes
                            .iter()
                            .find(|c| c.tbl == "orders" && c.op == arena_core::ChangeOp::Insert)
                    })
                    .flatten()
                    .map(|c| c.row_id.clone())
            });
            let placed = match &placing {
                Some(session_id) => {
                    // The kiosk shows "Ordered: n items"; a paused session stays covered.
                    let (reader, id) = (Arc::clone(&auth.reader), session_id.clone());
                    let paused = tokio::task::spawn_blocking(move || reader.session_paused(&id))
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or(true);
                    if !paused {
                        push_pause_state(auth, session_id, false, &cmd_id).await;
                    }
                    place_outcome(auth, &applied, &applied.intent_id).await
                }
                None => None,
            };
            let settled = match settling {
                Some(order_id) => settle_outcome(auth, &order_id, &cmd_id).await,
                None => None,
            };
            let ended = match ending {
                Some(session_id) => end_outcome(auth, &session_id).await,
                None => None,
            };
            Json(UiIntentResult {
                applied: crate::lan::registry::summary(&applied),
                settled,
                ended,
                placed,
            })
            .into_response()
        }
        Err(e) => {
            // apply() is one transaction: a refusal applied nothing, so the approval is given
            // back (the UI fixes the cause and retries). HUB_SLOW is the exception: the intent
            // may still run.
            if e.code != ErrorCode::HubSlow
                && let Some((ticket, index)) = redeemed
            {
                auth.refund(&ticket, index);
            }
            fail(ui_error(e))
        }
    }
}

/// After a transfer commits: the old PC locks (`<cmd_id>:old`, so its ack cannot satisfy the wait on
/// the new PC) and the new PC unlocks to the session screen, or locks as Paused when the session is
/// paused. The old PC is the other `devices_meta` row the intent wrote, and is left alone when a
/// session has since started on it.
async fn push_transfer(
    auth: &Arc<UiAuth>,
    session_id: &str,
    to_device: &str,
    applied: &arena_core::Applied,
    cmd_id: &str,
) {
    let from_device = applied
        .changes
        .iter()
        .find(|c| c.tbl == "devices_meta" && c.row_id != to_device)
        .map(|c| c.row_id.clone());
    if let Some(from_device) = from_device {
        let (reader, now) = (Arc::clone(&auth.reader), auth.clock.now_ms());
        let busy = match tokio::task::spawn_blocking(move || reader.session_screens(now)).await {
            Ok(Ok(screens)) => screens.iter().any(|(d, _)| *d == from_device),
            _ => false,
        };
        if busy {
            tracing::warn!(device_id = %from_device, cmd_id, "the old PC already has a session, not locked");
        } else {
            let sent = auth.presence.push_pc_command(
                &from_device,
                &format!("{cmd_id}:old"),
                arena_proto::PcAction::Lock,
                None,
                Some(arena_proto::LockReason::LockedByStaff),
            );
            tracing::info!(device_id = %from_device, cmd_id, sessions = sent, "old PC lock pushed");
        }
    }
    let (reader, id) = (Arc::clone(&auth.reader), session_id.to_owned());
    let paused = match tokio::task::spawn_blocking(move || reader.session_paused(&id)).await {
        Ok(Ok(paused)) => paused,
        Ok(Err(e)) => {
            tracing::warn!(session_id, error = %e, "could not read the session state");
            return;
        }
        Err(e) => {
            tracing::warn!(session_id, error = %e, "the session state read failed");
            return;
        }
    };
    push_pause_state(auth, session_id, paused, cmd_id).await;
}

/// The kiosk follows a pause (covered, "Session paused"), or a resume or an extend (unlocked to the
/// session screen like a start, with the new end time). The device is the one whose live session screen names the session.
async fn push_pause_state(auth: &Arc<UiAuth>, session_id: &str, paused: bool, cmd_id: &str) {
    let (reader, now) = (Arc::clone(&auth.reader), auth.clock.now_ms());
    let screens = match tokio::task::spawn_blocking(move || reader.session_screens(now)).await {
        Ok(Ok(screens)) => screens,
        Ok(Err(e)) => {
            tracing::warn!(session_id, error = %e, "could not read the session screen");
            return;
        }
        Err(e) => {
            tracing::warn!(session_id, error = %e, "the session screen read failed");
            return;
        }
    };
    let Some((device_id, screen)) = screens
        .into_iter()
        .find(|(_, s)| s.session_id == session_id)
    else {
        tracing::warn!(
            session_id,
            cmd_id,
            "no live session to push a pause state for"
        );
        return;
    };
    let sent = if paused {
        auth.presence.push_pc_command(
            &device_id,
            cmd_id,
            arena_proto::PcAction::Lock,
            None,
            Some(arena_proto::LockReason::Paused),
        )
    } else {
        auth.presence
            .push_session_started(&device_id, cmd_id, screen)
    };
    tracing::info!(device_id = %device_id, cmd_id, paused, sessions = sent, "pause state pushed");
}

/// What a settle committed, read back from the order row so a retried key answers the same,
/// and the PC back to its idle locked screen unless a new session already runs on it.
async fn settle_outcome(
    auth: &Arc<UiAuth>,
    order_id: &str,
    cmd_id: &str,
) -> Option<arena_proto::SettleOutcome> {
    let (reader, id) = (Arc::clone(&auth.reader), order_id.to_owned());
    let found = tokio::task::spawn_blocking(move || {
        Ok::<_, arena_proto::AppError>((reader.settlement(&id)?, reader.credit_collection(&id)?))
    })
    .await;
    let (settlement, collection) = match found {
        Ok(Ok((Some(s), c))) => (s, c),
        Ok(Ok((None, _))) => return None,
        Ok(Err(e)) => {
            tracing::warn!(order_id, error = %e, "could not read the settlement");
            return None;
        }
        Err(e) => {
            tracing::warn!(order_id, error = %e, "the settlement read failed");
            return None;
        }
    };
    if let Some(device_id) = settlement
        .device_id
        .as_deref()
        .filter(|_| !settlement.device_busy)
    {
        let sent = auth.presence.push_pc_command(
            device_id,
            cmd_id,
            arena_proto::PcAction::Lock,
            None,
            None,
        );
        tracing::info!(
            device_id,
            cmd_id,
            sessions = sent,
            "idle lock pushed after settle"
        );
    }
    Some(arena_proto::SettleOutcome {
        order_id: settlement.order_id,
        settled_at: settlement.settled_at,
        undo_until: settlement.settled_at + arena_proto::UNDO_WINDOW_MS,
        order_rev: settlement.rev,
        total_minor: settlement.total_minor,
        irreversible: settlement.irreversible,
        collection,
    })
}

/// What a placement committed, read back from the order and its lines so a retried key answers
/// the same.
async fn place_outcome(
    auth: &Arc<UiAuth>,
    applied: &arena_core::Applied,
    placement_id: &str,
) -> Option<arena_proto::PlaceOutcome> {
    let order_id = applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders")?
        .row_id
        .clone();
    let (reader, order, placement) = (
        Arc::clone(&auth.reader),
        order_id.clone(),
        placement_id.to_owned(),
    );
    match tokio::task::spawn_blocking(move || reader.placement(&order, &placement)).await {
        Ok(Ok(Some((rev, total_minor, placed_at)))) => Some(arena_proto::PlaceOutcome {
            order_id,
            order_rev: rev,
            placement_id: placement_id.to_owned(),
            placed_at,
            undo_until: placed_at + arena_proto::UNDO_WINDOW_MS,
            total_minor,
        }),
        Ok(Ok(None)) => None,
        Ok(Err(e)) => {
            tracing::warn!(order_id, error = %e, "could not read the placement");
            None
        }
        Err(e) => {
            tracing::warn!(order_id, error = %e, "the placement read failed");
            None
        }
    }
}

/// What an End committed, read back from the rows so a retried key answers the same.
async fn end_outcome(auth: &Arc<UiAuth>, session_id: &str) -> Option<arena_proto::EndOutcome> {
    let (reader, id) = (Arc::clone(&auth.reader), session_id.to_owned());
    match tokio::task::spawn_blocking(move || reader.ending(&id)).await {
        Ok(Ok(Some(e))) => Some(arena_proto::EndOutcome {
            session_id: e.session_id,
            order_id: e.order_id,
            ended_at: e.ended_at,
            undo_until: e.ended_at + arena_proto::UNDO_WINDOW_MS,
            session_rev: e.session_rev,
            total_minor: e.total_minor,
            irreversible: e.irreversible,
        }),
        Ok(Ok(None)) => None,
        Ok(Err(e)) => {
            tracing::warn!(session_id, error = %e, "could not read the end");
            None
        }
        Err(e) => {
            tracing::warn!(session_id, error = %e, "the end read failed");
            None
        }
    }
}

async fn player_write<T: serde::de::DeserializeOwned>(
    auth: &Arc<UiAuth>,
    headers: &HeaderMap,
    body: &[u8],
    shape: &str,
) -> Result<(String, SessionStaff, T), Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    let Ok(request) = serde_json::from_slice::<T>(body) else {
        return Err(fail(bad_frame(shape)));
    };
    if auth.writer.is_none() {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        ))));
    }
    Ok((token, me, request))
}

pub(crate) fn player_kek(auth: &UiAuth) -> Result<Arc<dyn arena_core::players::Kek>, UiError> {
    auth.kek.clone().ok_or_else(|| {
        ui_error(AppError::new(
            ErrorCode::Unsupported,
            "this hub has no key store for profiles",
        ))
    })
}

fn require_manage(me: &SessionStaff, ticket: &Option<String>, what: &str) -> Result<(), UiError> {
    if ticket.is_none() && !me.permissions.contains(&Permission::PlayerManage) {
        return Err(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            format!("{what} needs a manager's PIN"),
        )));
    }
    Ok(())
}

fn player_intent(
    me: &SessionStaff,
    now: i64,
    idempotency_key: String,
    base_rev: Option<i64>,
    payload: arena_proto::IntentPayload,
) -> arena_proto::Intent {
    arena_proto::Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key,
        issued_at: now,
        base_rev,
        actor: arena_proto::Actor {
            kind: ActorKind::Staff,
            id: me.staff_id.clone(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

async fn players(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (token, me, request) = match player_write::<UiPlayerRequest>(
        &auth,
        &headers,
        &body,
        "the body must be {\"form\": PlayerForm, \"idempotency_key\", \"elevation_ticket\"?}",
    )
    .await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let UiPlayerRequest {
        form,
        idempotency_key,
        elevation_ticket,
    } = request;
    let kek = match player_kek(&auth) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    if let Err(e) = require_manage(&me, &elevation_ticket, "creating a profile") {
        return fail(e);
    }
    let now = auth.clock.now_ms();
    if let Err(e) = form.check(&arena_core::players::utc_day(now)) {
        return fail(ui_error(e));
    }
    let Ok(permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
        return fail(internal());
    };
    let player_id = ulid::Ulid::generate().to_string();
    let id = player_id.clone();
    let sealed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = arena_core::staff::hash_secret(&form.password)?;
        arena_core::players::seal(&kek.kek()?, &form, &id, hash)
    })
    .await;
    let payload = match sealed {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    let intent = player_intent(
        &me,
        now,
        idempotency_key,
        None,
        arena_proto::IntentPayload::PlayerCreate(payload),
    );
    run_intent(&auth, &token, &me, intent, elevation_ticket).await
}

async fn session_convert(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (token, me, request) = match player_write::<arena_proto::UiSessionConvertRequest>(
        &auth,
        &headers,
        &body,
        "the body must be {\"session_id\", \"base_rev\", \"form\": PlayerForm, \"idempotency_key\", \"elevation_ticket\"?}",
    )
    .await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let arena_proto::UiSessionConvertRequest {
        session_id,
        base_rev,
        form,
        idempotency_key,
        elevation_ticket,
    } = request;
    let kek = match player_kek(&auth) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    if let Err(e) = require_manage(&me, &elevation_ticket, "creating a profile") {
        return fail(e);
    }
    let now = auth.clock.now_ms();
    if let Err(e) = form.check(&arena_core::players::utc_day(now)) {
        return fail(ui_error(e));
    }
    let Ok(permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
        return fail(internal());
    };
    let player_id = ulid::Ulid::generate().to_string();
    let id = player_id.clone();
    let sealed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = arena_core::staff::hash_secret(&form.password)?;
        arena_core::players::seal(&kek.kek()?, &form, &id, hash)
    })
    .await;
    let create = match sealed {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    let intent = player_intent(
        &me,
        now,
        idempotency_key,
        Some(base_rev),
        arena_proto::IntentPayload::SessionAssignPlayer(arena_proto::SessionAssignPlayer {
            session_id,
            player_id,
            create: Some(create),
        }),
    );
    run_intent(&auth, &token, &me, intent, elevation_ticket).await
}

async fn sessions_today(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let (reader, now) = (Arc::clone(&auth.reader), auth.clock.now_ms());
    match tokio::task::spawn_blocking(move || reader.sessions_today(now)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn player_update(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (token, me, request) = match player_write::<arena_proto::UiPlayerEditRequest>(
        &auth,
        &headers,
        &body,
        "the body must be {\"form\": PlayerEditForm, \"idempotency_key\", \"elevation_ticket\"?}",
    )
    .await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let arena_proto::UiPlayerEditRequest {
        form,
        idempotency_key,
        elevation_ticket,
    } = request;
    let kek = match player_kek(&auth) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    if let Err(e) = require_manage(&me, &elevation_ticket, "editing a profile") {
        return fail(e);
    }
    let now = auth.clock.now_ms();
    if let Err(e) = form.check(&arena_core::players::utc_day(now)) {
        return fail(ui_error(e));
    }
    let reader = Arc::clone(&auth.reader);
    let base_rev = form.base_rev;
    let sealed = tokio::task::spawn_blocking(move || {
        let (_, _, wrapped) = reader
            .player(&form.player_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no such player"))?;
        arena_core::players::reseal(&kek.kek()?, &form, &wrapped)
    })
    .await;
    let payload = match sealed {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    let intent = player_intent(
        &me,
        now,
        idempotency_key,
        Some(base_rev),
        arena_proto::IntentPayload::PlayerUpdate(payload),
    );
    run_intent(&auth, &token, &me, intent, elevation_ticket).await
}

async fn player_password(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (token, me, request) = match player_write::<arena_proto::UiPasswordResetRequest>(
        &auth,
        &headers,
        &body,
        "the body must be {\"form\": PlayerPasswordForm, \"idempotency_key\", \"elevation_ticket\"?}",
    )
    .await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let arena_proto::UiPasswordResetRequest {
        form,
        idempotency_key,
        elevation_ticket,
    } = request;
    if let Err(e) = require_manage(&me, &elevation_ticket, "resetting a password") {
        return fail(e);
    }
    if let Err(e) = form.check() {
        return fail(ui_error(e));
    }
    let Ok(permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
        return fail(internal());
    };
    let hashed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        arena_core::staff::hash_secret(&form.password).map(|h| (form.player_id, h))
    })
    .await;
    let (player_id, password_hash) = match hashed {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    let intent = player_intent(
        &me,
        auth.clock.now_ms(),
        idempotency_key,
        None,
        arena_proto::IntentPayload::PlayerPasswordReset(arena_proto::PlayerPasswordReset {
            player_id,
            password_hash,
        }),
    );
    run_intent(&auth, &token, &me, intent, elevation_ticket).await
}

async fn player_read<T: serde::Serialize + Send + 'static>(
    auth: Arc<UiAuth>,
    headers: HeaderMap,
    read: impl FnOnce(
        &mut arena_core::players::PlayerIndex,
        &arena_core::SnapshotReader,
        &[u8; arena_core::players::KEK_LEN],
    ) -> Result<T, AppError>
    + Send
    + 'static,
) -> Response {
    match player_read_value(auth, headers, read).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

async fn player_read_value<T: Send + 'static>(
    auth: Arc<UiAuth>,
    headers: HeaderMap,
    read: impl FnOnce(
        &mut arena_core::players::PlayerIndex,
        &arena_core::SnapshotReader,
        &[u8; arena_core::players::KEK_LEN],
    ) -> Result<T, AppError>
    + Send
    + 'static,
) -> Result<T, Response> {
    let Some(token) = bearer(&headers) else {
        return Err(fail(auth_failed()));
    };
    let me = match auth.acting_session(token).await {
        Ok(me) => me,
        Err(e) => return Err(fail(e)),
    };
    if !me.permissions.contains(&Permission::PlayerView) {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "looking up players needs the player.view permission",
        ))));
    }
    let Some(kek) = auth.kek.clone() else {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::Unsupported,
            "this hub has no key store for profiles",
        ))));
    };
    let (reader, index) = (Arc::clone(&auth.reader), Arc::clone(&auth.player_index));
    let done = tokio::task::spawn_blocking(move || {
        let kek = kek.kek()?;
        let mut index = index
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "player index lock poisoned"))?;
        read(&mut index, &reader, &kek)
    })
    .await;
    match done {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(fail(ui_error(e))),
        Err(_) => Err(fail(internal())),
    }
}

async fn player_search(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let q = q.get("q").cloned().unwrap_or_default();
    let hits = match player_read_value(Arc::clone(&auth), headers, move |i, r, k| {
        i.search(r, k, &q)
    })
    .await
    {
        Ok(hits) => hits,
        Err(r) => return r,
    };
    for hit in hits.iter().filter(|h| h.outstanding_minor > 0) {
        auth.pause_overdue_credit(Some(hit.player_id.clone())).await;
    }
    Json(hits).into_response()
}

async fn player_profile(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let id = q.get("id").cloned().unwrap_or_default();
    player_read(auth, headers, move |_, r, k| {
        arena_core::players::profile(r, k, &id)
    })
    .await
}

async fn player_wallet(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::PlayerView) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "reading a wallet needs the player.view permission",
        )));
    }
    let (reader, id) = (
        Arc::clone(&auth.reader),
        q.get("id").cloned().unwrap_or_default(),
    );
    auth.pause_overdue_credit(Some(id.clone())).await;
    let now_ms = auth.clock.now_ms();
    match tokio::task::spawn_blocking(move || reader.wallet(&id, now_ms)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn products(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    let now_ms = auth.clock.now_ms();
    match tokio::task::spawn_blocking(move || reader.products(now_ms)).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `GET /ui/pos/sales`: the business day's counter sales, newest first, for Recent sales on POS.
async fn pos_sales(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let (reader, now_ms) = (Arc::clone(&auth.reader), auth.clock.now_ms());
    match tokio::task::spawn_blocking(move || reader.pos_sales(now_ms)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn stock_expiry(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    let now_ms = auth.clock.now_ms();
    match tokio::task::spawn_blocking(move || reader.expiry(now_ms)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn stock_low(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    match tokio::task::spawn_blocking(move || reader.low_stock()).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn plans(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    match tokio::task::spawn_blocking(move || reader.plans()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

async fn rules(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    match tokio::task::spawn_blocking(move || reader.rules()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `GET /ui/staff`: the Settings → Staff list and the `users` limit, for a member who holds
/// `user.manage` (an Owner or a Manager).
async fn staff_list(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let me = match auth.session(&headers).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::UserManage) {
        return fail(ui_error(arena_core::rbac::denied()));
    }
    let reader = Arc::clone(&auth.reader);
    match tokio::task::spawn_blocking(move || reader.staff_list()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/staff/deactivate {staff_id, pin, idempotency_key}`: an Owner ends a member's
/// access, confirming with their **own** PIN. The PIN is checked first, under the elevation
/// throttle (five failures in a row pause it for 60 s), then `staff.deactivate` is applied with
/// the session's actor; `apply()` re-checks the Owner role, the self and last-Owner rules. The
/// member's sessions end with their next request.
async fn staff_deactivate(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(arena_proto::UiStaffDeactivateRequest {
        staff_id,
        pin,
        idempotency_key,
    }) = serde_json::from_slice::<arena_proto::UiStaffDeactivateRequest>(&body)
    else {
        return fail(bad_frame(
            "the body must be {\"staff_id\", \"pin\", \"idempotency_key\"}",
        ));
    };
    if auth.writer.is_none() {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    if !me.permissions.contains(&Permission::UserManage) {
        return fail(ui_error(arena_core::rbac::denied()));
    }
    let requester = me.staff_id.clone();
    let reserved = match auth.elevate_lockout.lock() {
        Ok(mut l) => l.begin(&requester, auth.clock.now_ms()),
        Err(_) => return fail(internal()),
    };
    if let Err(retry_after_s) = reserved {
        tracing::warn!(staff_id = %requester, "staff deactivation refused: paused");
        return fail(locked(retry_after_s));
    }
    let settle = |matched: Option<bool>| -> Option<u32> {
        let now = auth.clock.now_ms();
        let mut l = auth.elevate_lockout.lock().ok()?;
        match matched {
            Some(true) => l.finish(&requester, now, true),
            None => l.release(&requester, now),
            Some(false) => {
                l.finish(&requester, now, false);
                return l.paused_for(&requester, now);
            }
        }
        None
    };
    let Ok(permit) = Arc::clone(&auth.pin_checks).acquire_owned().await else {
        settle(None);
        return fail(internal());
    };
    let own = {
        let staff = Arc::clone(&auth.staff);
        let id = requester.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            staff.verify_own_pin(&id, &pin)
        })
        .await
    };
    match own {
        Ok(Ok(true)) => {
            settle(Some(true));
        }
        Ok(Ok(false)) => {
            let paused = settle(Some(false));
            tracing::warn!(staff_id = %requester, "staff deactivation: wrong pin");
            return match paused {
                Some(retry) => fail(locked(retry)),
                None => fail(ui_error(pin_rejected())),
            };
        }
        Ok(Err(e)) => {
            settle(None);
            return fail(ui_error(e));
        }
        Err(_) => {
            settle(None);
            return fail(internal());
        }
    }
    let intent = player_intent(
        &me,
        auth.clock.now_ms(),
        idempotency_key,
        None,
        arena_proto::IntentPayload::StaffDeactivate(arena_proto::StaffDeactivate { staff_id }),
    );
    run_intent(&auth, &token, &me, intent, None).await
}

async fn settings(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = auth.session(&headers).await {
        return fail(e);
    }
    let reader = Arc::clone(&auth.reader);
    match tokio::task::spawn_blocking(move || reader.settings()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// The auth routes, behind the same loopback origin guard as `/ui/snapshot`.
pub fn ui_auth_router(auth: Arc<UiAuth>) -> Router {
    if let Some(pairing) = &auth.pairing {
        let weak = Arc::downgrade(&auth);
        pairing.set_owner_check(move |token| {
            weak.upgrade()
                .is_some_and(|a| a.has_stream(token) && a.session_live(token))
        });
    }
    Router::new()
        .route("/ui/pairing-code", post(crate::ui_pairing::issue))
        .route("/ui/pairing-code/discard", post(crate::ui_pairing::discard))
        .route("/ui/auth/sign-in", post(sign_in))
        .route("/ui/auth/setup-code", post(setup_code))
        .route("/ui/auth/sign-out", post(sign_out))
        .route("/ui/me", get(me))
        .route("/ui/lock", post(lock))
        .route("/ui/unlock", post(unlock))
        .route("/ui/elevate", post(elevate))
        .route("/ui/elevate/drop", post(elevate_drop))
        .route("/ui/intent", post(intent))
        .route("/ui/players", post(players).get(player_search))
        .route("/ui/players/profile", get(player_profile))
        .route("/ui/players/wallet", get(player_wallet))
        .route("/ui/sessions", get(sessions_today))
        .route("/ui/sessions/convert", post(session_convert))
        .route("/ui/players/update", post(player_update))
        .route("/ui/players/password", post(player_password))
        .route("/ui/settings", get(settings))
        .route("/ui/staff", get(staff_list))
        .route("/ui/staff/deactivate", post(staff_deactivate))
        .route("/ui/products", get(products))
        .route("/ui/pos/sales", get(pos_sales))
        .route("/ui/stock/expiry", get(stock_expiry))
        .route("/ui/stock/low", get(stock_low))
        .route("/ui/plans", get(plans))
        .route("/ui/rules", get(rules))
        .route("/ui/coupons", get(crate::ui_coupons::list))
        .route("/ui/refund/invoice", get(crate::ui_refund::invoice))
        .route("/ui/shift/count", get(crate::ui_shift::count))
        .route(
            "/ui/reports/daily-sales",
            get(crate::ui_report::daily_sales),
        )
        .route("/ui/reports/sessions", get(crate::ui_report::sessions))
        .route("/ui/reports/players", get(crate::ui_report::players))
        .route("/ui/reports/staff", get(crate::ui_report::staff))
        .route("/ui/reports/stock", get(crate::ui_report::stock))
        .route(
            "/ui/reports/stock/moves",
            get(crate::ui_report::stock_moves),
        )
        .route(
            "/ui/reports/stock/waste",
            get(crate::ui_report::stock_waste),
        )
        .route(
            "/ui/reports/stock/expiring",
            get(crate::ui_report::stock_expiring),
        )
        .route("/ui/reports/shifts", get(crate::ui_report::shifts))
        .route("/ui/reports/shifts/{id}", get(crate::ui_report::shift))
        .route("/ui/reports/gst", get(crate::ui_report::gst))
        .route("/ui/reports/gst/export", post(crate::ui_report::gst_export))
        .route("/ui/coupons/preview", post(crate::ui_coupons::preview))
        .route(
            "/ui/discount/preview",
            post(crate::ui_coupons::discount_preview),
        )
        .route("/ui/printer/find", post(crate::ui_printer::find))
        .route("/ui/printer/health", get(crate::ui_printer::health))
        .route("/ui/printer/ready", get(crate::ui_printer::ready))
        .route("/ui/printer/test", post(crate::ui_printer::test))
        .route("/ui/printer/test-kot", post(crate::ui_printer::test_kot))
        .route("/ui/printer/skip", post(crate::ui_printer::skip))
        .route("/ui/audit", get(crate::ui_audit::list))
        .route("/ui/audit/item", get(crate::ui_audit::item))
        .route("/ui/audit/facets", get(crate::ui_audit::facets))
        .route("/ui/ads/event", post(crate::ui_ads::event))
        .route("/ui/events", get(crate::ui_events::events))
        .with_state(Arc::clone(&auth))
        .layer(axum::middleware::from_fn_with_state(
            auth.pair_gate.clone(),
            crate::ui_pair::pair_gate,
        ))
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use arena_core::rbac::{ROLE_COUNTER, ROLE_KITCHEN, ROLE_MANAGER};
    use arena_core::staff::hash_secret;
    use arena_core::{HubStatus, LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::{
        Actor, BranchBootstrap, Intent, IntentKind, IntentPayload, SettingsSet, StaffUpsert,
    };
    use arena_store::{LeaseRow, RowValue, Store, Table, write_lease};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    #[test]
    fn step_not_ready_is_a_conflict() {
        assert_eq!(status_of(ErrorCode::StepNotReady), StatusCode::CONFLICT);
    }

    const T0: i64 = 1_800_000_000_000;

    #[test]
    fn lockout_pauses_after_five_failures_and_resets_on_success() {
        let mut l = Lockout::default();
        let fail = |l: &mut Lockout, login: &str, at: i64| {
            l.begin(login, at).unwrap();
            l.finish(login, at, false);
        };
        for _ in 0..4 {
            fail(&mut l, "counter", T0);
        }
        l.begin("Counter", T0).unwrap();
        l.finish("Counter", T0, true);
        for _ in 0..4 {
            fail(&mut l, "COUNTER", T0);
        }
        // The success reset the count: a fifth attempt is still allowed, and fails.
        fail(&mut l, " counter ", T0);
        assert_eq!(l.begin("counter", T0), Err(60));
        assert_eq!(l.begin("Counter", T0 + 59_001), Err(1));
        // Once the pause is over the count starts again.
        fail(&mut l, "counter", T0 + 60_000);
        assert!(l.begin("counter", T0 + 60_000).is_ok());
        assert!(l.begin("other", T0).is_ok());
    }

    #[test]
    fn attempts_in_flight_count_toward_the_limit() {
        let mut l = Lockout::default();
        for _ in 0..LOCKOUT_FAILURES {
            l.begin("counter", T0).unwrap();
        }
        assert!(
            l.begin("counter", T0).is_err(),
            "a sixth concurrent attempt"
        );
        l.finish("counter", T0, true);
        assert!(
            l.begin("counter", T0).is_ok(),
            "a released slot is reusable"
        );
    }

    #[test]
    fn a_spray_of_junk_logins_does_not_reset_a_count() {
        let mut l = Lockout::default();
        for _ in 0..4 {
            l.begin("counter", T0).unwrap();
            l.finish("counter", T0, false);
        }
        for i in 0..(MAX_LOCKOUT_ENTRIES * 2) {
            let junk = format!("junk{i}");
            l.begin(&junk, T0).unwrap();
            l.finish(&junk, T0, false);
        }
        l.begin("counter", T0).unwrap();
        l.finish("counter", T0, false);
        assert_eq!(
            l.begin("counter", T0),
            Err(60),
            "the fifth failure still locks"
        );
        // Expired pauses are dropped when the map is full.
        for i in 0..MAX_LOCKOUT_ENTRIES {
            let junk = format!("late{i}");
            for _ in 0..LOCKOUT_FAILURES {
                l.begin(&junk, T0).unwrap();
                l.finish(&junk, T0, false);
            }
        }
        let before = l.entries.len();
        l.begin("fresh", T0 + LOCKOUT_MS + 1).unwrap();
        assert!(l.entries.len() < before);
    }

    pub(crate) struct Hub {
        pub(crate) _status: tokio::sync::watch::Sender<HubStatus>,
        pub(crate) _dir: tempfile::TempDir,
        pub(crate) path: std::path::PathBuf,
        pub(crate) clock: Arc<ManualClock>,
        pub(crate) app: Router,
        pub(crate) writer: arena_core::WriterHandle,
        pub(crate) manager_id: String,
        pub(crate) counter_id: String,
        pub(crate) counter_role: String,
        pub(crate) manager_role: String,
        pub(crate) auth: Arc<UiAuth>,
        /// The LAN registry behind the auth's presence (tests register sessions in it).
        pub(crate) lan: Arc<crate::lan::registry::Registry>,
        pub(crate) pairing: Arc<crate::pairing::Pairing>,
    }

    pub(crate) const TEST_KEK: [u8; 32] = [4; 32];

    struct TestKek;

    impl arena_core::players::Kek for TestKek {
        fn kek(&self) -> Result<[u8; 32], AppError> {
            Ok(TEST_KEK)
        }
    }

    pub(crate) fn intent_as(actor: Actor, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev,
            actor,
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    /// A bootstrapped hub with a Manager (`manager` / `m-pass`) and a Counter
    /// (`counter` / `c-pass`).
    pub(crate) async fn hub() -> Hub {
        hub_with(crate::ui_events::EventsConfig::default()).await
    }

    /// [`hub`] with the given `/ui/events` timings and buffer.
    pub(crate) async fn hub_with(config: crate::ui_events::EventsConfig) -> Hub {
        hub_built(config, true).await
    }

    /// [`hub`] on a Hub with no branch key store (`with_kek` false).
    pub(crate) async fn hub_without_kek() -> Hub {
        hub_built(crate::ui_events::EventsConfig::default(), false).await
    }

    async fn hub_built(config: crate::ui_events::EventsConfig, with_kek: bool) -> Hub {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: "01JHUB".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), "01JHUB");
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease));
        let (status_tx, rx) = status_channel(status);
        let clock = Arc::new(ManualClock::new(T0));
        let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
        let system = Actor {
            kind: ActorKind::System,
            id: "test".into(),
        };
        let boot = writer
            .submit_intent(intent_as(
                system,
                IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 360,
                    fy_start_month: 4,
                    onboarding: None,
                }),
                None,
            ))
            .await
            .unwrap();
        let roles: Vec<String> = boot
            .changes
            .iter()
            .filter(|c| c.tbl == "roles")
            .map(|c| c.row_id.clone())
            .collect();
        let role = |name: &str| {
            let i = arena_core::rbac::BUILTIN_ROLES
                .iter()
                .position(|r| *r == name)
                .unwrap();
            roles[i].clone()
        };
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::System,
                    id: "hub-licence".into(),
                },
                IntentPayload::LicenceApply(arena_proto::LicenceApply {
                    standing: arena_proto::LicenceStanding::Verified,
                    clock_wrong: false,
                    tier: arena_proto::Tier::Free,
                    flags: String::new(),
                    device_cap: 20,
                    user_cap: 10,
                }),
                None,
            ))
            .await
            .unwrap();
        // Cash settles need an open shift.
        writer
            .submit_intent(intent_as(
                owner.clone(),
                IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor: 0 }),
                None,
            ))
            .await
            .unwrap();
        let mut ids = Vec::new();
        for (login, display, role_name, pw, pin) in [
            ("manager", "Manager", ROLE_MANAGER, "m-pass", "2222"),
            ("counter", "Counter", ROLE_COUNTER, "c-pass", "3333"),
            ("kitchen", "Kitchen", ROLE_KITCHEN, "k-pass", "4444"),
        ] {
            let applied = writer
                .submit_intent(intent_as(
                    owner.clone(),
                    IntentPayload::StaffUpsert(StaffUpsert {
                        staff_id: None,
                        login: login.into(),
                        display_name: display.into(),
                        role_id: role(role_name),
                        password_hash: Some(hash_secret(pw).unwrap()),
                        pin_hash: Some(hash_secret(pin).unwrap()),
                        active: true,
                        setup_code_hash: None,
                        setup_expires_at: None,
                    }),
                    None,
                ))
                .await
                .unwrap();
            ids.push(applied.changes[0].row_id.clone());
        }
        let lan = Arc::new(crate::lan::registry::Registry::default());
        let pairing = Arc::new(
            crate::pairing::Pairing::new(clock.clone())
                .unwrap()
                .with_tick(std::time::Duration::from_millis(10)),
        );
        let auth = UiAuth::new(
            Arc::new(StaffDirectory::open(&path).unwrap()),
            Arc::new(SnapshotReader::open(&path).unwrap()),
            Some(writer.clone()),
            clock.clone(),
        )
        .with_status(status_tx.subscribe())
        .with_events_config(config)
        .with_presence(crate::lan::Presence::of(Arc::clone(&lan)))
        .with_pairing(Arc::clone(&pairing));
        let auth = Arc::new(if with_kek {
            auth.with_player_kek(Arc::new(TestKek))
        } else {
            auth
        });
        Hub {
            lan,
            pairing,
            _status: status_tx,
            _dir: dir,
            path,
            clock,
            app: ui_auth_router(Arc::clone(&auth)),
            auth,
            writer,
            manager_id: ids[0].clone(),
            counter_id: ids[1].clone(),
            counter_role: role(ROLE_COUNTER),
            manager_role: role(ROLE_MANAGER),
        }
    }

    pub(crate) async fn call(
        hub: &Hub,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        call_app(&hub.app, method, uri, token, body).await
    }

    pub(crate) async fn call_app(
        app: &Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1:7409");
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let body = body.map_or(Body::empty(), |b| Body::from(b.to_string()));
        let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn sign_in_as(hub: &Hub, login: &str, password: &str) -> (StatusCode, Value) {
        call(
            hub,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(json!({"login": login, "password": password})),
        )
        .await
    }

    pub(crate) async fn token_for(hub: &Hub, login: &str, password: &str) -> String {
        let (status, body) = sign_in_as(hub, login, password).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().unwrap().to_owned()
    }

    pub(crate) fn footer_set(actor: Actor, rev: i64) -> Value {
        json!({ "intent": intent_as(
            actor,
            IntentPayload::SettingsSet(SettingsSet {
                key: "receipt.footer".into(),
                value: "Thanks for playing".into(),
            }),
            Some(rev),
        ) })
    }

    #[tokio::test]
    async fn sign_in_returns_a_token_and_me_returns_the_staff() {
        let hub = hub().await;
        let (status, body) = sign_in_as(&hub, "Counter", "c-pass").await;
        assert_eq!(status, StatusCode::OK);
        let token = body["token"].as_str().unwrap();
        assert_eq!(token.len(), 43);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        let perms: Vec<&str> = arena_core::rbac::builtin_perms(ROLE_COUNTER)
            .into_iter()
            .map(arena_proto::Permission::as_str)
            .collect();
        let mut perms_sorted = perms.clone();
        perms_sorted.sort_by_key(|p| {
            arena_proto::Permission::ALL
                .iter()
                .position(|q| q.as_str() == *p)
        });
        let staff = json!({
            "staff_id": hub.counter_id,
            "display_name": "Counter",
            "role_name": "Counter",
            "permissions": perms_sorted,
        });
        assert!(!perms.contains(&"settings.edit"));
        assert_eq!(body["staff"], staff);
        let mut me = staff.clone();
        me["locked"] = json!(false);
        me["has_pin"] = json!(true);
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(token), None).await,
            (StatusCode::OK, me)
        );
        let (status, body) = call(&hub, "GET", "/ui/me", None, None).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("AUTH_FAILED"))
        );
        let (status, _) = call(&hub, "GET", "/ui/me", Some("nope"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn an_unknown_login_and_a_wrong_password_get_the_same_answer() {
        let hub = hub().await;
        let wrong = sign_in_as(&hub, "counter", "bad").await;
        let unknown = sign_in_as(&hub, "nobody", "x").await;
        assert_eq!(wrong, unknown);
        assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            wrong.1,
            json!({"code": "AUTH_FAILED", "message": AUTH_FAILED_MESSAGE})
        );
    }

    #[tokio::test]
    async fn five_failures_lock_the_login_for_sixty_seconds() {
        let hub = hub().await;
        for _ in 0..5 {
            assert_eq!(
                sign_in_as(&hub, "COUNTER", "bad").await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        // Even the right password is refused while paused.
        let (status, body) = sign_in_as(&hub, "counter", "c-pass").await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["code"], "AUTH_LOCKED");
        let retry = body["retry_after_s"].as_u64().unwrap();
        assert!((1..=60).contains(&retry), "{retry}");
        // Other logins are not affected.
        token_for(&hub, "manager", "m-pass").await;
        hub.clock.advance(30_000);
        let (_, body) = sign_in_as(&hub, "counter", "c-pass").await;
        assert_eq!(body["retry_after_s"], 30);
        hub.clock.advance(30_000);
        token_for(&hub, "counter", "c-pass").await;
    }

    #[tokio::test]
    async fn a_remote_unlock_of_an_offline_kiosk_fails_at_once() {
        let hub = hub().await;
        let secret = arena_core::devices::new_token_secret().unwrap();
        let device = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "Kiosk".into(),
                    token_hash: arena_core::devices::hash_token_secret(&secret).unwrap(),
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let token = token_for(&hub, "manager", "m-pass").await;
        let body = |id: &str| {
            json!({"intent": serde_json::to_value(intent_as(
                Actor { kind: ActorKind::Staff, id: "x".into() },
                IntentPayload::DeviceAdminUnlock(arena_proto::DeviceAdminUnlock {
                    device_id: id.into(),
                }),
                None,
            )).unwrap()})
        };
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "device_id");
        // Online: applied, one remote exit.
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["applied"]["changes"][0]["tbl"], "device_admin_exits");
    }

    async fn registered_pc(hub: &Hub) -> String {
        let secret = arena_core::devices::new_token_secret().unwrap();
        hub.writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "PC 1".into(),
                    token_hash: arena_core::devices::hash_token_secret(&secret).unwrap(),
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    fn screen_body(device: &str, action: arena_proto::PcAction, text: Option<&str>) -> Value {
        json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DeviceScreen(arena_proto::DeviceScreen {
                device_id: device.into(),
                action,
                text: text.map(Into::into),
            }),
            None,
        )).unwrap()})
    }

    #[tokio::test]
    async fn a_counter_screen_command_is_refused_offline_then_pushed_on_every_accept() {
        use arena_proto::{EventFrame, FrameBody, HubEvent, PcAction};
        let hub = hub().await;
        let device = registered_pc(&hub).await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let lock = screen_body(&device, PcAction::Lock, None);
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(lock.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["message"], "the device is offline");
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let key = lock["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut acks = hub.lan.pc_acks();
        for _ in 0..2 {
            let (status, reply) =
                call(&hub, "POST", "/ui/intent", Some(&token), Some(lock.clone())).await;
            assert_eq!(status, StatusCode::OK, "{reply}");
            let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
                panic!("a pushed command")
            };
            assert_eq!(
                arena_proto::parse_frame(t.as_str()).unwrap().body,
                FrameBody::Event(EventFrame {
                    event: HubEvent::PcCommand {
                        cmd_id: key.clone(),
                        action: PcAction::Lock,
                        text: None,
                        reason: Some(arena_proto::LockReason::LockedByStaff)
                    }
                })
            );
            assert!(hub.lan.ack_pc(&device, &key));
            assert_eq!(acks.recv().await.unwrap().cmd_id, key);
        }
        let store = Store::open_reader(&hub.path).unwrap();
        let events = store
            .changes()
            .unwrap()
            .into_iter()
            .filter(|c| c.intent_kind == "device.screen")
            .count();
        assert_eq!(events, 1, "the replay wrote nothing");
        let long = screen_body(&device, PcAction::Message, Some(&"x".repeat(141)));
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(long)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "text");
        assert!(rx.try_recv().is_err(), "a refused command is never pushed");
    }

    #[tokio::test]
    async fn a_counter_restart_needs_an_approval_and_is_pushed_once_approved() {
        let hub = hub().await;
        let device = registered_pc(&hub).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let token = token_for(&hub, "counter", "c-pass").await;
        let body = json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DevicePower(arena_proto::DevicePower {
                device_id: device.clone(),
                action: arena_proto::PcAction::Restart,
            }),
            None,
        )).unwrap()});
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(body.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        assert!(rx.try_recv().is_err());
        let (status, elevated) = call(
            &hub,
            "POST",
            "/ui/elevate",
            Some(&token),
            Some(json!({"items": [{"intent_kind": "device.power"}], "pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{elevated}");
        let mut with_ticket = body;
        with_ticket["elevation_ticket"] = elevated["ticket"].clone();
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(with_ticket)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert!(rx.try_recv().is_ok());
        let store = Store::open_reader(&hub.path).unwrap();
        let c = store.changes().unwrap().pop().unwrap();
        assert_eq!(
            (
                c.intent_kind.as_str(),
                c.actor_id.as_str(),
                c.elevated_by_id.as_deref()
            ),
            (
                "device.power",
                hub.counter_id.as_str(),
                Some(hub.manager_id.as_str())
            )
        );
    }

    pub(crate) async fn rated_pc(hub: &Hub, token_hash: String) -> String {
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let device = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "PC 1".into(),
                    token_hash,
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let group = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                    id: None,
                    name: "PCs".into(),
                    group_kind: "pc".into(),
                    colour: "teal".into(),
                    sort_order: 1,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        for payload in [
            IntentPayload::RateUpsert(arena_proto::RateUpsert {
                group_id: group.clone(),
                price_per_hour_minor: 12_000,
            }),
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: device.clone(),
                group_id: Some(group),
            }),
        ] {
            hub.writer
                .submit_intent(intent_as(owner(), payload, None))
                .await
                .unwrap();
        }
        device
    }

    fn start_body(device: &str) -> Value {
        json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::SessionStart(arena_proto::SessionStart {
        group_with: None,
                device_id: device.into(),
                mode: arena_proto::SessionMode::Postpaid,
                player_id: None,
                pack_id: None,
                plan_id: None,
            }),
            None,
        )).unwrap()})
    }

    #[tokio::test]
    async fn a_session_start_needs_a_paired_pc_online_but_a_manual_pc_counts_online() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let paired = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&paired)),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "PC_UNAVAILABLE");
        let store = Store::open_reader(&hub.path).unwrap();
        assert!(store.rows(arena_store::Table::Sessions).unwrap().is_empty());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(paired.clone()), tx);
        let body = start_body(&paired);
        let key = body["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["applied"]["changes"][0]["tbl"], "sessions");
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the session start pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::SessionStarted { cmd_id, session },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a session.started event")
        };
        assert_eq!((cmd_id, session.device_name.as_str()), (key, "PC 1"));
        assert!(session.lock_after_ms > 0);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&paired)),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "PC_BUSY");
    }

    #[tokio::test]
    async fn placing_an_order_replies_with_the_undo_outcome_pushes_the_kiosk_count_and_undoes() {
        use arena_proto::{OrderPlace, OrderPlaceReverted, PosSaleLine, ProductUpsert, StockIn};
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();
        let who = || Actor {
            kind: ActorKind::Staff,
            id: "x".into(),
        };
        let post = |token: String, payload: IntentPayload, rev: Option<i64>| {
            let hub = &hub;
            async move {
                call(
                    hub,
                    "POST",
                    "/ui/intent",
                    Some(&token),
                    Some(json!({ "intent": intent_as(who(), payload, rev) })),
                )
                .await
            }
        };
        let (status, _) = post(
            manager.clone(),
            IntentPayload::ProductUpsert(ProductUpsert {
                id: None,
                name: "Chips".into(),
                category: String::new(),
                price_minor: 1_000,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        let chips = listed[0]["id"].as_str().unwrap().to_owned();
        let (status, _) = post(
            manager,
            IntentPayload::StockIn(StockIn {
                product_id: chips.clone(),
                qty: 5,
                unit_cost_minor: 400,
                supplier: String::new(),
                expiry: None,
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let place = IntentPayload::OrderPlace(OrderPlace {
            session_id: session_id.clone(),
            lines: vec![PosSaleLine {
                product_id: chips.clone(),
                qty: 2,
            }],
            stock_override: None,
        });
        let (status, reply) = post(token.clone(), place, None).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let placed = &reply["placed"];
        assert_eq!(placed["total_minor"], 2_000);
        assert_eq!(
            placed["undo_until"].as_i64().unwrap() - hub.clock.now_ms(),
            arena_proto::UNDO_WINDOW_MS
        );
        assert!(reply["settled"].is_null() && reply["ended"].is_null());
        // The kiosk screen is refreshed with the count.
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the screen pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::SessionStarted { session, .. },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a session.started event")
        };
        assert_eq!(session.ordered_items, 2);

        // The Undo takes the order's rev and the placement id.
        let (status, reply) = post(
            token.clone(),
            IntentPayload::OrderPlaceReverted(OrderPlaceReverted {
                order_id: placed["order_id"].as_str().unwrap().into(),
                placement_id: placed["placement_id"].as_str().unwrap().into(),
            }),
            placed["order_rev"].as_i64(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed[0]["quantity"], 5);
    }

    /// The first product line on the session's live bill.
    fn first_item(hub: &Hub, session_id: &str) -> arena_proto::BillItem {
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        snapshot
            .bills
            .into_iter()
            .find(|b| b.session_id == session_id)
            .unwrap()
            .items
            .remove(0)
    }

    #[tokio::test]
    async fn a_kitchen_line_is_held_then_served_or_cancelled_over_the_ui_intent_route() {
        use arena_proto::{
            OrderLineCancel, OrderLineServed, OrderPlace, PosSaleLine, ProductUpsert, ReasonCode,
            StockIn,
        };
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();
        let who = || Actor {
            kind: ActorKind::Staff,
            id: "x".into(),
        };
        let post = |token: String, payload: IntentPayload, reason: Option<ReasonCode>| {
            let hub = &hub;
            async move {
                let mut i = intent_as(who(), payload, None);
                i.reason = reason;
                call(
                    hub,
                    "POST",
                    "/ui/intent",
                    Some(&token),
                    Some(json!({ "intent": i })),
                )
                .await
            }
        };
        let (status, _) = post(
            manager.clone(),
            IntentPayload::ProductUpsert(ProductUpsert {
                id: None,
                name: "Burger".into(),
                category: String::new(),
                price_minor: 9_000,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: true,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        let burger = listed[0]["id"].as_str().unwrap().to_owned();
        let (status, _) = post(
            manager.clone(),
            IntentPayload::StockIn(StockIn {
                product_id: burger.clone(),
                qty: 5,
                unit_cost_minor: 2_000,
                supplier: String::new(),
                expiry: None,
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let place = |qty: i64| {
            IntentPayload::OrderPlace(OrderPlace {
                session_id: session_id.clone(),
                lines: vec![PosSaleLine {
                    product_id: burger.clone(),
                    qty,
                }],
                stock_override: None,
            })
        };
        let (status, reply) = post(token.clone(), place(2), None).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        rx.recv().await.unwrap();
        // The stock is held, not taken: on hand stays 5 and the open bill lists an unserved line.
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed[0]["quantity"], 5);
        let item = first_item(&hub, &session_id);
        assert!(!item.served);
        let line_id = item.line_id;

        // Cancelling needs a reason and a `made` answer, and tells the kiosk the new count.
        let cancel = IntentPayload::OrderLineCancel(OrderLineCancel {
            line_id: line_id.clone(),
            made: false,
        });
        let (status, reply) = post(token.clone(), cancel.clone(), None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "REASON_REQUIRED");
        let (status, reply) = post(token.clone(), cancel, Some(ReasonCode::ChangedMind)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the screen pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::SessionStarted { session, .. },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a session.started event")
        };
        assert_eq!(session.ordered_items, 0);
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed[0]["quantity"], 5);

        // A new placement is served by its line id, which takes the stock.
        let (status, _) = post(token.clone(), place(2), None).await;
        assert_eq!(status, StatusCode::OK);
        let line_id = first_item(&hub, &session_id).line_id;
        let (status, reply) = post(
            token.clone(),
            IntentPayload::OrderLineServed(OrderLineServed {
                line_id: line_id.clone(),
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed[0]["quantity"], 3);
        // A served line cannot be cancelled.
        let (status, reply) = post(
            token.clone(),
            IntentPayload::OrderLineCancel(OrderLineCancel {
                line_id,
                made: true,
            }),
            Some(ReasonCode::Mistake),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "VALIDATION_FAILED");

        // Story 5.23: the live bill names the order a void takes. The counter holds no `pos.void`
        // and a void needs a reason and, for Other, a note.
        let (status, _) = post(token.clone(), place(1), None).await;
        assert_eq!(status, StatusCode::OK);
        while rx.try_recv().is_ok() {}
        let order_id = items_order(&hub, &session_id);
        let void = |note: Option<&str>| {
            IntentPayload::OrderVoid(arena_proto::OrderVoid {
                order_id: order_id.clone(),
                note: note.map(str::to_owned),
            })
        };
        let (status, reply) = post(token.clone(), void(None), Some(ReasonCode::Mistake)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        assert_eq!(reply["code"], "PERMISSION_DENIED");
        let (status, reply) = post(manager.clone(), void(None), None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "REASON_REQUIRED");
        let (status, reply) = post(manager.clone(), void(None), Some(ReasonCode::Other)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(
            (&reply["code"], &reply["field"]),
            (&json!("VALIDATION_FAILED"), &json!("note"))
        );

        // A manager voids it: the served units come back, the bill empties and the kiosk hears.
        let (status, reply) = post(
            manager.clone(),
            void(Some("Wrong table")),
            Some(ReasonCode::Other),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed[0]["quantity"], 5);
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the screen pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::SessionStarted { session, .. },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a session.started event")
        };
        assert_eq!(session.ordered_items, 0);
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        let bill = snapshot
            .bills
            .into_iter()
            .find(|b| b.session_id == session_id)
            .unwrap();
        assert!(bill.items.is_empty() && bill.items_order_id.is_none());
        // Voided already: STALE.
        let (status, reply) = post(manager, void(Some("again")), Some(ReasonCode::Other)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "STALE");
    }

    /// The order the session's live bill says its placed products sit on.
    fn items_order(hub: &Hub, session_id: &str) -> String {
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        snapshot
            .bills
            .into_iter()
            .find(|b| b.session_id == session_id)
            .unwrap()
            .items_order_id
            .expect("the bill names its order")
    }

    #[tokio::test]
    async fn ending_a_session_locks_the_kiosk_with_pay_at_counter_and_the_snapshot_bills_it() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let session_id = reply["applied"]["changes"][0]["row_id"].as_str().unwrap();
        rx.recv().await.unwrap();
        let mut body = json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.into(),
            }),
            Some(1),
        )).unwrap()});
        let key = body["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(body.take())).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the lock pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event:
                arena_proto::HubEvent::PcCommand {
                    cmd_id,
                    action,
                    reason,
                    ..
                },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a pc command")
        };
        assert_eq!(
            (cmd_id, action, reason),
            (
                key,
                arena_proto::PcAction::Lock,
                Some(arena_proto::LockReason::PayAtCounter)
            )
        );
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert!(snapshot.sessions.is_empty());
        assert_eq!(snapshot.bills.len(), 1);
        assert_eq!(snapshot.bills[0].subtotal_minor, 6_000);
        assert!(snapshot.bills[0].order_id.is_some());
    }

    #[tokio::test]
    async fn a_forced_end_locks_the_kiosk_with_ended_by_staff_and_offers_an_undo() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let session_id = reply["applied"]["changes"][0]["row_id"].as_str().unwrap();
        rx.recv().await.unwrap();
        let mut forced = intent_as(
            Actor {
                kind: ActorKind::Staff,
                id: "x".into(),
            },
            IntentPayload::SessionForceEnd(arena_proto::SessionForceEnd {
                session_id: session_id.into(),
                note: None,
            }),
            Some(1),
        );
        forced.reason = Some(arena_proto::ReasonCode::HardwareIssue);
        let key = forced.idempotency_key.clone();
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(json!({ "intent": forced })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["ended"]["irreversible"], false);
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the lock pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event:
                arena_proto::HubEvent::PcCommand {
                    cmd_id,
                    action,
                    reason,
                    ..
                },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a pc command")
        };
        assert_eq!(
            (cmd_id, action, reason),
            (
                key,
                arena_proto::PcAction::Lock,
                Some(arena_proto::LockReason::EndedByStaff)
            )
        );
    }

    async fn second_pc(hub: &Hub, first: &str) -> String {
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let store = Store::open_reader(&hub.path).unwrap();
        let group = store
            .rows(arena_store::Table::DevicesMeta)
            .unwrap()
            .iter()
            .find(|r| r["device_id"] == arena_store::RowValue::Text(first.to_owned()))
            .and_then(|r| match &r["group_id"] {
                arena_store::RowValue::Text(g) => Some(g.clone()),
                _ => None,
            })
            .unwrap();
        let device = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "PC 2".into(),
                    token_hash: arena_core::devices::hash_token_secret("t").unwrap(),
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        hub.writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                    device_id: device.clone(),
                    group_id: Some(group),
                }),
                None,
            ))
            .await
            .unwrap();
        device
    }

    fn session_rev(hub: &Hub, id: &str) -> i64 {
        let store = Store::open_reader(&hub.path).unwrap();
        store
            .rows(arena_store::Table::Sessions)
            .unwrap()
            .iter()
            .find(|r| r["id"] == arena_store::RowValue::Text(id.to_owned()))
            .map(|r| match r["rev"] {
                arena_store::RowValue::Integer(n) => n,
                _ => panic!("rev"),
            })
            .unwrap()
    }

    fn transfer_body(session_id: &str, device: &str, rev: i64) -> Value {
        plain_body(
            IntentPayload::SessionTransfer(arena_proto::SessionTransfer {
                session_id: session_id.into(),
                device_id: device.into(),
            }),
            Some(rev),
        )
    }

    async fn pc_event(
        rx: &mut tokio::sync::mpsc::Receiver<tokio_tungstenite::tungstenite::Message>,
    ) -> arena_proto::HubEvent {
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("a push to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame { event }) =
            arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("an event")
        };
        event
    }

    #[tokio::test]
    async fn a_transfer_needs_the_target_online_then_locks_the_old_pc_and_unlocks_the_new_one() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let old = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let new = second_pc(&hub, &old).await;
        let (old_tx, mut old_rx) = tokio::sync::mpsc::channel(8);
        let _old = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(old.clone()), old_tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&old)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        old_rx.recv().await.unwrap();
        let rev = session_rev(&hub, &session_id);
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(transfer_body(&session_id, &new, rev)),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "PC_UNAVAILABLE");
        assert!(old_rx.try_recv().is_err(), "a refusal pushes nothing");
        let (new_tx, mut new_rx) = tokio::sync::mpsc::channel(8);
        let _new = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(new.clone()), new_tx);
        let body = transfer_body(&session_id, &new, rev);
        let key = body["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let arena_proto::HubEvent::PcCommand {
            cmd_id,
            action,
            reason,
            ..
        } = pc_event(&mut old_rx).await
        else {
            panic!("the old PC locks")
        };
        assert_eq!(
            (cmd_id, action, reason),
            (
                format!("{key}:old"),
                arena_proto::PcAction::Lock,
                Some(arena_proto::LockReason::LockedByStaff)
            )
        );
        let arena_proto::HubEvent::SessionStarted { cmd_id, session } = pc_event(&mut new_rx).await
        else {
            panic!("the new PC unlocks to the session screen")
        };
        assert_eq!(
            (cmd_id.as_str(), session.device_name.as_str()),
            (key.as_str(), "PC 2")
        );
        assert_eq!(session.session_id, session_id);
        let (status, _) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(matches!(
            pc_event(&mut old_rx).await,
            arena_proto::HubEvent::PcCommand { .. }
        ));
        assert!(matches!(
            pc_event(&mut new_rx).await,
            arena_proto::HubEvent::SessionStarted { .. }
        ));
    }

    #[tokio::test]
    async fn a_paused_session_moves_with_the_new_pc_locked_as_paused() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let old = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let new = second_pc(&hub, &old).await;
        let (old_tx, mut old_rx) = tokio::sync::mpsc::channel(8);
        let _old = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(old.clone()), old_tx);
        let (new_tx, mut new_rx) = tokio::sync::mpsc::channel(8);
        let _new = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(new.clone()), new_tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&old)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        old_rx.recv().await.unwrap();
        let pause = plain_body(
            IntentPayload::SessionPause(arena_proto::SessionPause {
                session_id: session_id.clone(),
                reason: arena_proto::PauseReason::HardwareIssue,
                note: None,
            }),
            Some(session_rev(&hub, &session_id)),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(pause)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        old_rx.recv().await.unwrap();
        hub.clock.advance(60_000);
        let body = transfer_body(&session_id, &new, session_rev(&hub, &session_id));
        let key = body["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let arena_proto::HubEvent::PcCommand { cmd_id, reason, .. } = pc_event(&mut new_rx).await
        else {
            panic!("the new PC locks as paused")
        };
        assert_eq!(
            (cmd_id, reason),
            (key, Some(arena_proto::LockReason::Paused))
        );
        assert!(matches!(
            pc_event(&mut old_rx).await,
            arena_proto::HubEvent::PcCommand {
                reason: Some(arena_proto::LockReason::LockedByStaff),
                ..
            }
        ));
    }

    fn settle_body(order_id: &str, rev: i64, payments: Value) -> Value {
        let mut intent = serde_json::to_value(intent_as(
            Actor {
                kind: ActorKind::Staff,
                id: "x".into(),
            },
            IntentPayload::SessionSettle(arena_proto::SessionSettle {
                business: None,
                coupon: None,
                discount: None,
                order_id: order_id.into(),
                credit_override: None,
                payments: Vec::new(),
                receipt: None,
            }),
            Some(rev),
        ))
        .unwrap();
        intent["payload"]["payments"] = payments;
        json!({"intent": intent})
    }

    #[tokio::test]
    async fn settling_a_bill_answers_with_the_undo_window_and_returns_the_pc_to_its_idle_lock() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"].as_str().unwrap();
        rx.recv().await.unwrap();
        let end = json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.into(),
            }),
            Some(1),
        )).unwrap()});
        let (_, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(end)).await;
        let order = reply["applied"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["tbl"] == "orders")
            .unwrap()["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();

        let bad = settle_body(
            &order,
            1,
            json!([{"method": "upi", "amount_minor": 6000, "reference": "12"}]),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(bad)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "REFERENCE_REQUIRED");
        let short = settle_body(&order, 1, json!([{"method": "cash", "amount_minor": 10}]));
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(short)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "PAYMENT_SHORT");
        assert!(rx.try_recv().is_err(), "a refusal pushes nothing");

        hub.clock.advance(1_000);
        let body = settle_body(&order, 1, json!([{"method": "cash", "amount_minor": 6000}]));
        let (status, reply) =
            call(&hub, "POST", "/ui/intent", Some(&token), Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let settled_at = reply["settled"]["settled_at"].as_i64().unwrap();
        assert_eq!(settled_at, hub.clock.now_ms());
        assert_eq!(reply["settled"]["undo_until"], settled_at + 8_000);
        assert_eq!(reply["settled"]["order_id"], order.as_str());
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the idle lock pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::PcCommand { action, reason, .. },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a pc command")
        };
        assert_eq!((action, reason), (arena_proto::PcAction::Lock, None));

        hub.clock.advance(5_000);
        let (status, again) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{again}");
        assert_eq!(again["settled"]["settled_at"], settled_at);
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert!(snapshot.bills.is_empty());
    }

    fn convert_body(session: &str, rev: i64, username: &str, phone: &str) -> Value {
        json!({
            "session_id": session,
            "base_rev": rev,
            "idempotency_key": ulid::Ulid::generate().to_string(),
            "form": {
                "username": username,
                "password": "secret1",
                "phone": phone,
                "name": "Asha Rao",
                "email": "",
                "dob": "",
                "notes": "",
                "tags": [],
                "consent": true,
                "policy_version": "1",
            },
        })
    }

    #[tokio::test]
    async fn converting_a_guest_creates_the_profile_and_the_sessions_list_follows() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();
        let (status, list) = call(&hub, "GET", "/ui/sessions", Some(&token), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["sessions"][0]["state"], "running");
        assert_eq!(list["sessions"][0]["guest_no"], 1);
        assert!(list["sessions"][0]["username"].is_null());

        let raw = plain_body(
            IntentPayload::SessionAssignPlayer(arena_proto::SessionAssignPlayer {
                session_id: session_id.clone(),
                player_id: ulid::Ulid::generate().to_string(),
                create: Some(
                    serde_json::from_value(json!({
                        "player_id": "x", "username": "x", "password_hash": "x",
                        "phone_lookup": "x", "name_enc": "x", "phone_enc": "x", "tags": [],
                        "wrapped_key": "x", "consent": true, "policy_version": "1",
                    }))
                    .unwrap(),
                ),
            }),
            Some(1),
        );
        let (status, refused) = call(&hub, "POST", "/ui/intent", Some(&token), Some(raw)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");

        let (status, bad) = call(
            &hub,
            "POST",
            "/ui/sessions/convert",
            Some(&token),
            Some(convert_body(&session_id, 9, "asha_9", "+91 98765 43210")),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{bad}");
        assert_eq!(bad["code"], "STALE");

        let (status, ok) = call(
            &hub,
            "POST",
            "/ui/sessions/convert",
            Some(&token),
            Some(convert_body(&session_id, 1, "asha_9", "+91 98765 43210")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        let (_, list) = call(&hub, "GET", "/ui/sessions", Some(&token), None).await;
        assert_eq!(list["sessions"][0]["username"], "asha_9");
        assert_eq!(list["sessions"][0]["guest_no"], 1);
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert_eq!(snapshot.sessions[0].username.as_deref(), Some("asha_9"));

        let (status, again) = call(
            &hub,
            "POST",
            "/ui/sessions/convert",
            Some(&token),
            Some(convert_body(&session_id, 2, "ben_9", "+91 98765 43211")),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{again}");
        assert_eq!(again["code"], "VALIDATION_FAILED");

        let end = plain_body(
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.clone(),
            }),
            Some(2),
        );
        let (status, ended) = call(&hub, "POST", "/ui/intent", Some(&token), Some(end)).await;
        assert_eq!(status, StatusCode::OK, "{ended}");
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert_eq!(snapshot.bills[0].wallet_minor, Some(0));
    }

    #[tokio::test]
    async fn the_sessions_list_marks_unpaid_and_settled_bills_and_needs_a_session() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (status, _) = call(&hub, "GET", "/ui/sessions", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();
        hub.clock.advance(60 * 60_000);
        let end = plain_body(
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.clone(),
            }),
            Some(1),
        );
        let (_, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(end)).await;
        let order = reply["applied"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["tbl"] == "orders")
            .unwrap()["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (_, list) = call(&hub, "GET", "/ui/sessions", Some(&token), None).await;
        let row = &list["sessions"][0];
        assert_eq!(
            (row["state"].as_str(), row["bill_minor"].as_i64()),
            (Some("unpaid"), Some(12_000))
        );
        assert_eq!(row["order_id"], order.as_str());
        let body = settle_body(
            &order,
            1,
            json!([{"method": "cash", "amount_minor": 12000}]),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, list) = call(&hub, "GET", "/ui/sessions", Some(&token), None).await;
        assert_eq!(list["sessions"][0]["state"], "settled");
        assert!(list["sessions"][0]["order_id"].is_null());
    }

    fn plain_body(payload: IntentPayload, rev: Option<i64>) -> Value {
        json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            payload,
            rev,
        )).unwrap()})
    }

    #[tokio::test]
    async fn undoing_an_end_unlocks_the_kiosk_and_undoing_a_settle_reopens_the_bill() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let (_, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        let session_id = reply["applied"]["changes"][0]["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        rx.recv().await.unwrap();
        hub.clock.advance(41 * 60_000);
        let end = plain_body(
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.clone(),
            }),
            Some(1),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(end)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let ended_at = hub.clock.now_ms();
        assert_eq!(reply["ended"]["session_id"], session_id.as_str());
        assert_eq!(reply["ended"]["ended_at"], ended_at);
        assert_eq!(reply["ended"]["undo_until"], ended_at + 8_000);
        assert_eq!(reply["ended"]["session_rev"], 2);
        assert_eq!(reply["ended"]["total_minor"], 9_000);
        assert_eq!(reply["ended"]["irreversible"], false);
        rx.recv().await.unwrap();

        hub.clock.advance(3_000);
        let undo = plain_body(
            IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
                session_id: session_id.clone(),
            }),
            Some(2),
        );
        let key = undo["intent"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(undo)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert!(reply.get("ended").is_none());
        let Some(tokio_tungstenite::tungstenite::Message::Text(t)) = rx.recv().await else {
            panic!("the session screen pushed to the kiosk")
        };
        let arena_proto::FrameBody::Event(arena_proto::EventFrame {
            event: arena_proto::HubEvent::SessionStarted { cmd_id, session },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("a session.started event")
        };
        assert_eq!((cmd_id, session.device_name.as_str()), (key, "PC 1"));
        assert!(session.elapsed_ms >= 41 * 60_000 + 3_000);

        let again = plain_body(
            IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
                session_id: session_id.clone(),
            }),
            Some(3),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(again)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "UNDO_NOT_ALLOWED");
        assert!(rx.try_recv().is_err(), "a refusal pushes nothing");

        let end = plain_body(
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: session_id.clone(),
            }),
            Some(3),
        );
        let (_, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(end)).await;
        let order = reply["applied"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["tbl"] == "orders")
            .unwrap()["row_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(reply["ended"]["session_rev"], 4);
        rx.recv().await.unwrap();
        let settle = settle_body(&order, 1, json!([{"method": "cash", "amount_minor": 9000}]));
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(settle)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["settled"]["order_rev"], 2);
        assert_eq!(reply["settled"]["total_minor"], 9_000);
        assert_eq!(reply["settled"]["irreversible"], false);
        rx.recv().await.unwrap();

        hub.clock.advance(2_000);
        let undo = plain_body(
            IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
                order_id: order.clone(),
            }),
            Some(2),
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(undo)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert_eq!(snapshot.bills.len(), 1);
        assert_eq!(snapshot.bills[0].order_rev, Some(3));
    }

    #[tokio::test]
    async fn a_manual_console_takes_a_timed_session_from_the_hub() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let console = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: arena_proto::DeviceKind::Console,
                    class: arena_proto::ClientClass::Paired,
                    label: "TV 1".into(),
                    token_hash: String::new(),
                    tv_number: Some(1),
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let group = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                    id: None,
                    name: "PS5".into(),
                    group_kind: "console".into(),
                    colour: "teal".into(),
                    sort_order: 1,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        for payload in [
            IntentPayload::RateUpsert(arena_proto::RateUpsert {
                group_id: group.clone(),
                price_per_hour_minor: 12_000,
            }),
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: console.clone(),
                group_id: Some(group),
            }),
        ] {
            hub.writer
                .submit_intent(intent_as(owner(), payload, None))
                .await
                .unwrap();
        }
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&console)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let snapshot =
            arena_core::read_snapshot(&Store::open_reader(&hub.path).unwrap(), hub.clock.now_ms())
                .unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].device_id, console);
    }

    #[tokio::test]
    async fn restart_of_a_pc_in_session_is_pc_busy_and_nothing_is_pushed() {
        let hub = hub().await;
        let device = rated_pc(&hub, arena_core::devices::hash_token_secret("s").unwrap()).await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some(device.clone()), tx);
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(start_body(&device)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert!(rx.try_recv().is_ok(), "the session start itself is pushed");
        let power = json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DevicePower(arena_proto::DevicePower {
                device_id: device.clone(),
                action: arena_proto::PcAction::Restart,
            }),
            None,
        )).unwrap()});
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(power)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "PC_BUSY");
        assert_eq!(reply["message"], "End or pause the session first");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_test_session_for_an_offline_kiosk_is_refused_at_once() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let body = json!({"intent": serde_json::to_value(intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DeviceTestSessionStart(arena_proto::DeviceTestSessionStart {
                device_id: "01JNOSUCHDEVICE".into(),
            }),
            None,
        )).unwrap()});
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "device_id");
        assert_eq!(reply["message"], "the device is offline");
    }

    #[tokio::test]
    async fn the_session_actor_overwrites_the_clients_actor() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        // The client claims to be the system and to be elevated.
        let mut body = footer_set(
            Actor {
                kind: ActorKind::System,
                id: "spoof".into(),
            },
            1,
        );
        body["intent"]["elevated_by"] =
            json!({"kind": "pin", "id": "x", "mode": "hub", "verifier_version": 1});
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["applied"]["changes"][0]["tbl"], "settings");
        let store = Store::open_reader(&hub.path).unwrap();
        let c = store.changes().unwrap().pop().unwrap();
        assert_eq!(c.intent_kind, IntentKind::SettingsSet.as_str());
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("staff", hub.manager_id.as_str())
        );
        assert_eq!(c.elevated_by_kind, None);
    }

    #[tokio::test]
    async fn a_counter_settings_set_is_permission_denied() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let before = Store::open_reader(&hub.path).unwrap().changes().unwrap();
        let (status, body) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(footer_set(
                Actor {
                    kind: ActorKind::Owner,
                    id: "spoof".into(),
                },
                1,
            )),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "PERMISSION_DENIED");
        let store = Store::open_reader(&hub.path).unwrap();
        assert_eq!(store.changes().unwrap(), before);
        let footer = store
            .rows(Table::Settings)
            .unwrap()
            .into_iter()
            .find(|r| r["key"] == RowValue::Text("receipt.footer".into()))
            .unwrap();
        assert_eq!(footer["rev"], RowValue::Integer(1));
    }

    fn product_body(name: &str, ticket: Option<&str>) -> Value {
        let mut body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: name.into(),
                category: String::new(),
                price_minor: 100,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        ) });
        if let Some(t) = ticket {
            body["elevation_ticket"] = json!(t);
        }
        body
    }

    #[tokio::test]
    async fn a_counter_saves_a_product_with_a_managers_pin_ticket_once() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (status, denied) = post_intent(&hub, &token, product_body("Cola", None)).await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (status, bad) =
            elevate_items(&hub, &token, vec![item("product.upsert", None)], "9999").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{bad}");
        let (status, ok) =
            elevate_items(&hub, &token, vec![item("product.upsert", None)], "2222").await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        let (status, reply) = post_intent(&hub, &token, product_body("Cola", Some(&ticket))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (status, _) = post_intent(&hub, &token, product_body("Fanta", Some(&ticket))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&token), None).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn plans_are_listed_for_a_session_and_written_with_catalog_edit() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/plans", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let group = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                    id: None,
                    name: "PCs".into(),
                    group_kind: "pc".into(),
                    colour: "teal".into(),
                    sort_order: 1,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::PlanUpsert(arena_proto::PlanUpsert {
                id: None,
                plan_kind: "pack".into(),
                name: "5 h".into(),
                price_minor: 30_000,
                minutes: Some(300),
                validity_days: Some(7),
                window_start_min: None,
                window_end_min: None,
                group_ids: vec![group.clone()],
                active: true,
            }),
            None,
        ) });
        let (status, denied) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(body.clone()),
        )
        .await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (_, listed) = call(&hub, "GET", "/ui/plans", Some(&counter), None).await;
        assert_eq!(listed, json!([]));
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&manager), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/plans", Some(&counter), None).await;
        let p = &listed[0];
        assert_eq!(
            (
                p["name"].as_str(),
                p["plan_kind"].as_str(),
                p["minutes"].as_i64(),
                p["validity_days"].as_i64(),
                p["price_minor"].as_i64(),
                p["group_ids"].clone(),
                p["active"].as_bool(),
            ),
            (
                Some("5 h"),
                Some("pack"),
                Some(300),
                Some(7),
                Some(30_000),
                json!([group]),
                Some(true)
            )
        );
        assert!(p.get("window_start_min").is_none());
    }

    /// Story 5.21: Settings → Coupons lists coupons for catalogue editors, and the settle bar
    /// prices a code on a cart for anyone who can sell.
    #[tokio::test]
    async fn coupons_are_listed_to_catalog_editors_and_priced_for_sellers() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/coupons", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, denied) = call(&hub, "GET", "/ui/coupons", Some(&counter), None).await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (status, listed) = call(&hub, "GET", "/ui/coupons", Some(&manager), None).await;
        assert_eq!((status, listed), (StatusCode::OK, json!([])));

        let save = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::CouponUpsert(arena_proto::CouponUpsert {
                id: None,
                code: "save10".into(),
                percent_bp: Some(1_000),
                amount_minor: None,
                valid_from: None,
                valid_to: None,
                usage_limit: Some(3),
                min_bill_minor: 0,
                targets: Vec::new(),
                active: true,
            }),
            None,
        ) });
        let (status, denied) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(save.clone()),
        )
        .await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&manager), Some(save)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/coupons", Some(&manager), None).await;
        let coupon = &listed.as_array().unwrap()[0];
        assert_eq!(
            (
                coupon["code"].as_str(),
                coupon["percent_bp"].as_i64(),
                coupon["usage_limit"].as_i64(),
                coupon["used"].as_i64(),
                coupon["active"].as_bool(),
            ),
            (Some("SAVE10"), Some(1_000), Some(3), Some(0), Some(true))
        );

        let product = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                    id: None,
                    name: "Cola".into(),
                    category: String::new(),
                    price_minor: 4_000,
                    tax_rate_bp: None,
                    hsn: String::new(),
                    track_stock: false,
                    image: String::new(),
                    barcode: String::new(),
                    kitchen: false,
                    favourite: false,
                    active: true,
                    bundle: false,
                    components: Vec::new(),
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let preview =
            |code: &str| json!({"code": code, "lines": [{"product_id": product, "qty": 2}]});
        let (status, priced) = call(
            &hub,
            "POST",
            "/ui/coupons/preview",
            Some(&counter),
            Some(preview("Save10")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{priced}");
        assert_eq!(
            priced,
            json!({"code": "SAVE10", "discount_minor": 800, "total_minor": 7_200})
        );
        let (status, refused) = call(
            &hub,
            "POST",
            "/ui/coupons/preview",
            Some(&counter),
            Some(preview("NOSUCH")),
        )
        .await;
        assert_eq!(
            (status, refused["code"].as_str(), refused["field"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("COUPON_INVALID"),
                Some("coupon")
            )
        );
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/coupons/preview",
            None,
            Some(preview("SAVE10")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/coupons/preview",
            Some(&counter),
            Some(json!({"nope": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Story 5.22: anyone who can sell previews a manual discount; applying it needs
    /// `discount.manual`, which a counter gets from a manager's PIN (asked for with `discount`).
    #[tokio::test]
    async fn a_manual_discount_is_previewed_by_sellers_and_approved_by_a_manager_pin() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let product = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                    id: None,
                    name: "Cola".into(),
                    category: String::new(),
                    price_minor: 4_000,
                    tax_rate_bp: None,
                    hsn: String::new(),
                    track_stock: false,
                    image: String::new(),
                    barcode: String::new(),
                    kitchen: false,
                    favourite: false,
                    active: true,
                    bundle: false,
                    components: Vec::new(),
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let discount = json!({"percent_bp": 1_000, "reason": "regular_customer"});
        let body = |d: serde_json::Value| json!({"discount": d, "lines": [{"product_id": product, "qty": 2}]});
        let (status, priced) = call(
            &hub,
            "POST",
            "/ui/discount/preview",
            Some(&counter),
            Some(body(discount.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{priced}");
        assert_eq!(priced, json!({"discount_minor": 800, "total_minor": 7_200}));
        let (status, refused) = call(
            &hub,
            "POST",
            "/ui/discount/preview",
            Some(&counter),
            Some(body(json!({"percent_bp": 1_000, "reason": "other"}))),
        )
        .await;
        assert_eq!(
            (status, refused["code"].as_str(), refused["field"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("VALIDATION_FAILED"),
                Some("discount.note")
            )
        );
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/discount/preview",
            None,
            Some(body(discount.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/discount/preview",
            Some(&counter),
            Some(json!({"nope": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let sale = IntentPayload::PosSale(arena_proto::PosSale {
            business: None,
            coupon: None,
            discount: serde_json::from_value(discount).ok(),
            lines: vec![arena_proto::PosSaleLine {
                product_id: product.clone(),
                qty: 2,
            }],
            payments: vec![arena_proto::PaymentRow {
                method: arena_proto::PaymentMethod::Cash,
                amount_minor: 7_200,
                reference: None,
            }],
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt: None,
        });
        let who = || Actor {
            kind: ActorKind::Staff,
            id: "x".into(),
        };
        let (status, denied) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(json!({"intent": intent_as(who(), sale.clone(), None)})),
        )
        .await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        // `discount` is only for a settle, a counter sale or a plan sale.
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/elevate",
            Some(&counter),
            Some(
                json!({"items": [{"intent_kind": "stock.waste", "discount": true}], "pin": "2222"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, elevated) = call(
            &hub,
            "POST",
            "/ui/elevate",
            Some(&counter),
            Some(json!({"items": [{"intent_kind": "pos.sale", "discount": true}], "pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{elevated}");
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(json!({
                "intent": intent_as(who(), sale, None),
                "elevation_ticket": elevated["ticket"],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["settled"]["total_minor"], 7_200);
    }

    #[tokio::test]
    async fn rules_are_listed_with_their_conflicts_and_written_with_catalog_edit() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/rules", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let group = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                    id: None,
                    name: "PCs".into(),
                    group_kind: "pc".into(),
                    colour: "teal".into(),
                    sort_order: 1,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let rule = |name: &str, start: i64, end: i64| {
            json!({ "intent": intent_as(
                Actor { kind: ActorKind::Staff, id: "x".into() },
                IntentPayload::RuleUpsert(arena_proto::RuleUpsert {
                    id: None,
                    name: name.into(),
                    group_id: group.clone(),
                    rule_kind: "weekly".into(),
                    days_mask: Some(31),
                    start_min: Some(start),
                    end_min: Some(end),
                    date_from: None,
                    date_to: None,
                    adjust_kind: "percent_bp".into(),
                    adjust_value: -2000,
                    priority: 0,
                    active: true,
                }),
                None,
            ) })
        };
        let (status, denied) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(rule("Happy hour", 960, 1080)),
        )
        .await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (_, listed) = call(&hub, "GET", "/ui/rules", Some(&counter), None).await;
        assert_eq!(listed, json!([]));
        for body in [
            rule("Happy hour", 960, 1080),
            rule("Early bird", 1020, 1200),
        ] {
            let (status, reply) =
                call(&hub, "POST", "/ui/intent", Some(&manager), Some(body)).await;
            assert_eq!(status, StatusCode::OK, "{reply}");
        }
        let (_, listed) = call(&hub, "GET", "/ui/rules", Some(&counter), None).await;
        let happy = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "Happy hour")
            .unwrap();
        assert_eq!(
            (
                happy["rule_kind"].as_str(),
                happy["days_mask"].as_i64(),
                happy["start_min"].as_i64(),
                happy["adjust_kind"].as_str(),
                happy["adjust_value"].as_i64(),
                happy["group_id"].as_str(),
                happy["active"].as_bool(),
            ),
            (
                Some("weekly"),
                Some(31),
                Some(960),
                Some("percent_bp"),
                Some(-2000),
                Some(group.as_str()),
                Some(true)
            )
        );
        assert!(happy.get("date_from").is_none());
        assert_eq!(happy["conflicts"][0]["name"], "Early bird");
        assert!(happy["conflicts"][0]["other_wins"].is_boolean());
    }

    #[tokio::test]
    async fn products_are_listed_for_a_session_and_written_with_catalog_edit() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/products", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: "Cola".into(),
                category: "Drinks".into(),
                price_minor: 2500,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: "890".into(),
                kitchen: true,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        ) });
        let (status, denied) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(body.clone()),
        )
        .await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (status, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        assert_eq!((status, listed), (StatusCode::OK, json!([])));
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&manager), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        let p = &listed[0];
        assert_eq!(
            (
                p["name"].as_str(),
                p["price_minor"].as_i64(),
                p["barcode"].as_str(),
                p["kitchen"].as_bool()
            ),
            (Some("Cola"), Some(2500), Some("890"), Some(true))
        );
        assert_eq!(
            (
                p["available"].as_bool(),
                p["reason"].as_str(),
                p["quantity"].as_i64(),
                p["rev"].as_i64()
            ),
            (Some(false), Some("out_of_stock"), Some(0), Some(1))
        );
        let cola = p["id"].as_str().unwrap().to_owned();
        let combo = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: "Combo".into(),
                category: String::new(),
                price_minor: 4000,
                tax_rate_bp: Some(1200),
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: true,
                components: vec![arena_proto::BundleComponent { product_id: cola.clone(), qty: 2 }],
            }),
            None,
        ) });
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&manager), Some(combo)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        let b = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Combo")
            .unwrap();
        assert_eq!(
            (
                b["bundle"].as_bool(),
                b["track_stock"].as_bool(),
                b["price_minor"].as_i64()
            ),
            (Some(true), Some(false), Some(4000))
        );
        assert_eq!(
            b["components"],
            json!([{"product_id": cola, "name": "Cola", "qty": 2, "active": true}])
        );
        assert_eq!(b["reason"], "out_of_stock");
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some(&manager), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(&hub, "GET", "/ui/products", Some(&manager), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_expiry_lists_need_a_session_and_start_empty() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/stock/expiry", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, view) = call(&hub, "GET", "/ui/stock/expiry", Some(&counter), None).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["warn_days"], 3);
        assert_eq!(view["expiring"], json!([]));
        assert_eq!(view["expired"], json!([]));
        assert_eq!(view["today"].as_str().map(str::len), Some(10));
    }

    /// Story 5.26: the expected cash is only ever revealed for a stated count.
    #[tokio::test]
    async fn the_shift_reveal_needs_a_count_and_a_signed_in_member() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/shift/count?counted_minor=0", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, _) = call(&hub, "GET", "/ui/shift/count", Some(&counter), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "no count, no reveal");
        let (status, reply) = call(
            &hub,
            "GET",
            "/ui/shift/count?counted_minor=-1",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "counted_minor");
        let (status, reveal) = call(
            &hub,
            "GET",
            "/ui/shift/count?counted_minor=2500",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reveal}");
        // The fixture's shift opened with an empty drawer and took no cash.
        assert_eq!(reveal["counted_minor"], 2500);
        assert_eq!(reveal["expected_minor"], 0);
        assert_eq!(reveal["difference_minor"], 2500);
        assert_eq!(reveal["threshold_minor"], 10_000);
        assert_eq!(reveal["over_threshold"], false);
        assert_eq!(reveal["breakdown"]["float_minor"], 0);
    }

    /// Story 5.29: the GST reads need `report.view` and a GST branch; the export writes one ZIP
    /// into a writable folder and refuses a folder it cannot use.
    #[tokio::test]
    async fn the_gst_return_needs_report_view_a_gst_branch_and_a_writable_folder() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/reports/gst?month=2026-10", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().into_owned();
        let body = |month: &str, folder: &str| json!({ "month": month, "folder": folder });

        let (status, reply) = call(
            &hub,
            "GET",
            "/ui/reports/gst?month=2026-10",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/reports/gst/export",
            Some(&counter),
            Some(body("2026-10", &folder)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");

        // The test branch is simple-tax: there is no GST return.
        let (status, reply) = call(
            &hub,
            "GET",
            "/ui/reports/gst?month=2026-10",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(
            (reply["code"].as_str(), reply["field"].as_str()),
            (Some("VALIDATION_FAILED"), Some("tax"))
        );

        let gst = r#"{"country":"IN","gst":true,"gstin":"29ABCDE1234F1Z5","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#;
        hub.writer
            .submit(move |tx| {
                tx.execute(
                    "UPDATE settings SET value = ?1 WHERE key = 'branch.tax'",
                    [gst],
                )
                .map(|_| ())
                .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
            })
            .await
            .unwrap();

        let (status, month) = call(
            &hub,
            "GET",
            "/ui/reports/gst?month=2026-10",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{month}");
        assert_eq!(
            (month["month"].as_str(), month["empty"].as_bool()),
            (Some("2026-10"), Some(true))
        );
        assert_eq!(month["sections"].as_array().map(Vec::len), Some(4));
        let (status, reply) = call(
            &hub,
            "GET",
            "/ui/reports/gst?month=2026-13",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "month");

        let (status, done) = call(
            &hub,
            "POST",
            "/ui/reports/gst/export",
            Some(&manager),
            Some(body("2026-10", &folder)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let name = done["file_name"].as_str().unwrap();
        assert!(
            name.starts_with("GSTR1_") && name.ends_with("_2026-10.zip"),
            "{name}"
        );
        let written = std::fs::read(dir.path().join(name)).unwrap();
        assert!(written.starts_with(b"PK\x03\x04"));
        assert_eq!(done["bytes"], written.len());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file is left behind");

        // A folder that cannot be used is refused with its reason; nothing else is written.
        let file = dir.path().join(name).to_string_lossy().into_owned();
        for (bad, reason) in [
            ("relative/dir", "not_found"),
            ("/no/such/folder/here", "not_found"),
            (file.as_str(), "not_dir"),
            ("content://tree/primary%3AReports", "unsupported"),
        ] {
            let (status, reply) = call(
                &hub,
                "POST",
                "/ui/reports/gst/export",
                Some(&manager),
                Some(body("2026-10", bad)),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {reply}");
            assert_eq!(
                (reply["field"].as_str(), reply["message"].as_str()),
                (Some("folder"), Some(reason)),
                "{bad}"
            );
        }
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/reports/gst/export",
            Some(&manager),
            Some(json!({ "month": "2026-10" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
    }

    /// Story 5.28: the Shifts reads need `report.view`; an unknown shift is `NOT_FOUND`, an open
    /// one `STALE`, and a closed one returns the Z-report stored at close.
    #[tokio::test]
    async fn the_shift_reads_need_report_view_and_serve_the_stored_z_report() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/reports/shifts", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        for uri in ["/ui/reports/shifts", "/ui/reports/shifts/anything"] {
            let (status, reply) = call(&hub, "GET", uri, Some(&counter), None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {reply}");
            assert_eq!(reply["code"], "PERMISSION_DENIED");
        }
        let manager = token_for(&hub, "manager", "m-pass").await;
        let (status, list) = call(&hub, "GET", "/ui/reports/shifts", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["shifts"], json!([]), "the open shift is not listed");

        let (status, reply) =
            call(&hub, "GET", "/ui/reports/shifts/NOPE", Some(&manager), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
        assert_eq!(reply["code"], "NOT_FOUND");

        let shift_id = hub
            .writer
            .submit(|tx| arena_store::open_shift_id(tx))
            .await
            .unwrap()
            .expect("the fixture opened a shift");
        let (status, reply) = call(
            &hub,
            "GET",
            &format!("/ui/reports/shifts/{shift_id}"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{reply}");
        assert_eq!(reply["code"], "STALE");

        hub.writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::ShiftClose(arena_proto::ShiftClose {
                    counted_minor: 0,
                    deposit: None,
                    reason: None,
                    note: None,
                }),
                None,
            ))
            .await
            .unwrap();
        let (status, list) = call(&hub, "GET", "/ui/reports/shifts", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["shifts"].as_array().map(Vec::len), Some(1));
        assert_eq!(list["shifts"][0]["shift_id"], shift_id);
        assert_eq!(list["shifts"][0]["variance_minor"], 0);
        let (status, z) = call(
            &hub,
            "GET",
            &format!("/ui/reports/shifts/{shift_id}"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{z}");
        assert_eq!(z["shift_id"], shift_id);
        assert_eq!(z["rebuilt"], false, "the stored report is served");
        assert_eq!(z["counted_minor"], 0);
        assert_eq!(z["by_type"].as_array().map(Vec::len), Some(5));
    }

    /// Story 5.27: the Reports reads need `report.view` (the counter lacks it) and a valid range.
    #[tokio::test]
    async fn the_report_reads_need_report_view_and_a_valid_range() {
        let hub = hub().await;
        let url =
            |kind: &str, from: &str, to: &str| format!("/ui/reports/{kind}?from={from}&to={to}");
        let (status, _) = call(
            &hub,
            "GET",
            &url("daily-sales", "2027-01-15", "2027-01-15"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, _) = call(
            &hub,
            "GET",
            &url("sessions", "2027-01-15", "2027-01-15"),
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let (status, sales) = call(
            &hub,
            "GET",
            &url("daily-sales", "2027-01-15", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sales}");
        assert_eq!(sales["sales_minor"], 0);
        assert_eq!(sales["by_hour"].as_array().map(Vec::len), Some(24));
        assert_eq!(sales["received"].as_array().map(Vec::len), Some(2));
        let (status, sessions) = call(
            &hub,
            "GET",
            &url("sessions", "2027-01-15", "2027-01-16"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sessions}");
        assert_eq!(sessions["days"], 2);
        let (status, reply) = call(
            &hub,
            "GET",
            &url("daily-sales", "2027-01-16", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["field"], "to");
        let (status, reply) = call(
            &hub,
            "GET",
            &url("sessions", "2026-01-01", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        let (status, reply) = call(
            &hub,
            "GET",
            &url("sessions", "soon", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(
            (status, reply["field"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("from"))
        );
    }

    /// Story 5.31: the Stock reads need `report.view` and a valid range, value stock at cost,
    /// list movements newest first with product and reason filters, and net waste by reason.
    #[tokio::test]
    async fn the_stock_reports_need_report_view_and_read_stock_movements_and_waste() {
        let hub = hub().await;
        let urls = [
            "/ui/reports/stock",
            "/ui/reports/stock/moves?from=2027-01-15&to=2027-01-15",
            "/ui/reports/stock/waste?from=2027-01-15&to=2027-01-15",
            "/ui/reports/stock/expiring",
        ];
        for url in urls {
            let (status, _) = call(&hub, "GET", url, None, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        let counter = token_for(&hub, "counter", "c-pass").await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        for url in urls {
            let (status, reply) = call(&hub, "GET", url, Some(&counter), None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
            assert_eq!(reply["code"], "PERMISSION_DENIED");
        }

        // Nothing stocked yet.
        let (status, empty) = call(&hub, "GET", urls[0], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["items"].as_array().map(Vec::len), Some(0));
        assert_eq!(empty["total_value_minor"], 0);
        let (status, empty) = call(&hub, "GET", urls[2], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["by_reason"].as_array().map(Vec::len), Some(5));
        let (status, empty) = call(&hub, "GET", urls[3], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["expired"].as_array().map(Vec::len), Some(0));

        // A bad range is refused on the field, as on the other reports.
        for kind in ["moves", "waste"] {
            let (status, reply) = call(
                &hub,
                "GET",
                &format!("/ui/reports/stock/{kind}?from=2027-01-16&to=2027-01-15"),
                Some(&manager),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
            assert_eq!(reply["field"], "to");
        }

        // Two tracked products (20 Cola at 15; one Apple at 0 on hand) with a stock-in, a
        // waste, an undone waste and an expired write-off, written with plain SQL.
        let manager_id = hub.manager_id.clone();
        let day = T0 + 4 * 3_600_000;
        let seed = format!(
            "INSERT INTO products (product_id, name, category, price_minor, tax_rate_bp, hsn,
                                   track_stock, image, kitchen, favourite, active, bundle, rev,
                                   created_at, updated_at)
             VALUES ('PA', 'Cola', 'Drinks', 50, 0, '', 1, '', 0, 0, 1, 0, 1, 0, 0),
                    ('PB', 'Apple', 'Food', 30, 0, '', 1, '', 0, 0, 1, 0, 1, 0, 0);
             INSERT INTO stock (product_id, qty, avg_cost_minor, rev, created_at, updated_at)
             VALUES ('PA', 20, 15, 1, 0, 0);
             INSERT INTO stock_moves (move_id, product_id, reason, delta, before_qty,
                    before_avg_cost_minor, unit_cost_minor, supplier, lots_used, staff_id, at, rev,
                    created_at, updated_at, waste_reason, value_minor)
             VALUES ('M1', 'PA', 'stock_in', 25, 0, 0, 15, '', '[]', '{by}', {t1}, 1, 0, 0, NULL, 0),
                    ('M2', 'PA', 'waste', -4, 25, 15, 15, '', '[]', '{by}', {t2}, 1, 0, 0, 'spoiled', 60),
                    ('M3', 'PA', 'waste_reverted', 1, 21, 15, 15, '', '[]', '{by}', {t3}, 1, 0, 0, 'spoiled', 15),
                    ('M4', 'PB', 'expired', -2, 2, 10, 10, '', '[]', '{by}', {t4}, 1, 0, 0, NULL, 20);",
            by = manager_id,
            t1 = day,
            t2 = day + 1_000,
            t3 = day + 2_000,
            t4 = day + 3_000,
        );
        hub.writer
            .submit(move |tx| {
                tx.execute_batch(&seed)
                    .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
            })
            .await
            .unwrap();

        let (status, stock) = call(&hub, "GET", urls[0], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{stock}");
        let items = stock["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            (items[0]["name"].as_str(), items[0]["qty"].clone()),
            (Some("Apple"), json!(0))
        );
        assert_eq!(items[1]["value_minor"], 300);
        assert_eq!(stock["total_value_minor"], 300);

        let (status, moves) = call(&hub, "GET", urls[1], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{moves}");
        let lines = moves["moves"].as_array().unwrap();
        let ids: Vec<_> = lines.iter().filter_map(|m| m["move_id"].as_str()).collect();
        assert_eq!(ids, ["M4", "M3", "M2", "M1"], "newest first");
        assert_eq!(lines[3]["qty"], 25);
        assert_eq!(lines[3]["value_minor"], 375, "a stock-in is worth its cost");
        assert_eq!(lines[2]["value_minor"], -60);
        assert_eq!(lines[3]["staff"], "Manager");
        assert_eq!(moves["total"], 4);
        assert_eq!(moves["truncated"], false);
        let (status, only) = call(
            &hub,
            "GET",
            &format!("{}&product=PB&reason=expired", urls[1]),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{only}");
        assert_eq!(only["moves"].as_array().map(Vec::len), Some(1));
        let (_, other_day) = call(
            &hub,
            "GET",
            "/ui/reports/stock/moves?from=2027-01-16&to=2027-01-16",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(other_day["moves"].as_array().map(Vec::len), Some(0));

        let (status, waste) = call(&hub, "GET", urls[2], Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{waste}");
        let reasons = waste["by_reason"].as_array().unwrap();
        assert_eq!(
            (reasons[0]["reason"].as_str(), reasons[0]["qty"].clone()),
            (Some("spoiled"), json!(3)),
            "the undone unit nets out"
        );
        assert_eq!(reasons[0]["value_minor"], 45);
        assert_eq!(reasons[4]["reason"], "expired");
        assert_eq!(reasons[4]["value_minor"], 20);
        assert_eq!(waste["total_value_minor"], 65);
        let products = waste["by_product"].as_array().unwrap();
        assert_eq!(products[0]["product"], "Cola");
        assert_eq!(products[1]["product"], "Apple");
    }

    /// Story 5.30: the Players and Staff reads need `report.view` and a valid range; Players also
    /// needs the branch key; an erased player (no data key) has no name and nothing else personal.
    #[tokio::test]
    async fn the_players_and_staff_reports_need_report_view_and_show_an_erased_player_without_data()
    {
        let url =
            |kind: &str, from: &str, to: &str| format!("/ui/reports/{kind}?from={from}&to={to}");
        let hub = hub().await;
        for kind in ["players", "staff"] {
            let (status, _) = call(
                &hub,
                "GET",
                &url(kind, "2027-01-15", "2027-01-15"),
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        let counter = token_for(&hub, "counter", "c-pass").await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        for kind in ["players", "staff"] {
            let (status, reply) = call(
                &hub,
                "GET",
                &url(kind, "2027-01-15", "2027-01-15"),
                Some(&counter),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
            assert_eq!(reply["code"], "PERMISSION_DENIED");
        }

        // Empty range: empty lists, zero totals, the three credit buckets.
        let (status, empty) = call(
            &hub,
            "GET",
            &url("players", "2027-01-15", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["top"].as_array().map(Vec::len), Some(0));
        assert_eq!(empty["guest"]["bills"], 0);
        assert_eq!(empty["credit_buckets"].as_array().map(Vec::len), Some(3));
        let (status, empty) = call(
            &hub,
            "GET",
            &url("staff", "2027-01-15", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["staff"].as_array().map(Vec::len), Some(0));

        // A named player, an erased one (a row with no `player_keys`), a guest and a void.
        let form = arena_proto::PlayerForm {
            username: "asha_9".into(),
            password: "secret1".into(),
            phone: "+91 98765 43210".into(),
            name: "Asha Rao".into(),
            email: String::new(),
            dob: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            consent: true,
            policy_version: "1".into(),
        };
        let sealed = arena_core::players::seal(&TEST_KEK, &form, "P1", "hash".into()).unwrap();
        let manager_id = hub.manager_id.clone();
        // Every value is a ULID or base64url text, so it is safe to write into the SQL.
        let seed = format!(
            "INSERT INTO players (player_id, username, password_hash, phone_lookup, name_enc,
                                  phone_enc, tags, rev, created_at, updated_at)
             VALUES ('P1', 'asha_9', 'h', 'l1', '{name}', '{phone}', '[]', 1, 0, 0),
                    ('P2', 'erased_2', 'h', 'l2', 'junk', 'junk', '[]', 1, 0, 0);
             INSERT INTO player_keys (player_id, dek_wrapped) VALUES ('P1', '{key}');
             INSERT INTO credit_balances (player_id, limit_minor, outstanding_minor,
                                          reminder_days, rev, created_at, updated_at)
             VALUES ('P2', 1000, 100, '', 1, 0, 0);
             INSERT INTO orders (id, status, kind, subtotal_minor, total_minor, business_day, fy,
                                 player_id, settled_by, voided_by, rev, created_at, updated_at)
             VALUES ('O1', 'settled', 'pos_sale', 500, 500, '2027-01-15', 'FY', 'P1', '{by}', NULL, 1, 0, 0),
                    ('O2', 'settled', 'session', 300, 300, '2027-01-15', 'FY', 'P2', '{by}', NULL, 1, 0, 0),
                    ('O3', 'settled', 'pos_sale', 70, 70, '2027-01-15', 'FY', NULL, '{by}', NULL, 1, 0, 0),
                    ('O4', 'void', 'session', 120, 120, '2027-01-15', 'FY', 'P1', NULL, '{by}', 1, 0, 0);
             INSERT INTO wallet_txns (id, player_id, delta_minor, reason, staff_id, order_id,
                                      business_day, fy, rev, created_at, updated_at)
             VALUES ('W1', 'P2', 100, 'credit', 'S', 'O2', '2027-01-15', 'FY', 1, 1, 1);",
            name = sealed.name_enc,
            phone = sealed.phone_enc,
            key = sealed.wrapped_key,
            by = manager_id,
        );
        hub.writer
            .submit(move |tx| {
                tx.execute_batch(&seed)
                    .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
            })
            .await
            .unwrap();

        let (status, players) = call(
            &hub,
            "GET",
            &url("players", "2027-01-15", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{players}");
        let top = players["top"].as_array().unwrap();
        assert_eq!(top.len(), 2, "the guest is never listed as a player");
        assert_eq!(
            (top[0]["name"].as_str(), top[0]["erased"].as_bool()),
            (Some("Asha Rao"), Some(false))
        );
        assert_eq!(top[0]["spend_minor"], 500);
        assert_eq!(
            (top[1]["name"].as_str(), top[1]["erased"].as_bool()),
            (Some(""), Some(true)),
            "no data key: erased, no name"
        );
        assert_eq!(
            (
                players["guest"]["bills"].clone(),
                players["guest"]["spend_minor"].clone()
            ),
            (json!(1), json!(70))
        );
        assert_eq!(players["new_players"]["players"], 2);
        assert_eq!(players["credit_outstanding_minor"], 100);
        let debtors = players["debtors"].as_array().unwrap();
        assert_eq!(debtors.len(), 1);
        assert_eq!(debtors[0]["erased"], true);
        let text = players.to_string();
        assert!(
            !text.contains("98765") && !text.contains("phone") && !text.contains("junk"),
            "no personal data beyond the name: {text}"
        );

        let (status, staff) = call(
            &hub,
            "GET",
            &url("staff", "2027-01-15", "2027-01-15"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{staff}");
        let rows = staff["staff"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{staff}");
        assert_eq!(rows[0]["staff_id"], hub.manager_id.as_str());
        assert_eq!(rows[0]["name"], "Manager");
        assert_eq!(
            (
                rows[0]["sales_minor"].clone(),
                rows[0]["sales_count"].clone()
            ),
            (json!(870), json!(3))
        );
        assert_eq!(
            (
                rows[0]["voids_count"].clone(),
                rows[0]["voids_minor"].clone()
            ),
            (json!(1), json!(120)),
            "the void is counted apart and is not in sales"
        );

        for (kind, from, to, field) in [
            ("players", "2027-01-16", "2027-01-15", "to"),
            ("staff", "2026-01-01", "2027-01-15", "to"),
            ("staff", "soon", "2027-01-15", "from"),
        ] {
            let (status, reply) =
                call(&hub, "GET", &url(kind, from, to), Some(&manager), None).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
            assert_eq!(reply["code"], "VALIDATION_FAILED");
            assert_eq!(reply["field"], field);
        }
    }

    /// Story 5.30: a Hub with no key store cannot open names, so Players answers `UNSUPPORTED`
    /// while Staff (which holds no personal data) still answers.
    #[tokio::test]
    async fn the_players_report_is_unsupported_without_a_key_store() {
        let hub = hub_without_kek().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let range = "from=2027-01-15&to=2027-01-15";
        let (status, reply) = call(
            &hub,
            "GET",
            &format!("/ui/reports/players?{range}"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
        assert_eq!(reply["code"], "UNSUPPORTED");
        let (status, _) = call(
            &hub,
            "GET",
            &format!("/ui/reports/staff?{range}"),
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_refund_lookup_and_a_refund_of_an_unknown_invoice_are_refused_cleanly() {
        use arena_proto::{ReasonCode, RefundCreate, RefundLineReq};
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/refund/invoice?no=X", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, reply) = call(
            &hub,
            "GET",
            "/ui/refund/invoice?no=DEV01%2F26-27%2FE1a%2F000001",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
        assert_eq!(reply["code"], "INVOICE_NOT_FOUND");

        let who = Actor {
            kind: ActorKind::Staff,
            id: "x".into(),
        };
        let refund = |token: String, reason: Option<ReasonCode>| {
            let hub = &hub;
            let who = who.clone();
            async move {
                let mut intent = intent_as(
                    who,
                    IntentPayload::RefundCreate(RefundCreate {
                        invoice_no: "DEV01/26-27/E1a/000001".into(),
                        lines: vec![RefundLineReq {
                            line_id: "L".into(),
                            qty: 1,
                            return_to_stock: None,
                        }],
                        method: None,
                        reference: None,
                        note: None,
                        receipt: None,
                    }),
                    None,
                );
                intent.reason = reason;
                call(
                    hub,
                    "POST",
                    "/ui/intent",
                    Some(&token),
                    Some(json!({ "intent": intent })),
                )
                .await
            }
        };
        // A counter holds no `refund.issue`.
        let (status, reply) = refund(counter, Some(ReasonCode::Mistake)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        assert_eq!(reply["code"], "PERMISSION_DENIED");
        let (status, reply) = refund(manager, Some(ReasonCode::Mistake)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
        assert_eq!(reply["code"], "INVOICE_NOT_FOUND");
    }

    #[tokio::test]
    async fn a_counter_sale_settles_lists_in_recent_sales_and_a_short_bundle_overrides_with_a_pin()
    {
        use arena_proto::{
            BundleComponent, PaymentMethod, PaymentRow, PosSale, PosSaleLine, PosSaleReverted,
            ProductUpsert, StockIn,
        };
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/pos/sales", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, view) = call(&hub, "GET", "/ui/pos/sales", Some(&counter), None).await;
        assert_eq!((status, &view["sales"]), (StatusCode::OK, &json!([])));
        assert_eq!(view["business_day"].as_str().map(str::len), Some(10));

        let who = || Actor {
            kind: ActorKind::Staff,
            id: "x".into(),
        };
        let post = |token: String, payload: IntentPayload, rev: Option<i64>| {
            let hub = &hub;
            async move {
                call(
                    hub,
                    "POST",
                    "/ui/intent",
                    Some(&token),
                    Some(json!({ "intent": intent_as(who(), payload, rev) })),
                )
                .await
            }
        };
        let product = |name: &str, price: i64, bundle: Option<(&str, i64)>| {
            IntentPayload::ProductUpsert(ProductUpsert {
                id: None,
                name: name.into(),
                category: String::new(),
                price_minor: price,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: bundle.is_none(),
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: bundle.is_some(),
                components: bundle
                    .map(|(id, qty)| {
                        vec![BundleComponent {
                            product_id: id.into(),
                            qty,
                        }]
                    })
                    .unwrap_or_default(),
            })
        };
        let (status, made) = post(manager.clone(), product("Chips", 1_000, None), None).await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        let chips = listed[0]["id"].as_str().unwrap().to_owned();
        let (status, made) = post(
            manager.clone(),
            IntentPayload::StockIn(StockIn {
                product_id: chips.clone(),
                qty: 3,
                unit_cost_minor: 400,
                supplier: String::new(),
                expiry: None,
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let (status, made) = post(
            manager.clone(),
            product("Combo", 3_000, Some((&chips, 2))),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        let combo = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "Combo")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let sale = |lines: &[(&str, i64)], total: i64, over: bool| {
            IntentPayload::PosSale(PosSale {
                business: None,
                coupon: None,
                discount: None,
                lines: lines
                    .iter()
                    .map(|(id, qty)| PosSaleLine {
                        product_id: (*id).into(),
                        qty: *qty,
                    })
                    .collect(),
                payments: vec![PaymentRow {
                    method: PaymentMethod::Cash,
                    amount_minor: total,
                    reference: None,
                }],
                player_id: None,
                credit_override: None,
                stock_override: over.then_some(true),
                receipt: None,
            })
        };

        // A plain sale: the reply carries the settled outcome for the Undo toast.
        let (status, reply) = post(counter.clone(), sale(&[(&chips, 1)], 1_000, false), None).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let settled = &reply["settled"];
        assert_eq!(settled["total_minor"], 1_000);
        assert_eq!(settled["irreversible"], false);
        assert_eq!(
            settled["undo_until"].as_i64().unwrap() - settled["settled_at"].as_i64().unwrap(),
            arena_proto::UNDO_WINDOW_MS
        );
        let order = settled["order_id"].as_str().unwrap().to_owned();
        let (_, view) = call(&hub, "GET", "/ui/pos/sales", Some(&counter), None).await;
        assert_eq!(view["sales"][0]["order_id"], json!(order));
        assert_eq!(view["sales"][0]["lines"][0]["name"], "Chips");
        assert_eq!(view["sales"][0]["payments"][0]["method"], "cash");
        assert!(view["sales"][0]["invoice_no"].is_null());

        // Undo within the window removes it from the list.
        let (status, reply) = post(
            counter.clone(),
            IntentPayload::PosSaleReverted(PosSaleReverted {
                order_id: order.clone(),
            }),
            settled["order_rev"].as_i64(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, view) = call(&hub, "GET", "/ui/pos/sales", Some(&counter), None).await;
        assert_eq!(view["sales"], json!([]));

        // 3 chips on hand; two combos need 4: refused 409 BUNDLE_SHORT naming the component.
        let two = sale(&[(&combo, 2)], 6_000, false);
        let (status, refused) = post(counter.clone(), two, None).await;
        assert_eq!(
            (status, refused["code"].as_str()),
            (StatusCode::CONFLICT, Some("BUNDLE_SHORT"))
        );
        assert!(refused["message"].as_str().unwrap().contains("Chips"));
        // Overriding needs stock.override: the counter alone is denied, with a manager PIN it sells.
        let over = sale(&[(&combo, 2)], 6_000, true);
        let (status, denied) = post(counter.clone(), over.clone(), None).await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let (status, elevated) = call(
            &hub,
            "POST",
            "/ui/elevate",
            Some(&counter),
            Some(json!({"items": [{"intent_kind": "pos.sale"}], "pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{elevated}");
        let (status, reply) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&counter),
            Some(json!({
                "intent": intent_as(who(), over, None),
                "elevation_ticket": elevated["ticket"],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (_, listed) = call(&hub, "GET", "/ui/products", Some(&counter), None).await;
        let left = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == json!(chips))
            .unwrap();
        assert_eq!(left["quantity"], 0);
    }

    #[tokio::test]
    async fn the_low_stock_page_needs_a_session_and_starts_empty() {
        let hub = hub().await;
        let (status, _) = call(&hub, "GET", "/ui/stock/low", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, view) = call(&hub, "GET", "/ui/stock/low", Some(&counter), None).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["low"], json!([]));
        assert_eq!(view["tracked"], json!([]));
    }

    #[tokio::test]
    async fn after_sign_out_the_token_is_refused() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, settings) = call(&hub, "GET", "/ui/settings", Some(&token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            settings
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["key"] == "receipt.footer" && s["rev"] == 1)
        );
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some(&token), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(footer_set(
                Actor {
                    kind: ActorKind::Staff,
                    id: "x".into(),
                },
                1,
            )),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("AUTH_FAILED"))
        );
        let (status, _) = call(&hub, "POST", "/ui/intent", None, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(&hub, "GET", "/ui/settings", Some(&token), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn malformed_bodies_are_bad_frame() {
        let hub = hub().await;
        let (status, body) = call(
            &hub,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(json!({"x": 1})),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("BAD_FRAME"))
        );
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(json!({"intent": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_origin_guard_covers_the_auth_routes() {
        let hub = hub().await;
        let req = Request::post("/ui/auth/sign-in")
            .header(header::HOST, "127.0.0.1:7409")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::from(r#"{"login":"counter","password":"c-pass"}"#))
            .unwrap();
        let res = hub.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let req = Request::options("/ui/intent")
            .header(header::HOST, "127.0.0.1:7409")
            .header(header::ORIGIN, "http://localhost:5173")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization, content-type",
            )
            .body(Body::empty())
            .unwrap();
        let res = hub.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let h = res.headers();
        assert_eq!(
            h[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://localhost:5173"
        );
        assert!(
            h[header::ACCESS_CONTROL_ALLOW_METHODS]
                .to_str()
                .unwrap()
                .contains("POST")
        );
        assert!(
            h[header::ACCESS_CONTROL_ALLOW_HEADERS]
                .to_str()
                .unwrap()
                .contains("authorization")
        );
        // The audit viewer's read ticket header is allowed too.
        let req = Request::options("/ui/audit")
            .header(header::HOST, "127.0.0.1:7409")
            .header(header::ORIGIN, "http://localhost:5173")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization, x-arena-elevation",
            )
            .body(Body::empty())
            .unwrap();
        let res = hub.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let allowed: Vec<String> = res.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap()
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .collect();
        for wanted in ["authorization", "x-arena-elevation"] {
            assert!(allowed.iter().any(|h| h == wanted), "{wanted}: {allowed:?}");
        }
    }

    #[tokio::test]
    async fn concurrent_attempts_past_the_limit_are_locked() {
        let hub = hub().await;
        let attempts = (0..8).map(|_| sign_in_as(&hub, "counter", "bad"));
        let answers = futures_util::future::join_all(attempts).await;
        let codes: Vec<&str> = answers
            .iter()
            .map(|(_, body)| body["code"].as_str().unwrap())
            .collect();
        assert_eq!(
            codes.iter().filter(|c| **c == "AUTH_FAILED").count(),
            5,
            "{codes:?}"
        );
        assert_eq!(
            codes.iter().filter(|c| **c == "AUTH_LOCKED").count(),
            3,
            "{codes:?}"
        );
        let (status, _) = sign_in_as(&hub, "counter", "c-pass").await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn sessions_expire_after_twelve_hours() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        hub.clock.advance(SESSION_TTL_MS - 1);
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(&token), None).await.0,
            StatusCode::OK
        );
        hub.clock.advance(1);
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(&token), None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn sessions_follow_the_staff_row() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let edit = |display: &str, active: bool, rev| {
            intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(hub.counter_id.clone()),
                    login: "counter".into(),
                    display_name: display.into(),
                    role_id: hub.counter_role.clone(),
                    password_hash: None,
                    pin_hash: None,
                    active,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                Some(rev),
            )
        };
        hub.writer
            .submit_intent(edit("Till", true, 1))
            .await
            .unwrap();
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(me["display_name"], "Till", "the cached name is refreshed");
        hub.writer
            .submit_intent(edit("Till", false, 2))
            .await
            .unwrap();
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(&token), None).await.0,
            StatusCode::UNAUTHORIZED
        );
        // The session is gone, not just refused.
        assert_eq!(
            call(&hub, "GET", "/ui/settings", Some(&token), None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_sixth_sign_in_ends_the_oldest_session() {
        let hub = hub().await;
        let mut tokens = Vec::new();
        for _ in 0..=MAX_SESSIONS_PER_STAFF {
            tokens.push(token_for(&hub, "manager", "m-pass").await);
            hub.clock.advance(1);
        }
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(&tokens[0]), None).await.0,
            StatusCode::UNAUTHORIZED
        );
        for t in &tokens[1..] {
            assert_eq!(
                call(&hub, "GET", "/ui/me", Some(t), None).await.0,
                StatusCode::OK
            );
        }
    }

    #[tokio::test]
    async fn signing_out_an_unknown_token_is_no_content() {
        let hub = hub().await;
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some("nope"), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    // ---- Story 1.13: PIN elevation over the loopback API ----

    fn item(kind: &str, key: Option<&str>) -> Value {
        match key {
            Some(k) => json!({"intent_kind": kind, "key": k}),
            None => json!({"intent_kind": kind}),
        }
    }

    fn currency_item() -> Value {
        item("settings.set", Some("branch.currency"))
    }

    async fn elevate_items(
        hub: &Hub,
        token: &str,
        items: Vec<Value>,
        pin: &str,
    ) -> (StatusCode, Value) {
        call(
            hub,
            "POST",
            "/ui/elevate",
            Some(token),
            Some(json!({"items": items, "pin": pin})),
        )
        .await
    }

    /// Elevates one `settings.set` of the currency.
    async fn elevate_currency(hub: &Hub, token: &str, pin: &str) -> (StatusCode, Value) {
        elevate_items(hub, token, vec![currency_item()], pin).await
    }

    fn setting_rev(hub: &Hub, key: &str) -> i64 {
        Store::open_reader(&hub.path)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .unwrap()
            .rev
    }

    fn set_body(hub: &Hub, key: &str, value: &str, ticket: Option<&str>) -> Value {
        let mut body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "whoever".into() },
            IntentPayload::SettingsSet(SettingsSet { key: key.into(), value: value.into() }),
            Some(setting_rev(hub, key)),
        ) });
        if let Some(t) = ticket {
            body["elevation_ticket"] = json!(t);
        }
        body
    }

    fn currency_set(hub: &Hub, value: &str, ticket: Option<&str>) -> Value {
        set_body(hub, "branch.currency", value, ticket)
    }

    fn change_count(hub: &Hub) -> usize {
        Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len()
    }

    fn last_change(hub: &Hub) -> arena_store::ChangeRecord {
        Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .pop()
            .unwrap()
    }

    async fn post_intent(hub: &Hub, token: &str, body: Value) -> (StatusCode, Value) {
        call(hub, "POST", "/ui/intent", Some(token), Some(body)).await
    }

    fn assert_pin_required(r: (StatusCode, Value)) {
        assert_eq!(
            (r.0, r.1["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PIN_REQUIRED")),
            "{}",
            r.1
        );
    }

    /// Adds an Owner (`boss` / `b-pass`, PIN 5555) the test hub does not seed.
    async fn add_owner(hub: &Hub) -> String {
        let rows = Store::open_reader(&hub.path)
            .unwrap()
            .rows(Table::Roles)
            .unwrap();
        let role = rows
            .iter()
            .find(|r| r["name"] == RowValue::Text("Owner".into()))
            .map(|r| match &r["role_id"] {
                RowValue::Text(t) => t.clone(),
                other => panic!("role_id is {other:?}"),
            })
            .unwrap();
        let applied = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: "boss".into(),
                    display_name: "Boss".into(),
                    role_id: role,
                    password_hash: Some(hash_secret("b-pass").unwrap()),
                    pin_hash: Some(hash_secret("5555").unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                None,
            ))
            .await
            .unwrap();
        applied.changes[0].row_id.clone()
    }

    fn deactivate_body(staff_id: &str, pin: &str) -> Value {
        json!({ "staff_id": staff_id, "pin": pin, "idempotency_key": ulid::Ulid::generate().to_string() })
    }

    #[tokio::test]
    async fn settings_staff_lists_members_and_an_owner_deactivates_with_their_own_pin() {
        let hub = hub().await;
        let boss = add_owner(&hub).await;
        let owner_token = token_for(&hub, "boss", "b-pass").await;
        let manager_token = token_for(&hub, "manager", "m-pass").await;
        let counter_token = token_for(&hub, "counter", "c-pass").await;

        let (status, list) = call(&hub, "GET", "/ui/staff", Some(&manager_token), None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(
            (list["used"].as_i64(), list["limit"].as_i64()),
            (Some(4), Some(10))
        );
        assert_eq!(list["staff"].as_array().unwrap().len(), 4);
        assert_eq!(list["staff"][0]["status"], "active");
        let (status, denied) = call(&hub, "GET", "/ui/staff", Some(&counter_token), None).await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );

        // The generic intent route does not take it: the PIN is part of the form.
        let (_, refused) = post_intent(
            &hub,
            &owner_token,
            json!({ "intent": intent_as(
                Actor { kind: ActorKind::Staff, id: boss.clone() },
                IntentPayload::StaffDeactivate(arena_proto::StaffDeactivate {
                    staff_id: hub.counter_id.clone(),
                }),
                None,
            ) }),
        )
        .await;
        assert_eq!(refused["code"], "UNSUPPORTED");

        let n = change_count(&hub);
        let url = "/ui/staff/deactivate";
        // A wrong PIN, another member's PIN and a Manager (who is not an Owner) all fail.
        let (status, wrong) = call(
            &hub,
            "POST",
            url,
            Some(&owner_token),
            Some(deactivate_body(&hub.counter_id, "2222")),
        )
        .await;
        assert_eq!(
            (status, wrong["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PIN_REJECTED"))
        );
        let (_, not_owner) = call(
            &hub,
            "POST",
            url,
            Some(&manager_token),
            Some(deactivate_body(&hub.counter_id, "2222")),
        )
        .await;
        assert_eq!(not_owner["code"], "PERMISSION_DENIED");
        assert_eq!(change_count(&hub), n, "nothing written");

        // Nobody deactivates themselves.
        let (_, itself) = call(
            &hub,
            "POST",
            url,
            Some(&owner_token),
            Some(deactivate_body(&boss, "5555")),
        )
        .await;
        assert_eq!(itself["code"], "VALIDATION_FAILED");

        let (status, ok) = call(
            &hub,
            "POST",
            url,
            Some(&owner_token),
            Some(deactivate_body(&hub.counter_id, "5555")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        let change = last_change(&hub);
        assert_eq!(
            (change.actor_id.as_str(), change.row_id.as_str()),
            (boss.as_str(), hub.counter_id.as_str())
        );
        // The Counter's sign-in ended at once and the slot is free.
        let (status, _) = call(&hub, "GET", "/ui/me", Some(&counter_token), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (_, list) = call(&hub, "GET", "/ui/staff", Some(&owner_token), None).await;
        assert_eq!(list["used"].as_i64(), Some(3));
        assert!(
            list["staff"]
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["id"] != hub.counter_id.as_str())
        );
    }
    #[tokio::test]
    async fn a_counter_turns_hub_high_contrast_on_with_a_manager_pin() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        assert!(!hub.auth.reader().snapshot(T0).unwrap().ui.hc);
        let (status, denied) =
            post_intent(&hub, &token, set_body(&hub, "display.hc.hub", "true", None)).await;
        assert_eq!(
            (status, denied["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let items = vec![item("settings.set", Some("display.hc.hub"))];
        let (status, ok) = elevate_items(&hub, &token, items, "2222").await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        let ticket = ok["ticket"].as_str().unwrap();
        let (status, reply) = post_intent(
            &hub,
            &token,
            set_body(&hub, "display.hc.hub", "true", Some(ticket)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let c = last_change(&hub);
        assert_eq!(c.tbl, "settings");
        assert!(c.row_id.ends_with("/display.hc.hub"));
        assert_eq!(
            (c.actor_id.as_str(), c.elevated_by_id.as_deref()),
            (hub.counter_id.as_str(), Some(hub.manager_id.as_str()))
        );
        assert!(hub.auth.reader().snapshot(T0).unwrap().ui.hc);
    }

    #[tokio::test]
    async fn a_manager_pin_approves_the_counters_settings_set() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (status, ok) = elevate_currency(&hub, &token, "2222").await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        let ticket = ok["ticket"].as_str().unwrap();
        assert_eq!(ticket.len(), 22);
        // The answer lists what was approved, and never says whose PIN it was.
        assert_eq!(ok["items"], json!([currency_item()]));
        assert_eq!(ok["expires_in_s"], 60);
        assert!(!ok.to_string().contains(&hub.manager_id));
        let (status, reply) =
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(ticket))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let c = last_change(&hub);
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("staff", hub.counter_id.as_str())
        );
        assert_eq!(
            (
                c.elevated_by_kind.as_deref(),
                c.elevated_by_id.as_deref(),
                c.elevated_by_mode.as_deref(),
                c.elevated_by_verifier_version
            ),
            (
                Some("pin"),
                Some(hub.manager_id.as_str()),
                Some("hub"),
                Some(1)
            )
        );
        // Single use: the same ticket is spent.
        let n = change_count(&hub);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "EUR", Some(ticket))).await,
        );
        assert_eq!(change_count(&hub), n);
    }

    #[tokio::test]
    async fn wrong_lacking_own_and_non_permission_pins_get_one_generic_answer() {
        let hub = hub().await;
        let n = change_count(&hub);
        let want =
            json!({"code": "PIN_REJECTED", "message": arena_core::staff::PIN_REJECTED_MESSAGE});
        for (items, pin) in [
            (vec![currency_item()], "9999"),
            // Kitchen's PIN: its role lacks settings.edit.
            (vec![currency_item()], "4444"),
            // The counter's own PIN: nobody approves their own intent.
            (vec![currency_item()], "3333"),
            (vec![currency_item()], "12"),
            // A system-only kind is never elevated, even with the manager's PIN.
            (vec![item("branch.bootstrap", None)], "2222"),
        ] {
            // The throttle (its own test) is reset between rows.
            let row_token = token_for(&hub, "counter", "c-pass").await;
            let (status, body) = elevate_items(&hub, &row_token, items.clone(), pin).await;
            assert_eq!(
                (status, &body),
                (StatusCode::FORBIDDEN, &want),
                "{items:?} {pin}"
            );
            hub.auth.elevate_lockout.lock().unwrap().entries.clear();
        }
        assert_eq!(change_count(&hub), n);
        // A manager holds settings.edit already: nothing to approve.
        let mtoken = token_for(&hub, "manager", "m-pass").await;
        let (status, body) = elevate_currency(&hub, &mtoken, "2222").await;
        assert_eq!(
            (status, body["code"].as_str(), body["field"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("VALIDATION_FAILED"),
                Some("items")
            )
        );
        // No session, no elevation.
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/elevate",
            None,
            Some(json!({"items": [currency_item()], "pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let token = token_for(&hub, "counter", "c-pass").await;
        let (status, body) = call(
            &hub,
            "POST",
            "/ui/elevate",
            Some(&token),
            Some(json!({"pin": "2222"})),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("BAD_FRAME"))
        );
        // Item shapes: none, more than 10, a settings.set without its key, a key on another kind,
        // a count beyond u8; and an unhandled kind.
        let eleven: Vec<Value> = (0..11).map(|_| currency_item()).collect();
        let three_hundred: Vec<Value> = (0..300).map(|_| currency_item()).collect();
        for items in [
            vec![],
            eleven,
            three_hundred,
            vec![item("settings.set", None)],
            vec![item("device.revoke", Some("x"))],
        ] {
            let (status, body) = elevate_items(&hub, &token, items, "2222").await;
            assert_eq!(
                (status, body["field"].as_str()),
                (StatusCode::UNPROCESSABLE_ENTITY, Some("items")),
                "{body}"
            );
        }
        let (status, body) = elevate_items(
            &hub,
            &token,
            vec![item("console.plan_requested", None)],
            "2222",
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("UNSUPPORTED"))
        );
        // None of those counted as a wrong PIN.
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn a_follower_refuses_elevation_before_checking_the_pin() {
        let hub = hub().await;
        let follower = ui_auth_router(Arc::new(UiAuth::new(
            Arc::new(StaffDirectory::open(&hub.path).unwrap()),
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            None,
            hub.clock.clone(),
        )));
        let (_, body) = call_app(
            &follower,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(json!({"login": "counter", "password": "c-pass"})),
        )
        .await;
        let token = body["token"].as_str().unwrap().to_owned();
        for pin in ["9999", "9999", "9999", "9999", "9999", "2222"] {
            let (status, body) = call_app(
                &follower,
                "POST",
                "/ui/elevate",
                Some(&token),
                Some(json!({"items": [currency_item()], "pin": pin})),
            )
            .await;
            assert_eq!(
                (status, body["code"].as_str()),
                (StatusCode::SERVICE_UNAVAILABLE, Some("NOT_PRIMARY"))
            );
        }
    }

    #[tokio::test]
    async fn a_follower_may_grant_a_read_elevation() {
        let hub = hub().await;
        let follower = ui_auth_router(Arc::new(UiAuth::new(
            Arc::new(StaffDirectory::open(&hub.path).unwrap()),
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            None,
            hub.clock.clone(),
        )));
        let (_, body) = call_app(
            &follower,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(json!({"login": "counter", "password": "c-pass"})),
        )
        .await;
        let token = body["token"].as_str().unwrap().to_owned();
        let (status, body) = call_app(
            &follower,
            "POST",
            "/ui/elevate",
            Some(&token),
            Some(json!({"items": [{"permission": "audit.view"}], "pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["expires_in_s"], 600);
    }

    #[tokio::test]
    async fn a_spoofed_elevated_by_is_stripped_and_the_counter_denied() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let mut body = currency_set(&hub, "USD", None);
        body["intent"]["elevated_by"] =
            json!({"kind": "pin", "id": hub.manager_id, "mode": "hub", "verifier_version": 1});
        let n = change_count(&hub);
        let (status, reply) = post_intent(&hub, &token, body).await;
        assert_eq!(
            (status, reply["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        assert_eq!(change_count(&hub), n);
    }

    #[tokio::test]
    async fn tickets_are_bound_to_their_session_items_and_sixty_seconds() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let other = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_items(
            &hub,
            &token,
            vec![
                currency_item(),
                item("settings.set", Some("branch.fy_start_month")),
            ],
            "2222",
        )
        .await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        // Another session, another kind, another key and a junk ticket are refused without
        // spending anything.
        assert_pin_required(
            post_intent(&hub, &other, currency_set(&hub, "USD", Some(&ticket))).await,
        );
        let mut revoke = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke { device_id: "01JNOPEAAAAAAAAAAAAAAAAAAA".into() }),
            None,
        ), "elevation_ticket": ticket });
        revoke["intent"]["reason"] = json!("lost");
        assert_pin_required(post_intent(&hub, &token, revoke).await);
        assert_pin_required(
            post_intent(
                &hub,
                &token,
                set_body(&hub, "branch.tz", "Asia/Dubai", Some(&ticket)),
            )
            .await,
        );
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "USD", Some("junk"))).await,
        );
        // Each approved item once, in any order.
        let (status, reply) = post_intent(
            &hub,
            &token,
            set_body(&hub, "branch.fy_start_month", "1", Some(&ticket)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let (status, reply) =
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(&ticket))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "GBP", Some(&ticket))).await,
        );
        // Expiry: a ticket is good for 60 s.
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let late = ok["ticket"].as_str().unwrap().to_owned();
        hub.clock.advance(TICKET_TTL_MS);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "GBP", Some(&late))).await,
        );
        // Dropped: its approvals are gone.
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let dropped = ok["ticket"].as_str().unwrap().to_owned();
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/elevate/drop",
            Some(&other),
            Some(json!({"ticket": dropped})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(
            hub.auth.tickets.lock().unwrap().contains_key(&dropped),
            "another session cannot drop it"
        );
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/elevate/drop",
            Some(&token),
            Some(json!({"ticket": dropped})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "GBP", Some(&dropped))).await,
        );
        // Signing out ends the session's tickets.
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let t = ok["ticket"].as_str().unwrap().to_owned();
        call(&hub, "POST", "/ui/auth/sign-out", Some(&token), None).await;
        assert!(!hub.auth.tickets.lock().unwrap().contains_key(&t));
        let (status, _) = post_intent(&hub, &token, currency_set(&hub, "GBP", Some(&t))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_session_holds_at_most_four_tickets_and_the_oldest_goes() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let mut tickets = Vec::new();
        for _ in 0..5 {
            let (_, ok) = elevate_currency(&hub, &token, "2222").await;
            tickets.push(ok["ticket"].as_str().unwrap().to_owned());
            hub.clock.advance(1);
        }
        let live = hub
            .auth
            .tickets
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.session == token)
            .count();
        assert_eq!(live, MAX_TICKETS_PER_SESSION);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(&tickets[0]))).await,
        );
        let (status, reply) =
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(&tickets[1]))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
    }

    #[tokio::test]
    async fn a_refusal_gives_the_approval_back_but_never_revives_a_ticket() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        // STALE (nothing applied): the approval is given back.
        let mut stale = currency_set(&hub, "USD", Some(&ticket));
        stale["intent"]["base_rev"] = json!(99);
        let (_, reply) = post_intent(&hub, &token, stale).await;
        assert_eq!(reply["code"], "STALE");
        let (status, reply) =
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(&ticket))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        // A refusal after the ticket expired does not bring it back.
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        let (elevated, index) = hub
            .auth
            .redeem(&ticket, &token, &{
                let mut i: Intent =
                    serde_json::from_value(currency_set(&hub, "EUR", None)["intent"].clone())
                        .unwrap();
                // As `/ui/intent` sets it: the session's member.
                i.actor.id = hub.counter_id.clone();
                i
            })
            .unwrap();
        assert_eq!(elevated.id, hub.manager_id);
        hub.clock.advance(TICKET_TTL_MS);
        hub.auth.refund(&ticket, index);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "EUR", Some(&ticket))).await,
        );
        // Nor does a refund for a dropped ticket.
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        let (_, index) = hub
            .auth
            .redeem(&ticket, &token, &{
                let mut i: Intent =
                    serde_json::from_value(currency_set(&hub, "EUR", None)["intent"].clone())
                        .unwrap();
                // As `/ui/intent` sets it: the session's member.
                i.actor.id = hub.counter_id.clone();
                i
            })
            .unwrap();
        hub.auth.drop_tickets(&token);
        hub.auth.refund(&ticket, index);
        assert!(!hub.auth.tickets.lock().unwrap().contains_key(&ticket));
    }

    async fn rotate_manager_pin(hub: &Hub, pin: &str, rev: i64) {
        hub.writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(hub.manager_id.clone()),
                    login: "manager".into(),
                    display_name: "Manager".into(),
                    role_id: hub.manager_role.clone(),
                    password_hash: None,
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                Some(rev),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_pin_rotated_after_elevation_needs_entering_again_and_the_same_key_then_applies() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        rotate_manager_pin(&hub, "2468", 1).await;
        let n = change_count(&hub);
        let mut body = currency_set(&hub, "USD", Some(&ticket));
        assert_pin_required(post_intent(&hub, &token, body.clone()).await);
        assert_eq!(change_count(&hub), n);
        // The old PIN no longer approves anything; the new one does, at version 2.
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.1["code"],
            "PIN_REJECTED"
        );
        let (_, ok) = elevate_currency(&hub, &token, "2468").await;
        // The same intent (same idempotency key) retried with the new ticket applies.
        body["elevation_ticket"] = ok["ticket"].clone();
        let (status, reply) = post_intent(&hub, &token, body).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(last_change(&hub).elevated_by_verifier_version, Some(2));
    }

    #[tokio::test]
    async fn five_wrong_pins_pause_elevation_for_the_staff_member() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        for _ in 0..4 {
            assert_eq!(
                elevate_currency(&hub, &token, "9999").await.1["code"],
                "PIN_REJECTED"
            );
        }
        // The fifth failure already answers the pause, with its countdown.
        let (status, body) = elevate_currency(&hub, &token, "9999").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::TOO_MANY_REQUESTS, Some("AUTH_LOCKED"))
        );
        assert_eq!(body["retry_after_s"], 60);
        // Even the right PIN waits, and a new session (sign out, sign in) is still paused.
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.1["code"],
            "AUTH_LOCKED"
        );
        call(&hub, "POST", "/ui/auth/sign-out", Some(&token), None).await;
        let token = token_for(&hub, "counter", "c-pass").await;
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.1["code"],
            "AUTH_LOCKED"
        );
        // Another staff member is not affected.
        let kitchen = token_for(&hub, "kitchen", "k-pass").await;
        assert_eq!(
            elevate_currency(&hub, &kitchen, "2222").await.0,
            StatusCode::OK
        );
        hub.clock.advance(30_000);
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.1["retry_after_s"],
            30
        );
        hub.clock.advance(30_000);
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.0,
            StatusCode::OK
        );
        // A success resets the count.
        for _ in 0..4 {
            elevate_currency(&hub, &token, "9999").await;
        }
        assert_eq!(
            elevate_currency(&hub, &token, "2222").await.0,
            StatusCode::OK
        );
        assert_eq!(
            elevate_currency(&hub, &token, "9999").await.1["code"],
            "PIN_REJECTED"
        );
    }

    #[tokio::test]
    async fn an_internal_error_mid_streak_neither_counts_nor_resets_it() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        for _ in 0..4 {
            assert_eq!(
                elevate_currency(&hub, &token, "9999").await.1["code"],
                "PIN_REJECTED"
            );
        }
        // The PIN check cannot run (its permits are gone): INTERNAL, the reservation released.
        hub.auth.pin_checks.close();
        let (status, body) = elevate_currency(&hub, &token, "2222").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::INTERNAL_SERVER_ERROR, Some("INTERNAL"))
        );
        let lockout = hub.auth.elevate_lockout.lock().unwrap();
        let entry = lockout.entries.get(&hub.counter_id).copied().unwrap();
        assert_eq!(
            (entry.failures, entry.in_flight, entry.locked_until),
            (4, 0, None)
        );
    }

    #[test]
    fn the_elevation_lockout_keys_exactly() {
        let mut l = Lockout::exact();
        for _ in 0..LOCKOUT_FAILURES {
            l.begin("01JAB", T0).unwrap();
            l.finish("01JAB", T0, false);
        }
        assert!(l.begin("01JAB", T0).is_err());
        assert!(l.begin("01jab", T0).is_ok());
        l.release("01jab", T0);
        assert!(!l.entries.contains_key("01jab"));
    }

    #[tokio::test]
    async fn a_revoke_without_a_reason_keeps_its_approval_and_the_same_key_then_applies() {
        let hub = hub().await;
        let hash = arena_core::devices::hash_token_secret("secret-secret-secret").unwrap();
        let device = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "Kiosk 1".into(),
                    token_hash: hash,
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_items(&hub, &token, vec![item("device.revoke", None)], "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        let mut body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke { device_id: device.clone() }),
            None,
        ), "elevation_ticket": ticket });
        let (status, reply) = post_intent(&hub, &token, body.clone()).await;
        assert_eq!(
            (status, reply["code"].as_str(), reply["field"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("REASON_REQUIRED"),
                Some("reason")
            )
        );
        // The same intent (same idempotency key) with a reason applies on the same ticket.
        body["intent"]["reason"] = json!("stolen");
        let (status, reply) = post_intent(&hub, &token, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let c = last_change(&hub);
        assert_eq!(
            (c.intent_kind.as_str(), c.reason.as_deref()),
            ("device.revoke", Some("stolen"))
        );
        // Spent now (a retry of the applied intent would need a new approval).
        body["intent"]["idempotency_key"] = json!(ulid::Ulid::generate().to_string());
        let (_, reply) = post_intent(&hub, &token, body).await;
        assert_eq!(reply["code"], "PIN_REQUIRED");
    }

    // ── Idle lock (story 1.14) ─────────────────────────────────────────────────────────

    async fn unlock_with(hub: &Hub, token: &str, pin: &str) -> (StatusCode, Value) {
        call(
            hub,
            "POST",
            "/ui/unlock",
            Some(token),
            Some(json!({"pin": pin})),
        )
        .await
    }

    async fn lock_session(hub: &Hub, token: &str) {
        let (status, body) = call(hub, "POST", "/ui/lock", Some(token), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    fn assert_session_locked(r: (StatusCode, Value)) {
        assert_eq!(
            (r.0, r.1["code"].as_str()),
            (StatusCode::LOCKED, Some("SESSION_LOCKED")),
            "{}",
            r.1
        );
    }

    fn footer_body(value: &str, hub: &Hub) -> Value {
        set_body(hub, "receipt.footer", value, None)
    }

    #[tokio::test]
    async fn a_locked_session_refuses_intents_and_elevation_until_its_own_pin_unlocks_it() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        // A ticket granted before the lock does not survive it.
        let (_, ok) = elevate_currency(&hub, &counter, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        lock_session(&hub, &token).await;
        lock_session(&hub, &counter).await;
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(me["locked"], true);
        let n = change_count(&hub);
        assert_session_locked(post_intent(&hub, &token, footer_body("x", &hub)).await);
        assert_session_locked(elevate_currency(&hub, &counter, "2222").await);
        assert_eq!(change_count(&hub), n, "nothing applied while locked");
        // The own PIN unlocks the same session for the same member.
        let (status, ok) = unlock_with(&hub, &token, "2222").await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        assert_eq!(ok["switched"], false);
        assert_eq!(ok["staff"]["staff_id"], hub.manager_id.as_str());
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(
            (me["locked"].as_bool(), me["staff_id"].as_str()),
            (Some(false), Some(hub.manager_id.as_str()))
        );
        let (status, reply) = post_intent(&hub, &token, footer_body("after", &hub)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(last_change(&hub).actor_id, hub.manager_id);
        // The counter's own PIN unlocks theirs; the pre-lock ticket is gone.
        let (_, ok) = unlock_with(&hub, &counter, "3333").await;
        assert_eq!(ok["switched"], false);
        assert_pin_required(
            post_intent(&hub, &counter, currency_set(&hub, "USD", Some(&ticket))).await,
        );
        // Locking and unlocking without a session is refused.
        assert_eq!(
            call(&hub, "POST", "/ui/lock", None, None).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(
                &hub,
                "POST",
                "/ui/unlock",
                Some("nope"),
                Some(json!({"pin": "2222"}))
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let (status, body) = call(&hub, "POST", "/ui/unlock", Some(&token), Some(json!({}))).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("BAD_FRAME"))
        );
    }

    #[tokio::test]
    async fn another_members_pin_unlocks_and_rebinds_the_session_to_them() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &token).await;
        let (status, ok) = unlock_with(&hub, &token, "2222").await;
        assert_eq!(status, StatusCode::OK, "{ok}");
        assert_eq!(ok["switched"], true);
        assert_eq!(ok["staff"]["staff_id"], hub.manager_id.as_str());
        assert_eq!(ok["staff"]["display_name"], "Manager");
        assert!(
            ok["staff"]["permissions"]
                .as_array()
                .unwrap()
                .contains(&json!("settings.edit"))
        );
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(me["staff_id"], hub.manager_id.as_str());
        // The same token now acts as the manager, whatever actor the client sends.
        let (status, reply) = post_intent(&hub, &token, currency_set(&hub, "USD", None)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let c = last_change(&hub);
        assert_eq!(
            (
                c.actor_kind.as_str(),
                c.actor_id.as_str(),
                c.elevated_by_kind
            ),
            ("staff", hub.manager_id.as_str(), None)
        );
        // A Kitchen member's PIN rebinds as well (any active member with a PIN).
        lock_session(&hub, &token).await;
        let (_, ok) = unlock_with(&hub, &token, "4444").await;
        assert_eq!(
            (
                ok["switched"].as_bool(),
                ok["staff"]["display_name"].as_str()
            ),
            (Some(true), Some("Kitchen"))
        );
    }

    #[tokio::test]
    async fn five_wrong_unlock_pins_pause_unlocking_and_sign_out_still_works() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &token).await;
        for i in 0..4 {
            let (status, body) = unlock_with(&hub, &token, "9999").await;
            assert_eq!(
                (status, body),
                (
                    StatusCode::FORBIDDEN,
                    json!({"code": "PIN_REJECTED", "message": UNLOCK_REJECTED_MESSAGE})
                ),
                "attempt {i}"
            );
        }
        // Junk is a failure too.
        let (status, body) = unlock_with(&hub, &token, "12ab").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::TOO_MANY_REQUESTS, Some("AUTH_LOCKED"))
        );
        assert_eq!(body["retry_after_s"], 60);
        // Even the right PIN waits.
        let (_, body) = unlock_with(&hub, &token, "3333").await;
        assert_eq!(body["code"], "AUTH_LOCKED");
        hub.clock.advance(30_000);
        let (_, body) = unlock_with(&hub, &token, "3333").await;
        assert_eq!(body["retry_after_s"], 30);
        assert_session_locked(post_intent(&hub, &token, footer_body("x", &hub)).await);
        hub.clock.advance(30_000);
        let (status, _) = unlock_with(&hub, &token, "3333").await;
        assert_eq!(status, StatusCode::OK);
        // Sign-out works while paused.
        let other = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &other).await;
        for _ in 0..5 {
            unlock_with(&hub, &other, "0000").await;
        }
        assert_eq!(
            unlock_with(&hub, &other, "3333").await.1["code"],
            "AUTH_LOCKED"
        );
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some(&other), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(&other), None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn the_hub_keeps_applying_while_a_session_is_locked() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let hash = arena_core::devices::hash_token_secret("secret-secret-secret").unwrap();
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let device = hub
            .writer
            .submit_intent(intent_as(
                owner.clone(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "Kiosk 1".into(),
                    token_hash: hash,
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        lock_session(&hub, &token).await;
        let mut revoke = intent_as(
            owner,
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: device.clone(),
            }),
            None,
        );
        revoke.reason = Some(arena_proto::ReasonCode::Lost);
        hub.writer.submit_intent(revoke).await.unwrap();
        unlock_with(&hub, &token, "3333").await;
        let snap = hub.auth.reader().snapshot(hub.clock.now_ms()).unwrap();
        assert!(snap.devices.iter().all(|d| d.device_id != device));
    }

    #[tokio::test]
    async fn the_snapshot_carries_the_idle_lock_and_out_of_range_values_are_refused() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        assert_eq!(
            hub.auth.reader().snapshot(T0).unwrap().ui.idle_lock_minutes,
            5
        );
        let n = change_count(&hub);
        for bad in ["0", "31"] {
            let (status, body) =
                post_intent(&hub, &token, set_body(&hub, "idle_lock_minutes", bad, None)).await;
            assert_eq!(
                (status, body["code"].as_str(), body["field"].as_str()),
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Some("VALIDATION_FAILED"),
                    Some("value")
                ),
                "{bad}"
            );
        }
        assert_eq!(change_count(&hub), n);
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "idle_lock_minutes", "10", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            hub.auth.reader().snapshot(T0).unwrap().ui.idle_lock_minutes,
            10
        );
    }

    /// Log lines written while the guard lives, as text.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test]
    async fn lock_and_unlock_log_the_staff_ids_and_never_the_pin() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let logs = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::INFO)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        lock_session(&hub, &token).await;
        unlock_with(&hub, &token, "9753").await;
        unlock_with(&hub, &token, "3333").await;
        lock_session(&hub, &token).await;
        unlock_with(&hub, &token, "2222").await;
        drop(guard);
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let named = |msg: &str| -> Vec<&Value> {
            lines
                .iter()
                .filter(|l| l["fields"]["message"] == msg)
                .map(|l| &l["fields"])
                .collect()
        };
        let locked = named("ui.locked");
        assert_eq!(locked.len(), 2, "{text}");
        assert_eq!(locked[0]["staff_id"], hub.counter_id.as_str());
        assert_eq!(locked[0]["at"], T0);
        let unlocked = named("ui.unlocked");
        assert_eq!(unlocked.len(), 2, "{text}");
        assert_eq!(unlocked[0]["staff_id"], hub.counter_id.as_str());
        assert_eq!(unlocked[0].get("previous_staff_id"), None);
        assert_eq!(unlocked[1]["staff_id"], hub.manager_id.as_str());
        assert_eq!(unlocked[1]["previous_staff_id"], hub.counter_id.as_str());
        for pin in ["9753", "3333", "2222"] {
            assert!(!text.contains(pin), "a PIN reached the log: {text}");
        }
        assert!(
            !text.contains("pin_len") && !text.contains("length"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn unlocking_a_session_that_is_not_locked_is_refused_before_any_pin_check() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (status, body) = unlock_with(&hub, &token, "2222").await;
        assert_eq!(
            (status, body["code"].as_str(), body["field"].as_str()),
            (
                StatusCode::CONFLICT,
                Some("VALIDATION_FAILED"),
                Some("session")
            )
        );
        // Still the counter, and no failure was counted.
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(me["staff_id"], hub.counter_id.as_str());
        assert!(hub.auth.unlock_lockout.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn unlock_pauses_double_up_to_fifteen_minutes_and_reset_on_success() {
        let mut t = UnlockThrottle::default();
        let keys = unlock_keys("S", "T");
        let mut now = T0;
        let mut pauses = Vec::new();
        for _ in 0..6 {
            let mut last = None;
            for _ in 0..LOCKOUT_FAILURES {
                t.begin(&keys, now).unwrap();
                last = t.finish(&keys, now, false);
            }
            let (retry, _) = last.unwrap();
            pauses.push(retry);
            assert_eq!(t.begin(&keys, now), Err(retry));
            now += i64::from(retry) * 1000;
        }
        assert_eq!(pauses, vec![60, 120, 240, 480, 900, 900]);
        t.begin(&keys, now).unwrap();
        t.finish(&keys, now, true);
        for _ in 0..LOCKOUT_FAILURES {
            t.begin(&keys, now).unwrap();
            t.finish(&keys, now, false);
        }
        assert_eq!(
            t.begin(&keys, now),
            Err(60),
            "a success restarts the doubling"
        );
        // Either key alone refuses: another token of the same member, or this token re-bound.
        assert!(t.begin(&unlock_keys("S", "OTHER"), now).is_err());
        assert!(t.begin(&unlock_keys("OTHER", "T"), now).is_err());
        assert!(t.begin(&unlock_keys("OTHER", "OTHER"), now).is_ok());
    }

    #[tokio::test]
    async fn the_unlock_pause_follows_the_member_into_a_new_session_and_doubles() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &token).await;
        for _ in 0..5 {
            unlock_with(&hub, &token, "9999").await;
        }
        // A fresh session for the same member is still paused.
        let fresh = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &fresh).await;
        let (status, body) = unlock_with(&hub, &fresh, "3333").await;
        assert_eq!(
            (
                status,
                body["code"].as_str(),
                body["retry_after_s"].as_u64()
            ),
            (StatusCode::TOO_MANY_REQUESTS, Some("AUTH_LOCKED"), Some(60))
        );
        hub.clock.advance(60_000);
        for _ in 0..5 {
            unlock_with(&hub, &fresh, "9999").await;
        }
        let (_, body) = unlock_with(&hub, &token, "3333").await;
        assert_eq!(body["retry_after_s"], 120, "the second pause doubles");
        hub.clock.advance(120_000);
        assert_eq!(unlock_with(&hub, &token, "3333").await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_switched_session_cannot_redeem_its_earlier_tickets() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        lock_session(&hub, &token).await;
        let (_, ok) = unlock_with(&hub, &token, "4444").await;
        assert_eq!(ok["switched"], true);
        let n = change_count(&hub);
        assert_pin_required(
            post_intent(&hub, &token, currency_set(&hub, "USD", Some(&ticket))).await,
        );
        assert_eq!(change_count(&hub), n);
    }

    #[tokio::test]
    async fn a_ticket_requested_before_the_session_changed_hands_is_refused() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let items = vec![ElevationItem::intent(
            IntentKind::SettingsSet,
            Some("branch.currency".into()),
        )];
        let issue = || {
            hub.auth
                .issue_ticket("T1", &token, &hub.counter_id, &hub.manager_id, 1, &items)
        };
        lock_session(&hub, &token).await;
        assert_eq!(issue().unwrap_err().code, ErrorCode::SessionLocked);
        unlock_with(&hub, &token, "2222").await;
        // Unlocked, but now acting as the manager: the counter's request is not theirs.
        assert_eq!(issue().unwrap_err().code, ErrorCode::SessionLocked);
        assert!(hub.auth.tickets.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_staff_refresh_never_reverts_a_rebound_session() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let stale = hub.auth.session_for(&token).await.unwrap();
        lock_session(&hub, &token).await;
        unlock_with(&hub, &token, "2222").await;
        // A refresh that read the counter before the re-bind lands after it.
        let (staff, locked) = hub
            .auth
            .store_refresh(&token, &hub.counter_id, Some(stale))
            .unwrap();
        assert_eq!(
            (staff.staff_id.as_str(), locked),
            (hub.manager_id.as_str(), false)
        );
        // Nor does a "member gone" answer for the old member end the session.
        hub.auth
            .store_refresh(&token, &hub.counter_id, None)
            .unwrap();
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(me["staff_id"], hub.manager_id.as_str());
    }

    #[tokio::test]
    async fn a_member_deactivated_after_their_pin_matched_unlocks_nothing() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        lock_session(&hub, &token).await;
        let reached = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        *hub.auth.unlock_pause.lock().unwrap() = Some((reached.clone(), resume.clone()));
        let app = hub.app.clone();
        let t = token.clone();
        let pending = tokio::spawn(async move {
            call_app(
                &app,
                "POST",
                "/ui/unlock",
                Some(&t),
                Some(json!({"pin": "2222"})),
            )
            .await
        });
        reached.notified().await;
        hub.writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(hub.manager_id.clone()),
                    login: "manager".into(),
                    display_name: "Manager".into(),
                    role_id: hub.manager_role.clone(),
                    password_hash: None,
                    pin_hash: None,
                    active: false,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                Some(1),
            ))
            .await
            .unwrap();
        resume.notify_one();
        let (status, body) = pending.await.unwrap();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PIN_REJECTED"))
        );
        let keys = unlock_keys(&hub.counter_id, &token);
        assert_eq!(
            hub.auth.unlock_lockout.lock().unwrap().entries[&keys[0]].failures,
            1,
            "it counts as a failure"
        );
        let (_, me) = call(&hub, "GET", "/ui/me", Some(&token), None).await;
        assert_eq!(
            (me["locked"].as_bool(), me["staff_id"].as_str()),
            (Some(true), Some(hub.counter_id.as_str()))
        );
    }

    #[tokio::test]
    async fn dropping_a_ticket_and_reading_settings_work_while_locked() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, ok) = elevate_currency(&hub, &token, "2222").await;
        let ticket = ok["ticket"].as_str().unwrap().to_owned();
        lock_session(&hub, &token).await;
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/elevate/drop",
            Some(&token),
            Some(json!({"ticket": ticket})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, settings) = call(&hub, "GET", "/ui/settings", Some(&token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            settings
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["key"] == "idle_lock_minutes")
        );
    }

    #[tokio::test]
    async fn the_own_member_wins_when_two_share_a_pin() {
        let hub = hub().await;
        rotate_manager_pin(&hub, "3333", 1).await;
        for (login, pw, id) in [
            ("counter", "c-pass", hub.counter_id.clone()),
            ("manager", "m-pass", hub.manager_id.clone()),
        ] {
            let token = token_for(&hub, login, pw).await;
            lock_session(&hub, &token).await;
            let (status, ok) = unlock_with(&hub, &token, "3333").await;
            assert_eq!(status, StatusCode::OK, "{ok}");
            assert_eq!(
                (ok["switched"].as_bool(), ok["staff"]["staff_id"].as_str()),
                (Some(false), Some(id.as_str())),
                "{login}"
            );
        }
    }

    fn probes(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(arena_proto::BACKUP_PROBE_PREFIX)
            })
            .count()
    }

    fn setting_value(hub: &Hub, key: &str) -> String {
        Store::open_reader(&hub.path)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .unwrap()
            .value
    }

    #[tokio::test]
    async fn a_writable_backup_location_is_saved_after_the_probe() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "system.backup_dir", &path, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(setting_value(&hub, "system.backup_dir"), path);
        assert_eq!(probes(dir.path()), 0, "the probe file is removed");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        let (status, _) =
            post_intent(&hub, &token, set_body(&hub, "system.backup_dir", "", None)).await;
        assert_eq!(status, StatusCode::OK, "clearing needs no probe");
    }

    #[tokio::test]
    async fn a_missing_file_or_read_only_backup_location_is_refused_with_its_reason() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        let before = change_count(&hub);
        for (path, reason) in [
            (dir.path().join("missing"), "not_found"),
            (file.clone(), "not_dir"),
        ] {
            let (status, body) = post_intent(
                &hub,
                &token,
                set_body(&hub, "system.backup_dir", &path.to_string_lossy(), None),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert_eq!(body["code"], "VALIDATION_FAILED");
            assert_eq!(body["field"], "system.backup_dir");
            assert_eq!(body["message"], reason);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ro = dir.path().join("ro");
            std::fs::create_dir(&ro).unwrap();
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
            let root = std::fs::write(ro.join("root-check"), b"x").is_ok();
            let (status, body) = post_intent(
                &hub,
                &token,
                set_body(&hub, "system.backup_dir", &ro.to_string_lossy(), None),
            )
            .await;
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
            let (status, body) = if root {
                let sealed = if cfg!(target_os = "linux") {
                    "/proc/self"
                } else {
                    "/"
                };
                post_intent(
                    &hub,
                    &token,
                    set_body(&hub, "system.backup_dir", sealed, None),
                )
                .await
            } else {
                assert_eq!(probes(&ro), 0);
                (status, body)
            };
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert_eq!(body["message"], "not_writable");
        }
        assert_eq!(change_count(&hub), before);
        assert_eq!(setting_value(&hub, "system.backup_dir"), "");
    }

    #[tokio::test]
    async fn a_counter_is_denied_operating_settings_and_no_probe_is_written() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let dir = tempfile::tempdir().unwrap();
        let before = change_count(&hub);
        for body in [
            set_body(
                &hub,
                "system.backup_dir",
                &dir.path().to_string_lossy(),
                None,
            ),
            set_body(&hub, "branch.fulfilment", "seat", None),
            set_body(&hub, "kiosk.volume_default", "20", None),
        ] {
            let (status, reply) = post_intent(&hub, &token, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
            assert_eq!(reply["code"], "PERMISSION_DENIED");
        }
        let consent = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "whoever".into() },
            IntentPayload::AdsConsent(arena_proto::AdsConsentSet {
                given: true,
                policy_version: arena_proto::ADS_POLICY_VERSION,
            }),
            Some(setting_rev(&hub, "branch.ads_consent")),
        ) });
        let (status, reply) = post_intent(&hub, &token, consent).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{reply}");
        assert_eq!(change_count(&hub), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn ads_consent_records_the_session_staff_and_healthz_reports_setup() {
        let hub = hub().await;
        let health = crate::health::router(hub._status.subscribe(), hub.clock.clone());
        let (_, unpublished) = call_app(&health, "GET", "/healthz", None, None).await;
        assert_eq!(unpublished["setup"], Value::Null);
        let job = tokio::spawn(arena_core::setup::publish_setup(
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            Some(hub.writer.subscribe()),
            hub._status.clone(),
        ));
        let setup_is = |want: Value| {
            let health = health.clone();
            async move {
                for _ in 0..100 {
                    let (_, body) = call_app(&health, "GET", "/healthz", None, None).await;
                    if body["setup"] == want {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                panic!("healthz never reported {want}");
            }
        };
        setup_is(json!({"opening_hours_set": false, "backup_location_set": false, "ad_consent": "missing", "ad_rating": "PG", "printer": "unset", "products_count": 0, "groups_without_rate": 0, "manager_exists": true, "test_session": "unset", "live": false})).await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let consent = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Owner, id: "spoof".into() },
            IntentPayload::AdsConsent(arena_proto::AdsConsentSet {
                given: true,
                policy_version: arena_proto::ADS_POLICY_VERSION,
            }),
            Some(setting_rev(&hub, "branch.ads_consent")),
        ) });
        let (status, body) = post_intent(&hub, &token, consent).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let stored =
            arena_proto::AdsConsent::parse(&setting_value(&hub, "branch.ads_consent")).unwrap();
        assert!(stored.given);
        assert_eq!(stored.at, Some(T0));
        assert_eq!(stored.staff_id.as_deref(), Some(hub.manager_id.as_str()));
        let c = last_change(&hub);
        assert_eq!(c.intent_kind, "ads.consent");
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("staff", hub.manager_id.as_str())
        );
        let hours = format!(
            "[{}]",
            [r#"{"open_min":600,"close_min":1380}"#; 7].join(",")
        );
        let dir = tempfile::tempdir().unwrap();
        for (key, value) in [
            ("branch.opening_hours", hours.as_str()),
            ("system.backup_dir", &dir.path().to_string_lossy()),
        ] {
            let (status, body) = post_intent(&hub, &token, set_body(&hub, key, value, None)).await;
            assert_eq!(status, StatusCode::OK, "{key}: {body}");
        }
        let done = json!({"opening_hours_set": true, "backup_location_set": true, "ad_consent": "given", "ad_rating": "PG", "printer": "unset", "products_count": 0, "groups_without_rate": 0, "manager_exists": true, "test_session": "unset", "live": false});
        setup_is(done.clone()).await;
        let snapshot = SnapshotReader::open(&hub.path)
            .unwrap()
            .snapshot(T0)
            .unwrap();
        assert_eq!(serde_json::to_value(snapshot.setup).unwrap(), done);
        assert_eq!(
            arena_proto::AdsConsent::parse(&setting_value(&hub, "branch.ads_consent"))
                .unwrap()
                .policy_version,
            Some(arena_proto::ADS_POLICY_VERSION)
        );
        let (status, body) = post_intent(&hub, &token, product_body("Cola", None)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let mut with_product = done.clone();
        with_product["products_count"] = json!(1);
        setup_is(with_product).await;
        let id = last_change(&hub).row_id;
        let delete = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::ProductDelete(arena_proto::ProductDelete { id }),
            Some(1),
        ) });
        let (status, body) = post_intent(&hub, &token, delete).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        setup_is(done).await;
        job.abort();
    }

    fn counting_probe(
        delay: std::time::Duration,
    ) -> (BackupProbe, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let probe: BackupProbe = Arc::new(move |_| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(delay);
            Ok(())
        });
        (probe, calls)
    }

    fn with_probe(hub: &mut Hub, probe: BackupProbe, timeout: std::time::Duration) {
        let auth = Arc::new(
            UiAuth::new(
                Arc::new(StaffDirectory::open(&hub.path).unwrap()),
                Arc::new(SnapshotReader::open(&hub.path).unwrap()),
                Some(hub.writer.clone()),
                hub.clock.clone(),
            )
            .with_backup_probe(probe, timeout),
        );
        hub.app = ui_auth_router(Arc::clone(&auth));
        hub.auth = auth;
    }

    #[tokio::test]
    async fn the_probe_runs_only_for_an_authorized_fresh_save_at_the_current_rev() {
        let mut hub = hub().await;
        let (probe, calls) = counting_probe(std::time::Duration::ZERO);
        with_probe(&mut hub, probe, BACKUP_PROBE_TIMEOUT);
        let calls_now = || calls.load(std::sync::atomic::Ordering::SeqCst);
        let path = "/mnt/backup";
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, body) = post_intent(
            &hub,
            &counter,
            set_body(&hub, "system.backup_dir", path, Some("bogus-ticket")),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "PIN_REQUIRED");
        let manager = token_for(&hub, "manager", "m-pass").await;
        let (status, _) = post_intent(
            &hub,
            &manager,
            set_body(&hub, "system.backup_dir", path, Some("bogus-ticket")),
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        let mut stale = set_body(&hub, "system.backup_dir", path, None);
        stale["intent"]["base_rev"] = json!(99);
        let (status, body) = post_intent(&hub, &manager, stale).await;
        assert_eq!(body["code"], "STALE", "{status} {body}");
        let (status, body) = post_intent(
            &hub,
            &manager,
            set_body(&hub, "system.backup_dir", "relative/dir", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["message"], "not_found");
        assert_eq!(
            calls_now(),
            0,
            "no probe before authorization and the rev check"
        );
        let fresh = set_body(&hub, "system.backup_dir", path, None);
        let (status, first) = post_intent(&hub, &manager, fresh.clone()).await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert_eq!(calls_now(), 1);
        let (status, replay) = post_intent(&hub, &manager, fresh).await;
        assert_eq!(status, StatusCode::OK, "{replay}");
        assert_eq!(replay, first, "the replay returns the original result");
        assert_eq!(calls_now(), 1, "a replay never probes");
    }

    struct ScriptedFolder(Option<arena_proto::BackupDirProblem>);

    impl arena_core::setup::BackupFolder for ScriptedFolder {
        fn probe(&self, _location: &str) -> Result<(), arena_proto::BackupDirProblem> {
            self.0.map_or(Ok(()), Err)
        }
    }

    fn with_folder(hub: &mut Hub, folder: Option<ScriptedFolder>) {
        let mut auth = UiAuth::new(
            Arc::new(StaffDirectory::open(&hub.path).unwrap()),
            Arc::new(SnapshotReader::open(&hub.path).unwrap()),
            Some(hub.writer.clone()),
            hub.clock.clone(),
        );
        if let Some(folder) = folder {
            auth = auth.with_backup_folder(Arc::new(folder));
        }
        let auth = Arc::new(auth);
        hub.app = ui_auth_router(Arc::clone(&auth));
        hub.auth = auth;
    }

    const TREE: &str = "content://com.android.externalstorage.documents/tree/1A2B-3C4D%3ABk";

    #[tokio::test]
    async fn a_folder_the_picker_chose_is_saved_after_the_platform_probe() {
        let mut hub = hub().await;
        with_folder(&mut hub, Some(ScriptedFolder(None)));
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "system.backup_dir", TREE, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(setting_value(&hub, "system.backup_dir"), TREE);
    }

    #[tokio::test]
    async fn a_folder_the_platform_cannot_write_is_refused_with_its_reason() {
        for (problem, reason) in [
            (arena_proto::BackupDirProblem::NotWritable, "not_writable"),
            (arena_proto::BackupDirProblem::NotFound, "not_found"),
        ] {
            let mut hub = hub().await;
            with_folder(&mut hub, Some(ScriptedFolder(Some(problem))));
            let token = token_for(&hub, "manager", "m-pass").await;
            let before = change_count(&hub);
            let (status, body) = post_intent(
                &hub,
                &token,
                set_body(&hub, "system.backup_dir", TREE, None),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert_eq!(body["field"], "system.backup_dir");
            assert_eq!(body["message"], reason);
            assert_eq!(change_count(&hub), before);
            assert_eq!(setting_value(&hub, "system.backup_dir"), "");
        }
    }

    #[tokio::test]
    async fn a_picker_folder_is_refused_on_a_hub_with_no_platform_folder() {
        let mut hub = hub().await;
        with_folder(&mut hub, None);
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "system.backup_dir", TREE, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["message"], "not_found");
    }

    #[tokio::test]
    async fn a_counter_changes_the_folder_only_with_a_manager_ticket() {
        let mut hub = hub().await;
        with_folder(&mut hub, Some(ScriptedFolder(None)));
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, body) = post_intent(
            &hub,
            &counter,
            set_body(&hub, "system.backup_dir", TREE, None),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(setting_value(&hub, "system.backup_dir"), "");
    }
    #[tokio::test]
    async fn a_probe_that_hangs_times_out_as_not_writable() {
        let mut hub = hub().await;
        let (probe, calls) = counting_probe(std::time::Duration::from_secs(2));
        with_probe(&mut hub, probe, std::time::Duration::from_millis(100));
        let token = token_for(&hub, "manager", "m-pass").await;
        let before = change_count(&hub);
        let started = std::time::Instant::now();
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "system.backup_dir", "/net/dead/share", None),
        )
        .await;
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "system.backup_dir");
        assert_eq!(body["message"], "not_writable");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(change_count(&hub), before);
    }

    #[tokio::test]
    async fn consent_and_the_hours_flag_are_refused_as_raw_settings() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let before = change_count(&hub);
        for (key, value) in [
            (
                "branch.ads_consent",
                r#"{"given":true,"at":1,"staff_id":"x","policy_version":1}"#,
            ),
            ("branch.opening_hours_set", "true"),
        ] {
            let (status, body) = post_intent(&hub, &token, set_body(&hub, key, value, None)).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{key}: {body}");
            assert_eq!(body["code"], "VALIDATION_FAILED");
            assert_eq!(body["field"], "key");
        }
        let closed = format!("[{}]", [r#"{"closed":true}"#; 7].join(","));
        let (status, body) = post_intent(
            &hub,
            &token,
            set_body(&hub, "branch.opening_hours", &closed, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(body["field"], "branch.opening_hours");
        assert_eq!(change_count(&hub), before);
        assert_eq!(setting_value(&hub, "branch.opening_hours_set"), "false");
    }
}

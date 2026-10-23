//! Staff sign-in for the local Hub UI, on the loopback server (127.0.0.1, same origin guard as
//! `/ui/snapshot`):
//!
//! - `POST /ui/auth/sign-in {login, password}` answers [`SignInOk`], or `AUTH_FAILED` (401) with
//!   one fixed message for every cause. Five consecutive failures for a login (compared without
//!   case) pause it for 60 s: every attempt in that window, right or wrong, gets `AUTH_LOCKED`
//!   (429) with `retry_after_s`. A success resets the count. The lockout is in memory, keyed
//!   by login, and timed by the injected [`Clock`].
//! - `POST /ui/auth/sign-out` (bearer token) ends the session.
//! - `GET /ui/me` (bearer token) answers the session's [`SessionStaff`], or 401.
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
//! - `POST /ui/elevate/drop {ticket}` (bearer token) gives up a ticket's unused approvals (204);
//!   signing out drops the session's tickets.
//! - `POST /ui/intent {intent, elevation_ticket?}` (bearer token) submits the intent with the
//!   session's `actor{staff, staff_id}`, whatever actor the client sent, and drops any
//!   client-supplied `elevated_by`. A ticket is redeemed for one unused approved item matching
//!   the intent (kind, and the key of a `settings.set`; else `PIN_REQUIRED`) and becomes
//!   `elevated_by{pin, staff_id, hub, verifier_version}`, which `apply()` re-checks. A refusal
//!   (nothing applied) gives the item back.
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
    ElevationKind, ElevationMode, ErrorCode, IntentKind, IntentResult, MAX_TICKET_USES, Permission,
    Requirement, SessionStaff, SignInOk, SignInRequest, UiError, UiIntentRequest,
};
use axum::extract::State;
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
/// Unexpired tickets one session may hold; a new one replaces the oldest.
const MAX_TICKETS_PER_SESSION: usize = 4;

/// PIN checks (each up to 50 argon2id verifications) that may run at once.
const MAX_CONCURRENT_PIN_CHECKS: usize = 2;

/// A granted elevation, held only in memory.
#[derive(Debug, Clone)]
struct Ticket {
    /// The session token it was issued to.
    session: String,
    /// The approving staff member.
    staff_id: String,
    verifier_version: i64,
    expires_at: i64,
    /// The approved intents, each redeemed once (`true` once used).
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

#[derive(Debug, Clone)]
struct Session {
    staff: SessionStaff,
    created_at: i64,
}

/// The state behind the auth routes.
pub struct UiAuth {
    staff: Arc<StaffDirectory>,
    reader: Arc<SnapshotReader>,
    writer: Option<WriterHandle>,
    clock: Arc<dyn Clock>,
    lockout: Mutex<Lockout>,
    /// PIN elevation failures, keyed by the requesting staff id (so a new session does not
    /// reset them).
    elevate_lockout: Mutex<Lockout>,
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
}

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
            elevate_lockout: Mutex::new(Lockout::exact()),
            pin_checks: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PIN_CHECKS)),
            tickets: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            ended: tokio::sync::watch::channel(0).0,
            status: None,
            events_config: crate::ui_events::EventsConfig::default(),
            events: std::sync::OnceLock::new(),
        }
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
        let mut sessions = self.sessions.lock().map_err(|_| internal())?;
        match current {
            Some(fresh) => {
                if let Some(s) = sessions.get_mut(&token) {
                    s.staff = fresh.clone();
                }
                Ok(fresh)
            }
            None => {
                sessions.remove(&token);
                drop(sessions);
                self.session_ended();
                Err(auth_failed())
            }
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
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

fn internal() -> UiError {
    UiError {
        code: ErrorCode::Internal,
        message: "the hub could not complete this request".into(),
        field: None,
        retry_after_s: None,
    }
}

fn bad_frame(message: &str) -> UiError {
    UiError {
        code: ErrorCode::BadFrame,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

/// The wire form of an [`AppError`]. An internal error's message may carry database details,
/// so only its code is sent.
fn ui_error(e: AppError) -> UiError {
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
        ErrorCode::AuthLocked => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::PermissionDenied
        | ErrorCode::Forbidden
        | ErrorCode::PinRequired
        | ErrorCode::PinRejected => StatusCode::FORBIDDEN,
        ErrorCode::ReasonRequired => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::BadFrame | ErrorCode::Unsupported => StatusCode::BAD_REQUEST,
        ErrorCode::ValidationFailed | ErrorCode::Expired => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::Stale | ErrorCode::AlreadyBootstrapped | ErrorCode::IdempotencyConflict => {
            StatusCode::CONFLICT
        }
        ErrorCode::NotPrimary | ErrorCode::HubSlow => StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => StatusCode::UNAUTHORIZED,
        ErrorCode::SchemaTooNew | ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn fail(e: UiError) -> Response {
    (status_of(e.code), Json(e)).into_response()
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
    let token = match new_session_token() {
        Ok(t) => t,
        Err(e) => return fail(ui_error(e)),
    };
    let now = auth.clock.now_ms();
    match auth.sessions.lock() {
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
                    auth.session_ended();
                }
            }
            s.insert(
                token.clone(),
                Session {
                    staff: me.clone(),
                    created_at: now,
                },
            );
        }
        Err(_) => return fail(internal()),
    }
    tracing::info!(staff_id = %me.staff_id, "staff signed in");
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
            if removed {
                auth.session_ended();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => fail(internal()),
    }
}

async fn me(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    match auth.session(&headers).await {
        Ok(me) => Json(me).into_response(),
        Err(e) => fail(e),
    }
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

/// The permissions `items` need from an approver (catalogue order, each once), or `None` when
/// one of them is not a permission (it can never be elevated). Checks the items' shape first.
fn item_permissions(items: &[ElevationItem]) -> Result<Option<Vec<Permission>>, UiError> {
    if items.is_empty() || items.len() > MAX_TICKET_USES {
        return Err(invalid_items(&format!(
            "an elevation approves 1–{MAX_TICKET_USES} intents"
        )));
    }
    let mut perms = Vec::new();
    let mut all_perms = true;
    for item in items {
        let kind = item.intent_kind;
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
        match kind.requirement() {
            Requirement::Perm(p) => {
                if !perms.contains(&p) {
                    perms.push(p);
                }
            }
            _ => all_perms = false,
        }
    }
    perms.sort_by_key(|p| Permission::ALL.iter().position(|q| q == p));
    Ok(all_perms.then_some(perms))
}

async fn elevate(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.session_for(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(ElevateRequest { items, pin }) = serde_json::from_slice::<ElevateRequest>(&body) else {
        return fail(bad_frame(
            "the body must be {\"items\": [{\"intent_kind\", \"key\"?}], \"pin\"}",
        ));
    };
    // A follower could never apply what it approved: refuse before any PIN is checked.
    if auth.writer.is_none() {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    let perms = match item_permissions(&items) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    if let Some(perms) = &perms
        && perms.iter().all(|p| me.permissions.contains(p))
    {
        return fail(invalid_items("you can do this without approval"));
    }
    let kinds: Vec<&str> = items.iter().map(|i| i.intent_kind.as_str()).collect();
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
    let now = auth.clock.now_ms();
    match auth.tickets.lock() {
        Ok(mut tickets) => {
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
                ticket.clone(),
                Ticket {
                    session: token,
                    staff_id: approver.clone(),
                    verifier_version,
                    expires_at: now + TICKET_TTL_MS,
                    items: items.iter().cloned().map(|i| (i, false)).collect(),
                },
            );
        }
        Err(_) => return fail(internal()),
    }
    tracing::info!(
        staff_id = %me.staff_id,
        approver = %approver,
        kinds = ?kinds,
        "pin elevation granted"
    );
    Json(ElevateOk {
        ticket,
        items,
        expires_in_s: u32::try_from(TICKET_TTL_MS / 1000).unwrap_or(u32::MAX),
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
        if t.session != token {
            return Err(ui_error(pin_required()));
        }
        let Some(index) = t.items.iter().position(|(item, used)| {
            !used && item.intent_kind == kind && item.key.as_deref() == key
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
    let me = match auth.session_for(&token).await {
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
        id: me.staff_id,
    };
    intent.elevated_by = None;
    // A follower spends no ticket use.
    let Some(writer) = &auth.writer else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    };
    let mut redeemed = None;
    if let Some(ticket) = elevation_ticket {
        if intent.kind != intent.payload.kind() {
            return fail(ui_error(pin_required()));
        }
        match auth.redeem(&ticket, &token, &intent) {
            Ok((elevated, index)) => {
                intent.elevated_by = Some(elevated);
                redeemed = Some((ticket, index));
            }
            Err(e) => return fail(e),
        }
    }
    match writer.submit_intent(intent).await {
        Ok(applied) => Json(IntentResult {
            applied: crate::lan::registry::summary(&applied),
        })
        .into_response(),
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
    Router::new()
        .route("/ui/auth/sign-in", post(sign_in))
        .route("/ui/auth/sign-out", post(sign_out))
        .route("/ui/me", get(me))
        .route("/ui/elevate", post(elevate))
        .route("/ui/elevate/drop", post(elevate_drop))
        .route("/ui/intent", post(intent))
        .route("/ui/settings", get(settings))
        .route("/ui/events", get(crate::ui_events::events))
        .with_state(auth)
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
                    }),
                    None,
                ))
                .await
                .unwrap();
            ids.push(applied.changes[0].row_id.clone());
        }
        let auth = Arc::new(
            UiAuth::new(
                Arc::new(StaffDirectory::open(&path).unwrap()),
                Arc::new(SnapshotReader::open(&path).unwrap()),
                Some(writer.clone()),
                clock.clone(),
            )
            .with_status(status_tx.subscribe())
            .with_events_config(config),
        );
        Hub {
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
        assert_eq!(
            call(&hub, "GET", "/ui/me", Some(token), None).await,
            (StatusCode::OK, staff)
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
            .redeem(
                &ticket,
                &token,
                &serde_json::from_value(currency_set(&hub, "EUR", None)["intent"].clone()).unwrap(),
            )
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
            .redeem(
                &ticket,
                &token,
                &serde_json::from_value(currency_set(&hub, "EUR", None)["intent"].clone()).unwrap(),
            )
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
                    class: arena_proto::ClientClass::Paired,
                    label: "Kiosk 1".into(),
                    token_hash: hash,
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
}

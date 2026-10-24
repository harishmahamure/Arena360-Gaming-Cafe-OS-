//! Process allow/block enforcement in the player's session (Story 1.18).
//!
//! The branch's `kiosk.process_allow` / `kiosk.process_block` lists and `kiosk.process_mode`
//! arrive from the Hub (a `settings.get` after every hello, then `settings.changed` pushes), are
//! cached in `queue.db`'s `kv` table and published on [`Ctx::lists`](crate::runtime::Ctx). The
//! enforcer:
//!
//! - **Watches** process creation ([`ProcessWatcher`]) and, for every new process, decides with
//!   [`arena_kiosk_core::procpolicy::decide`] and kills a disallowed one ([`ProcessKiller`]).
//!   The time from the creation event to the kill is logged (`debug`, `elapsed_ms`). Creation
//!   events go through a bounded channel ([`EVENT_QUEUE`]); when it is full the event is dropped
//!   and a sweep is requested instead. At most [`MAX_IN_FLIGHT`] checks run at once.
//! - **Sweeps** every running process at start, right after the creation watch (re)starts,
//!   within 2 s of a list change, every [`RESWEEP`] as a safety net, and every
//!   [`POLL_FALLBACK`] while the creation watch is down.
//! - **Mode.** In `report` mode nothing is ever killed and no toast is shown; each disallowed
//!   process is reported once (per pid and path) as `kiosk.process_blocked{mode: report}`. With
//!   no mode ever received from the Hub the kiosk reports only (fail safe).
//! - **Tells** the player: an IPC `toast{player.app.blocked}` to the app, but only for a kill
//!   that followed a creation event and whose process had a visible top-level window or was
//!   started within [`TOAST_RECENT`] by a process of the player's session. Sweeps never toast.
//!   Kills within [`TOAST_COLLAPSE`] of the previous toast send none.
//! - **Reports** each kill as a `kiosk.process_blocked` intent minted at kill time through the
//!   offline queue (applied once by the Hub). The same path and reason again within
//!   [`REPORT_COLLAPSE`] is not reported again, and at most [`REPORT_CAP`] reports go out per
//!   minute; the rest are counted in one `kiosk.log{warn, process_blocked_summary, "N more"}` a
//!   minute. A failed kill is warned about once per pid and path and reported once as
//!   `kiosk.log{error, kill_failed}`.
//!
//! **Admin exit (Story 1.20).** While an admin exit runs nothing is checked or killed. When it
//! ends, every `explorer.exe` in the player's session (the exit's desktop) is closed, whatever
//! the mode, under the same safety rules, and a sweep follows at once.
//!
//! **Safety rules** (checked before any decision, in this order; failing any one means the
//! process is left alone):
//!
//! 1. Never the service itself (its own pid), pid 0 or 4.
//! 2. Only a process in the player's session (the active console session whose user is the
//!    configured `player_user`) whose token user is the player's SID. SYSTEM, LocalService,
//!    NetworkService, DWM/UMFD and every other account are never considered, so `csrss`,
//!    `winlogon`, `dwm`, `fontdrvhost` and services are out of reach. A process whose session,
//!    token or image path cannot be read is left alone (warned once per pid and reason).
//! 3. Never the configured kiosk app (its path re-read from the config at every sweep) or the
//!    service image (matched by their full path). In debug builds their signature is not even
//!    checked; in release builds a failed check is logged once and they are still left running.
//! 4. Never a WebView2 process (image file name `msedgewebview2.exe`) that descends from the
//!    kiosk app (parent pids from creation events and snapshots), so the kiosk UI's runtime is
//!    never taken down, wherever it lives. Every other descendant of the app (the app is the
//!    player's shell) is decided by the lists like any process.
//! 5. Never a critical Windows image by full path under `%SystemRoot%\System32` ([`PROTECTED`]),
//!    even if it somehow runs as the player.
//!
//! The kill itself re-checks the process (same image path, user and session) on the handle it
//! terminates, so a reused pid is never hit.
//!
//! Signatures are verified only when the decision depends on them (an allow rule naming a
//! signer matched the path, or a block rule names a signer), cached by `(path, size, mtime)`,
//! and verified once per cache key even when several checks want it at the same time.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use arena_kiosk_core::ipc::TOAST_APP_BLOCKED;
use arena_kiosk_core::procpolicy::{
    Decision, Dirs, ProcInfo, decide, expand_dirs, file_name, normalize_path,
};
use arena_proto::{
    DEFAULT_PROCESS_ALLOW, DEFAULT_PROCESS_BLOCK, KEY_PROCESS_ALLOW, KEY_PROCESS_BLOCK,
    KEY_PROCESS_MODE, KioskLogLevel, KioskProcessBlocked, MAX_EXE_NAME_CHARS,
    MAX_KIOSK_LOG_DETAIL_CHARS, ProcessBlockReason, ProcessList, ProcessMode, ProcessRule,
    SettingEntry, parse_process_mode, parse_process_rules, truncate_path_left,
};
use tokio::sync::{Notify, Semaphore, mpsc, watch};

use crate::queue::QueueDb;
use crate::runtime::Ctx;

pub mod fake;
#[cfg(windows)]
pub mod windows;

/// Kills within this long of the last toast send no new one.
pub const TOAST_COLLAPSE: Duration = Duration::from_secs(1);
/// A killed process started this recently by a player-session process gets a toast.
pub const TOAST_RECENT: Duration = Duration::from_secs(5);
/// The same path and reason within this long is not reported to the Hub again.
pub const REPORT_COLLAPSE: Duration = Duration::from_secs(5);
/// Most `kiosk.process_blocked` reports queued per [`REPORT_WINDOW`].
pub const REPORT_CAP: u32 = 30;
/// The report cap's window.
pub const REPORT_WINDOW: Duration = Duration::from_secs(60);
/// The `kiosk.log` code of the per-minute count of reports over the cap.
pub const SUMMARY_CODE: &str = "process_blocked_summary";
/// The `kiosk.log` code of a failed kill.
pub const KILL_FAILED_CODE: &str = "kill_failed";
/// The safety-net sweep interval while the creation watch runs.
pub const RESWEEP: Duration = Duration::from_secs(10);
/// The sweep interval while the creation watch is down.
pub const POLL_FALLBACK: Duration = Duration::from_millis(500);
/// How long a failed creation watch waits before it is started again.
pub const RESUBSCRIBE: Duration = Duration::from_secs(30);
/// Signature results kept at most (the cache is cleared when it fills).
pub const SIG_CACHE_CAP: usize = 4096;
/// Creation events buffered at most; past this a sweep is requested instead.
pub const EVENT_QUEUE: usize = 1024;
/// Process checks running at once at most.
pub const MAX_IN_FLIGHT: usize = 8;
/// The only image name exempt as a descendant of the kiosk app (its WebView2 runtime).
pub const WEBVIEW2_IMAGE: &str = "msedgewebview2.exe";
/// The admin exit's desktop, closed in the player's session when the exit ends.
pub const EXPLORER_IMAGE: &str = "explorer.exe";
/// How far up the parent chain the kiosk-app exemption looks.
const MAX_ANCESTRY: usize = 32;
/// Entries kept at most in each warn-once set (cleared when it fills).
const WARN_ONCE_CAP: usize = 4096;

/// Critical Windows images (under `%SystemRoot%\System32`) never killed.
pub const PROTECTED: &[&str] = &[
    "csrss.exe",
    "winlogon.exe",
    "wininit.exe",
    "smss.exe",
    "services.exe",
    "lsass.exe",
    "dwm.exe",
    "LogonUI.exe",
    "fontdrvhost.exe",
    "svchost.exe",
    "consent.exe",
    "userinit.exe",
    "sihost.exe",
    "ctfmon.exe",
];

// ---- the platform ----

/// A process-creation event.
#[derive(Debug, Clone, Copy)]
pub struct ProcStart {
    pub pid: u32,
    /// The creating process, when the event carries it.
    pub parent_pid: Option<u32>,
    /// When the event was received (the kill latency is measured from here).
    pub at: Instant,
}

/// One running process in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub parent_pid: Option<u32>,
}

/// The player's logged-on session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSession {
    /// The player account's SID (`S-1-5-21-…`).
    pub sid: String,
    pub session_id: u32,
}

/// What the enforcer needs to know about a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcFacts {
    pub pid: u32,
    pub session_id: u32,
    /// The process token's user SID.
    pub user_sid: String,
    /// The image's full path (`QueryFullProcessImageNameW`), not yet normalised.
    pub path: String,
}

/// What inspecting a process found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inspect {
    Facts(ProcFacts),
    /// The process no longer exists.
    Gone,
    /// It exists, but `reason` (`open`, `session`, `token`, `path`) could not be read.
    Unreadable {
        /// Its session, when that much was read.
        session_id: Option<u32>,
        reason: &'static str,
    },
}

/// A file's identity for the signature cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub size: u64,
    pub mtime: i64,
}

/// An Authenticode result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Signature {
    /// The signer's subject CN (of the file, or of the catalog that signs it).
    pub signer: Option<String>,
    /// Whether `WinVerifyTrust` accepted the signature.
    pub valid: bool,
}

/// Where a creation watch sends its events: a bounded queue. When it is full the event is
/// dropped and a sweep is requested, so nothing is missed for long and a burst cannot grow
/// memory without bound.
pub struct EventSink {
    tx: mpsc::Sender<ProcStart>,
    sweep: Arc<Notify>,
    overflowing: AtomicBool,
}

impl EventSink {
    /// A sink into `tx` that wakes `sweep` on overflow.
    pub fn new(tx: mpsc::Sender<ProcStart>, sweep: Arc<Notify>) -> Self {
        Self {
            tx,
            sweep,
            overflowing: AtomicBool::new(false),
        }
    }

    /// Sends one event. Returns `false` once the watch is no longer wanted (the receiver is
    /// gone).
    pub fn send(&self, ev: ProcStart) -> bool {
        match self.tx.try_send(ev) {
            Ok(()) => {
                self.overflowing.store(false, Ordering::Relaxed);
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                if !self.overflowing.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        capacity = EVENT_QUEUE,
                        "process creation events are arriving faster than they are checked; sweeping instead"
                    );
                }
                self.sweep.notify_one();
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Whether the watch is no longer wanted.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Process creation and inspection.
pub trait ProcessWatcher: Send + Sync {
    /// Starts sending creation events to `sink` from the watcher's own thread. The watch has
    /// ended when `sink` is dropped.
    fn subscribe(&self, sink: EventSink) -> io::Result<()>;
    /// The processes running now, with their parents.
    fn running(&self) -> io::Result<Vec<ProcEntry>>;
    /// The player's session, when `account` is logged on at the console.
    fn player_session(&self, account: &str) -> Option<PlayerSession>;
    /// `pid`'s session, token user and image path.
    fn inspect(&self, pid: u32) -> Inspect;
    /// Whether `pid` owns a visible top-level window. The default (and the Windows service,
    /// which runs in session 0 and cannot see the player's desktop) says no.
    fn has_visible_window(&self, _pid: u32) -> bool {
        false
    }
}

/// Authenticode checks.
pub trait SignatureVerifier: Send + Sync {
    /// The file's size and modification time, for the cache.
    fn stamp(&self, path: &str) -> Option<FileStamp>;
    /// The file's signature: embedded, or through the system catalogs.
    fn verify(&self, path: &str) -> Signature;
}

/// Terminating a process.
pub trait ProcessKiller: Send + Sync {
    /// Terminates `facts.pid`, but only while it is still the process `facts` describes (same
    /// image path, token user and session), so a reused pid is never hit. A process that is
    /// gone or changed is `NotFound`.
    fn kill(&self, facts: &ProcFacts) -> io::Result<()>;
}

/// The three platform halves.
#[derive(Clone)]
pub struct ProcessApi {
    pub watcher: Arc<dyn ProcessWatcher>,
    pub verifier: Arc<dyn SignatureVerifier>,
    pub killer: Arc<dyn ProcessKiller>,
}

impl ProcessApi {
    /// One object playing all three parts (the fakes, the Windows implementation).
    pub fn of<T>(all: Arc<T>) -> Self
    where
        T: ProcessWatcher + SignatureVerifier + ProcessKiller + 'static,
    {
        Self {
            watcher: Arc::clone(&all) as Arc<dyn ProcessWatcher>,
            verifier: Arc::clone(&all) as Arc<dyn SignatureVerifier>,
            killer: all as Arc<dyn ProcessKiller>,
        }
    }
}

// ---- the lists ----

/// The branch's two lists and mode, as the Hub last sent them (or the defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessLists {
    pub allow: Vec<ProcessRule>,
    pub block: Vec<ProcessRule>,
    pub mode: ProcessMode,
}

fn defaults_of(list: ProcessList) -> Vec<ProcessRule> {
    let text = match list {
        ProcessList::Allow => DEFAULT_PROCESS_ALLOW,
        ProcessList::Block => DEFAULT_PROCESS_BLOCK,
    };
    parse_process_rules(list, text).unwrap_or_default()
}

fn key_of(list: ProcessList) -> &'static str {
    match list {
        ProcessList::Allow => KEY_PROCESS_ALLOW,
        ProcessList::Block => KEY_PROCESS_BLOCK,
    }
}

/// The mode a kiosk uses until the Hub has told it one: report only (nothing is killed on a
/// kiosk that never heard from its Hub, or whose cache is damaged).
pub const UNKNOWN_MODE: ProcessMode = ProcessMode::Report;

/// The setting keys the kiosk reads.
pub const PROCESS_KEYS: [&str; 3] = [KEY_PROCESS_ALLOW, KEY_PROCESS_BLOCK, KEY_PROCESS_MODE];

impl ProcessLists {
    /// The lists every branch starts with, in [`UNKNOWN_MODE`].
    pub fn defaults() -> Self {
        Self {
            allow: defaults_of(ProcessList::Allow),
            block: defaults_of(ProcessList::Block),
            mode: UNKNOWN_MODE,
        }
    }

    /// The cached lists and mode in `queue.db`, each falling back to the default when it was
    /// never received (or the cached copy no longer parses).
    pub fn load(queue: &QueueDb) -> Self {
        let cached = |key: &str| match queue.kv_get(key) {
            Ok(v) => v.map(|(value, _)| value),
            Err(e) => {
                tracing::error!(key, error = %e, "the process list cache cannot be read; using the default");
                None
            }
        };
        let get = |list: ProcessList| {
            let key = key_of(list);
            match cached(key).map(|v| parse_process_rules(list, &v)) {
                Some(Ok(rules)) => rules,
                Some(Err(e)) => {
                    tracing::error!(key, error = %e, "the cached process list is invalid; using the default");
                    defaults_of(list)
                }
                None => defaults_of(list),
            }
        };
        let mode = match cached(KEY_PROCESS_MODE).map(|v| parse_process_mode(&v)) {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                tracing::error!(error = %e, "the cached process mode is invalid; reporting only");
                UNKNOWN_MODE
            }
            None => UNKNOWN_MODE,
        };
        Self {
            allow: get(ProcessList::Allow),
            block: get(ProcessList::Block),
            mode,
        }
    }

    /// One of the two lists.
    pub fn get(&self, list: ProcessList) -> &[ProcessRule] {
        match list {
            ProcessList::Allow => &self.allow,
            ProcessList::Block => &self.block,
        }
    }

    fn get_mut(&mut self, list: ProcessList) -> &mut Vec<ProcessRule> {
        match list {
            ProcessList::Allow => &mut self.allow,
            ProcessList::Block => &mut self.block,
        }
    }
}

/// The revisions applied so far, so an older revision never overwrites a newer one (a
/// `settings.get` reply overtaken by a push, or a lagging Hub). Seeded from the cache at every
/// connect.
#[derive(Debug, Default)]
pub struct AppliedRevs(HashMap<String, i64>);

impl AppliedRevs {
    /// The revisions cached in `queue.db`.
    pub fn from_cache(queue: &QueueDb) -> Self {
        let mut revs = HashMap::new();
        for key in PROCESS_KEYS {
            match queue.kv_get(key) {
                Ok(Some((_, rev))) => {
                    revs.insert(key.to_owned(), rev);
                }
                Ok(None) => {}
                Err(e) => tracing::error!(key, error = %e, "the process list cache cannot be read"),
            }
        }
        Self(revs)
    }

    /// The revision applied for `key`, if any.
    pub fn get(&self, key: &str) -> Option<i64> {
        self.0.get(key).copied()
    }
}

enum Parsed {
    List(ProcessList, Vec<ProcessRule>),
    Mode(ProcessMode),
}

/// Applies setting values from the Hub: each valid process list or mode is cached and, when it
/// differs, published (which triggers a sweep). A value older than the one applied is ignored;
/// an invalid value is refused with an `error` line and the previous one stays. Other keys are
/// ignored.
pub fn apply_settings(ctx: &Ctx, values: &[SettingEntry], revs: &mut AppliedRevs) {
    for entry in values {
        let parsed = if entry.key == KEY_PROCESS_MODE {
            parse_process_mode(&entry.value).map(Parsed::Mode)
        } else if let Some(list) = ProcessList::of_key(&entry.key) {
            parse_process_rules(list, &entry.value).map(|r| Parsed::List(list, r))
        } else {
            continue;
        };
        if revs.get(&entry.key).is_some_and(|r| entry.rev < r) {
            tracing::warn!(key = %entry.key, rev = entry.rev, applied = revs.get(&entry.key), "ignoring an older process setting");
            continue;
        }
        let parsed = match parsed {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(key = %entry.key, rev = entry.rev, error = %e, "the hub sent an invalid process setting; keeping the previous one");
                continue;
            }
        };
        revs.0.insert(entry.key.clone(), entry.rev);
        if let Err(e) = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
            &entry.key,
            &entry.value,
            entry.rev,
        ) {
            tracing::error!(key = %entry.key, error = %e, "could not cache the process setting");
        }
        ctx.lists.send_if_modified(|current| {
            let mut next = (**current).clone();
            match &parsed {
                Parsed::List(list, rules) => {
                    if current.get(*list) == rules.as_slice() {
                        return false;
                    }
                    *next.get_mut(*list) = rules.clone();
                    tracing::info!(key = %entry.key, rev = entry.rev, rules = rules.len(), "process list updated");
                }
                Parsed::Mode(mode) => {
                    if current.mode == *mode {
                        return false;
                    }
                    next.mode = *mode;
                    tracing::info!(rev = entry.rev, mode = mode.as_str(), "process enforcement mode updated");
                }
            }
            *current = Arc::new(next);
            true
        });
    }
}

// ---- the report cap ----

/// The global per-minute cap on `kiosk.process_blocked` reports.
#[derive(Debug, Default)]
pub struct ReportCap {
    window: Option<Instant>,
    sent: u32,
    suppressed: u32,
}

impl ReportCap {
    /// Ends a window that is over: the count of reports it held back, if any (to go out as one
    /// summary).
    pub fn roll(&mut self, now: Instant) -> Option<u32> {
        let w = self.window?;
        if now.duration_since(w) < REPORT_WINDOW {
            return None;
        }
        let held = std::mem::take(&mut self.suppressed);
        self.window = None;
        self.sent = 0;
        (held > 0).then_some(held)
    }

    /// Whether one more report may go out now, and the summary of a window that just ended.
    pub fn admit(&mut self, now: Instant) -> (bool, Option<u32>) {
        let summary = self.roll(now);
        self.window.get_or_insert(now);
        if self.sent < REPORT_CAP {
            self.sent += 1;
            (true, summary)
        } else {
            self.suppressed += 1;
            (false, summary)
        }
    }
}

// ---- the enforcer ----

/// Where this install lives.
#[derive(Debug, Clone, Default)]
pub struct EnforceOptions {
    /// What `{install}` expands to.
    pub install_dir: Option<PathBuf>,
    /// The kiosk app image: never killed, and neither are its WebView2 descendants.
    pub app_image: Option<PathBuf>,
    /// When set, the app image is re-read from this `config.json` (`app_path`) at every sweep.
    pub app_config: Option<PathBuf>,
    /// Other images never killed (the service).
    pub own_images: Vec<PathBuf>,
    /// `%SystemRoot%` (default `C:\Windows`): what `{system}` expands to, and the root of
    /// [`PROTECTED`].
    pub system_root: String,
}

impl EnforceOptions {
    /// This install: the service's own directory and image, and the configured app.
    pub fn for_this_install(config: &Path) -> Self {
        let exe = std::env::current_exe().ok();
        Self {
            install_dir: exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)),
            app_image: Some(crate::launcher::configured_app_path(config)),
            app_config: Some(config.to_path_buf()),
            own_images: exe.into_iter().collect(),
            system_root: std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()),
        }
    }
}

/// A disallowed process the enforcer found (and, in enforce mode, killed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub pid: u32,
    /// The normalised image path.
    pub path: String,
    pub reason: ProcessBlockReason,
    pub mode: ProcessMode,
    /// Whether the player should see a toast (a kill after a creation event, of a process
    /// with a visible window or started just now by the player's session).
    pub toast: bool,
}

type SigKey = (String, FileStamp);

#[derive(Default)]
struct Reported {
    last_toast: Option<Instant>,
    recent: HashMap<(String, ProcessBlockReason), Instant>,
    /// Report mode: the live processes already reported (pid, lowercase path).
    seen: HashSet<(u32, String)>,
    cap: ReportCap,
}

#[derive(Default)]
struct WarnOnce<K>(HashSet<K>);

impl<K: std::hash::Hash + Eq> WarnOnce<K> {
    /// `true` the first time `k` is seen.
    fn first(&mut self, k: K) -> bool {
        if self.0.len() >= WARN_ONCE_CAP {
            self.0.clear();
        }
        self.0.insert(k)
    }
}

struct Inner {
    ctx: Arc<Ctx>,
    api: ProcessApi,
    install_dir: Option<String>,
    system_root: String,
    /// The kiosk app's folded path.
    app: Mutex<Option<String>>,
    app_config: Option<PathBuf>,
    own: Vec<String>,
    protected: Vec<String>,
    /// pid → (parent pid, when it was learned).
    parents: Mutex<HashMap<u32, (Option<u32>, Instant)>>,
    sigs: Mutex<HashMap<SigKey, Arc<OnceLock<Signature>>>>,
    reported: Mutex<Reported>,
    warned_own: Mutex<HashSet<String>>,
    warned_unreadable: Mutex<WarnOnce<(u32, &'static str)>>,
    warned_kill: Mutex<WarnOnce<(u32, String)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn folded(path: &str) -> String {
    normalize_path(path).to_lowercase()
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

impl Inner {
    fn new(ctx: Arc<Ctx>, api: ProcessApi, opts: EnforceOptions) -> Self {
        let root = normalize_path(&opts.system_root);
        Self {
            ctx,
            api,
            install_dir: opts.install_dir.map(|d| d.display().to_string()),
            system_root: root.clone(),
            app: Mutex::new(opts.app_image.map(|p| folded(&p.display().to_string()))),
            app_config: opts.app_config,
            own: opts
                .own_images
                .iter()
                .map(|p| folded(&p.display().to_string()))
                .collect(),
            protected: PROTECTED
                .iter()
                .map(|n| format!(r"{root}\System32\{n}").to_lowercase())
                .collect(),
            parents: Mutex::default(),
            sigs: Mutex::default(),
            reported: Mutex::default(),
            warned_own: Mutex::default(),
            warned_unreadable: Mutex::default(),
            warned_kill: Mutex::default(),
        }
    }

    fn player_account(&self) -> String {
        self.ctx
            .config()
            .map(|c| c.player_user)
            .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
    }

    /// Re-reads the configured app path (a re-provision may change it).
    fn refresh_app(&self) {
        if let Some(config) = &self.app_config {
            let path = crate::launcher::configured_app_path(config);
            *lock(&self.app) = Some(folded(&path.display().to_string()));
        }
    }

    fn note_parent(&self, pid: u32, parent: Option<u32>) {
        lock(&self.parents).insert(pid, (parent, Instant::now()));
    }

    /// Replaces the parent table with a snapshot taken at `taken`, keeping what creation events
    /// added since.
    fn note_snapshot(&self, entries: &[ProcEntry], taken: Instant) {
        let live: HashSet<u32> = entries.iter().map(|e| e.pid).collect();
        let mut parents = lock(&self.parents);
        parents.retain(|pid, (_, at)| live.contains(pid) || *at >= taken);
        for e in entries {
            parents.insert(e.pid, (e.parent_pid, taken));
        }
    }

    fn parent_of(&self, pid: u32) -> Option<u32> {
        lock(&self.parents)
            .get(&pid)
            .and_then(|(p, _)| *p)
            .filter(|p| *p != pid && *p != 0)
    }

    fn player_facts(&self, pid: u32, player: &PlayerSession) -> Option<ProcFacts> {
        match self.api.watcher.inspect(pid) {
            Inspect::Facts(f)
                if f.session_id == player.session_id
                    && f.user_sid.eq_ignore_ascii_case(&player.sid) =>
            {
                Some(f)
            }
            _ => None,
        }
    }

    /// Whether `pid` descends from the running kiosk app (in the player's session).
    fn is_app_descendant(&self, pid: u32, player: &PlayerSession) -> bool {
        let Some(app) = lock(&self.app).clone() else {
            return false;
        };
        let mut cur = pid;
        for _ in 0..MAX_ANCESTRY {
            let Some(parent) = self.parent_of(cur) else {
                return false;
            };
            if self
                .player_facts(parent, player)
                .is_some_and(|f| folded(&f.path) == app)
            {
                return true;
            }
            cur = parent;
        }
        false
    }

    /// Whether `pid` was started by a process of the player's session.
    fn started_by_player(&self, pid: u32, player: &PlayerSession) -> bool {
        self.parent_of(pid)
            .is_some_and(|p| self.player_facts(p, player).is_some())
    }

    /// The signature of `path`, verified once per cache key (concurrent callers wait for the
    /// first one's result).
    fn signature(&self, path: &str) -> Signature {
        let Some(stamp) = self.api.verifier.stamp(path) else {
            return self.api.verifier.verify(path);
        };
        let key = (path.to_lowercase(), stamp);
        let cell = {
            let mut sigs = lock(&self.sigs);
            if sigs.len() >= SIG_CACHE_CAP && !sigs.contains_key(&key) {
                sigs.clear();
            }
            Arc::clone(sigs.entry(key).or_default())
        };
        cell.get_or_init(|| self.api.verifier.verify(path)).clone()
    }

    /// The full decision for `path`, verifying the signature only when it matters.
    fn decision(&self, path: &str, lists: &ProcessLists) -> Decision {
        let dirs = Dirs {
            install: self.install_dir.as_deref(),
            system: Some(&self.system_root),
        };
        let allow = expand_dirs(&lists.allow, dirs);
        let block = expand_dirs(&lists.block, dirs);
        let unsigned = ProcInfo {
            path: path.to_owned(),
            signer: None,
            valid_sig: false,
        };
        let quick = decide(&unsigned, &allow, &block);
        let signer_blocks = block.iter().any(|r| r.signer.is_some());
        let needs_signature = match quick {
            // A block match without a signer: the signature changes nothing.
            Decision::Kill(ProcessBlockReason::Blocked) => false,
            // A signed allow rule matched the path.
            Decision::Kill(ProcessBlockReason::Signature) => true,
            // Only a signer-scoped block rule could still change the outcome or reason.
            Decision::Kill(ProcessBlockReason::NotAllowed) | Decision::Allow => signer_blocks,
        };
        if !needs_signature {
            return quick;
        }
        let sig = self.signature(path);
        decide(
            &ProcInfo {
                path: path.to_owned(),
                signer: sig.signer,
                valid_sig: sig.valid,
            },
            &allow,
            &block,
        )
    }

    /// Checks one process and, when it is in the player's session and not allowed, kills it
    /// (enforce mode) or notes it (report mode). `event` is the creation event that led here
    /// (sweeps pass none and never toast).
    fn check(
        &self,
        pid: u32,
        player: &PlayerSession,
        lists: &ProcessLists,
        event: Option<&ProcStart>,
    ) -> Option<Hit> {
        if pid == 0 || pid == 4 || pid == std::process::id() || self.ctx.admin.suspended() {
            return None;
        }
        let facts = match self.api.watcher.inspect(pid) {
            Inspect::Facts(f) => f,
            Inspect::Gone => return None,
            Inspect::Unreadable { session_id, reason } => {
                if session_id.is_none_or(|s| s == player.session_id)
                    && lock(&self.warned_unreadable).first((pid, reason))
                {
                    tracing::warn!(
                        pid,
                        reason,
                        "cannot read a process's details; leaving it alone"
                    );
                }
                return None;
            }
        };
        if facts.session_id != player.session_id
            || !facts.user_sid.eq_ignore_ascii_case(&player.sid)
        {
            return None;
        }
        let path = normalize_path(&facts.path);
        if path.is_empty() {
            return None;
        }
        let lower = path.to_lowercase();
        let is_app = lock(&self.app).as_ref() == Some(&lower);
        if is_app || self.own.contains(&lower) {
            if !cfg!(debug_assertions)
                && self.decision(&path, lists) != Decision::Allow
                && lock(&self.warned_own).insert(lower)
            {
                tracing::warn!(path = %path, "the kiosk's own image fails the process policy (signature?); it is never killed");
            }
            return None;
        }
        if self.protected.contains(&lower) {
            return None;
        }
        let Decision::Kill(reason) = self.decision(&path, lists) else {
            return None;
        };
        if file_name(&path).eq_ignore_ascii_case(WEBVIEW2_IMAGE)
            && self.is_app_descendant(pid, player)
        {
            tracing::debug!(pid, path = %path, "the kiosk app's WebView2 is never killed");
            return None;
        }
        let mode = lists.mode;
        if mode == ProcessMode::Report {
            if !lock(&self.reported).seen.insert((pid, lower)) {
                return None;
            }
            return Some(Hit {
                pid,
                path,
                reason,
                mode,
                toast: false,
            });
        }
        let toast = event.is_some_and(|ev| {
            self.api.watcher.has_visible_window(pid)
                || (ev.at.elapsed() <= TOAST_RECENT && self.started_by_player(pid, player))
        });
        match self.api.killer.kill(&facts) {
            Ok(()) => Some(Hit {
                pid,
                path,
                reason,
                mode,
                toast,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(pid, path = %path, error = %e, "the process went away before the kill");
                None
            }
            Err(e) => {
                if lock(&self.warned_kill).first((pid, lower)) {
                    tracing::warn!(pid, path = %path, error = %e, "could not kill a disallowed process");
                    let detail = truncate_path_left(
                        &clean(&format!("pid {pid}: {e}: {path}")),
                        MAX_KIOSK_LOG_DETAIL_CHARS,
                    );
                    if let Err(e) =
                        self.ctx
                            .log_event(KioskLogLevel::Error, KILL_FAILED_CODE, &detail)
                    {
                        tracing::error!(error = %e, "could not queue kill_failed");
                    }
                }
                None
            }
        }
    }

    /// Closes every process in the player's session whose image file name is `name` (the
    /// admin exit's Explorer), never the app, the service or a protected image. Returns how many.
    fn close_named(&self, name: &str) -> usize {
        self.refresh_app();
        let Some(player) = self.player() else {
            return 0;
        };
        let Ok(entries) = self.api.watcher.running() else {
            return 0;
        };
        let app = lock(&self.app).clone();
        let mut closed = 0;
        for e in entries {
            if e.pid == 0 || e.pid == 4 || e.pid == std::process::id() {
                continue;
            }
            let Some(facts) = self.player_facts(e.pid, &player) else {
                continue;
            };
            let path = normalize_path(&facts.path);
            let lower = path.to_lowercase();
            if !file_name(&path).eq_ignore_ascii_case(name)
                || app.as_ref() == Some(&lower)
                || self.own.contains(&lower)
                || self.protected.contains(&lower)
            {
                continue;
            }
            match self.api.killer.kill(&facts) {
                Ok(()) => {
                    closed += 1;
                    tracing::info!(pid = e.pid, path = %path, "admin exit over: closed the desktop");
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => {
                    tracing::warn!(pid = e.pid, error = %err, "could not close the desktop")
                }
            }
        }
        closed
    }

    fn player(&self) -> Option<PlayerSession> {
        self.api.watcher.player_session(&self.player_account())
    }

    /// Checks every running process. Returns what it found.
    fn sweep(&self) -> Vec<Hit> {
        self.refresh_app();
        let Some(player) = self.player() else {
            return Vec::new();
        };
        let lists = Arc::clone(&self.ctx.lists.borrow());
        let taken = Instant::now();
        let entries = match self.api.watcher.running() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "could not list processes");
                return Vec::new();
            }
        };
        self.note_snapshot(&entries, taken);
        {
            let live: HashSet<u32> = entries.iter().map(|e| e.pid).collect();
            lock(&self.reported)
                .seen
                .retain(|(pid, _)| live.contains(pid));
        }
        entries
            .iter()
            .filter_map(|e| self.check(e.pid, &player, &lists, None))
            .collect()
    }

    fn device_id(&self) -> String {
        self.ctx
            .config()
            .map(|c| c.device_id)
            .unwrap_or_else(|| "unprovisioned".into())
    }

    fn summarize(&self, held: u32) {
        tracing::warn!(
            held,
            "kiosk.process_blocked reports over the per-minute cap"
        );
        if let Err(e) =
            self.ctx
                .log_event(KioskLogLevel::Warn, SUMMARY_CODE, &format!("{held} more"))
        {
            tracing::error!(error = %e, "could not queue the process_blocked summary");
        }
    }

    /// Sends the summary of a report window that has ended.
    fn flush_summary(&self) {
        let held = lock(&self.reported).cap.roll(Instant::now());
        if let Some(n) = held {
            self.summarize(n);
        }
    }

    /// Toast (collapsed) and report (deduplicated and capped) one hit.
    fn report(&self, k: &Hit) {
        let now = Instant::now();
        let (toast, report, summary) = {
            let mut r = lock(&self.reported);
            let toast = k.toast
                && k.mode == ProcessMode::Enforce
                && r.last_toast
                    .is_none_or(|t| now.duration_since(t) >= TOAST_COLLAPSE);
            if toast {
                r.last_toast = Some(now);
            }
            r.recent
                .retain(|_, at| now.duration_since(*at) < REPORT_COLLAPSE);
            let key = (k.path.to_lowercase(), k.reason);
            let fresh = match r.recent.entry(key) {
                std::collections::hash_map::Entry::Occupied(_) => false,
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(now);
                    true
                }
            };
            let (report, summary) = if fresh {
                r.cap.admit(now)
            } else {
                (false, r.cap.roll(now))
            };
            (toast, report, summary)
        };
        if toast {
            let _ = self.ctx.toasts.send(TOAST_APP_BLOCKED.to_owned());
        }
        if let Some(n) = summary {
            self.summarize(n);
        }
        if !report {
            return;
        }
        let exe_name: String = clean(file_name(&k.path))
            .chars()
            .take(MAX_EXE_NAME_CHARS)
            .collect();
        let mut payload = KioskProcessBlocked {
            exe_name,
            path: clean(&k.path),
            reason: k.reason,
            mode: k.mode,
        };
        // Short enough that the stored detail keeps the whole (left-shortened) path.
        let head = KioskProcessBlocked {
            path: String::new(),
            ..payload.clone()
        }
        .detail()
        .chars()
        .count();
        payload.path = truncate_path_left(
            &payload.path,
            MAX_KIOSK_LOG_DETAIL_CHARS.saturating_sub(head).max(1),
        );
        if let Err(e) = payload.validate() {
            tracing::warn!(error = %e, "not reporting a process the hub would refuse");
            return;
        }
        let intent = crate::intents::kiosk_process_blocked(
            &self.device_id(),
            self.ctx.clock.now_ms(),
            payload,
        );
        match lock(&self.ctx.queue).push(&intent) {
            Ok(()) => self.ctx.queued.notify_one(),
            Err(e) => tracing::error!(error = %e, "could not queue kiosk.process_blocked"),
        }
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

fn log_hit(k: &Hit, elapsed_ms: Option<u64>) {
    match (k.mode, elapsed_ms) {
        (ProcessMode::Report, _) => {
            tracing::info!(pid = k.pid, path = %k.path, reason = k.reason.as_str(), "report mode: a disallowed process left running");
        }
        (ProcessMode::Enforce, Some(elapsed_ms)) => {
            tracing::debug!(pid = k.pid, elapsed_ms, "process creation to kill");
            tracing::info!(pid = k.pid, path = %k.path, reason = k.reason.as_str(), elapsed_ms, "killed a disallowed process");
        }
        (ProcessMode::Enforce, None) => {
            tracing::info!(pid = k.pid, path = %k.path, reason = k.reason.as_str(), "sweep killed a disallowed process");
        }
    }
}

async fn handle_start(inner: Arc<Inner>, ev: ProcStart) {
    let i = Arc::clone(&inner);
    let hit = blocking(move || {
        let player = i.player()?;
        let lists = Arc::clone(&i.ctx.lists.borrow());
        i.check(ev.pid, &player, &lists, Some(&ev))
    })
    .await
    .flatten();
    if let Some(k) = hit {
        log_hit(&k, Some(ev.at.elapsed().as_millis() as u64));
        inner.report(&k);
    }
}

async fn sweeper(inner: Arc<Inner>, now: Arc<Notify>, polling: watch::Receiver<bool>) {
    loop {
        let started = Instant::now();
        let i = Arc::clone(&inner);
        let hits = blocking(move || i.sweep()).await.unwrap_or_default();
        for k in &hits {
            log_hit(k, None);
            inner.report(k);
        }
        inner.flush_summary();
        tracing::debug!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            hits = hits.len(),
            "process sweep"
        );
        let wait = if *polling.borrow() {
            POLL_FALLBACK
        } else {
            RESWEEP
        };
        tokio::select! {
            () = now.notified() => {}
            () = tokio::time::sleep(wait) => {}
        }
    }
}

async fn subscribe(
    inner: &Inner,
    polling: &watch::Sender<bool>,
    sweep_now: &Arc<Notify>,
) -> Option<mpsc::Receiver<ProcStart>> {
    let (tx, rx) = mpsc::channel(EVENT_QUEUE);
    let sink = EventSink::new(tx, Arc::clone(sweep_now));
    let w = Arc::clone(&inner.api.watcher);
    let result = blocking(move || w.subscribe(sink)).await;
    // Sweep either way: right after a (re)subscribe it covers what started in between.
    sweep_now.notify_one();
    match result {
        Some(Ok(())) => {
            polling.send_replace(false);
            tracing::info!("watching process creation");
            Some(rx)
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "cannot watch process creation; polling instead");
            polling.send_replace(true);
            None
        }
        None => {
            polling.send_replace(true);
            None
        }
    }
}

/// Runs the enforcer until `stop` is set.
pub(crate) async fn run(
    ctx: Arc<Ctx>,
    api: ProcessApi,
    opts: EnforceOptions,
    mut stop: watch::Receiver<bool>,
) {
    if *stop.borrow_and_update() {
        return;
    }
    let inner = Arc::new(Inner::new(Arc::clone(&ctx), api, opts));
    let sweep_now = Arc::new(Notify::new());
    let checks = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let (polling_tx, polling) = watch::channel(false);
    let sweeps = tokio::spawn(sweeper(Arc::clone(&inner), Arc::clone(&sweep_now), polling));
    let mut lists = ctx.lists.subscribe();
    lists.mark_unchanged();
    let mut exits = ctx.admin.active.subscribe();
    exits.mark_unchanged();
    let mut rx = subscribe(&inner, &polling_tx, &sweep_now).await;
    let mut retry_at = tokio::time::Instant::now() + RESUBSCRIBE;
    'run: loop {
        tokio::select! {
            ev = async {
                match rx.as_mut() {
                    Some(r) => r.recv().await,
                    None => std::future::pending().await,
                }
            } => match ev {
                Some(ev) => {
                    inner.note_parent(ev.pid, ev.parent_pid);
                    let permit = tokio::select! {
                        p = Arc::clone(&checks).acquire_owned() => match p {
                            Ok(p) => p,
                            Err(_) => break 'run,
                        },
                        _ = stop.changed() => break 'run,
                    };
                    let i = Arc::clone(&inner);
                    tokio::spawn(async move {
                        let _permit = permit;
                        handle_start(i, ev).await;
                    });
                }
                None => {
                    tracing::error!("the process creation watch ended; polling until it restarts");
                    rx = None;
                    polling_tx.send_replace(true);
                    sweep_now.notify_one();
                    retry_at = tokio::time::Instant::now() + RESUBSCRIBE;
                }
            },
            () = tokio::time::sleep_until(retry_at), if rx.is_none() => {
                rx = subscribe(&inner, &polling_tx, &sweep_now).await;
                retry_at = tokio::time::Instant::now() + RESUBSCRIBE;
            }
            changed = exits.changed() => {
                if changed.is_err() {
                    break;
                }
                if exits.borrow_and_update().is_none() {
                    let i = Arc::clone(&inner);
                    blocking(move || i.close_named(EXPLORER_IMAGE)).await;
                    sweep_now.notify_one();
                }
            }
            changed = lists.changed() => {
                if changed.is_err() {
                    break;
                }
                lists.mark_unchanged();
                sweep_now.notify_one();
            }
            _ = stop.changed() => break,
        }
    }
    sweeps.abort();
}

#[cfg(test)]
mod tests;

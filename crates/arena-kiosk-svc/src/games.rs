//! The Games tab (Story 6.23): what the player's library holds on this PC and starting one.
//!
//! - `games` lists the enabled library games this PC has ([`crate::library::load_visible`],
//!   games only: the Apps tab is Story 6.28), sorted by name, each with whether it is running.
//!   The answer also says whether any game is running, which is the shell's "game running"
//!   signal (the app asks again every few seconds while a session is on screen).
//! - `game_launch{id}` starts a game. The id must be in this PC's library, and what would start
//!   (the install path recorded for this PC, else the executable path, else the launch URI) must
//!   pass the same allow-list the process enforcer applies, before anything starts. The game
//!   starts in the player's session through the [`GameHost`]. A game that is already running is
//!   not started a second time: the answer is `already_running{pid}` and the app brings its
//!   window to the front. A start queues a `game.launched` intent for the Hub (player and session
//!   are the Hub's to name); a refused or failed start queues a `kiosk.log{error,
//!   game_launch_failed}` and answers `failed`.
//! - **Apps (Story 6.28).** `apps` lists the library's `kind: app` entries this PC has, the same
//!   way, and `app_launch{id}` starts one through the same `spec_of` and allow-list check as a
//!   game, but with no booster, no play tracking, no `game.launched` and nothing in
//!   [`GameState::any_running`]: an app is not a game. A start queues a `kiosk.log{info,
//!   app_launched}` (detail `<name> (<app id>)`) for the Hub. An app that is already open (one
//!   this service started) is brought forward, not started twice. A refused or failed start
//!   queues `kiosk.log{error, game_launch_failed}` and answers `failed`; the id of a game is not
//!   an app.
//! - A game started from a launcher URI hands over to the launcher, so what is tracked is the
//!   process the service started (the launcher's URI handler), not the game it goes on to start.
//!
//! - **The booster (Story 6.25).** Before a game starts, the programs on the branch's close list
//!   (`kiosk.booster_close`) are closed in the player's session through the process enforcer's
//!   kill path (the app, the service and protected images are never touched); with
//!   `kiosk.booster_power_plan` on, the active power plan is remembered (queue kv
//!   `games.power_restore`, so a restart still restores it) and High performance is switched on;
//!   once the game has started its priority is set (`kiosk.booster_priority`). The session-end
//!   cleanup restores the remembered plan ([`restore_power_plan`], its `power_plan` step). A
//!   program that will not close, or a plan or priority that will not change, is logged (a
//!   `kiosk.log` warning for the Hub) and the game still starts.
//! - **Launch time (Story 6.25).** From the click, the service watches for the started process's
//!   first window; the `game.launched` record is queued once it has appeared (`launch_ms`), or
//!   with none when the process exits first or [`LAUNCH_WINDOW_TIMEOUT_MS`] passes. The record's
//!   time is still the click. A game started from a launcher link is measured by the launcher's
//!   window, as it is tracked by the launcher's process.
//!
//! - **Play hours (Story 6.27).** Each game this service starts is tracked with the time it
//!   started. When its process is seen to have exited (the run loop checks every [`POLL`], and a
//!   launch or a list check does too), or the session ends (the cleanup's `programs` step calls
//!   [`record_running`] before any program is ended), one `game.play_recorded{game_id,
//!   started_at, ended_at, minutes}` is queued for the Hub and the game is no longer tracked, so
//!   a play is recorded once. It is queued like any kiosk event, so a Hub that is unreachable gets
//!   it once on reconnect. The Hub names the player and session. A game started from a launcher
//!   link is tracked by the launcher's handler process, so its recorded minutes may be short
//!   (the same limit as launch time).
//!
//! Nothing here is written without a session on screen, and nothing is started while the PC is
//! locked.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_kiosk_core::procpolicy::{Decision, ProcInfo, decide, normalize_path};
use arena_proto::{
    AppError, Booster, BoosterPriority, ErrorCode, KioskGameLaunched, KioskGamePlayRecorded,
    KioskLibraryEntry, KioskLogLevel, LAUNCH_WINDOW_TIMEOUT_MS, MAX_PLAY_MINUTES, is_launch_uri,
    launcher_install_globs,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::procs::ProcessLists;
use crate::runtime::Ctx;

/// How often the running games are checked for having exited.
pub const POLL: Duration = Duration::from_secs(2);
/// How often a started game is checked for its first window.
pub const WINDOW_POLL: Duration = Duration::from_millis(250);
/// The `kiosk.log` code of a game that would not start.
pub const FAILED_CODE: &str = "game_launch_failed";
/// The `kiosk.log` code of an app that was started (Story 6.28).
pub const APP_LAUNCHED_CODE: &str = "app_launched";
/// The `kiosk.log` code of a booster step that did not work (the game started anyway).
pub const BOOSTER_CODE: &str = "booster_failed";
/// The id of Windows' High performance power plan.
pub const HIGH_PERFORMANCE_PLAN: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
/// The queue kv key holding the power plan to restore at session end.
const POWER_RESTORE_KEY: &str = "games.power_restore";
/// The longest `kiosk.log` detail of a failed launch, in characters.
const DETAIL_MAX: usize = 300;

/// What to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The executable's full path, when the game starts from one.
    pub exe: Option<String>,
    /// The entry's launch arguments (with `exe` only).
    pub args: String,
    /// The launch URI (`steam://...`), when there is no executable to start.
    pub uri: Option<String>,
}

/// The PC side of starting games: Windows starts them in the player's session, tests fake it.
pub trait GameHost: Send + Sync {
    /// Starts `spec` as the player and answers the id of the process started.
    fn start(&self, spec: &LaunchSpec) -> Result<u32, String>;
    /// Whether process `pid` is still running.
    fn alive(&self, pid: u32) -> bool;
    /// Sets the priority of process `pid` (Story 6.25).
    fn set_priority(&self, pid: u32, priority: BoosterPriority) -> Result<(), String>;
    /// Whether process `pid` has shown a main window in the player's session (Story 6.25).
    fn has_window(&self, pid: u32) -> bool;
    /// The id of the active power plan (Story 6.25).
    fn power_plan(&self) -> Result<String, String>;
    /// Makes the power plan `id` the active one (Story 6.25).
    fn set_power_plan(&self, id: &str) -> Result<(), String>;
    /// The id of a running process whose image file name is one of `images` (Story 6.26: a
    /// launcher that is already open). The default finds none.
    fn find_process(&self, _images: &[&str]) -> Option<u32> {
        None
    }
}

/// A game this service started that is being timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tracked {
    pid: u32,
    /// When it started, on the wall clock (Unix milliseconds).
    started_at: i64,
}

/// The games this service started that have not exited yet.
#[derive(Default)]
pub struct GameState {
    host: Mutex<Option<Arc<dyn GameHost>>>,
    running: Mutex<HashMap<String, Tracked>>,
    /// Games whose start is in progress, so two quick requests cannot start one twice.
    starting: Mutex<HashSet<String>>,
    /// The launchers (Story 6.26) this service opened that have not exited yet, by launcher id.
    /// Apart from `running`: an open launcher is not a game running.
    launchers: Mutex<HashMap<String, u32>>,
    /// Launchers whose open is in progress.
    opening: Mutex<HashSet<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl GameState {
    /// Sets the host that starts games (without one, nothing can be started).
    pub fn set_host(&self, host: Arc<dyn GameHost>) {
        *lock(&self.host) = Some(host);
    }

    pub(crate) fn host(&self) -> Option<Arc<dyn GameHost>> {
        lock(&self.host).clone()
    }

    /// The process of game `id` when it is running. A game that has exited stays tracked until
    /// [`reap`] records its play.
    fn running_pid(&self, id: &str, host: &dyn GameHost) -> Option<u32> {
        let pid = lock(&self.running).get(id)?.pid;
        host.alive(pid).then_some(pid)
    }

    /// Claims the right to start game `id`; `false` when another start of it is in progress.
    fn begin(&self, id: &str) -> bool {
        lock(&self.starting).insert(id.to_owned())
    }

    fn end(&self, id: &str) {
        lock(&self.starting).remove(id);
    }

    /// Starts timing game `id`; answers the play it replaces, if one was left unrecorded.
    pub(crate) fn started(&self, id: &str, pid: u32, started_at: i64) -> Option<(String, i64)> {
        lock(&self.running)
            .insert(id.to_owned(), Tracked { pid, started_at })
            .map(|old| (id.to_owned(), old.started_at))
    }

    /// The process of launcher `id` when this service opened it and it is still running.
    pub(crate) fn launcher_pid(&self, id: &str, host: &dyn GameHost) -> Option<u32> {
        let pid = lock(&self.launchers).get(id).copied()?;
        if host.alive(pid) {
            Some(pid)
        } else {
            lock(&self.launchers).remove(id);
            None
        }
    }

    pub(crate) fn begin_launcher(&self, id: &str) -> bool {
        lock(&self.opening).insert(id.to_owned())
    }

    pub(crate) fn end_launcher(&self, id: &str) {
        lock(&self.opening).remove(id);
    }

    pub(crate) fn launcher_started(&self, id: &str, pid: u32) {
        lock(&self.launchers).insert(id.to_owned(), pid);
    }

    /// Stops timing the games that have exited; answers them as `(game id, started_at)` and how
    /// many are still running.
    fn take_exited(&self, host: &dyn GameHost) -> (Vec<(String, i64)>, usize) {
        let mut running = lock(&self.running);
        let mut gone: Vec<(String, i64)> = running
            .iter()
            .filter(|(_, t)| !host.alive(t.pid))
            .map(|(id, t)| (id.clone(), t.started_at))
            .collect();
        for (id, _) in &gone {
            running.remove(id);
        }
        gone.sort();
        (gone, running.len())
    }

    /// Stops timing every game, running or not; answers them as `(game id, started_at)`.
    fn take_all(&self) -> Vec<(String, i64)> {
        let mut all: Vec<(String, i64)> = lock(&self.running)
            .drain()
            .map(|(id, t)| (id, t.started_at))
            .collect();
        all.sort();
        all
    }

    /// Whether any game this service started is still running.
    pub fn any_running(&self) -> bool {
        match self.host() {
            Some(host) => lock(&self.running).values().any(|t| host.alive(t.pid)),
            None => false,
        }
    }
}

/// One tile of the Games tab. The category and tags feed the tab's filter rail (Story 6.24); the
/// category (the genre), launcher and age rating are the Home hero's "Game details" (Story 6.29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub category: String,
    pub tags: Vec<String>,
    pub launcher: String,
    pub age_rating: String,
}

/// The entries of `kind` (`game` or `app`) this PC lists, sorted by name (case-insensitive, then
/// id).
pub fn cards(
    entries: Vec<KioskLibraryEntry>,
    kind: &str,
    running: impl Fn(&str) -> bool,
) -> Vec<Card> {
    let mut cards: Vec<Card> = entries
        .into_iter()
        .filter(|e| e.kind == kind)
        .map(|e| Card {
            running: running(&e.id),
            id: e.id,
            name: e.name,
            category: e.category,
            tags: e.tags,
            launcher: e.launcher,
            age_rating: e.age_rating,
        })
        .collect();
    cards.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    cards
}

/// What starts the game on this PC: its install path here, else its executable path when that
/// exists, else its launch URI. `None` when it has none of them.
pub fn spec_of(
    entry: &KioskLibraryEntry,
    device_id: &str,
    exists: impl Fn(&str) -> bool,
) -> Option<LaunchSpec> {
    let installed = entry
        .installs
        .iter()
        .find(|i| i.device_id == device_id && !i.path.is_empty())
        .map(|i| i.path.clone());
    let exe = installed.or_else(|| {
        (!entry.executable_path.is_empty() && exists(&entry.executable_path))
            .then(|| entry.executable_path.clone())
    });
    if exe.is_some() {
        return Some(LaunchSpec {
            exe,
            args: entry.launch_args.clone(),
            uri: None,
        });
    }
    // A quote would end the handler's quoted argument early.
    (is_launch_uri(&entry.launch_uri) && !entry.launch_uri.contains('"')).then(|| LaunchSpec {
        exe: None,
        args: String::new(),
        uri: Some(entry.launch_uri.clone()),
    })
}

/// Whether the allow-list lets `spec` start: an executable must be allowed (and not blocked) by
/// path, as the process enforcer would decide when it appears; a URI needs its launcher's
/// install rules in the allow-list, because the launcher is what starts.
pub fn allowed(lists: &ProcessLists, entry: &KioskLibraryEntry, spec: &LaunchSpec) -> bool {
    if let Some(exe) = &spec.exe {
        let proc = ProcInfo {
            path: normalize_path(exe),
            signer: None,
            valid_sig: false,
        };
        return decide(&proc, &lists.allow, &lists.block) == Decision::Allow;
    }
    let globs = launcher_install_globs(&entry.launcher);
    !globs.is_empty()
        && globs.iter().all(|g| {
            lists
                .allow
                .iter()
                .any(|r| r.signer.is_none() && r.path_glob.eq_ignore_ascii_case(g))
        })
}

fn device_id(ctx: &Ctx) -> Result<String, AppError> {
    ctx.config()
        .map(|c| c.device_id)
        .ok_or_else(|| AppError::new(ErrorCode::Forbidden, "this PC is not paired"))
}

fn on_screen(ctx: &Ctx) -> bool {
    let screen = ctx.pc.screen.borrow();
    screen.session.is_some() && !screen.locked
}

/// The `games` request: `{games: [{id, name, running}], running}`.
pub fn list(ctx: &Ctx) -> Result<Value, AppError> {
    if !on_screen(ctx) {
        return Ok(json!({ "games": [], "running": false }));
    }
    let device = device_id(ctx)?;
    let entries = {
        let queue = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        crate::library::load_visible(&queue, &device)
    };
    Ok(list_of(ctx, entries))
}

/// [`list`] for the library `entries` this PC has.
fn list_of(ctx: &Ctx, entries: Vec<KioskLibraryEntry>) -> Value {
    let running = reap(ctx) > 0;
    let host = ctx.games.host();
    let is_running = |id: &str| {
        host.as_deref()
            .is_some_and(|h| ctx.games.running_pid(id, h).is_some())
    };
    json!({ "games": cards(entries, "game", is_running), "running": running })
}

/// The `apps` request: `{apps: [{id, name, running, category, tags}]}`; empty with no session on
/// screen or while locked (Story 6.28).
pub fn list_apps(ctx: &Ctx) -> Result<Value, AppError> {
    if !on_screen(ctx) {
        return Ok(json!({ "apps": [] }));
    }
    let device = device_id(ctx)?;
    let entries = {
        let queue = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        crate::library::load_visible(&queue, &device)
    };
    Ok(apps_of(ctx, entries))
}

/// [`list_apps`] for the library `entries` this PC has.
fn apps_of(ctx: &Ctx, entries: Vec<KioskLibraryEntry>) -> Value {
    let host = ctx.games.host();
    let is_open = |id: &str| {
        host.as_deref()
            .is_some_and(|h| ctx.games.launcher_pid(id, h).is_some())
    };
    json!({ "apps": cards(entries, "app", is_open) })
}

/// Logs the refusal or failure for the Hub and answers `failed`.
fn failed(ctx: &Ctx, what: &str, why: &str) -> Value {
    let detail: String = format!("{what}: {why}").chars().take(DETAIL_MAX).collect();
    tracing::warn!(%detail, "a game would not start");
    if let Err(e) = ctx.log_event(KioskLogLevel::Error, FAILED_CODE, &detail) {
        tracing::error!(error = %e, "the failed launch could not be queued");
    }
    json!({ "outcome": "failed" })
}

/// The `game_launch` request: `{id}`; the answer is `{outcome: started | already_running | failed,
/// pid?}`.
pub async fn launch(ctx: &Arc<Ctx>, body: Value) -> Result<Value, AppError> {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::BadFrame, "game_launch needs an id"))?
        .to_owned();
    if ctx.games.host().is_none() {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "this PC cannot start games",
        ));
    }
    if !on_screen(ctx) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            "games start only in a session",
        ));
    }
    let device = device_id(ctx)?;
    let entries = {
        let queue = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        crate::library::load_visible(&queue, &device)
    };
    launch_in(ctx, &device, entries, id).await
}

/// [`launch`] of game `id` among the library `entries` this PC has, as device `device`.
async fn launch_in(
    ctx: &Arc<Ctx>,
    device: &str,
    entries: Vec<KioskLibraryEntry>,
    id: String,
) -> Result<Value, AppError> {
    let host = ctx
        .games
        .host()
        .ok_or_else(|| AppError::new(ErrorCode::Unsupported, "this PC cannot start games"))?;
    let Some(entry) = entries.into_iter().find(|e| e.id == id && e.kind == "game") else {
        return Ok(failed(ctx, &id, "not in this PC's library"));
    };
    // A game that exited since the last check is recorded before it can be started again.
    reap(ctx);
    if let Some(pid) = ctx.games.running_pid(&entry.id, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    if !ctx.games.begin(&entry.id) {
        // Another request is starting this game right now (a double click): not an error.
        return Ok(json!({ "outcome": "started" }));
    }
    let result = start_entry(ctx, device, &entry, &host).await;
    ctx.games.end(&entry.id);
    result
}

/// The `app_launch` request: `{id}`; the answer is `{outcome: started | already_running | failed,
/// pid?}` (Story 6.28).
pub async fn launch_app(ctx: &Arc<Ctx>, body: Value) -> Result<Value, AppError> {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::BadFrame, "app_launch needs an id"))?
        .to_owned();
    if ctx.games.host().is_none() {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "this PC cannot start apps",
        ));
    }
    if !on_screen(ctx) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            "apps start only in a session",
        ));
    }
    let device = device_id(ctx)?;
    let entries = {
        let queue = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        crate::library::load_visible(&queue, &device)
    };
    launch_app_in(ctx, &device, entries, id).await
}

/// [`launch_app`] of app `id` among the library `entries` this PC has, as device `device`. An
/// app is tracked only to bring it forward (in the launcher maps: library ids cannot collide with
/// `steam|epic|riot`), never as a game.
async fn launch_app_in(
    ctx: &Arc<Ctx>,
    device: &str,
    entries: Vec<KioskLibraryEntry>,
    id: String,
) -> Result<Value, AppError> {
    let host = ctx
        .games
        .host()
        .ok_or_else(|| AppError::new(ErrorCode::Unsupported, "this PC cannot start apps"))?;
    let Some(entry) = entries.into_iter().find(|e| e.id == id && e.kind == "app") else {
        return Ok(failed(ctx, &id, "not in this PC's apps"));
    };
    if let Some(pid) = ctx.games.launcher_pid(&entry.id, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    if !ctx.games.begin_launcher(&entry.id) {
        // Another request is starting this app right now (a double click): not an error.
        return Ok(json!({ "outcome": "started" }));
    }
    let result = start_app(ctx, device, &entry, &host).await;
    ctx.games.end_launcher(&entry.id);
    result
}

/// Checks and starts app `entry` (the caller holds its start claim).
async fn start_app(
    ctx: &Arc<Ctx>,
    device: &str,
    entry: &KioskLibraryEntry,
    host: &Arc<dyn GameHost>,
) -> Result<Value, AppError> {
    // Seen again under the claim: a start that finished meanwhile is open now.
    if let Some(pid) = ctx.games.launcher_pid(&entry.id, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    let Some(spec) = spec_of(entry, device, crate::library::path_exists) else {
        return Ok(failed(ctx, &entry.name, "nothing to start on this PC"));
    };
    let lists = Arc::clone(&ctx.lists.borrow());
    if !allowed(&lists, entry, &spec) {
        return Ok(failed(ctx, &entry.name, "not allowed on this PC"));
    }
    let start_host = Arc::clone(host);
    let started = tokio::task::spawn_blocking(move || start_host.start(&spec))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return Ok(failed(ctx, &entry.name, &why)),
    };
    ctx.games.launcher_started(&entry.id, pid);
    tracing::info!(app = %entry.id, pid, "an app started");
    let detail: String = format!("{} ({})", entry.name, entry.id)
        .chars()
        .take(DETAIL_MAX)
        .collect();
    if let Err(e) = ctx.log_event(KioskLogLevel::Info, APP_LAUNCHED_CODE, &detail) {
        tracing::error!(error = %e, "the app launch could not be queued");
    }
    Ok(json!({ "outcome": "started" }))
}

/// Checks and starts `entry` (the caller holds its start claim).
async fn start_entry(
    ctx: &Arc<Ctx>,
    device: &str,
    entry: &KioskLibraryEntry,
    host: &Arc<dyn GameHost>,
) -> Result<Value, AppError> {
    // Seen again under the claim: a start that finished meanwhile is running now.
    if let Some(pid) = ctx.games.running_pid(&entry.id, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    let Some(spec) = spec_of(entry, device, crate::library::path_exists) else {
        return Ok(failed(ctx, &entry.name, "nothing to start on this PC"));
    };
    let lists = Arc::clone(&ctx.lists.borrow());
    if !allowed(&lists, entry, &spec) {
        return Ok(failed(ctx, &entry.name, "not allowed on this PC"));
    }
    let clicked = tokio::time::Instant::now();
    let clicked_ms = ctx.clock.now_ms();
    let booster =
        crate::operating::load(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner())).booster;
    boost_before(ctx, host, &booster).await;
    let start_host = Arc::clone(host);
    let started = tokio::task::spawn_blocking(move || start_host.start(&spec))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return Ok(failed(ctx, &entry.name, &why)),
    };
    if let Some(old) = ctx.games.started(&entry.id, pid, ctx.clock.now_ms()) {
        record_plays(ctx, vec![old]);
    }
    if booster.priority != BoosterPriority::Normal
        && let Err(why) = host.set_priority(pid, booster.priority)
    {
        booster_failed(ctx, &format!("priority: {why}"));
    }
    tracing::info!(game = %entry.id, pid, "a game started");
    tokio::spawn(record_launch(
        Arc::clone(ctx),
        Arc::clone(host),
        Launch {
            device: device.to_owned(),
            game_id: entry.id.clone(),
            name: entry.name.clone(),
            pid,
            clicked,
            clicked_ms,
        },
    ));
    Ok(json!({ "outcome": "started" }))
}

/// Logs a booster step that did not work, for the Hub and the service log. The game still starts.
fn booster_failed(ctx: &Ctx, what: &str) {
    let detail: String = what.chars().take(DETAIL_MAX).collect();
    tracing::warn!(%detail, "a booster step did not work");
    if let Err(e) = ctx.log_event(KioskLogLevel::Warn, BOOSTER_CODE, &detail) {
        tracing::error!(error = %e, "the booster failure could not be queued");
    }
}

/// What runs before the game starts: closes the listed programs and raises the power plan. A
/// failure of either is logged and does not stop the game.
async fn boost_before(ctx: &Arc<Ctx>, host: &Arc<dyn GameHost>, booster: &Booster) {
    if !booster.close.is_empty() {
        let close_ctx = Arc::clone(ctx);
        let names = booster.close.clone();
        let closed = tokio::task::spawn_blocking(move || {
            let images: Vec<&str> = names.iter().map(String::as_str).collect();
            crate::cleanup::close(&close_ctx, crate::procs::CloseScope::Images(&images))
        })
        .await;
        match closed {
            Ok(Ok(report)) if report.failed == 0 => {
                tracing::info!(
                    closed = report.closed,
                    "the booster closed the listed programs"
                );
            }
            Ok(Ok(report)) => booster_failed(
                ctx,
                &format!("{} listed program(s) would not close", report.failed),
            ),
            Ok(Err(why)) => booster_failed(ctx, &format!("close: {why}")),
            Err(e) => booster_failed(ctx, &format!("close: {e}")),
        }
    }
    if booster.high_performance {
        let (ctx, host) = (Arc::clone(ctx), Arc::clone(host));
        let raised = tokio::task::spawn_blocking(move || {
            raise_power_plan(&ctx, host.as_ref()).map_err(|why| (ctx, why))
        })
        .await;
        if let Ok(Err((ctx, why))) = raised {
            booster_failed(&ctx, &format!("power plan: {why}"));
        }
    }
}

fn kv_get(ctx: &Ctx, key: &str) -> Option<String> {
    match ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .kv_get(key)
    {
        Ok(Some((v, _))) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Stores `value` (empty clears it).
fn kv_set(ctx: &Ctx, key: &str, value: &str) -> Result<(), String> {
    ctx.queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .kv_set(key, value, 0)
        .map_err(|e| e.to_string())
}

/// Switches to High performance, first remembering the plan to go back to. A plan already
/// remembered (an earlier launch in this session raised it) is kept: the first one is the
/// original.
fn raise_power_plan(ctx: &Ctx, host: &dyn GameHost) -> Result<(), String> {
    if kv_get(ctx, POWER_RESTORE_KEY).is_some() {
        return Ok(());
    }
    let current = host.power_plan()?;
    if current.eq_ignore_ascii_case(HIGH_PERFORMANCE_PLAN) {
        return Ok(());
    }
    kv_set(ctx, POWER_RESTORE_KEY, &current)?;
    if let Err(why) = host.set_power_plan(HIGH_PERFORMANCE_PLAN) {
        let _ = kv_set(ctx, POWER_RESTORE_KEY, "");
        return Err(why);
    }
    Ok(())
}

/// Puts back the power plan a launch replaced (the cleanup's `power_plan` step); nothing when
/// none was. A failure keeps the memory, so the next cleanup tries again.
pub fn restore_power_plan(ctx: &Ctx) -> Result<(), String> {
    let Some(original) = kv_get(ctx, POWER_RESTORE_KEY) else {
        return Ok(());
    };
    let Some(host) = ctx.games.host() else {
        return Err("this PC cannot change its power plan".into());
    };
    host.set_power_plan(&original)?;
    kv_set(ctx, POWER_RESTORE_KEY, "")
}

/// A started game whose launch is to be recorded.
struct Launch {
    device: String,
    game_id: String,
    name: String,
    pid: u32,
    clicked: tokio::time::Instant,
    clicked_ms: i64,
}

/// The milliseconds from `clicked` to the first window of process `pid`; `None` when the process
/// exits first or `timeout` passes with no window.
async fn first_window_ms(
    host: &dyn GameHost,
    pid: u32,
    clicked: tokio::time::Instant,
    poll: Duration,
    timeout: Duration,
) -> Option<u32> {
    loop {
        let elapsed = clicked.elapsed();
        if host.has_window(pid) {
            return Some(u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX));
        }
        if elapsed >= timeout || !host.alive(pid) {
            return None;
        }
        tokio::time::sleep(poll).await;
    }
}

/// Waits for the game's first window, then queues `game.launched` (with the time, or without).
async fn record_launch(ctx: Arc<Ctx>, host: Arc<dyn GameHost>, launch: Launch) {
    let launch_ms = first_window_ms(
        host.as_ref(),
        launch.pid,
        launch.clicked,
        WINDOW_POLL,
        Duration::from_millis(u64::from(LAUNCH_WINDOW_TIMEOUT_MS)),
    )
    .await
    .map(|ms| ms.min(LAUNCH_WINDOW_TIMEOUT_MS));
    let intent = crate::intents::game_launched(
        &launch.device,
        launch.clicked_ms,
        KioskGameLaunched {
            game_id: launch.game_id,
            name: launch.name,
            launch_ms,
        },
    );
    let queued = ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(&intent);
    match queued {
        Ok(()) => ctx.queued.notify_one(),
        Err(e) => tracing::error!(error = %e, "the game launch could not be queued"),
    }
}

/// Stops timing the games that have exited, queueing each one's play; answers how many games
/// are still running.
pub fn reap(ctx: &Ctx) -> usize {
    let Some(host) = ctx.games.host() else {
        return 0;
    };
    let (gone, running) = ctx.games.take_exited(host.as_ref());
    record_plays(ctx, gone);
    running
}

/// The session is ending: first records the games already seen to have exited (at the time they
/// were noticed), then queues the play of every game still being timed with the end now, and stops
/// timing them (so a later process exit records nothing). Called before any program is ended.
pub fn record_running(ctx: &Ctx) {
    reap(ctx);
    record_plays(ctx, ctx.games.take_all());
}

/// The `game.play_recorded` of a play from `started_at` to `ended_at` on the wall clock: whole
/// minutes to the nearest, never negative and never over the Hub's limit (the start is pulled
/// forward instead).
fn play_of(game_id: &str, started_at: i64, ended_at: i64) -> KioskGamePlayRecorded {
    let max_ms = MAX_PLAY_MINUTES * 60_000;
    let span = (ended_at - started_at).clamp(0, max_ms);
    KioskGamePlayRecorded {
        game_id: game_id.to_owned(),
        started_at: ended_at - span,
        ended_at,
        minutes: (span + 30_000) / 60_000,
    }
}

/// Queues one `game.play_recorded` for each of `plays` (`(game id, started_at)`), ended now.
fn record_plays(ctx: &Ctx, plays: Vec<(String, i64)>) {
    if plays.is_empty() {
        return;
    }
    // As for any kiosk event: the Hub binds the intent to the paired device it arrives on.
    let device = ctx
        .config()
        .map_or_else(|| "unprovisioned".into(), |c| c.device_id);
    let ended_at = ctx.clock.now_ms();
    for (game_id, started_at) in plays {
        let play = play_of(&game_id, started_at, ended_at);
        if let Err(e) = play.validate() {
            tracing::error!(%game_id, error = %e, "not recording a play the hub would refuse");
            continue;
        }
        tracing::info!(%game_id, minutes = play.minutes, "a game stopped; recording its play");
        let intent = crate::intents::game_play_recorded(&device, ended_at, play);
        let queued = ctx
            .queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(&intent);
        match queued {
            Ok(()) => ctx.queued.notify_one(),
            Err(e) => tracing::error!(error = %e, "the game play could not be queued"),
        }
    }
}

/// Keeps the running games current until `stop` is set.
pub async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(POLL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                reap(&ctx);
            }
            _ = stop.changed() => return,
        }
    }
}

/// A host that starts nothing real: process ids are handed out in order and a test decides
/// which have exited and which have shown a window.
pub mod fake {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use arena_proto::BoosterPriority;

    use super::{GameHost, LaunchSpec};

    /// The plan a fake PC starts on (Windows' Balanced).
    pub const BALANCED_PLAN: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";

    pub struct FakeGames {
        inner: Mutex<Inner>,
    }

    struct Inner {
        next: u32,
        alive: HashSet<u32>,
        windows: HashSet<u32>,
        started: Vec<LaunchSpec>,
        fail: Option<String>,
        priorities: Vec<(u32, BoosterPriority)>,
        priority_fail: Option<String>,
        plan: String,
        plan_sets: Vec<String>,
        plan_fail: Option<String>,
        outside: Vec<(String, u32)>,
    }

    impl Default for FakeGames {
        fn default() -> Self {
            Self {
                inner: Mutex::new(Inner {
                    next: 0,
                    alive: HashSet::new(),
                    windows: HashSet::new(),
                    started: Vec::new(),
                    fail: None,
                    priorities: Vec::new(),
                    priority_fail: None,
                    plan: BALANCED_PLAN.into(),
                    plan_sets: Vec::new(),
                    plan_fail: None,
                    outside: Vec::new(),
                }),
            }
        }
    }

    impl FakeGames {
        /// Makes every later start fail with `why` (`None`: succeed again).
        pub fn fail_with(&self, why: Option<&str>) {
            self.inner.lock().unwrap().fail = why.map(str::to_owned);
        }

        /// What was started, in order.
        pub fn started(&self) -> Vec<LaunchSpec> {
            self.inner.lock().unwrap().started.clone()
        }

        /// Ends process `pid`.
        pub fn exit(&self, pid: u32) {
            self.inner.lock().unwrap().alive.remove(&pid);
        }

        /// Shows a main window for process `pid`.
        pub fn show_window(&self, pid: u32) {
            self.inner.lock().unwrap().windows.insert(pid);
        }

        /// The process ids still running.
        pub fn alive_pids(&self) -> Vec<u32> {
            let mut v: Vec<u32> = self.inner.lock().unwrap().alive.iter().copied().collect();
            v.sort_unstable();
            v
        }

        /// The priorities set, in order.
        pub fn priorities(&self) -> Vec<(u32, BoosterPriority)> {
            self.inner.lock().unwrap().priorities.clone()
        }

        /// Makes every later priority change fail with `why` (`None`: succeed again).
        pub fn fail_priority_with(&self, why: Option<&str>) {
            self.inner.lock().unwrap().priority_fail = why.map(str::to_owned);
        }

        /// The active power plan.
        pub fn plan(&self) -> String {
            self.inner.lock().unwrap().plan.clone()
        }

        /// Makes the PC run on power plan `id`.
        pub fn set_plan(&self, id: &str) {
            self.inner.lock().unwrap().plan = id.to_owned();
        }

        /// Makes a process with image `image` (any of the names asked for) appear as running with
        /// id `pid`, as if started outside this service.
        pub fn show_process(&self, image: &str, pid: u32) {
            self.inner
                .lock()
                .unwrap()
                .outside
                .push((image.to_owned(), pid));
        }

        /// Every plan switched to, in order.
        pub fn plan_sets(&self) -> Vec<String> {
            self.inner.lock().unwrap().plan_sets.clone()
        }

        /// Makes every later plan switch fail with `why` (`None`: succeed again).
        pub fn fail_plan_with(&self, why: Option<&str>) {
            self.inner.lock().unwrap().plan_fail = why.map(str::to_owned);
        }
    }

    impl GameHost for FakeGames {
        fn start(&self, spec: &LaunchSpec) -> Result<u32, String> {
            let mut inner = self.inner.lock().unwrap();
            if let Some(why) = &inner.fail {
                return Err(why.clone());
            }
            inner.next += 1;
            let pid = 1000 + inner.next;
            inner.alive.insert(pid);
            inner.started.push(spec.clone());
            Ok(pid)
        }

        fn alive(&self, pid: u32) -> bool {
            self.inner.lock().unwrap().alive.contains(&pid)
        }

        fn set_priority(&self, pid: u32, priority: BoosterPriority) -> Result<(), String> {
            let mut inner = self.inner.lock().unwrap();
            if let Some(why) = &inner.priority_fail {
                return Err(why.clone());
            }
            inner.priorities.push((pid, priority));
            Ok(())
        }

        fn has_window(&self, pid: u32) -> bool {
            self.inner.lock().unwrap().windows.contains(&pid)
        }

        fn power_plan(&self) -> Result<String, String> {
            Ok(self.inner.lock().unwrap().plan.clone())
        }

        fn set_power_plan(&self, id: &str) -> Result<(), String> {
            let mut inner = self.inner.lock().unwrap();
            if let Some(why) = &inner.plan_fail {
                return Err(why.clone());
            }
            inner.plan = id.to_owned();
            inner.plan_sets.push(id.to_owned());
            Ok(())
        }

        fn find_process(&self, images: &[&str]) -> Option<u32> {
            let inner = self.inner.lock().unwrap();
            inner
                .outside
                .iter()
                .find(|(image, _)| images.iter().any(|i| i.eq_ignore_ascii_case(image)))
                .map(|(_, pid)| *pid)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

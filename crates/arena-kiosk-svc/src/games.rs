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
//! - A game started from a launcher URI hands over to the launcher, so what is tracked is the
//!   process the service started (the launcher's URI handler), not the game it goes on to start.
//!
//! Nothing here is written without a session on screen, and nothing is started while the PC is
//! locked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_kiosk_core::procpolicy::{Decision, ProcInfo, decide, normalize_path};
use arena_proto::{
    AppError, ErrorCode, KioskGameLaunched, KioskLibraryEntry, KioskLogLevel, is_launch_uri,
    launcher_install_globs,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::procs::ProcessLists;
use crate::runtime::Ctx;

/// How often the running games are checked for having exited.
pub const POLL: Duration = Duration::from_secs(2);
/// The `kiosk.log` code of a game that would not start.
pub const FAILED_CODE: &str = "game_launch_failed";
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
}

/// The games this service started that have not exited yet.
#[derive(Default)]
pub struct GameState {
    host: Mutex<Option<Arc<dyn GameHost>>>,
    running: Mutex<HashMap<String, u32>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl GameState {
    /// Sets the host that starts games (without one, nothing can be started).
    pub fn set_host(&self, host: Arc<dyn GameHost>) {
        *lock(&self.host) = Some(host);
    }

    fn host(&self) -> Option<Arc<dyn GameHost>> {
        lock(&self.host).clone()
    }

    /// The process of game `id` when it is running.
    fn running_pid(&self, id: &str, host: &dyn GameHost) -> Option<u32> {
        let pid = lock(&self.running).get(id).copied()?;
        if host.alive(pid) {
            Some(pid)
        } else {
            lock(&self.running).remove(id);
            None
        }
    }

    fn started(&self, id: &str, pid: u32) {
        lock(&self.running).insert(id.to_owned(), pid);
    }

    /// Forgets the games that have exited; answers how many are still running.
    pub fn sweep(&self, host: &dyn GameHost) -> usize {
        let mut running = lock(&self.running);
        running.retain(|_, pid| host.alive(*pid));
        running.len()
    }

    /// Whether any game this service started is still running.
    pub fn any_running(&self) -> bool {
        match self.host() {
            Some(host) => self.sweep(host.as_ref()) > 0,
            None => false,
        }
    }
}

/// One tile of the Games tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub running: bool,
}

/// The games this PC lists, sorted by name (case-insensitive, then id).
pub fn cards(entries: Vec<KioskLibraryEntry>, running: impl Fn(&str) -> bool) -> Vec<Card> {
    let mut cards: Vec<Card> = entries
        .into_iter()
        .filter(|e| e.kind == "game")
        .map(|e| Card {
            running: running(&e.id),
            id: e.id,
            name: e.name,
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
    let running = ctx.games.any_running();
    let host = ctx.games.host();
    let is_running = |id: &str| {
        host.as_deref()
            .is_some_and(|h| ctx.games.running_pid(id, h).is_some())
    };
    json!({ "games": cards(entries, is_running), "running": running })
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
    if let Some(pid) = ctx.games.running_pid(&entry.id, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    let Some(spec) = spec_of(&entry, device, crate::library::path_exists) else {
        return Ok(failed(ctx, &entry.name, "nothing to start on this PC"));
    };
    let lists = Arc::clone(&ctx.lists.borrow());
    if !allowed(&lists, &entry, &spec) {
        return Ok(failed(ctx, &entry.name, "not allowed on this PC"));
    }
    let start_host = Arc::clone(&host);
    let started = tokio::task::spawn_blocking(move || start_host.start(&spec))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return Ok(failed(ctx, &entry.name, &why)),
    };
    ctx.games.started(&entry.id, pid);
    let intent = crate::intents::game_launched(
        device,
        ctx.clock.now_ms(),
        KioskGameLaunched {
            game_id: entry.id.clone(),
            name: entry.name.clone(),
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
    tracing::info!(game = %entry.id, pid, "a game started");
    Ok(json!({ "outcome": "started" }))
}

/// Keeps the running games current until `stop` is set.
pub async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(POLL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if let Some(host) = ctx.games.host() {
                    ctx.games.sweep(host.as_ref());
                }
            }
            _ = stop.changed() => return,
        }
    }
}

/// A host that starts nothing real: process ids are handed out in order and a test decides
/// which have exited.
pub mod fake {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use super::{GameHost, LaunchSpec};

    #[derive(Default)]
    pub struct FakeGames {
        inner: Mutex<Inner>,
    }

    #[derive(Default)]
    struct Inner {
        next: u32,
        alive: HashSet<u32>,
        started: Vec<LaunchSpec>,
        fail: Option<String>,
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

        /// The process ids still running.
        pub fn alive_pids(&self) -> Vec<u32> {
            let mut v: Vec<u32> = self.inner.lock().unwrap().alive.iter().copied().collect();
            v.sort_unstable();
            v
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
    }
}

#[cfg(test)]
mod tests;

//! Launcher cards on the Games tab (Story 6.26): "Play on your own account" for Steam, Epic and
//! Riot.
//!
//! - `launchers` lists the launchers installed on this PC, in the order Steam, Epic, Riot, each
//!   with whether it is open. A launcher is installed when its executable is found where the
//!   launcher's own scan (Story 6.4) found it, or at the launcher's usual place. One that is not
//!   installed is not listed.
//! - `launcher_open{id}` opens that launcher, nothing else: the app names `steam`, `epic` or
//!   `riot` and never a path. The executable must pass the same allow-list the process enforcer
//!   applies, before anything starts. A launcher is signed out at every session end (Story 6.12),
//!   so a fresh start is on its login screen; the player signs in with their own account and the
//!   service never sees it. A launcher this service started that is still running, or that is
//!   running for another reason (a game started through its link), is not started a second
//!   time: the answer is `already_running{pid}` and the app brings its window to the front. A
//!   start queues a `launcher.opened` intent for the Hub (player and session are the Hub's to
//!   name); a refused or failed start queues a `kiosk.log{error, launcher_open_failed}` and
//!   answers `failed`.
//!
//! Nothing here starts without a session on screen, or while the PC is locked.

use std::sync::Arc;

use arena_kiosk_core::procpolicy::{Decision, ProcInfo, decide, normalize_path};
use arena_proto::{AppError, ErrorCode, KioskLogLevel, ScanLauncher};
use serde::Serialize;
use serde_json::{Value, json};

use crate::games::{GameHost, LaunchSpec};
use crate::procs::ProcessLists;
use crate::runtime::Ctx;

/// The queue kv key holding the launchers the last scan found (a JSON array of
/// [`ScanLauncher`]).
pub const SCAN_KEY: &str = "scan.launchers";
/// The `kiosk.log` code of a launcher that would not open.
pub const FAILED_CODE: &str = "launcher_open_failed";
/// The longest `kiosk.log` detail of a failed open, in characters.
const DETAIL_MAX: usize = 300;

/// One launcher the Games tab can open.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// `steam`, `epic` or `riot`: the id the app sends.
    pub id: &'static str,
    /// The name on the card.
    pub name: &'static str,
    /// The launcher's executables relative to the folder its scan reports, tried in order.
    scan_relative: &'static [&'static str],
    /// Where the launcher usually installs, tried after the scanned folder.
    usual: &'static [&'static str],
    /// The image names of its programs, to find it when it is already open.
    images: &'static [&'static str],
}

/// The launchers, in the order the cards appear.
pub const LAUNCHERS: [Def; 3] = [
    Def {
        id: "steam",
        name: "Steam",
        scan_relative: &["steam.exe"],
        usual: &[r"C:\Program Files (x86)\Steam\steam.exe"],
        images: &["steam.exe"],
    },
    Def {
        id: "epic",
        name: "Epic Games",
        scan_relative: &[
            r"Portal\Binaries\Win64\EpicGamesLauncher.exe",
            r"Portal\Binaries\Win32\EpicGamesLauncher.exe",
        ],
        usual: &[
            r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
            r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win32\EpicGamesLauncher.exe",
            r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
        ],
        images: &["EpicGamesLauncher.exe"],
    },
    Def {
        id: "riot",
        name: "Riot Client",
        scan_relative: &["RiotClientServices.exe"],
        usual: &[r"C:\Riot Games\Riot Client\RiotClientServices.exe"],
        images: &["RiotClientServices.exe", "RiotClientUx.exe"],
    },
];

/// The launcher whose id is `id`.
pub fn def_of(id: &str) -> Option<&'static Def> {
    LAUNCHERS.iter().find(|d| d.id == id)
}

/// The executable of `def` on this PC: under the folder the scan found for it, else at a usual
/// place; the first that `exists`.
pub fn exe_of(
    def: &Def,
    scanned: &[ScanLauncher],
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let from_scan = scanned
        .iter()
        .filter(|s| s.launcher == def.id)
        .flat_map(|s| {
            def.scan_relative
                .iter()
                .map(move |rel| format!(r"{}\{rel}", s.path.trim_end_matches(['\\', '/'])))
        });
    from_scan
        .chain(def.usual.iter().map(|p| (*p).to_owned()))
        .find(|p| exists(p))
}

/// The launchers the last scan found (empty before the first scan, or when its record is
/// damaged).
pub fn scanned(ctx: &Ctx) -> Vec<ScanLauncher> {
    let stored = ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .kv_get(SCAN_KEY);
    match stored {
        Ok(Some((value, _))) if !value.is_empty() => serde_json::from_str(&value)
            .map_err(|e| tracing::warn!(error = %e, "the recorded scan launchers cannot be read"))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// One card of the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    pub id: &'static str,
    pub name: &'static str,
    pub running: bool,
}

fn on_screen(ctx: &Ctx) -> bool {
    let screen = ctx.pc.screen.borrow();
    screen.session.is_some() && !screen.locked
}

fn host_of(ctx: &Ctx) -> Result<Arc<dyn GameHost>, AppError> {
    ctx.games
        .host()
        .ok_or_else(|| AppError::new(ErrorCode::Unsupported, "this PC cannot start programs"))
}

/// The process of launcher `def` when it is open: the one this service started, else any of its
/// programs running on the PC.
fn open_pid(ctx: &Ctx, def: &Def, host: &dyn GameHost) -> Option<u32> {
    ctx.games
        .launcher_pid(def.id, host)
        .or_else(|| host.find_process(def.images))
}

/// The `launchers` request: `{launchers: [{id, name, running}]}`; empty with no session on
/// screen.
pub fn list(ctx: &Ctx, exists: impl Fn(&str) -> bool) -> Result<Value, AppError> {
    if !on_screen(ctx) {
        return Ok(json!({ "launchers": [] }));
    }
    let host = ctx.games.host();
    let scan = scanned(ctx);
    let cards: Vec<Card> = LAUNCHERS
        .iter()
        .filter(|d| exe_of(d, &scan, &exists).is_some())
        .map(|d| Card {
            id: d.id,
            name: d.name,
            running: host
                .as_deref()
                .is_some_and(|h| open_pid(ctx, d, h).is_some()),
        })
        .collect();
    Ok(json!({ "launchers": cards }))
}

/// Whether the allow-list lets the executable `exe` start.
pub fn allowed(lists: &ProcessLists, exe: &str) -> bool {
    let proc = ProcInfo {
        path: normalize_path(exe),
        signer: None,
        valid_sig: false,
    };
    decide(&proc, &lists.allow, &lists.block) == Decision::Allow
}

/// Logs the refusal or failure for the Hub and answers `failed`.
fn failed(ctx: &Ctx, what: &str, why: &str) -> Value {
    let detail: String = format!("{what}: {why}").chars().take(DETAIL_MAX).collect();
    tracing::warn!(%detail, "a launcher would not open");
    if let Err(e) = ctx.log_event(KioskLogLevel::Error, FAILED_CODE, &detail) {
        tracing::error!(error = %e, "the failed open could not be queued");
    }
    json!({ "outcome": "failed" })
}

/// The `launcher_open` request: `{id}`; the answer is `{outcome: started | already_running |
/// failed, pid?}`.
pub async fn open(ctx: &Arc<Ctx>, body: Value) -> Result<Value, AppError> {
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::BadFrame, "launcher_open needs an id"))?
        .to_owned();
    let host = host_of(ctx)?;
    if !on_screen(ctx) {
        return Err(AppError::new(
            ErrorCode::Forbidden,
            "launchers open only in a session",
        ));
    }
    let device = ctx
        .config()
        .map(|c| c.device_id)
        .ok_or_else(|| AppError::new(ErrorCode::Forbidden, "this PC is not paired"))?;
    open_in(ctx, &device, &id, &host, crate::library::path_exists).await
}

/// [`open`] of launcher `id` as device `device`, with `exists` telling which files are here.
async fn open_in(
    ctx: &Arc<Ctx>,
    device: &str,
    id: &str,
    host: &Arc<dyn GameHost>,
    exists: impl Fn(&str) -> bool,
) -> Result<Value, AppError> {
    let Some(def) = def_of(id) else {
        return Ok(failed(ctx, id, "not a launcher"));
    };
    if let Some(pid) = open_pid(ctx, def, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    if !ctx.games.begin_launcher(def.id) {
        // Another request is opening it right now (a double click): not an error.
        return Ok(json!({ "outcome": "started" }));
    }
    let result = start(ctx, device, def, host, exists).await;
    ctx.games.end_launcher(def.id);
    result
}

/// Checks and starts launcher `def` (the caller holds its start claim).
async fn start(
    ctx: &Arc<Ctx>,
    device: &str,
    def: &Def,
    host: &Arc<dyn GameHost>,
    exists: impl Fn(&str) -> bool,
) -> Result<Value, AppError> {
    // Seen again under the claim: an open that finished meanwhile is running now.
    if let Some(pid) = open_pid(ctx, def, host.as_ref()) {
        return Ok(json!({ "outcome": "already_running", "pid": pid }));
    }
    let Some(exe) = exe_of(def, &scanned(ctx), exists) else {
        return Ok(failed(ctx, def.name, "not installed on this PC"));
    };
    let lists = Arc::clone(&ctx.lists.borrow());
    if !allowed(&lists, &exe) {
        return Ok(failed(ctx, def.name, "not allowed on this PC"));
    }
    let clicked_ms = ctx.clock.now_ms();
    let spec = LaunchSpec {
        exe: Some(exe),
        args: String::new(),
        uri: None,
    };
    let start_host = Arc::clone(host);
    let started = tokio::task::spawn_blocking(move || start_host.start(&spec))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    let pid = match started {
        Ok(pid) => pid,
        Err(why) => return Ok(failed(ctx, def.name, &why)),
    };
    ctx.games.launcher_started(def.id, pid);
    tracing::info!(launcher = def.id, pid, "a launcher opened");
    let intent = crate::intents::launcher_opened(device, clicked_ms, def.id);
    let queued = ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(&intent);
    match queued {
        Ok(()) => ctx.queued.notify_one(),
        Err(e) => tracing::error!(error = %e, "the launcher open could not be queued"),
    }
    Ok(json!({ "outcome": "started" }))
}

#[cfg(test)]
mod tests;

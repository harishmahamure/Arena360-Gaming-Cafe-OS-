//! Session-end cleanup (Story 6.11): the next player must not find the last player's programs,
//! clipboard, temp files or volume.
//!
//! A named, ordered list of [`CleanupStep`]s ([`Pipeline::standard`]; adding a step is adding
//! one entry there) runs on the generic session-end signal: [`pc::set_locked`](crate::pc) with
//! the reason `PayAtCounter`, `EndedByStaff`, `TimesUp` or `LoggedOut` while a session is dirty (any path,
//! now or later). `TimesUpSeeCounter` (Staff handles at grace end) does not trigger; staff's
//! later End does.
//!
//! - **Dirty.** Set when a session starts (a test session too), kept in the queue kv as
//!   `cleanup.pending` so a restart mid-clean re-runs it, cleared when the pipeline succeeds or
//!   staff say "Checked". A second trigger while the pipeline runs (the Hub's `TimesUp` after
//!   the local one) only updates the lock to show afterwards; after it, nothing is dirty.
//! - **The lock.** While the pipeline runs, and after a failure, the screen is `locked` with
//!   [`LockReason::PreparingPc`] (kiosk-local; the Hub never sends it). The reason the Hub or
//!   the timer asked for is kept (`cleanup.after`) and shown when the pipeline succeeds. A
//!   session that starts meanwhile (an Undo of an End) cancels the deferred lock.
//! - **Failure.** Each step retries once ([`Timings::cleanup_retry`](crate::Timings)). A step
//!   failing twice queues `kiosk.cleanup_failed{step, detail}` (offline works: it is queued like
//!   any kiosk event) and the other steps still run. The screen then holds
//!   (`cleanup.hold`) until a `cleanup_retry` pc command runs the pipeline again or a
//!   `cleanup_checked` releases it. Success queues `kiosk.log` info `cleanup.done`.
//!
//! - **Sign-out (Story 6.12).** After `programs`, one [`signout::SignOut`] step per launcher or
//!   browser (`signout_steam`, `signout_epic`, `signout_riot`, `signout_discord`,
//!   `reset_chrome`, `reset_edge`, `reset_firefox`) ends the target's programs, checks they are
//!   gone, and only then removes its saved logins; then `credentials` clears the player's
//!   Credential Manager. A failure uses the path above and names the step.
//!
//! The OS parts sit behind [`CleanupHost`] (Windows: [`cleanup_windows`](crate::cleanup_windows);
//! tests: [`fake::FakeHost`]). The `programs` step uses the process enforcer's kill path through
//! [`Ctx::closer`](crate::Ctx).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use arena_proto::{
    KioskCleanupFailed, KioskLogLevel, LockReason, MAX_CLEANUP_DETAIL_CHARS, MAX_CLEANUP_STEP_LEN,
};
use tokio::sync::{Notify, watch};

use crate::procs::{CloseReport, CloseScope};
use crate::queue::QueueDb;
use crate::runtime::Ctx;

pub mod fake;
pub mod signout;

/// The step names (also the `step` of a `kiosk.cleanup_failed`).
pub const STEP_PROGRAMS: &str = "programs";
pub const STEP_CLIPBOARD: &str = "clipboard";
pub const STEP_TEMP_FILES: &str = "temp_files";
pub const STEP_VOLUME: &str = "volume";

/// The volume used when the branch default was never synced.
pub const DEFAULT_VOLUME: u8 = 50;
/// The `kiosk.log` code of a finished cleanup.
pub const DONE_CODE: &str = "cleanup.done";

const PENDING_KEY: &str = "cleanup.pending";
const AFTER_KEY: &str = "cleanup.after";
const HOLD_KEY: &str = "cleanup.hold";
/// `cleanup.after` when the lock to show afterwards has no reason.
const NO_REASON: &str = "none";

/// The OS parts of a cleanup (the `programs` step has its own path, [`Ctx::closer`](crate::Ctx)).
/// Every method is blocking and runs on a blocking thread.
pub trait CleanupHost: Send + Sync {
    /// Empties the clipboard and its history in the player's session.
    fn clear_clipboard(&self) -> Result<(), String>;
    /// Deletes the contents of the player account's temp folder (not the folder, nothing else).
    fn clear_temp_files(&self) -> Result<(), String>;
    /// Sets the system volume to `percent` (0 to 100) and unmutes.
    fn set_volume(&self, percent: u8) -> Result<(), String>;
    /// The player account's profile folder (`None` when it has none yet).
    fn profile_dir(&self) -> Result<Option<PathBuf>, String>;
    /// The Steam install folder (`None` when Steam is not installed).
    fn steam_dir(&self) -> Option<PathBuf>;
    /// The branch's folder of clean browser templates (`None` when it keeps none): a browser's
    /// is `<folder>/chrome|edge|firefox`, with `Local` and `Roaming` inside, laid out like the
    /// player's `AppData\Local` and `AppData\Roaming`.
    fn browser_templates(&self) -> Option<PathBuf>;
    /// Forgets the remembered Steam account in the player's registry.
    fn clear_steam_login(&self) -> Result<(), String>;
    /// Removes the player's Windows Credential Manager entries.
    fn clear_credentials(&self) -> Result<(), String>;
}

/// What a step is given.
pub struct StepEnv<'a> {
    pub ctx: &'a Ctx,
    pub host: &'a dyn CleanupHost,
}

/// One named cleanup step. A failure is a short reason with no personal data (no file names,
/// no window titles).
pub trait CleanupStep: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, env: &StepEnv<'_>) -> Result<(), String>;
}

/// Ends the player's programs (not the kiosk app and its WebView2, the service or protected
/// System32 images). Mode `report` does not stop it.
pub struct Programs;
/// Clears the clipboard and its history.
pub struct Clipboard;
/// Clears the player account's temp folder.
pub struct TempFiles;
/// Sets the system volume to the branch default.
pub struct Volume;

impl CleanupStep for Programs {
    fn name(&self) -> &'static str {
        STEP_PROGRAMS
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        let report = close(env.ctx, CloseScope::All)?;
        tracing::info!(
            closed = report.closed,
            failed = report.failed,
            "cleanup ended the player's programs"
        );
        match report.failed {
            0 => Ok(()),
            1 => Err("1 program would not close".into()),
            n => Err(format!("{n} programs would not close")),
        }
    }
}

impl CleanupStep for Clipboard {
    fn name(&self) -> &'static str {
        STEP_CLIPBOARD
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        env.host.clear_clipboard()
    }
}

impl CleanupStep for TempFiles {
    fn name(&self) -> &'static str {
        STEP_TEMP_FILES
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        env.host.clear_temp_files()
    }
}

impl CleanupStep for Volume {
    fn name(&self) -> &'static str {
        STEP_VOLUME
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        let default =
            crate::operating::load(&env.ctx.queue.lock().unwrap_or_else(|p| p.into_inner()))
                .volume_default;
        env.host.set_volume(default.unwrap_or(DEFAULT_VOLUME))
    }
}

/// Ends the player's programs that match `scope` through the process enforcer's kill path
/// (the app, the service and protected images are never touched).
pub(crate) fn close(ctx: &Ctx, scope: CloseScope<'_>) -> Result<CloseReport, String> {
    let closer = ctx.closer.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let Some(closer) = closer else {
        return Err("process control is not running".into());
    };
    closer(scope).map_err(|e| format!("could not list the programs: {e}"))
}

/// The ordered steps.
#[derive(Clone)]
pub struct Pipeline {
    steps: Vec<Arc<dyn CleanupStep>>,
}

impl Pipeline {
    /// The standard cleanup: the programs go first, so launchers are shut down before their
    /// sign-out files are removed; the sign-outs and credentials follow, then the rest.
    pub fn standard() -> Self {
        let mut steps: Vec<Arc<dyn CleanupStep>> = vec![Arc::new(Programs)];
        steps.extend(
            signout::Target::ALL
                .into_iter()
                .map(|t| Arc::new(signout::SignOut(t)) as Arc<dyn CleanupStep>),
        );
        steps.push(Arc::new(signout::Credentials));
        steps.push(Arc::new(Clipboard));
        steps.push(Arc::new(TempFiles));
        steps.push(Arc::new(Volume));
        Self { steps }
    }

    /// A pipeline of exactly these steps.
    pub fn of(steps: Vec<Arc<dyn CleanupStep>>) -> Self {
        Self { steps }
    }

    /// The step names, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.steps.iter().map(|s| s.name()).collect()
    }
}

/// What the cleanup keeps in memory (the rest is in the queue kv, so a restart finds it).
pub struct CleanupState {
    /// A worker runs ([`Runtime::cleanup`](crate::Runtime)): without one, locks are untouched.
    enabled: AtomicBool,
    /// A session started since the last successful cleanup (`cleanup.pending`).
    dirty: AtomicBool,
    /// Bumped when the cleanup is cancelled or released, so a running pipeline stops.
    epoch: AtomicU64,
    /// Serialises the state changes (trigger, cancel, retry, checked, finish).
    guard: std::sync::Mutex<()>,
    wake: Notify,
    /// The volume slider (Story 6.15): the host it sets through and the level it last set.
    pub(crate) volume: crate::volume::VolumeState,
}

impl CleanupState {
    pub fn load(queue: &QueueDb) -> Self {
        let dirty = matches!(queue.kv_get(PENDING_KEY), Ok(Some((v, _))) if v == "1");
        Self {
            enabled: AtomicBool::new(false),
            dirty: AtomicBool::new(dirty),
            epoch: AtomicU64::new(0),
            guard: std::sync::Mutex::new(()),
            wake: Notify::new(),
            volume: Default::default(),
        }
    }

    /// Whether a session has run since the PC was last cleaned.
    pub fn dirty(&self) -> bool {
        self.dirty.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.guard.lock().unwrap_or_else(|p| p.into_inner())
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
        Ok(_) => None,
        Err(e) => {
            tracing::error!(key, error = %e, "the cleanup state cannot be read");
            None
        }
    }
}

/// Stores `value` (empty clears it).
fn kv_set(ctx: &Ctx, key: &str, value: &str) {
    if let Err(e) = ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .kv_set(key, value, 0)
    {
        tracing::error!(key, error = %e, "the cleanup state cannot be saved");
    }
}

fn reason_key(reason: Option<LockReason>) -> String {
    match reason {
        Some(r) => serde_json::to_value(r)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| NO_REASON.to_owned()),
        None => NO_REASON.to_owned(),
    }
}

fn parse_reason(key: &str) -> Option<LockReason> {
    serde_json::from_value(serde_json::Value::String(key.to_owned())).ok()
}

/// Whether a cleanup is running or held (its lock is still to be shown).
fn active(ctx: &Ctx) -> bool {
    kv_get(ctx, AFTER_KEY).is_some()
}

fn holding(ctx: &Ctx) -> bool {
    kv_get(ctx, HOLD_KEY).is_some()
}

fn set_dirty(ctx: &Ctx, dirty: bool) {
    if ctx.cleanup.dirty.swap(dirty, Ordering::SeqCst) != dirty {
        kv_set(ctx, PENDING_KEY, if dirty { "1" } else { "0" });
    }
}

/// A session started on this PC (or a test session): the next end needs a cleanup.
pub(crate) fn mark_dirty(ctx: &Ctx) {
    set_dirty(ctx, true);
}

/// Whether `reason` is a session-end lock: the generic signal that runs the cleanup.
fn ends_session(reason: Option<LockReason>) -> bool {
    matches!(
        reason,
        Some(
            LockReason::PayAtCounter
                | LockReason::EndedByStaff
                | LockReason::TimesUp
                | LockReason::LoggedOut
        )
    )
}

/// What [`pc::set_locked`](crate::pc) shows: the cleanup's own lock in place of `reason` while
/// a cleanup runs (or just starts), remembering `reason` for afterwards. An unlock (a session
/// started, staff unlocked) cancels the deferred lock.
pub(crate) fn intercept(ctx: &Ctx, locked: bool, reason: Option<LockReason>) -> Option<LockReason> {
    let state = &ctx.cleanup;
    if !state.enabled.load(Ordering::SeqCst) {
        return reason;
    }
    let _guard = state.lock();
    if !locked {
        if active(ctx) {
            tracing::info!("a session started or staff unlocked; the cleanup lock is cancelled");
            kv_set(ctx, AFTER_KEY, "");
            kv_set(ctx, HOLD_KEY, "");
            state.epoch.fetch_add(1, Ordering::SeqCst);
        }
        return reason;
    }
    if active(ctx) {
        // Running or held: show the newest lock afterwards.
        kv_set(ctx, AFTER_KEY, &reason_key(reason));
        return Some(LockReason::PreparingPc);
    }
    if ends_session(reason) && state.dirty() {
        tracing::info!("the session ended; cleaning the PC before the lock shows");
        kv_set(ctx, AFTER_KEY, &reason_key(reason));
        state.wake.notify_one();
        return Some(LockReason::PreparingPc);
    }
    reason
}

/// `cleanup_retry`: runs the held pipeline again.
pub(crate) fn retry(ctx: &Ctx) {
    let state = &ctx.cleanup;
    if !state.enabled.load(Ordering::SeqCst) {
        return;
    }
    let _guard = state.lock();
    if !holding(ctx) {
        tracing::info!("retry cleanup: nothing is held");
        return;
    }
    tracing::info!("retry cleanup");
    kv_set(ctx, HOLD_KEY, "");
    set_dirty(ctx, true);
    state.wake.notify_one();
}

/// `cleanup_checked`: staff checked the PC; the held lock is released without a run.
pub(crate) fn checked(ctx: &Ctx) {
    let state = &ctx.cleanup;
    if !state.enabled.load(Ordering::SeqCst) {
        return;
    }
    let _guard = state.lock();
    let Some(after) = kv_get(ctx, AFTER_KEY).filter(|_| holding(ctx)) else {
        tracing::info!("cleanup checked: nothing is held");
        return;
    };
    tracing::info!("cleanup checked by staff; the hold is released");
    state.epoch.fetch_add(1, Ordering::SeqCst);
    release(ctx, &after);
}

/// Ends the cleanup state and shows the remembered lock. The caller holds the guard.
fn release(ctx: &Ctx, after: &str) {
    kv_set(ctx, AFTER_KEY, "");
    kv_set(ctx, HOLD_KEY, "");
    set_dirty(ctx, false);
    crate::pc::show_lock(ctx, parse_reason(after));
}

/// Shortens and cleans a reason for the Hub: one line, no control characters.
fn clean_detail(detail: &str) -> String {
    detail
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CLEANUP_DETAIL_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn queue_intent(ctx: &Ctx, intent: &arena_proto::Intent, what: &str) {
    match ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(intent)
    {
        Ok(()) => ctx.queued.notify_one(),
        Err(e) => tracing::error!(error = %e, "could not queue {what}"),
    }
}

fn device_id(ctx: &Ctx) -> String {
    ctx.config()
        .map_or_else(|| "unprovisioned".into(), |c| c.device_id)
}

fn report_failure(ctx: &Ctx, step: &str, detail: &str) {
    let payload = KioskCleanupFailed {
        step: step.chars().take(MAX_CLEANUP_STEP_LEN).collect(),
        detail: clean_detail(detail),
    };
    if let Err(e) = payload.validate() {
        tracing::error!(step, error = %e, "not reporting a cleanup failure the hub would refuse");
        return;
    }
    let intent = crate::intents::kiosk_cleanup_failed(&device_id(ctx), ctx.clock.now_ms(), payload);
    queue_intent(ctx, &intent, "kiosk.cleanup_failed");
}

/// Runs one step, with its one retry. `Err` is the second failure's reason.
async fn run_step(
    ctx: &Arc<Ctx>,
    host: &Arc<dyn CleanupHost>,
    step: &Arc<dyn CleanupStep>,
) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        let (c, h, s) = (Arc::clone(ctx), Arc::clone(host), Arc::clone(step));
        let outcome = tokio::task::spawn_blocking(move || {
            s.run(&StepEnv {
                ctx: &c,
                host: h.as_ref(),
            })
        })
        .await
        .unwrap_or_else(|_| Err("the step stopped unexpectedly".into()));
        match outcome {
            Ok(()) => return Ok(()),
            Err(e) if attempt == 0 => {
                tracing::warn!(step = step.name(), error = %e, "a cleanup step failed; retrying once");
                attempt += 1;
                tokio::time::sleep(ctx.timings.cleanup_retry).await;
            }
            Err(e) => {
                tracing::error!(step = step.name(), error = %e, "a cleanup step failed twice");
                return Err(e);
            }
        }
    }
}

/// Runs the pipeline once for the cleanup that is waiting.
async fn run_pipeline(
    ctx: &Arc<Ctx>,
    host: &Arc<dyn CleanupHost>,
    pipeline: &Pipeline,
    stop: &watch::Receiver<bool>,
) {
    let epoch = ctx.cleanup.epoch.load(Ordering::SeqCst);
    let mut failed = 0;
    for step in &pipeline.steps {
        if ctx.cleanup.epoch.load(Ordering::SeqCst) != epoch {
            tracing::info!("the cleanup was cancelled; stopping");
            return;
        }
        if *stop.borrow() {
            return;
        }
        if let Err(detail) = run_step(ctx, host, step).await {
            if ctx.cleanup.epoch.load(Ordering::SeqCst) != epoch {
                tracing::info!("the cleanup was cancelled; its failure is dropped");
                return;
            }
            failed += 1;
            report_failure(ctx, step.name(), &detail);
        }
    }
    let guard = ctx.cleanup.lock();
    if ctx.cleanup.epoch.load(Ordering::SeqCst) != epoch {
        tracing::info!("the cleanup was cancelled; stopping");
        return;
    }
    if failed > 0 {
        tracing::warn!(
            failed,
            "the cleanup left steps undone; holding the lock for staff"
        );
        kv_set(ctx, HOLD_KEY, "1");
        return;
    }
    let after = kv_get(ctx, AFTER_KEY).unwrap_or_else(|| NO_REASON.to_owned());
    release(ctx, &after);
    drop(guard);
    let names = pipeline.names().join(",");
    if let Err(e) = ctx.log_event(KioskLogLevel::Info, DONE_CODE, &names) {
        tracing::error!(error = %e, "could not queue cleanup.done");
    }
    tracing::info!("the PC is clean");
}

/// Whether a pipeline run is waiting.
fn waiting(ctx: &Ctx) -> bool {
    active(ctx) && !holding(ctx)
}

/// A restart finds the state the last run left: a cleanup in progress runs again; a held one
/// keeps its lock for staff.
fn restore(ctx: &Ctx) {
    let _guard = ctx.cleanup.lock();
    if !active(ctx) {
        return;
    }
    tracing::info!(
        held = holding(ctx),
        "a cleanup was under way when the service stopped"
    );
    set_dirty(ctx, true);
    crate::pc::show_cleanup_lock(ctx);
}

/// The cleanup worker: waits for the session-end signal and runs the pipeline.
pub(crate) async fn run(
    ctx: Arc<Ctx>,
    host: Arc<dyn CleanupHost>,
    pipeline: Pipeline,
    mut stop: watch::Receiver<bool>,
) {
    restore(&ctx);
    loop {
        if *stop.borrow() {
            return;
        }
        if waiting(&ctx) {
            run_pipeline(&ctx, &host, &pipeline, &stop).await;
            continue;
        }
        tokio::select! {
            () = ctx.cleanup.wake.notified() => {}
            _ = stop.changed() => return,
        }
    }
}

/// Switches the cleanup on (a worker is about to run).
pub(crate) fn enable(ctx: &Ctx) {
    ctx.cleanup.enabled.store(true, Ordering::SeqCst);
}

/// Deletes everything inside `dir` but not `dir` itself. A link (symlink or junction) is
/// removed, never followed. A missing `dir` is fine. Items that cannot be removed (a locked
/// file) are counted: the error names only how many, never a file name.
pub fn clear_dir_contents(dir: &std::path::Path) -> Result<(), String> {
    // The player owns their profile and may swap the folder itself for a link; the service
    // must not empty whatever it points at.
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err("the temp folder is a link, not a folder".into());
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("the temp folder cannot be read: {e}")),
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("the temp folder cannot be read: {e}")),
    };
    let mut left = 0usize;
    for entry in entries {
        let Ok(entry) = entry else {
            left += 1;
            continue;
        };
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let kind = meta.file_type();
        let removed = if kind.is_symlink() {
            // A directory link goes with `remove_dir`, a file link with `remove_file`.
            std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir(&path))
        } else if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match removed {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => left += 1,
        }
    }
    match left {
        0 => Ok(()),
        1 => Err("1 temp item could not be removed".into()),
        n => Err(format!("{n} temp items could not be removed")),
    }
}

/// How long a fresh `cleanup_retry` default waits between a step's two tries.
pub const DEFAULT_RETRY: Duration = Duration::from_secs(3);

#[cfg(test)]
mod tests;

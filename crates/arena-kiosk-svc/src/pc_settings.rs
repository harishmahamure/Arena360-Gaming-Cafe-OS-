//! PC settings saved to the player's profile (Story 6.35).
//!
//! The app's PC settings panel asks three things of the service:
//!
//! - `pc_settings`: the panel's content ([`arena_proto::PcSettingsPanel`]): this PC's current
//!   mouse speed, display mode, sound output and keyboard layout, and what the player may choose
//!   here. The display modes offered are the branch's allowed list (`kiosk.display_modes`; empty
//!   means this PC's own mode only) **and** what this PC's monitor supports.
//! - `pc_settings_set{..}`: one or more changes. Each is applied at once through the
//!   [`PcSettingsHost`]; a profiled player's choices are then saved by the Hub
//!   (`player.pc_settings_set`), a guest's are not (the Hub answers `saved:false`). With the Hub
//!   unreachable the change stays applied for the session and the app is told `HUB_SLOW`.
//!   A display mode is applied but not saved: it goes back after
//!   [`DISPLAY_REVERT_SECS`](arena_proto::DISPLAY_REVERT_SECS) unless the player presses "Keep
//!   this".
//! - `pc_settings_keep`: the player kept the new display mode; it is saved now.
//!
//! The service records the PC's own values (the baseline) before the first change of a session
//! and keeps them in the queue kv, so a restart mid-session still finds them. The session-end
//! cleanup's `pc_settings` step puts them back ([`restore`]). At session start the settings saved
//! for the player (`player.pc_settings`) are applied one by one ([`session_started`]); one this PC
//! cannot honour (a mode not allowed or not supported, a missing device or layout) is skipped and
//! the PC keeps its baseline for it.
//!
//! The OS parts sit behind [`PcSettingsHost`] (Windows:
//! [`pc_settings_windows`](crate::pc_settings_windows); tests: [`fake::FakeHost`]). Every host
//! method is blocking and runs on a blocking thread.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{
    AppError, DISPLAY_REVERT_SECS, ErrorCode, PcLayout, PcSettings, PcSettingsChanged,
    PcSettingsPanel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::Ctx;

pub mod fake;

/// The kv key the baseline is kept under.
const BASELINE_KEY: &str = "pc_settings.baseline";

/// What this PC can do and what it is doing now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcSnapshot {
    /// The pointer speed, 1 to 20.
    pub mouse_speed: u8,
    /// The current display mode, `WxH@Hz`.
    pub display_mode: String,
    /// Every mode the monitor supports, `WxH@Hz`.
    pub display_modes: Vec<String>,
    /// The sound outputs, by name.
    pub audio_outputs: Vec<String>,
    /// The default sound output now.
    pub audio_output: Option<String>,
    pub keyboard_layouts: Vec<PcLayout>,
    /// The active keyboard layout's id.
    pub keyboard_layout: Option<String>,
}

/// The OS parts of the PC settings. Every method is blocking.
pub trait PcSettingsHost: Send + Sync {
    /// Reads the current values and what the PC offers.
    fn snapshot(&self) -> Result<PcSnapshot, String>;
    /// Sets the pointer speed (1 to 20).
    fn set_mouse_speed(&self, speed: u8) -> Result<(), String>;
    /// Switches the display to `mode` (`WxH@Hz`).
    fn set_display_mode(&self, mode: &str) -> Result<(), String>;
    /// Makes the sound output named `name` the player's default.
    fn set_audio_output(&self, name: &str) -> Result<(), String>;
    /// Activates the keyboard layout `id`.
    fn set_keyboard_layout(&self, id: &str) -> Result<(), String>;
}

/// A display change that waits for "Keep this".
struct Pending {
    /// The mode that goes back if it is not kept.
    previous: String,
    /// The mode waiting to be kept.
    mode: String,
    /// Tells a timer whether it is still the current one.
    epoch: u64,
    deadline: tokio::time::Instant,
}

struct Inner {
    host: Option<Arc<dyn PcSettingsHost>>,
    /// The PC's own values; `None` until read from the kv, and `Some(None)` when nothing was
    /// changed since the last cleanup.
    baseline: Option<Option<PcSettings>>,
    pending: Option<Pending>,
    epoch: u64,
    revert_after: Duration,
}

/// What the service remembers about the player's PC settings.
pub struct PcSettingsState {
    inner: Mutex<Inner>,
}

impl Default for PcSettingsState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                host: None,
                baseline: None,
                pending: None,
                epoch: 0,
                revert_after: Duration::from_secs(u64::from(DISPLAY_REVERT_SECS)),
            }),
        }
    }
}

impl PcSettingsState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Gives the panel the host it changes settings through.
    pub(crate) fn set_host(&self, host: Arc<dyn PcSettingsHost>) {
        self.lock().host = Some(host);
    }

    fn host(&self) -> Option<Arc<dyn PcSettingsHost>> {
        self.lock().host.clone()
    }

    /// How long a display change waits for "Keep this" (tests shorten it).
    pub fn set_revert_after(&self, after: Duration) {
        self.lock().revert_after = after;
    }

    /// Whether a display change waits for "Keep this".
    pub fn pending(&self) -> bool {
        self.lock().pending.is_some()
    }

    /// Cancels a waiting display change (its timer then does nothing) and says what it was.
    fn take_pending(&self) -> Option<Pending> {
        let mut inner = self.lock();
        inner.epoch += 1;
        inner.pending.take()
    }
}

fn unsupported() -> AppError {
    AppError::new(ErrorCode::Unsupported, "this PC has no PC settings control")
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn host(ctx: &Ctx) -> Result<Arc<dyn PcSettingsHost>, AppError> {
    ctx.pc_settings.host().ok_or_else(unsupported)
}

/// Runs a blocking host call off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|_| Err("the PC settings call stopped unexpectedly".into()))
}

fn could_not(what: &str, e: &str) -> AppError {
    tracing::warn!(error = %e, "{what} could not be set");
    AppError::new(ErrorCode::Internal, format!("{what} could not be set"))
}

fn operating(ctx: &Ctx) -> crate::operating::KioskOperating {
    crate::operating::load(&ctx.queue.lock().unwrap_or_else(|p| p.into_inner()))
}

// ---- The baseline ----

/// `Err` when the stored baseline cannot be read: it may exist, so nothing may record over it.
fn kv_read(ctx: &Ctx) -> Result<Option<PcSettings>, ()> {
    let stored = ctx
        .queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .kv_get(BASELINE_KEY);
    match stored {
        Ok(Some((v, _))) if !v.is_empty() => Ok(serde_json::from_str(&v).ok()),
        Ok(_) => Ok(None),
        Err(e) => {
            tracing::error!(error = %e, "the PC settings baseline cannot be read");
            Err(())
        }
    }
}

fn kv_write(ctx: &Ctx, baseline: Option<&PcSettings>) {
    let value = baseline
        .and_then(|b| serde_json::to_string(b).ok())
        .unwrap_or_default();
    if let Err(e) =
        ctx.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .kv_set(BASELINE_KEY, &value, 0)
    {
        tracing::error!(error = %e, "the PC settings baseline cannot be saved");
    }
}

/// The PC's own values, recorded before this session's first change.
fn baseline_checked(ctx: &Ctx) -> Result<Option<PcSettings>, ()> {
    let mut inner = ctx.pc_settings.lock();
    if inner.baseline.is_none() {
        // A failed read is not cached, so the next call reads again.
        inner.baseline = Some(kv_read(ctx)?);
    }
    Ok(inner.baseline.clone().flatten())
}

fn baseline(ctx: &Ctx) -> Option<PcSettings> {
    baseline_checked(ctx).ok().flatten()
}

/// Records `snap` as the baseline unless one is already recorded (a session that starts while an
/// earlier cleanup failed keeps the earlier, truer one).
fn record_baseline(ctx: &Ctx, snap: &PcSnapshot) {
    if !matches!(baseline_checked(ctx), Ok(None)) {
        return;
    }
    let base = PcSettings {
        mouse_speed: Some(snap.mouse_speed),
        display_mode: Some(snap.display_mode.clone()),
        audio_output: snap.audio_output.clone(),
        keyboard_layout: snap.keyboard_layout.clone(),
    };
    kv_write(ctx, Some(&base));
    ctx.pc_settings.lock().baseline = Some(Some(base));
}

// ---- What the player may choose ----

/// The display modes the player may pick on this PC: the branch's allowed list that the monitor
/// supports, in the branch's order. An empty allowed list means this PC's own mode only.
fn display_choices(ctx: &Ctx, snap: &PcSnapshot) -> Vec<String> {
    let allowed = operating(ctx).display_modes;
    if allowed.is_empty() {
        let own = baseline(ctx)
            .and_then(|b| b.display_mode)
            .unwrap_or_else(|| snap.display_mode.clone());
        return vec![own];
    }
    allowed
        .iter()
        .map(ToString::to_string)
        .filter(|m| snap.display_modes.contains(m))
        .collect()
}

/// Why `patch` cannot be applied on this PC, if it cannot: the field it names.
fn unhonourable(ctx: &Ctx, snap: &PcSnapshot, patch: &PcSettings) -> Option<&'static str> {
    if let Some(mode) = &patch.display_mode
        && !display_choices(ctx, snap).contains(mode)
    {
        return Some("display_mode");
    }
    if let Some(out) = &patch.audio_output
        && !snap.audio_outputs.contains(out)
    {
        return Some("audio_output");
    }
    if let Some(id) = &patch.keyboard_layout
        && !snap.keyboard_layouts.iter().any(|l| &l.id == id)
    {
        return Some("keyboard_layout");
    }
    None
}

fn panel(ctx: &Ctx, snap: PcSnapshot) -> PcSettingsPanel {
    let revert_ms = {
        let inner = ctx.pc_settings.lock();
        inner.pending.as_ref().map(|p| {
            p.deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_millis() as u64
        })
    };
    PcSettingsPanel {
        display_modes: display_choices(ctx, &snap),
        mouse_speed: snap.mouse_speed,
        display_mode: snap.display_mode,
        audio_outputs: snap.audio_outputs,
        audio_output: snap.audio_output,
        keyboard_layouts: snap.keyboard_layouts,
        keyboard_layout: snap.keyboard_layout,
        revert_ms,
    }
}

/// The `pc_settings` request: the panel's content.
pub async fn view(ctx: &Arc<Ctx>) -> Result<Value, AppError> {
    let host = host(ctx)?;
    let snap = blocking(move || host.snapshot())
        .await
        .map_err(|e| could_not("the PC settings", &e))?;
    serde_json::to_value(panel(ctx, snap))
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
}

// ---- Changing ----

fn has_session(ctx: &Ctx) -> bool {
    let screen = ctx.pc.screen.borrow();
    screen.session.is_some() && !screen.locked
}

async fn apply_mouse(host: &Arc<dyn PcSettingsHost>, speed: u8) -> Result<(), String> {
    let host = Arc::clone(host);
    blocking(move || host.set_mouse_speed(speed)).await
}

async fn apply_display(host: &Arc<dyn PcSettingsHost>, mode: &str) -> Result<(), String> {
    let (host, mode) = (Arc::clone(host), mode.to_owned());
    blocking(move || host.set_display_mode(&mode)).await
}

async fn apply_audio(host: &Arc<dyn PcSettingsHost>, name: &str) -> Result<(), String> {
    let (host, name) = (Arc::clone(host), name.to_owned());
    blocking(move || host.set_audio_output(&name)).await
}

async fn apply_layout(host: &Arc<dyn PcSettingsHost>, id: &str) -> Result<(), String> {
    let (host, id) = (Arc::clone(host), id.to_owned());
    blocking(move || host.set_keyboard_layout(&id)).await
}

/// The `pc_settings_set` request: applies each change at once, then has the Hub save what is a
/// profiled player's (everything but a display mode, which waits for "Keep this").
pub async fn change(ctx: &Arc<Ctx>, body: Value) -> Result<Value, AppError> {
    let patch: PcSettings = serde_json::from_value(body)
        .map_err(|_| AppError::new(ErrorCode::BadFrame, "malformed PC settings change"))?;
    patch.validate()?;
    if patch.is_empty() {
        return Err(invalid("settings", "choose at least one PC setting"));
    }
    let host = host(ctx)?;
    if !has_session(ctx) {
        return Err(invalid("settings", "there is no session to change"));
    }
    let snapshot = {
        let host = Arc::clone(&host);
        blocking(move || host.snapshot()).await
    }
    .map_err(|e| could_not("the PC settings", &e))?;
    if let Some(field) = unhonourable(ctx, &snapshot, &patch) {
        return Err(invalid(field, "this PC cannot use that choice"));
    }
    record_baseline(ctx, &snapshot);

    let mut to_save = PcSettings::default();
    if let Some(speed) = patch.mouse_speed {
        apply_mouse(&host, speed)
            .await
            .map_err(|e| could_not("the mouse speed", &e))?;
        to_save.mouse_speed = Some(speed);
    }
    if let Some(name) = &patch.audio_output {
        apply_audio(&host, name)
            .await
            .map_err(|e| could_not("the sound output", &e))?;
        to_save.audio_output = Some(name.clone());
    }
    if let Some(id) = &patch.keyboard_layout {
        apply_layout(&host, id)
            .await
            .map_err(|e| could_not("the keyboard layout", &e))?;
        to_save.keyboard_layout = Some(id.clone());
    }
    let mut revert_ms = None;
    if let Some(mode) = &patch.display_mode {
        revert_ms = start_display_change(ctx, &host, &snapshot, mode).await?;
    }
    let saved = if to_save.is_empty() {
        false
    } else {
        crate::player_session::pc_settings_set(ctx, &to_save).await?
    };
    Ok(changed(saved, revert_ms))
}

fn changed(saved: bool, revert_ms: Option<u64>) -> Value {
    serde_json::to_value(PcSettingsChanged { saved, revert_ms }).unwrap_or(Value::Null)
}

/// Switches the display to `mode` and starts the countdown that puts the previous mode back.
async fn start_display_change(
    ctx: &Arc<Ctx>,
    host: &Arc<dyn PcSettingsHost>,
    snapshot: &PcSnapshot,
    mode: &str,
) -> Result<Option<u64>, AppError> {
    if snapshot.display_mode == mode {
        // Already on it: nothing to apply, nothing to confirm.
        return Ok(None);
    }
    apply_display(host, mode)
        .await
        .map_err(|e| could_not("the display mode", &e))?;
    let (epoch, after) = {
        let mut inner = ctx.pc_settings.lock();
        inner.epoch += 1;
        // A second change before the answer keeps the mode the PC was on before the first.
        let previous = inner
            .pending
            .take()
            .map_or_else(|| snapshot.display_mode.clone(), |p| p.previous);
        let after = inner.revert_after;
        inner.pending = Some(Pending {
            previous,
            mode: mode.to_owned(),
            epoch: inner.epoch,
            deadline: tokio::time::Instant::now() + after,
        });
        (inner.epoch, after)
    };
    let ctx = Arc::clone(ctx);
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        revert_if_waiting(&ctx, epoch).await;
    });
    Ok(Some(after.as_millis() as u64))
}

/// The countdown ran out: the previous display mode comes back, and nothing is saved.
async fn revert_if_waiting(ctx: &Arc<Ctx>, epoch: u64) {
    let previous = {
        let mut inner = ctx.pc_settings.lock();
        match &inner.pending {
            Some(p) if p.epoch == epoch => inner.pending.take().map(|p| p.previous),
            _ => None,
        }
    };
    let (Some(previous), Some(host)) = (previous, ctx.pc_settings.host()) else {
        return;
    };
    tracing::info!("the new display mode was not kept; going back");
    if let Err(e) = apply_display(&host, &previous).await {
        tracing::error!(error = %e, "the display mode did not go back");
        let _ = ctx.log_event(
            arena_proto::KioskLogLevel::Warn,
            "pc_settings.revert_failed",
            "the display mode did not go back",
        );
    }
}

/// The `pc_settings_keep` request: the player kept the new display mode, so the Hub saves it.
pub async fn keep(ctx: &Arc<Ctx>) -> Result<Value, AppError> {
    let Some(pending) = ctx.pc_settings.take_pending() else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "no display change is waiting",
        ));
    };
    let patch = PcSettings {
        display_mode: Some(pending.mode),
        ..Default::default()
    };
    let saved = crate::player_session::pc_settings_set(ctx, &patch).await?;
    Ok(changed(saved, None))
}

// ---- Session start and end ----

/// A session started: the settings saved for the player are applied, one by one, off the caller's
/// thread. A guest, a player with nothing saved, a Hub that does not answer and a PC with no
/// settings control all leave the PC as it is.
pub fn session_started(ctx: &Arc<Ctx>) {
    if ctx.pc_settings.host().is_none() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let ctx = Arc::clone(ctx);
    runtime.spawn(async move {
        let saved = match crate::player_session::pc_settings(&ctx).await {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => return,
            Err(e) => {
                tracing::info!(
                    code = e.code.as_str(),
                    "the saved PC settings were not read"
                );
                return;
            }
        };
        apply_saved(&ctx, saved).await;
    });
}

/// Applies a player's saved settings. Each one this PC cannot honour is skipped.
pub async fn apply_saved(ctx: &Arc<Ctx>, saved: PcSettings) {
    let Some(host) = ctx.pc_settings.host() else {
        return;
    };
    let snapshot = {
        let host = Arc::clone(&host);
        blocking(move || host.snapshot()).await
    };
    let snapshot = match snapshot {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "the PC could not be read; the saved settings are skipped");
            return;
        }
    };
    if !has_session(ctx) {
        // The session ended while the Hub and the PC were being asked.
        return;
    }
    record_baseline(ctx, &snapshot);
    // The display first, then the rest: a skipped one never stops the others.
    let skip = |field: &str| {
        tracing::info!(field, "a saved PC setting cannot be honoured here; skipped");
    };
    if let Some(speed) = saved.mouse_speed {
        match apply_mouse(&host, speed).await {
            Ok(()) => {}
            Err(e) => tracing::warn!(error = %e, "the saved mouse speed was not applied"),
        }
    }
    let honourable = |one: PcSettings| unhonourable(ctx, &snapshot, &one);
    if let Some(mode) = &saved.display_mode {
        let one = PcSettings {
            display_mode: Some(mode.clone()),
            ..Default::default()
        };
        match honourable(one) {
            Some(field) => skip(field),
            None if *mode == snapshot.display_mode => {}
            None => {
                if let Err(e) = apply_display(&host, mode).await {
                    tracing::warn!(error = %e, "the saved display mode was not applied");
                }
            }
        }
    }
    if let Some(name) = &saved.audio_output {
        let one = PcSettings {
            audio_output: Some(name.clone()),
            ..Default::default()
        };
        match honourable(one) {
            Some(field) => skip(field),
            None => {
                if let Err(e) = apply_audio(&host, name).await {
                    tracing::warn!(error = %e, "the saved sound output was not applied");
                }
            }
        }
    }
    if let Some(id) = &saved.keyboard_layout {
        let one = PcSettings {
            keyboard_layout: Some(id.clone()),
            ..Default::default()
        };
        match honourable(one) {
            Some(field) => skip(field),
            None => {
                if let Err(e) = apply_layout(&host, id).await {
                    tracing::warn!(error = %e, "the saved keyboard layout was not applied");
                }
            }
        }
    }
}

/// The cleanup's `pc_settings` step: puts the mouse, display, sound output and keyboard layout
/// back to the baseline recorded before this session's first change, and forgets it. Nothing
/// changed means nothing to do. A value that will not go back is a failure (the baseline stays,
/// so a retry tries again).
pub fn restore(ctx: &Ctx) -> Result<(), String> {
    // A display change still waiting for "Keep this" must not fire after the session.
    ctx.pc_settings.take_pending();
    let Some(base) =
        baseline_checked(ctx).map_err(|()| "the PC settings baseline cannot be read".to_owned())?
    else {
        return Ok(());
    };
    let Some(host) = ctx.pc_settings.host() else {
        return Ok(());
    };
    let mut failed = 0usize;
    let mut note = |what: &str, result: Result<(), String>| {
        if let Err(e) = result {
            tracing::warn!(error = %e, "{what} did not go back to the PC's own");
            failed += 1;
        }
    };
    if let Some(speed) = base.mouse_speed {
        note("the mouse speed", host.set_mouse_speed(speed));
    }
    if let Some(mode) = &base.display_mode {
        note("the display mode", host.set_display_mode(mode));
    }
    if let Some(name) = &base.audio_output {
        note("the sound output", host.set_audio_output(name));
    }
    if let Some(id) = &base.keyboard_layout {
        note("the keyboard layout", host.set_keyboard_layout(id));
    }
    match failed {
        0 => {
            kv_write(ctx, None);
            ctx.pc_settings.lock().baseline = Some(None);
            Ok(())
        }
        1 => Err("1 PC setting would not go back".into()),
        n => Err(format!("{n} PC settings would not go back")),
    }
}

#[cfg(test)]
mod tests;

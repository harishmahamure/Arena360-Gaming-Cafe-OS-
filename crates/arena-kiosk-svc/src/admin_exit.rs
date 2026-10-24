//! The admin exit (Story 1.20): a manager drops the kiosk to the desktop for 15 minutes.
//!
//! - **At the PC.** The app's hidden Ctrl+Alt+Shift+A panel sends IPC `admin_exit{pin}`. While
//!   the throttle ([`arena_kiosk_core::admin_exit::Throttle`], held here so an app restart
//!   resets nothing) is paused nothing is checked. Otherwise:
//!   - **Online:** `kiosk.verify_admin_pin` over the pinned-TLS link. `PIN_REJECTED` and
//!     `AUTH_LOCKED` are final. Any other failure (unreachable, `HUB_SLOW`, an older Hub) falls
//!     through to the offline check.
//!   - **Offline:** argon2id against the cached verifier's entries (at most 50), off the async
//!     threads.
//!   - A match grants the exit and queues `device.admin_exit{mode}` elevated by
//!     `{pin, staff_id, mode, verifier_version}`, with its ULID key and `issued_at` minted now;
//!     the 1.16 queue delivers it (at once when connected). A miss counts toward the throttle,
//!     is logged (device and count, never the PIN) and queues `kiosk.log{warn,
//!     admin_exit_failed}`. The PIN is never stored or logged; the service's copy is cleared after the check on a
//!     best-effort basis.
//! - **Remotely.** The Hub pushes `admin_unlock{exit_id}`; the exit is granted and
//!   `kiosk.admin_unlock_ack{exit_id}` answered.
//! - **One at a time.** An attempt holds a lock from the throttle check to its outcome, so
//!   parallel attempts cannot get past the limit. A grant is refused while an exit runs; a
//!   refused remote unlock is not acknowledged and is reported re-locked.
//! - **Granting** records the exit in `queue.db` (`kv admin_exit`), starts Explorer in the
//!   player's session ([`AppLauncher::start_desktop`]), suspends process enforcement and tells
//!   the app (`admin_exit_started{until}`: it drops topmost and the keyboard lockdown).
//! - **Re-locking** at `until`: Explorer is stopped ([`AppLauncher::stop_desktop`]), the
//!   enforcer resumes with a sweep (and closes any Explorer left in the player session), the app
//!   locks down again (`admin_exit_ended`) and `device.admin_relock{exit_id}` is queued.
//! - **Restart.** An exit found recorded when the service starts re-locks at once: enforcement
//!   never stays suspended across a restart.
//! - **Verifier.** The Hub's `event{verifier}` is stored DPAPI-protected (machine scope) in
//!   `verifier.bin` when its version is higher than the cached one.

use std::sync::{Arc, Mutex};

use arena_kiosk_core::admin_exit::{ActiveExit, Gate, Throttle, replaces};
use arena_kiosk_core::ipc::SecretPin;
use arena_kiosk_core::{ClientError, HubClient};
use arena_proto::{
    ADMIN_EXIT_FAILED_CODE, AdminExitMode, AppError, ElevationMode, ErrorCode, FrameBody,
    KioskLogLevel, MAX_VERIFIER_ENTRIES, ManagerVerifier,
};
use tokio::sync::watch;

use crate::runtime::Ctx;
use crate::secret::SecretStore;

/// The `queue.db` `kv` key of the running exit (empty: none).
pub const KV_ADMIN_EXIT: &str = "admin_exit";

/// The service's admin-exit state.
pub struct AdminState {
    pub throttle: Mutex<Throttle>,
    /// The running exit, if any (the app supervisor and the enforcer watch it).
    pub active: watch::Sender<Option<ActiveExit>>,
    /// The cached manager verifier.
    pub verifier: Mutex<Option<ManagerVerifier>>,
    verifier_store: Box<dyn SecretStore>,
    /// The live Hub connection, while there is one.
    pub hub: Mutex<Option<Arc<HubClient>>>,
    /// Held for the whole of one PIN attempt (attempts run one at a time).
    attempt: tokio::sync::Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AdminState {
    /// Loads the cached verifier from `store` (a missing or unreadable one: none).
    pub fn load(verifier_store: Box<dyn SecretStore>) -> Self {
        let verifier = match verifier_store.load() {
            Ok(Some(bytes)) => match serde_json::from_slice::<ManagerVerifier>(&bytes) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!(error = %e, "verifier.bin is corrupt; ignoring it");
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                tracing::error!(error = %e, "verifier.bin cannot be read");
                None
            }
        };
        Self {
            throttle: Mutex::new(Throttle::default()),
            active: watch::channel(None).0,
            verifier: Mutex::new(verifier),
            verifier_store,
            hub: Mutex::new(None),
            attempt: tokio::sync::Mutex::new(()),
        }
    }

    /// The cached verifier's version.
    pub fn verifier_version(&self) -> Option<i64> {
        lock(&self.verifier).as_ref().map(|v| v.version)
    }

    /// Whether an exit runs now (process enforcement is suspended).
    pub fn suspended(&self) -> bool {
        self.active.borrow().is_some()
    }
}

/// Stores a pushed verifier when it is newer than the cached one.
pub fn store_verifier(ctx: &Ctx, verifier: ManagerVerifier) {
    let a = &ctx.admin;
    if !replaces(a.verifier_version(), verifier.version) {
        return;
    }
    let bytes = match serde_json::to_vec(&verifier) {
        Ok(b) => b,
        Err(e) => return tracing::error!(error = %e, "could not encode the verifier"),
    };
    if let Err(e) = a.verifier_store.save(&bytes) {
        tracing::error!(error = %e, "could not store the verifier");
    }
    tracing::info!(
        version = verifier.version,
        entries = verifier.entries.len(),
        "manager verifier updated"
    );
    *lock(&a.verifier) = Some(verifier);
}

/// Why an exit request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Wrong PIN, or no right to exit: one generic answer.
    Rejected,
    /// Paused: this many seconds are left.
    Locked(u32),
    /// Something broke on this side (the exit could not be recorded).
    Internal(String),
}

impl Refused {
    /// The `reply` error and body the app gets.
    pub fn to_reply(&self) -> (AppError, serde_json::Value) {
        match self {
            Self::Rejected => (
                AppError::new(ErrorCode::PinRejected, "that PIN can't approve this"),
                serde_json::Value::Null,
            ),
            Self::Locked(s) => (
                AppError::new(
                    ErrorCode::AuthLocked,
                    "too many wrong PINs; try again later",
                ),
                serde_json::json!({ "retry_after_s": s }),
            ),
            Self::Internal(m) => (
                AppError::new(ErrorCode::Internal, m.clone()),
                serde_json::Value::Null,
            ),
        }
    }
}

/// Who approved, and how.
struct Approval {
    staff_id: String,
    mode: AdminExitMode,
    verifier_version: i64,
}

/// What the Hub said about a PIN.
enum Online {
    Approved(Approval),
    Refused(Refused),
    /// Unreachable, slow or not understood: check offline.
    Unavailable,
}

async fn check_online(ctx: &Ctx, pin: &str) -> Online {
    let client = lock(&ctx.admin.hub).clone();
    let Some(client) = client.filter(|c| !c.is_closed()) else {
        return Online::Unavailable;
    };
    let body = serde_json::json!({ "pin_proof": pin });
    match client.request("kiosk.verify_admin_pin", body).await {
        Ok(frame) => match frame.body {
            FrameBody::AdminPinOk(ok) => Online::Approved(Approval {
                staff_id: ok.staff_id,
                mode: AdminExitMode::Hub,
                verifier_version: ok.verifier_version,
            }),
            other => {
                tracing::warn!(
                    kind = other.kind(),
                    "unexpected reply to kiosk.verify_admin_pin"
                );
                Online::Unavailable
            }
        },
        Err(ClientError::Hub(e)) if e.code == ErrorCode::PinRejected => {
            Online::Refused(Refused::Rejected)
        }
        Err(ClientError::Hub(e)) if e.code == ErrorCode::AuthLocked => {
            Online::Refused(Refused::Locked(60))
        }
        Err(e) => {
            tracing::info!(error = %e, "the hub could not check the admin PIN; checking offline");
            Online::Unavailable
        }
    }
}

/// Checks `pin` against the cached verifier (at most [`MAX_VERIFIER_ENTRIES`] argon2id checks),
/// on a blocking thread.
async fn check_offline(ctx: &Ctx, pin: &str) -> Option<Approval> {
    let verifier = lock(&ctx.admin.verifier).clone()?;
    let pin = SecretPin::new(pin);
    tokio::task::spawn_blocking(move || {
        let found = verifier
            .entries
            .iter()
            .take(MAX_VERIFIER_ENTRIES)
            .find(|e| verify_pin(pin.expose(), &e.pin_hash))
            .map(|e| e.staff_id.clone());
        drop(pin);
        found.map(|staff_id| Approval {
            staff_id,
            mode: AdminExitMode::CachedVerifier,
            verifier_version: verifier.version,
        })
    })
    .await
    .ok()
    .flatten()
}

/// Whether `pin` matches the argon2id PHC string `hash` (a malformed hash never does).
pub fn verify_pin(pin: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    parsed.algorithm == argon2::ARGON2ID_IDENT
        && Argon2::default()
            .verify_password(pin.as_bytes(), &parsed)
            .is_ok()
}

fn device_id(ctx: &Ctx) -> String {
    ctx.config()
        .map(|c| c.device_id)
        .unwrap_or_else(|| "unprovisioned".into())
}

/// Handles the app's `admin_exit{pin}`: the exit's `until` when granted.
pub async fn request(ctx: &Arc<Ctx>, mut pin: SecretPin) -> Result<i64, Refused> {
    // One attempt at a time: each one passes the throttle only after the previous one counted,
    // so parallel attempts cannot get past the limit.
    let _turn = ctx.admin.attempt.lock().await;
    let now = ctx.clock.now_ms();
    if let Gate::Paused { seconds } = lock(&ctx.admin.throttle).gate(now) {
        pin.clear();
        return Err(Refused::Locked(seconds));
    }
    if ctx.admin.suspended() {
        pin.clear();
        return Err(Refused::Internal("an admin exit is already running".into()));
    }
    let approval = match check_online(ctx, pin.expose()).await {
        Online::Approved(a) => Some(a),
        Online::Refused(Refused::Rejected) => None,
        Online::Refused(_) => {
            // The Hub paused this device: so does the prompt here (a restart of the app or a
            // fall-back to the offline check cannot get around it).
            pin.clear();
            let gate = lock(&ctx.admin.throttle).pause(ctx.clock.now_ms());
            tracing::warn!(device_id = %device_id(ctx), "admin exit paused by the hub after wrong PINs");
            return Err(match gate {
                Gate::Paused { seconds } => Refused::Locked(seconds),
                Gate::Open => Refused::Rejected,
            });
        }
        Online::Unavailable => check_offline(ctx, pin.expose()).await,
    };
    pin.clear();
    let now = ctx.clock.now_ms();
    let Some(a) = approval else {
        let (gate, count) = {
            let mut t = lock(&ctx.admin.throttle);
            let before = t.failures();
            let gate = t.fail(now);
            (gate, before + 1)
        };
        let device = device_id(ctx);
        tracing::warn!(device_id = %device, failures = count, "admin exit refused: wrong PIN");
        if let Err(e) = ctx.log_event(
            KioskLogLevel::Warn,
            ADMIN_EXIT_FAILED_CODE,
            &format!("failed admin exit attempt {count} in a row"),
        ) {
            tracing::error!(error = %e, "could not queue admin_exit_failed");
        }
        return Err(match gate {
            Gate::Paused { seconds } => Refused::Locked(seconds),
            Gate::Open => Refused::Rejected,
        });
    };
    lock(&ctx.admin.throttle).succeed();
    let exit_id = ulid::Ulid::generate().to_string();
    let until = grant(ctx, &exit_id, now).await?;
    let intent = crate::intents::device_admin_exit(
        &device_id(ctx),
        now,
        &exit_id,
        a.mode,
        &a.staff_id,
        match a.mode {
            AdminExitMode::CachedVerifier => ElevationMode::CachedVerifier,
            _ => ElevationMode::Hub,
        },
        a.verifier_version,
    );
    let pushed = lock(&ctx.queue).push(&intent);
    if let Err(e) = pushed {
        tracing::error!(error = %e, "could not queue device.admin_exit; not granting it");
        rollback(ctx).await;
        return Err(Refused::Internal("the exit could not be recorded".into()));
    }
    ctx.queued.notify_one();
    tracing::info!(exit_id = %exit_id, staff_id = %a.staff_id, mode = a.mode.as_str(), "admin exit granted");
    Ok(until)
}

/// Starts the exit `exit_id` at `now_ms`. Refused while another exit runs, when the exit cannot
/// be recorded in `queue.db`, or when the desktop does not start (the lockdown then stays).
/// Returns `until`.
pub async fn grant(ctx: &Arc<Ctx>, exit_id: &str, now_ms: i64) -> Result<i64, Refused> {
    if ctx.admin.suspended() {
        return Err(Refused::Internal("an admin exit is already running".into()));
    }
    let exit = ActiveExit::start(exit_id, now_ms);
    let until = exit.until;
    let recorded = serde_json::to_string(&exit)
        .map_err(|e| e.to_string())
        .and_then(|json| {
            lock(&ctx.queue)
                .kv_set(KV_ADMIN_EXIT, &json, 0)
                .map_err(|e| e.to_string())
        });
    if let Err(e) = recorded {
        tracing::error!(error = %e, "could not record the admin exit; not granting it");
        return Err(Refused::Internal("the exit could not be recorded".into()));
    }
    // Suspended before Explorer starts, so the enforcer never takes it for an intruder.
    ctx.admin.active.send_replace(Some(exit));
    let launcher = Arc::clone(&ctx.launcher);
    let started = match tokio::task::spawn_blocking(move || launcher.start_desktop()).await {
        Ok(r) => r,
        Err(e) => Err(format!("desktop start panicked: {e}")),
    };
    if let Err(e) = started {
        tracing::error!(detail = %e, "could not start the desktop; staying locked down");
        rollback(ctx).await;
        return Err(Refused::Internal("the desktop could not be started".into()));
    }
    Ok(until)
}

/// Undoes a grant that did not complete: locked down again, nothing reported.
async fn rollback(ctx: &Arc<Ctx>) {
    if ctx.admin.active.send_replace(None).is_none() {
        return;
    }
    if let Err(e) = lock(&ctx.queue).kv_set(KV_ADMIN_EXIT, "", 0) {
        tracing::error!(error = %e, "could not clear the admin exit record");
    }
    let launcher = Arc::clone(&ctx.launcher);
    let _ = tokio::task::spawn_blocking(move || launcher.stop_desktop()).await;
}

/// Ends the running exit, if any: Explorer stops, enforcement resumes, the app locks down and
/// `device.admin_relock` is queued.
pub async fn relock(ctx: &Arc<Ctx>) {
    let Some(exit) = ctx.admin.active.send_replace(None) else {
        return;
    };
    finish(ctx, &exit).await;
}

async fn finish(ctx: &Arc<Ctx>, exit: &ActiveExit) {
    if let Err(e) = lock(&ctx.queue).kv_set(KV_ADMIN_EXIT, "", 0) {
        tracing::error!(error = %e, "could not clear the admin exit record");
    }
    let launcher = Arc::clone(&ctx.launcher);
    if let Err(e) = tokio::task::spawn_blocking(move || launcher.stop_desktop()).await {
        tracing::error!(error = %e, "desktop stop panicked");
    }
    let intent =
        crate::intents::device_admin_relock(&device_id(ctx), ctx.clock.now_ms(), &exit.exit_id);
    match lock(&ctx.queue).push(&intent) {
        Ok(()) => ctx.queued.notify_one(),
        Err(e) => tracing::error!(error = %e, "could not queue device.admin_relock"),
    }
    tracing::info!(exit_id = %exit.exit_id, "admin exit over; locked down again");
}

/// The exit recorded in `queue.db`, if any.
fn recorded(ctx: &Ctx) -> Option<ActiveExit> {
    let (json, _) = lock(&ctx.queue).kv_get(KV_ADMIN_EXIT).ok().flatten()?;
    if json.is_empty() {
        return None;
    }
    serde_json::from_str(&json).ok()
}

/// A remote unlock from the Hub: grant it and acknowledge. An unlock that cannot be granted
/// (an exit already runs, or the grant failed) is not acknowledged and is reported re-locked at
/// once, so the Hub's row closes; one with a malformed id is ignored.
pub async fn grant_remote(ctx: &Arc<Ctx>, client: &HubClient, exit_id: String) {
    if let Err(e) = arena_proto::validate_exit_id(&exit_id) {
        tracing::warn!(error = %e.message, "ignoring a remote unlock with a malformed exit id");
        return;
    }
    let until = match grant(ctx, &exit_id, ctx.clock.now_ms()).await {
        Ok(until) => until,
        Err(r) => {
            tracing::warn!(exit_id = %exit_id, refused = ?r, "remote unlock refused; reporting it re-locked");
            let intent =
                crate::intents::device_admin_relock(&device_id(ctx), ctx.clock.now_ms(), &exit_id);
            match lock(&ctx.queue).push(&intent) {
                Ok(()) => ctx.queued.notify_one(),
                Err(e) => tracing::error!(error = %e, "could not queue device.admin_relock"),
            }
            return;
        }
    };
    tracing::info!(exit_id = %exit_id, until, "remote unlock granted");
    let body = serde_json::json!({ "exit_id": exit_id });
    if let Err(e) = client.request("kiosk.admin_unlock_ack", body).await {
        tracing::warn!(error = %e, "could not acknowledge the remote unlock");
    }
}

/// Re-locks a recorded exit at once, then re-locks each exit when it is due, until `stop`.
pub(crate) async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    if let Some(exit) = arena_kiosk_core::admin_exit::on_restart(recorded(&ctx)) {
        tracing::warn!(exit_id = %exit.exit_id, "an admin exit was running when the service stopped; re-locking now");
        finish(&ctx, &exit).await;
    }
    let mut active = ctx.admin.active.subscribe();
    loop {
        let due = active.borrow_and_update().clone();
        let wait = match &due {
            Some(e) if e.is_due(ctx.clock.now_ms()) => {
                relock(&ctx).await;
                continue;
            }
            Some(e) => {
                let left = u64::try_from(e.until - ctx.clock.now_ms()).unwrap_or(0);
                std::time::Duration::from_millis(left).min(ctx.timings.admin_tick)
            }
            None => ctx.timings.admin_tick,
        };
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            changed = active.changed() => if changed.is_err() { return; },
            _ = stop.changed() => return,
        }
    }
}

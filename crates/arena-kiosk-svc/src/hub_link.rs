//! The service's link to the Hub: pinned TLS through `HubClient`, `hello{class: paired}`, the
//! offline queue replay, and reconnecting.
//!
//! - **Unreachable / closed:** the app is told `reconnecting` and the link retries with the
//!   jittered [`Backoff`] (2–30 s, reset after a successful hello).
//! - **Pin mismatch:** refused, with an `error` line naming both fingerprints. There is no
//!   unpinned fallback; the link keeps retrying with back-off (the pinned Hub may come back).
//! - **`TOKEN_REVOKED` / `TOKEN_INVALID`:** the app is told `token_rejected` and fast
//!   reconnects stop: one try every 30 s until a try succeeds (after a re-provision).
//! - **Replay:** on every connect, and whenever an event is queued, the queue is sent oldest
//!   first, one at a time ([`arena_kiosk_core::queue`] has the rules).
//! - **Settings:** after every hello the link asks `settings.get` for the process lists and
//!   mode (waiting at most [`SETTINGS_TIMEOUT`]) and applies the answer; a pushed
//!   `settings.changed` applies the same way, live. A failed or slow `settings.get` keeps the
//!   cached values (they still apply offline). The applied revisions start from the cached
//!   ones, so an older revision never replaces a newer one. This kiosk's own high contrast
//!   (`display.hc.<device_id>`) is read and applied the same way ([`crate::theme`]).
//! - **Library:** the branch game and app library (`kiosk.library`, Story 6.3) is read and applied
//!   the same way and cached for cold starts ([`crate::library`]).
//! - **Branch time zone:** `branch.tz` is read and applied the same way and cached; the daily
//!   scan ([`crate::scan::schedule`]) runs at 05:00 in it.
//! - **Admin exit:** the live client is shared with [`crate::admin_exit`] (online PIN checks);
//!   a pushed `verifier` is stored when newer, and `admin_unlock` grants a remote exit and is
//!   acknowledged.

use std::sync::Arc;

use arena_kiosk_core::queue::{ReplayStep, classify};
use arena_kiosk_core::{Backoff, ClientError, HubClient};
use arena_proto::{ClientClass, ErrorCode, FrameBody, Hello, HubEvent, KioskHubState, Versions};
use tokio::sync::watch;

use crate::procs::{AppliedRevs, PROCESS_KEYS, apply_settings};
use crate::runtime::Ctx;

/// How long the link waits for the `settings.get` reply after hello.
pub const SETTINGS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How one connection attempt ended.
enum Ended {
    /// It connected, then the connection closed.
    Closed,
    /// It never connected.
    Failed,
    /// The Hub refused the token.
    Rejected,
    /// Nothing to connect with yet.
    NotProvisioned,
    Shutdown,
}

fn is_token_refusal(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::TokenRevoked | ErrorCode::TokenInvalid)
}

fn seed(ctx: &Ctx) -> u64 {
    (ctx.clock.now_ms() as u64) ^ (u64::from(std::process::id()) << 32)
}

/// Runs the link until `stop` is set.
pub(crate) async fn run(ctx: Arc<Ctx>, mut stop: watch::Receiver<bool>) {
    let t = ctx.timings.clone();
    let mut backoff = Backoff::with_bounds(seed(&ctx), t.backoff_min, t.backoff_max);
    let mut rejected = false;
    let mut warned_unprovisioned = false;
    loop {
        if *stop.borrow() {
            return;
        }
        if *ctx.link_hold.borrow() {
            // Held down (dev fixtures): unreachable until released, then connect at once.
            ctx.set_hub_state(KioskHubState::Reconnecting);
            let mut hold = ctx.link_hold.subscribe();
            tokio::select! {
                () = async { let _ = hold.wait_for(|held| !*held).await; } => {}
                _ = stop.changed() => return,
            }
            continue;
        }
        let ended = attempt(&ctx, &mut backoff, &mut stop).await;
        let wait = match ended {
            Ended::Shutdown => return,
            Ended::Rejected => {
                rejected = true;
                ctx.set_hub_state(KioskHubState::TokenRejected);
                t.rejected_retry
            }
            Ended::NotProvisioned => {
                if !warned_unprovisioned {
                    tracing::info!("unpaired: no config.json or token; waiting for pairing");
                    warned_unprovisioned = true;
                }
                ctx.set_hub_state(KioskHubState::Unpaired);
                tokio::select! {
                    () = tokio::time::sleep(t.rejected_retry) => {}
                    () = ctx.provisioned.notified() => {}
                    _ = stop.changed() => return,
                }
                continue;
            }
            Ended::Closed => {
                rejected = false;
                ctx.set_hub_state(KioskHubState::Reconnecting);
                backoff.next_delay()
            }
            // A rejected token stays rejected (and slow) until a hello succeeds.
            Ended::Failed if rejected => t.rejected_retry,
            Ended::Failed => {
                ctx.set_hub_state(KioskHubState::Reconnecting);
                backoff.next_delay()
            }
        };
        tracing::debug!(wait_ms = wait.as_millis() as u64, "next hub connect");
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
    }
}

fn load_token(ctx: &Ctx) -> Option<String> {
    match ctx.secrets.load() {
        Ok(Some(bytes)) => String::from_utf8(bytes)
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty()),
        Ok(None) => None,
        Err(e) => {
            tracing::error!(error = %e, "the device token cannot be read");
            None
        }
    }
}

async fn attempt(ctx: &Arc<Ctx>, backoff: &mut Backoff, stop: &mut watch::Receiver<bool>) -> Ended {
    let (Some(cfg), Some(token)) = (ctx.config(), load_token(ctx)) else {
        return Ended::NotProvisioned;
    };
    let hello = Hello {
        credential: token,
        class: ClientClass::Paired,
        versions: Versions {
            app: env!("CARGO_PKG_VERSION").to_owned(),
        },
        capabilities: Vec::new(),
    };
    let url = cfg.hub_url();
    let client = tokio::select! {
        r = HubClient::connect(&url, &cfg.hub_fingerprint, hello) => r,
        _ = stop.changed() => return Ended::Shutdown,
    };
    let client = match client {
        Ok(c) => c,
        Err(ClientError::PinMismatch {
            expected,
            presented,
        }) => {
            tracing::error!(
                hub = %cfg.hub_addr,
                expected_fingerprint = %expected,
                presented_fingerprint = %presented,
                "the hub certificate does not match the pinned fingerprint; refused (no fallback)"
            );
            return Ended::Failed;
        }
        Err(ClientError::Hub(e)) if is_token_refusal(e.code) => {
            tracing::error!(
                code = e.code.as_str(),
                device_id = %cfg.device_id,
                "the hub refused the device token; provision this kiosk again"
            );
            return Ended::Rejected;
        }
        Err(e) => {
            tracing::warn!(hub = %cfg.hub_addr, error = %e, "hub unreachable");
            return Ended::Failed;
        }
    };
    backoff.reset();
    ctx.set_hub_state(KioskHubState::Connected);
    let client = Arc::new(client);
    *ctx.admin.hub.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::clone(&client));
    let ended = serve(ctx, &client, stop).await;
    *ctx.admin.hub.lock().unwrap_or_else(|p| p.into_inner()) = None;
    ended
}

/// While connected: replay the queue, then wait for new events, the close or shutdown.
async fn serve(ctx: &Arc<Ctx>, client: &HubClient, stop: &mut watch::Receiver<bool>) -> Ended {
    let mut events = client.events();
    let device_id = ctx.config().map(|c| c.device_id);
    let mut revs = {
        let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        let mut revs = AppliedRevs::from_cache(&q);
        if let Some(id) = &device_id {
            revs.seed(&q, &crate::theme::key_for(id));
        }
        crate::operating::seed(&q, &mut revs);
        crate::library::seed(&q, &mut revs);
        crate::scan::schedule::seed(&q, &mut revs);
        revs
    };
    tokio::select! {
        () = fetch_settings(ctx, client, &mut revs) => {}
        _ = stop.changed() => return Ended::Shutdown,
    }
    if let Some(id) = &device_id {
        tokio::select! {
            () = fetch_theme(ctx, client, id, &mut revs) => {}
            _ = stop.changed() => return Ended::Shutdown,
        }
    }
    tokio::select! {
        () = fetch_operating(ctx, client, &mut revs) => {}
        _ = stop.changed() => return Ended::Shutdown,
    }
    tokio::select! {
        () = fetch_library(ctx, client, &mut revs) => {}
        _ = stop.changed() => return Ended::Shutdown,
    }
    tokio::select! {
        () = fetch_branch_tz(ctx, client, &mut revs) => {}
        _ = stop.changed() => return Ended::Shutdown,
    }
    // Set once when a replay stops; new events do not push it back.
    let mut retry_at: Option<tokio::time::Instant> = None;
    let mut hold = ctx.link_hold.subscribe();
    let mut skew_warned = false;
    loop {
        if retry_at.is_none() && replay(ctx, client, &mut skew_warned).await == ReplayStep::Stop {
            retry_at = Some(tokio::time::Instant::now() + ctx.timings.replay_retry);
        }
        tokio::select! {
            closed = client.closed() => {
                return match closed.error {
                    Some(e) if is_token_refusal(e.code) => {
                        tracing::error!(code = e.code.as_str(), "the hub closed the link: token refused");
                        Ended::Rejected
                    }
                    _ => {
                        tracing::warn!("the hub link closed");
                        Ended::Closed
                    }
                };
            }
            () = ctx.queued.notified() => {}
            () = async { let _ = hold.wait_for(|held| *held).await; } => {
                tracing::info!("the hub link is held down; closing it");
                return Ended::Closed;
            }
            () = async {
                match retry_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => retry_at = None,
            ev = async {
                match events.as_mut() {
                    Some(e) => e.next_event().await,
                    None => std::future::pending().await,
                }
            } => {
                match ev {
                    Some(HubEvent::SettingsChanged(values)) => {
                        apply_settings(ctx, &values.values, &mut revs);
                        crate::operating::apply(ctx, &values.values, &mut revs);
                        crate::library::apply(ctx, &values.values, &mut revs);
                        crate::scan::schedule::apply(ctx, &values.values, &mut revs);
                        if let Some(id) = &device_id {
                            crate::theme::apply(ctx, id, &values.values, &mut revs);
                        }
                    }
                    Some(HubEvent::Verifier { verifier }) => {
                        crate::admin_exit::store_verifier(ctx, verifier);
                    }
                    Some(HubEvent::AdminUnlock { exit_id }) => {
                        crate::admin_exit::grant_remote(ctx, client, exit_id).await;
                    }
                    Some(HubEvent::PcCommand {
                        cmd_id,
                        action,
                        text,
                        reason,
                    }) => {
                        pc_command(ctx, client, cmd_id, action, text, reason).await;
                    }
                    Some(HubEvent::SessionStarted { cmd_id, session }) => {
                        crate::session::sync(ctx, session, true);
                        ack(client, &cmd_id).await;
                    }
                    Some(HubEvent::SessionTick(session)) => {
                        crate::session::sync(ctx, session, false);
                    }
                    Some(HubEvent::SessionWarning { minutes_left, .. }) => {
                        crate::session::warn(ctx, minutes_left);
                    }
                    Some(HubEvent::TestSession { lock_after_ms }) => {
                        crate::test_session::begin(ctx, lock_after_ms);
                    }
                    Some(HubEvent::Applied(_) | HubEvent::CatalogChanged { .. }) => {}
                    None => events = None,
                }
            }
            _ = stop.changed() => return Ended::Shutdown,
        }
    }
}

async fn ack(client: &HubClient, cmd_id: &str) {
    let body = serde_json::json!({ "cmd_id": cmd_id });
    if let Err(e) = client.request("kiosk.pc_ack", body).await {
        tracing::warn!(error = %e, "could not acknowledge the pc command");
    }
}

async fn pc_command(
    ctx: &Ctx,
    client: &HubClient,
    cmd_id: String,
    action: arena_proto::PcAction,
    text: Option<String>,
    reason: Option<arena_proto::LockReason>,
) {
    let step = crate::pc::apply(ctx, &cmd_id, action, text, reason);
    if step == crate::pc::Step::Refused {
        return;
    }
    ack(client, &cmd_id).await;
    if let crate::pc::Step::AckThenPower(action) = step
        && let Err(e) = crate::power::run(action)
    {
        tracing::error!(error = %e, action = action.as_str(), "the power command failed");
    }
}

/// Asks the Hub for the process lists and mode and applies them. A failure, or no answer
/// within [`SETTINGS_TIMEOUT`], is logged and the cached values stay in effect.
async fn fetch_settings(ctx: &Ctx, client: &HubClient, revs: &mut AppliedRevs) {
    let body = serde_json::json!({ "keys": PROCESS_KEYS });
    match tokio::time::timeout(SETTINGS_TIMEOUT, client.request("settings.get", body)).await {
        Ok(Ok(frame)) => match frame.body {
            FrameBody::SettingsValues(v) => apply_settings(ctx, &v.values, revs),
            other => tracing::warn!(kind = other.kind(), "unexpected reply to settings.get"),
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "settings.get failed; keeping the cached process settings")
        }
        Err(_) => tracing::warn!(
            timeout_ms = SETTINGS_TIMEOUT.as_millis() as u64,
            "settings.get timed out; keeping the cached process settings"
        ),
    }
}

/// Asks the Hub for this kiosk's high contrast (its own `display.hc.<device_id>`, on its own so
/// a Hub that does not know the key yet never costs the process settings) and applies it. A
/// failure, or no answer within [`SETTINGS_TIMEOUT`], keeps the cached value.
async fn fetch_theme(ctx: &Ctx, client: &HubClient, device_id: &str, revs: &mut AppliedRevs) {
    let body = serde_json::json!({ "keys": [crate::theme::key_for(device_id)] });
    match tokio::time::timeout(SETTINGS_TIMEOUT, client.request("settings.get", body)).await {
        Ok(Ok(frame)) => match frame.body {
            FrameBody::SettingsValues(v) => crate::theme::apply(ctx, device_id, &v.values, revs),
            other => tracing::warn!(kind = other.kind(), "unexpected reply to settings.get"),
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "settings.get failed; keeping the cached high contrast")
        }
        Err(_) => tracing::warn!(
            timeout_ms = SETTINGS_TIMEOUT.as_millis() as u64,
            "settings.get timed out; keeping the cached high contrast"
        ),
    }
}

async fn fetch_operating(ctx: &Ctx, client: &HubClient, revs: &mut AppliedRevs) {
    let body = serde_json::json!({ "keys": crate::operating::KEYS });
    match tokio::time::timeout(SETTINGS_TIMEOUT, client.request("settings.get", body)).await {
        Ok(Ok(frame)) => match frame.body {
            FrameBody::SettingsValues(v) => crate::operating::apply(ctx, &v.values, revs),
            other => tracing::warn!(kind = other.kind(), "unexpected reply to settings.get"),
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "settings.get failed; keeping the cached operating settings")
        }
        Err(_) => tracing::warn!(
            timeout_ms = SETTINGS_TIMEOUT.as_millis() as u64,
            "settings.get timed out; keeping the cached operating settings"
        ),
    }
}

/// Asks the Hub for the branch game library and caches it. A failure, or no answer within
/// [`SETTINGS_TIMEOUT`], keeps the cached library (it still applies offline).
async fn fetch_library(ctx: &Ctx, client: &HubClient, revs: &mut AppliedRevs) {
    let body = serde_json::json!({ "keys": crate::library::KEYS });
    match tokio::time::timeout(SETTINGS_TIMEOUT, client.request("settings.get", body)).await {
        Ok(Ok(frame)) => match frame.body {
            FrameBody::SettingsValues(v) => crate::library::apply(ctx, &v.values, revs),
            other => tracing::warn!(kind = other.kind(), "unexpected reply to settings.get"),
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "settings.get failed; keeping the cached game library")
        }
        Err(_) => tracing::warn!(
            timeout_ms = SETTINGS_TIMEOUT.as_millis() as u64,
            "settings.get timed out; keeping the cached game library"
        ),
    }
}

/// Asks the Hub for the branch time zone (the daily scan runs at 05:00 in it) and caches it. A
/// failure, or no answer within [`SETTINGS_TIMEOUT`], keeps the cached zone (unknown means UTC).
async fn fetch_branch_tz(ctx: &Ctx, client: &HubClient, revs: &mut AppliedRevs) {
    let body = serde_json::json!({ "keys": crate::scan::schedule::KEYS });
    match tokio::time::timeout(SETTINGS_TIMEOUT, client.request("settings.get", body)).await {
        Ok(Ok(frame)) => match frame.body {
            FrameBody::SettingsValues(v) => crate::scan::schedule::apply(ctx, &v.values, revs),
            other => tracing::warn!(kind = other.kind(), "unexpected reply to settings.get"),
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "settings.get failed; keeping the cached branch time zone")
        }
        Err(_) => tracing::warn!(
            timeout_ms = SETTINGS_TIMEOUT.as_millis() as u64,
            "settings.get timed out; keeping the cached branch time zone"
        ),
    }
}

/// Sends the queue oldest first, one at a time. Returns [`ReplayStep::Stop`] when a transient
/// failure stopped it, otherwise [`ReplayStep::Applied`] once the queue is empty.
async fn replay(ctx: &Ctx, client: &HubClient, skew_warned: &mut bool) -> ReplayStep {
    loop {
        let next = {
            let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
            match q.drop_expired(ctx.clock.now_ms()) {
                Ok(keys) => {
                    for key in keys {
                        tracing::warn!(idempotency_key = %key, "dropping a queued kiosk event older than 7 days");
                    }
                }
                Err(e) => tracing::error!(error = %e, "queue ttl sweep failed"),
            }
            match q.oldest() {
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "queue read failed");
                    return ReplayStep::Stop;
                }
            }
        };
        let Some(entry) = next else {
            return ReplayStep::Applied;
        };
        let key = entry.intent.idempotency_key.clone();
        let result = client.submit(entry.intent).await;
        let step = classify(&result);
        match (&step, &result) {
            (ReplayStep::Applied, _) => {
                tracing::debug!(idempotency_key = %key, "queued intent applied");
            }
            (ReplayStep::Rejected, Err(e)) => {
                tracing::warn!(
                    idempotency_key = %key,
                    code = e.code().map_or("?", ErrorCode::as_str),
                    "the hub refused a queued intent for good; dropping it"
                );
            }
            (_, Err(e)) if arena_kiosk_core::queue::is_clock_skew(e) => {
                // Kept: fixing this kiosk's clock lets it through. Warned once per connection.
                if !*skew_warned {
                    *skew_warned = true;
                    tracing::warn!(
                        idempotency_key = %key,
                        detail = %e,
                        "this kiosk's clock is ahead of the hub; queued events wait until it is fixed"
                    );
                }
                return ReplayStep::Stop;
            }
            (_, r) => {
                tracing::info!(
                    idempotency_key = %key,
                    error = %r.as_ref().err().map(ToString::to_string).unwrap_or_default(),
                    "queue replay paused"
                );
                return ReplayStep::Stop;
            }
        }
        let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = q.delete(entry.seq) {
            tracing::error!(error = %e, "queue delete failed");
            return ReplayStep::Stop;
        }
    }
}

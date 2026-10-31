//! Following the branch to a new Hub (Story 10.19).
//!
//! A Hub that hands the branch to another pushes `hub.successor`: the new Hub's address and
//! certificate fingerprint, the branch and an expiry, signed with the key of the certificate this
//! kiosk pins ([`arena_kiosk_core::successor::verify`]). The kiosk keeps a good one beside its pin
//! (`queue.db`, key `hub.successor`) and uses it only when its own Hub refuses, closes or cannot be
//! reached: it connects to the successor, accepts only the named fingerprint, presents its
//! existing token and, once the successor says hello, replaces its pin (`config.json`) and drops
//! the successor. A bad successor is dropped, the old pin stays and the reason is queued as a
//! `kiosk.log` event (`successor_rejected`), which the next Hub the kiosk reaches records.

use arena_kiosk_core::successor::{Rejection, verify};
use arena_proto::{KioskLogLevel, SignedSuccessor};

use crate::config::KioskConfig;
use crate::runtime::Ctx;

const SUCCESSOR_KEY: &str = "hub.successor";
const BRANCH_KEY: &str = "hub.branch_id";

/// The `kiosk.log` code a rejected successor is reported under.
pub const REJECTED_CODE: &str = "successor_rejected";

fn read(ctx: &Ctx, key: &str) -> Option<String> {
    let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
    match q.kv_get(key) {
        Ok(v) => v.map(|(value, _)| value).filter(|v| !v.is_empty()),
        Err(e) => {
            tracing::warn!(error = %e, key, "the kiosk cache cannot be read");
            None
        }
    }
}

fn write(ctx: &Ctx, key: &str, value: &str) {
    let q = ctx.queue.lock().unwrap_or_else(|p| p.into_inner());
    if let Err(e) = q.kv_set(key, value, 0) {
        tracing::warn!(error = %e, key, "the kiosk cache cannot be written");
    }
}

/// The branch the Hub last said it serves.
pub fn branch_id(ctx: &Ctx) -> Option<String> {
    read(ctx, BRANCH_KEY)
}

/// Keeps the branch a Hub named in its hello.
pub fn remember_branch(ctx: &Ctx, branch_id: Option<&str>) {
    if let Some(id) = branch_id
        && self::branch_id(ctx).as_deref() != Some(id)
    {
        write(ctx, BRANCH_KEY, id);
    }
}

/// Forgets the stored successor.
pub fn clear(ctx: &Ctx) {
    if read(ctx, SUCCESSOR_KEY).is_some() {
        write(ctx, SUCCESSOR_KEY, "");
    }
}

fn reject(ctx: &Ctx, why: &str) {
    tracing::warn!(
        reason = why,
        "hub.successor ignored; keeping the pinned hub"
    );
    let _ = ctx.log_event(KioskLogLevel::Warn, REJECTED_CODE, why);
}

/// A pushed `hub.successor`: stored when it checks out against the pin, the branch and the clock,
/// otherwise ignored and reported.
pub fn receive(ctx: &Ctx, hello_branch: Option<&str>, signed: SignedSuccessor) {
    let Some(cfg) = ctx.config() else { return };
    let branch = hello_branch.map(str::to_owned).or_else(|| branch_id(ctx));
    match verify(
        &signed,
        &cfg.hub_fingerprint,
        branch.as_deref(),
        ctx.clock.now_ms(),
    ) {
        Ok(()) => match serde_json::to_string(&signed) {
            Ok(text) => {
                tracing::info!(
                    address = %signed.successor.address,
                    "the branch is moving; successor stored beside the pin"
                );
                write(ctx, SUCCESSOR_KEY, &text);
            }
            Err(e) => tracing::error!(error = %e, "could not encode the successor"),
        },
        Err(r) => reject(ctx, r.as_str()),
    }
}

/// The stored successor, checked again against the current pin and clock. One that no longer
/// checks out is dropped (and reported, unless it merely ran out).
pub fn current(ctx: &Ctx, cfg: &KioskConfig) -> Option<SignedSuccessor> {
    let text = read(ctx, SUCCESSOR_KEY)?;
    let Ok(signed) = serde_json::from_str::<SignedSuccessor>(&text) else {
        clear(ctx);
        return None;
    };
    match verify(
        &signed,
        &cfg.hub_fingerprint,
        branch_id(ctx).as_deref(),
        ctx.clock.now_ms(),
    ) {
        Ok(()) => Some(signed),
        Err(Rejection::Expired) => {
            clear(ctx);
            None
        }
        Err(r) => {
            clear(ctx);
            reject(ctx, r.as_str());
            None
        }
    }
}

/// Whether a successor waits (the link then retries at its minimum back-off).
pub fn pending(ctx: &Ctx) -> bool {
    ctx.config().is_some_and(|cfg| current(ctx, &cfg).is_some())
}

/// Reports a successor the kiosk reached but could not use, and forgets it.
pub fn refused(ctx: &Ctx, why: &str) {
    clear(ctx);
    reject(ctx, why);
}

/// The successor said hello: its address and fingerprint become the pin, the token stays, and the
/// successor is spent.
pub fn adopt(ctx: &Ctx, cfg: &KioskConfig, signed: &SignedSuccessor) {
    let next = KioskConfig {
        hub_addr: signed.successor.address.clone(),
        hub_fingerprint: signed.successor.fingerprint.to_ascii_lowercase(),
        ..cfg.clone()
    };
    let saved = next
        .validate()
        .map_err(std::io::Error::other)
        .and_then(|()| next.save(&ctx.data.config()));
    match saved {
        Ok(()) => tracing::info!(
            hub = %next.hub_addr,
            "followed the branch to its new hub; the pin was replaced"
        ),
        Err(e) => tracing::error!(error = %e, "the new hub's pin could not be saved"),
    }
    write(ctx, BRANCH_KEY, &signed.successor.branch_id);
    clear(ctx);
}

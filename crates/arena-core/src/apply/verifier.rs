//! The manager verifier's version history (Story 1.20).
//!
//! The verifier a kiosk caches for offline admin exits lists the active staff with a PIN whose
//! role allows `device.remote`. Whenever that list changes (who is on it, their PIN hash or PIN
//! version), a new version is recorded in `verifier_history` with the time it took effect, in
//! the same transaction as the change that caused it. The version is
//! `max(pin_verifier_version over all staff) + hub_meta.verifier_counter`, the counter bumped
//! on every change; it never goes down (and is at least one above the previous version).
//! `apply()` reads the history to flag an offline exit whose cached verifier was already
//! out of date when the exit happened.

use arena_proto::{AppError, MAX_VERIFIER_ENTRIES, Permission};
use arena_store::{
    META_VERIFIER_COUNTER, PinCandidate, StoreTx, get_meta, insert_verifier_version,
    latest_verifier, list_pin_candidates, max_pin_verifier_version, set_meta,
};
use sha2::{Digest, Sha256};

/// The staff the verifier lists, as stored now.
pub(crate) fn entries(tx: &StoreTx<'_>) -> Result<Vec<PinCandidate>, AppError> {
    list_pin_candidates(
        tx,
        &[Permission::DeviceRemote.as_str()],
        "",
        MAX_VERIFIER_ENTRIES,
    )
}

/// A SHA-256 (hex) over the entries, in id order.
pub(crate) fn digest(entries: &[PinCandidate]) -> String {
    let mut h = Sha256::new();
    for e in entries {
        h.update(e.staff_id.as_bytes());
        h.update([0]);
        h.update(e.pin_hash.as_bytes());
        h.update([0]);
        h.update(e.pin_verifier_version.to_string().as_bytes());
        h.update([b'\n']);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Records a new verifier version effective `now_ms` when the list changed. Returns it.
pub(crate) fn refresh(tx: &StoreTx<'_>, now_ms: i64) -> Result<Option<i64>, AppError> {
    let now = entries(tx)?;
    let d = digest(&now);
    let last = latest_verifier(tx)?;
    match &last {
        Some((_, old)) if *old == d => return Ok(None),
        None if now.is_empty() => return Ok(None),
        _ => {}
    }
    let counter = get_meta(tx, META_VERIFIER_COUNTER)?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        .saturating_add(1);
    set_meta(tx, META_VERIFIER_COUNTER, &counter.to_string())?;
    let floor = last.map_or(1, |(v, _)| v.saturating_add(1));
    let version = max_pin_verifier_version(tx)?
        .saturating_add(counter)
        .max(floor);
    let ids: Vec<String> = now.iter().map(|e| e.staff_id.clone()).collect();
    insert_verifier_version(tx, version, now_ms, &d, &ids)?;
    tracing::info!(version, entries = now.len(), "manager verifier changed");
    Ok(Some(version))
}

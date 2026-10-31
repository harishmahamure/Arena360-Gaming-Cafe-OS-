//! Device standing against the plan's device limit (Story 10.12).
//!
//! Recomputed inside the transaction of `licence.apply` and `device.revoke`: the live devices
//! (the ones the cap of Story 2.19 counts) are ordered by pairing, the oldest up to
//! `licence.device_cap` stay active and the rest are marked `over_cap`, newest pairing first. A
//! device changes only when its flag changes, so a recompute that finds nothing to move writes
//! nothing. A session is never read or written here: a marked device keeps running what it has
//! and is refused only new starts and transfers onto it.

use arena_proto::AppError;
use arena_store::{ChangeOp, RowKey, StoreTx, live_devices_by_age, set_device_over_cap};

use super::mutate::Written;

pub(super) fn recompute(tx: &StoreTx<'_>) -> Result<Vec<Written>, AppError> {
    let cap = crate::entitlement::limit(tx, "devices")?
        .map_or(usize::MAX, |c| usize::try_from(c).unwrap_or(0));
    let mut written = Vec::new();
    for (i, (device_id, was)) in live_devices_by_age(tx)?.into_iter().enumerate() {
        let over = i >= cap;
        if over == was {
            continue;
        }
        set_device_over_cap(tx, &device_id, over)?;
        written.push(Written {
            key: RowKey::DevicesMeta { device_id },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

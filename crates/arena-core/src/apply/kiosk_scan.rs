//! `kiosk.scan_reported` (Story 6.4): a paired kiosk's installed-software scan (device actor
//! only, no `base_rev`, no reason). The Hub keeps the latest scan per device in `kiosk_scans`:
//! the first report inserts the row (rev 1) and every newer one replaces it. A report that is
//! not newer than the stored one (`scanned_at` ≤ stored) is refused `VALIDATION_FAILED` on
//! `scanned_at` and the stored scan stays. Like `kiosk.log`, a queued report is replayed in
//! order and the idempotency key makes it apply once.

use arena_proto::{AppError, ErrorCode, Intent, KioskScanReported};
use arena_store::{
    ChangeOp, KioskScanRow, KioskScanWrite, RowKey, StoreTx, find_kiosk_scan, insert_kiosk_scan,
    replace_kiosk_scan,
};

use super::Target;
use super::mutate::Written;

/// The branch and the device's stored scan.
pub(crate) struct ScanTarget {
    branch: Option<String>,
    stored: Option<KioskScanRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, intent: &Intent) -> Result<Target, AppError> {
    let Target::Branch(branch) = super::kiosk_log::load(tx)? else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the branch target did not load",
        ));
    };
    Ok(Target::KioskScan(Box::new(ScanTarget {
        branch,
        stored: find_kiosk_scan(tx, &intent.actor.id)?,
    })))
}

pub(super) fn validate(
    intent: &Intent,
    p: &KioskScanReported,
    t: &ScanTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "kiosk.scan_reported takes no base_rev"));
    }
    p.validate(intent.issued_at)?;
    if t.branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    if t.stored
        .as_ref()
        .is_some_and(|s| p.scanned_at <= s.scanned_at)
    {
        return Err(invalid(
            "scanned_at",
            "this PC already has a scan from the same time or later",
        ));
    }
    Ok(())
}

fn json<T: serde::Serialize>(v: &T) -> Result<String, AppError> {
    serde_json::to_string(v).map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &KioskScanReported,
    t: &ScanTarget,
) -> Result<Vec<Written>, AppError> {
    // The permission step checked that the actor is a live device.
    let device_id = intent.actor.id.clone();
    let entries = json(&p.entries)?;
    let launchers = json(&p.launchers)?;
    let fields = KioskScanWrite {
        scanned_at: p.scanned_at,
        entries: &entries,
        launchers: &launchers,
        skipped: i64::from(p.skipped),
    };
    let op = if t.stored.is_some() {
        replace_kiosk_scan(tx, &device_id, &fields)?;
        ChangeOp::Update
    } else {
        insert_kiosk_scan(tx, &device_id, &fields)?;
        ChangeOp::Insert
    };
    Ok(vec![Written {
        key: RowKey::KioskScans { device_id },
        op,
        before: None,
    }])
}

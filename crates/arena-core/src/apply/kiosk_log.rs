//! `kiosk.log` and `kiosk.process_blocked`: one event a paired kiosk recorded (device actor
//! only). The kiosk service queues these while the Hub is unreachable and replays them in
//! creation order; dedupe on the idempotency key makes each one apply exactly once. Each writes
//! one `device_events` row whose `at` is the intent's `issued_at` (when the kiosk recorded it).
//! A blocked process is stored as `level = warn`, `code = process_blocked` and the detail
//! `<exe> (<reason>): <path>`.
//!
//! `kiosk.cleanup_failed` (Story 6.11) writes an `error` row `cleanup_failed` with the detail
//! `<step>: <detail>` and, in the same transaction, a `check_pc` alert for the device unless one
//! is already open.

use arena_proto::{
    AlertKind, AppError, ErrorCode, Intent, KioskCleanupFailed, KioskLog, KioskProcessBlocked,
};
use arena_store::{
    ChangeOp, NewAlert, NewDeviceEvent, RowKey, StoreTx, current_branch_id, find_open_alert,
    insert_alert, insert_device_event,
};

use super::Target;
use super::mutate::Written;

pub use arena_proto::{MAX_KIOSK_LOG_CODE_LEN, MAX_KIOSK_LOG_DETAIL_CHARS};

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Target, AppError> {
    Ok(Target::Branch(current_branch_id(tx)?))
}

/// The `device_events.code` of a `kiosk.process_blocked` row.
pub const PROCESS_BLOCKED_CODE: &str = "process_blocked";
/// The `device_events.code` of a `kiosk.cleanup_failed` row.
pub const CLEANUP_FAILED_CODE: &str = "cleanup_failed";

/// A kiosk event's fields, from either payload.
pub(super) enum Event<'a> {
    Log(&'a KioskLog),
    ProcessBlocked(&'a KioskProcessBlocked),
    CleanupFailed(&'a KioskCleanupFailed),
}

pub(super) fn validate(
    intent: &Intent,
    event: Event<'_>,
    branch: Option<&String>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid(
            "base_rev",
            &format!("{} takes no base_rev", intent.kind.as_str()),
        ));
    }
    match event {
        Event::Log(log) => log.validate()?,
        Event::ProcessBlocked(p) => p.validate()?,
        Event::CleanupFailed(p) => p.validate()?,
    }
    if branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    event: Event<'_>,
    branch: Option<&String>,
) -> Result<Vec<Written>, AppError> {
    let branch_id = branch.ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            "a kiosk event validated without a branch",
        )
    })?;
    let detail;
    let (level, code, detail) = match event {
        Event::Log(log) => (log.level.as_str(), log.code.as_str(), log.detail.as_str()),
        Event::ProcessBlocked(p) => {
            detail = p.detail();
            ("warn", PROCESS_BLOCKED_CODE, detail.as_str())
        }
        Event::CleanupFailed(c) => {
            detail = c.event_detail();
            ("error", CLEANUP_FAILED_CODE, detail.as_str())
        }
    };
    let event_id = ulid::Ulid::generate().to_string();
    insert_device_event(
        tx,
        &NewDeviceEvent {
            event_id: &event_id,
            branch_id,
            // The permission step checked that the actor is a live device.
            device_id: &intent.actor.id,
            at: intent.issued_at,
            level,
            code,
            detail,
        },
    )?;
    let mut written = vec![Written {
        key: RowKey::DeviceEvents { event_id },
        op: ChangeOp::Insert,
        before: None,
    }];
    if matches!(event, Event::CleanupFailed(_)) {
        // In the same transaction: one open `check_pc` alert per device.
        let kind = AlertKind::CheckPc.as_str();
        if find_open_alert(tx, kind, Some(&intent.actor.id))?.is_none() {
            let id = ulid::Ulid::generate().to_string();
            insert_alert(
                tx,
                &NewAlert {
                    id: &id,
                    kind,
                    device_id: Some(&intent.actor.id),
                    raised_at: intent.issued_at,
                    product_id: None,
                    qty: None,
                    threshold: None,
                },
            )?;
            written.push(Written {
                key: RowKey::Alerts { id },
                op: ChangeOp::Insert,
                before: None,
            });
        }
    }
    Ok(written)
}

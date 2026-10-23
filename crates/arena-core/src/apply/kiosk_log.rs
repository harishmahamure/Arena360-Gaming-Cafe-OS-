//! `kiosk.log`: one event a paired kiosk recorded (device actor only). The kiosk service queues
//! these while the Hub is unreachable and replays them in creation order; dedupe on the
//! idempotency key makes each one apply exactly once. The row's `at` is the intent's
//! `issued_at` (when the kiosk recorded it).

use arena_proto::{AppError, ErrorCode, Intent, KioskLog};
use arena_store::{
    ChangeOp, NewDeviceEvent, RowKey, StoreTx, current_branch_id, insert_device_event,
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

pub(super) fn validate(
    intent: &Intent,
    log: &KioskLog,
    branch: Option<&String>,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "kiosk.log takes no base_rev"));
    }
    log.validate()?;
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
    log: &KioskLog,
    branch: Option<&String>,
) -> Result<Vec<Written>, AppError> {
    let branch_id = branch.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "kiosk.log validated without a branch")
    })?;
    let event_id = ulid::Ulid::generate().to_string();
    insert_device_event(
        tx,
        &NewDeviceEvent {
            event_id: &event_id,
            branch_id,
            // The permission step checked that the actor is a live device.
            device_id: &intent.actor.id,
            at: intent.issued_at,
            level: log.level.as_str(),
            code: &log.code,
            detail: &log.detail,
        },
    )?;
    Ok(vec![Written {
        key: RowKey::DeviceEvents { event_id },
        op: ChangeOp::Insert,
        before: None,
    }])
}

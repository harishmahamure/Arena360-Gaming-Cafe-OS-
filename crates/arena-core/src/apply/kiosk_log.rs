//! `kiosk.log` and `kiosk.process_blocked`: one event a paired kiosk recorded (device actor
//! only). The kiosk service queues these while the Hub is unreachable and replays them in
//! creation order; dedupe on the idempotency key makes each one apply exactly once. Each writes
//! one `device_events` row whose `at` is the intent's `issued_at` (when the kiosk recorded it).
//! A blocked process is stored as `level = warn`, `code = process_blocked` and the detail
//! `<exe> (<reason>): <path>`.

use arena_proto::{AppError, ErrorCode, Intent, KioskLog, KioskProcessBlocked};
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

/// The `device_events.code` of a `kiosk.process_blocked` row.
pub const PROCESS_BLOCKED_CODE: &str = "process_blocked";

/// A kiosk event's fields, from either payload.
pub(super) enum Event<'a> {
    Log(&'a KioskLog),
    ProcessBlocked(&'a KioskProcessBlocked),
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
    Ok(vec![Written {
        key: RowKey::DeviceEvents { event_id },
        op: ChangeOp::Insert,
        before: None,
    }])
}

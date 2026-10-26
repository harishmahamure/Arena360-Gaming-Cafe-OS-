use arena_proto::{
    ActorKind, AppError, ClientClass, ErrorCode, Intent, IntentPayload, KEY_TEST_SESSION,
    TestSessionOutcome, TestSessionState, TestSessionStatus,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, DeviceRow, RowKey, SettingRef, StoreTx, find_device, find_device_meta,
    find_setting, update_setting,
};

use super::Target;
use super::mutate::Written;
use crate::rbac;

#[derive(Default)]
pub(crate) struct SessionTarget {
    row: Option<SettingRef>,
    device: Option<DeviceRow>,
    meta: Option<DeviceMetaRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn device_id(p: &IntentPayload) -> &str {
    match p {
        IntentPayload::DeviceTestSessionStart(s) => &s.device_id,
        IntentPayload::DeviceTestSessionCompleted(c) => &c.device_id,
        _ => "",
    }
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<Target, AppError> {
    let id = device_id(p);
    Ok(Target::TestSession(Box::new(SessionTarget {
        row: find_setting(tx, KEY_TEST_SESSION)?,
        device: find_device(tx, id)?,
        meta: find_device_meta(tx, id)?.filter(|m| m.deleted_at.is_none()),
    })))
}

pub(super) fn authorize(intent: &Intent) -> Result<(), AppError> {
    let IntentPayload::DeviceTestSessionCompleted(c) = &intent.payload else {
        return Ok(());
    };
    let ok = match intent.actor.kind {
        ActorKind::Device => {
            c.device_id == intent.actor.id && c.outcome == TestSessionOutcome::Locked
        }
        ActorKind::System => c.outcome == TestSessionOutcome::TimedOut,
        _ => true,
    };
    if ok { Ok(()) } else { Err(rbac::denied()) }
}

fn status(t: &SessionTarget) -> Result<(&SettingRef, TestSessionStatus), AppError> {
    let row = t
        .row
        .as_ref()
        .ok_or_else(|| invalid("key", "no such setting"))?;
    Ok((
        row,
        TestSessionStatus::parse(&row.value).unwrap_or_else(TestSessionStatus::unset),
    ))
}

pub(super) fn validate(intent: &Intent, t: &SessionTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "a test session takes no base_rev"));
    }
    let (_, current) = status(t)?;
    match &intent.payload {
        IntentPayload::DeviceTestSessionStart(_) => {
            let Some(d) = &t.device else {
                return Err(invalid("device_id", "no such device"));
            };
            if d.revoked_at.is_some() {
                return Err(invalid("device_id", "the device is revoked"));
            }
            let Some(m) = t
                .meta
                .as_ref()
                .filter(|m| d.class == ClientClass::Paired.as_str() && m.kind == "pc")
            else {
                return Err(invalid(
                    "device_id",
                    "only a kiosk PC can run a test session",
                ));
            };
            if m.group_id.is_none() {
                return Err(invalid("device_id", "put the PC in a group first"));
            }
            if m.state == "maintenance" {
                return Err(invalid("device_id", "the PC is in maintenance"));
            }
            if current.active(intent.issued_at) {
                return Err(invalid("device_id", "a test session is already running"));
            }
            Ok(())
        }
        IntentPayload::DeviceTestSessionCompleted(c) => {
            if current.state == TestSessionState::Running
                && current.device_id.as_deref() == Some(c.device_id.as_str())
            {
                Ok(())
            } else {
                Err(invalid(
                    "device_id",
                    "no test session is running on that PC",
                ))
            }
        }
        _ => Ok(()),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &SessionTarget,
) -> Result<Vec<Written>, AppError> {
    let (row, current) = status(t)?;
    let next = match &intent.payload {
        IntentPayload::DeviceTestSessionStart(s) => TestSessionStatus {
            state: TestSessionState::Running,
            device_id: Some(s.device_id.clone()),
            at: Some(intent.issued_at),
        },
        IntentPayload::DeviceTestSessionCompleted(c) => TestSessionStatus {
            state: match c.outcome {
                TestSessionOutcome::Locked => TestSessionState::Passed,
                TestSessionOutcome::TimedOut => TestSessionState::Failed,
            },
            ..current
        },
        _ => return Err(AppError::new(ErrorCode::Internal, "not a test session")),
    };
    update_setting(tx, &row.branch_id, &row.key, &next.to_json(), None, None)?;
    Ok(vec![Written {
        key: RowKey::Settings {
            branch_id: row.branch_id.clone(),
            key: row.key.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}

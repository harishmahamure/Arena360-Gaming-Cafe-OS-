use arena_proto::{AppError, ErrorCode, Intent, IntentPayload};
use arena_store::{
    ChangeOp, DeviceMetaRow, NewDeviceEvent, RowKey, StoreTx, current_branch_id, find_device_meta,
    insert_device_event,
};

use super::mutate::Written;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PcTarget {
    branch: Option<String>,
    device: Option<DeviceMetaRow>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn parts(p: &IntentPayload) -> Option<(&str, &str, Option<&str>)> {
    match p {
        IntentPayload::DeviceScreen(s) => {
            Some((&s.device_id, s.action.as_str(), s.text.as_deref()))
        }
        IntentPayload::DevicePower(w) => Some((&w.device_id, w.action.as_str(), None)),
        _ => None,
    }
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<PcTarget, AppError> {
    let device = match parts(p) {
        Some((id, ..)) => find_device_meta(tx, id)?.filter(|d| d.deleted_at.is_none()),
        None => None,
    };
    Ok(PcTarget {
        branch: current_branch_id(tx)?,
        device,
    })
}

pub(super) fn validate(intent: &Intent, t: &PcTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "a PC command takes no base_rev"));
    }
    match &intent.payload {
        IntentPayload::DeviceScreen(s) => s.validate()?,
        IntentPayload::DevicePower(w) => w.validate()?,
        _ => {}
    }
    if t.branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    match &t.device {
        Some(d) if d.kind == "pc" => Ok(()),
        Some(_) => Err(invalid("device_id", "only a PC takes this command")),
        None => Err(invalid("device_id", "no such device")),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &PcTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(branch_id), Some((device_id, action, text))) = (&t.branch, parts(&intent.payload))
    else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a PC command validated without its target",
        ));
    };
    let event_id = ulid::Ulid::generate().to_string();
    insert_device_event(
        tx,
        &NewDeviceEvent {
            event_id: &event_id,
            branch_id,
            device_id,
            at: now_ms,
            level: "info",
            code: &format!("pc.{action}"),
            detail: text.map_or("", str::trim),
        },
    )?;
    Ok(vec![Written {
        key: RowKey::DeviceEvents { event_id },
        op: ChangeOp::Insert,
        before: None,
    }])
}

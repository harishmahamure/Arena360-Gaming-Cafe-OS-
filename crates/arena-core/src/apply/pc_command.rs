use arena_proto::{AppError, ErrorCode, Intent, IntentPayload, MessageScope};
use arena_store::{
    ChangeOp, DeviceMetaRow, NewDeviceEvent, RowKey, StoreTx, current_branch_id,
    device_has_live_session, find_device_meta, insert_device_event, list_device_meta,
};

use super::mutate::Written;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PcTarget {
    branch: Option<String>,
    device: Option<DeviceMetaRow>,
    busy: bool,
    reach: Vec<String>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn parts(p: &IntentPayload) -> Option<(&str, &str)> {
    match p {
        IntentPayload::DeviceScreen(s) => Some((&s.device_id, s.action.as_str())),
        IntentPayload::DevicePower(w) => Some((&w.device_id, w.action.as_str())),
        IntentPayload::DeviceMessage(m) => m.device_id.as_deref().map(|id| (id, "message")),
        _ => None,
    }
}

fn is_all(p: &IntentPayload) -> bool {
    matches!(p, IntentPayload::DeviceMessage(m) if m.scope == MessageScope::All)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<PcTarget, AppError> {
    let device = match parts(p) {
        Some((id, ..)) => find_device_meta(tx, id)?.filter(|d| d.deleted_at.is_none()),
        None => None,
    };
    let busy = match (p, &device) {
        (IntentPayload::DevicePower(_), Some(d)) => {
            d.state == "in_session" || device_has_live_session(tx, &d.device_id)?
        }
        _ => false,
    };
    let mut reach = Vec::new();
    if is_all(p) {
        for d in list_device_meta(tx)? {
            if d.kind == "pc"
                && d.deleted_at.is_none()
                && device_has_live_session(tx, &d.device_id)?
            {
                reach.push(d.device_id);
            }
        }
    }
    Ok(PcTarget {
        branch: current_branch_id(tx)?,
        device,
        busy,
        reach,
    })
}

pub(super) fn validate(intent: &Intent, t: &PcTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "a PC command takes no base_rev"));
    }
    match &intent.payload {
        IntentPayload::DeviceScreen(s) => s.validate()?,
        IntentPayload::DevicePower(w) => w.validate()?,
        IntentPayload::DeviceMessage(m) => m.validate()?,
        _ => {}
    }
    if t.branch.is_none() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the branch has not been bootstrapped",
        ));
    }
    if is_all(&intent.payload) {
        return Ok(());
    }
    match &t.device {
        Some(d) if d.kind == "pc" && t.busy => Err(super::session::busy_error()),
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
    let Some(branch_id) = &t.branch else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a PC command validated without its branch",
        ));
    };
    let (targets, action, detail): (Vec<&str>, &str, &str) = match &intent.payload {
        IntentPayload::DeviceMessage(m) if m.scope == MessageScope::All => (
            t.reach.iter().map(String::as_str).collect(),
            "message",
            m.text.trim(),
        ),
        IntentPayload::DeviceMessage(m) => (
            parts(&intent.payload)
                .map(|(id, _)| id)
                .into_iter()
                .collect(),
            "message",
            m.text.trim(),
        ),
        other => match parts(other) {
            Some((id, action)) => (vec![id], action, ""),
            None => {
                return Err(AppError::new(
                    ErrorCode::Internal,
                    "a PC command validated without its target",
                ));
            }
        },
    };
    let mut written = Vec::with_capacity(targets.len());
    for device_id in targets {
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
                detail,
            },
        )?;
        written.push(Written {
            key: RowKey::DeviceEvents { event_id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}

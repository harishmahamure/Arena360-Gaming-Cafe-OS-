use arena_proto::{AlertKind, AlertRaise, AppError, ErrorCode, Intent, IntentPayload};
use arena_store::{
    AlertRow, ChangeOp, NewAlert, RowKey, StoreTx, ack_alert, find_alert, find_device,
    find_open_alert, insert_alert,
};

use super::mutate::Written;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct AlertTarget {
    device: bool,
    open: Option<AlertRow>,
    alert: Option<AlertRow>,
}

pub(super) fn load(tx: &StoreTx<'_>, payload: &IntentPayload) -> Result<AlertTarget, AppError> {
    match payload {
        IntentPayload::AlertRaise(r) => load_raise(tx, r),
        IntentPayload::AlertAck(a) => Ok(AlertTarget {
            alert: find_alert(tx, &a.alert_id)?,
            ..AlertTarget::default()
        }),
        _ => Ok(AlertTarget::default()),
    }
}

fn load_raise(tx: &StoreTx<'_>, r: &AlertRaise) -> Result<AlertTarget, AppError> {
    let device = match &r.device_id {
        Some(id) => find_device(tx, id)?.is_some_and(|d| d.revoked_at.is_none()),
        None => false,
    };
    Ok(AlertTarget {
        device,
        open: match AlertKind::parse(&r.alert_kind) {
            Some(kind) => find_open_alert(tx, kind.as_str(), r.device_id.as_deref())?,
            None => None,
        },
        alert: None,
    })
}

fn stale(message: &str) -> AppError {
    AppError::new(ErrorCode::Stale, message)
}

pub(super) fn validate(intent: &Intent, t: &AlertTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "alert intents take no base_rev"));
    }
    match &intent.payload {
        IntentPayload::AlertRaise(r) => {
            match AlertKind::parse(&r.alert_kind) {
                None => return Err(invalid("alert_kind", "this alert kind is not known")),
                Some(AlertKind::StockLow) => {
                    return Err(invalid(
                        "alert_kind",
                        "low stock is raised by stock changes",
                    ));
                }
                Some(_) => {}
            }
            if r.device_id.is_some() && !t.device {
                return Err(AppError::with_field(
                    ErrorCode::NotFound,
                    "device_id",
                    "no such device",
                ));
            }
            if t.open.is_some() {
                return Err(stale("this alert is already open"));
            }
            Ok(())
        }
        IntentPayload::AlertAck(_) => match &t.alert {
            None => Err(AppError::with_field(
                ErrorCode::NotFound,
                "alert_id",
                "no such alert",
            )),
            Some(a) if a.acked_at.is_some() => Err(stale("this alert is already acknowledged")),
            Some(_) => Ok(()),
        },
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &AlertTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    match &intent.payload {
        IntentPayload::AlertRaise(r) if AlertKind::parse(&r.alert_kind).is_some() => {
            let id = ulid::Ulid::generate().to_string();
            insert_alert(
                tx,
                &NewAlert {
                    id: &id,
                    kind: &r.alert_kind,
                    device_id: r.device_id.as_deref(),
                    raised_at: now_ms,
                    product_id: None,
                    qty: None,
                    threshold: None,
                },
            )?;
            Ok(vec![Written {
                key: RowKey::Alerts { id },
                op: ChangeOp::Insert,
                before: None,
            }])
        }
        IntentPayload::AlertAck(a) if t.alert.is_some() => {
            ack_alert(tx, &a.alert_id, now_ms, &intent.actor.id)?;
            Ok(vec![Written {
                key: RowKey::Alerts {
                    id: a.alert_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            }])
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "an alert intent reached mutate without its target",
        )),
    }
}

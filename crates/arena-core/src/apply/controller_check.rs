use arena_proto::{
    AlertKind, AppError, ControllerCheck, ControllerResult, ErrorCode, Intent, IntentPayload,
};
use arena_store::{
    AlertRow, ChangeOp, DeviceMetaRow, LiveSession, NewAlert, RowKey, StoreTx, find_device_meta,
    find_open_alert, find_session, insert_alert, set_controller_check,
};

use super::mutate::Written;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct CheckTarget {
    session: Option<LiveSession>,
    device: Option<DeviceMetaRow>,
    marked: bool,
    open_alert: Option<AlertRow>,
}

pub(super) fn load(tx: &StoreTx<'_>, c: &ControllerCheck) -> Result<CheckTarget, AppError> {
    let Some(session) = find_session(tx, &c.session_id)? else {
        return Ok(CheckTarget::default());
    };
    let marked = tx
        .query_row(
            "SELECT controller_check IS NOT NULL FROM sessions WHERE id = ?1",
            [&session.id],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    Ok(CheckTarget {
        device: find_device_meta(tx, &session.device_id)?,
        open_alert: find_open_alert(
            tx,
            AlertKind::ControllerMissing.as_str(),
            Some(&session.device_id),
        )?,
        marked,
        session: Some(session),
    })
}

pub(super) fn validate(intent: &Intent, t: &CheckTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "controller.check takes no base_rev"));
    }
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    if t.device.as_ref().is_none_or(|d| d.kind != "console") {
        return Err(invalid(
            "session_id",
            "only a console session has a controller check",
        ));
    }
    if s.ended_at.is_none() {
        return Err(invalid(
            "session_id",
            "the controllers are checked once the session has ended",
        ));
    }
    if t.marked {
        return Err(AppError::new(
            ErrorCode::Stale,
            "these controllers are already marked",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &CheckTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (IntentPayload::ControllerCheck(c), Some(s)) = (&intent.payload, &t.session) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a controller check reached mutate without its session",
        ));
    };
    set_controller_check(tx, &s.id, c.result.as_str(), now_ms, &intent.actor.id)?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    if c.result == ControllerResult::Missing && t.open_alert.is_none() {
        let id = ulid::Ulid::generate().to_string();
        insert_alert(
            tx,
            &NewAlert {
                id: &id,
                kind: AlertKind::ControllerMissing.as_str(),
                device_id: Some(&s.device_id),
                raised_at: now_ms,
            },
        )?;
        written.push(Written {
            key: RowKey::Alerts { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}

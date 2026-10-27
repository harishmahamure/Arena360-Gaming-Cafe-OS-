use arena_proto::{AppError, ErrorCode, Intent, SessionExpire};
use arena_store::{
    ChangeOp, DeviceMetaRow, LiveSession, RowKey, SegmentRow, StoreTx, close_segments,
    expire_session, find_device_meta, find_session, list_segments, set_device_state,
};

use super::mutate::Written;
use super::session::invalid;

#[derive(Default)]
pub(crate) struct ExpireTarget {
    session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    device: Option<DeviceMetaRow>,
}

pub(super) fn load(tx: &StoreTx<'_>, e: &SessionExpire) -> Result<ExpireTarget, AppError> {
    let Some(session) = find_session(tx, &e.session_id)? else {
        return Ok(ExpireTarget::default());
    };
    Ok(ExpireTarget {
        segments: list_segments(tx, Some(&session.id))?,
        device: find_device_meta(tx, &session.device_id)?,
        session: Some(session),
    })
}

fn stale(message: &str) -> AppError {
    AppError::new(ErrorCode::Stale, message)
}

pub(super) fn validate(intent: &Intent, t: &ExpireTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "session.expire takes no base_rev"));
    }
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    if s.state != "active" {
        return Err(stale("this session is no longer running"));
    }
    match s.ends_at {
        None => Err(invalid("session_id", "this session has no end time")),
        Some(at) if at > intent.issued_at => Err(stale("this session is not due yet")),
        Some(_) => Ok(()),
    }
}

pub(super) fn mutate(tx: &StoreTx<'_>, t: &ExpireTarget) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let ended_at = s.ends_at.unwrap_or(s.started_at);
    close_segments(tx, &s.id, ended_at)?;
    expire_session(tx, &s.id, ended_at)?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    for seg in t.segments.iter().filter(|g| g.to_at.is_none()) {
        written.push(Written {
            key: RowKey::SessionSegments { id: seg.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    if let Some(d) = &t.device {
        let state = d.prior_state.as_deref().filter(|p| *p != "in_session");
        set_device_state(tx, &d.device_id, state.unwrap_or("online"), None, None)?;
        written.push(Written {
            key: RowKey::DevicesMeta {
                device_id: d.device_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

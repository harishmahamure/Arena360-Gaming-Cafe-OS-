use arena_proto::{AlertKind, AppError, ErrorCode, Intent, SessionExpire};
use arena_store::{
    AlertRow, ChangeOp, DeviceMetaRow, LiveSession, NewAlert, RowKey, SegmentRow, StoreTx,
    close_segments, expire_session, find_device_meta, find_open_alert, find_session, grace_session,
    hold_session_expired, insert_alert, list_segments, set_device_state,
};

use super::mutate::Written;
use super::prepaid::{self, CloseExtras, Closing, Ending};
use super::session::invalid;

const MIN_MS: i64 = 60_000;

#[derive(Default)]
pub(crate) struct ExpireTarget {
    session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    device: Option<DeviceMetaRow>,
    close: CloseExtras,
    open_alert: Option<AlertRow>,
}

/// What reaching a timed session's end does next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Grace,
    Close,
}

pub(super) fn load(tx: &StoreTx<'_>, e: &SessionExpire) -> Result<ExpireTarget, AppError> {
    let Some(session) = find_session(tx, &e.session_id)? else {
        return Ok(ExpireTarget::default());
    };
    Ok(ExpireTarget {
        segments: list_segments(tx, Some(&session.id))?,
        device: find_device_meta(tx, &session.device_id)?,
        close: prepaid::load_close(tx, &session)?,
        open_alert: find_open_alert(tx, AlertKind::TimeUp.as_str(), Some(&session.device_id))?,
        session: Some(session),
    })
}

fn stale(message: &str) -> AppError {
    AppError::new(ErrorCode::Stale, message)
}

/// Whether a due session first gets its grace or is closed out now: grace applies from the end
/// time for the session's snapshot grace period, and a session found past it (after a restart)
/// goes straight to the time-up action. A console has no grace and always waits for staff.
fn step(s: &LiveSession, console: bool, ends_at: i64, now: i64) -> Step {
    let grace_ms = if console {
        0
    } else {
        i64::from(s.grace_min) * MIN_MS
    };
    if s.state == "active" && grace_ms > 0 && now < ends_at + grace_ms {
        Step::Grace
    } else {
        Step::Close
    }
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
    if s.state != "active" && s.state != "grace" {
        return Err(stale("this session is no longer running"));
    }
    let Some(ends_at) = s.ends_at else {
        return Err(invalid("session_id", "this session has no end time"));
    };
    if ends_at > intent.issued_at {
        return Err(stale("this session is not due yet"));
    }
    if s.state == "grace" && ends_at + i64::from(s.grace_min) * MIN_MS > intent.issued_at {
        return Err(stale("this session is still in its grace period"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &ExpireTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let ended_at = s.ends_at.unwrap_or(s.started_at);
    let console = t.device.as_ref().is_some_and(|d| d.kind == "console");
    if step(s, console, ended_at, intent.issued_at) == Step::Grace {
        grace_session(tx, &s.id)?;
        return Ok(vec![Written {
            key: RowKey::Sessions { id: s.id.clone() },
            op: ChangeOp::Update,
            before: None,
        }]);
    }
    if console || s.time_up_mode == arena_proto::TimeUpMode::StaffHandles.as_str() {
        return hold(tx, t, s, ended_at, now_ms);
    }
    if prepaid::is_prepaid(s) {
        return prepaid::close_out(
            tx,
            &Closing {
                session: s,
                segments: &t.segments,
                device: t.device.as_ref(),
                extras: &t.close,
                ending: Ending::Expired,
                ended_at,
                actor_id: "hub",
            },
            now_ms,
        );
    }
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

/// "Staff handles": the time is up, no more is charged, and the session stays `expired` on its
/// device with a "TIME UP" alert until staff end it.
fn hold(
    tx: &StoreTx<'_>,
    t: &ExpireTarget,
    s: &LiveSession,
    ended_at: i64,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    close_segments(tx, &s.id, ended_at)?;
    hold_session_expired(tx, &s.id)?;
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
    if t.open_alert.is_none() {
        let id = ulid::Ulid::generate().to_string();
        insert_alert(
            tx,
            &NewAlert {
                id: &id,
                kind: AlertKind::TimeUp.as_str(),
                device_id: Some(&s.device_id),
                raised_at: now_ms,
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
    Ok(written)
}

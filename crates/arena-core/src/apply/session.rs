use arena_proto::{AppError, ErrorCode, Intent, SessionMode, SessionStart};
use arena_store::{
    ChangeOp, DeviceMetaRow, NewSession, RateRow, RowKey, StoreTx, device_has_live_session,
    find_active_rate, find_device_meta, find_player, insert_segment, insert_session, next_guest_no,
    set_device_state,
};

use super::mutate::Written;
use crate::status::Epoch;

#[derive(Default)]
pub(crate) struct SessionTarget {
    device: Option<DeviceMetaRow>,
    busy: bool,
    rate: Option<RateRow>,
    player: bool,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, s: &SessionStart) -> Result<SessionTarget, AppError> {
    let device = find_device_meta(tx, &s.device_id)?.filter(|d| d.deleted_at.is_none());
    let busy = match &device {
        Some(d) => d.state == "in_session" || device_has_live_session(tx, &d.device_id)?,
        None => false,
    };
    let rate = match device.as_ref().and_then(|d| d.group_id.as_deref()) {
        Some(g) => find_active_rate(tx, g)?,
        None => None,
    };
    let player = match &s.player_id {
        Some(id) => find_player(tx, id)?.is_some(),
        None => false,
    };
    Ok(SessionTarget {
        device,
        busy,
        rate,
        player,
    })
}

pub(super) fn busy_error() -> AppError {
    AppError::with_field(
        ErrorCode::PcBusy,
        "device_id",
        "End or pause the session first",
    )
}

pub(super) fn validate(
    intent: &Intent,
    s: &SessionStart,
    t: &SessionTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "session.start takes no base_rev"));
    }
    if s.mode != SessionMode::Postpaid {
        return Err(invalid("mode", "only a postpaid session can start here"));
    }
    let Some(d) = &t.device else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "device_id",
            "no such device",
        ));
    };
    if d.kind != "pc" {
        return Err(invalid("device_id", "only a PC takes a session"));
    }
    if d.group_id.is_none() {
        return Err(AppError::with_field(
            ErrorCode::DeviceUnassigned,
            "device_id",
            "Assign this PC to a group before starting a session.",
        ));
    }
    if d.state == "maintenance" {
        return Err(AppError::with_field(
            ErrorCode::PcUnavailable,
            "device_id",
            "This PC is in maintenance.",
        ));
    }
    if t.busy {
        return Err(AppError::with_field(
            ErrorCode::PcBusy,
            "device_id",
            "This PC already has a session.",
        ));
    }
    if t.rate.is_none() {
        return Err(AppError::with_field(
            ErrorCode::NoRate,
            "device_id",
            "This PC's group has no rate.",
        ));
    }
    if s.player_id.is_some() && !t.player {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    s: &SessionStart,
    t: &SessionTarget,
    epoch: &Epoch,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(d), Some(rate), Some(group_id)) = (
        &t.device,
        &t.rate,
        t.device.as_ref().and_then(|d| d.group_id.as_deref()),
    ) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let rules = crate::billing::read(tx)?;
    let guest_no = match s.player_id {
        Some(_) => None,
        None => Some(next_guest_no(tx, &business_day)?),
    };
    let id = ulid::Ulid::generate().to_string();
    let segment_id = ulid::Ulid::generate().to_string();
    insert_session(
        tx,
        &NewSession {
            id: &id,
            player_id: s.player_id.as_deref(),
            guest_no,
            device_id: &d.device_id,
            group_id,
            mode: s.mode.as_str(),
            rate_id: &rate.rate_id,
            started_at: now_ms,
            rounding_min: rules.rounding_min,
            minimum_min: rules.minimum_min,
            grace_min: rules.grace_min,
            time_up_mode: rules.time_up_mode.as_str(),
            epoch: &epoch.to_string(),
            business_day: &business_day,
            fy: &fy,
        },
    )?;
    insert_segment(
        tx,
        &segment_id,
        &id,
        now_ms,
        &d.device_id,
        rate.price_per_hour_minor,
    )?;
    set_device_state(tx, &d.device_id, "in_session", Some(&d.state), None)?;
    Ok(vec![
        Written {
            key: RowKey::Sessions { id },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::SessionSegments { id: segment_id },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::DevicesMeta {
                device_id: d.device_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        },
    ])
}

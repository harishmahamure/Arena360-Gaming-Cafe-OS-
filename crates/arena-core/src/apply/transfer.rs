//! Moving a running or paused PC session to another free PC when the first one breaks.
//!
//! The open rate segment closes at the transfer and the next opens on the new device at the new
//! group's resolved rate (a pack, pass or block session stays at rate 0). Time already used is
//! never repriced, and a pause is split between the two segments exactly as a rate flip splits it.

use arena_proto::{AppError, ErrorCode, Intent, SessionMode, SessionTransfer};
use arena_store::{
    ChangeOp, DeviceMetaRow, LiveSession, PlayerPackRow, RateRow, RowKey, SegmentRow, StoreTx,
    add_segment_pause, close_segments, device_has_live_session, find_active_rate, find_device_meta,
    find_player_pack, find_session, insert_segment, list_segments, move_open_orders, move_session,
    set_device_state,
};

use super::mutate::Written;
use super::rate_flip::{open_of, own_end, resize_wallet};
use super::session::{check_rev, invalid};

#[derive(Default)]
pub(crate) struct TransferTarget {
    session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    source: Option<DeviceMetaRow>,
    target: Option<DeviceMetaRow>,
    busy: bool,
    rate: Option<RateRow>,
    rule_id: Option<String>,
    unchecked: bool,
    pack: Option<PlayerPackRow>,
}

/// Whether a session of this mode runs at the group's hourly rate (a pack, pass or block is
/// priced by its plan instead).
fn priced_by_rate(s: &LiveSession) -> bool {
    s.mode == SessionMode::Postpaid.as_str() || s.mode == SessionMode::Wallet.as_str()
}

fn plan_backed(s: &LiveSession) -> bool {
    s.mode == SessionMode::Pack.as_str() || s.mode == SessionMode::Pass.as_str()
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    t: &SessionTransfer,
    now_ms: i64,
) -> Result<TransferTarget, AppError> {
    let Some(session) = find_session(tx, &t.session_id)? else {
        return Ok(TransferTarget::default());
    };
    let target = find_device_meta(tx, &t.device_id)?.filter(|d| d.deleted_at.is_none());
    let busy = match &target {
        Some(d) => d.state == "in_session" || device_has_live_session(tx, &d.device_id)?,
        None => false,
    };
    let (rate, rule_id) = match target.as_ref().and_then(|d| d.group_id.as_deref()) {
        Some(g) => match find_active_rate(tx, g)? {
            Some(base) => {
                let (rate, resolved) = crate::pricing::priced(tx, base, now_ms)?;
                (Some(rate), resolved.rule_id)
            }
            None => (None, None),
        },
        None => (None, None),
    };
    let unchecked = match &target {
        Some(d) => arena_store::unchecked_console_session(tx, &d.device_id)?.is_some(),
        None => false,
    };
    let pack = match (session.player_id.as_deref(), session.pack_id.as_deref()) {
        (Some(player_id), Some(pack_id)) if plan_backed(&session) => {
            find_player_pack(tx, player_id, pack_id)?
        }
        _ => None,
    };
    Ok(TransferTarget {
        segments: list_segments(tx, Some(&session.id))?,
        source: find_device_meta(tx, &session.device_id)?,
        target,
        busy,
        rate,
        rule_id,
        unchecked,
        pack,
        session: Some(session),
    })
}

pub(super) fn base_rev(intent: &Intent, t: &TransferTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, s.rev, "session.transfer")
}

pub(super) fn validate(t: &TransferTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    if !matches!(s.state.as_str(), "active" | "paused") {
        return Err(invalid(
            "session_id",
            "only a running or paused session can move",
        ));
    }
    let Some(d) = &t.target else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "device_id",
            "no such device",
        ));
    };
    if d.device_id == s.device_id {
        return Err(invalid("device_id", "the session is already on this PC"));
    }
    let Some(group_id) = &d.group_id else {
        return Err(AppError::with_field(
            ErrorCode::DeviceUnassigned,
            "device_id",
            "Assign this PC to a group before moving a session here.",
        ));
    };
    let source_kind = t.source.as_ref().map_or("pc", |m| m.kind.as_str());
    if d.kind != source_kind {
        return Err(AppError::with_field(
            ErrorCode::GroupKindMismatch,
            "device_id",
            "A session moves only to a device of the same kind.",
        ));
    }
    if !matches!(d.kind.as_str(), "pc" | "console") {
        return Err(invalid(
            "device_id",
            "only a PC or a console takes a session",
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
    if t.unchecked {
        return Err(super::session::unchecked_error());
    }
    if priced_by_rate(s) && t.rate.is_none() {
        return Err(AppError::with_field(
            ErrorCode::NoRate,
            "device_id",
            "This PC's group has no rate.",
        ));
    }
    let plan_ok = if plan_backed(s) {
        t.pack
            .as_ref()
            .is_some_and(|p| p.group_ids.iter().any(|g| g == group_id))
    } else if s.mode == SessionMode::Block.as_str() {
        *group_id == s.group_id
    } else {
        true
    };
    if !plan_ok {
        return Err(AppError::with_field(
            ErrorCode::PlanNotForGroup,
            "device_id",
            "This session's plan is not for that PC's group.",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    tr: &SessionTransfer,
    t: &TransferTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let internal = || {
        AppError::new(
            ErrorCode::Internal,
            "a transfer validated without its target",
        )
    };
    let (s, dst) = t
        .session
        .as_ref()
        .zip(t.target.as_ref())
        .ok_or_else(internal)?;
    let group_id = dst.group_id.as_deref().ok_or_else(internal)?;
    let open = open_of(&t.segments).ok_or_else(internal)?;
    let split = s
        .paused_at
        .map_or(0, |paused_at| (now_ms - paused_at.max(open.from_at)).max(0));
    if split > 0 {
        add_segment_pause(tx, &s.id, split)?;
    }
    close_segments(tx, &s.id, now_ms)?;
    let by_rate = priced_by_rate(s);
    let rate = t.rate.as_ref().filter(|_| by_rate);
    let rate_minor = rate.map_or(0, |r| r.price_per_hour_minor);
    let segment_id = ulid::Ulid::generate().to_string();
    insert_segment(
        tx,
        &segment_id,
        &s.id,
        now_ms,
        &tr.device_id,
        rate_minor,
        rate.and(t.rule_id.as_deref()),
    )?;
    if s.mode == SessionMode::Wallet.as_str()
        && rate_minor != open.rate_minor_per_hour
        && own_end(s).is_some_and(|end| now_ms < end)
    {
        resize_wallet(tx, s, &t.segments, open, split, now_ms, rate_minor)?;
    }
    move_session(
        tx,
        &s.id,
        &tr.device_id,
        group_id,
        rate.map(|r| r.rate_id.as_str()),
    )?;
    let mut written = vec![
        Written {
            key: RowKey::Sessions { id: s.id.clone() },
            op: ChangeOp::Update,
            before: None,
        },
        Written {
            key: RowKey::SessionSegments {
                id: open.id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        },
        Written {
            key: RowKey::SessionSegments { id: segment_id },
            op: ChangeOp::Insert,
            before: None,
        },
    ];
    if s.mode == SessionMode::Block.as_str() {
        for id in move_open_orders(tx, &s.id, &tr.device_id)? {
            written.push(Written {
                key: RowKey::Orders { id },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    if let Some(src) = &t.source {
        if src.state != "maintenance" {
            let state = src.prior_state.as_deref().filter(|p| *p != "in_session");
            set_device_state(tx, &src.device_id, state.unwrap_or("online"), None, None)?;
        }
        written.push(Written {
            key: RowKey::DevicesMeta {
                device_id: src.device_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    set_device_state(tx, &dst.device_id, "in_session", Some(&dst.state), None)?;
    written.push(Written {
        key: RowKey::DevicesMeta {
            device_id: dst.device_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

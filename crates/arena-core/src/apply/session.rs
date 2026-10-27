use arena_proto::{
    AppError, CARD_REFERENCE_MAX, ErrorCode, Intent, PaymentMethod, PaymentRow, SessionEnd,
    SessionMode, SessionSettle, SessionStart,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, LiveSession, NewOrder, NewOrderLine, NewPayment, NewSession, OrderRow,
    RateRow, RowKey, SegmentRow, StoreTx, close_segments, device_has_live_session, end_session,
    find_active_rate, find_device_meta, find_order, find_player, find_session, insert_order,
    insert_order_line, insert_payment, insert_segment, insert_session, list_segments,
    next_guest_no, set_device_state, settle_order,
};

use super::mutate::Written;
use crate::status::Epoch;

const MAX_PAYMENTS: usize = 8;

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

#[derive(Default)]
pub(crate) struct EndTarget {
    session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    device: Option<DeviceMetaRow>,
}

pub(super) fn load_end(tx: &StoreTx<'_>, e: &SessionEnd) -> Result<EndTarget, AppError> {
    let Some(session) = find_session(tx, &e.session_id)? else {
        return Ok(EndTarget::default());
    };
    Ok(EndTarget {
        segments: list_segments(tx, Some(&session.id))?,
        device: find_device_meta(tx, &session.device_id)?,
        session: Some(session),
    })
}

pub(super) fn base_rev_end(intent: &Intent, t: &EndTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    match intent.base_rev {
        None => Err(invalid("base_rev", "session.end needs base_rev")),
        Some(b) if b != s.rev => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        Some(_) => Ok(()),
    }
}

pub(super) fn validate_end(t: &EndTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(invalid("session_id", "no such session"));
    };
    if !["active", "paused", "grace"].contains(&s.state.as_str()) {
        return Err(invalid("session_id", "this session has already ended"));
    }
    if s.mode != SessionMode::Postpaid.as_str() {
        return Err(invalid("session_id", "only a postpaid session ends here"));
    }
    Ok(())
}

pub(super) fn mutate_end(
    tx: &StoreTx<'_>,
    t: &EndTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let lines = crate::billing::bill(
        &t.segments,
        s.rounding_min,
        s.minimum_min,
        s.paused_total_ms,
        now_ms,
    );
    let subtotal: i64 = lines.iter().map(|l| l.amount_minor).sum();
    let order_id = ulid::Ulid::generate().to_string();
    close_segments(tx, &s.id, now_ms)?;
    end_session(tx, &s.id, now_ms)?;
    insert_order(
        tx,
        &NewOrder {
            id: &order_id,
            session_id: &s.id,
            player_id: s.player_id.as_deref(),
            guest_no: s.guest_no,
            device_id: &s.device_id,
            subtotal_minor: subtotal,
            business_day: &s.business_day,
            fy: &s.fy,
        },
    )?;
    let mut written = vec![
        Written {
            key: RowKey::Sessions { id: s.id.clone() },
            op: ChangeOp::Update,
            before: None,
        },
        Written {
            key: RowKey::Orders {
                id: order_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        },
    ];
    for (i, (seg, line)) in t.segments.iter().zip(&lines).enumerate() {
        let id = ulid::Ulid::generate().to_string();
        insert_order_line(
            tx,
            &NewOrderLine {
                id: &id,
                order_id: &order_id,
                segment_id: &seg.id,
                device_id: &line.device_id,
                from_at: line.from_at,
                to_at: line.to_at,
                minutes: line.minutes,
                rate_minor_per_hour: line.rate_minor_per_hour,
                amount_minor: line.amount_minor,
                position: i as i64,
            },
        )?;
        written.push(Written {
            key: RowKey::SessionSegments { id: seg.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
        written.push(Written {
            key: RowKey::OrderLines { id },
            op: ChangeOp::Insert,
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

#[derive(Default)]
pub(crate) struct SettleTarget {
    order: Option<OrderRow>,
}

pub(super) fn load_settle(tx: &StoreTx<'_>, s: &SessionSettle) -> Result<SettleTarget, AppError> {
    Ok(SettleTarget {
        order: find_order(tx, &s.order_id)?,
    })
}

pub(super) fn base_rev_settle(intent: &Intent, t: &SettleTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    match intent.base_rev {
        None => Err(invalid("base_rev", "session.settle needs base_rev")),
        Some(b) if b != o.rev => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        Some(_) => Ok(()),
    }
}

fn reference_error(i: usize, message: &str) -> AppError {
    AppError::with_field(
        ErrorCode::ReferenceRequired,
        format!("payments[{i}].reference"),
        message,
    )
}

fn check_reference(i: usize, p: &PaymentRow) -> Result<(), AppError> {
    let reference = p.reference.as_deref().map(str::trim);
    match (p.method, reference) {
        (PaymentMethod::Cash, None | Some("")) => Ok(()),
        (PaymentMethod::Cash, Some(_)) => Err(invalid(
            &format!("payments[{i}].reference"),
            "a cash payment takes no reference",
        )),
        (PaymentMethod::Upi, Some(r)) if r.len() == 4 && r.bytes().all(|b| b.is_ascii_digit()) => {
            Ok(())
        }
        (PaymentMethod::Upi, _) => Err(reference_error(
            i,
            "Enter the last 4 digits of the UPI reference",
        )),
        (PaymentMethod::Card, Some(r))
            if !r.is_empty()
                && r.chars().count() <= CARD_REFERENCE_MAX
                && !r.chars().any(char::is_control) =>
        {
            Ok(())
        }
        (PaymentMethod::Card, _) => Err(reference_error(i, "Enter the card terminal reference")),
    }
}

pub(super) fn validate_settle(s: &SessionSettle, t: &SettleTarget) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    match o.status.as_str() {
        "open" => {}
        "settled" => {
            return Err(AppError::with_field(
                ErrorCode::Stale,
                "order_id",
                "this bill is already settled",
            ));
        }
        _ => return Err(invalid("order_id", "this bill is void")),
    }
    if s.payments.len() > MAX_PAYMENTS {
        return Err(invalid("payments", "too many payments"));
    }
    for (i, p) in s.payments.iter().enumerate() {
        if p.amount_minor <= 0 {
            return Err(invalid(
                &format!("payments[{i}].amount_minor"),
                "each payment must be above 0",
            ));
        }
        check_reference(i, p)?;
    }
    let paid = s
        .payments
        .iter()
        .try_fold(0i64, |sum, p| sum.checked_add(p.amount_minor))
        .ok_or_else(|| invalid("payments", "the payments are too large"))?;
    if paid != o.total_minor {
        return Err(AppError::with_field(
            ErrorCode::PaymentShort,
            "payments",
            "The payments must add up to the bill.",
        ));
    }
    Ok(())
}

pub(super) fn mutate_settle(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &SessionSettle,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let mut written = Vec::with_capacity(s.payments.len() + 1);
    for (i, p) in s.payments.iter().enumerate() {
        let id = ulid::Ulid::generate().to_string();
        let reference = p
            .reference
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty());
        insert_payment(
            tx,
            &NewPayment {
                id: &id,
                order_id: &s.order_id,
                method: p.method.as_str(),
                amount_minor: p.amount_minor,
                reference,
                staff_id: &intent.actor.id,
                position: i as i64,
                business_day: &business_day,
                fy: &fy,
            },
        )?;
        written.push(Written {
            key: RowKey::Payments { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    settle_order(tx, &s.order_id, now_ms, &intent.actor.id)?;
    written.push(Written {
        key: RowKey::Orders {
            id: s.order_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

use arena_proto::{
    AppError, CARD_REFERENCE_MAX, ErrorCode, Intent, PaymentMethod, PaymentRow,
    SessionAssignPlayer, SessionEnd, SessionEndReverted, SessionMode, SessionPause, SessionResume,
    SessionSettle, SessionSettleReverted, SessionStart, TimeUpMode, UNDO_WINDOW_MS,
};
use arena_store::{
    ChangeOp, CreditRow, DeviceMetaRow, LiveSession, NewOrder, NewOrderLine, NewPayment,
    NewSession, NewWalletTxn, OrderRow, OrderTxn, RateRow, RowKey, SegmentRow, StoreTx, Table,
    add_order_amount, adjust_outstanding, adjust_wallet, assign_order_player,
    assign_session_player, close_segments, count_product_lines, count_unserved_lines, delete_row,
    device_has_live_session, effect_recorded, end_session, find_active_rate, find_credit,
    find_device_meta, find_order, find_order_for_session, find_player, find_session, find_wallet,
    fold_pause, forward_intent_id, insert_order, insert_order_line, insert_payment, insert_segment,
    insert_session, insert_wallet_txn, list_order_wallet_txns, list_payment_ids,
    list_segment_line_ids, list_segments, next_guest_no, pause_session, read_row, reopen_order,
    reopen_segments, reopen_session, resume_session, set_device_state, set_group_state,
    set_order_group, set_override_day, settle_order, sum_reserved_minor, sum_segment_amounts,
};

use super::mutate::Written;
use crate::status::Epoch;

pub(super) const MAX_PAYMENTS: usize = 8;

#[derive(Default)]
pub(crate) struct SessionTarget {
    device: Option<DeviceMetaRow>,
    busy: bool,
    rate: Option<RateRow>,
    rule_id: Option<String>,
    player: bool,
    unchecked: bool,
    prepaid: super::prepaid::StartExtras,
    group_host: Option<LiveSession>,
    group_host_ordered: bool,
}

pub(super) fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    s: &SessionStart,
    now_ms: i64,
) -> Result<SessionTarget, AppError> {
    let device = find_device_meta(tx, &s.device_id)?.filter(|d| d.deleted_at.is_none());
    let busy = match &device {
        Some(d) => d.state == "in_session" || device_has_live_session(tx, &d.device_id)?,
        None => false,
    };
    let unchecked = match &device {
        Some(d) => arena_store::unchecked_console_session(tx, &d.device_id)?.is_some(),
        None => false,
    };
    let (rate, rule_id) = match device.as_ref().and_then(|d| d.group_id.as_deref()) {
        Some(g) => match find_active_rate(tx, g)? {
            Some(base) => {
                let (rate, resolved) = crate::pricing::priced(tx, base, now_ms)?;
                (Some(rate), resolved.rule_id)
            }
            None => (None, None),
        },
        None => (None, None),
    };
    let player = match &s.player_id {
        Some(id) => find_player(tx, id)?.is_some(),
        None => false,
    };
    let prepaid = if player || s.mode == SessionMode::Block {
        super::prepaid::load_start(tx, s, rate.as_ref(), now_ms)?
    } else {
        super::prepaid::StartExtras::default()
    };
    let group_host = match &s.group_with {
        Some(id) => find_session(tx, id)?,
        None => None,
    };
    let group_host_ordered = match &group_host {
        Some(h) => find_order_for_session(tx, &h.id)?.is_some(),
        None => false,
    };
    Ok(SessionTarget {
        device,
        busy,
        rate,
        rule_id,
        player,
        unchecked,
        prepaid,
        group_host,
        group_host_ordered,
    })
}

pub(super) fn unchecked_error() -> AppError {
    AppError::with_field(
        ErrorCode::ControllersUnchecked,
        "device_id",
        "Mark controllers first",
    )
}

pub(super) fn busy_error() -> AppError {
    AppError::with_field(
        ErrorCode::PcBusy,
        "device_id",
        "End or pause the session first",
    )
}

/// Whether a session of this mode runs at the group's hourly rate (a pack, pass or block is
/// priced by its plan instead).
fn priced_by_rate(mode: SessionMode) -> bool {
    matches!(mode, SessionMode::Postpaid | SessionMode::Wallet)
}

pub(super) fn validate(
    intent: &Intent,
    s: &SessionStart,
    t: &SessionTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "session.start takes no base_rev"));
    }
    let needs_player = matches!(
        s.mode,
        SessionMode::Pack | SessionMode::Pass | SessionMode::Wallet
    );
    if needs_player && s.player_id.is_none() {
        return Err(invalid(
            "player_id",
            "a pack, pass or wallet session needs a player profile",
        ));
    }
    let Some(d) = &t.device else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "device_id",
            "no such device",
        ));
    };
    if !matches!(d.kind.as_str(), "pc" | "console") {
        return Err(invalid(
            "device_id",
            "only a PC or a console takes a session",
        ));
    }
    let Some(group_id) = &d.group_id else {
        return Err(AppError::with_field(
            ErrorCode::DeviceUnassigned,
            "device_id",
            "Assign this PC to a group before starting a session.",
        ));
    };
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
        return Err(unchecked_error());
    }
    if t.rate.is_none() && priced_by_rate(s.mode) {
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
    if s.group_with.is_some() {
        super::group_bill::check_start(s.mode, t.group_host.as_ref(), t.group_host_ordered)?;
    }
    super::prepaid::check_start(s, group_id, &t.prepaid)
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    s: &SessionStart,
    t: &SessionTarget,
    epoch: &Epoch,
    actor_id: &str,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (Some(d), Some(group_id)) = (
        &t.device,
        t.device.as_ref().and_then(|d| d.group_id.as_deref()),
    ) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let rate = t.rate.as_ref().filter(|_| priced_by_rate(s.mode));
    let plan = super::prepaid::plan_start(s, &t.prepaid, now_ms)?;
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let console = d.kind == "console";
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
            rate_id: rate.map(|r| r.rate_id.as_str()),
            pack_id: plan.pack_id.as_deref(),
            started_at: now_ms,
            ends_at: plan.ends_at,
            reserved_minor: plan.reserved_minor,
            rounding_min: rules.rounding_min,
            minimum_min: rules.minimum_min,
            grace_min: if console { 0 } else { rules.grace_min },
            time_up_mode: if console {
                TimeUpMode::StaffHandles.as_str()
            } else {
                rules.time_up_mode.as_str()
            },
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
        rate.map_or(0, |r| r.price_per_hour_minor),
        rate.and(t.rule_id.as_deref()),
    )?;
    set_device_state(tx, &d.device_id, "in_session", Some(&d.state), None)?;
    let mut written = vec![
        Written {
            key: RowKey::Sessions { id: id.clone() },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::SessionSegments {
                id: segment_id.clone(),
            },
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
    ];
    if let (SessionMode::Block, Some(ends_at)) = (s.mode, plan.ends_at) {
        written.extend(super::prepaid::open_block_bill(
            tx,
            &t.prepaid,
            &super::prepaid::BlockBill {
                session_id: &id,
                segment_id: &segment_id,
                player_id: s.player_id.as_deref(),
                guest_no,
                device_id: &d.device_id,
                business_day: &business_day,
                fy: &fy,
                ends_at,
                wallet_payer: s.pay_from_wallet.then_some(actor_id),
            },
            now_ms,
        )?);
    }
    if let Some(host) = &t.group_host {
        super::group_bill::join_new(tx, host, &id, &mut written)?;
    }
    Ok(written)
}

#[derive(Default)]
pub(crate) struct EndTarget {
    pub(super) session: Option<LiveSession>,
    segments: Vec<SegmentRow>,
    device: Option<DeviceMetaRow>,
    close: super::prepaid::CloseExtras,
    group: Option<super::group_bill::GroupEnd>,
    /// The bill order the session already has (one a placement opened), which the segment lines
    /// join instead of a new order.
    order: Option<OrderRow>,
}

pub(super) fn load_end(tx: &StoreTx<'_>, e: &SessionEnd) -> Result<EndTarget, AppError> {
    let Some(session) = find_session(tx, &e.session_id)? else {
        return Ok(EndTarget::default());
    };
    Ok(EndTarget {
        segments: list_segments(tx, Some(&session.id))?,
        device: find_device_meta(tx, &session.device_id)?,
        close: super::prepaid::load_close(tx, &session)?,
        group: super::group_bill::load_end(tx, &session)?,
        order: find_order_for_session(tx, &session.id)?,
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
    let held = s.state == "expired" && s.ended_at.is_none();
    if !held && !["active", "paused", "grace"].contains(&s.state.as_str()) {
        return Err(invalid("session_id", "this session has already ended"));
    }
    Ok(())
}

pub(super) fn mutate_end(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &EndTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a session validated without its target",
        ));
    };
    let ended_at = match (s.state.as_str(), s.ends_at) {
        ("grace" | "expired", Some(at)) => at.min(now_ms),
        _ => now_ms,
    };
    if s.mode == SessionMode::Block.as_str() {
        return super::prepaid::end_block(
            tx,
            s,
            &t.segments,
            t.device.as_ref(),
            ended_at,
            &intent.actor.id,
        );
    }
    if super::prepaid::is_prepaid(s) {
        return super::prepaid::close_out(
            tx,
            &super::prepaid::Closing {
                session: s,
                segments: &t.segments,
                device: t.device.as_ref(),
                extras: &t.close,
                ending: super::prepaid::Ending::Staff,
                ended_at,
                actor_id: &intent.actor.id,
            },
            now_ms,
        );
    }
    let own_lines = crate::billing::bill(
        &t.segments,
        s.rounding_min,
        s.minimum_min,
        s.paused_at,
        now_ms,
    );
    let billed = match &t.group {
        Some(g) if !g.is_last() => None,
        other => Some(other.as_ref()),
    };
    let mut entries: Vec<(&SegmentRow, arena_proto::BillLine)> = Vec::new();
    if let Some(group) = billed {
        let mut members: Vec<(&LiveSession, &[SegmentRow], Vec<arena_proto::BillLine>)> =
            vec![(s, t.segments.as_slice(), own_lines)];
        if let Some(g) = group {
            for (m, segs) in &g.others {
                let lines = crate::billing::bill(segs, m.rounding_min, m.minimum_min, None, now_ms);
                members.push((m, segs.as_slice(), lines));
            }
        }
        members.sort_by(|x, y| (x.0.started_at, &x.0.id).cmp(&(y.0.started_at, &y.0.id)));
        for (_, segs, lines) in members {
            entries.extend(segs.iter().zip(lines));
        }
    }
    let subtotal: i64 = entries.iter().map(|(_, l)| l.amount_minor).sum();
    if let Some(paused_at) = s.paused_at {
        fold_pause(tx, &s.id, (now_ms - paused_at).max(0))?;
    }
    close_segments(tx, &s.id, now_ms)?;
    end_session(tx, &s.id, now_ms, &intent.actor.id)?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    let segment_written = |id: &str| Written {
        key: RowKey::SessionSegments { id: id.to_owned() },
        op: ChangeOp::Update,
        before: None,
    };
    let interleave = t.group.is_none();
    if billed.is_none() {
        written.extend(t.segments.iter().map(|seg| segment_written(&seg.id)));
    }
    if billed.is_some() {
        let group = billed.flatten();
        let (player_id, guest_no) = match group {
            Some(g) => (g.group.payer_player_id.as_deref(), g.group.payer_guest_no),
            None => (s.player_id.as_deref(), s.guest_no),
        };
        // A session whose order already holds placed products ends into that order.
        let order_id = match &t.order {
            Some(o) => {
                add_order_amount(tx, &o.id, subtotal)?;
                written.push(Written {
                    key: RowKey::Orders { id: o.id.clone() },
                    op: ChangeOp::Update,
                    before: None,
                });
                o.id.clone()
            }
            None => {
                let order_id = ulid::Ulid::generate().to_string();
                insert_order(
                    tx,
                    &NewOrder {
                        id: &order_id,
                        session_id: &s.id,
                        player_id,
                        guest_no,
                        device_id: &s.device_id,
                        subtotal_minor: subtotal,
                        business_day: &s.business_day,
                        fy: &s.fy,
                    },
                )?;
                written.push(Written {
                    key: RowKey::Orders {
                        id: order_id.clone(),
                    },
                    op: ChangeOp::Insert,
                    before: None,
                });
                order_id
            }
        };
        if let Some(g) = group {
            set_order_group(tx, &order_id, &g.group.id)?;
            set_group_state(tx, &g.group.id, "closed")?;
            written.push(Written {
                key: RowKey::GroupBills {
                    id: g.group.id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        }
        if !interleave {
            written.extend(t.segments.iter().map(|seg| segment_written(&seg.id)));
        }
        for (i, (seg, line)) in entries.iter().enumerate() {
            if interleave {
                written.push(segment_written(&seg.id));
            }
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
                key: RowKey::OrderLines { id },
                op: ChangeOp::Insert,
                before: None,
            });
        }
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
    pub(super) wallet_minor: i64,
    pub(super) credit: Option<CreditRow>,
    pub(super) credit_blocked: bool,
    pub(super) business_day: String,
    pub(super) shift_open: bool,
    /// The bill is a postpaid session's that is still running: it settles once the session ends.
    running: bool,
    /// The bill still has lines that are not served (Story 5.13).
    unserved: bool,
    /// The coupon the settle names, with the bill it is priced on (Story 5.21).
    coupon: Option<super::coupon::CouponBill>,
}

pub(super) fn load_settle(
    tx: &StoreTx<'_>,
    s: &SessionSettle,
    now_ms: i64,
) -> Result<SettleTarget, AppError> {
    let order = find_order(tx, &s.order_id)?;
    let mut target = load_payer(
        tx,
        order.as_ref().and_then(|o| o.player_id.as_deref()),
        &s.payments,
        now_ms,
    )?;
    target.running = match order.as_ref().and_then(|o| o.session_id.as_deref()) {
        Some(id) => find_session(tx, id)?.is_some_and(|s| {
            super::group_bill::is_running(&s) && s.mode != SessionMode::Block.as_str()
        }),
        None => false,
    };
    target.unserved = match &order {
        Some(o) => count_unserved_lines(tx, &o.id)? > 0,
        None => false,
    };
    target.coupon = match (s.coupon.is_some() || s.discount.is_some(), &order) {
        (true, Some(o)) => Some(super::coupon::load_session_bill(
            tx,
            s.coupon.as_deref(),
            &o.id,
            now_ms,
        )?),
        _ => None,
    };
    target.order = order;
    Ok(target)
}

pub(super) fn load_payer(
    tx: &StoreTx<'_>,
    player_id: Option<&str>,
    payments: &[PaymentRow],
    now_ms: i64,
) -> Result<SettleTarget, AppError> {
    let wallet_minor = match player_id {
        Some(p) => available_wallet(tx, p)?,
        None => 0,
    };
    let wants_credit = payments.iter().any(|p| p.method == PaymentMethod::Credit);
    let credit = match player_id {
        Some(p) if wants_credit => find_credit(tx, p)?,
        _ => None,
    };
    let business_day = if wants_credit {
        crate::locale::stamp(tx, now_ms)?
            .map(|(day, _)| day)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let credit_blocked = match &credit {
        Some(c) if super::credit_pause::rule_on(tx)? => {
            c.paused
                || (c.outstanding_minor > 0
                    && super::credit_pause::overdue(tx, &c.player_id, &business_day)?)
        }
        _ => false,
    };
    Ok(SettleTarget {
        order: None,
        wallet_minor,
        credit,
        credit_blocked,
        business_day,
        shift_open: arena_store::open_shift_id(tx)?.is_some(),
        running: false,
        unserved: false,
        coupon: None,
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

pub(super) fn check_reference(i: usize, p: &PaymentRow) -> Result<(), AppError> {
    let reference = p.reference.as_deref().map(str::trim);
    match (p.method, reference) {
        (PaymentMethod::Cash, None | Some("")) => Ok(()),
        (PaymentMethod::Wallet | PaymentMethod::Credit, None | Some("")) => Ok(()),
        (PaymentMethod::Wallet | PaymentMethod::Credit, Some(_)) => Err(invalid(
            &format!("payments[{i}].reference"),
            "a wallet or credit payment takes no reference",
        )),
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
    if t.running {
        return Err(invalid(
            "order_id",
            "this session is still running; end it to settle the bill",
        ));
    }
    if t.unserved {
        return Err(invalid(
            "order_id",
            "Mark served or cancel unserved items first",
        ));
    }
    if s.discount.is_some() && o.group_bill_id.is_some() {
        return Err(invalid("discount", "a group bill takes no discount"));
    }
    let total = match &t.coupon {
        Some(bill) => o.total_minor - bill.price(o.total_minor, s.discount.as_ref())?.off(),
        None => o.total_minor,
    };
    validate_payments(
        &s.payments,
        s.credit_override,
        t,
        o.player_id.is_some(),
        total,
    )
}

pub(super) fn validate_payments(
    payments: &[PaymentRow],
    credit_override: Option<bool>,
    t: &SettleTarget,
    has_player: bool,
    total_minor: i64,
) -> Result<(), AppError> {
    if payments.len() > MAX_PAYMENTS {
        return Err(invalid("payments", "too many payments"));
    }
    super::shift::gate(payments, t.shift_open)?;
    for (i, p) in payments.iter().enumerate() {
        if p.amount_minor <= 0 {
            return Err(invalid(
                &format!("payments[{i}].amount_minor"),
                "each payment must be above 0",
            ));
        }
        check_reference(i, p)?;
    }
    let mut wallet_rows = payments
        .iter()
        .enumerate()
        .filter(|(_, p)| p.method == PaymentMethod::Wallet);
    if let Some((i, p)) = wallet_rows.next() {
        let field = format!("payments[{i}].method");
        if !has_player {
            return Err(invalid(&field, "a guest bill has no wallet"));
        }
        if wallet_rows.next().is_some() {
            return Err(invalid(&field, "use the wallet once on a bill"));
        }
        if p.amount_minor > t.wallet_minor {
            return Err(AppError::with_field(
                ErrorCode::InsufficientBalance,
                format!("payments[{i}].amount_minor"),
                "The wallet balance is too low.",
            ));
        }
    }
    validate_credit(payments, credit_override, t, has_player)?;
    let paid = payments
        .iter()
        .try_fold(0i64, |sum, p| sum.checked_add(p.amount_minor))
        .ok_or_else(|| invalid("payments", "the payments are too large"))?;
    if paid != total_minor {
        return Err(AppError::with_field(
            ErrorCode::PaymentShort,
            "payments",
            "The payments must add up to the bill.",
        ));
    }
    Ok(())
}

pub(super) struct Stamp<'a> {
    pub staff_id: &'a str,
    pub order_id: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub(super) fn insert_payments(
    tx: &StoreTx<'_>,
    stamp: &Stamp<'_>,
    payments: &[PaymentRow],
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    for (i, p) in payments.iter().enumerate() {
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
                order_id: stamp.order_id,
                method: p.method.as_str(),
                amount_minor: p.amount_minor,
                reference,
                staff_id: stamp.staff_id,
                position: i as i64,
                business_day: stamp.business_day,
                fy: stamp.fy,
            },
        )?;
        written.push(Written {
            key: RowKey::Payments { id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    super::shift::record_sales(tx, stamp.staff_id, stamp.order_id, payments, written)
}

pub(super) fn record_payer_txns(
    tx: &StoreTx<'_>,
    stamp: &Stamp<'_>,
    payments: &[PaymentRow],
    player_id: Option<&str>,
    credit: Option<&CreditRow>,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    if let (Some(player_id), Some(w)) = (
        player_id,
        payments.iter().find(|p| p.method == PaymentMethod::Wallet),
    ) {
        let txn_id = ulid::Ulid::generate().to_string();
        insert_wallet_txn(
            tx,
            &NewWalletTxn {
                id: &txn_id,
                player_id,
                delta_minor: -w.amount_minor,
                reason: "bill",
                method: None,
                staff_id: stamp.staff_id,
                order_id: stamp.order_id,
                business_day: stamp.business_day,
                fy: stamp.fy,
            },
        )?;
        adjust_wallet(tx, player_id, -w.amount_minor)?;
        written.push(Written {
            key: RowKey::WalletTxns { id: txn_id },
            op: ChangeOp::Insert,
            before: None,
        });
        written.push(Written {
            key: RowKey::Wallets {
                player_id: player_id.to_owned(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    if let (Some(player_id), Some(c), Some(credit)) = (
        player_id,
        payments.iter().find(|p| p.method == PaymentMethod::Credit),
        credit,
    ) {
        let over = credit.outstanding_minor + c.amount_minor > credit.limit_minor;
        let txn_id = ulid::Ulid::generate().to_string();
        insert_wallet_txn(
            tx,
            &NewWalletTxn {
                id: &txn_id,
                player_id,
                delta_minor: c.amount_minor,
                reason: "credit",
                method: over.then_some("override"),
                staff_id: stamp.staff_id,
                order_id: stamp.order_id,
                business_day: stamp.business_day,
                fy: stamp.fy,
            },
        )?;
        adjust_outstanding(tx, player_id, c.amount_minor)?;
        if over {
            set_override_day(tx, player_id, Some(stamp.business_day))?;
        }
        written.push(Written {
            key: RowKey::WalletTxns { id: txn_id },
            op: ChangeOp::Insert,
            before: None,
        });
        written.push(Written {
            key: RowKey::CreditBalances {
                player_id: player_id.to_owned(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(())
}

pub(super) fn mutate_settle(
    tx: &StoreTx<'_>,
    intent: &Intent,
    s: &SessionSettle,
    t: &SettleTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let stamp = Stamp {
        staff_id: &intent.actor.id,
        order_id: &s.order_id,
        business_day: &business_day,
        fy: &fy,
    };
    let mut written = Vec::with_capacity(s.payments.len() + 1);
    insert_payments(tx, &stamp, &s.payments, &mut written)?;
    settle_order(tx, &s.order_id, now_ms, &intent.actor.id)?;
    written.push(Written {
        key: RowKey::Orders {
            id: s.order_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    if let (Some(bill), Some(o)) = (&t.coupon, &t.order) {
        let adjust = bill.price(o.total_minor, s.discount.as_ref())?;
        if let Some(quote) = &adjust.coupon {
            super::coupon::redeem(tx, quote, &s.order_id, true, &mut written)?;
        }
        if let Some(d) = &adjust.discount {
            let by = intent
                .elevated_by
                .as_ref()
                .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
            super::discount::record(tx, d, &s.order_id, by, true)?;
        }
    }
    let player_id = t.order.as_ref().and_then(|o| o.player_id.as_deref());
    record_payer_txns(
        tx,
        &stamp,
        &s.payments,
        player_id,
        t.credit.as_ref(),
        &mut written,
    )?;
    Ok(written)
}

#[derive(Default)]
pub(crate) struct PauseTarget {
    session: Option<LiveSession>,
    open_segments: Vec<String>,
}

fn load_pausable(tx: &StoreTx<'_>, session_id: &str) -> Result<PauseTarget, AppError> {
    let Some(session) = find_session(tx, session_id)? else {
        return Ok(PauseTarget::default());
    };
    let open_segments = list_segments(tx, Some(&session.id))?
        .into_iter()
        .filter(|g| g.to_at.is_none())
        .map(|g| g.id)
        .collect();
    Ok(PauseTarget {
        session: Some(session),
        open_segments,
    })
}

pub(super) fn load_pause(tx: &StoreTx<'_>, p: &SessionPause) -> Result<PauseTarget, AppError> {
    load_pausable(tx, &p.session_id)
}

pub(super) fn load_resume(tx: &StoreTx<'_>, r: &SessionResume) -> Result<PauseTarget, AppError> {
    load_pausable(tx, &r.session_id)
}

pub(super) fn base_rev_pause(intent: &Intent, t: &PauseTarget, what: &str) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, s.rev, what)
}

fn postpaid_in_state<'a>(
    t: &'a PauseTarget,
    state: &str,
    wrong_state: &str,
) -> Result<&'a LiveSession, AppError> {
    let Some(s) = &t.session else {
        return Err(invalid("session_id", "no such session"));
    };
    if ["ended", "expired", "orphaned"].contains(&s.state.as_str()) {
        return Err(invalid("session_id", "this session has already ended"));
    }
    if s.mode != SessionMode::Postpaid.as_str() {
        return Err(invalid(
            "session_id",
            "only a postpaid session can be paused",
        ));
    }
    if s.state != state {
        return Err(invalid("session_id", wrong_state));
    }
    Ok(s)
}

pub(super) fn validate_pause(p: &SessionPause, t: &PauseTarget) -> Result<(), AppError> {
    let wrong = if t.session.as_ref().is_some_and(|s| s.state == "paused") {
        "this session is already paused"
    } else {
        "only a running session can be paused"
    };
    postpaid_in_state(t, "active", wrong)?;
    p.checked_note()?;
    Ok(())
}

pub(super) fn validate_resume(t: &PauseTarget) -> Result<(), AppError> {
    postpaid_in_state(t, "paused", "this session is not paused")?;
    Ok(())
}

pub(super) fn mutate_pause(
    tx: &StoreTx<'_>,
    p: &SessionPause,
    t: &PauseTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a pause validated without its target",
        ));
    };
    let note = p.checked_note()?;
    pause_session(tx, &s.id, now_ms, p.reason.as_str(), note.as_deref())?;
    Ok(vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }])
}

pub(super) fn mutate_resume(
    tx: &StoreTx<'_>,
    t: &PauseTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some((s, paused_at)) = t
        .session
        .as_ref()
        .and_then(|s| s.paused_at.map(|at| (s, at)))
    else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a resume validated without its target",
        ));
    };
    resume_session(tx, &s.id, (now_ms - paused_at).max(0))?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    for id in &t.open_segments {
        written.push(Written {
            key: RowKey::SessionSegments { id: id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

pub(super) fn undo_denied(message: &str) -> AppError {
    AppError::new(ErrorCode::UndoNotAllowed, message)
}

pub(super) fn in_window(now_ms: i64, at: Option<i64>) -> bool {
    at.is_some_and(|a| now_ms - a <= UNDO_WINDOW_MS)
}

pub(super) fn same_staff(intent: &Intent, by: Option<&str>) -> bool {
    by == Some(intent.actor.id.as_str())
}

pub(super) fn check_rev(intent: &Intent, current: i64, what: &str) -> Result<(), AppError> {
    match intent.base_rev {
        None => Err(invalid("base_rev", &format!("{what} needs base_rev"))),
        Some(b) if b != current => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        Some(_) => Ok(()),
    }
}

#[derive(Default)]
pub(crate) struct EndRevertTarget {
    pub(super) session: Option<LiveSession>,
    pub(super) order: Option<OrderRow>,
    segments: Vec<SegmentRow>,
    pub(super) device: Option<DeviceMetaRow>,
    pub(super) busy: bool,
    pub(super) paid: bool,
    pub(super) effect: bool,
    pub(super) now: i64,
}

pub(super) fn load_end_reverted(
    tx: &StoreTx<'_>,
    r: &SessionEndReverted,
    now: i64,
) -> Result<EndRevertTarget, AppError> {
    load_end_reverted_of(tx, &r.session_id, now, "session.end")
}

/// The end-undo target of `session_id`, whose forward end was an intent of kind `forward_kind`.
pub(super) fn load_end_reverted_of(
    tx: &StoreTx<'_>,
    session_id: &str,
    now: i64,
    forward_kind: &str,
) -> Result<EndRevertTarget, AppError> {
    let Some(session) = find_session(tx, session_id)? else {
        return Ok(EndRevertTarget {
            now,
            ..EndRevertTarget::default()
        });
    };
    let order = find_order_for_session(tx, &session.id)?;
    let device = find_device_meta(tx, &session.device_id)?;
    let busy = match &device {
        Some(d) => d.state == "in_session" || device_has_live_session(tx, &d.device_id)?,
        None => false,
    };
    let (paid, effect) = match &order {
        Some(o) => {
            let forward = forward_intent_id(tx, "sessions", &session.id, forward_kind)?;
            (
                !list_payment_ids(tx, &o.id)?.is_empty(),
                effect_recorded(tx, forward.as_deref(), &o.id)?,
            )
        }
        None => (false, false),
    };
    Ok(EndRevertTarget {
        segments: list_segments(tx, Some(&session.id))?,
        session: Some(session),
        order,
        device,
        busy,
        paid,
        effect,
        now,
    })
}

pub(super) fn base_rev_end_reverted(intent: &Intent, t: &EndRevertTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, s.rev, "session.end.reverted")
}

pub(super) fn validate_end_reverted(intent: &Intent, t: &EndRevertTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(invalid("session_id", "no such session"));
    };
    if s.state != "ended" || s.mode != SessionMode::Postpaid.as_str() {
        return Err(undo_denied("this session has not ended"));
    }
    if t.order.is_none() && s.group_bill_id.is_some() {
        return Err(undo_denied(
            "only the last session of a group to end can be undone",
        ));
    }
    if t.order.as_ref().is_none_or(|o| o.status != "open") || t.paid {
        return Err(undo_denied("the bill is no longer open and unpaid"));
    }
    if t.busy {
        return Err(undo_denied("This PC already has a session."));
    }
    if !t
        .device
        .as_ref()
        .is_some_and(|d| d.deleted_at.is_none() && d.state != "maintenance")
    {
        return Err(undo_denied("This PC is not available."));
    }
    if !same_staff(intent, s.ended_by.as_deref()) {
        return Err(undo_denied("only the staff member who ended it can undo"));
    }
    if !in_window(t.now, s.ended_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    Ok(())
}

pub(super) fn mutate_end_reverted(
    tx: &StoreTx<'_>,
    t: &EndRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let (Some(s), Some(o), Some(d), Some(ended_at)) = (
        &t.session,
        &t.order,
        &t.device,
        t.session.as_ref().and_then(|s| s.ended_at),
    ) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    reopen_segments(tx, &s.id, ended_at)?;
    reopen_session(tx, &s.id)?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    for seg in t.segments.iter().filter(|g| g.to_at == Some(ended_at)) {
        written.push(Written {
            key: RowKey::SessionSegments { id: seg.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    // Only the session (time) lines go: products placed on the bill stay, and with them the
    // order, whose total shrinks by the time lines.
    let segment_total = sum_segment_amounts(tx, &o.id)?;
    for id in list_segment_line_ids(tx, &o.id)? {
        let key = RowKey::OrderLines { id };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::OrderLines, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    let key = RowKey::Orders { id: o.id.clone() };
    if count_product_lines(tx, &o.id)? == 0 {
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Orders, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    } else {
        add_order_amount(tx, &o.id, -segment_total)?;
        written.push(Written {
            key,
            op: ChangeOp::Update,
            before: None,
        });
    }
    if let Some(group_id) = &o.group_bill_id {
        set_group_state(tx, group_id, "open")?;
        written.push(Written {
            key: RowKey::GroupBills {
                id: group_id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    set_device_state(tx, &d.device_id, "in_session", Some(&d.state), None)?;
    written.push(Written {
        key: RowKey::DevicesMeta {
            device_id: d.device_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

#[derive(Default)]
pub(crate) struct SettleRevertTarget {
    pub(super) order: Option<OrderRow>,
    pub(super) payments: Vec<String>,
    pub(super) txns: Vec<OrderTxn>,
    credit_blocked: bool,
    effect: bool,
    now: i64,
}

impl SettleRevertTarget {
    /// The payments and wallet debits of the bill an end-undo target would drop, to be undone
    /// before the bill itself goes.
    pub(super) fn of(tx: &StoreTx<'_>, end: &EndRevertTarget) -> Result<Self, AppError> {
        let Some(order) = end.order.clone() else {
            return Ok(Self::default());
        };
        Ok(Self {
            payments: list_payment_ids(tx, &order.id)?,
            txns: list_order_wallet_txns(tx, &order.id)?,
            order: Some(order),
            ..Self::default()
        })
    }
}

pub(super) fn load_settle_reverted(
    tx: &StoreTx<'_>,
    r: &SessionSettleReverted,
    now: i64,
) -> Result<SettleRevertTarget, AppError> {
    let Some(order) = find_order(tx, &r.order_id)? else {
        return Ok(SettleRevertTarget {
            now,
            ..SettleRevertTarget::default()
        });
    };
    let forward = forward_intent_id(tx, "orders", &order.id, "session.settle")?;
    let txns = list_order_wallet_txns(tx, &order.id)?;
    let mut credit_blocked = false;
    for txn in txns.iter().filter(|t| t.reason == "credit") {
        let owed = find_credit(tx, &txn.player_id)?.map_or(0, |c| c.outstanding_minor);
        credit_blocked |= owed < txn.delta_minor;
    }
    Ok(SettleRevertTarget {
        payments: list_payment_ids(tx, &order.id)?,
        txns,
        credit_blocked,
        effect: effect_recorded(tx, forward.as_deref(), &order.id)?,
        order: Some(order),
        now,
    })
}

pub(super) fn base_rev_settle_reverted(
    intent: &Intent,
    t: &SettleRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "order_id",
            "no such bill",
        ));
    };
    check_rev(intent, o.rev, "session.settle.reverted")
}

pub(super) fn validate_settle_reverted(
    intent: &Intent,
    t: &SettleRevertTarget,
) -> Result<(), AppError> {
    let Some(o) = &t.order else {
        return Err(invalid("order_id", "no such bill"));
    };
    if o.kind != "session" || o.session_id.is_none() {
        return Err(invalid("order_id", "this is not a session bill"));
    }
    if o.status != "settled" {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "order_id",
            "this bill is not settled",
        ));
    }
    if !same_staff(intent, o.settled_by.as_deref()) {
        return Err(undo_denied("only the staff member who settled it can undo"));
    }
    if !in_window(t.now, o.settled_at) {
        return Err(undo_denied("the undo window has closed"));
    }
    if t.effect {
        return Err(undo_denied("a receipt, drawer or invoice already happened"));
    }
    if t.credit_blocked {
        return Err(undo_denied("the credit has already been collected"));
    }
    Ok(())
}

pub(super) fn mutate_settle_reverted(
    tx: &StoreTx<'_>,
    t: &SettleRevertTarget,
) -> Result<Vec<Written>, AppError> {
    let Some(o) = &t.order else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an undo validated without its target",
        ));
    };
    let mut written = Vec::with_capacity(t.payments.len() + 1);
    super::shift::reverse_sales(
        tx,
        o.settled_by.as_deref().unwrap_or_default(),
        &o.id,
        &mut written,
    )?;
    super::receipt_notify::skip_on_revert(tx, &o.id, &mut written)?;
    for id in &t.payments {
        let key = RowKey::Payments { id: id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::Payments, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
    }
    for txn in &t.txns {
        let key = RowKey::WalletTxns { id: txn.id.clone() };
        let before = read_row(tx, &key)?;
        delete_row(tx, Table::WalletTxns, &before)?;
        written.push(Written {
            key,
            op: ChangeOp::Delete,
            before: Some(before),
        });
        if txn.reason == "credit" {
            adjust_outstanding(tx, &txn.player_id, -txn.delta_minor)?;
            if txn.method.as_deref() == Some("override") {
                set_override_day(tx, &txn.player_id, None)?;
            }
            written.push(Written {
                key: RowKey::CreditBalances {
                    player_id: txn.player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        } else {
            adjust_wallet(tx, &txn.player_id, -txn.delta_minor)?;
            written.push(Written {
                key: RowKey::Wallets {
                    player_id: txn.player_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    super::coupon::release_on_revert(tx, &o.id, true, &mut written)?;
    super::discount::release_on_revert(tx, &o.id, true)?;
    reopen_order(tx, &o.id)?;
    written.push(Written {
        key: RowKey::Orders { id: o.id.clone() },
        op: ChangeOp::Update,
        before: None,
    });
    Ok(written)
}

#[derive(Default)]
pub(crate) struct AssignTarget {
    session: Option<LiveSession>,
    order: Option<OrderRow>,
    player: bool,
    create: Option<super::players::PlayerTarget>,
}

pub(super) fn load_assign(
    tx: &StoreTx<'_>,
    a: &SessionAssignPlayer,
) -> Result<AssignTarget, AppError> {
    let Some(session) = find_session(tx, &a.session_id)? else {
        return Ok(AssignTarget::default());
    };
    Ok(AssignTarget {
        order: find_order_for_session(tx, &session.id)?,
        player: find_player(tx, &a.player_id)?.is_some(),
        create: a
            .create
            .as_ref()
            .map(|c| super::players::load_create(tx, c))
            .transpose()?,
        session: Some(session),
    })
}

pub(super) fn base_rev_assign(intent: &Intent, t: &AssignTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "session_id",
            "no such session",
        ));
    };
    check_rev(intent, s.rev, "session.assign_player")
}

pub(super) fn validate_assign(a: &SessionAssignPlayer, t: &AssignTarget) -> Result<(), AppError> {
    let Some(s) = &t.session else {
        return Err(invalid("session_id", "no such session"));
    };
    if s.player_id.is_some() {
        return Err(invalid("session_id", "this session already has a player"));
    }
    if s.group_bill_id.is_some() {
        return Err(invalid(
            "session_id",
            "a session in a group bill cannot be converted; change the group payer",
        ));
    }
    if t.order.as_ref().is_some_and(|o| o.status != "open") {
        return Err(invalid("session_id", "this bill is already settled"));
    }
    match (&a.create, &t.create) {
        (Some(c), Some(target)) => {
            if c.player_id != a.player_id {
                return Err(invalid("player_id", "the new player does not match"));
            }
            super::players::validate(c, target)
        }
        (None, _) if !t.player => Err(AppError::with_field(
            ErrorCode::NotFound,
            "player_id",
            "no such player",
        )),
        _ => Ok(()),
    }
}

pub(super) fn mutate_assign(
    tx: &StoreTx<'_>,
    a: &SessionAssignPlayer,
    t: &AssignTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let Some(s) = &t.session else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "an assignment validated without its target",
        ));
    };
    let mut written = match &a.create {
        Some(c) => super::players::mutate(tx, c, now_ms)?,
        None => Vec::new(),
    };
    assign_session_player(tx, &s.id, &a.player_id)?;
    written.push(Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    });
    if let Some(o) = &t.order {
        assign_order_player(tx, &o.id, &a.player_id)?;
        written.push(Written {
            key: RowKey::Orders { id: o.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

fn validate_credit(
    payments: &[PaymentRow],
    credit_override: Option<bool>,
    t: &SettleTarget,
    has_player: bool,
) -> Result<(), AppError> {
    let mut rows = payments
        .iter()
        .enumerate()
        .filter(|(_, p)| p.method == PaymentMethod::Credit);
    let Some((i, p)) = rows.next() else {
        return Ok(());
    };
    let method = format!("payments[{i}].method");
    if !has_player {
        return Err(invalid(&method, "a guest bill has no credit"));
    }
    if rows.next().is_some() {
        return Err(invalid(&method, "use credit once on a bill"));
    }
    let Some(credit) = t.credit.as_ref().filter(|c| c.limit_minor > 0) else {
        return Err(invalid(&method, "credit is not enabled for this player"));
    };
    if t.credit_blocked {
        return Err(AppError::with_field(
            ErrorCode::CreditPaused,
            method,
            "Credit paused — 15+ days overdue.",
        ));
    }
    let owed = credit
        .outstanding_minor
        .checked_add(p.amount_minor)
        .ok_or_else(|| invalid("payments", "the payments are too large"))?;
    if owed <= credit.limit_minor {
        return Ok(());
    }
    let amount = format!("payments[{i}].amount_minor");
    if credit_override != Some(true) {
        return Err(AppError::with_field(
            ErrorCode::CreditLimit,
            amount,
            "Credit limit reached, see counter.",
        ));
    }
    if credit.last_override_day.as_deref() == Some(t.business_day.as_str()) {
        return Err(AppError::with_field(
            ErrorCode::OverrideUsedToday,
            amount,
            "A manager already overrode this limit today.",
        ));
    }
    Ok(())
}

/// The wallet balance a spend may use: the balance less what the player's running wallet
/// sessions have set aside.
pub(super) fn available_wallet(tx: &StoreTx<'_>, player_id: &str) -> Result<i64, AppError> {
    let balance = find_wallet(tx, player_id)?.map_or(0, |w| w.balance_minor);
    Ok((balance - sum_reserved_minor(tx, player_id)?).max(0))
}

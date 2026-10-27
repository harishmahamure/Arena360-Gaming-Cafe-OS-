//! Sessions paid for up front: starting on a pack, a pass, a block or the wallet, and the
//! close-out they share whether staff end them or the timer expires them.

use arena_proto::{
    AppError, BillLine, ErrorCode, PaymentMethod, PaymentRow, SessionMode, SessionStart,
};
use arena_store::{
    ChangeOp, DeviceMetaRow, LiveSession, NewOrder, NewOrderLine, PlanRow, PlayerPackRow, RateRow,
    RowKey, SegmentRow, StoreTx, close_segments, deduct_pack_minutes, end_session, expire_session,
    find_plan, find_player_pack, find_wallet, insert_order, insert_order_line, list_plan_groups,
    pack_in_use, set_device_state, settle_order, sum_reserved_minor,
};

use super::mutate::Written;
use super::session::{Stamp, insert_payments, invalid, record_payer_txns};
use crate::billing::{self, WalletHorizon};
use crate::windows::PlayWindow;

const MIN_MS: i64 = 60_000;

/// What a pack, pass, block or wallet start reads before it validates.
#[derive(Default)]
pub(super) struct StartExtras {
    pack: Option<PlayerPackRow>,
    pack_busy: bool,
    horizon: Option<WalletHorizon>,
    short: Option<String>,
    plan: Option<PlanRow>,
    plan_groups: Vec<String>,
    window: Option<PlayWindow>,
    window_ends_at: Option<i64>,
    no_zone: bool,
    now: i64,
}

/// The end time and set-aside amount a started prepaid session gets.
pub(super) struct StartPlan {
    pub ends_at: Option<i64>,
    pub reserved_minor: i64,
    pub pack_id: Option<String>,
}

pub(super) fn load_start(
    tx: &StoreTx<'_>,
    s: &SessionStart,
    rate: Option<&RateRow>,
    now_ms: i64,
) -> Result<StartExtras, AppError> {
    let mut extras = StartExtras {
        now: now_ms,
        ..StartExtras::default()
    };
    let tz = crate::locale::tz_at(tx, now_ms)?;
    let open_window = |extras: &mut StartExtras, start, end| {
        extras.window = PlayWindow::new(start, end);
        extras.window_ends_at = extras
            .window
            .zip(tz)
            .and_then(|(w, tz)| w.ends_at(now_ms, tz));
        extras.no_zone = extras.window.is_some() && tz.is_none();
    };
    if s.mode == SessionMode::Block {
        if let Some(plan_id) = s.plan_id.as_deref() {
            extras.plan = find_plan(tx, plan_id)?.filter(|p| p.kind == "block" && p.active);
            if let Some(p) = extras.plan.clone() {
                extras.plan_groups = list_plan_groups(tx, &p.plan_id)?;
                open_window(&mut extras, p.window_start_min, p.window_end_min);
            }
        }
        return Ok(extras);
    }
    let Some(player_id) = s.player_id.as_deref() else {
        return Ok(extras);
    };
    match s.mode {
        SessionMode::Pack | SessionMode::Pass => {
            if let Some(pack_id) = s.pack_id.as_deref() {
                extras.pack = find_player_pack(tx, player_id, pack_id)?;
                extras.pack_busy = extras.pack.is_some() && pack_in_use(tx, pack_id)?;
                if let Some(p) = extras.pack.clone() {
                    open_window(&mut extras, p.window_start_min, p.window_end_min);
                }
            }
        }
        SessionMode::Wallet => {
            let Some(rate) = rate else {
                return Ok(extras);
            };
            let balance = find_wallet(tx, player_id)?.map_or(0, |w| w.balance_minor);
            let available = (balance - sum_reserved_minor(tx, player_id)?).max(0);
            let rules = billing::read(tx)?;
            let price = rate.price_per_hour_minor;
            extras.horizon =
                billing::wallet_horizon(price, rules.rounding_min, rules.minimum_min, available);
            if extras.horizon.is_none() {
                let first = billing::minimum_bill(price, rules.rounding_min, rules.minimum_min);
                let money = |minor| crate::locale::format_money(tx, now_ms, minor);
                extras.short = Some(format!(
                    "Balance {}. Top up at counter. Rate {} an hour, minimum {} ({} min).",
                    money(available)?,
                    money(price)?,
                    money(first.cost_minor)?,
                    first.minutes,
                ));
            }
        }
        _ => {}
    }
    Ok(extras)
}

/// The checks a pack or wallet start adds to the ones every start makes.
pub(super) fn check_start(
    s: &SessionStart,
    group_id: &str,
    x: &StartExtras,
) -> Result<(), AppError> {
    if s.plan_id.is_some() && s.mode != SessionMode::Block {
        return Err(invalid("plan_id", "only a block session takes a plan"));
    }
    match s.mode {
        SessionMode::Postpaid => match s.pack_id {
            None => Ok(()),
            Some(_) => Err(invalid("pack_id", "only a pack or pass session takes one")),
        },
        SessionMode::Wallet => {
            if s.pack_id.is_some() {
                return Err(invalid("pack_id", "only a pack or pass session takes one"));
            }
            match (&x.horizon, &x.short) {
                (Some(_), _) => Ok(()),
                (None, short) => Err(AppError::with_field(
                    ErrorCode::InsufficientBalance,
                    "player_id",
                    short.clone().unwrap_or_else(|| {
                        "The wallet balance is too low. Top up at counter.".to_owned()
                    }),
                )),
            }
        }
        SessionMode::Pack | SessionMode::Pass => check_pack(s, group_id, x),
        SessionMode::Block => check_block(s, group_id, x),
    }
}

fn check_pack(s: &SessionStart, group_id: &str, x: &StartExtras) -> Result<(), AppError> {
    let pass = s.mode == SessionMode::Pass;
    let (kind, noun) = if pass {
        ("pass", "pass")
    } else {
        ("pack", "pack")
    };
    let Some(pack_id) = s.pack_id.as_deref() else {
        return Err(invalid("pack_id", &format!("choose a {noun}")));
    };
    let pack = x
        .pack
        .as_ref()
        .filter(|p| p.kind == kind && p.id == pack_id)
        .ok_or_else(|| {
            AppError::with_field(ErrorCode::NotFound, "pack_id", format!("no such {noun}"))
        })?;
    if !pack.group_ids.iter().any(|g| g == group_id) {
        return Err(AppError::with_field(
            ErrorCode::PlanNotForGroup,
            "pack_id",
            format!("This {noun} is not for this PC's group."),
        ));
    }
    if pack.expires_at <= x.now {
        return Err(AppError::with_field(
            ErrorCode::PackExpired,
            "pack_id",
            format!("This {noun} has expired."),
        ));
    }
    if !pass && pack.minutes_left.unwrap_or(0) <= 0 {
        return Err(AppError::with_field(
            ErrorCode::PackExpired,
            "pack_id",
            "This pack has no time left.",
        ));
    }
    if x.pack_busy {
        return Err(invalid(
            "pack_id",
            &format!("This {noun} is already running a session."),
        ));
    }
    if pass {
        window_open("pack_id", "This pass plays", x)?;
    }
    Ok(())
}

fn check_block(s: &SessionStart, group_id: &str, x: &StartExtras) -> Result<(), AppError> {
    if s.pack_id.is_some() {
        return Err(invalid("pack_id", "only a pack or pass session takes one"));
    }
    if s.plan_id.is_none() {
        return Err(invalid("plan_id", "choose a block"));
    }
    if x.plan.is_none() {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "plan_id",
            "no such block",
        ));
    }
    if !x.plan_groups.iter().any(|g| g == group_id) {
        return Err(AppError::with_field(
            ErrorCode::PlanNotForGroup,
            "plan_id",
            "This block is not for this PC's group.",
        ));
    }
    window_open("plan_id", "This block starts", x)
}

/// Refuses `PASS_OUTSIDE_WINDOW`, naming the window, when now is outside it.
fn window_open(field: &str, lead: &str, x: &StartExtras) -> Result<(), AppError> {
    if x.no_zone {
        return Err(AppError::new(
            ErrorCode::Internal,
            "the branch has not been bootstrapped",
        ));
    }
    match (&x.window, x.window_ends_at) {
        (Some(_), Some(_)) => Ok(()),
        (Some(w), None) => Err(AppError::with_field(
            ErrorCode::PassOutsideWindow,
            field,
            format!("{lead} {}.", w.label()),
        )),
        (None, _) => Err(AppError::new(
            ErrorCode::Internal,
            "a pass or block has no play window",
        )),
    }
}

/// The end time and reservation of a validated start at `now_ms`.
pub(super) fn plan_start(
    s: &SessionStart,
    x: &StartExtras,
    now_ms: i64,
) -> Result<StartPlan, AppError> {
    let internal = || AppError::new(ErrorCode::Internal, "a session validated without its plan");
    match s.mode {
        SessionMode::Pack => {
            let pack = x.pack.as_ref().ok_or_else(internal)?;
            let left = pack.minutes_left.ok_or_else(internal)?;
            Ok(StartPlan {
                ends_at: Some(now_ms + left * MIN_MS),
                reserved_minor: 0,
                pack_id: Some(pack.id.clone()),
            })
        }
        SessionMode::Pass => {
            let pack = x.pack.as_ref().ok_or_else(internal)?;
            Ok(StartPlan {
                ends_at: Some(x.window_ends_at.ok_or_else(internal)?),
                reserved_minor: 0,
                pack_id: Some(pack.id.clone()),
            })
        }
        SessionMode::Block => Ok(StartPlan {
            ends_at: Some(x.window_ends_at.ok_or_else(internal)?),
            reserved_minor: 0,
            pack_id: None,
        }),
        SessionMode::Wallet => {
            let h = x.horizon.ok_or_else(internal)?;
            Ok(StartPlan {
                ends_at: Some(now_ms + h.minutes * MIN_MS),
                reserved_minor: h.cost_minor,
                pack_id: None,
            })
        }
        _ => Ok(StartPlan {
            ends_at: None,
            reserved_minor: 0,
            pack_id: None,
        }),
    }
}

/// What a close-out reads: the owned pack of a pack session and the wallet of a wallet session.
#[derive(Default)]
pub(super) struct CloseExtras {
    pack: Option<PlayerPackRow>,
    wallet_minor: i64,
}

pub(super) fn is_prepaid(s: &LiveSession) -> bool {
    [SessionMode::Pack, SessionMode::Pass, SessionMode::Wallet]
        .iter()
        .any(|m| s.mode == m.as_str())
}

pub(super) fn load_close(tx: &StoreTx<'_>, s: &LiveSession) -> Result<CloseExtras, AppError> {
    let (Some(player_id), true) = (s.player_id.as_deref(), is_prepaid(s)) else {
        return Ok(CloseExtras::default());
    };
    Ok(CloseExtras {
        pack: match s.pack_id.as_deref() {
            Some(pack_id) => find_player_pack(tx, player_id, pack_id)?,
            None => None,
        },
        wallet_minor: find_wallet(tx, player_id)?.map_or(0, |w| w.balance_minor),
    })
}

/// Where a wallet session's own time stops: its end less the minutes extended since, which were
/// charged when they were added.
fn base_end(s: &LiveSession) -> Option<i64> {
    s.ends_at
        .filter(|_| s.extended_min > 0)
        .map(|end| end - s.extended_min * MIN_MS)
}

/// How a prepaid session ended, and so who closed it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    Staff,
    Expired,
}

pub(super) struct Closing<'a> {
    pub session: &'a LiveSession,
    pub segments: &'a [SegmentRow],
    pub device: Option<&'a DeviceMetaRow>,
    pub extras: &'a CloseExtras,
    pub ending: Ending,
    pub ended_at: i64,
    pub actor_id: &'a str,
}

/// Ends a pack or wallet session and settles its bill at once.
///
/// A pack session writes a zero bill with the unpaused minutes (rounded up, capped at the pack's
/// minutes left) and takes them off the pack. A wallet session bills by the postpaid formula,
/// pays it from the wallet in one payment (never more than the balance: any shortfall comes off
/// the last line so the lines, the total and the payment still agree) and debits the wallet.
pub(super) fn close_out(
    tx: &StoreTx<'_>,
    c: &Closing<'_>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let s = c.session;
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    close_segments(tx, &s.id, c.ended_at)?;
    match c.ending {
        Ending::Staff => end_session(tx, &s.id, c.ended_at, c.actor_id)?,
        Ending::Expired => expire_session(tx, &s.id, c.ended_at)?,
    }
    let is_pack = s.mode == SessionMode::Pack.as_str();
    let zero_bill = is_pack || s.mode == SessionMode::Pass.as_str();
    let mut lines = if zero_bill {
        billing::bill(c.segments, 0, 0, None, c.ended_at)
    } else {
        let bill_at = base_end(s).map_or(c.ended_at, |end| c.ended_at.min(end));
        billing::bill(c.segments, s.rounding_min, s.minimum_min, None, bill_at)
    };
    let left = c.extras.pack.as_ref().and_then(|p| p.minutes_left);
    let used = lines.iter().map(|l| l.minutes).sum::<i64>();
    let used = left.map_or(used, |l| used.min(l));
    let debit = if zero_bill {
        0
    } else {
        let subtotal: i64 = lines.iter().map(|l| l.amount_minor).sum();
        let debit = subtotal.min(c.extras.wallet_minor.max(0));
        trim_lines(&mut lines, subtotal - debit);
        debit
    };
    let order_id = ulid::Ulid::generate().to_string();
    insert_order(
        tx,
        &NewOrder {
            id: &order_id,
            session_id: &s.id,
            player_id: s.player_id.as_deref(),
            guest_no: s.guest_no,
            device_id: &s.device_id,
            subtotal_minor: debit,
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
    for (i, (seg, line)) in c.segments.iter().zip(&lines).enumerate() {
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
    let stamp = Stamp {
        staff_id: c.actor_id,
        order_id: &order_id,
        business_day: &business_day,
        fy: &fy,
    };
    if debit > 0 {
        let payments = [PaymentRow {
            method: PaymentMethod::Wallet,
            amount_minor: debit,
            reference: None,
        }];
        insert_payments(tx, &stamp, &payments, &mut written)?;
        record_payer_txns(
            tx,
            &stamp,
            &payments,
            s.player_id.as_deref(),
            None,
            &mut written,
        )?;
    }
    settle_order(tx, &order_id, now_ms, c.actor_id)?;
    if let Some(pack) = c.extras.pack.as_ref().filter(|_| is_pack) {
        deduct_pack_minutes(tx, &pack.id, used)?;
        written.push(Written {
            key: RowKey::PlayerPacks {
                id: pack.id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    if let Some(d) = c.device {
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

/// The rows of a just-inserted block session that its bill hangs from.
pub(super) struct BlockBill<'a> {
    pub session_id: &'a str,
    pub segment_id: &'a str,
    pub player_id: Option<&'a str>,
    pub guest_no: Option<i64>,
    pub device_id: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
    pub ends_at: i64,
}

/// Opens the bill of a block session at its start: one line at the block price, unpaid until
/// staff settle it (now or at the end).
pub(super) fn open_block_bill(
    tx: &StoreTx<'_>,
    x: &StartExtras,
    b: &BlockBill<'_>,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let plan = x
        .plan
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "a block validated without its plan"))?;
    let order_id = ulid::Ulid::generate().to_string();
    let line_id = ulid::Ulid::generate().to_string();
    insert_order(
        tx,
        &NewOrder {
            id: &order_id,
            session_id: b.session_id,
            player_id: b.player_id,
            guest_no: b.guest_no,
            device_id: b.device_id,
            subtotal_minor: plan.price_minor,
            business_day: b.business_day,
            fy: b.fy,
        },
    )?;
    insert_order_line(
        tx,
        &NewOrderLine {
            id: &line_id,
            order_id: &order_id,
            segment_id: b.segment_id,
            device_id: b.device_id,
            from_at: now_ms,
            to_at: b.ends_at,
            minutes: (b.ends_at - now_ms + MIN_MS - 1) / MIN_MS,
            rate_minor_per_hour: 0,
            amount_minor: plan.price_minor,
            position: 0,
        },
    )?;
    Ok(vec![
        Written {
            key: RowKey::Orders { id: order_id },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::OrderLines { id: line_id },
            op: ChangeOp::Insert,
            before: None,
        },
    ])
}

/// Ends a block session: the block price was billed at the start, so ending early charges
/// nothing more and refunds nothing.
pub(super) fn end_block(
    tx: &StoreTx<'_>,
    s: &LiveSession,
    segments: &[SegmentRow],
    device: Option<&DeviceMetaRow>,
    ended_at: i64,
    actor_id: &str,
) -> Result<Vec<Written>, AppError> {
    close_segments(tx, &s.id, ended_at)?;
    end_session(tx, &s.id, ended_at, actor_id)?;
    let mut written = vec![Written {
        key: RowKey::Sessions { id: s.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }];
    for seg in segments.iter().filter(|g| g.to_at.is_none()) {
        written.push(Written {
            key: RowKey::SessionSegments { id: seg.id.clone() },
            op: ChangeOp::Update,
            before: None,
        });
    }
    if let Some(d) = device {
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

fn trim_lines(lines: &mut [BillLine], mut shortfall: i64) {
    for line in lines.iter_mut().rev() {
        let cut = shortfall.min(line.amount_minor);
        line.amount_minor -= cut;
        shortfall -= cut;
    }
}

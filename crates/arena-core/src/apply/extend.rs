//! Extending a running prepaid session from the Hub: more time, paid from the wallet, a pack or
//! credit.
//!
//! The added time is charged as its own settled order (tied to the session by
//! `orders.extends_session_id`), so the session's close-out bill is not touched: a pack session
//! still takes only its own pack's minutes and a wallet session is billed only up to its original
//! end (see [`super::prepaid`]). Ending early refunds nothing, like a block.

use arena_proto::{
    AppError, DEFAULT_OPENING_HOURS, EXTEND_OPTIONS_MIN, ErrorCode, ExtendSource, Intent,
    KEY_OPENING_HOURS, PaymentMethod, PaymentRow, SessionExtend, SessionMode, parse_opening_hours,
};
use arena_store::{
    ChangeOp, LiveSession, NewExtensionOrder, PlayerPackRow, RateRow, RowKey, StoreTx,
    deduct_pack_minutes, extend_session, find_active_rate, find_device_meta, find_player_pack,
    find_session, find_setting, insert_extension_order, list_segments, pack_in_use,
};

use super::mutate::Written;
use super::session::{
    SettleTarget, Stamp, insert_payments, invalid, load_payer, record_payer_txns, validate_payments,
};

const MIN_MS: i64 = 60_000;

#[derive(Default)]
pub(crate) struct ExtendTarget {
    session: Option<LiveSession>,
    group_id: Option<String>,
    segment_id: Option<String>,
    rate: Option<RateRow>,
    pack: Option<PlayerPackRow>,
    pack_busy: bool,
    price_minor: i64,
    payments: Vec<PaymentRow>,
    short: Option<String>,
    payer: SettleTarget,
    closing_at: Option<i64>,
    now: i64,
}

/// The half-up price of `minutes` at `rate_minor_per_hour`, the way [`crate::billing::bill`]
/// prices a line.
fn price_of(rate_minor_per_hour: i64, minutes: i64) -> i64 {
    let amount = (i128::from(rate_minor_per_hour) * i128::from(minutes) + 30) / 60;
    i64::try_from(amount).unwrap_or(i64::MAX)
}

/// The next closing time after `now_ms` from the branch opening hours, if the branch keeps any.
fn closing_at(tx: &StoreTx<'_>, now_ms: i64) -> Result<Option<i64>, AppError> {
    let Some(tz) = crate::locale::tz_at(tx, now_ms)? else {
        return Ok(None);
    };
    let hours = find_setting(tx, KEY_OPENING_HOURS)?;
    let Ok(week) = parse_opening_hours(hours.as_ref().map_or(DEFAULT_OPENING_HOURS, |s| &s.value))
    else {
        return Ok(None);
    };
    Ok(crate::session_screen::closes_in(&week, tz, now_ms)
        .and_then(|ms| i64::try_from(ms).ok())
        .map(|ms| now_ms + ms))
}

pub(super) fn load(
    tx: &StoreTx<'_>,
    e: &SessionExtend,
    now_ms: i64,
) -> Result<ExtendTarget, AppError> {
    let mut t = ExtendTarget {
        now: now_ms,
        ..ExtendTarget::default()
    };
    let Some(session) = find_session(tx, &e.session_id)? else {
        return Ok(t);
    };
    t.group_id = find_device_meta(tx, &session.device_id)?.and_then(|d| d.group_id);
    t.rate = match &t.group_id {
        Some(g) => find_active_rate(tx, g)?,
        None => None,
    };
    t.segment_id = list_segments(tx, Some(&session.id))?
        .into_iter()
        .rev()
        .find(|g| g.to_at.is_none())
        .map(|g| g.id);
    t.closing_at = closing_at(tx, now_ms)?;
    let player_id = session.player_id.clone();
    if e.source == ExtendSource::Pack {
        if let (Some(player_id), Some(pack_id)) = (player_id.as_deref(), e.pack_id.as_deref()) {
            t.pack = find_player_pack(tx, player_id, pack_id)?;
            t.pack_busy = t.pack.is_some() && pack_in_use(tx, pack_id)?;
        }
    } else if let Some(rate) = &t.rate {
        t.price_minor = price_of(rate.price_per_hour_minor, e.minutes);
        if t.price_minor > 0 {
            let method = if e.source == ExtendSource::Credit {
                PaymentMethod::Credit
            } else {
                PaymentMethod::Wallet
            };
            t.payments = vec![PaymentRow {
                method,
                amount_minor: t.price_minor,
                reference: None,
            }];
        }
    }
    t.payer = load_payer(tx, player_id.as_deref(), &t.payments, now_ms)?;
    if e.source == ExtendSource::Wallet && t.price_minor > t.payer.wallet_minor {
        let money = |minor| crate::locale::format_money(tx, now_ms, minor);
        t.short = Some(format!(
            "Balance {}. Top up at counter.",
            money(t.payer.wallet_minor)?
        ));
    }
    t.session = Some(session);
    Ok(t)
}

fn not_found(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::NotFound, field, message)
}

fn source_error(mut e: AppError) -> AppError {
    e.field = Some("source".to_owned());
    e
}

pub(super) fn validate(
    intent: &Intent,
    e: &SessionExtend,
    t: &ExtendTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "session.extend takes no base_rev"));
    }
    let Some(s) = &t.session else {
        return Err(not_found("session_id", "no such session"));
    };
    if !EXTEND_OPTIONS_MIN.contains(&e.minutes) {
        return Err(invalid("minutes", "extend by 30 min, 1 h or 2 h"));
    }
    if s.state == "paused" {
        return Err(invalid(
            "session_id",
            "resume the session before extending it",
        ));
    }
    if !["active", "grace"].contains(&s.state.as_str()) {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "session_id",
            "this session is no longer running",
        ));
    }
    if s.mode == SessionMode::Postpaid.as_str() {
        return Err(invalid(
            "session_id",
            "a postpaid session is open, it has no end to extend",
        ));
    }
    if s.mode != SessionMode::Pack.as_str() && s.mode != SessionMode::Wallet.as_str() {
        return Err(invalid(
            "session_id",
            "a pass or block plays its fixed window",
        ));
    }
    let (Some(ends_at), Some(_)) = (s.ends_at, s.player_id.as_deref()) else {
        return Err(invalid("session_id", "this session has no end time"));
    };
    let new_end = ends_at + e.minutes * MIN_MS;
    if new_end <= t.now {
        return Err(invalid(
            "minutes",
            "this session ended too long ago to extend",
        ));
    }
    if t.closing_at.is_some_and(|closing| new_end > closing) {
        return Err(AppError::with_field(
            ErrorCode::PastClosing,
            "minutes",
            "That would run past closing time.",
        ));
    }
    match e.source {
        ExtendSource::Pack => validate_pack(e, s, t),
        ExtendSource::Wallet | ExtendSource::Credit => {
            if e.pack_id.is_some() {
                return Err(invalid("pack_id", "only a pack source takes one"));
            }
            if t.rate.is_none() {
                return Err(AppError::with_field(
                    ErrorCode::NoRate,
                    "session_id",
                    "This PC's group has no rate.",
                ));
            }
            if let Some(short) = &t.short {
                return Err(AppError::with_field(
                    ErrorCode::InsufficientBalance,
                    "source",
                    short.clone(),
                ));
            }
            validate_payments(
                &t.payments,
                e.credit_override,
                &t.payer,
                true,
                t.price_minor,
            )
            .map_err(source_error)
        }
    }
}

fn validate_pack(e: &SessionExtend, s: &LiveSession, t: &ExtendTarget) -> Result<(), AppError> {
    let Some(pack_id) = e.pack_id.as_deref() else {
        return Err(invalid("pack_id", "choose a pack"));
    };
    let pack = t
        .pack
        .as_ref()
        .filter(|p| p.kind == "pack" && p.id == pack_id)
        .ok_or_else(|| not_found("pack_id", "no such pack"))?;
    let group_ok = t
        .group_id
        .as_deref()
        .is_some_and(|g| pack.group_ids.iter().any(|p| p == g));
    if !group_ok {
        return Err(AppError::with_field(
            ErrorCode::PlanNotForGroup,
            "pack_id",
            "This pack is not for this PC's group.",
        ));
    }
    if pack.expires_at <= t.now || pack.minutes_left.unwrap_or(0) <= 0 {
        return Err(AppError::with_field(
            ErrorCode::PackExpired,
            "pack_id",
            "This pack has expired or has no time left.",
        ));
    }
    if t.pack_busy || s.pack_id.as_deref() == Some(pack_id) {
        return Err(invalid(
            "pack_id",
            "This pack is already running a session.",
        ));
    }
    let left = pack.minutes_left.unwrap_or(0);
    if left < e.minutes {
        return Err(AppError::with_field(
            ErrorCode::InsufficientBalance,
            "pack_id",
            format!("This pack has {left} min left."),
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    e: &SessionExtend,
    t: &ExtendTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let internal = || {
        AppError::new(
            ErrorCode::Internal,
            "an extend validated without its target",
        )
    };
    let s = t.session.as_ref().ok_or_else(internal)?;
    let ends_at = s.ends_at.ok_or_else(internal)?;
    let segment_id = t.segment_id.as_deref().ok_or_else(internal)?;
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let new_end = ends_at + e.minutes * MIN_MS;
    extend_session(tx, &s.id, new_end, e.minutes)?;
    let order_id = ulid::Ulid::generate().to_string();
    let line_id = ulid::Ulid::generate().to_string();
    let (rate, amount) = match e.source {
        ExtendSource::Pack => (0, 0),
        _ => (
            t.rate.as_ref().ok_or_else(internal)?.price_per_hour_minor,
            t.price_minor,
        ),
    };
    insert_extension_order(
        tx,
        &NewExtensionOrder {
            order_id: &order_id,
            line_id: &line_id,
            session_id: &s.id,
            segment_id,
            player_id: s.player_id.as_deref(),
            device_id: &s.device_id,
            from_at: ends_at,
            to_at: new_end,
            minutes: e.minutes,
            rate_minor_per_hour: rate,
            amount_minor: amount,
            settled_at: now_ms,
            settled_by: &intent.actor.id,
            business_day: &business_day,
            fy: &fy,
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
        Written {
            key: RowKey::OrderLines { id: line_id },
            op: ChangeOp::Insert,
            before: None,
        },
    ];
    let stamp = Stamp {
        staff_id: &intent.actor.id,
        order_id: &order_id,
        business_day: &business_day,
        fy: &fy,
    };
    insert_payments(tx, &stamp, &t.payments, &mut written)?;
    record_payer_txns(
        tx,
        &stamp,
        &t.payments,
        s.player_id.as_deref(),
        t.payer.credit.as_ref(),
        &mut written,
    )?;
    if let Some(pack) = t.pack.as_ref().filter(|_| e.source == ExtendSource::Pack) {
        deduct_pack_minutes(tx, &pack.id, e.minutes)?;
        written.push(Written {
            key: RowKey::PlayerPacks {
                id: pack.id.clone(),
            },
            op: ChangeOp::Update,
            before: None,
        });
    }
    Ok(written)
}

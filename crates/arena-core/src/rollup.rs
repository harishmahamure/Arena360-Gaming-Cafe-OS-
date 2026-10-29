//! The daily rollup engine (Story 8.5, AD-35): builds `rollups/<yyyy-mm-dd>.json`, schema v1, for
//! one business day from about twenty queries over stamped columns and stored totals, on the
//! snapshot reader's own connection. It never runs inside `apply()`, never writes a row and makes
//! no network call: publishing is Story 8.6.
//!
//! Loading (`arena-store`) and folding (here) are separate, like the Z-report: the fold is pure,
//! so a fixed set of rows gives byte-identical JSON, and revenue, payments and cash come from the
//! same folds the Z-report uses, so they equal it.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, DEFAULT_STOCK_EXPIRY_WARN_DAYS, ErrorCode, KEY_STOCK_EXPIRY_WARN_DAYS,
    ROLLUP_MIN_SCHEMA, ROLLUP_PLAYERS_CAP, ROLLUP_SCHEMA, RollupAged, RollupAmount, RollupByMethod,
    RollupByType, RollupCredit, RollupCreditPlayer, RollupExpiring, RollupPlayHours, RollupRevenue,
    RollupSessions, RollupShift, RollupStock, RollupStockLevel, RollupV1, RollupWaste,
    SALES_DISCOUNTS, SALES_PLANS, SALES_PLAY_TIME, SALES_PRODUCTS, SALES_TAX_ADDED,
    STOCK_EXPIRY_WARN_RANGE,
};
use arena_store::{RollupRows, Store};
use chrono::{Days, NaiveDate};
use chrono_tz::Tz;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const MINUTE_MS: i64 = 60_000;

/// What the fold needs besides the rows.
pub struct RollupContext<'a> {
    pub branch_id: &'a str,
    pub currency: &'a str,
    pub business_day: &'a str,
    pub tz: Tz,
}

/// Parses a business day (`YYYY-MM-DD`).
pub fn parse_day(day: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| {
        AppError::with_field(
            ErrorCode::ValidationFailed,
            "business_day",
            "Pick a valid date.",
        )
    })
}

/// A UTC window that surely holds every instant of the business day whatever the branch's zone and
/// day start (a zone is at most 14 hours from UTC and a day starts within the day before).
pub fn day_window(day: NaiveDate) -> (i64, i64) {
    let midnight = day
        .and_hms_opt(0, 0, 0)
        .map_or(0, |t| t.and_utc().timestamp_millis());
    (midnight - 2 * DAY_MS, midnight + 3 * DAY_MS)
}

fn amount_of(list: &[arena_proto::ReportAmount], key: &str) -> (i64, i64) {
    list.iter()
        .find(|a| a.key == key)
        .map_or((0, 0), |a| (a.amount_minor, a.count))
}

/// Folds `rows` into the v1 document. `day_of` maps a UTC time to its business day; rows that
/// carry only a time are kept when it maps to `ctx.business_day`.
pub fn fold_rollup(
    rows: &RollupRows,
    ctx: &RollupContext<'_>,
    day_of: &dyn Fn(i64) -> String,
) -> RollupV1 {
    let day = ctx.business_day;
    let sales = crate::reports::daily_sales(&rows.sales, day, day, ctx.tz);
    let (play, _) = amount_of(&sales.by_type, SALES_PLAY_TIME);
    let (products, _) = amount_of(&sales.by_type, SALES_PRODUCTS);
    let (plans, _) = amount_of(&sales.by_type, SALES_PLANS);
    let (discounts, discounts_n) = amount_of(&sales.by_type, SALES_DISCOUNTS);
    let (tax_added, _) = amount_of(&sales.by_type, SALES_TAX_ADDED);

    let mut by_method = RollupByMethod::default();
    for p in &rows.payments {
        match p.method.as_str() {
            "cash" => by_method.cash += p.amount_minor,
            "upi" => by_method.upi += p.amount_minor,
            "card" => by_method.card += p.amount_minor,
            "wallet" => by_method.wallet += p.amount_minor,
            "credit" => by_method.credit += p.amount_minor,
            _ => {}
        }
    }

    let (mut void_n, mut void_amount) = (0, 0);
    for v in rows.voids.iter().filter(|v| day_of(v.voided_at) == day) {
        void_n += 1;
        void_amount += v.amount_minor;
    }

    let session_ms: i64 = rows
        .segments
        .iter()
        .map(|s| (s.to_at - s.from_at).max(0))
        .sum();

    let shifts = rows
        .shifts
        .iter()
        .filter_map(|s| {
            let closed_at = s.closed_at?;
            (day_of(closed_at) == day).then(|| RollupShift {
                shift_id: s.id.clone(),
                staff_name: s.opened_by.clone(),
                opened_at: s.opened_at,
                closed_at,
                float: s.float_minor,
                expected: s.expected_minor.unwrap_or(0),
                counted: s.counted_minor.unwrap_or(0),
                variance: s.variance_minor.unwrap_or(0),
                reason: s.variance_reason.clone(),
                approver_name: s.approver.clone(),
            })
        })
        .collect();

    let waste = crate::stock_report::waste_report(&rows.moves, day, day, day_of)
        .by_product
        .into_iter()
        .flat_map(|p| {
            let product_id = p.product_id;
            p.reasons.into_iter().map(move |r| RollupWaste {
                product_id: product_id.clone(),
                qty: r.qty,
                value: r.value_minor,
                reason: r.reason,
            })
        })
        .collect();

    let mut aged = RollupAged::default();
    let mut outstanding = 0;
    for d in &rows.debtors {
        outstanding += d.outstanding_minor;
        aged.d0_7 += d.aging.bucket_0_7_minor;
        aged.d8_14 += d.aging.bucket_8_14_minor;
        aged.d15p += d.aging.bucket_15_plus_minor;
    }
    let players = rows
        .debtors
        .iter()
        .take(ROLLUP_PLAYERS_CAP)
        .map(|d| RollupCreditPlayer {
            player_id: d.player_id.clone(),
            username: d.username.clone(),
            outstanding: d.outstanding_minor,
            oldest_unpaid_on: d.aging.oldest_unpaid_day.clone().unwrap_or_default(),
            consent: d.consent,
        })
        .collect();

    let mut games = BTreeMap::<&str, i64>::new();
    for p in rows.plays.iter().filter(|p| day_of(p.ended_at) == day) {
        *games.entry(p.game_id.as_str()).or_default() += p.minutes;
    }
    let mut play_hours: Vec<RollupPlayHours> = games
        .into_iter()
        .filter(|(_, minutes)| *minutes > 0)
        .map(|(game_id, minutes)| RollupPlayHours {
            game_id: game_id.to_owned(),
            minutes,
        })
        .collect();
    play_hours.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.game_id.cmp(&b.game_id))
    });

    RollupV1 {
        schema: ROLLUP_SCHEMA,
        branch_id: ctx.branch_id.to_owned(),
        business_day: day.to_owned(),
        currency: ctx.currency.to_owned(),
        revenue: RollupRevenue {
            total: sales.sales_minor,
            by_type: RollupByType {
                play,
                products,
                plans,
                other: discounts + tax_added,
            },
            by_method,
        },
        refunds: RollupAmount {
            count: rows.refund_count,
            amount: rows.sales.refunded_minor,
        },
        discounts: RollupAmount {
            count: discounts_n,
            amount: -discounts,
        },
        voids: RollupAmount {
            count: void_n,
            amount: void_amount,
        },
        sessions: RollupSessions {
            count: rows.session_count,
            minutes: session_ms / MINUTE_MS,
        },
        shifts,
        stock: RollupStock {
            levels: rows
                .levels
                .iter()
                .map(|l| RollupStockLevel {
                    product_id: l.product_id.clone(),
                    name: l.name.clone(),
                    qty: l.qty,
                    low_qty: l.low_stock_qty,
                })
                .collect(),
            waste,
            expiring: rows
                .lots
                .iter()
                .map(|l| RollupExpiring {
                    product_id: l.product_id.clone(),
                    lot: l.lot_id.clone(),
                    qty: l.qty,
                    expires_on: l.expires_on.clone(),
                })
                .collect(),
        },
        credit: RollupCredit {
            outstanding,
            aged,
            players,
        },
        play_hours,
    }
}

/// The branch's stock-expiry warning window in days.
fn warn_days(store: &Store) -> Result<u64, AppError> {
    let days = store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_STOCK_EXPIRY_WARN_DAYS)
        .and_then(|s| s.value.parse::<i64>().ok())
        .filter(|d| (STOCK_EXPIRY_WARN_RANGE.0..=STOCK_EXPIRY_WARN_RANGE.1).contains(d))
        .unwrap_or_else(|| DEFAULT_STOCK_EXPIRY_WARN_DAYS.parse().unwrap_or(3));
    Ok(u64::try_from(days).unwrap_or(3))
}

/// Builds the rollup of `business_day` as of `now_ms` (Hub clock). `None` before the branch has
/// been bootstrapped; `ValidationFailed` on `business_day` for a date that does not exist.
pub fn build_rollup(
    store: &Store,
    business_day: &str,
    now_ms: i64,
    schema: u32,
) -> Result<Option<RollupV1>, AppError> {
    // The writers this release keeps: v1 only so far (Story 8.7).
    if !(ROLLUP_MIN_SCHEMA..=ROLLUP_SCHEMA).contains(&schema) {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "schema",
            "No writer for that rollup schema.",
        ));
    }
    let date = parse_day(business_day)?;
    let (Some(branch), Some(locale)) = (store.branch_summary()?, store.branch_locale()?) else {
        return Ok(None);
    };
    let (from_ms, to_ms) = day_window(date);
    let expiry_until = date
        .checked_add_days(Days::new(warn_days(store)?))
        .unwrap_or(date)
        .to_string();
    let rows = store.rollup_rows(business_day, from_ms, to_ms, &expiry_until, now_ms)?;
    let day_of = crate::locale::business_day_fn(store)?;
    let tz = crate::locale::current_tz(store, now_ms)?;
    Ok(Some(fold_rollup(
        &rows,
        &RollupContext {
            branch_id: &branch.branch_id,
            currency: &locale.currency,
            business_day,
            tz,
        },
        &*day_of,
    )))
}

#[cfg(test)]
mod tests;

//! The Reports screen's figures (Story 5.27): Daily sales and Sessions, built from the rows
//! `arena-store` reads. Money comes from stored totals and the stamped `business_day` columns;
//! only the hour of day (and so utilisation) needs the branch time zone. Reading never goes
//! through `apply()`, so a 90-day report cannot block a write.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, CASH_DEPOSIT, CASH_DRAWER_OPEN, CASH_EXPENSE, CREDIT_BUCKET_0_7, CREDIT_BUCKET_8_14,
    CREDIT_BUCKET_15_PLUS, CreditBucketRow, CreditDebtor, DailySales, DeviceSessions, ErrorCode,
    GroupSessions, GuestTotals, HourSales, NamedSales, NewReturning, PLAYERS_DEBTOR_LIMIT,
    PLAYERS_TOP_LIMIT, PlayerSpend, PlayersReport, RECEIVED_CREDIT_COLLECT, RECEIVED_TOPUP,
    REPORT_MAX_DAYS, ReportAmount, SALES_DISCOUNTS, SALES_PLANS, SALES_PLAY_TIME, SALES_PRODUCTS,
    SALES_TAX_ADDED, SessionsReport, ShiftVariance, StaffReport, StaffRow, ZCashLine, ZReference,
    ZReferenceGroup, ZRefund, ZReport, ZVoid, ZWaste,
};
use arena_store::{
    PlayersReportRows, SalesReportRows, SessionsReportRows, StaffReportRows, ZCashEventRow,
    ZReportRows,
};
use chrono::{NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;

use crate::players::{KEK_LEN, open_field_of};

const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;

/// The range `from` to `to` (inclusive business days, `YYYY-MM-DD`) and how many days it covers.
pub fn parse_range(from: &str, to: &str) -> Result<i64, AppError> {
    let day = |field: &str, text: &str| {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| {
            AppError::with_field(ErrorCode::ValidationFailed, field, "Pick a valid date.")
        })
    };
    let (a, b) = (day("from", from)?, day("to", to)?);
    let days = (b - a).num_days() + 1;
    if days < 1 {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "to",
            "The end date is before the start date.",
        ));
    }
    if days > REPORT_MAX_DAYS {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "to",
            "Pick a range of at most 90 days.",
        ));
    }
    Ok(days)
}

/// The local hour (0 to 23) of `ts_ms` in `tz`, and how far into that hour it is (ms).
fn local_hour(ts_ms: i64, tz: Tz) -> (usize, i64) {
    let local = tz
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(|| tz.timestamp_millis_opt(0).unwrap());
    let into = i64::from(local.minute()) * MINUTE_MS
        + i64::from(local.second()) * 1000
        + i64::from(local.timestamp_subsec_millis());
    (local.hour() as usize, into)
}

/// Adds the time from `from_ms` to `to_ms` to the local-hour buckets, splitting at each local hour.
fn add_span(buckets: &mut [i64; 24], from_ms: i64, to_ms: i64, tz: Tz) {
    let mut at = from_ms;
    // A slice never spans more than a fortnight; the cap only guards against a corrupt row.
    for _ in 0..(24 * 14) {
        if at >= to_ms {
            break;
        }
        let (hour, into) = local_hour(at, tz);
        let end = (at + (HOUR_MS - into)).min(to_ms);
        buckets[hour] += end - at;
        at = end;
    }
}

/// Daily sales: the settled bills' stored totals split by type, hour, group and category.
pub fn daily_sales(rows: &SalesReportRows, from: &str, to: &str, tz: Tz) -> DailySales {
    #[derive(Default)]
    struct Parts {
        play: i64,
        play_n: i64,
        products: i64,
        products_n: i64,
        plans: i64,
        plans_n: i64,
    }
    let mut parts = BTreeMap::<&str, Parts>::new();
    let mut categories = BTreeMap::<&str, (i64, i64)>::new();
    for l in &rows.lines {
        let p = parts.entry(l.order_id.as_str()).or_default();
        if l.is_product {
            p.products += l.amount_minor;
            p.products_n += l.qty;
            let c = categories.entry(l.category.as_str()).or_default();
            c.0 += l.amount_minor;
            c.1 += l.qty;
        } else if l.is_plan {
            p.plans += l.amount_minor;
            p.plans_n += 1;
        } else {
            p.play += l.amount_minor;
            p.play_n += 1;
        }
    }

    let (mut sales, mut tax) = (0i64, 0i64);
    let (mut play, mut play_n, mut products, mut products_n, mut plans, mut plans_n) =
        (0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
    let (mut discounts, mut discounts_n, mut tax_added, mut tax_added_n) = (0i64, 0i64, 0i64, 0i64);
    let mut hours = vec![(0i64, 0i64); 24];
    let mut groups = BTreeMap::<&str, (i64, i64)>::new();
    for o in &rows.orders {
        sales += o.total_minor;
        tax += o.tax_minor;
        let p = parts.get(o.id.as_str());
        let lines = p.map_or(0, |p| p.play + p.products + p.plans);
        if let Some(p) = p {
            (play, play_n) = (play + p.play, play_n + p.play_n);
            (products, products_n) = (products + p.products, products_n + p.products_n);
            (plans, plans_n) = (plans + p.plans, plans_n + p.plans_n);
        }
        let off = o.coupon_minor + o.discount_minor;
        if off != 0 {
            discounts -= off;
            discounts_n += 1;
        }
        // What is left is the tax a tax-exclusive bill adds on top; it makes the rows add up to
        // the stored total whatever the bill's lines and adjustments were.
        let added = o.total_minor - (lines - off);
        if added != 0 {
            tax_added += added;
            tax_added_n += 1;
        }
        let (hour, _) = local_hour(o.settled_at, tz);
        hours[hour].0 += o.total_minor;
        hours[hour].1 += 1;
        let g = groups.entry(o.group_id.as_str()).or_default();
        g.0 += o.total_minor;
        g.1 += 1;
    }

    let amount = |key: &str, amount_minor: i64, count: i64| ReportAmount {
        key: key.to_owned(),
        amount_minor,
        count,
    };
    let by_type = vec![
        amount(SALES_PLAY_TIME, play, play_n),
        amount(SALES_PRODUCTS, products, products_n),
        amount(SALES_PLANS, plans, plans_n),
        amount(SALES_DISCOUNTS, discounts, discounts_n),
        amount(SALES_TAX_ADDED, tax_added, tax_added_n),
    ];

    let mut by_group = Vec::new();
    for (id, name) in &rows.groups {
        if let Some((amount_minor, count)) = groups.remove(id.as_str()) {
            by_group.push(NamedSales {
                id: id.clone(),
                name: name.clone(),
                amount_minor,
                count,
            });
        }
    }
    // Sales with no group (a counter sale) or a group that is gone.
    for (id, (amount_minor, count)) in groups {
        by_group.push(NamedSales {
            id: id.to_owned(),
            name: String::new(),
            amount_minor,
            count,
        });
    }

    let mut by_category: Vec<NamedSales> = categories
        .into_iter()
        .map(|(name, (amount_minor, count))| NamedSales {
            id: name.to_owned(),
            name: name.to_owned(),
            amount_minor,
            count,
        })
        .collect();
    by_category.sort_by(|a, b| {
        b.amount_minor
            .cmp(&a.amount_minor)
            .then_with(|| a.name.cmp(&b.name))
    });

    let received = [RECEIVED_TOPUP, RECEIVED_CREDIT_COLLECT]
        .into_iter()
        .map(|kind| {
            let (amount_minor, count) = rows
                .received
                .iter()
                .filter(|r| r.kind == kind)
                .fold((0, 0), |(a, c), r| (a + r.amount_minor, c + r.count));
            amount(kind, amount_minor, count)
        })
        .collect();

    DailySales {
        from: from.to_owned(),
        to: to.to_owned(),
        orders: i64::try_from(rows.orders.len()).unwrap_or(i64::MAX),
        sales_minor: sales,
        refunds_minor: rows.refunded_minor,
        net_minor: sales - rows.refunded_minor,
        tax_minor: tax,
        by_type,
        by_hour: hours
            .into_iter()
            .enumerate()
            .map(|(hour, (amount_minor, count))| HourSales {
                hour: hour as u8,
                amount_minor,
                count,
            })
            .collect(),
        by_group,
        by_category,
        received,
    }
}

/// Sessions: count, billed minutes and revenue by device and group, and utilisation per local
/// hour for each group (minutes in session divided by minutes available over `days`).
pub fn sessions_report(
    rows: &SessionsReportRows,
    from: &str,
    to: &str,
    days: i64,
    tz: Tz,
) -> SessionsReport {
    // Minutes the session was actually on a device, for sessions whose bill has no time lines.
    let mut slice_ms = BTreeMap::<&str, i64>::new();
    let mut hours = BTreeMap::<&str, [i64; 24]>::new();
    for s in &rows.spans {
        *slice_ms.entry(s.session_id.as_str()).or_default() += (s.to_at - s.from_at).max(0);
        add_span(
            hours.entry(s.group_id.as_str()).or_insert([0; 24]),
            s.from_at,
            s.to_at,
            tz,
        );
    }

    let mut devices = BTreeMap::<&str, DeviceSessions>::new();
    for d in &rows.devices {
        devices.insert(
            d.device_id.as_str(),
            DeviceSessions {
                device_id: d.device_id.clone(),
                label: d.label.clone(),
                group_id: d.group_id.clone(),
                sessions: 0,
                minutes: 0,
                revenue_minor: 0,
            },
        );
    }
    let mut group_totals = BTreeMap::<&str, (i64, i64, i64)>::new();
    for s in &rows.sessions {
        let minutes = if s.billed_min > 0 {
            s.billed_min
        } else {
            slice_ms.get(s.id.as_str()).copied().unwrap_or(0) / MINUTE_MS
        };
        let d = devices
            .entry(s.device_id.as_str())
            .or_insert_with(|| DeviceSessions {
                device_id: s.device_id.clone(),
                label: s.device_id.clone(),
                group_id: s.group_id.clone(),
                sessions: 0,
                minutes: 0,
                revenue_minor: 0,
            });
        d.sessions += 1;
        d.minutes += minutes;
        d.revenue_minor += s.revenue_minor;
        let g = group_totals.entry(s.group_id.as_str()).or_default();
        g.0 += 1;
        g.1 += minutes;
        g.2 += s.revenue_minor;
    }

    let mut by_group = Vec::new();
    for (id, name) in &rows.groups {
        let device_count =
            i64::try_from(rows.devices.iter().filter(|d| &d.group_id == id).count()).unwrap_or(0);
        let totals = group_totals.remove(id.as_str());
        if device_count == 0 && totals.is_none() {
            continue;
        }
        let (sessions, minutes, revenue_minor) = totals.unwrap_or_default();
        let available_ms = device_count * days * HOUR_MS;
        let buckets = hours.get(id.as_str()).copied().unwrap_or([0; 24]);
        by_group.push(GroupSessions {
            group_id: id.clone(),
            name: name.clone(),
            devices: device_count,
            sessions,
            minutes,
            revenue_minor,
            utilisation_bp: buckets
                .iter()
                .map(|used| {
                    if available_ms == 0 {
                        0
                    } else {
                        (used.saturating_mul(10_000) / available_ms).min(10_000) as u16
                    }
                })
                .collect(),
        });
    }

    let mut by_device: Vec<DeviceSessions> = devices.into_values().collect();
    by_device.sort_by(|a, b| {
        b.revenue_minor
            .cmp(&a.revenue_minor)
            .then_with(|| b.minutes.cmp(&a.minutes))
            .then_with(|| a.label.cmp(&b.label))
    });

    SessionsReport {
        from: from.to_owned(),
        to: to.to_owned(),
        days,
        sessions: by_device.iter().map(|d| d.sessions).sum(),
        minutes: by_device.iter().map(|d| d.minutes).sum(),
        revenue_minor: by_device.iter().map(|d| d.revenue_minor).sum(),
        by_group,
        by_device,
    }
}

/// The payment methods in the order a Z-report lists them; any other method follows, by name.
const Z_METHODS: [&str; 5] = ["cash", "upi", "card", "wallet", "credit"];

/// The Z-report of a shift (Story 5.28), folded from the rows of its window. Sales by type reuse
/// the Story 5.27 fold, so they add up to the stored totals; payments are the positive payment
/// rows (top-ups and credit collections are payments, not sales); refunds are negative lines. The
/// close facts (counted, expected, variance, reason, approver) come from `rows.shift`.
pub fn z_report(rows: &ZReportRows, tz: Tz) -> ZReport {
    let s = &rows.shift;
    let sales = daily_sales(&rows.sales, "", "", tz);

    let mut methods = BTreeMap::<&str, (i64, i64)>::new();
    for p in &rows.payments {
        let m = methods.entry(p.method.as_str()).or_default();
        m.0 += p.amount_minor;
        m.1 += 1;
    }
    let mut ordered: Vec<&str> = Z_METHODS
        .into_iter()
        .filter(|m| methods.contains_key(m))
        .collect();
    ordered.extend(methods.keys().filter(|m| !Z_METHODS.contains(m)));
    let payments = ordered
        .into_iter()
        .map(|m| ReportAmount {
            key: m.to_owned(),
            amount_minor: methods[m].0,
            count: methods[m].1,
        })
        .collect();

    let references = ["upi", "card"]
        .into_iter()
        .filter_map(|method| {
            let list: Vec<ZReference> = rows
                .payments
                .iter()
                .filter(|p| p.method == method)
                .map(|p| ZReference {
                    at: p.at,
                    invoice_no: p.invoice_no.clone().unwrap_or_default(),
                    amount_minor: p.amount_minor,
                    reference: p.reference.clone().unwrap_or_default(),
                })
                .collect();
            (!list.is_empty()).then(|| ZReferenceGroup {
                method: method.to_owned(),
                rows: list,
            })
        })
        .collect();

    let received = [RECEIVED_TOPUP, RECEIVED_CREDIT_COLLECT]
        .into_iter()
        .map(|kind| {
            let (amount_minor, count) = rows
                .received
                .iter()
                .filter(|r| r.kind == kind)
                .fold((0, 0), |(a, c), r| (a + r.amount_minor, c + r.count));
            ReportAmount {
                key: kind.to_owned(),
                amount_minor,
                count,
            }
        })
        .collect();

    let refunds: Vec<ZRefund> = rows
        .refunds
        .iter()
        .map(|r| ZRefund {
            credit_note_no: r.credit_note_no.clone(),
            method: r.method.clone().unwrap_or_default(),
            amount_minor: -r.amount_minor,
        })
        .collect();

    let mut waste = BTreeMap::<&str, (i64, i64)>::new();
    for w in &rows.waste {
        let e = waste.entry(w.reason.as_str()).or_default();
        e.0 += w.qty;
        e.1 += w.value_minor;
    }
    let mut voids = BTreeMap::<&str, (i64, i64)>::new();
    for v in &rows.voids {
        let e = voids.entry(v.reason.as_str()).or_default();
        e.0 += 1;
        e.1 += v.amount_minor;
    }

    let line = |e: &ZCashEventRow, expense: bool| ZCashLine {
        category: e.category.clone().filter(|_| expense).unwrap_or_default(),
        reference: e.reference.clone().filter(|_| !expense).unwrap_or_default(),
        note: e.note.clone().unwrap_or_default(),
        paid_from: e.paid_from.clone().filter(|_| expense).unwrap_or_default(),
        amount_minor: e
            .spent_minor
            .filter(|_| expense)
            .unwrap_or_else(|| e.amount_minor.abs()),
    };
    let of_kind = |kind: &str, expense: bool| -> Vec<ZCashLine> {
        rows.cash_events
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| line(e, expense))
            .collect()
    };
    let drawer_opens = rows
        .cash_events
        .iter()
        .filter(|e| e.kind == CASH_DRAWER_OPEN)
        .count();

    ZReport {
        shift_id: s.id.clone(),
        opened_at: s.opened_at,
        closed_at: s.closed_at.unwrap_or(s.opened_at),
        opened_by: s.opened_by.clone(),
        closed_by: s.closed_by.clone(),
        rebuilt: false,
        orders: sales.orders,
        sales_minor: sales.sales_minor,
        tax_minor: sales.tax_minor,
        by_type: sales.by_type,
        payments,
        references,
        received,
        refunds_minor: refunds.iter().map(|r| r.amount_minor).sum(),
        refunds,
        waste: waste
            .into_iter()
            .filter(|(_, (qty, value_minor))| *qty != 0 || *value_minor != 0)
            .map(|(reason, (qty, value_minor))| ZWaste {
                reason: reason.to_owned(),
                qty,
                value_minor,
            })
            .collect(),
        expenses: of_kind(CASH_EXPENSE, true),
        deposits: of_kind(CASH_DEPOSIT, false),
        voids: voids
            .into_iter()
            .map(|(reason, (count, amount_minor))| ZVoid {
                reason: reason.to_owned(),
                count,
                amount_minor,
            })
            .collect(),
        drawer_opens: i64::try_from(drawer_opens).unwrap_or(i64::MAX),
        float_minor: s.float_minor,
        expected_minor: s.expected_minor.unwrap_or(0),
        counted_minor: s.counted_minor.unwrap_or(0),
        variance_minor: s.variance_minor.unwrap_or(0),
        variance_reason: s.variance_reason.clone(),
        variance_note: s.variance_note.clone(),
        approver: s.approver.clone(),
    }
}

/// A player's name opened with the branch key, or `None` when the data key is absent or will not
/// open (an erased player): no other personal data is read.
fn open_name(
    kek: &[u8; KEK_LEN],
    player_id: &str,
    name_enc: Option<&str>,
    dek_wrapped: Option<&str>,
) -> Option<String> {
    open_field_of(kek, player_id, dek_wrapped, "name", name_enc?).ok()
}

/// The Players report (Story 5.30): top spenders, the guest total, new against returning players
/// and credit outstanding by age. `today` is the business day credit is aged as of; names are
/// opened with `kek` and a player whose key is absent is `erased` with an empty name.
pub fn players_report(
    rows: &PlayersReportRows,
    from: &str,
    to: &str,
    today: &str,
    kek: &[u8; KEK_LEN],
) -> PlayersReport {
    let mut top = Vec::with_capacity(rows.players.len());
    let mut new_players = NewReturning {
        players: 0,
        spend_minor: 0,
    };
    let mut returning_players = new_players.clone();
    for p in &rows.players {
        let name = open_name(
            kek,
            &p.player_id,
            p.name_enc.as_deref(),
            p.dek_wrapped.as_deref(),
        );
        top.push(PlayerSpend {
            player_id: p.player_id.clone(),
            erased: name.is_none(),
            name: name.unwrap_or_default(),
            visits: p.visits,
            spend_minor: p.spend_minor,
        });
        let group = if p.first_day.as_str() >= from {
            &mut new_players
        } else {
            &mut returning_players
        };
        group.players += 1;
        group.spend_minor += p.spend_minor;
    }
    top.sort_by(|a, b| {
        b.spend_minor
            .cmp(&a.spend_minor)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    top.truncate(PLAYERS_TOP_LIMIT);

    let (mut b07, mut b814, mut b15, mut outstanding) = (0i64, 0i64, 0i64, 0i64);
    let mut debtors = Vec::with_capacity(rows.debtors.len());
    for d in &rows.debtors {
        b07 += d.aging.bucket_0_7_minor;
        b814 += d.aging.bucket_8_14_minor;
        b15 += d.aging.bucket_15_plus_minor;
        outstanding += d.outstanding_minor;
        let name = open_name(
            kek,
            &d.player_id,
            d.name_enc.as_deref(),
            d.dek_wrapped.as_deref(),
        );
        debtors.push(CreditDebtor {
            player_id: d.player_id.clone(),
            erased: name.is_none(),
            name: name.unwrap_or_default(),
            outstanding_minor: d.outstanding_minor,
            oldest_age_days: d.aging.oldest_age_days,
        });
    }
    debtors.sort_by(|a, b| {
        b.outstanding_minor
            .cmp(&a.outstanding_minor)
            .then_with(|| b.oldest_age_days.cmp(&a.oldest_age_days))
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    debtors.truncate(PLAYERS_DEBTOR_LIMIT);

    let bucket = |key: &str, amount_minor: i64| CreditBucketRow {
        key: key.to_owned(),
        amount_minor,
    };
    PlayersReport {
        from: from.to_owned(),
        to: to.to_owned(),
        top,
        guest: GuestTotals {
            bills: rows.guest.bills,
            spend_minor: rows.guest.spend_minor,
        },
        new_players,
        returning_players,
        credit_as_of: today.to_owned(),
        credit_buckets: vec![
            bucket(CREDIT_BUCKET_0_7, b07),
            bucket(CREDIT_BUCKET_8_14, b814),
            bucket(CREDIT_BUCKET_15_PLUS, b15),
        ],
        credit_outstanding_minor: outstanding,
        debtors,
    }
}

/// The UTC milliseconds range to read closed shifts from for the business days `from` to `to`: the
/// range with a day and more either side, because a business day is local time shifted by the
/// day start and can reach into the neighbouring UTC days.
pub fn shift_read_window(from: &str, to: &str) -> Result<(i64, i64), AppError> {
    const DAY_MS: i64 = 86_400_000;
    let day = |text: &str| {
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|t| t.and_utc().timestamp_millis())
            .ok_or_else(|| AppError::new(ErrorCode::ValidationFailed, "Pick a valid date."))
    };
    Ok((day(from)? - 2 * DAY_MS, day(to)? + 4 * DAY_MS))
}

/// The Staff report (Story 5.30): per staff member with any activity in the range, what they sold,
/// started, discounted, voided and refunded, and the variance of the shifts they closed in it.
/// `day_of` gives the local business day of a UTC time (a shift's `closed_at`). A staff member no
/// longer in the staff table has an empty name.
pub fn staff_report(
    rows: &StaffReportRows,
    from: &str,
    to: &str,
    day_of: &dyn Fn(i64) -> String,
) -> StaffReport {
    let mut by_id = BTreeMap::<String, StaffRow>::new();
    fn entry<'a>(
        by_id: &'a mut BTreeMap<String, StaffRow>,
        names: &[(String, String)],
        id: &str,
    ) -> &'a mut StaffRow {
        by_id.entry(id.to_owned()).or_insert_with(|| StaffRow {
            staff_id: id.to_owned(),
            name: names
                .iter()
                .find(|(staff_id, _)| staff_id == id)
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
            sales_minor: 0,
            sales_count: 0,
            sessions_started: 0,
            discounts_count: 0,
            discounts_minor: 0,
            voids_count: 0,
            voids_minor: 0,
            refunds_count: 0,
            refunds_minor: 0,
            shifts: Vec::new(),
            variance_minor: 0,
        })
    }
    for r in &rows.sales {
        let row = entry(&mut by_id, &rows.names, &r.staff_id);
        (row.sales_count, row.sales_minor) = (r.count, r.value_minor);
    }
    for r in &rows.sessions {
        entry(&mut by_id, &rows.names, &r.staff_id).sessions_started = r.count;
    }
    for r in &rows.discounts {
        let row = entry(&mut by_id, &rows.names, &r.staff_id);
        (row.discounts_count, row.discounts_minor) = (r.count, r.value_minor);
    }
    for r in &rows.voids {
        let row = entry(&mut by_id, &rows.names, &r.staff_id);
        (row.voids_count, row.voids_minor) = (r.count, r.value_minor);
    }
    for r in &rows.refunds {
        let row = entry(&mut by_id, &rows.names, &r.staff_id);
        (row.refunds_count, row.refunds_minor) = (r.count, r.value_minor);
    }
    for s in &rows.shifts {
        let day = day_of(s.closed_at);
        if day.as_str() < from || day.as_str() > to {
            continue;
        }
        let row = entry(&mut by_id, &rows.names, &s.staff_id);
        row.variance_minor += s.variance_minor;
        row.shifts.push(ShiftVariance {
            shift_id: s.shift_id.clone(),
            closed_at: s.closed_at,
            variance_minor: s.variance_minor,
        });
    }
    let mut staff: Vec<StaffRow> = by_id.into_values().collect();
    staff.sort_by(|a, b| {
        b.sales_minor
            .cmp(&a.sales_minor)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.staff_id.cmp(&b.staff_id))
    });
    StaffReport {
        from: from.to_owned(),
        to: to.to_owned(),
        staff,
    }
}

#[cfg(test)]
mod tests {
    use arena_store::SessionReportRow;
    use arena_store::{ReceivedRow, ReportDevice, SalesLineRow, SalesOrderRow, SegmentSpan};

    use super::*;

    fn order(id: &str, kind: &str, total: i64, group: &str, settled_at: i64) -> SalesOrderRow {
        SalesOrderRow {
            id: id.into(),
            kind: kind.into(),
            total_minor: total,
            tax_minor: 0,
            coupon_minor: 0,
            discount_minor: 0,
            settled_at,
            group_id: group.into(),
        }
    }

    fn line(order: &str, amount: i64, product: bool, plan: bool, cat: &str) -> SalesLineRow {
        SalesLineRow {
            order_id: order.into(),
            amount_minor: amount,
            qty: 1,
            is_product: product,
            is_plan: plan,
            category: cat.into(),
        }
    }

    fn shift_detail() -> arena_store::ShiftDetail {
        arena_store::ShiftDetail {
            id: "SH".into(),
            open: false,
            opened_at: 1_000,
            closed_at: Some(9_000),
            opened_by: "Meera".into(),
            closed_by: "Asha".into(),
            float_minor: 500,
            counted_minor: Some(1_400),
            expected_minor: Some(1_500),
            variance_minor: Some(-100),
            variance_reason: None,
            variance_note: None,
            approver: None,
            z_report: None,
        }
    }

    fn payment(method: &str, amount: i64, reference: Option<&str>) -> arena_store::ZPaymentRow {
        arena_store::ZPaymentRow {
            method: method.into(),
            amount_minor: amount,
            reference: reference.map(Into::into),
            at: 2_000,
            invoice_no: None,
        }
    }

    #[test]
    fn a_z_report_folds_the_shift_rows_and_adds_up_to_the_stored_totals() {
        let rows = ZReportRows {
            shift: shift_detail(),
            sales: SalesReportRows {
                orders: vec![order("a", "pos_sale", 700, "", 2_000)],
                lines: vec![line("a", 700, true, false, "Drinks")],
                ..SalesReportRows::default()
            },
            received: vec![ReceivedRow {
                kind: "credit_collect".into(),
                amount_minor: 300,
                count: 1,
            }],
            payments: vec![
                payment("upi", 400, Some("R1")),
                payment("cash", 300, None),
                payment("gift", 5, None),
                payment("card", 300, Some("T-9")),
                payment("cash", 300, None),
            ],
            refunds: vec![arena_store::ZRefundRow {
                credit_note_no: "CN/1".into(),
                method: None,
                amount_minor: 50,
            }],
            waste: vec![
                arena_store::ZWasteRow {
                    reason: "spoiled".into(),
                    qty: 1,
                    value_minor: 20,
                },
                arena_store::ZWasteRow {
                    reason: "spoiled".into(),
                    qty: 2,
                    value_minor: 40,
                },
            ],
            voids: vec![
                arena_store::ZVoidRow {
                    reason: "mistake".into(),
                    amount_minor: 90,
                },
                arena_store::ZVoidRow {
                    reason: "mistake".into(),
                    amount_minor: 10,
                },
            ],
            cash_events: vec![
                ZCashEventRow {
                    kind: "expense".into(),
                    amount_minor: 0,
                    reference: None,
                    category: Some("repairs".into()),
                    paid_from: Some("owner".into()),
                    spent_minor: Some(250),
                    note: None,
                },
                ZCashEventRow {
                    kind: "deposit".into(),
                    amount_minor: -800,
                    reference: Some("S-1".into()),
                    category: None,
                    paid_from: None,
                    spent_minor: None,
                    note: None,
                },
                ZCashEventRow {
                    kind: "drawer_open".into(),
                    amount_minor: 0,
                    reference: None,
                    category: None,
                    paid_from: None,
                    spent_minor: None,
                    note: None,
                },
            ],
        };
        let z = z_report(&rows, Tz::UTC);
        assert_eq!((z.orders, z.sales_minor), (1, 700));
        assert_eq!(z.by_type.iter().map(|t| t.amount_minor).sum::<i64>(), 700);
        // Cash, UPI and card in that order, other methods after; counts per row.
        assert_eq!(
            z.payments
                .iter()
                .map(|p| (p.key.as_str(), p.amount_minor, p.count))
                .collect::<Vec<_>>(),
            [
                ("cash", 600, 2),
                ("upi", 400, 1),
                ("card", 300, 1),
                ("gift", 5, 1)
            ]
        );
        assert_eq!(
            z.references
                .iter()
                .map(|g| g.method.as_str())
                .collect::<Vec<_>>(),
            ["upi", "card"]
        );
        assert_eq!(z.references[0].rows[0].reference, "R1");
        assert_eq!(
            z.received
                .iter()
                .map(|r| (r.key.as_str(), r.amount_minor))
                .collect::<Vec<_>>(),
            [("topup", 0), ("credit_collect", 300)]
        );
        assert_eq!(
            (
                z.refunds[0].amount_minor,
                z.refunds[0].method.as_str(),
                z.refunds_minor
            ),
            (-50, "", -50)
        );
        assert_eq!(
            (z.waste.len(), z.waste[0].qty, z.waste[0].value_minor),
            (1, 3, 60)
        );
        assert_eq!((z.voids[0].count, z.voids[0].amount_minor), (2, 100));
        assert_eq!(
            (z.expenses[0].amount_minor, z.expenses[0].paid_from.as_str()),
            (250, "owner")
        );
        assert_eq!(
            (z.deposits[0].amount_minor, z.deposits[0].reference.as_str()),
            (800, "S-1")
        );
        assert_eq!(z.drawer_opens, 1);
        assert_eq!(
            (z.expected_minor, z.counted_minor, z.variance_minor),
            (1_500, 1_400, -100)
        );
        assert_eq!((z.opened_at, z.closed_at), (1_000, 9_000));
    }

    #[test]
    fn an_empty_shift_has_empty_sections_and_zero_totals() {
        let rows = ZReportRows {
            shift: shift_detail(),
            sales: SalesReportRows::default(),
            received: vec![],
            payments: vec![],
            refunds: vec![],
            waste: vec![],
            voids: vec![],
            cash_events: vec![],
        };
        let z = z_report(&rows, Tz::UTC);
        assert_eq!(
            (z.orders, z.sales_minor, z.refunds_minor, z.drawer_opens),
            (0, 0, 0, 0)
        );
        assert!(z.payments.is_empty() && z.references.is_empty() && z.waste.is_empty());
        assert_eq!(z.by_type.len(), 5);
        assert_eq!(z.received.len(), 2);
    }

    #[test]
    fn range_is_validated() {
        assert_eq!(parse_range("2026-10-01", "2026-10-01").unwrap(), 1);
        assert_eq!(parse_range("2026-07-01", "2026-09-28").unwrap(), 90);
        assert!(parse_range("2026-10-02", "2026-10-01").is_err());
        assert!(parse_range("2026-01-01", "2026-12-31").is_err());
        assert!(parse_range("nope", "2026-10-01").is_err());
    }

    #[test]
    fn daily_sales_rows_add_up_to_the_stored_totals() {
        // 12:30 and 12:45 UTC are 18:00 and 18:15 in Kolkata (UTC+5:30).
        let at = |h: i64, m: i64| (h * 60 + m) * MINUTE_MS;
        let mut with_coupon = order("b", "pos_sale", 380, "", at(12, 45));
        with_coupon.coupon_minor = 20;
        let mut gst = order("c", "plan_sale", 1180, "g1", at(3, 0));
        gst.tax_minor = 180;
        let rows = SalesReportRows {
            orders: vec![
                order("a", "session", 500, "g1", at(12, 30)),
                with_coupon,
                gst,
            ],
            lines: vec![
                line("a", 300, false, false, ""),
                line("a", 200, true, false, "Drinks"),
                line("b", 400, true, false, "Snacks"),
                // A tax-exclusive plan: 1,000 plus 180 tax.
                line("c", 1000, false, true, ""),
            ],
            received: vec![ReceivedRow {
                kind: "topup".into(),
                amount_minor: 700,
                count: 2,
            }],
            refunded_minor: 100,
            groups: vec![("g1".into(), "PC".into())],
        };
        let r = daily_sales(&rows, "2026-10-01", "2026-10-01", chrono_tz::Asia::Kolkata);
        assert_eq!((r.orders, r.sales_minor, r.net_minor), (3, 2060, 1960));
        assert_eq!(r.by_type.iter().map(|t| t.amount_minor).sum::<i64>(), 2060);
        let of = |k: &str| r.by_type.iter().find(|t| t.key == k).unwrap().amount_minor;
        assert_eq!(
            [
                of("play_time"),
                of("products"),
                of("plans"),
                of("discounts"),
                of("tax_added")
            ],
            [300, 600, 1000, -20, 180]
        );
        assert_eq!(r.by_hour.len(), 24);
        assert_eq!((r.by_hour[18].amount_minor, r.by_hour[18].count), (880, 2));
        assert_eq!(r.by_hour[8].amount_minor, 1180);
        assert_eq!(r.by_group.len(), 2);
        assert_eq!(
            (r.by_group[0].name.as_str(), r.by_group[0].amount_minor),
            ("PC", 1680)
        );
        assert_eq!(
            (r.by_group[1].id.as_str(), r.by_group[1].amount_minor),
            ("", 380)
        );
        assert_eq!(r.by_category[0].name, "Snacks");
        assert_eq!(r.received[0].amount_minor, 700);
        assert_eq!(r.received[1].amount_minor, 0);
    }

    #[test]
    fn sessions_report_counts_minutes_and_utilisation() {
        // Two PCs, a one-day range; s1 ran 12:45-13:45 UTC = 18:15-19:15 in Kolkata.
        let at = |h: i64, m: i64| (h * 60 + m) * MINUTE_MS;
        let rows = SessionsReportRows {
            sessions: vec![
                SessionReportRow {
                    id: "s1".into(),
                    device_id: "d1".into(),
                    group_id: "g1".into(),
                    billed_min: 60,
                    revenue_minor: 600,
                },
                SessionReportRow {
                    id: "s2".into(),
                    device_id: "d2".into(),
                    group_id: "g1".into(),
                    billed_min: 0,
                    revenue_minor: 0,
                },
            ],
            spans: vec![
                SegmentSpan {
                    session_id: "s1".into(),
                    group_id: "g1".into(),
                    from_at: at(12, 45),
                    to_at: at(13, 45),
                },
                SegmentSpan {
                    session_id: "s2".into(),
                    group_id: "g1".into(),
                    from_at: at(12, 30),
                    to_at: at(12, 45),
                },
            ],
            devices: vec![
                ReportDevice {
                    device_id: "d1".into(),
                    label: "PC 1".into(),
                    group_id: "g1".into(),
                },
                ReportDevice {
                    device_id: "d2".into(),
                    label: "PC 2".into(),
                    group_id: "g1".into(),
                },
            ],
            groups: vec![("g1".into(), "PC".into())],
        };
        let r = sessions_report(
            &rows,
            "2026-10-01",
            "2026-10-01",
            1,
            chrono_tz::Asia::Kolkata,
        );
        assert_eq!((r.sessions, r.minutes, r.revenue_minor), (2, 75, 600));
        let g = &r.by_group[0];
        assert_eq!((g.devices, g.sessions, g.minutes), (2, 2, 75));
        // 18:00 hour: 45 + 15 minutes of 120 available; 19:00 hour: 15 of 120.
        assert_eq!(g.utilisation_bp[18], 5000);
        assert_eq!(g.utilisation_bp[19], 1250);
        assert_eq!(g.utilisation_bp[3], 0);
        assert_eq!(r.by_device[0].label, "PC 1");
    }

    const KEK: [u8; KEK_LEN] = [4; KEK_LEN];

    /// A player's sealed name and wrapped key, as the store hands them to the fold.
    fn sealed(player_id: &str, name: &str) -> (String, String) {
        let form = arena_proto::PlayerForm {
            username: "someone".into(),
            password: "secret1".into(),
            phone: "+91 98765 43210".into(),
            name: name.into(),
            email: String::new(),
            dob: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            consent: true,
            policy_version: "1".into(),
        };
        let s = crate::players::seal(&KEK, &form, player_id, "hash".into()).unwrap();
        (s.name_enc, s.wrapped_key)
    }

    fn spend_row(
        id: &str,
        name: Option<&str>,
        visits: i64,
        spend: i64,
        first: &str,
    ) -> arena_store::PlayerSalesRow {
        let (name_enc, dek_wrapped) = match name {
            Some(n) => {
                let (e, k) = sealed(id, n);
                (Some(e), Some(k))
            }
            // A player row whose data key is gone.
            None => (Some("ciphertext".to_owned()), None),
        };
        arena_store::PlayerSalesRow {
            player_id: id.into(),
            visits,
            spend_minor: spend,
            first_day: first.into(),
            name_enc,
            dek_wrapped,
        }
    }

    fn debtor(
        id: &str,
        name: Option<&str>,
        outstanding: i64,
        buckets: (i64, i64, i64),
        age: i64,
    ) -> arena_store::DebtorRow {
        let (name_enc, dek_wrapped) = match name {
            Some(n) => {
                let (e, k) = sealed(id, n);
                (Some(e), Some(k))
            }
            None => (None, None),
        };
        arena_store::DebtorRow {
            player_id: id.into(),
            outstanding_minor: outstanding,
            aging: arena_proto::CreditAging {
                oldest_unpaid_day: None,
                oldest_age_days: age,
                bucket_0_7_minor: buckets.0,
                bucket_8_14_minor: buckets.1,
                bucket_15_plus_minor: buckets.2,
            },
            name_enc,
            dek_wrapped,
        }
    }

    #[test]
    fn players_rank_by_spend_then_name_and_split_new_from_returning() {
        let rows = PlayersReportRows {
            players: vec![
                spend_row("P1", Some("Zed"), 3, 900, "2026-10-10"),
                spend_row("P2", Some("Asha"), 2, 900, "2026-10-12"),
                spend_row("P3", Some("Bala"), 1, 400, "2026-09-01"),
                spend_row("P4", None, 4, 1200, "2026-08-01"),
            ],
            guest: arena_store::GuestSalesRow {
                bills: 5,
                spend_minor: 350,
            },
            debtors: Vec::new(),
        };
        let r = players_report(&rows, "2026-10-10", "2026-10-20", "2026-10-20", &KEK);
        let ranked: Vec<_> = r
            .top
            .iter()
            .map(|p| (p.name.as_str(), p.erased, p.visits, p.spend_minor))
            .collect();
        assert_eq!(
            ranked,
            [
                ("", true, 4, 1200),
                ("Asha", false, 2, 900),
                ("Zed", false, 3, 900),
                ("Bala", false, 1, 400),
            ],
            "spend descending, then name; an erased player has no name"
        );
        assert_eq!((r.guest.bills, r.guest.spend_minor), (5, 350));
        // P1 and P2 made their first sale in the range, P3 and P4 came back.
        assert_eq!(
            (r.new_players.players, r.new_players.spend_minor),
            (2, 1800)
        );
        assert_eq!(
            (r.returning_players.players, r.returning_players.spend_minor),
            (2, 1600)
        );
        assert!(r.top.iter().all(|p| p.player_id != "guest"));
    }

    #[test]
    fn the_top_list_stops_at_twenty() {
        let rows = PlayersReportRows {
            players: (0..25)
                .map(|i| {
                    spend_row(
                        &format!("P{i:02}"),
                        Some(&format!("N{i:02}")),
                        1,
                        100 + i,
                        "2026-10-10",
                    )
                })
                .collect(),
            ..PlayersReportRows::default()
        };
        let r = players_report(&rows, "2026-10-10", "2026-10-10", "2026-10-10", &KEK);
        assert_eq!(r.top.len(), PLAYERS_TOP_LIMIT);
        assert_eq!(r.top[0].spend_minor, 124);
        assert_eq!(
            r.new_players.players, 25,
            "the cut applies to the list only"
        );
    }

    #[test]
    fn credit_is_summed_over_every_debtor_into_three_buckets() {
        let rows = PlayersReportRows {
            debtors: vec![
                debtor("P1", Some("Asha"), 80, (80, 0, 0), 3),
                debtor("P2", Some("Bala"), 300, (0, 300, 0), 10),
                debtor("P3", None, 200, (0, 0, 200), 20),
                debtor("P4", Some("Chitra"), 300, (100, 0, 200), 25),
            ],
            ..PlayersReportRows::default()
        };
        let r = players_report(&rows, "2026-10-10", "2026-10-20", "2026-10-20", &KEK);
        let buckets: Vec<_> = r
            .credit_buckets
            .iter()
            .map(|b| (b.key.as_str(), b.amount_minor))
            .collect();
        assert_eq!(buckets, [("0_7", 180), ("8_14", 300), ("15_plus", 400)]);
        assert_eq!(r.credit_outstanding_minor, 880);
        assert_eq!(r.credit_as_of, "2026-10-20");
        let order: Vec<_> = r
            .debtors
            .iter()
            .map(|d| (d.player_id.as_str(), d.name.as_str(), d.erased))
            .collect();
        assert_eq!(
            order,
            [
                ("P4", "Chitra", false),
                ("P2", "Bala", false),
                ("P3", "", true),
                ("P1", "Asha", false),
            ],
            "largest outstanding first, the older tab first on a tie"
        );
    }

    #[test]
    fn a_wrong_branch_key_reads_every_player_as_erased() {
        let rows = PlayersReportRows {
            players: vec![spend_row("P1", Some("Asha"), 1, 100, "2026-10-10")],
            debtors: vec![debtor("P1", Some("Asha"), 50, (50, 0, 0), 1)],
            ..PlayersReportRows::default()
        };
        let r = players_report(
            &rows,
            "2026-10-10",
            "2026-10-10",
            "2026-10-10",
            &[9; KEK_LEN],
        );
        assert!(r.top[0].erased && r.top[0].name.is_empty());
        assert!(r.debtors[0].erased && r.debtors[0].name.is_empty());
    }

    #[test]
    fn an_empty_players_range_is_zeroes_and_empty_lists() {
        let r = players_report(
            &PlayersReportRows::default(),
            "2026-10-10",
            "2026-10-10",
            "2026-10-10",
            &KEK,
        );
        assert!(r.top.is_empty() && r.debtors.is_empty());
        assert_eq!((r.guest.bills, r.guest.spend_minor), (0, 0));
        assert_eq!(r.credit_buckets.len(), 3);
        assert!(r.credit_buckets.iter().all(|b| b.amount_minor == 0));
        assert_eq!((r.new_players.players, r.returning_players.players), (0, 0));
    }

    fn count(id: &str, count: i64, value: i64) -> arena_store::StaffCountRow {
        arena_store::StaffCountRow {
            staff_id: id.into(),
            count,
            value_minor: value,
        }
    }

    fn shift(staff: &str, id: &str, closed_at: i64, variance: i64) -> arena_store::StaffShiftRow {
        arena_store::StaffShiftRow {
            staff_id: staff.into(),
            shift_id: id.into(),
            closed_at,
            variance_minor: variance,
        }
    }

    #[test]
    fn staff_figures_land_on_the_right_member_and_voids_stay_out_of_sales() {
        let rows = StaffReportRows {
            sales: vec![count("S1", 3, 1200), count("S2", 1, 300)],
            sessions: vec![count("S1", 4, 0)],
            discounts: vec![count("M1", 2, 150)],
            voids: vec![count("M1", 1, 120)],
            refunds: vec![count("M1", 1, 90), count("GONE", 1, 10)],
            shifts: vec![
                shift("S1", "H1", 1_000, -50),
                shift("S1", "H2", 2_000, 20),
                shift("S2", "H3", 9_000_000, 7),
            ],
            names: vec![
                ("S1".into(), "Sita".into()),
                ("S2".into(), "Ravi".into()),
                ("M1".into(), "Meera".into()),
                ("IDLE".into(), "Idle".into()),
            ],
        };
        // Days are 1,000 ms long, so shifts closed at 1,000 and 2,000 are on days 1 and 2.
        let day_of = |ms: i64| format!("2026-10-{:02}", ms / 1000);
        let r = staff_report(&rows, "2026-10-01", "2026-10-05", &day_of);
        let by_id = |id: &str| r.staff.iter().find(|s| s.staff_id == id).unwrap();

        let s1 = by_id("S1");
        assert_eq!(
            (
                s1.name.as_str(),
                s1.sales_minor,
                s1.sales_count,
                s1.sessions_started
            ),
            ("Sita", 1200, 3, 4)
        );
        assert_eq!(s1.variance_minor, -30);
        assert_eq!(
            s1.shifts
                .iter()
                .map(|s| (s.shift_id.as_str(), s.variance_minor))
                .collect::<Vec<_>>(),
            [("H1", -50), ("H2", 20)]
        );
        let m1 = by_id("M1");
        assert_eq!((m1.discounts_count, m1.discounts_minor), (2, 150));
        assert_eq!((m1.voids_count, m1.voids_minor), (1, 120));
        assert_eq!((m1.refunds_count, m1.refunds_minor), (1, 90));
        assert_eq!(m1.sales_minor, 0, "a void is not a sale");

        let s2 = by_id("S2");
        assert!(
            s2.shifts.is_empty(),
            "closed on day 9000, outside the range"
        );
        assert_eq!(s2.variance_minor, 0);
        assert_eq!(by_id("GONE").name, "", "no longer in the staff table");
        assert!(
            r.staff.iter().all(|s| s.staff_id != "IDLE"),
            "a member with no activity is not listed"
        );
        assert_eq!(
            r.staff
                .iter()
                .map(|s| s.staff_id.as_str())
                .collect::<Vec<_>>(),
            ["S1", "S2", "GONE", "M1"],
            "sales descending, then name (empty first)"
        );
    }

    #[test]
    fn a_shift_closed_after_midnight_belongs_to_the_previous_business_day() {
        // 2026-10-11 01:30 in Kolkata with a 02:00 day start is business day 2026-10-10.
        let tz = chrono_tz::Asia::Kolkata;
        let closed_at = tz
            .with_ymd_and_hms(2026, 10, 11, 1, 30, 0)
            .unwrap()
            .timestamp_millis();
        let rows = StaffReportRows {
            shifts: vec![shift("S1", "H1", closed_at, -5)],
            ..StaffReportRows::default()
        };
        let day_of = |ms: i64| crate::locale::business_day_of(ms, tz, 120).to_string();
        let on = |from: &str, to: &str| staff_report(&rows, from, to, &day_of).staff.len();
        assert_eq!(on("2026-10-10", "2026-10-10"), 1);
        assert_eq!(on("2026-10-11", "2026-10-11"), 0);
        let (from_ms, to_ms) = shift_read_window("2026-10-10", "2026-10-10").unwrap();
        assert!((from_ms..=to_ms).contains(&closed_at));
    }

    #[test]
    fn an_empty_staff_range_lists_nobody() {
        let r = staff_report(&StaffReportRows::default(), "a", "b", &|_| String::new());
        assert!(r.staff.is_empty());
    }
}

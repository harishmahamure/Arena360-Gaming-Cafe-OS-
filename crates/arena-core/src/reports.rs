//! The Reports screen's figures (Story 5.27): Daily sales and Sessions, built from the rows
//! `arena-store` reads. Money comes from stored totals and the stamped `business_day` columns;
//! only the hour of day (and so utilisation) needs the branch time zone. Reading never goes
//! through `apply()`, so a 90-day report cannot block a write.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, CASH_DEPOSIT, CASH_DRAWER_OPEN, CASH_EXPENSE, DailySales, DeviceSessions, ErrorCode,
    GroupSessions, HourSales, NamedSales, RECEIVED_CREDIT_COLLECT, RECEIVED_TOPUP, REPORT_MAX_DAYS,
    ReportAmount, SALES_DISCOUNTS, SALES_PLANS, SALES_PLAY_TIME, SALES_PRODUCTS, SALES_TAX_ADDED,
    SessionsReport, ZCashLine, ZReference, ZReferenceGroup, ZRefund, ZReport, ZVoid, ZWaste,
};
use arena_store::{SalesReportRows, SessionsReportRows, ZCashEventRow, ZReportRows};
use chrono::{NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;

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
}

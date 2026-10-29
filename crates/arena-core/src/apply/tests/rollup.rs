//! Story 8.5: the daily rollup built from a real ledger equals that day's Z-report, and builds
//! within its budget on a busy day.

use super::prepaid::{group_of, sql};
use super::session::{pc, submit, table, text};
use super::stock::owner;
use super::z_report::{cash_sum, close, fixture_shift, only_shift, z_of};
use super::*;
use crate::snapshot::SnapshotReader;
use arena_proto::{DeviceKind, RollupV1, ShiftClose};

fn amount(list: &[arena_proto::ReportAmount], key: &str) -> i64 {
    list.iter()
        .find(|a| a.key == key)
        .map_or(0, |a| a.amount_minor)
}

fn rollup_of(f: &Fixture, day: &str) -> RollupV1 {
    SnapshotReader::open(&f.path)
        .unwrap()
        .daily_rollup(day, f.clock.now_ms())
        .unwrap()
        .unwrap()
}

fn business_day(f: &Fixture) -> String {
    let orders = table(f, Table::Orders);
    text(orders.first().unwrap(), "business_day")
}

#[tokio::test]
async fn the_rollup_of_a_day_equals_the_z_report_of_its_shift() {
    let f = bootstrapped_closed().await;
    fixture_shift(&f).await;
    let expected = 100_000 + 12_000 - 4_000 - 5_000 - 20_000;
    submit(
        &f,
        owner(),
        IntentPayload::ShiftClose(ShiftClose {
            counted_minor: expected - 500,
            ..close(0)
        }),
    )
    .await
    .unwrap();
    let z = z_of(&f);
    let day = business_day(&f);
    let r = rollup_of(&f, &day);

    assert_eq!((r.schema, r.business_day.as_str()), (1, day.as_str()));
    // Revenue, payments and cash equal the Z-report.
    assert_eq!(r.revenue.total, z.sales_minor);
    assert_eq!(r.revenue.by_method.cash, amount(&z.payments, "cash"));
    assert_eq!(r.revenue.by_method.upi, amount(&z.payments, "upi"));
    assert_eq!(r.revenue.by_method.card, amount(&z.payments, "card"));
    assert_eq!(
        r.revenue.by_method.cash,
        cash_sum(&f, &["sale", "sale_reverted"])
    );
    let t = &r.revenue.by_type;
    assert_eq!(t.play + t.products + t.plans + t.other, r.revenue.total);
    assert_eq!(t.products, amount(&z.by_type, "products"));
    // The undone sale is gone, the refund and the void are there.
    assert_eq!((r.refunds.count, r.refunds.amount), (1, -z.refunds_minor));
    assert_eq!(r.voids.count, z.voids.iter().map(|v| v.count).sum::<i64>());
    assert_eq!(
        r.voids.amount,
        z.voids.iter().map(|v| v.amount_minor).sum::<i64>()
    );
    // The shift and its variance.
    assert_eq!(r.shifts.len(), 1);
    let s = &r.shifts[0];
    assert_eq!(s.shift_id, text(&only_shift(&f), "id"));
    assert_eq!(
        (s.float, s.expected, s.counted, s.variance),
        (
            z.float_minor,
            z.expected_minor,
            z.counted_minor,
            z.variance_minor
        )
    );
    // Waste at cost, by product and reason.
    let waste: i64 = z.waste.iter().map(|w| w.value_minor).sum();
    assert_eq!(r.stock.waste.iter().map(|w| w.value).sum::<i64>(), waste);
    assert_eq!(r.stock.waste.len(), z.waste.len());
    // Stock levels list the tracked product; one session started.
    assert!(r.stock.levels.iter().any(|l| l.name == "Cola"));
    assert_eq!(r.sessions.count, 1);
    // Nothing leaks: no secret-looking field and no player name.
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("pin") && !json.contains("password") && !json.contains("phone"));
}

#[tokio::test]
async fn a_day_with_no_sales_is_zeros_and_a_bad_date_or_no_branch_is_handled() {
    let f = bootstrapped_closed().await;
    let r = rollup_of(&f, "2001-01-01");
    assert_eq!(r.revenue.total, 0);
    assert!(r.shifts.is_empty() && r.credit.players.is_empty() && r.play_hours.is_empty());
    assert_eq!(r.credit.outstanding, 0);

    let reader = SnapshotReader::open(&f.path).unwrap();
    let bad = reader.daily_rollup("2026-13-40", 0).unwrap_err();
    assert_eq!(bad.code, arena_proto::ErrorCode::ValidationFailed);

    let unbooted = primary();
    let none = SnapshotReader::open(&unbooted.path)
        .unwrap()
        .daily_rollup("2026-03-10", 0)
        .unwrap();
    assert!(none.is_none());
}

fn change_count(f: &Fixture) -> i64 {
    let mut n = 0;
    reopen(f)
        .write_tx(|tx| {
            n = tx
                .query_row("SELECT COUNT(*) FROM changes", [], |r| r.get(0))
                .unwrap();
            Ok(())
        })
        .unwrap();
    n
}

#[tokio::test]
async fn the_rollup_writes_no_row() {
    let f = bootstrapped_closed().await;
    fixture_shift(&f).await;
    let before = change_count(&f);
    let day = business_day(&f);
    rollup_of(&f, &day);
    assert_eq!(change_count(&f), before);
}

/// The reference PC and a busy day: 1,000 sessions and 500 orders. The rollup builds in well
/// under 5 s; a debug build gets the same ceiling.
#[tokio::test]
async fn a_busy_day_rolls_up_within_its_budget() {
    let f = bootstrapped_closed().await;
    let device = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let group = group_of(&f, &device);
    let day = crate::locale::current_business_day(&reopen(&f), f.clock.now_ms())
        .unwrap()
        .unwrap();
    let now = f.clock.now_ms();
    sql(
        &f,
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
             INSERT INTO sessions (id, device_id, group_id, mode, state, started_at, ended_at,
                                   rounding_min, minimum_min, grace_min, time_up_mode, epoch,
                                   business_day, fy, rev, created_at, updated_at)
             SELECT 'S' || i, '{device}', '{group}', 'postpaid', 'ended', {now}, {now} + 3600000,
                    1, 0, 0, 'lock', 'E', '{day}', '2026-27', 1, {now}, {now} FROM n;
             INSERT INTO session_segments (id, session_id, from_at, to_at, device_id,
                                           rate_minor_per_hour, rev, created_at, updated_at)
             SELECT 'G' || id, id, started_at, ended_at, device_id, 6000, 1, {now}, {now}
             FROM sessions WHERE id LIKE 'S%';
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500)
             INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                                 created_at, updated_at, settled_at, kind)
             SELECT 'O' || i, 'settled', 5000, 5000, '{day}', '2026-27', 1, {now}, {now}, {now},
                    'pos_sale' FROM n;
             INSERT INTO payments (id, order_id, method, amount_minor, staff_id, position,
                                   business_day, fy, rev, created_at, updated_at)
             SELECT 'P' || id, id, 'cash', 5000, 'test', 0, '{day}', '2026-27', 1, {now}, {now}
             FROM orders WHERE id LIKE 'O%';"
        ),
    )
    .await;
    let reader = SnapshotReader::open(&f.path).unwrap();
    reader.daily_rollup(&day, now).unwrap(); // warm the connection
    let started = std::time::Instant::now();
    let r = reader.daily_rollup(&day, now).unwrap().unwrap();
    let took = started.elapsed();
    assert_eq!(r.sessions.count, 1000);
    assert_eq!(r.sessions.minutes, 1000 * 60);
    assert_eq!(r.revenue.total, 500 * 5000);
    assert_eq!(r.revenue.by_method.cash, 500 * 5000);
    assert!(took.as_secs() < 5, "the rollup took {took:?} (budget 5 s)");
}

//! Story 8.5: the daily rollup built from a real ledger equals that day's Z-report, and builds
//! within its budget on a busy day.

use super::prepaid::{group_of, sql};
use super::session::{pc, submit, table, text};
use super::stock::owner;
use super::z_report::{cash_sum, close, fixture_shift, only_shift, z_of};
use super::*;
use crate::snapshot::SnapshotReader;
use arena_proto::{DeviceKind, ROLLUP_SCHEMA, RollupPublish, RollupV1, ShiftClose};

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

fn rollup_system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub-rollup".into(),
    }
}

fn publish(date: &str, built_at: i64) -> IntentPayload {
    IntentPayload::RollupPublish(RollupPublish {
        date: date.into(),
        schema: ROLLUP_SCHEMA,
        built_at,
        key: format!("rollups/{date}.json"),
    })
}

fn system_intent(payload: IntentPayload) -> Intent {
    super::stock::intent(rollup_system(), payload)
}

#[tokio::test]
async fn a_rollup_publish_upserts_only_that_days_rollup_log_row() {
    let f = bootstrapped_closed().await;
    let first = f
        .writer
        .submit_intent(system_intent(publish("2026-10-29", T0)))
        .await
        .unwrap();
    assert_eq!(first.changes.len(), 1);
    let c = &first.changes[0];
    assert_eq!(
        (c.tbl.as_str(), c.row_id.as_str(), c.op),
        ("rollup_log", "2026-10-29", ChangeOp::Insert)
    );
    f.writer
        .submit_intent(system_intent(publish("2026-10-30", T0)))
        .await
        .unwrap();
    // A regeneration overwrites: the same row, updated.
    let again = f
        .writer
        .submit_intent(system_intent(publish("2026-10-29", T0 + 600_000)))
        .await
        .unwrap();
    assert_eq!(again.changes[0].op, ChangeOp::Update);

    let store = reopen(&f);
    let rows = store.rows(Table::RollupLog).unwrap();
    assert_eq!(rows.len(), 2);
    let day = store.rollup_log("2026-10-29").unwrap().unwrap();
    assert_eq!(
        (day.schema, day.built_at, day.key.as_str()),
        (1, T0 + 600_000, "rollups/2026-10-29.json")
    );
    assert_eq!(store.rollup_log("latest").unwrap(), None);
}

#[tokio::test]
async fn only_the_system_can_publish_a_rollup_and_the_payload_is_checked() {
    let f = bootstrapped_closed().await;
    let refused = f
        .writer
        .submit_intent(super::stock::intent(owner(), publish("2026-10-29", T0)))
        .await;
    assert!(refused.is_err(), "an owner cannot publish a rollup");
    for (date, built_at, key) in [
        ("latest", T0, "rollups/x.json"),
        ("2026-13-40", T0, "rollups/x.json"),
        ("2026-10-29", -1, "rollups/x.json"),
        ("2026-10-29", T0, ""),
    ] {
        let mut p = RollupPublish {
            date: date.into(),
            schema: ROLLUP_SCHEMA,
            built_at,
            key: key.into(),
        };
        let e = f
            .writer
            .submit_intent(system_intent(IntentPayload::RollupPublish(p.clone())))
            .await
            .unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::ValidationFailed,
            "{date} {built_at} {key:?}"
        );
        p.schema = 0;
        p.date = "2026-10-29".into();
        p.key = "rollups/x.json".into();
        p.built_at = T0;
        assert!(
            f.writer
                .submit_intent(system_intent(IntentPayload::RollupPublish(p)))
                .await
                .is_err(),
            "schema 0"
        );
    }
    assert!(reopen(&f).rows(Table::RollupLog).unwrap().is_empty());
}

#[tokio::test]
async fn the_planner_reads_the_real_ledger_shifts_and_late_changes() {
    use std::collections::BTreeSet;

    let f = bootstrapped_closed().await;
    fixture_shift(&f).await;
    let expected = 100_000 + 12_000 - 4_000 - 5_000 - 20_000;
    let day = business_day(&f);
    let reader = SnapshotReader::open(&f.path).unwrap();
    let none = BTreeSet::new();
    let plan = |changes: &[arena_store::ChangedRow], carried: &BTreeSet<String>| {
        reader
            .rollup_plan(f.clock.now_ms(), changes, carried, false)
            .unwrap()
    };

    // Not paid as far as the Hub can tell: no upload yet.
    assert_eq!(plan(&[], &none), None);
    f.writer
        .submit(|tx| {
            arena_store::write_upload_cursor(
                tx,
                &arena_store::UploadCursor {
                    epoch: "1:X".into(),
                    last_seq_uploaded: 0,
                    uploaded_at: T0,
                },
            )
        })
        .await
        .unwrap();
    // A shift is open and the day is not over: nothing is due, the clock time is ahead.
    let p = plan(&[], &none).unwrap();
    assert!(p.due.is_empty(), "{p:?}");
    assert!(p.next_at.is_some());

    // The last shift closes: today is due at once.
    submit(
        &f,
        owner(),
        IntentPayload::ShiftClose(ShiftClose {
            counted_minor: expected,
            ..close(0)
        }),
    )
    .await
    .unwrap();
    assert_eq!(plan(&[], &none).unwrap().due, std::slice::from_ref(&day));

    // Published: quiet again, until a ledger row stamped to the day changes.
    f.writer
        .submit_intent(super::stock::intent(rollup_system(), publish(&day, T0)))
        .await
        .unwrap();
    assert!(plan(&[], &none).unwrap().due.is_empty());
    let order = text(table(&f, Table::Orders).first().unwrap(), "id");
    let seq = reopen(&f)
        .changes()
        .unwrap()
        .iter()
        .rfind(|c| c.tbl == "orders" && c.row_id == order)
        .unwrap()
        .seq;
    let late = arena_store::ChangedRow {
        epoch: reopen(&f).changes().unwrap().last().unwrap().epoch.clone(),
        seq,
        tbl: "orders".into(),
        row_id: order,
    };
    let p = plan(std::slice::from_ref(&late), &none).unwrap();
    assert_eq!(p.due, std::slice::from_ref(&day));
    assert_eq!(p.dirty, BTreeSet::from([day.clone()]));
    // A change outside the ledger tables re-opens nothing.
    let config = arena_store::ChangedRow {
        tbl: "settings".into(),
        ..late
    };
    assert!(plan(&[config], &none).unwrap().due.is_empty());
    // A day carried over from a pass that could not publish it is still due.
    assert_eq!(
        plan(&[], &BTreeSet::from([day.clone()])).unwrap().due,
        [day]
    );
}

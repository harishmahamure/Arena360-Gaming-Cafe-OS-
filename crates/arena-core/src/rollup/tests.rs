use super::*;
use arena_proto::CreditAging;
use arena_store::{
    RollupDebtorRow, RollupLotRow, RollupPaymentRow, RollupPlayRow, RollupVoidRow, SalesLineRow,
    SalesOrderRow, SegmentSpan, ShiftDetail, StockMoveReportRow, StockMovesRows, ThresholdRow,
};

const DAY: &str = "2026-03-10";

/// Times under a million belong to the fixture day, later ones to the next.
fn day_of(ts: i64) -> String {
    if ts < 1_000_000 { DAY } else { "2026-03-11" }.to_owned()
}

fn order(id: &str, kind: &str, total: i64, tax: i64, coupon: i64, discount: i64) -> SalesOrderRow {
    SalesOrderRow {
        id: id.into(),
        kind: kind.into(),
        total_minor: total,
        tax_minor: tax,
        coupon_minor: coupon,
        discount_minor: discount,
        settled_at: 100,
        group_id: "g1".into(),
    }
}

fn line(order_id: &str, amount: i64, qty: i64, product: bool, plan: bool) -> SalesLineRow {
    SalesLineRow {
        order_id: order_id.into(),
        amount_minor: amount,
        qty,
        is_product: product,
        is_plan: plan,
        category: if product {
            "Drinks".into()
        } else {
            String::new()
        },
    }
}

fn pay(method: &str, amount_minor: i64) -> RollupPaymentRow {
    RollupPaymentRow {
        method: method.into(),
        amount_minor,
    }
}

fn shift(id: &str, closed_at: i64) -> ShiftDetail {
    ShiftDetail {
        id: id.into(),
        open: false,
        opened_at: 100,
        closed_at: Some(closed_at),
        opened_by: "Asha".into(),
        closed_by: "Asha".into(),
        float_minor: 100_000,
        counted_minor: Some(120_000),
        expected_minor: Some(121_000),
        variance_minor: Some(-1_000),
        variance_reason: Some("short".into()),
        variance_note: None,
        approver: Some("Ravi".into()),
        z_report: None,
    }
}

fn mv(
    id: &str,
    product: (&str, &str),
    reason: &str,
    waste: Option<&str>,
    delta: i64,
    value: i64,
    at: i64,
) -> StockMoveReportRow {
    StockMoveReportRow {
        move_id: id.into(),
        product_id: product.0.into(),
        product: product.1.into(),
        reason: reason.into(),
        waste_reason: waste.map(str::to_owned),
        delta,
        unit_cost_minor: 20,
        value_minor: value,
        staff_id: "S".into(),
        at,
    }
}

fn debtor(
    id: &str,
    name: &str,
    owed: i64,
    oldest: &str,
    buckets: (i64, i64, i64),
    consent: bool,
) -> RollupDebtorRow {
    RollupDebtorRow {
        player_id: id.into(),
        username: name.into(),
        outstanding_minor: owed,
        aging: CreditAging {
            oldest_unpaid_day: Some(oldest.into()),
            oldest_age_days: 0,
            bucket_0_7_minor: buckets.0,
            bucket_8_14_minor: buckets.1,
            bucket_15_plus_minor: buckets.2,
        },
        consent,
    }
}

fn play(game: &str, ended_at: i64, minutes: i64) -> RollupPlayRow {
    RollupPlayRow {
        game_id: game.into(),
        ended_at,
        minutes,
    }
}

fn ctx() -> RollupContext<'static> {
    RollupContext {
        branch_id: "B1",
        currency: "INR",
        business_day: DAY,
        tz: Tz::UTC,
    }
}

/// A busy day: a session bill, a tax-exclusive counter sale, a plan sale with a coupon and a
/// manual discount, a refund, voids, waste (one undone), a closed shift with variance, credit
/// and plays; and rows of the next day that must not count.
fn fixture() -> RollupRows {
    let mut rows = RollupRows::default();
    rows.sales.orders = vec![
        order("o1", "session", 10_000, 0, 0, 0),
        order("o2", "pos_sale", 5_900, 900, 0, 0),
        order("o3", "plan_sale", 9_000, 0, 500, 500),
    ];
    rows.sales.lines = vec![
        line("o1", 10_000, 1, false, false),
        line("o2", 5_000, 2, true, false),
        line("o3", 10_000, 1, false, true),
    ];
    rows.sales.refunded_minor = 2_000;
    rows.refund_count = 1;
    rows.payments = vec![
        pay("cash", 10_000),
        pay("upi", 5_900),
        pay("wallet", 5_000),
        pay("card", 4_000),
        pay("cash", 3_000),
    ];
    rows.voids = vec![
        RollupVoidRow {
            voided_at: 100,
            amount_minor: 700,
        },
        RollupVoidRow {
            voided_at: 2_000_000,
            amount_minor: 900,
        },
    ];
    rows.session_count = 3;
    rows.segments = vec![
        SegmentSpan {
            session_id: "s1".into(),
            group_id: "g1".into(),
            from_at: 0,
            to_at: 3_600_000,
        },
        SegmentSpan {
            session_id: "s2".into(),
            group_id: "g1".into(),
            from_at: 0,
            to_at: 90_000,
        },
    ];
    rows.shifts = vec![shift("S1", 500), shift("S2", 2_000_000)];
    rows.levels = vec![
        ThresholdRow {
            product_id: "P2".into(),
            name: "Chips".into(),
            qty: 3,
            low_stock_qty: None,
        },
        ThresholdRow {
            product_id: "P1".into(),
            name: "Cola".into(),
            qty: 12,
            low_stock_qty: Some(5),
        },
    ];
    rows.moves = StockMovesRows {
        moves: vec![
            mv("M1", ("P1", "Cola"), "waste", Some("spoiled"), -2, 40, 200),
            mv(
                "M2",
                ("P1", "Cola"),
                "waste_reverted",
                Some("spoiled"),
                1,
                20,
                250,
            ),
            mv("M3", ("P2", "Chips"), "expired", None, -3, 30, 300),
            mv(
                "M4",
                ("P2", "Chips"),
                "waste",
                Some("damaged"),
                -9,
                90,
                2_000_000,
            ),
        ],
        staff: Vec::new(),
    };
    rows.lots = vec![RollupLotRow {
        product_id: "P1".into(),
        lot_id: "L1".into(),
        qty: 4,
        expires_on: "2026-03-12".into(),
    }];
    rows.debtors = vec![
        debtor(
            "PL1",
            "asha01",
            7_000,
            "2026-02-20",
            (1_000, 2_000, 4_000),
            true,
        ),
        debtor("PL2", "ravi", 3_000, "2026-03-08", (3_000, 0, 0), false),
    ];
    rows.plays = vec![
        play("G1", 400, 90),
        play("G2", 410, 50),
        play("G1", 420, 60),
        play("G3", 2_000_000, 30),
    ];
    rows
}

fn built() -> RollupV1 {
    fold_rollup(&fixture(), &ctx(), &day_of)
}

#[test]
fn the_fixed_fixture_equals_the_golden_file_exactly() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/golden/rollup_v1.json")).unwrap();
    assert_eq!(serde_json::to_value(built()).unwrap(), golden);
}

#[test]
fn the_types_add_up_to_the_total_and_the_methods_to_the_payments() {
    let r = built();
    let t = &r.revenue.by_type;
    assert_eq!(t.play + t.products + t.plans + t.other, r.revenue.total);
    let m = &r.revenue.by_method;
    assert_eq!(
        m.cash + m.upi + m.card + m.wallet + m.credit,
        13_000 + 5_900 + 4_000 + 5_000
    );
}

#[test]
fn the_same_rows_give_byte_identical_json() {
    assert_eq!(
        serde_json::to_string(&built()).unwrap(),
        serde_json::to_string(&built()).unwrap()
    );
}

#[test]
fn a_day_with_no_rows_is_a_valid_document_of_zeros() {
    let r = fold_rollup(&RollupRows::default(), &ctx(), &day_of);
    assert_eq!(r.schema, 1);
    assert_eq!(r.revenue, RollupRevenue::default());
    assert!(r.shifts.is_empty() && r.play_hours.is_empty() && r.credit.players.is_empty());
    let back: RollupV1 = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back, r);
}

#[test]
fn credit_players_are_capped_but_the_totals_cover_everyone() {
    let rows = RollupRows {
        debtors: (0..ROLLUP_PLAYERS_CAP + 5)
            .map(|i| {
                debtor(
                    &format!("P{i:04}"),
                    "u",
                    100,
                    "2026-03-09",
                    (100, 0, 0),
                    false,
                )
            })
            .collect(),
        ..RollupRows::default()
    };
    let r = fold_rollup(&rows, &ctx(), &day_of);
    assert_eq!(r.credit.players.len(), ROLLUP_PLAYERS_CAP);
    assert_eq!(r.credit.outstanding, 100 * (ROLLUP_PLAYERS_CAP as i64 + 5));
}

#[test]
fn a_date_that_does_not_exist_is_refused() {
    let e = parse_day("2026-13-40").unwrap_err();
    assert_eq!(e.code, ErrorCode::ValidationFailed);
    assert_eq!(e.field.as_deref(), Some("business_day"));
}

#[test]
fn a_shift_is_flagged_over_threshold_only_when_the_hub_knows_the_threshold() {
    let flags = |limit: Option<i64>| {
        let mut rows = fixture();
        rows.variance_threshold = limit;
        fold_rollup(&rows, &ctx(), &day_of)
            .shifts
            .iter()
            .map(|s| s.over_threshold)
            .collect::<Vec<_>>()
    };
    // The fixture shift is 1 000 short: beyond 500, within 1 000 and 5 000.
    assert_eq!(flags(Some(500)), [Some(true)]);
    assert_eq!(flags(Some(1_000)), [Some(false)]);
    assert_eq!(flags(Some(5_000)), [Some(false)]);
    assert_eq!(flags(None), [None]);
}

//! Story 5.21: coupons applied at settle.

use super::prepaid::{group_of, sql};
use super::session::{
    billed, end, int, pay, plan_of, refusal, rev_of, sale_order, sell_intent, sell_revert_intent,
    settle_intent, settle_revert_intent, submit, table, text,
};
use super::stock::{intent, make, owner, staff, stock_in, up};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::DeviceKind;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    CouponPreviewRequest, CouponTarget, CouponUpsert, PaymentRow, PosSale, PosSaleLine,
    PosSaleReverted, ProductUpsert,
};

// The fixture clock is 2027-01-15 in Asia/Kolkata.
const GST: &str = r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#;

fn coupon(code: &str) -> CouponUpsert {
    CouponUpsert {
        id: None,
        code: code.into(),
        percent_bp: Some(1_000),
        amount_minor: None,
        valid_from: None,
        valid_to: None,
        usage_limit: None,
        min_bill_minor: 0,
        targets: Vec::new(),
        active: true,
    }
}

fn target(kind: &str, id: &str) -> CouponTarget {
    CouponTarget {
        kind: kind.into(),
        id: id.into(),
    }
}

async fn save(
    f: &Fixture,
    actor: Actor,
    u: CouponUpsert,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, IntentPayload::CouponUpsert(u));
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

async fn made(f: &Fixture, u: CouponUpsert) -> String {
    save(f, owner(), u, None)
        .await
        .unwrap()
        .changes
        .iter()
        .find(|c| c.tbl == "coupons")
        .unwrap()
        .row_id
        .clone()
}

fn coupon_row(f: &Fixture, code: &str) -> Row {
    table(f, Table::Coupons)
        .into_iter()
        .find(|c| text(c, "code") == code)
        .unwrap()
}

fn used(f: &Fixture, code: &str) -> i64 {
    int(&coupon_row(f, code), "used")
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn code_of(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    refusal(r)
}

fn sale(lines: &[(&str, i64)], payments: Vec<PaymentRow>, code: Option<&str>) -> PosSale {
    PosSale {
        kiosk: None,
        business: None,
        coupon: code.map(Into::into),
        discount: None,
        lines: lines
            .iter()
            .map(|(id, qty)| PosSaleLine {
                product_id: (*id).into(),
                qty: *qty,
            })
            .collect(),
        payments,
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    }
}

async fn sell(f: &Fixture, s: PosSale) -> Result<Applied, AppError> {
    submit(f, owner(), IntentPayload::PosSale(s)).await
}

async fn product(f: &Fixture, name: &str, price: i64, rate_bp: Option<u16>) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: price,
            tax_rate_bp: rate_bp,
            ..up(name)
        },
    )
    .await;
    stock_in(f, &id, 20, 100).await;
    id
}

async fn gst(f: &Fixture) {
    sql(
        f,
        format!("UPDATE settings SET value = '{GST}' WHERE key = 'branch.tax'"),
    )
    .await;
}

fn pos_order(f: &Fixture) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .rev()
        .find(|o| text(o, "kind") == "pos_sale")
        .unwrap()
}

#[tokio::test]
async fn a_coupon_is_stored_upper_cased_with_its_terms_and_targets() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let group = group_of(
        &f,
        &super::session::pc(&f, DeviceKind::Pc, "pc", Some(1_000)).await,
    );
    let id = made(
        &f,
        CouponUpsert {
            valid_from: Some("2027-01-01".into()),
            valid_to: Some("2027-02-01".into()),
            usage_limit: Some(50),
            min_bill_minor: 10_000,
            targets: vec![target("product", &cola), target("group", &group)],
            ..coupon(" diwali-10 ")
        },
    )
    .await;
    let row = coupon_row(&f, "DIWALI-10");
    assert_eq!(text(&row, "coupon_id"), id);
    assert_eq!(
        (
            int(&row, "percent_bp"),
            row["amount_minor"].clone(),
            text(&row, "valid_from"),
            text(&row, "valid_to"),
            int(&row, "usage_limit"),
            int(&row, "used"),
            int(&row, "min_bill_minor"),
            int(&row, "active"),
            int(&row, "rev"),
        ),
        (
            1_000,
            RowValue::Null,
            "2027-01-01".to_owned(),
            "2027-02-01".to_owned(),
            50,
            0,
            10_000,
            1,
            1
        )
    );
    let targets = table(&f, Table::CouponTargets);
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|t| text(t, "coupon_id") == id));
    let kinds: Vec<String> = targets.iter().map(|t| text(t, "target_kind")).collect();
    assert_eq!(kinds, ["group", "product"]);

    made(
        &f,
        CouponUpsert {
            percent_bp: None,
            amount_minor: Some(5_000),
            ..coupon("FLAT-50")
        },
    )
    .await;
    assert_eq!(int(&coupon_row(&f, "FLAT-50"), "amount_minor"), 5_000);
}

#[tokio::test]
async fn a_bad_coupon_edit_is_refused_with_its_field_and_writes_nothing() {
    let f = bootstrapped().await;
    made(&f, coupon("TAKEN")).await;
    let before = changes(&f);
    let cases: Vec<(CouponUpsert, &str)> = vec![
        (coupon(""), "code"),
        (coupon("  "), "code"),
        (coupon("AB"), "code"),
        (coupon("HAS SPACE"), "code"),
        (coupon(&"X".repeat(25)), "code"),
        (coupon("taken"), "code"),
        (
            CouponUpsert {
                amount_minor: Some(100),
                ..coupon("BOTH")
            },
            "percent_bp",
        ),
        (
            CouponUpsert {
                percent_bp: None,
                ..coupon("NEITHER")
            },
            "percent_bp",
        ),
        (
            CouponUpsert {
                percent_bp: Some(10_001),
                ..coupon("OVER")
            },
            "percent_bp",
        ),
        (
            CouponUpsert {
                percent_bp: Some(0),
                ..coupon("ZERO")
            },
            "percent_bp",
        ),
        (
            CouponUpsert {
                percent_bp: None,
                amount_minor: Some(0),
                ..coupon("NOAMT")
            },
            "amount_minor",
        ),
        (
            CouponUpsert {
                valid_from: Some("2027-02-01".into()),
                valid_to: Some("2027-01-01".into()),
                ..coupon("BACKWARDS")
            },
            "valid_to",
        ),
        (
            CouponUpsert {
                valid_from: Some("1/2/2027".into()),
                ..coupon("BADDATE")
            },
            "valid_from",
        ),
        (
            CouponUpsert {
                usage_limit: Some(0),
                ..coupon("NOLIMIT")
            },
            "usage_limit",
        ),
        (
            CouponUpsert {
                min_bill_minor: -1,
                ..coupon("NEGMIN")
            },
            "min_bill_minor",
        ),
        (
            CouponUpsert {
                targets: vec![target("product", "nope")],
                ..coupon("GHOST")
            },
            "targets",
        ),
        (
            CouponUpsert {
                targets: vec![target("shelf", "x")],
                ..coupon("SHELF")
            },
            "targets",
        ),
        (
            CouponUpsert {
                id: Some("missing".into()),
                ..coupon("EDITNONE")
            },
            "id",
        ),
    ];
    for (u, field) in cases {
        let label = u.code.clone();
        assert_eq!(
            code_of(save(&f, owner(), u, None).await),
            (ErrorCode::ValidationFailed, Some(field.into())),
            "{label}"
        );
    }
    assert_eq!(changes(&f), before);
    assert_eq!(table(&f, Table::Coupons).len(), 1);
}

#[tokio::test]
async fn editing_needs_the_current_rev_replaces_targets_and_keeps_the_use_count() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let chips = product(&f, "Chips", 2_000, None).await;
    let id = made(
        &f,
        CouponUpsert {
            targets: vec![target("product", &cola)],
            ..coupon("SAVE10")
        },
    )
    .await;
    sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("save10")),
    )
    .await
    .unwrap();
    let rev = int(&coupon_row(&f, "SAVE10"), "rev");
    assert_eq!(rev, 2, "a use is a write");

    let edit = CouponUpsert {
        id: Some(id.clone()),
        percent_bp: None,
        amount_minor: Some(300),
        active: false,
        targets: vec![target("product", &chips)],
        ..coupon("SAVE10")
    };
    assert_eq!(
        code_of(save(&f, owner(), edit.clone(), Some(rev - 1)).await),
        (ErrorCode::Stale, Some("base_rev".into()))
    );
    assert_eq!(
        code_of(
            save(
                &f,
                staff(&f, "mira", ROLE_MANAGER).await,
                edit.clone(),
                None
            )
            .await
        ),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    save(&f, owner(), edit, Some(rev)).await.unwrap();
    let row = coupon_row(&f, "SAVE10");
    assert_eq!(
        (
            row["percent_bp"].clone(),
            int(&row, "amount_minor"),
            int(&row, "active"),
            int(&row, "used"),
            int(&row, "rev")
        ),
        (RowValue::Null, 300, 0, 1, rev + 1)
    );
    let targets = table(&f, Table::CouponTargets);
    assert_eq!(targets.len(), 1);
    assert_eq!(text(&targets[0], "target_id"), chips);

    // A deactivated coupon stays listed and the same code can be edited back to itself.
    assert_eq!(reopen(&f).coupons().unwrap().len(), 1);

    // A used coupon keeps its code: orders name it by code and Undo finds it again that way.
    let renamed = CouponUpsert {
        id: Some(id),
        amount_minor: Some(300),
        percent_bp: None,
        targets: vec![target("product", &chips)],
        ..coupon("RENAMED")
    };
    assert_eq!(
        code_of(save(&f, owner(), renamed, Some(rev + 1)).await),
        (ErrorCode::ValidationFailed, Some("code".into()))
    );
}

#[tokio::test]
async fn only_catalog_editors_write_coupons() {
    let f = bootstrapped().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    assert_eq!(
        code_of(save(&f, counter, coupon("NOPE1"), None).await).0,
        ErrorCode::PermissionDenied
    );
    save(
        &f,
        staff(&f, "mira", ROLE_MANAGER).await,
        coupon("YES10"),
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_percent_coupon_is_spread_over_two_rates_and_tax_is_computed_per_rate() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 1_180, Some(1_800)).await;
    let chips = product(&f, "Chips", 105, Some(500)).await;
    gst(&f).await;
    made(&f, coupon("TEN")).await;

    // 10% of 1,285 is 129 (half-up); the exact shares are 10.54 at 5% and 118.46 at 18%, so the
    // spare minor unit goes to the larger remainder: 11 and 118.
    sell(
        &f,
        sale(
            &[(&cola, 1), (&chips, 1)],
            vec![pay(Cash, 1_156, None)],
            Some("ten"),
        ),
    )
    .await
    .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "subtotal_minor"),
            int(&o, "tax_minor"),
            int(&o, "total_minor"),
            text(&o, "coupon_code"),
            int(&o, "coupon_minor")
        ),
        (990, 166, 1_156, "TEN".to_owned(), 129)
    );
    let breakup: serde_json::Value = serde_json::from_str(&text(&o, "tax_breakup")).unwrap();
    assert_eq!(
        (
            breakup[0]["rate_bp"].clone(),
            breakup[0]["taxable_minor"].clone(),
            breakup[0]["tax_minor"].clone(),
            breakup[1]["rate_bp"].clone(),
            breakup[1]["taxable_minor"].clone(),
            breakup[1]["tax_minor"].clone(),
        ),
        (
            500.into(),
            90.into(),
            4.into(),
            1_800.into(),
            900.into(),
            162.into()
        )
    );
    // The lines keep their own prices: the coupon is a line of its own, not a price change.
    let mut amounts: Vec<i64> = table(&f, Table::OrderLines)
        .iter()
        .map(|l| int(l, "amount_minor"))
        .collect();
    amounts.sort_unstable();
    assert_eq!(amounts, [105, 1_180]);
    assert_eq!(used(&f, "TEN"), 1);
    let cash = table(&f, Table::CashEvents);
    assert_eq!(int(cash.last().unwrap(), "amount_minor"), 1_156);
}

#[tokio::test]
async fn a_targeted_coupon_discounts_only_the_eligible_lines() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 1_180, Some(1_800)).await;
    let chips = product(&f, "Chips", 105, Some(500)).await;
    gst(&f).await;
    made(
        &f,
        CouponUpsert {
            targets: vec![target("product", &cola)],
            ..coupon("COLA10")
        },
    )
    .await;
    sell(
        &f,
        sale(
            &[(&cola, 1), (&chips, 1)],
            vec![pay(Cash, 1_167, None)],
            Some("COLA10"),
        ),
    )
    .await
    .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (int(&o, "total_minor"), int(&o, "coupon_minor")),
        (1_167, 118)
    );

    // Nothing in this cart is eligible.
    let before = changes(&f);
    assert_eq!(
        code_of(
            sell(
                &f,
                sale(&[(&chips, 1)], vec![pay(Cash, 105, None)], Some("COLA10"))
            )
            .await
        ),
        (ErrorCode::CouponInvalid, Some("coupon".into()))
    );
    assert_eq!(changes(&f), before);
    assert_eq!(used(&f, "COLA10"), 1);
}

#[tokio::test]
async fn a_bundle_line_is_a_product_line_for_a_target() {
    let f = bootstrapped().await;
    let chips = product(&f, "Chips", 2_000, None).await;
    let combo = make(
        &f,
        ProductUpsert {
            bundle: true,
            track_stock: false,
            components: vec![arena_proto::BundleComponent {
                product_id: chips.clone(),
                qty: 2,
            }],
            price_minor: 5_000,
            ..up("Combo")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            targets: vec![target("product", &combo)],
            ..coupon("COMBO10")
        },
    )
    .await;
    sell(
        &f,
        sale(
            &[(&combo, 1), (&chips, 1)],
            vec![pay(Cash, 6_500, None)],
            Some("COMBO10"),
        ),
    )
    .await
    .unwrap();
    assert_eq!(int(&pos_order(&f), "coupon_minor"), 500);
}

#[tokio::test]
async fn a_fixed_coupon_is_never_more_than_the_eligible_amount() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 3_000, None).await;
    made(
        &f,
        CouponUpsert {
            percent_bp: None,
            amount_minor: Some(50_000),
            ..coupon("BIG")
        },
    )
    .await;
    sell(&f, sale(&[(&cola, 1)], Vec::new(), Some("BIG")))
        .await
        .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "total_minor"),
            int(&o, "coupon_minor"),
            int(&o, "subtotal_minor")
        ),
        (0, 3_000, 0)
    );
    assert!(table(&f, Table::Payments).is_empty());
}

#[tokio::test]
async fn an_invalid_coupon_is_refused_with_its_reason_and_the_bill_is_unchanged() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(
        &f,
        CouponUpsert {
            valid_to: Some("2027-01-14".into()),
            ..coupon("EXPIRED")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            valid_from: Some("2027-01-16".into()),
            ..coupon("LATER")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            valid_from: Some("2027-01-15".into()),
            valid_to: Some("2027-01-15".into()),
            ..coupon("TODAY")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            active: false,
            ..coupon("OFF")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            min_bill_minor: 5_000,
            ..coupon("BIGBILL")
        },
    )
    .await;
    made(
        &f,
        CouponUpsert {
            usage_limit: Some(1),
            ..coupon("ONCE")
        },
    )
    .await;
    let before = changes(&f);
    let reasons = [
        ("EXPIRED", "expired"),
        ("LATER", "not valid yet"),
        ("OFF", "not active"),
        ("BIGBILL", "minimum"),
        ("NOSUCH", "does not exist"),
        ("", "does not exist"),
    ];
    for (code, reason) in reasons {
        let e = sell(
            &f,
            sale(&[(&cola, 1)], vec![pay(Cash, 4_000, None)], Some(code)),
        )
        .await
        .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::CouponInvalid, Some("coupon")),
            "{code}"
        );
        assert!(e.message.contains(reason), "{code}: {}", e.message);
    }
    assert_eq!(changes(&f), before, "nothing was written");
    assert!(table(&f, Table::Orders).is_empty());

    // The last day of a window is inclusive.
    sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("TODAY")),
    )
    .await
    .unwrap();
    // The minimum is checked against the bill before the coupon.
    sell(
        &f,
        sale(&[(&cola, 2)], vec![pay(Cash, 7_200, None)], Some("BIGBILL")),
    )
    .await
    .unwrap();

    sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("ONCE")),
    )
    .await
    .unwrap();
    let e = sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("ONCE")),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::CouponInvalid);
    assert!(e.message.contains("used up"));
    assert_eq!(used(&f, "ONCE"), 1);
}

#[tokio::test]
async fn the_total_with_the_coupon_is_what_the_payments_must_add_up_to() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(&f, coupon("TEN")).await;
    let before = changes(&f);
    assert_eq!(
        code_of(
            sell(
                &f,
                sale(&[(&cola, 1)], vec![pay(Cash, 4_000, None)], Some("TEN"))
            )
            .await
        ),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);
    assert_eq!(used(&f, "TEN"), 0);
}

#[tokio::test]
async fn two_sales_racing_for_the_last_use_one_wins_and_the_other_is_refused() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(
        &f,
        CouponUpsert {
            usage_limit: Some(1),
            ..coupon("LAST")
        },
    )
    .await;
    let one = || sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("LAST"));
    let (a, b) = tokio::join!(sell(&f, one()), sell(&f, one()));
    let refused = match (a, b) {
        (Ok(_), Err(e)) | (Err(e), Ok(_)) => e,
        other => panic!("one wins, one is refused: {other:?}"),
    };
    assert_eq!(refused.code, ErrorCode::CouponInvalid);
    assert_eq!(used(&f, "LAST"), 1);
    assert_eq!(
        table(&f, Table::Orders)
            .iter()
            .filter(|o| text(o, "coupon_code") == "LAST")
            .count(),
        1
    );
}

#[tokio::test]
async fn undoing_a_counter_sale_gives_the_use_back() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(&f, coupon("TEN")).await;
    let a = sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("TEN")),
    )
    .await
    .unwrap();
    let order = a.changes[0].row_id.clone();
    assert_eq!(used(&f, "TEN"), 1);
    f.clock.advance(3_000);
    let rev = rev_of(&f, Table::Orders, &order);
    let mut i = intent(
        owner(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.clone(),
        }),
    );
    i.base_rev = Some(rev);
    i.issued_at = f.clock.now_ms();
    let undone = f.writer.submit_intent(i).await.unwrap();
    assert!(undone.changes.iter().any(|c| c.tbl == "coupons"));
    assert_eq!(used(&f, "TEN"), 0);
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn a_session_bill_takes_the_coupon_off_its_total_and_undo_puts_it_back() {
    let f = bootstrapped().await;
    let (order, device, rev) = billed(&f, 10_000, 41).await;
    let group = group_of(&f, &device);
    made(
        &f,
        CouponUpsert {
            targets: vec![target("group", &group)],
            ..coupon("PLAY10")
        },
    )
    .await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 7_500, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.coupon = Some("play10".into());
    }
    let before = changes(&f);
    assert_eq!(
        code_of(end(&f, i.clone()).await),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);

    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.payments = vec![pay(Cash, 6_750, None)];
    }
    i.intent_id = ulid::Ulid::generate().to_string();
    i.idempotency_key = ulid::Ulid::generate().to_string();
    let a = end(&f, i).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["payments", "cash_events", "orders", "coupons"]
    );
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor"),
            text(o, "coupon_code"),
            int(o, "coupon_minor")
        ),
        ("settled".to_owned(), 6_750, 6_750, "PLAY10".to_owned(), 750)
    );
    assert_eq!(used(&f, "PLAY10"), 1);

    f.clock.advance(3_000);
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, settle_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor")
        ),
        ("open".to_owned(), 7_500, 7_500)
    );
    assert_eq!(o["coupon_code"], RowValue::Null);
    assert_eq!(o["coupon_minor"], RowValue::Null);
    assert_eq!(used(&f, "PLAY10"), 0);

    // The same bill settles again, with or without the coupon.
    let rev = rev_of(&f, Table::Orders, &order);
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 7_500, None)]),
    )
    .await
    .unwrap();
    assert_eq!(used(&f, "PLAY10"), 0);
}

#[tokio::test]
async fn a_group_coupon_does_not_apply_to_another_groups_time() {
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let other = group_of(
        &f,
        &super::session::pc(&f, DeviceKind::Pc, "pc", Some(1_000)).await,
    );
    made(
        &f,
        CouponUpsert {
            targets: vec![target("group", &other)],
            ..coupon("ELSEWHERE")
        },
    )
    .await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 6_750, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.coupon = Some("ELSEWHERE".into());
    }
    let e = end(&f, i).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::CouponInvalid);
    assert!(e.message.contains("does not apply"));
}

#[tokio::test]
async fn a_plan_sale_takes_the_coupon_and_undo_gives_the_use_back() {
    let f = bootstrapped().await;
    let p = super::session::player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let other = plan_of(&f, "pack", 20_000, true).await;
    made(
        &f,
        CouponUpsert {
            percent_bp: None,
            amount_minor: Some(5_000),
            targets: vec![target("plan", &plan)],
            ..coupon("PACK50")
        },
    )
    .await;
    let with = |plan: &str, paid: i64| {
        let mut i = sell_intent(&p, plan, vec![pay(Cash, paid, None)]);
        if let IntentPayload::PlanSell(s) = &mut i.payload {
            s.coupon = Some("pack50".into());
        }
        i
    };
    assert_eq!(
        end(&f, with(&other, 20_000)).await.unwrap_err().code,
        ErrorCode::CouponInvalid
    );
    assert_eq!(
        code_of(end(&f, with(&plan, 30_000)).await),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    let a = end(&f, with(&plan, 25_000)).await.unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "coupons"));
    let o = sale_order(&f);
    assert_eq!(
        (
            int(&o, "subtotal_minor"),
            int(&o, "total_minor"),
            text(&o, "coupon_code"),
            int(&o, "coupon_minor")
        ),
        (25_000, 25_000, "PACK50".to_owned(), 5_000)
    );
    assert_eq!(
        int(&table(&f, Table::OrderLines)[0], "amount_minor"),
        30_000
    );
    assert_eq!(used(&f, "PACK50"), 1);

    let order = text(&o, "id");
    f.clock.advance(3_000);
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, sell_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    assert_eq!(used(&f, "PACK50"), 0);
}

#[tokio::test]
async fn a_coupon_with_nothing_to_pay_is_refused() {
    let f = bootstrapped().await;
    let free = make(
        &f,
        ProductUpsert {
            price_minor: 0,
            ..up("Water")
        },
    )
    .await;
    stock_in(&f, &free, 5, 0).await;
    made(&f, coupon("TEN")).await;
    let e = sell(&f, sale(&[(&free, 1)], Vec::new(), Some("TEN")))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::CouponInvalid);
    assert!(e.message.contains("nothing to pay"));
}

#[tokio::test]
async fn the_preview_prices_a_code_on_a_cart_a_bill_and_a_plan_without_writing() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(&f, coupon("TEN")).await;
    let (order, _, _) = billed(&f, 10_000, 41).await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let before = changes(&f);
    let store = reopen(&f);
    let preview = |req: CouponPreviewRequest| {
        store.read_with(|conn| crate::apply::coupon_preview(conn, &req, f.clock.now_ms()))
    };
    let cart = preview(CouponPreviewRequest {
        code: "ten".into(),
        order_id: None,
        lines: Some(vec![PosSaleLine {
            product_id: cola.clone(),
            qty: 2,
        }]),
        plan_id: None,
    })
    .unwrap();
    assert_eq!(
        (cart.code.as_str(), cart.discount_minor, cart.total_minor),
        ("TEN", 800, 7_200)
    );
    let bill = preview(CouponPreviewRequest {
        code: "TEN".into(),
        order_id: Some(order),
        lines: None,
        plan_id: None,
    })
    .unwrap();
    assert_eq!((bill.discount_minor, bill.total_minor), (750, 6_750));
    let pack = preview(CouponPreviewRequest {
        code: "TEN".into(),
        order_id: None,
        lines: None,
        plan_id: Some(plan.clone()),
    })
    .unwrap();
    assert_eq!((pack.discount_minor, pack.total_minor), (3_000, 27_000));
    let bad = preview(CouponPreviewRequest {
        code: "NOPE".into(),
        order_id: None,
        lines: None,
        plan_id: Some(plan.clone()),
    })
    .unwrap_err();
    assert_eq!(bad.code, ErrorCode::CouponInvalid);
    let none = preview(CouponPreviewRequest {
        code: "TEN".into(),
        order_id: None,
        lines: None,
        plan_id: None,
    })
    .unwrap_err();
    assert_eq!(none.code, ErrorCode::ValidationFailed);
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn the_receipt_source_carries_the_coupon_and_the_net_total() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    made(&f, coupon("TEN")).await;
    let a = sell(
        &f,
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], Some("TEN")),
    )
    .await
    .unwrap();
    let order = a.changes[0].row_id.clone();
    let source = reopen(&f)
        .read_with(|conn| arena_store::load_receipt_source(conn, &order))
        .unwrap()
        .unwrap();
    assert_eq!(source.head.coupon, Some(("TEN".to_owned(), 400)));
    assert_eq!(source.head.total_minor, 3_600);

    let bytes = crate::snapshot::SnapshotReader::open(&f.path)
        .unwrap()
        .receipt_bytes(&order, true, f.clock.now_ms())
        .unwrap()
        .unwrap();
    let printed = String::from_utf8_lossy(&bytes);
    let line = printed
        .lines()
        .find(|l| l.starts_with("Coupon TEN"))
        .expect("the receipt prints the coupon");
    assert!(line.ends_with("-4.00"), "{line}");
    assert!(printed.find("Coupon TEN") < printed.find("TOTAL"));
}

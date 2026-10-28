//! Story 5.22: a manual discount at settle, approved with a manager PIN.

use super::prepaid::{group_of, sql};
use super::session::{
    billed, end, int, pay, plan_of, refusal, rev_of, sale_order, sell_intent, sell_revert_intent,
    settle_intent, settle_revert_intent, table, text,
};
use super::stock::{intent, make, manager_with_pin, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    CouponTarget, CouponUpsert, DiscountPreviewRequest, DiscountReason, ElevatedBy, ElevationKind,
    ElevationMode, ManualDiscount, PaymentRow, PosSale, PosSaleLine, PosSaleReverted,
    ProductUpsert,
};

// The fixture clock is 2027-01-15 in Asia/Kolkata.
const GST: &str = r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#;

fn percent(bp: i64) -> ManualDiscount {
    ManualDiscount {
        percent_bp: Some(bp),
        amount_minor: None,
        reason: DiscountReason::RegularCustomer,
        note: None,
    }
}

fn amount(minor: i64) -> ManualDiscount {
    ManualDiscount {
        percent_bp: None,
        amount_minor: Some(minor),
        reason: DiscountReason::ServiceIssue,
        note: None,
    }
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn sale(
    lines: &[(&str, i64)],
    payments: Vec<PaymentRow>,
    coupon: Option<&str>,
    discount: Option<ManualDiscount>,
) -> PosSale {
    PosSale {
        business: None,
        coupon: coupon.map(Into::into),
        discount,
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

async fn sell_as(
    f: &Fixture,
    actor: Actor,
    s: PosSale,
    elevated_by: Option<&str>,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, IntentPayload::PosSale(s));
    i.elevated_by = elevated_by.map(|id| ElevatedBy {
        kind: ElevationKind::Pin,
        id: id.into(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    f.writer.submit_intent(i).await
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

async fn coupon(f: &Fixture, code: &str, percent_bp: i64) {
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::CouponUpsert(CouponUpsert {
                id: None,
                code: code.into(),
                percent_bp: Some(percent_bp),
                amount_minor: None,
                valid_from: None,
                valid_to: None,
                usage_limit: None,
                min_bill_minor: 0,
                targets: Vec::<CouponTarget>::new(),
                active: true,
            }),
        ))
        .await
        .unwrap();
}

fn pos_order(f: &Fixture) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .rev()
        .find(|o| text(o, "kind") == "pos_sale")
        .unwrap()
}

#[tokio::test]
async fn a_counter_sale_takes_the_discount_and_stores_its_reason_and_approver() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let d = ManualDiscount {
        reason: DiscountReason::Other,
        note: Some("  broken fridge  ".into()),
        ..percent(1_000)
    };
    sell_as(
        &f,
        owner(),
        sale(&[(&cola, 1)], vec![pay(Cash, 3_600, None)], None, Some(d)),
        None,
    )
    .await
    .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "subtotal_minor"),
            int(&o, "total_minor"),
            int(&o, "discount_minor"),
            text(&o, "discount_reason"),
            text(&o, "discount_note"),
            text(&o, "discount_by"),
            o["coupon_minor"].clone(),
        ),
        (
            3_600,
            3_600,
            400,
            "other".to_owned(),
            "broken fridge".to_owned(),
            "o".to_owned(),
            RowValue::Null
        )
    );
}

#[tokio::test]
async fn the_coupon_applies_first_and_the_discount_takes_a_share_of_the_remainder() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 1_180, Some(1_800)).await;
    let chips = product(&f, "Chips", 105, Some(500)).await;
    gst(&f).await;
    coupon(&f, "TEN", 1_000).await;
    // The coupon takes 129 off 1,285 (11 at 5%, 118 at 18%); 10% of the 1,156 left is 116, shared
    // 9 and 107 over what is left of each rate (94 and 1,062) by largest remainder.
    sell_as(
        &f,
        owner(),
        sale(
            &[(&cola, 1), (&chips, 1)],
            vec![pay(Cash, 1_040, None)],
            Some("ten"),
            Some(percent(1_000)),
        ),
        None,
    )
    .await
    .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "total_minor"),
            int(&o, "coupon_minor"),
            int(&o, "discount_minor")
        ),
        (1_040, 129, 116)
    );
    let breakup: serde_json::Value = serde_json::from_str(&text(&o, "tax_breakup")).unwrap();
    assert_eq!(
        (
            breakup[0]["rate_bp"].clone(),
            breakup[0]["taxable_minor"].clone(),
            breakup[1]["rate_bp"].clone(),
            breakup[1]["taxable_minor"].clone(),
        ),
        (500.into(), 81.into(), 1_800.into(), 809.into())
    );
}

#[tokio::test]
async fn a_discount_needs_discount_manual_on_the_actor_or_the_approving_manager() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    let s = sale(
        &[(&cola, 1)],
        vec![pay(Cash, 3_000, None)],
        None,
        Some(amount(1_000)),
    );
    let before = changes(&f);
    assert_eq!(
        refusal(sell_as(&f, counter.clone(), s.clone(), None).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);
    // The same counter sells without a discount on its own.
    let plain = sale(&[(&cola, 1)], vec![pay(Cash, 4_000, None)], None, None);
    sell_as(&f, counter.clone(), plain, None).await.unwrap();
    // With a manager's PIN the counter's sale takes it, and the manager is the approver on record.
    sell_as(&f, counter.clone(), s, Some(&manager))
        .await
        .unwrap();
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "discount_minor"),
            text(&o, "discount_by"),
            text(&o, "settled_by")
        ),
        (1_000, manager.clone(), counter.id.clone())
    );
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(change.actor_id, counter.id);
    assert_eq!(change.elevated_by_id.as_deref(), Some(manager.as_str()));
}

#[tokio::test]
async fn a_bad_discount_is_refused_with_its_field_and_nothing_is_written() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let before = changes(&f);
    let other = |note: Option<&str>| ManualDiscount {
        reason: DiscountReason::Other,
        note: note.map(Into::into),
        ..percent(500)
    };
    let cases: Vec<(ManualDiscount, &str)> = vec![
        (
            ManualDiscount {
                amount_minor: Some(100),
                ..percent(500)
            },
            "discount.percent_bp",
        ),
        (
            ManualDiscount {
                percent_bp: None,
                ..percent(500)
            },
            "discount.percent_bp",
        ),
        (percent(0), "discount.percent_bp"),
        (percent(10_001), "discount.percent_bp"),
        (amount(0), "discount.amount_minor"),
        (amount(4_001), "discount.amount_minor"),
        (other(None), "discount.note"),
        (other(Some("   ")), "discount.note"),
        (other(Some(&"x".repeat(201))), "discount.note"),
        (other(Some("bad\u{7}text")), "discount.note"),
    ];
    for (d, field) in cases {
        let s = sale(&[(&cola, 1)], vec![pay(Cash, 4_000, None)], None, Some(d));
        assert_eq!(
            refusal(sell_as(&f, owner(), s, None).await),
            (ErrorCode::ValidationFailed, Some(field.into())),
            "{field}"
        );
    }
    assert_eq!(changes(&f), before);
    // 100% is allowed and leaves nothing to pay.
    let s = sale(&[(&cola, 1)], Vec::new(), None, Some(percent(10_000)));
    sell_as(&f, owner(), s, None).await.unwrap();
    assert_eq!(int(&pos_order(&f), "total_minor"), 0);
}

#[tokio::test]
async fn the_discount_comes_off_what_the_coupon_left() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    coupon(&f, "HALF", 5_000).await;
    // 2,000 is left after the coupon: an amount over that is refused, 2,000 itself is fine.
    let s = |paid: i64, d: i64| {
        sale(
            &[(&cola, 1)],
            if paid == 0 {
                Vec::new()
            } else {
                vec![pay(Cash, paid, None)]
            },
            Some("HALF"),
            Some(amount(d)),
        )
    };
    assert_eq!(
        refusal(sell_as(&f, owner(), s(0, 2_001), None).await),
        (
            ErrorCode::ValidationFailed,
            Some("discount.amount_minor".into())
        )
    );
    let a = sell_as(&f, owner(), s(0, 2_000), None).await;
    assert!(a.is_ok(), "{a:?}");
    let o = pos_order(&f);
    assert_eq!(
        (
            int(&o, "total_minor"),
            int(&o, "coupon_minor"),
            int(&o, "discount_minor")
        ),
        (0, 2_000, 2_000)
    );
}

#[tokio::test]
async fn the_total_with_the_discount_is_what_the_payments_must_add_up_to() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    let s = sale(
        &[(&cola, 1)],
        vec![pay(Cash, 4_000, None)],
        None,
        Some(percent(2_500)),
    );
    let before = changes(&f);
    assert_eq!(
        refusal(sell_as(&f, owner(), s, None).await),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn undoing_a_counter_sale_reverts_it_whole() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    coupon(&f, "TEN", 1_000).await;
    let a = sell_as(
        &f,
        owner(),
        sale(
            &[(&cola, 1)],
            vec![pay(Cash, 3_240, None)],
            Some("TEN"),
            Some(percent(1_000)),
        ),
        None,
    )
    .await
    .unwrap();
    let order = a.changes[0].row_id.clone();
    assert_eq!(int(&pos_order(&f), "discount_minor"), 360);
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
    f.writer.submit_intent(i).await.unwrap();
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn a_session_bill_takes_the_discount_off_its_total_and_undo_puts_it_back() {
    let f = bootstrapped().await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 7_500, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.discount = Some(percent(1_000));
    }
    let before = changes(&f);
    assert_eq!(
        refusal(end(&f, i.clone()).await),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);

    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.payments = vec![pay(Cash, 6_750, None)];
    }
    i.intent_id = ulid::Ulid::generate().to_string();
    i.idempotency_key = ulid::Ulid::generate().to_string();
    end(&f, i).await.unwrap();
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor"),
            int(o, "discount_minor"),
            text(o, "discount_reason"),
            text(o, "discount_by"),
        ),
        (
            "settled".to_owned(),
            6_750,
            6_750,
            750,
            "regular_customer".to_owned(),
            "o".to_owned()
        )
    );

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
    assert_eq!(
        (
            o["discount_minor"].clone(),
            o["discount_reason"].clone(),
            o["discount_note"].clone(),
            o["discount_by"].clone()
        ),
        (
            RowValue::Null,
            RowValue::Null,
            RowValue::Null,
            RowValue::Null
        )
    );
}

#[tokio::test]
async fn a_session_bill_takes_the_coupon_then_the_discount_and_undo_restores_both() {
    let f = bootstrapped().await;
    let (order, device, rev) = billed(&f, 10_000, 41).await;
    let _ = group_of(&f, &device);
    coupon(&f, "TEN", 1_000).await;
    let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, 6_075, None)]);
    if let IntentPayload::SessionSettle(s) = &mut i.payload {
        s.coupon = Some("ten".into());
        s.discount = Some(percent(1_000));
    }
    end(&f, i).await.unwrap();
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(
        (
            int(o, "total_minor"),
            int(o, "coupon_minor"),
            int(o, "discount_minor")
        ),
        (6_075, 750, 675)
    );
    f.clock.advance(3_000);
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, settle_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    let o = &table(&f, Table::Orders)[0];
    assert_eq!(
        (int(o, "subtotal_minor"), int(o, "total_minor")),
        (7_500, 7_500)
    );
    assert_eq!(o["coupon_minor"], RowValue::Null);
    assert_eq!(o["discount_minor"], RowValue::Null);
}

#[tokio::test]
async fn a_plan_sale_takes_the_discount() {
    let f = bootstrapped().await;
    let p = super::session::player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let with = |paid: i64| {
        let mut i = sell_intent(&p, &plan, vec![pay(Cash, paid, None)]);
        if let IntentPayload::PlanSell(s) = &mut i.payload {
            s.discount = Some(amount(5_000));
        }
        i
    };
    assert_eq!(
        refusal(end(&f, with(30_000)).await),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    end(&f, with(25_000)).await.unwrap();
    let o = sale_order(&f);
    assert_eq!(
        (
            int(&o, "subtotal_minor"),
            int(&o, "total_minor"),
            int(&o, "discount_minor"),
            text(&o, "discount_reason")
        ),
        (25_000, 25_000, 5_000, "service_issue".to_owned())
    );
    let order = text(&o, "id");
    f.clock.advance(3_000);
    let rev = rev_of(&f, Table::Orders, &order);
    end(&f, sell_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_preview_prices_the_discount_after_the_coupon_without_writing() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    coupon(&f, "TEN", 1_000).await;
    let (order, _, _) = billed(&f, 10_000, 41).await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let before = changes(&f);
    let store = reopen(&f);
    let preview = |req: DiscountPreviewRequest| {
        store.read_with(|conn| crate::apply::discount_preview(conn, &req, f.clock.now_ms()))
    };
    let base = |discount: ManualDiscount, coupon: Option<&str>| DiscountPreviewRequest {
        discount,
        coupon: coupon.map(Into::into),
        order_id: None,
        lines: None,
        plan_id: None,
    };
    let cart = preview(DiscountPreviewRequest {
        lines: Some(vec![PosSaleLine {
            product_id: cola.clone(),
            qty: 2,
        }]),
        ..base(percent(1_000), Some("ten"))
    })
    .unwrap();
    assert_eq!((cart.discount_minor, cart.total_minor), (720, 6_480));
    let bill = preview(DiscountPreviewRequest {
        order_id: Some(order),
        ..base(amount(500), None)
    })
    .unwrap();
    assert_eq!((bill.discount_minor, bill.total_minor), (500, 7_000));
    let pack = preview(DiscountPreviewRequest {
        plan_id: Some(plan.clone()),
        ..base(percent(2_000), None)
    })
    .unwrap();
    assert_eq!((pack.discount_minor, pack.total_minor), (6_000, 24_000));
    let over = preview(DiscountPreviewRequest {
        lines: Some(vec![PosSaleLine {
            product_id: cola,
            qty: 1,
        }]),
        ..base(amount(5_000), None)
    })
    .unwrap_err();
    assert_eq!(
        (over.code, over.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("discount.amount_minor"))
    );
    let none = preview(base(percent(500), None)).unwrap_err();
    assert_eq!(none.code, ErrorCode::ValidationFailed);
    let bad_coupon = preview(DiscountPreviewRequest {
        plan_id: Some(plan),
        ..base(percent(500), Some("NOPE"))
    })
    .unwrap_err();
    assert_eq!(bad_coupon.code, ErrorCode::CouponInvalid);
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn the_receipt_source_carries_the_discount_and_prints_it_after_the_coupon() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    coupon(&f, "TEN", 1_000).await;
    let a = sell_as(
        &f,
        owner(),
        sale(
            &[(&cola, 1)],
            vec![pay(Cash, 3_240, None)],
            Some("TEN"),
            Some(percent(1_000)),
        ),
        None,
    )
    .await
    .unwrap();
    let order = a.changes[0].row_id.clone();
    let source = reopen(&f)
        .read_with(|conn| arena_store::load_receipt_source(conn, &order))
        .unwrap()
        .unwrap();
    assert_eq!(source.head.discount, Some(360));
    assert_eq!(source.head.total_minor, 3_240);
    let bytes = crate::snapshot::SnapshotReader::open(&f.path)
        .unwrap()
        .receipt_bytes(&order, true, f.clock.now_ms())
        .unwrap()
        .unwrap();
    let printed = String::from_utf8_lossy(&bytes);
    let line = printed
        .lines()
        .find(|l| l.starts_with("Discount"))
        .expect("the receipt prints the discount");
    assert!(line.ends_with("-3.60"), "{line}");
    assert!(printed.find("Coupon TEN") < printed.find("Discount"));
    assert!(printed.find("Discount") < printed.find("TOTAL"));
}

#[tokio::test]
async fn stock_is_taken_for_the_full_quantity_whatever_the_discount() {
    let f = bootstrapped().await;
    let cola = product(&f, "Cola", 4_000, None).await;
    sell_as(
        &f,
        owner(),
        sale(
            &[(&cola, 3)],
            vec![pay(Cash, 6_000, None)],
            None,
            Some(percent(5_000)),
        ),
        None,
    )
    .await
    .unwrap();
    let stock = reopen(&f).rows(Table::Stock).unwrap();
    assert_eq!(int(&stock[0], "qty"), 17);
}

/// `i` as `counter`, approved by `manager` when one is named.
fn acting(mut i: Intent, counter: &Actor, manager: Option<&str>) -> Intent {
    i.actor = counter.clone();
    i.elevated_by = manager.map(|id| ElevatedBy {
        kind: ElevationKind::Pin,
        id: id.into(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i
}

#[tokio::test]
async fn a_session_settle_and_a_plan_sale_discount_need_the_approving_managers_permission() {
    let f = bootstrapped().await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;

    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let settle = |paid: i64| {
        let mut i = settle_intent(&order, Some(rev), vec![pay(Cash, paid, None)]);
        if let IntentPayload::SessionSettle(s) = &mut i.payload {
            s.discount = Some(percent(1_000));
        }
        i
    };
    let before = changes(&f);
    assert_eq!(
        refusal(end(&f, acting(settle(6_750), &counter, None)).await).0,
        ErrorCode::PermissionDenied
    );
    // A staff member without `discount.manual` cannot approve it either.
    assert_eq!(
        refusal(end(&f, acting(settle(6_750), &counter, Some(&counter.id))).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);
    end(&f, acting(settle(6_750), &counter, Some(&manager)))
        .await
        .unwrap();
    assert_eq!(
        text(&table(&f, Table::Orders)[0], "discount_by"),
        manager.clone()
    );

    let p = super::session::player(&f, "asha", "+919876543210").await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let sell = || {
        let mut i = sell_intent(&p, &plan, vec![pay(Cash, 25_000, None)]);
        if let IntentPayload::PlanSell(s) = &mut i.payload {
            s.discount = Some(amount(5_000));
        }
        i
    };
    let before = changes(&f);
    assert_eq!(
        refusal(end(&f, acting(sell(), &counter, None)).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);
    end(&f, acting(sell(), &counter, Some(&manager)))
        .await
        .unwrap();
    assert_eq!(text(&sale_order(&f), "discount_by"), manager);
}

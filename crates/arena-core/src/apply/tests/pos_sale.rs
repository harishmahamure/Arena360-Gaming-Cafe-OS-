use super::prepaid::sql;
use super::session::{
    credit_player, int, owed, pay, player, record_effect, refusal, rev_of, submit, table, text,
    topped_up,
};
use super::stock::{
    held, intent, lots_of, low_alerts, make, manager_with_pin, owner, put, set_threshold, sin,
    staff, stock_in, up,
};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::{Cash, Credit, Upi, Wallet};
use arena_proto::{
    BundleComponent, PaymentRow, PosSale, PosSaleLine, PosSaleReverted, ProductUpsert,
};

fn sale_of(lines: &[(&str, i64)], payments: Vec<PaymentRow>) -> PosSale {
    PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: lines
            .iter()
            .map(|(id, qty)| PosSaleLine {
                product_id: (*id).into(),
                qty: *qty,
                kitchen_notes: Vec::new(),
            })
            .collect(),
        payments,
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    }
}

async fn sell(f: &Fixture, actor: Actor, s: PosSale) -> Result<Applied, AppError> {
    submit(f, actor, IntentPayload::PosSale(s)).await
}

fn revert_intent(actor: Actor, order: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.into(),
        }),
    );
    i.base_rev = rev;
    i
}

async fn revert(f: &Fixture, actor: Actor, order: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Orders, order);
    let mut i = revert_intent(actor, order, Some(rev));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn pos_orders(f: &Fixture) -> Vec<Row> {
    table(f, Table::Orders)
        .into_iter()
        .filter(|o| text(o, "kind") == "pos_sale")
        .collect()
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn priced(name: &str, price: i64, track: bool) -> ProductUpsert {
    ProductUpsert {
        price_minor: price,
        track_stock: track,
        ..up(name)
    }
}

async fn cola(f: &Fixture, price: i64, qty: i64) -> String {
    let id = make(f, priced("Cola", price, true)).await;
    if qty > 0 {
        stock_in(f, &id, qty, 2_000).await;
    }
    id
}

async fn combo(f: &Fixture, chips: &str, per: i64) -> String {
    make(
        f,
        ProductUpsert {
            bundle: true,
            track_stock: false,
            components: vec![BundleComponent {
                product_id: chips.into(),
                qty: per,
            }],
            ..priced("Combo", 5_000, false)
        },
    )
    .await
}

fn moves_of(f: &Fixture, id: &str, reason: &str) -> Vec<arena_store::MoveRow> {
    reopen(f)
        .moves(id)
        .unwrap()
        .into_iter()
        .filter(|m| m.reason == reason)
        .collect()
}

async fn set_tax(f: &Fixture, json: &str) {
    sql(
        f,
        format!("UPDATE settings SET value = '{json}' WHERE key = 'branch.tax'"),
    )
    .await;
}

fn kinds(a: &Applied) -> Vec<&str> {
    a.changes.iter().map(|c| c.tbl.as_str()).collect()
}

#[tokio::test]
async fn a_cash_walk_up_sale_settles_writes_the_cash_event_and_takes_stock_in_lot_order() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    let mut early = sin(&id, 1, 2_000);
    early.expiry = Some("2027-03-01".into());
    put(&f, owner(), early).await.unwrap();
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    let a = sell(
        &f,
        counter.clone(),
        sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)]),
    )
    .await
    .unwrap();
    assert_eq!(
        kinds(&a),
        [
            "orders",
            "order_lines",
            "payments",
            "cash_events",
            "stock",
            "stock_lots",
            "stock_lots",
            "stock_moves"
        ]
    );
    let orders = pos_orders(&f);
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor"),
            int(o, "tax_minor"),
            text(o, "settled_by"),
            int(o, "settled_at"),
        ),
        (
            "settled".to_owned(),
            8_000,
            8_000,
            0,
            counter.id.clone(),
            f.clock.now_ms()
        )
    );
    assert_eq!(o["session_id"], RowValue::Null);
    assert_eq!(o["player_id"], RowValue::Null);
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(
        (
            text(line, "order_id"),
            text(line, "product_id"),
            text(line, "label"),
            int(line, "qty"),
            int(line, "unit_price_minor"),
            int(line, "amount_minor"),
            int(line, "tax_rate_bp"),
        ),
        (
            text(o, "id"),
            id.clone(),
            "Cola".to_owned(),
            2,
            4_000,
            8_000,
            0
        )
    );
    let payments = table(&f, Table::Payments);
    assert_eq!(payments.len(), 1);
    assert_eq!(
        (
            text(&payments[0], "method"),
            int(&payments[0], "amount_minor")
        ),
        ("cash".to_owned(), 8_000)
    );
    let events = table(&f, Table::CashEvents);
    assert_eq!(events.len(), 1);
    assert_eq!(
        (
            text(&events[0], "kind"),
            int(&events[0], "amount_minor"),
            text(&events[0], "order_id"),
            text(&events[0], "staff_id"),
        ),
        ("sale".to_owned(), 8_000, text(o, "id"), counter.id.clone())
    );
    // Stock went down by 2, from the earliest-expiry lot first.
    assert_eq!(held(&f, &id).0, 4);
    assert_eq!(
        lots_of(&f, &id),
        [(None, 4), (Some("2027-03-01".into()), 0)]
    );
    let sale_moves = moves_of(&f, &id, "sale");
    assert_eq!(sale_moves.len(), 1);
    let m = &sale_moves[0];
    assert_eq!(
        (
            m.delta,
            m.before_qty,
            m.order_id.clone(),
            m.staff_id.clone()
        ),
        (-2, 6, Some(text(o, "id")), counter.id)
    );
    assert_eq!(m.value_minor, 4_000);
    assert!(
        crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms())
            .unwrap()
            .bills
            .is_empty()
    );
    let s = reopen(&f).settlement(&text(o, "id")).unwrap().unwrap();
    assert_eq!((s.total_minor, s.irreversible), (8_000, false));
}

#[tokio::test]
async fn a_mixed_sale_writes_one_cash_event_for_the_cash_row_only() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    sell(
        &f,
        owner(),
        sale_of(
            &[(&id, 2)],
            vec![pay(Cash, 3_000, None), pay(Upi, 5_000, Some("1234"))],
        ),
    )
    .await
    .unwrap();
    assert_eq!(table(&f, Table::Payments).len(), 2);
    let events = table(&f, Table::CashEvents);
    assert_eq!(events.len(), 1);
    assert_eq!(int(&events[0], "amount_minor"), 3_000);
    // No cash at all: no cash event.
    sell(
        &f,
        owner(),
        sale_of(&[(&id, 1)], vec![pay(Upi, 4_000, Some("4321"))]),
    )
    .await
    .unwrap();
    assert_eq!(table(&f, Table::CashEvents).len(), 1);
}

#[tokio::test]
async fn cash_without_an_open_shift_is_refused_and_writes_nothing() {
    let f = bootstrapped_closed().await;
    let id = cola(&f, 4_000, 5).await;
    let before = changes(&f);
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)])
            )
            .await
        ),
        (ErrorCode::ShiftNotOpen, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);
    assert!(pos_orders(&f).is_empty());
    assert_eq!(held(&f, &id).0, 5);
    // UPI needs no shift.
    sell(
        &f,
        owner(),
        sale_of(&[(&id, 2)], vec![pay(Upi, 8_000, Some("1234"))]),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &id).0, 3);
}

#[tokio::test]
async fn a_plain_product_short_of_stock_is_out_of_stock_even_with_an_override() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 1).await;
    let before = changes(&f);
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)])
            )
            .await
        ),
        (ErrorCode::OutOfStock, Some("lines[0]".into()))
    );
    let mut over = sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)]);
    over.stock_override = Some(true);
    assert_eq!(
        refusal(sell(&f, owner(), over).await).0,
        ErrorCode::OutOfStock
    );
    assert_eq!(changes(&f), before);
    assert_eq!(held(&f, &id).0, 1);
    // A product never stocked is out of stock too.
    let none = cola(&f, 100, 0).await;
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&none, 1)], vec![pay(Cash, 100, None)])
            )
            .await
        )
        .0,
        ErrorCode::OutOfStock
    );
}

#[tokio::test]
async fn lines_of_one_product_count_together_with_a_bundle_that_uses_it() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 3, 500).await;
    let bundle = combo(&f, &chips, 2).await;
    // 2 in the combo + 2 plain = 4 chips wanted, 3 on hand: the combo is the short one.
    let both = sale_of(&[(&bundle, 1), (&chips, 2)], vec![pay(Cash, 7_000, None)]);
    let e = sell(&f, owner(), both).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BundleShort);
    assert!(e.message.contains("Chips"), "{}", e.message);
    // 2 + 1 = 3 fits exactly.
    sell(
        &f,
        owner(),
        sale_of(&[(&bundle, 1), (&chips, 1)], vec![pay(Cash, 6_000, None)]),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &chips).0, 0);
    assert_eq!(
        moves_of(&f, &chips, "sale").len(),
        1,
        "one move per product"
    );
}

#[tokio::test]
async fn a_short_bundle_names_its_component_and_a_manager_override_raises_it() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 1, 500).await;
    let bundle = combo(&f, &chips, 2).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    let pay_combo = || vec![pay(Cash, 5_000, None)];

    let before = changes(&f);
    let e = sell(&f, counter.clone(), sale_of(&[(&bundle, 1)], pay_combo()))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::BundleShort, Some("lines[0]"))
    );
    assert!(e.message.contains("Chips"), "{}", e.message);
    assert_eq!(changes(&f), before);

    // Without the right, an override is denied (and nothing is written).
    let mut over = sale_of(&[(&bundle, 1)], pay_combo());
    over.stock_override = Some(true);
    assert_eq!(
        refusal(sell(&f, counter.clone(), over.clone()).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);

    // With the manager's PIN it sells: Chips is raised by the missing unit first.
    let mut approved = intent(counter.clone(), IntentPayload::PosSale(over));
    approved.issued_at = f.clock.now_ms();
    approved.elevated_by = Some(ElevatedBy {
        kind: ElevationKind::Pin,
        id: manager.clone(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    let a = f.writer.submit_intent(approved).await.unwrap();
    assert_eq!(held(&f, &chips).0, 0);
    let adjust = moves_of(&f, &chips, "count_adjust");
    assert_eq!(adjust.len(), 1);
    let o = &pos_orders(&f)[0];
    assert_eq!(
        (
            adjust[0].delta,
            adjust[0].note.clone(),
            adjust[0].elevated_by.clone(),
            adjust[0].order_id.clone(),
            adjust[0].staff_id.clone(),
        ),
        (
            1,
            Some("Override at sale".to_owned()),
            Some(manager.clone()),
            Some(text(o, "id")),
            counter.id.clone()
        )
    );
    let sold = moves_of(&f, &chips, "sale");
    assert_eq!((sold.len(), sold[0].delta, sold[0].before_qty), (1, -2, 2));
    // The order has the bundle as one line, at the bundle's own price.
    let line = &table(&f, Table::OrderLines)[0];
    assert_eq!(
        (
            text(line, "label"),
            int(line, "qty"),
            int(line, "amount_minor")
        ),
        ("Combo".to_owned(), 1, 5_000)
    );
    assert_eq!(table(&f, Table::OrderLines).len(), 1);
    let first = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.intent_id == a.intent_id)
        .unwrap();
    assert_eq!(first.actor_id, counter.id);
    assert_eq!(first.elevated_by_id.as_deref(), Some(manager.as_str()));
    // The stock never went below zero: the lots add up to what is on hand.
    let lots: i64 = lots_of(&f, &chips).iter().map(|(_, q)| q).sum();
    assert_eq!(lots, 0);
}

#[tokio::test]
async fn an_owner_may_override_and_an_override_without_a_shortage_changes_nothing_else() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 5, 500).await;
    let bundle = combo(&f, &chips, 2).await;
    let mut s = sale_of(&[(&bundle, 1)], vec![pay(Cash, 5_000, None)]);
    s.stock_override = Some(true);
    sell(&f, owner(), s).await.unwrap();
    assert_eq!(held(&f, &chips).0, 3);
    assert!(moves_of(&f, &chips, "count_adjust").is_empty());
}

#[tokio::test]
async fn expired_lots_are_never_sold() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 0).await;
    let mut lot = sin(&id, 3, 2_000);
    lot.expiry = Some("2027-01-15".into());
    put(&f, owner(), lot).await.unwrap();
    sql(&f, "UPDATE stock_lots SET expiry = '2027-01-14'".into()).await;
    let before = changes(&f);
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)])
            )
            .await
        )
        .0,
        ErrorCode::OutOfStock
    );
    assert_eq!(changes(&f), before);
    // Fresh stock arrives: the sale takes from it and leaves the expired lot alone.
    stock_in(&f, &id, 2, 2_000).await;
    sell(
        &f,
        owner(),
        sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)]),
    )
    .await
    .unwrap();
    assert_eq!(
        lots_of(&f, &id),
        [(None, 0), (Some("2027-01-14".into()), 3)]
    );
    assert_eq!(held(&f, &id).0, 3);
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)])
            )
            .await
        )
        .0,
        ErrorCode::OutOfStock
    );
}

#[tokio::test]
async fn an_expired_bundle_component_is_short_and_an_override_sells_from_a_new_lot() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    let mut lot = sin(&chips, 2, 500);
    lot.expiry = Some("2027-01-15".into());
    put(&f, owner(), lot).await.unwrap();
    sql(&f, "UPDATE stock_lots SET expiry = '2027-01-14'".into()).await;
    let bundle = combo(&f, &chips, 2).await;
    let mut s = sale_of(&[(&bundle, 1)], vec![pay(Cash, 5_000, None)]);
    assert_eq!(
        refusal(sell(&f, owner(), s.clone()).await).0,
        ErrorCode::BundleShort
    );
    s.stock_override = Some(true);
    sell(&f, owner(), s).await.unwrap();
    // The expired lot is untouched; the raise lot was used up.
    assert_eq!(
        lots_of(&f, &chips),
        [(None, 0), (Some("2027-01-14".into()), 2)]
    );
    assert_eq!(held(&f, &chips).0, 2);
    assert_eq!(moves_of(&f, &chips, "count_adjust")[0].delta, 2);
}

#[tokio::test]
async fn an_untracked_product_moves_no_stock() {
    let f = bootstrapped().await;
    let tea = make(&f, priced("Tea", 1_500, false)).await;
    sell(
        &f,
        owner(),
        sale_of(&[(&tea, 3)], vec![pay(Cash, 4_500, None)]),
    )
    .await
    .unwrap();
    assert!(reopen(&f).moves(&tea).unwrap().is_empty());
    assert!(table(&f, Table::Stock).is_empty());
    assert_eq!(table(&f, Table::OrderLines).len(), 1);
}

#[tokio::test]
async fn an_exclusive_tax_adds_to_the_total_and_the_payments_must_match() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    set_tax(
        &f,
        r#"{"country":"IN","gst":false,"gstin":"","inclusive":false,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":500}"#,
    )
    .await;
    let before = changes(&f);
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&id, 2)], vec![pay(Cash, 8_000, None)])
            )
            .await
        ),
        (ErrorCode::PaymentShort, Some("payments".into()))
    );
    assert_eq!(changes(&f), before);
    sell(
        &f,
        owner(),
        sale_of(&[(&id, 2)], vec![pay(Cash, 8_400, None)]),
    )
    .await
    .unwrap();
    let o = &pos_orders(&f)[0];
    assert_eq!(
        (
            int(o, "subtotal_minor"),
            int(o, "tax_minor"),
            int(o, "total_minor")
        ),
        (8_000, 400, 8_400)
    );
    assert_eq!(
        text(o, "tax_breakup"),
        r#"[{"rate_bp":500,"taxable_minor":8000,"tax_minor":400}]"#
    );
    // The simple rate is the branch's, whatever the product carries.
    assert_eq!(int(&table(&f, Table::OrderLines)[0], "tax_rate_bp"), 500);
    assert_eq!(table(&f, Table::CashEvents).len(), 1);
    assert_eq!(int(&table(&f, Table::CashEvents)[0], "amount_minor"), 8_400);
}

#[tokio::test]
async fn an_inclusive_simple_tax_is_carved_out_of_the_price_and_gst_uses_each_products_rate() {
    let f = bootstrapped().await;
    let a = make(
        &f,
        ProductUpsert {
            tax_rate_bp: Some(1_800),
            ..priced("Cola", 1_180, true)
        },
    )
    .await;
    let b = make(
        &f,
        ProductUpsert {
            tax_rate_bp: Some(500),
            ..priced("Chips", 105, true)
        },
    )
    .await;
    stock_in(&f, &a, 5, 100).await;
    stock_in(&f, &b, 5, 100).await;
    // The default branch tax is simple, inclusive, 0%: the products' own rates are ignored.
    sell(
        &f,
        owner(),
        sale_of(&[(&a, 1), (&b, 1)], vec![pay(Cash, 1_285, None)]),
    )
    .await
    .unwrap();
    let o = &pos_orders(&f)[0];
    assert_eq!((int(o, "tax_minor"), int(o, "total_minor")), (0, 1_285));

    set_tax(
        &f,
        r#"{"country":"IN","gst":true,"gstin":"27AAPFU0939F1ZV","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#,
    )
    .await;
    sell(
        &f,
        owner(),
        sale_of(&[(&a, 1), (&b, 1)], vec![pay(Cash, 1_285, None)]),
    )
    .await
    .unwrap();
    let gst = pos_orders(&f)
        .into_iter()
        .find(|o| int(o, "tax_minor") != 0)
        .unwrap();
    assert_eq!(
        (
            int(&gst, "subtotal_minor"),
            int(&gst, "tax_minor"),
            int(&gst, "total_minor")
        ),
        (1_100, 185, 1_285)
    );
    let breakup: serde_json::Value = serde_json::from_str(&text(&gst, "tax_breakup")).unwrap();
    assert_eq!(breakup.as_array().unwrap().len(), 2);
    assert_eq!(breakup[1]["rate_bp"], 1_800);
    assert_eq!(breakup[1]["cgst_minor"], 90);
}

#[tokio::test]
async fn undo_inside_the_window_puts_everything_back() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    let mut early = sin(&id, 1, 2_000);
    early.expiry = Some("2027-03-01".into());
    put(&f, owner(), early).await.unwrap();
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let lots_before = lots_of(&f, &id);
    let moves_before = reopen(&f).moves(&id).unwrap().len();
    let a = sell(
        &f,
        counter.clone(),
        sale_of(&[(&id, 4)], vec![pay(Cash, 16_000, None)]),
    )
    .await
    .unwrap();
    let order = a.changes[0].row_id.clone();
    assert_eq!(held(&f, &id).0, 2);

    f.clock.advance(7_000);
    let undone = revert(&f, counter.clone(), &order).await.unwrap();
    assert!(kinds(&undone).contains(&"orders"));
    // Stock and every lot are back, and the move log keeps both moves.
    assert_eq!(held(&f, &id).0, 6);
    assert_eq!(lots_of(&f, &id), lots_before);
    let moves = reopen(&f).moves(&id).unwrap();
    assert_eq!(moves.len(), moves_before + 2);
    let back = moves_of(&f, &id, "sale_reverted");
    assert_eq!(back.len(), 1);
    assert_eq!(
        (back[0].delta, back[0].before_qty, back[0].order_id.clone()),
        (4, 2, Some(order.clone()))
    );
    // Cash: the sale row stays and a compensating row cancels it.
    let events = table(&f, Table::CashEvents);
    assert_eq!(events.len(), 2);
    let (sale, reverted) = (
        events.iter().find(|e| text(e, "kind") == "sale").unwrap(),
        events
            .iter()
            .find(|e| text(e, "kind") == "sale_reverted")
            .unwrap(),
    );
    assert_eq!(int(reverted, "amount_minor"), -16_000);
    assert_eq!(text(reverted, "reverses"), text(sale, "id"));
    // The order, its lines and payments are gone.
    assert!(pos_orders(&f).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert!(table(&f, Table::Payments).is_empty());
}

#[tokio::test]
async fn undo_is_refused_late_by_another_staff_member_or_after_an_effect() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 20).await;
    let asha = staff(&f, "asha", ROLE_COUNTER).await;
    let ravi = staff(&f, "ravi", ROLE_COUNTER).await;
    let one = || sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)]);
    let denied = |r: Result<Applied, AppError>| refusal(r).0;

    let late = sell(&f, asha.clone(), one()).await.unwrap().changes[0]
        .row_id
        .clone();
    f.clock.advance(9_000);
    assert_eq!(
        denied(revert(&f, asha.clone(), &late).await),
        ErrorCode::UndoNotAllowed
    );

    let other = sell(&f, asha.clone(), one()).await.unwrap().changes[0]
        .row_id
        .clone();
    assert_eq!(
        denied(revert(&f, ravi.clone(), &other).await),
        ErrorCode::UndoNotAllowed
    );

    let printed = sell(&f, asha.clone(), one()).await.unwrap().changes[0]
        .row_id
        .clone();
    record_effect(&f, arena_proto::EffectKind::ReceiptPrinted, "", &printed).await;
    assert_eq!(
        denied(revert(&f, asha.clone(), &printed).await),
        ErrorCode::UndoNotAllowed
    );

    // Nothing was undone: three orders, 17 on hand, no compensating rows.
    assert_eq!(pos_orders(&f).len(), 3);
    assert_eq!(held(&f, &id).0, 17);
    assert!(moves_of(&f, &id, "sale_reverted").is_empty());
    assert!(
        table(&f, Table::CashEvents)
            .iter()
            .all(|e| text(e, "kind") != "sale_reverted")
    );
    // A stale base_rev and a missing one are refused too, and so is the wrong kind of bill.
    let fresh = sell(&f, asha.clone(), one()).await.unwrap().changes[0]
        .row_id
        .clone();
    let mut stale = revert_intent(asha.clone(), &fresh, Some(99));
    stale.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(stale).await).0,
        ErrorCode::Stale
    );
    let mut bare = revert_intent(asha.clone(), &fresh, None);
    bare.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(bare).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    let mut gone = revert_intent(asha, "nope", Some(0));
    gone.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(gone).await).0,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn an_undone_override_keeps_the_stock_correction() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 1, 500).await;
    let bundle = combo(&f, &chips, 2).await;
    let mut s = sale_of(&[(&bundle, 1)], vec![pay(Cash, 5_000, None)]);
    s.stock_override = Some(true);
    let order = sell(&f, owner(), s).await.unwrap().changes[0]
        .row_id
        .clone();
    assert_eq!(held(&f, &chips).0, 0);
    revert(&f, owner(), &order).await.unwrap();
    // The raise stays: 1 counted + 1 raised, and every lot is full again.
    assert_eq!(held(&f, &chips).0, 2);
    assert_eq!(lots_of(&f, &chips), [(None, 1), (None, 1)]);
    assert_eq!(moves_of(&f, &chips, "count_adjust").len(), 1);
}

#[tokio::test]
async fn an_undo_is_refused_when_a_lot_it_drew_from_changed() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 3).await;
    let order = sell(
        &f,
        owner(),
        sale_of(&[(&id, 3)], vec![pay(Cash, 12_000, None)]),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    // A lot that grew behind the sale's back has no room to take the units back.
    sql(
        &f,
        "UPDATE stock_lots SET qty_remaining = qty_in".to_owned(),
    )
    .await;
    assert_eq!(
        refusal(revert(&f, owner(), &order).await).0,
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(pos_orders(&f).len(), 1);
}

#[tokio::test]
async fn a_player_pays_with_wallet_and_credit_and_undo_restores_both() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 10).await;
    let p = credit_player(&f, "asha", "+919876543210", 50_000).await;
    topped_up(&f, &p, 6_000).await;
    let mut s = sale_of(
        &[(&id, 5)],
        vec![
            pay(Wallet, 6_000, None),
            pay(Credit, 10_000, None),
            pay(Cash, 4_000, None),
        ],
    );
    s.player_id = Some(p.clone());
    let a = sell(&f, owner(), s).await.unwrap();
    assert!(kinds(&a).contains(&"wallet_txns"));
    let o = pos_orders(&f).pop().unwrap();
    assert_eq!(text(&o, "player_id"), p);
    assert_eq!(owed(&f, &p), 10_000);
    assert_eq!(super::session::balance(&f, &p), 0);
    revert(&f, owner(), &text(&o, "id")).await.unwrap();
    assert_eq!(owed(&f, &p), 0);
    assert_eq!(super::session::balance(&f, &p), 6_000);
    assert_eq!(held(&f, &id).0, 10);
}

#[tokio::test]
async fn wallet_and_credit_need_an_attached_player_and_a_missing_one_is_refused() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 10).await;
    let p = player(&f, "ravi", "+919876543211").await;
    let before = changes(&f);
    let guest = sale_of(&[(&id, 1)], vec![pay(Wallet, 4_000, None)]);
    assert_eq!(
        refusal(sell(&f, owner(), guest).await),
        (
            ErrorCode::ValidationFailed,
            Some("payments[0].method".into())
        )
    );
    let mut ghost = sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)]);
    ghost.player_id = Some("nobody".into());
    assert_eq!(
        refusal(sell(&f, owner(), ghost).await),
        (ErrorCode::NotFound, Some("player_id".into()))
    );
    let mut poor = sale_of(&[(&id, 1)], vec![pay(Wallet, 4_000, None)]);
    poor.player_id = Some(p);
    assert_eq!(
        refusal(sell(&f, owner(), poor).await).0,
        ErrorCode::InsufficientBalance
    );
    assert_eq!(changes(&f), before);
    assert!(pos_orders(&f).is_empty());
}

#[tokio::test]
async fn the_lines_are_checked() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    let off = make(
        &f,
        ProductUpsert {
            active: false,
            ..priced("Old", 100, false)
        },
    )
    .await;
    let gone = make(&f, priced("Gone", 100, false)).await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductDelete(arena_proto::ProductDelete { id: gone.clone() }),
        ))
        .await
        .unwrap();
    let many: Vec<(String, i64)> = (0..101).map(|i| (format!("p{i}"), 1)).collect();
    let many_refs: Vec<(&str, i64)> = many.iter().map(|(s, q)| (s.as_str(), *q)).collect();
    let field = |f: &str| (ErrorCode::ValidationFailed, Some(f.to_owned()));
    type Case<'a> = (Vec<(&'a str, i64)>, (ErrorCode, Option<String>));
    let cases: Vec<Case> = vec![
        (vec![], field("lines")),
        (many_refs, field("lines")),
        (vec![(&id, 0)], field("lines[0]")),
        (vec![(&id, 1_000)], field("lines[0]")),
        (vec![(&id, -1)], field("lines[0]")),
        (vec![(&id, 1), (&id, 2)], field("lines[1]")),
        (vec![(&id, 1), (&off, 1)], field("lines[1]")),
        (vec![("nope", 1)], field("lines[0]")),
        (vec![(&gone, 1)], field("lines[0]")),
    ];
    let before = changes(&f);
    for (lines, want) in cases {
        let s = sale_of(&lines, vec![pay(Cash, 100, None)]);
        assert_eq!(refusal(sell(&f, owner(), s).await), want, "{lines:?}");
    }
    assert_eq!(changes(&f), before);
    // A quantity of 999 is fine when there is stock.
    let stocked = cola(&f, 1, 999).await;
    sell(
        &f,
        owner(),
        sale_of(&[(&stocked, 999)], vec![pay(Cash, 999, None)]),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_bundle_with_an_inactive_component_cannot_be_sold() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 5, 500).await;
    let bundle = combo(&f, &chips, 1).await;
    f.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductUpsert(ProductUpsert {
                id: Some(chips.clone()),
                active: false,
                ..priced("Chips", 1_000, true)
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        refusal(
            sell(
                &f,
                owner(),
                sale_of(&[(&bundle, 1)], vec![pay(Cash, 5_000, None)])
            )
            .await
        ),
        (ErrorCode::ValidationFailed, Some("lines[0]".into()))
    );
}

#[tokio::test]
async fn the_payments_must_add_up_and_a_sale_takes_no_base_rev() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    let before = changes(&f);
    for bad in [
        vec![],
        vec![pay(Cash, 7_999, None)],
        vec![pay(Cash, 8_001, None)],
    ] {
        assert_eq!(
            refusal(sell(&f, owner(), sale_of(&[(&id, 2)], bad)).await),
            (ErrorCode::PaymentShort, Some("payments".into()))
        );
    }
    let mut with_rev = intent(
        owner(),
        IntentPayload::PosSale(sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)])),
    );
    with_rev.base_rev = Some(0);
    assert_eq!(
        refusal(f.writer.submit_intent(with_rev).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn kitchen_staff_may_not_sell_and_a_counter_may() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    let cook = staff(&f, "cook", crate::rbac::ROLE_KITCHEN).await;
    assert_eq!(
        refusal(sell(&f, cook, sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)])).await).0,
        ErrorCode::PermissionDenied
    );
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    sell(
        &f,
        counter,
        sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)]),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_sale_and_its_undo_raise_and_clear_the_low_stock_alert() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 5).await;
    set_threshold(&f, owner(), &id, Some(2)).await.unwrap();
    assert!(low_alerts(&f).is_empty());
    let order = sell(
        &f,
        owner(),
        sale_of(&[(&id, 3)], vec![pay(Cash, 12_000, None)]),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    assert_eq!(low_alerts(&f).len(), 1);
    revert(&f, owner(), &order).await.unwrap();
    assert!(low_alerts(&f).is_empty());
}

#[tokio::test]
async fn recent_sales_list_the_business_days_counter_sales_newest_first() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 20).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let first = sell(
        &f,
        counter,
        sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)]),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    f.clock.advance(1_000);
    let p = player(&f, "ravi", "+919876543211").await;
    let mut second = sale_of(
        &[(&id, 2)],
        vec![pay(Cash, 5_000, None), pay(Upi, 3_000, Some("1234"))],
    );
    second.player_id = Some(p);
    let second = sell(&f, owner(), second).await.unwrap().changes[0]
        .row_id
        .clone();
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let view = reader.pos_sales(f.clock.now_ms()).unwrap();
    assert_eq!(view.business_day, "2027-01-15");
    assert_eq!(
        view.sales
            .iter()
            .map(|s| s.order_id.as_str())
            .collect::<Vec<_>>(),
        [second.as_str(), first.as_str()]
    );
    let top = &view.sales[0];
    assert_eq!(
        (
            top.player.as_deref(),
            top.total_minor,
            top.tax_minor,
            top.invoice_no.clone(),
            top.lines.len(),
            top.payments.len()
        ),
        (Some("ravi"), 8_000, 0, None, 1, 2)
    );
    assert_eq!(
        (
            top.lines[0].name.as_str(),
            top.lines[0].qty,
            top.lines[0].unit_price_minor,
            top.lines[0].amount_minor
        ),
        ("Cola", 2, 4_000, 8_000)
    );
    assert_eq!(view.sales[1].staff, "asha");
    // An undone sale is not listed; neither is a plan sale or a top-up.
    revert(&f, owner(), &second).await.unwrap();
    assert_eq!(reader.pos_sales(f.clock.now_ms()).unwrap().sales.len(), 1);
}

#[tokio::test]
async fn a_plain_line_sharing_a_product_with_a_bundle_is_never_raised_by_an_override() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 1, 500).await;
    let bundle = combo(&f, &chips, 1).await;
    // The plain line alone asks for 2 chips of 1: out of stock, whatever the order of lines.
    let mut s = sale_of(&[(&chips, 2), (&bundle, 1)], vec![pay(Cash, 7_000, None)]);
    s.stock_override = Some(true);
    let before = changes(&f);
    assert_eq!(refusal(sell(&f, owner(), s).await).0, ErrorCode::OutOfStock);
    assert_eq!(changes(&f), before);
    assert_eq!(held(&f, &chips).0, 1);
}

#[tokio::test]
async fn a_credit_override_at_the_counter_needs_credit_manage() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 10).await;
    let p = credit_player(&f, "asha", "+919876543210", 5_000).await;
    let counter = staff(&f, "asha2", ROLE_COUNTER).await;
    let mut s = sale_of(&[(&id, 3)], vec![pay(Credit, 12_000, None)]);
    s.player_id = Some(p);
    s.credit_override = Some(true);
    let before = changes(&f);
    assert_eq!(
        refusal(sell(&f, counter, s.clone()).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);
    sell(&f, owner(), s).await.unwrap();
}

/// Story 5.27: Daily sales reads the stored totals; an undone sale nets to zero.
#[tokio::test]
async fn daily_sales_count_settled_counter_sales_and_net_an_undone_one_to_zero() {
    let f = bootstrapped().await;
    let id = cola(&f, 4_000, 20).await;
    let first = sell(
        &f,
        owner(),
        sale_of(&[(&id, 1)], vec![pay(Cash, 4_000, None)]),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    f.clock.advance(1_000);
    let second = sell(
        &f,
        owner(),
        sale_of(&[(&id, 2)], vec![pay(Upi, 8_000, Some("1234"))]),
    )
    .await
    .unwrap()
    .changes[0]
        .row_id
        .clone();
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let day = "2027-01-15";
    let now = f.clock.now_ms();
    let r = reader.daily_sales(day, day, now).unwrap();
    assert_eq!((r.orders, r.sales_minor, r.net_minor), (2, 12_000, 12_000));
    let of = |k: &str| r.by_type.iter().find(|t| t.key == k).unwrap().amount_minor;
    assert_eq!(
        (of("products"), of("play_time"), of("plans")),
        (12_000, 0, 0)
    );
    assert_eq!(
        r.by_type.iter().map(|t| t.amount_minor).sum::<i64>(),
        12_000
    );
    assert_eq!(
        r.by_hour.iter().map(|h| h.amount_minor).sum::<i64>(),
        12_000
    );
    assert_eq!(r.by_category.len(), 1);
    assert_eq!(r.by_category[0].amount_minor, 12_000);
    assert_eq!(r.by_category[0].count, 3);
    assert_eq!(r.by_group.len(), 1, "counter sales have no group");
    assert_eq!(r.by_group[0].id, "");

    revert(&f, owner(), &second).await.unwrap();
    let r = reader.daily_sales(day, day, f.clock.now_ms()).unwrap();
    assert_eq!((r.orders, r.sales_minor), (1, 4_000));
    // The day after sees nothing; a bad range is refused.
    let next = reader
        .daily_sales("2027-01-16", "2027-01-16", f.clock.now_ms())
        .unwrap();
    assert_eq!((next.orders, next.sales_minor), (0, 0));
    assert!(reader.daily_sales(day, "2026-01-01", now).is_err());
    assert_ne!(first, second);

    let s = reader.sessions_report(day, day, now).unwrap();
    assert_eq!((s.sessions, s.minutes, s.revenue_minor), (0, 0, 0));
}

//! Story 6.32: a kiosk order paid from the wallet (a settled sale tied to the PC) or left at the
//! counter (an open order with no session that staff settle later), and what the PC shows for it.

use super::prepaid::sql;
use super::session::{
    balance, end, end_intent, int, pay, pc, player, refusal, rev_of, sessions, settle_intent,
    settle_revert_intent, start, submit, table, text, topped_up,
};
use super::stock::{held, intent, make, owner, stock_in, up};
use super::*;
use arena_proto::{
    CounterOrder, DeviceKind, FLAG_KIOSK_ORDERS, KioskSale, OrderPlace, OrderVoid, PaymentMethod,
    PosSale, PosSaleLine, ProductUpsert, ReasonCode,
};

fn licence() -> String {
    format!(
        "INSERT INTO settings (branch_id, key, value, rev, updated_at)
         SELECT branch_id, 'licence.flags', '{FLAG_KIOSK_ORDERS}', 0, 0 FROM branch WHERE true
         ON CONFLICT (branch_id, key) DO UPDATE SET value = excluded.value"
    )
}

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn lines(product: &str, qty: i64) -> Vec<PosSaleLine> {
    vec![PosSaleLine {
        product_id: product.into(),
        qty,
    }]
}

fn counter(device: &str, player: Option<&str>, product: &str, qty: i64) -> IntentPayload {
    IntentPayload::OrderPlace(OrderPlace {
        counter: Some(CounterOrder {
            device_id: device.into(),
            player_id: player.map(Into::into),
        }),
        session_id: String::new(),
        lines: lines(product, qty),
        stock_override: None,
    })
}

fn sale(kiosk: Option<&str>, player: Option<&str>, product: &str, qty: i64) -> PosSale {
    PosSale {
        kiosk: kiosk.map(|d| KioskSale {
            device_id: d.into(),
        }),
        lines: lines(product, qty),
        payments: Vec::new(),
        player_id: player.map(Into::into),
        credit_override: None,
        stock_override: None,
        receipt: None,
        business: None,
        coupon: None,
        discount: None,
    }
}

fn wallet_sale(device: &str, player: &str, product: &str, qty: i64) -> IntentPayload {
    IntentPayload::PosSale(sale(Some(device), Some(player), product, qty))
}

async fn cola(f: &Fixture, qty: i64) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: 4_000,
            track_stock: true,
            ..up("Cola")
        },
    )
    .await;
    stock_in(f, &id, qty, 2_000).await;
    id
}

fn stock_of(f: &Fixture, id: &str) -> i64 {
    held(f, id).0
}

fn orders(f: &Fixture) -> Vec<Row> {
    table(f, Table::Orders)
        .into_iter()
        .filter(|o| text(o, "kind") == "pos_sale")
        .collect()
}

fn unpaid(f: &Fixture, device: &str) -> i64 {
    reopen(f).unpaid_counter_minor(device).unwrap()
}

async fn end_session(f: &Fixture) {
    let s = sessions(f).pop().unwrap();
    end(f, end_intent(&text(&s, "id"), Some(int(&s, "rev"))))
        .await
        .unwrap();
}

#[tokio::test]
async fn pay_at_counter_opens_an_unpaid_order_tied_to_the_pc_and_the_player() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let p = player(&f, "asha", "+919876543210").await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;

    // The player has not had a session here yet, so the order cannot be tied to them.
    let (code, _) = refusal(submit(&f, device(&d), counter(&d, Some(&p), &id, 2)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);

    // Once their session has ended the order may be tied to them.
    start(&f, &d, Some(&p)).await.unwrap();
    end_session(&f).await;
    submit(&f, device(&d), counter(&d, Some(&p), &id, 2))
        .await
        .unwrap();

    let o = orders(&f).pop().unwrap();
    assert_eq!(text(&o, "status"), "open");
    assert_eq!(o["session_id"], RowValue::Null);
    assert_eq!(text(&o, "device_id"), d);
    assert_eq!(text(&o, "player_id"), p);
    assert_eq!(int(&o, "total_minor"), 8_000);
    // Nothing is paid and no invoice number exists yet.
    assert!(table(&f, Table::Payments).is_empty());
    assert_eq!(o["invoice_no"], RowValue::Null);
    // Stock was taken, the PC owes the amount and its attract screen says so.
    assert_eq!(stock_of(&f, &id), 3);
    assert_eq!(unpaid(&f, &d), 8_000);
    let views = crate::attract::attract_views(
        &reopen(&f),
        std::slice::from_ref(&d),
        f.clock.now_ms(),
        &|_| crate::attract::GameMediaHashes::default(),
    )
    .unwrap();
    assert_eq!(views[0].1.unpaid_minor, 8_000);
}

#[tokio::test]
async fn a_device_cannot_leave_an_order_for_another_pc_or_player_or_override_stock() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let mine = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let p = player(&f, "asha", "+919876543210").await;

    let (code, _) = refusal(submit(&f, device(&other), counter(&mine, None, &id, 1)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let (code, _) = refusal(submit(&f, device(&mine), counter(&mine, Some(&p), &id, 1)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let mut over = counter(&mine, None, &id, 1);
    if let IntentPayload::OrderPlace(o) = &mut over {
        o.stock_override = Some(true);
    }
    let (code, _) = refusal(submit(&f, device(&mine), over).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert!(orders(&f).is_empty());
    assert_eq!(unpaid(&f, &mine), 0);
}

#[tokio::test]
async fn a_short_counter_order_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 1).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let (code, field) = refusal(submit(&f, device(&d), counter(&d, None, &id, 3)).await);
    assert_eq!(code, ErrorCode::OutOfStock);
    assert_eq!(field.as_deref(), Some("lines[0]"));
    assert!(orders(&f).is_empty());
    assert_eq!(stock_of(&f, &id), 1);
}

#[tokio::test]
async fn without_the_licence_flag_a_counter_order_is_not_in_tier() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let (code, _) = refusal(submit(&f, device(&d), counter(&d, None, &id, 1)).await);
    assert_eq!(code, ErrorCode::FeatureNotInTier);
}

#[tokio::test]
async fn staff_settle_an_unpaid_order_with_any_mix_and_undo_reopens_it() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    submit(&f, device(&d), counter(&d, None, &id, 2))
        .await
        .unwrap();
    let o = orders(&f).pop().unwrap();
    let order = text(&o, "id");

    end(
        &f,
        settle_intent(
            &order,
            Some(int(&o, "rev")),
            vec![
                pay(PaymentMethod::Cash, 5_000, None),
                pay(PaymentMethod::Upi, 3_000, Some("1234")),
            ],
        ),
    )
    .await
    .unwrap();
    let o = orders(&f).pop().unwrap();
    assert_eq!(text(&o, "status"), "settled");
    assert_eq!(table(&f, Table::Payments).len(), 2);
    assert_eq!(unpaid(&f, &d), 0);

    end(&f, settle_revert_intent(&order, Some(int(&o, "rev"))))
        .await
        .unwrap();
    let o = orders(&f).pop().unwrap();
    assert_eq!(text(&o, "status"), "open");
    assert!(table(&f, Table::Payments).is_empty());
    assert_eq!(unpaid(&f, &d), 8_000);
}

#[tokio::test]
async fn a_counter_sale_is_never_reopened_as_an_unpaid_order() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let mut s = sale(None, None, &id, 1);
    s.payments = vec![pay(PaymentMethod::Cash, 4_000, None)];
    let applied = submit(&f, owner(), IntentPayload::PosSale(s))
        .await
        .unwrap();
    let order = applied.changes[0].row_id.clone();
    let rev = rev_of(&f, Table::Orders, &order);
    let (code, _) = refusal(end(&f, settle_revert_intent(&order, Some(rev))).await);
    assert_eq!(code, ErrorCode::ValidationFailed);
    assert_eq!(text(&orders(&f)[0], "status"), "settled");
}

#[tokio::test]
async fn staff_void_an_unpaid_order_and_its_stock_comes_back() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    submit(&f, device(&d), counter(&d, None, &id, 2))
        .await
        .unwrap();
    let order = text(&orders(&f)[0], "id");
    assert_eq!(stock_of(&f, &id), 3);

    let mut v = intent(
        owner(),
        IntentPayload::OrderVoid(OrderVoid {
            order_id: order,
            note: None,
        }),
    );
    v.reason = Some(ReasonCode::Mistake);
    end(&f, v).await.unwrap();
    assert_eq!(text(&orders(&f)[0], "status"), "void");
    assert_eq!(stock_of(&f, &id), 5);
    assert_eq!(unpaid(&f, &d), 0);
}

#[tokio::test]
async fn pay_now_settles_from_the_wallet_in_one_transaction_and_ties_the_order_to_the_pc() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(&f, &d, Some(&p)).await.unwrap();

    submit(&f, device(&d), wallet_sale(&d, &p, &id, 2))
        .await
        .unwrap();
    let o = orders(&f).pop().unwrap();
    assert_eq!(text(&o, "status"), "settled");
    assert_eq!(text(&o, "device_id"), d);
    assert_eq!(text(&o, "player_id"), p);
    assert_eq!(int(&o, "total_minor"), 8_000);
    assert_eq!(balance(&f, &p), 2_000);
    let pays: Vec<_> = table(&f, Table::Payments)
        .into_iter()
        .filter(|p| text(p, "order_id") == text(&o, "id"))
        .collect();
    assert_eq!(pays.len(), 1);
    assert_eq!(text(&pays[0], "method"), "wallet");
    assert_eq!(int(&pays[0], "amount_minor"), 8_000);
    assert_eq!(stock_of(&f, &id), 3);
    assert_eq!(unpaid(&f, &d), 0);
}

#[tokio::test]
async fn pay_now_with_too_little_wallet_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 4_000).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(&f, &d, Some(&p)).await.unwrap();

    let before = table(&f, Table::Orders).len();
    let (code, _) = refusal(submit(&f, device(&d), wallet_sale(&d, &p, &id, 2)).await);
    assert_eq!(code, ErrorCode::InsufficientBalance);
    assert_eq!(table(&f, Table::Orders).len(), before);
    assert_eq!(balance(&f, &p), 4_000);
    assert_eq!(stock_of(&f, &id), 5);
}

#[tokio::test]
async fn pay_now_is_only_for_the_player_running_on_the_device_and_carries_nothing_else() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let p = player(&f, "asha", "+919876543210").await;
    let q = player(&f, "ravi", "+919876543211").await;
    topped_up(&f, &p, 20_000).await;
    topped_up(&f, &q, 20_000).await;
    let mine = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(&f, &mine, Some(&p)).await.unwrap();
    start(&f, &other, Some(&q)).await.unwrap();

    // Another player's wallet, another PC's name, and a sale that brings its own payments.
    let (code, _) = refusal(submit(&f, device(&mine), wallet_sale(&mine, &q, &id, 1)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let (code, _) = refusal(submit(&f, device(&other), wallet_sale(&mine, &p, &id, 1)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let mut paid = sale(Some(&mine), Some(&p), &id, 1);
    paid.payments = vec![pay(PaymentMethod::Cash, 4_000, None)];
    let (code, field) = refusal(submit(&f, device(&mine), IntentPayload::PosSale(paid)).await);
    assert_eq!(code, ErrorCode::ValidationFailed);
    assert_eq!(field.as_deref(), Some("payments"));
    let mut over = sale(Some(&mine), Some(&p), &id, 1);
    over.stock_override = Some(true);
    let (code, _) = refusal(submit(&f, device(&mine), IntentPayload::PosSale(over)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert_eq!(balance(&f, &p), 20_000);
    assert_eq!(balance(&f, &q), 20_000);
    assert!(orders(&f).is_empty());
}

#[tokio::test]
async fn the_menu_offers_what_the_pc_can_take() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    cola(&f, 5).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 7_000).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let dir = crate::DeviceDirectory::open(&f.path).unwrap();
    let pay_of = || dir.shop_menu(f.clock.now_ms(), &d).unwrap().pay;

    // No session: only Pay at counter.
    let none = pay_of();
    assert!(!none.bill && none.counter && none.wallet_minor.is_none());

    // A guest session: the bill and the counter, no wallet.
    start(&f, &d, None).await.unwrap();
    let guest = pay_of();
    assert!(guest.bill && guest.counter && guest.wallet_minor.is_none());
    end_session(&f).await;

    // A profiled session: the wallet balance too.
    start(&f, &d, Some(&p)).await.unwrap();
    let signed_in = pay_of();
    assert!(signed_in.bill && signed_in.counter);
    assert_eq!(signed_in.wallet_minor, Some(7_000));
}

#[tokio::test]
async fn an_open_counter_order_has_no_payments_and_a_settled_one_has_them() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    submit(&f, device(&d), counter(&d, None, &id, 1))
        .await
        .unwrap();
    let o = orders(&f).pop().unwrap();
    let order = text(&o, "id");
    let paid_rows = || {
        table(&f, Table::Payments)
            .iter()
            .filter(|p| text(p, "order_id") == order)
            .count()
    };
    assert_eq!(paid_rows(), 0);
    assert_eq!(o["invoice_no"], RowValue::Null);

    end(
        &f,
        settle_intent(
            &order,
            Some(int(&o, "rev")),
            vec![pay(PaymentMethod::Cash, 4_000, None)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(paid_rows(), 1);
}

#[tokio::test]
async fn the_snapshot_lists_an_unpaid_counter_order_as_a_bill_of_its_pc_with_no_session() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    submit(&f, device(&d), counter(&d, None, &id, 2))
        .await
        .unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.bills.len(), 1);
    let bill = &snap.bills[0];
    // The floor tile sums these for "UNPAID", and POS Open orders lists the ones with no session.
    assert_eq!(bill.session_id, "");
    assert_eq!(bill.device_id, d);
    assert!(bill.order_id.is_some());
    assert_eq!(bill.remaining_minor, 8_000);
    assert_eq!(bill.items.len(), 1);
    assert_eq!(bill.items[0].qty, 2);

    // Settled, it is no longer a bill.
    let o = orders(&f).pop().unwrap();
    end(
        &f,
        settle_intent(
            &text(&o, "id"),
            Some(int(&o, "rev")),
            vec![pay(PaymentMethod::Cash, 8_000, None)],
        ),
    )
    .await
    .unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(snap.bills.is_empty());
}

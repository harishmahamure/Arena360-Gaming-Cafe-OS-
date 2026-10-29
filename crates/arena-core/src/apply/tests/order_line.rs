use super::session::{
    end, end_intent, int, pay, pc, record_effect, refusal, rev_of, sessions, settle_intent, start,
    submit, table, text,
};
use super::stock::{held, intent, lots_of, make, owner, staff, stock_in, up};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    BundleComponent, DeviceKind, KEY_INVENTORY_TAKE_STOCK_AT, OrderLineCancel, OrderLineServed,
    OrderPlace, OrderPlaceReverted, PaymentRow, PosSale, PosSaleLine, ProductUpsert,
};

fn place_of(session: &str, lines: &[(&str, i64)]) -> OrderPlace {
    OrderPlace {
        counter: None,
        session_id: session.into(),
        lines: lines
            .iter()
            .map(|(id, qty)| PosSaleLine {
                product_id: (*id).into(),
                qty: *qty,
            })
            .collect(),
        stock_override: None,
    }
}

async fn place(f: &Fixture, actor: Actor, p: OrderPlace) -> Result<Applied, AppError> {
    submit(f, actor, IntentPayload::OrderPlace(p)).await
}

pub(super) async fn serve(f: &Fixture, actor: Actor, line: &str) -> Result<Applied, AppError> {
    submit(
        f,
        actor,
        IntentPayload::OrderLineServed(OrderLineServed {
            line_id: line.into(),
        }),
    )
    .await
}

fn cancel_intent(actor: Actor, line: &str, made: bool, reason: Option<ReasonCode>) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::OrderLineCancel(OrderLineCancel {
            line_id: line.into(),
            made,
        }),
    );
    i.reason = reason;
    i
}

pub(super) async fn cancel(
    f: &Fixture,
    actor: Actor,
    line: &str,
    made: bool,
) -> Result<Applied, AppError> {
    let mut i = cancel_intent(actor, line, made, Some(ReasonCode::ChangedMind));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn unplace(
    f: &Fixture,
    actor: Actor,
    order: &str,
    placement: &str,
) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::OrderPlaceReverted(OrderPlaceReverted {
            order_id: order.into(),
            placement_id: placement.into(),
        }),
    );
    i.base_rev = Some(rev_of(f, Table::Orders, order));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn set_mode(f: &Fixture, value: &str) {
    let store = reopen(f);
    let row = store
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text(KEY_INVENTORY_TAKE_STOCK_AT.into()))
        .expect("the setting is seeded");
    let mut i = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: KEY_INVENTORY_TAKE_STOCK_AT.into(),
            value: value.into(),
        }),
    );
    i.base_rev = Some(int(&row, "rev"));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
}

fn priced(name: &str, price: i64, kitchen: bool) -> ProductUpsert {
    ProductUpsert {
        price_minor: price,
        kitchen,
        ..up(name)
    }
}

/// A tracked kitchen product with `qty` on hand at cost 2 000.
async fn burger(f: &Fixture, qty: i64) -> String {
    let id = make(f, priced("Burger", 9_000, true)).await;
    stock_in(f, &id, qty, 2_000).await;
    id
}

async fn cola(f: &Fixture, qty: i64) -> String {
    let id = make(f, priced("Cola", 4_000, false)).await;
    stock_in(f, &id, qty, 1_000).await;
    id
}

async fn running(f: &Fixture) -> String {
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    text(sessions(f).last().unwrap(), "id")
}

fn order_of(f: &Fixture, session: &str) -> Option<Row> {
    table(f, Table::Orders)
        .into_iter()
        .find(|o| matches!(&o["session_id"], RowValue::Text(t) if t == session))
}

fn lines_of(f: &Fixture, order: &str) -> Vec<Row> {
    let mut lines: Vec<Row> = table(f, Table::OrderLines)
        .into_iter()
        .filter(|l| text(l, "order_id") == order)
        .collect();
    lines.sort_by_key(|l| int(l, "position"));
    lines
}

/// The line of `label` on the session's bill.
fn line(f: &Fixture, session: &str, label: &str) -> Row {
    let order = text(&order_of(f, session).unwrap(), "id");
    lines_of(f, &order)
        .into_iter()
        .find(|l| l["label"] == RowValue::Text(label.into()))
        .unwrap()
}

fn holds(f: &Fixture) -> Vec<Row> {
    table(f, Table::StockHolds)
}

fn open_holds(f: &Fixture) -> Vec<Row> {
    holds(f)
        .into_iter()
        .filter(|h| h["released_at"] == RowValue::Null)
        .collect()
}

fn moves_of(f: &Fixture, id: &str, reason: &str) -> Vec<arena_store::MoveRow> {
    reopen(f)
        .moves(id)
        .unwrap()
        .into_iter()
        .filter(|m| m.reason == reason)
        .collect()
}

fn code(r: Result<Applied, AppError>) -> ErrorCode {
    refusal(r).0
}

fn available(f: &Fixture, id: &str) -> bool {
    crate::catalog::products(&reopen(f), f.clock.now_ms())
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
        .available
}

#[tokio::test]
async fn served_mode_holds_a_kitchen_line_and_takes_no_stock() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let tables: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["orders", "order_lines", "stock_holds", "stock"]);
    let l = line(&f, &session, "Burger");
    assert_eq!(int(&l, "served"), 0);
    assert_eq!(l["served_at"], RowValue::Null);
    let h = open_holds(&f);
    assert_eq!(h.len(), 1);
    assert_eq!(
        (
            text(&h[0], "order_line_id"),
            text(&h[0], "item_id"),
            int(&h[0], "qty")
        ),
        (text(&l, "id"), id.clone(), 2)
    );
    assert!(moves_of(&f, &id, "sale").is_empty(), "no stock move yet");
    assert_eq!(held(&f, &id).0, 5, "the shelf is untouched");
    assert_eq!(reopen(&f).held_qty(&id).unwrap(), 2, "available is 5 - 2");
}

#[tokio::test]
async fn served_mode_serves_a_plain_line_at_once() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let l = line(&f, &session, "Cola");
    assert_eq!(int(&l, "served"), 1);
    assert_eq!(text(&l, "served_by"), "o");
    assert!(holds(&f).is_empty());
    assert_eq!(moves_of(&f, &id, "sale").len(), 1);
    assert_eq!(held(&f, &id).0, 4);
}

#[tokio::test]
async fn order_mode_takes_stock_at_placement_and_leaves_a_kitchen_line_unserved() {
    let f = bootstrapped().await;
    set_mode(&f, "order").await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let l = line(&f, &session, "Burger");
    assert_eq!(int(&l, "served"), 0);
    assert!(holds(&f).is_empty());
    assert_eq!(moves_of(&f, &id, "sale").len(), 1);
    assert_eq!(held(&f, &id).0, 3);
}

#[tokio::test]
async fn a_bundle_holds_each_tracked_part_times_the_quantity() {
    let f = bootstrapped().await;
    let patty = make(&f, priced("Patty", 100, false)).await;
    let bun = make(&f, priced("Bun", 100, false)).await;
    stock_in(&f, &patty, 10, 100).await;
    stock_in(&f, &bun, 10, 100).await;
    let meal = make(
        &f,
        ProductUpsert {
            bundle: true,
            track_stock: false,
            kitchen: true,
            components: vec![
                BundleComponent {
                    product_id: patty.clone(),
                    qty: 1,
                },
                BundleComponent {
                    product_id: bun.clone(),
                    qty: 2,
                },
            ],
            ..priced("Meal", 12_000, true)
        },
    )
    .await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&meal, 3)]))
        .await
        .unwrap();
    let mut h: Vec<(String, i64)> = open_holds(&f)
        .iter()
        .map(|h| (text(h, "item_id"), int(h, "qty")))
        .collect();
    h.sort();
    let mut want = vec![(patty.clone(), 3), (bun.clone(), 6)];
    want.sort();
    assert_eq!(h, want);
    assert_eq!(held(&f, &patty).0, 10);
    // Serving takes both parts and closes both holds.
    let l = line(&f, &session, "Meal");
    serve(&f, owner(), &text(&l, "id")).await.unwrap();
    assert_eq!((held(&f, &patty).0, held(&f, &bun).0), (7, 4));
    assert!(open_holds(&f).is_empty());
    assert_eq!(holds(&f).len(), 2, "the holds stay as a record");
}

#[tokio::test]
async fn marking_a_held_line_served_takes_its_stock_and_closes_the_hold() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let l = line(&f, &session, "Burger");
    f.clock.advance(5_000);
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let a = serve(&f, counter.clone(), &text(&l, "id")).await.unwrap();
    let tables: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(
        tables,
        [
            "stock",
            "stock_lots",
            "stock_moves",
            "stock_holds",
            "order_lines"
        ]
    );
    let l = line(&f, &session, "Burger");
    assert_eq!(int(&l, "served"), 1);
    assert_eq!(text(&l, "served_by"), counter.id);
    assert!(int(&l, "served_at") > 0);
    assert_eq!(held(&f, &id).0, 3);
    let moved = moves_of(&f, &id, "sale");
    assert_eq!(moved.len(), 1);
    assert_eq!((moved[0].delta, moved[0].value_minor), (-2, 4_000));
    assert_eq!(moved[0].staff_id, counter.id);
    assert!(open_holds(&f).is_empty());
    assert!(matches!(holds(&f)[0]["released_at"], RowValue::Integer(_)));
    // The bill is unchanged by serving.
    let o = order_of(&f, &session).unwrap();
    assert_eq!(int(&o, "total_minor"), 18_000);
}

#[tokio::test]
async fn marking_served_in_order_mode_changes_no_stock() {
    let f = bootstrapped().await;
    set_mode(&f, "order").await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let l = line(&f, &session, "Burger");
    let a = serve(&f, owner(), &text(&l, "id")).await.unwrap();
    assert!(a.changes.iter().all(|c| c.tbl == "order_lines"));
    assert_eq!(int(&line(&f, &session, "Burger"), "served"), 1);
    assert_eq!(held(&f, &id).0, 3);
    assert_eq!(moves_of(&f, &id, "sale").len(), 1);
}

#[tokio::test]
async fn serving_twice_or_without_the_stock_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    // The stock fell below the hold: 5 on hand, 4 written off.
    let mut w = intent(
        owner(),
        IntentPayload::StockWaste(arena_proto::StockWaste {
            product_id: id.clone(),
            qty: 4,
        }),
    );
    w.reason = Some(ReasonCode::Spoiled);
    w.issued_at = f.clock.now_ms();
    f.writer.submit_intent(w).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    let (c, field) = refusal(serve(&f, owner(), &l).await);
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::OutOfStock, Some("line_id"))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(int(&line(&f, &session, "Burger"), "served"), 0);
    assert_eq!(open_holds(&f).len(), 1);

    // With stock back, it serves once and not twice.
    stock_in(&f, &id, 5, 2_000).await;
    serve(&f, owner(), &l).await.unwrap();
    let (c, field) = refusal(serve(&f, owner(), &l).await);
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("line_id"))
    );
}

#[tokio::test]
async fn an_unknown_line_or_a_time_line_cannot_be_served() {
    let f = bootstrapped().await;
    let session = running(&f).await;
    let (c, field) = refusal(serve(&f, owner(), "nope").await);
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::NotFound, Some("line_id"))
    );
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let time_line = text(&lines_of(&f, &order)[0], "id");
    assert_eq!(
        code(serve(&f, owner(), &time_line).await),
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn cancelling_a_held_line_that_was_not_made_releases_the_hold() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let chips = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    let moves_before = reopen(&f).moves(&id).unwrap().len();
    let a = cancel(&f, owner(), &l, false).await.unwrap();
    let tables: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["stock_holds", "stock", "order_lines", "orders"]);
    assert_eq!(reopen(&f).moves(&id).unwrap().len(), moves_before);
    assert!(open_holds(&f).is_empty());
    assert_eq!(held(&f, &id).0, 5);
    let o = order_of(&f, &session).unwrap();
    assert_eq!(
        (int(&o, "subtotal_minor"), int(&o, "total_minor")),
        (4_000, 4_000)
    );
    assert_eq!(lines_of(&f, &text(&o, "id")).len(), 1);
    // The audit keeps who and why.
    let changes = reopen(&f).changes().unwrap();
    let c = changes
        .iter()
        .rev()
        .find(|c| c.intent_kind == "order.line_cancel")
        .unwrap();
    assert_eq!(c.reason.as_deref(), Some("changed_mind"));
    assert_eq!(c.actor_id, "o");
}

#[tokio::test]
async fn cancelling_the_only_line_removes_the_order() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    assert!(order_of(&f, &session).is_none());
    assert!(table(&f, Table::OrderLines).is_empty());
    // A new placement opens a fresh order.
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    assert!(order_of(&f, &session).is_some());
}

#[tokio::test]
async fn cancelling_in_order_mode_returns_the_stock_through_a_return_move() {
    let f = bootstrapped().await;
    set_mode(&f, "order").await;
    let id = burger(&f, 5).await;
    let chips = cola(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    assert_eq!(held(&f, &id).0, 5);
    let back = moves_of(&f, &id, "return");
    assert_eq!(back.len(), 1);
    assert_eq!(
        (
            back[0].delta,
            back[0].placement_id.clone(),
            back[0].unit_cost_minor
        ),
        (2, Some(a.intent_id.clone()), 2_000)
    );
    assert!(moves_of(&f, &id, "waste").is_empty(), "not made: no waste");
    let o = order_of(&f, &session).unwrap();
    assert_eq!(int(&o, "total_minor"), 4_000);
    let lots = lots_of(&f, &id);
    assert_eq!(lots.iter().map(|l| l.1).sum::<i64>(), 5);
}

#[tokio::test]
async fn cancelling_a_made_line_records_waste_at_cost_in_both_modes() {
    for mode in ["served", "order"] {
        let f = bootstrapped().await;
        set_mode(&f, mode).await;
        let id = burger(&f, 5).await;
        let chips = cola(&f, 5).await;
        let session = running(&f).await;
        place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
            .await
            .unwrap();
        let l = text(&line(&f, &session, "Burger"), "id");
        cancel(&f, owner(), &l, true).await.unwrap();
        assert_eq!(held(&f, &id).0, 3, "{mode}: the made burgers are gone");
        let waste = moves_of(&f, &id, "waste");
        assert_eq!(waste.len(), 1, "{mode}");
        assert_eq!(
            (
                waste[0].delta,
                waste[0].value_minor,
                waste[0].waste_reason.clone()
            ),
            (-2, 4_000, Some("cancelled_order".to_owned())),
            "{mode}"
        );
        assert_eq!(
            moves_of(&f, &id, "return").len(),
            usize::from(mode == "order")
        );
        assert!(open_holds(&f).is_empty(), "{mode}");
        assert_eq!(
            int(&order_of(&f, &session).unwrap(), "total_minor"),
            4_000,
            "{mode}"
        );
    }
}

#[tokio::test]
async fn waste_of_a_made_held_line_is_capped_at_the_stock_on_hand() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 4)]))
        .await
        .unwrap();
    // Four held of five; 4 are written off by hand, leaving 1 on hand.
    let mut w = intent(
        owner(),
        IntentPayload::StockWaste(arena_proto::StockWaste {
            product_id: id.clone(),
            qty: 4,
        }),
    );
    w.reason = Some(ReasonCode::Spoiled);
    w.issued_at = f.clock.now_ms();
    f.writer.submit_intent(w).await.unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, true).await.unwrap();
    assert_eq!(held(&f, &id).0, 0);
    let cancelled: Vec<_> = moves_of(&f, &id, "waste")
        .into_iter()
        .filter(|m| m.waste_reason.as_deref() == Some("cancelled_order"))
        .collect();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].delta, -1, "only what was on hand");
}

#[tokio::test]
async fn a_served_line_cannot_be_cancelled_and_a_reason_is_required() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let kitchen = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1), (&kitchen, 1)]))
        .await
        .unwrap();
    let served = text(&line(&f, &session, "Cola"), "id");
    let before = reopen(&f).changes().unwrap().len();
    let (c, field) = refusal(cancel(&f, owner(), &served, false).await);
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("line_id"))
    );
    // No reason, or one that does not apply.
    let pending = text(&line(&f, &session, "Burger"), "id");
    for reason in [None, Some(ReasonCode::Spoiled)] {
        let mut i = cancel_intent(owner(), &pending, false, reason);
        i.issued_at = f.clock.now_ms();
        let (c, _) = refusal(f.writer.submit_intent(i).await);
        assert_eq!(
            c,
            if reason.is_none() {
                ErrorCode::ReasonRequired
            } else {
                ErrorCode::ValidationFailed
            }
        );
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    // Serving takes no reason either.
    let mut i = intent(
        owner(),
        IntentPayload::OrderLineServed(OrderLineServed {
            line_id: pending.clone(),
        }),
    );
    i.reason = Some(ReasonCode::Mistake);
    i.issued_at = f.clock.now_ms();
    assert_eq!(
        code(f.writer.submit_intent(i).await),
        ErrorCode::ValidationFailed
    );
    // Neither intent takes a base_rev.
    let mut i = cancel_intent(owner(), &pending, false, Some(ReasonCode::Other));
    i.base_rev = Some(1);
    i.issued_at = f.clock.now_ms();
    assert_eq!(
        code(f.writer.submit_intent(i).await),
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn both_intents_need_pos_sell() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    let cook = staff(&f, "cook", ROLE_KITCHEN).await;
    assert_eq!(
        code(serve(&f, cook.clone(), &l).await),
        ErrorCode::PermissionDenied
    );
    let mut i = cancel_intent(cook, &l, false, Some(ReasonCode::Other));
    i.issued_at = f.clock.now_ms();
    assert_eq!(
        code(f.writer.submit_intent(i).await),
        ErrorCode::PermissionDenied
    );
    assert_eq!(int(&line(&f, &session, "Burger"), "served"), 0);
}

#[tokio::test]
async fn available_stock_is_on_hand_less_open_holds_for_sales_and_the_catalogue() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 4)]))
        .await
        .unwrap();
    assert!(available(&f, &id), "one is still free");
    // A counter sale of 2 is refused, a sale of 1 goes through.
    let sale = |qty: i64| PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: vec![PosSaleLine {
            product_id: id.clone(),
            qty,
        }],
        payments: vec![pay(Cash, 9_000 * qty, None)],
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    };
    let before = reopen(&f).changes().unwrap().len();
    let (c, field) = refusal(submit(&f, owner(), IntentPayload::PosSale(sale(2))).await);
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::OutOfStock, Some("lines[0]"))
    );
    // So is a placement of 2.
    let (c, _) = refusal(place(&f, owner(), place_of(&session, &[(&id, 2)])).await);
    assert_eq!(c, ErrorCode::OutOfStock);
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    submit(&f, owner(), IntentPayload::PosSale(sale(1)))
        .await
        .unwrap();
    assert!(!available(&f, &id), "all five are spoken for");
    // Releasing a hold frees the stock for sale again.
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    assert!(available(&f, &id));
}

#[tokio::test]
async fn counter_sale_lines_are_served_in_both_modes() {
    for mode in ["served", "order"] {
        let f = bootstrapped().await;
        set_mode(&f, mode).await;
        let id = burger(&f, 5).await;
        submit(
            &f,
            owner(),
            IntentPayload::PosSale(PosSale {
                kiosk: None,
                business: None,
                coupon: None,
                discount: None,
                lines: vec![PosSaleLine {
                    product_id: id.clone(),
                    qty: 2,
                }],
                payments: vec![PaymentRow {
                    method: Cash,
                    amount_minor: 18_000,
                    reference: None,
                }],
                player_id: None,
                credit_override: None,
                stock_override: None,
                receipt: None,
            }),
        )
        .await
        .unwrap();
        let lines = table(&f, Table::OrderLines);
        assert_eq!(lines.len(), 1);
        assert_eq!(int(&lines[0], "served"), 1, "{mode}");
        assert!(holds(&f).is_empty(), "{mode}");
        assert_eq!(held(&f, &id).0, 3, "{mode}: taken at settle");
    }
}

#[tokio::test]
async fn a_session_bill_cannot_settle_while_a_line_is_unserved() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let rev = rev_of(&f, Table::Orders, &order);
    let before = reopen(&f).changes().unwrap().len();
    let (c, field) = refusal(
        end(
            &f,
            settle_intent(&order, Some(rev), vec![pay(Cash, 19_000, None)]),
        )
        .await,
    );
    assert_eq!(
        (c, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("order_id"))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    // Serve it and the same settle goes through.
    let l = text(&line(&f, &session, "Burger"), "id");
    serve(&f, owner(), &l).await.unwrap();
    let rev = rev_of(&f, Table::Orders, &order);
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 19_000, None)]),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &id).0, 4);
}

#[tokio::test]
async fn a_cancelled_line_lets_the_bill_settle() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let rev = rev_of(&f, Table::Orders, &order);
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 10_000, None)]),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &id).0, 5);
}

#[tokio::test]
async fn undoing_a_placement_removes_its_lines_and_open_holds() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let chips = cola(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    f.clock.advance(2_000);
    let u = unplace(&f, owner(), &order, &a.intent_id).await.unwrap();
    assert!(u.changes.iter().any(|c| c.tbl == "stock_holds"));
    assert!(holds(&f).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert!(order_of(&f, &session).is_none());
    assert_eq!(held(&f, &id).0, 5);
    assert_eq!(held(&f, &chips).0, 5);
    assert_eq!(reopen(&f).held_qty(&id).unwrap(), 0);
}

#[tokio::test]
async fn a_placement_whose_held_line_was_served_still_undoes_and_returns_the_stock() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    serve(&f, owner(), &l).await.unwrap();
    assert_eq!(held(&f, &id).0, 3);
    let order = text(&order_of(&f, &session).unwrap(), "id");
    f.clock.advance(2_000);
    unplace(&f, owner(), &order, &a.intent_id).await.unwrap();
    assert_eq!(held(&f, &id).0, 5);
    assert!(table(&f, Table::OrderLines).is_empty());
}

#[tokio::test]
async fn a_placement_cannot_be_undone_once_an_order_mode_line_was_cancelled() {
    let f = bootstrapped().await;
    set_mode(&f, "order").await;
    let id = burger(&f, 5).await;
    let chips = cola(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    f.clock.advance(1_000);
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        code(unplace(&f, owner(), &order, &a.intent_id).await),
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
}

#[tokio::test]
async fn a_served_mode_cancel_does_not_block_the_undo_of_the_rest() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let chips = cola(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 1)]))
        .await
        .unwrap();
    let l = text(&line(&f, &session, "Burger"), "id");
    cancel(&f, owner(), &l, false).await.unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    f.clock.advance(1_000);
    unplace(&f, owner(), &order, &a.intent_id).await.unwrap();
    assert_eq!(held(&f, &chips).0, 5);
    assert!(order_of(&f, &session).is_none());
}

#[tokio::test]
async fn a_recorded_effect_still_blocks_the_undo_of_a_placement() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    record_effect(&f, arena_proto::EffectKind::ReceiptPrinted, "", &order).await;
    f.clock.advance(1_000);
    assert_eq!(
        code(unplace(&f, owner(), &order, &a.intent_id).await),
        ErrorCode::UndoNotAllowed
    );
}

#[tokio::test]
async fn the_mode_follows_the_setting_at_placement_and_a_held_line_survives_a_switch() {
    let f = bootstrapped().await;
    let id = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    // Switching to `order` later does not strand the held line: serving still takes its hold.
    set_mode(&f, "order").await;
    let l = text(&line(&f, &session, "Burger"), "id");
    serve(&f, owner(), &l).await.unwrap();
    assert_eq!(held(&f, &id).0, 3);
    assert!(open_holds(&f).is_empty());
}

#[tokio::test]
async fn the_setting_only_accepts_served_or_order() {
    let f = bootstrapped().await;
    let store = reopen(&f);
    let row = store
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text(KEY_INVENTORY_TAKE_STOCK_AT.into()))
        .unwrap();
    assert_eq!(text(&row, "value"), "served");
    let mut i = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: KEY_INVENTORY_TAKE_STOCK_AT.into(),
            value: "later".into(),
        }),
    );
    i.base_rev = Some(int(&row, "rev"));
    i.issued_at = f.clock.now_ms();
    assert_eq!(
        code(f.writer.submit_intent(i).await),
        ErrorCode::ValidationFailed
    );
}

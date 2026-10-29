use super::prepaid::{begin_block, group_of, ist, sql, wallet_session, window_plan};
use super::session::{
    end, end_intent, end_revert_intent, int, pay, pc, player, record_effect, refusal, rev_of,
    sessions, settle_intent, start, submit, table, text, topped_up,
};
use super::stock::{held, intent, lots_of, make, manager_with_pin, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    BundleComponent, DeviceKind, OrderPlace, OrderPlaceReverted, PosSaleLine, ProductUpsert,
    SessionGroup, SessionMode,
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

fn unplace_intent(actor: Actor, order: &str, placement: &str, rev: Option<i64>) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::OrderPlaceReverted(OrderPlaceReverted {
            order_id: order.into(),
            placement_id: placement.into(),
        }),
    );
    i.base_rev = rev;
    i.issued_at = 0;
    i
}

async fn unplace(
    f: &Fixture,
    actor: Actor,
    order: &str,
    placement: &str,
) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::Orders, order);
    let mut i = unplace_intent(actor, order, placement, Some(rev));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn priced(name: &str, price: i64, track: bool) -> ProductUpsert {
    ProductUpsert {
        price_minor: price,
        track_stock: track,
        ..up(name)
    }
}

async fn cola(f: &Fixture, qty: i64) -> String {
    let id = make(f, priced("Cola", 4_000, true)).await;
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

/// A running postpaid guest session on a PC at 100.00 an hour.
async fn running(f: &Fixture) -> (String, String) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    let s = text(sessions(f).last().unwrap(), "id");
    (s, d)
}

fn orders_of(f: &Fixture, session: &str) -> Vec<Row> {
    table(f, Table::Orders)
        .into_iter()
        .filter(|o| matches!(&o["session_id"], RowValue::Text(t) if t == session))
        .collect()
}

fn lines_of(f: &Fixture, order: &str) -> Vec<Row> {
    let mut lines: Vec<Row> = table(f, Table::OrderLines)
        .into_iter()
        .filter(|l| text(l, "order_id") == order)
        .collect();
    lines.sort_by_key(|l| int(l, "position"));
    lines
}

fn moves_of(f: &Fixture, id: &str, reason: &str) -> Vec<arena_store::MoveRow> {
    reopen(f)
        .moves(id)
        .unwrap()
        .into_iter()
        .filter(|m| m.reason == reason)
        .collect()
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn code(r: Result<Applied, AppError>) -> ErrorCode {
    refusal(r).0
}

#[tokio::test]
async fn the_first_placement_opens_the_bill_order_and_takes_stock() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let (session, device) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    let a = place(&f, counter.clone(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let kinds: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "orders",
            "order_lines",
            "stock",
            "stock_lots",
            "stock_moves"
        ]
    );
    let orders = orders_of(&f, &session);
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(
        (
            text(o, "kind"),
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor"),
            text(o, "device_id"),
        ),
        (
            "session".to_owned(),
            "open".to_owned(),
            8_000,
            8_000,
            device
        )
    );
    assert!(table(&f, Table::Payments).is_empty());
    let lines = lines_of(&f, &text(o, "id"));
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    assert_eq!(
        (
            text(l, "label"),
            int(l, "qty"),
            int(l, "unit_price_minor"),
            int(l, "amount_minor"),
            int(l, "tax_rate_bp"),
            text(l, "placement_id"),
            text(l, "placed_by"),
        ),
        (
            "Cola".to_owned(),
            2,
            4_000,
            8_000,
            0,
            a.intent_id.clone(),
            counter.id.clone()
        )
    );
    assert_eq!(held(&f, &id).0, 3);
    let moved = moves_of(&f, &id, "sale");
    assert_eq!(moved.len(), 1);
    assert_eq!(
        (
            moved[0].delta,
            moved[0].placement_id.clone(),
            moved[0].order_id.clone()
        ),
        (-2, Some(a.intent_id.clone()), Some(text(o, "id")))
    );
}

#[tokio::test]
async fn a_second_placement_appends_lines_with_its_own_placement_id() {
    let f = bootstrapped().await;
    let id = cola(&f, 10).await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 10, 500).await;
    let (session, _) = running(&f).await;

    let first = place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let second = place(&f, owner(), place_of(&session, &[(&chips, 3), (&id, 1)]))
        .await
        .unwrap();
    assert_ne!(first.intent_id, second.intent_id);
    let orders = orders_of(&f, &session);
    assert_eq!(orders.len(), 1, "one order for the session");
    let o = &orders[0];
    assert_eq!(
        (int(o, "subtotal_minor"), int(o, "total_minor")),
        (8_000 + 3_000 + 4_000, 15_000)
    );
    let lines = lines_of(&f, &text(o, "id"));
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines
            .iter()
            .map(|l| text(l, "placement_id"))
            .collect::<Vec<_>>(),
        [
            first.intent_id.clone(),
            second.intent_id.clone(),
            second.intent_id.clone()
        ]
    );
    // The order's rev grew with each placement.
    assert_eq!(int(o, "rev"), 2);
    assert_eq!(held(&f, &id).0, 7);
    assert_eq!(held(&f, &chips).0, 7);
}

#[tokio::test]
async fn an_untracked_product_moves_no_stock_and_a_paused_session_takes_an_order() {
    let f = bootstrapped().await;
    let id = make(&f, priced("Noodles", 6_000, false)).await;
    let (session, _) = running(&f).await;
    let rev = int(sessions(&f).last().unwrap(), "rev");
    let mut pause = intent(
        owner(),
        IntentPayload::SessionPause(arena_proto::SessionPause {
            session_id: session.clone(),
            reason: arena_proto::PauseReason::PlayerSteppedAway,
            note: None,
        }),
    );
    pause.base_rev = Some(rev);
    f.writer.submit_intent(pause).await.unwrap();
    assert_eq!(text(sessions(&f).last().unwrap(), "state"), "paused");
    let a = place(&f, owner(), place_of(&session, &[(&id, 3)]))
        .await
        .unwrap();
    assert!(a.changes.iter().all(|c| !c.tbl.starts_with("stock")));
    assert_eq!(int(&orders_of(&f, &session)[0], "total_minor"), 18_000);
}

#[tokio::test]
async fn a_placement_after_an_undone_one_sorts_after_every_line_left() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let (session, _) = running(&f).await;
    let first = place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let order = text(&orders_of(&f, &session)[0], "id");
    f.clock.advance(1_000);
    unplace(&f, owner(), &order, &first.intent_id)
        .await
        .unwrap();
    place(&f, owner(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let mut positions: Vec<i64> = lines_of(&f, &order)
        .iter()
        .map(|l| int(l, "position"))
        .collect();
    positions.sort_unstable();
    positions.dedup();
    assert_eq!(positions.len(), 2, "no two lines share a position");
}

#[tokio::test]
async fn ending_after_an_order_joins_the_time_lines_to_the_one_order() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let (session, _) = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    let a = end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    assert!(
        !a.changes
            .iter()
            .any(|c| c.tbl == "orders" && c.op == ChangeOp::Insert),
        "no second order"
    );
    let orders = orders_of(&f, &session);
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor")
        ),
        ("open".to_owned(), 18_000, 18_000)
    );
    let lines = lines_of(&f, &text(o, "id"));
    assert_eq!(lines.len(), 2);
    // The time line sorts before the product line.
    assert_eq!(
        (
            lines[0]["product_id"].clone(),
            int(&lines[0], "amount_minor")
        ),
        (RowValue::Null, 10_000)
    );
    assert_eq!(
        (text(&lines[1], "label"), int(&lines[1], "amount_minor")),
        ("Cola".to_owned(), 8_000)
    );
}

#[tokio::test]
async fn undoing_the_end_keeps_the_order_and_its_products_and_drops_the_time_lines() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let (session, _) = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let ended = sessions(&f).pop().unwrap();
    f.clock.advance(3_000);
    let mut undo = end_revert_intent(&session, Some(int(&ended, "rev")));
    undo.issued_at = f.clock.now_ms();
    let a = f.writer.submit_intent(undo).await.unwrap();
    assert!(
        a.changes
            .iter()
            .any(|c| c.tbl == "orders" && c.op == ChangeOp::Update)
    );
    assert!(
        !a.changes
            .iter()
            .any(|c| c.tbl == "orders" && c.op == ChangeOp::Delete)
    );
    let orders = orders_of(&f, &session);
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(
        (
            text(o, "status"),
            int(o, "subtotal_minor"),
            int(o, "total_minor")
        ),
        ("open".to_owned(), 8_000, 8_000)
    );
    let lines = lines_of(&f, &text(o, "id"));
    assert_eq!(lines.len(), 1);
    assert_eq!(text(&lines[0], "label"), "Cola");
    assert_eq!(held(&f, &id).0, 3, "the stock stays taken");
}

#[tokio::test]
async fn undoing_the_end_of_a_session_with_no_order_still_deletes_the_bill() {
    let f = bootstrapped().await;
    let (session, _) = running(&f).await;
    f.clock.advance(30 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    assert_eq!(orders_of(&f, &session).len(), 1);
    let ended = sessions(&f).pop().unwrap();
    end(&f, {
        let mut i = end_revert_intent(&session, Some(int(&ended, "rev")));
        i.issued_at = f.clock.now_ms();
        i
    })
    .await
    .unwrap();
    assert!(orders_of(&f, &session).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
}

#[tokio::test]
async fn undoing_a_placement_gives_back_its_stock_and_leaves_earlier_ones() {
    let f = bootstrapped().await;
    let id = cola(&f, 10).await;
    let mut early = super::stock::sin(&id, 2, 2_000);
    early.expiry = Some("2027-03-01".into());
    super::stock::put(&f, owner(), early).await.unwrap();
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;

    let first = place(&f, counter.clone(), place_of(&session, &[(&id, 3)]))
        .await
        .unwrap();
    let lots_after_first = lots_of(&f, &id);
    f.clock.advance(1_000);
    let second = place(&f, counter.clone(), place_of(&session, &[(&id, 4)]))
        .await
        .unwrap();
    let order = text(&orders_of(&f, &session)[0], "id");
    assert_eq!(held(&f, &id).0, 5);
    f.clock.advance(1_000);

    unplace(&f, counter.clone(), &order, &second.intent_id)
        .await
        .unwrap();
    // The second placement's lines and stock are gone; the first stays.
    assert_eq!(held(&f, &id).0, 9);
    assert_eq!(lots_of(&f, &id), lots_after_first);
    let lines = lines_of(&f, &order);
    assert_eq!(lines.len(), 1);
    assert_eq!(text(&lines[0], "placement_id"), first.intent_id);
    let o = &orders_of(&f, &session)[0];
    assert_eq!(int(o, "total_minor"), 12_000);
    assert_eq!(int(o, "subtotal_minor"), 12_000);
    let back = moves_of(&f, &id, "sale_reverted");
    assert_eq!(back.len(), 1);
    assert_eq!(
        (back[0].delta, back[0].placement_id.clone()),
        (4, Some(second.intent_id.clone()))
    );
    assert_eq!(moves_of(&f, &id, "sale").len(), 2, "the ledger keeps both");

    // The same placement cannot be undone twice.
    assert_eq!(
        code(unplace(&f, counter.clone(), &order, &second.intent_id).await),
        ErrorCode::ValidationFailed
    );

    // Undoing the only placement removes the order too.
    unplace(&f, counter, &order, &first.intent_id)
        .await
        .unwrap();
    assert!(orders_of(&f, &session).is_empty());
    assert!(table(&f, Table::OrderLines).is_empty());
    assert_eq!(held(&f, &id).0, 12);
}

#[tokio::test]
async fn a_placement_undo_is_refused_late_by_other_staff_after_an_effect_or_stale() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let (session, _) = running(&f).await;
    let asha = staff(&f, "asha", ROLE_COUNTER).await;
    let ravi = staff(&f, "ravi", ROLE_COUNTER).await;

    let a = place(&f, asha.clone(), place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
    let order = text(&orders_of(&f, &session)[0], "id");
    assert_eq!(
        code(unplace(&f, ravi.clone(), &order, &a.intent_id).await),
        ErrorCode::UndoNotAllowed
    );
    record_effect(&f, arena_proto::EffectKind::ReceiptPrinted, "", &order).await;
    assert_eq!(
        code(unplace(&f, asha.clone(), &order, &a.intent_id).await),
        ErrorCode::UndoNotAllowed
    );
    let before = changes(&f);
    let mut stale = unplace_intent(asha.clone(), &order, &a.intent_id, Some(99));
    stale.issued_at = f.clock.now_ms();
    assert_eq!(code(f.writer.submit_intent(stale).await), ErrorCode::Stale);
    let mut bare = unplace_intent(asha.clone(), &order, &a.intent_id, None);
    bare.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(bare).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    assert_eq!(changes(&f), before);

    // Late: a fresh order past eight seconds.
    let (other, _) = running(&f).await;
    let b = place(&f, asha.clone(), place_of(&other, &[(&id, 1)]))
        .await
        .unwrap();
    let other_order = text(&orders_of(&f, &other)[0], "id");
    f.clock.advance(9_000);
    assert_eq!(
        code(unplace(&f, asha, &other_order, &b.intent_id).await),
        ErrorCode::UndoNotAllowed
    );
    assert_eq!(held(&f, &id).0, 18);
}

#[tokio::test]
async fn short_stock_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let id = cola(&f, 1).await;
    let (session, _) = running(&f).await;
    let before = changes(&f);
    let e = place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::OutOfStock, Some("lines[0]"))
    );
    assert_eq!(changes(&f), before);
    assert!(orders_of(&f, &session).is_empty());
    assert_eq!(held(&f, &id).0, 1);

    // Expired stock is never sold.
    let old = make(&f, priced("Milk", 3_000, true)).await;
    let mut lot = super::stock::sin(&old, 4, 1_000);
    lot.expiry = Some("2027-01-15".into());
    super::stock::put(&f, owner(), lot).await.unwrap();
    sql(
        &f,
        "UPDATE stock_lots SET expiry = '2027-01-14' WHERE expiry IS NOT NULL".into(),
    )
    .await;
    assert_eq!(
        code(place(&f, owner(), place_of(&session, &[(&old, 1)])).await),
        ErrorCode::OutOfStock
    );
}

#[tokio::test]
async fn a_short_bundle_names_its_component_and_a_manager_override_raises_it() {
    let f = bootstrapped().await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 1, 500).await;
    let bundle = combo(&f, &chips, 2).await;
    let (session, _) = running(&f).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;

    let e = place(&f, counter.clone(), place_of(&session, &[(&bundle, 1)]))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::BundleShort, Some("lines[0]"))
    );
    assert!(e.message.contains("Chips"), "{}", e.message);

    let mut over = place_of(&session, &[(&bundle, 1)]);
    over.stock_override = Some(true);
    let before = changes(&f);
    assert_eq!(
        code(place(&f, counter.clone(), over.clone()).await),
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);

    let mut approved = intent(counter.clone(), IntentPayload::OrderPlace(over));
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
    assert_eq!(
        (
            adjust[0].delta,
            adjust[0].note.clone(),
            adjust[0].elevated_by.clone()
        ),
        (1, Some("Override at sale".to_owned()), Some(manager))
    );
    let sold = moves_of(&f, &chips, "sale");
    assert_eq!(
        (sold.len(), sold[0].delta, sold[0].placement_id.clone()),
        (1, -2, Some(a.intent_id))
    );
    // The bundle is one line at its own price.
    let order = text(&orders_of(&f, &session)[0], "id");
    let lines = lines_of(&f, &order);
    assert_eq!(
        (text(&lines[0], "label"), int(&lines[0], "amount_minor")),
        ("Combo".to_owned(), 5_000)
    );
}

#[tokio::test]
async fn only_a_running_postpaid_or_block_session_takes_an_order() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let item = |s: &str| place_of(s, &[(&id, 1)]);

    // Unknown session.
    assert_eq!(
        refusal(place(&f, owner(), item("nope")).await),
        (ErrorCode::NotFound, Some("session_id".into()))
    );

    // An ended session.
    let (ended, _) = running(&f).await;
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&ended, Some(int(&s, "rev"))))
        .await
        .unwrap();
    assert_eq!(
        refusal(place(&f, owner(), item(&ended)).await),
        (ErrorCode::ValidationFailed, Some("session_id".into()))
    );

    // A wallet session.
    let p = player(&f, "asha", "9000000001").await;
    topped_up(&f, &p, 50_000).await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let wallet = wallet_session(&f, &d, &p).await;
    assert_eq!(
        refusal(place(&f, owner(), item(&wallet)).await),
        (ErrorCode::ValidationFailed, Some("session_id".into()))
    );

    // A grouped session.
    let (host, _) = running(&f).await;
    let (member, _) = running(&f).await;
    let rev = rev_of(&f, Table::Sessions, &host);
    let mut g = intent(
        owner(),
        IntentPayload::SessionGroup(SessionGroup {
            session_id: host.clone(),
            member_ids: vec![member.clone()],
        }),
    );
    g.base_rev = Some(rev);
    g.issued_at = f.clock.now_ms();
    f.writer.submit_intent(g).await.unwrap();
    for s in [&host, &member] {
        assert_eq!(
            refusal(place(&f, owner(), item(s)).await),
            (ErrorCode::ValidationFailed, Some("session_id".into()))
        );
    }
    assert!(
        table(&f, Table::OrderLines)
            .iter()
            .all(|l| l["product_id"] == RowValue::Null)
    );
    assert_eq!(held(&f, &id).0, 20);
}

#[tokio::test]
async fn a_session_with_an_order_cannot_join_a_group_bill() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let (host, _) = running(&f).await;
    let (member, _) = running(&f).await;
    place(&f, owner(), place_of(&member, &[(&id, 1)]))
        .await
        .unwrap();
    let rev = rev_of(&f, Table::Sessions, &host);
    let mut g = intent(
        owner(),
        IntentPayload::SessionGroup(SessionGroup {
            session_id: host,
            member_ids: vec![member],
        }),
    );
    g.base_rev = Some(rev);
    g.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(g).await),
        (ErrorCode::ValidationFailed, Some("member_ids".into()))
    );
}

#[tokio::test]
async fn the_lines_are_checked_like_a_counter_sale() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let off = make(
        &f,
        ProductUpsert {
            active: false,
            ..priced("Old", 1_000, false)
        },
    )
    .await;
    let (session, _) = running(&f).await;
    let field = |r: Result<Applied, AppError>| refusal(r);
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[])).await),
        (ErrorCode::ValidationFailed, Some("lines".into()))
    );
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[(&id, 0)])).await),
        (ErrorCode::ValidationFailed, Some("lines[0]".into()))
    );
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[(&id, 1000)])).await),
        (ErrorCode::ValidationFailed, Some("lines[0]".into()))
    );
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[(&id, 1), (&id, 1)])).await),
        (ErrorCode::ValidationFailed, Some("lines[1]".into()))
    );
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[(&off, 1)])).await),
        (ErrorCode::ValidationFailed, Some("lines[0]".into()))
    );
    assert_eq!(
        field(place(&f, owner(), place_of(&session, &[("nope", 1)])).await),
        (ErrorCode::ValidationFailed, Some("lines[0]".into()))
    );
    // A placement takes no base_rev.
    let mut with_rev = intent(
        owner(),
        IntentPayload::OrderPlace(place_of(&session, &[(&id, 1)])),
    );
    with_rev.base_rev = Some(0);
    with_rev.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(with_rev).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn kitchen_staff_may_not_place_and_a_counter_may() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let (session, _) = running(&f).await;
    let kitchen = staff(&f, "kit", crate::rbac::ROLE_KITCHEN).await;
    assert_eq!(
        code(place(&f, kitchen, place_of(&session, &[(&id, 1)])).await),
        ErrorCode::PermissionDenied
    );
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    place(&f, counter, place_of(&session, &[(&id, 1)]))
        .await
        .unwrap();
}

#[tokio::test]
async fn one_settle_covers_the_time_and_the_products_and_a_running_session_cannot_settle() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let (session, _) = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let order = text(&orders_of(&f, &session)[0], "id");
    // While the session runs its bill cannot be settled.
    let rev = rev_of(&f, Table::Orders, &order);
    assert_eq!(
        refusal(
            end(
                &f,
                settle_intent(&order, Some(rev), vec![pay(Cash, 8_000, None)])
            )
            .await
        ),
        (ErrorCode::ValidationFailed, Some("order_id".into()))
    );

    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let rev = rev_of(&f, Table::Orders, &order);
    // Only the time or only the products is short.
    for short in [10_000, 8_000] {
        assert_eq!(
            code(
                end(
                    &f,
                    settle_intent(&order, Some(rev), vec![pay(Cash, short, None)])
                )
                .await
            ),
            ErrorCode::PaymentShort
        );
    }
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 18_000, None)]),
    )
    .await
    .unwrap();
    let o = &orders_of(&f, &session)[0];
    assert_eq!(
        (text(o, "status"), int(o, "total_minor")),
        ("settled".to_owned(), 18_000)
    );
    assert_eq!(table(&f, Table::Payments).len(), 1);
    // A settled bill takes no more.
    assert_eq!(
        code(place(&f, owner(), place_of(&session, &[(&id, 1)])).await),
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn a_block_session_takes_an_order_on_its_open_bill_and_ending_leaves_it() {
    let f = bootstrapped().await;
    let id = cola(&f, 5).await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:45"));
    begin_block(&f, &d, None, &plan, SessionMode::Block)
        .await
        .unwrap();
    let session = text(&sessions(&f)[0], "id");
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let orders = orders_of(&f, &session);
    assert_eq!(orders.len(), 1, "the block bill is the order");
    let order = text(&orders[0], "id");
    assert_eq!(int(&orders[0], "total_minor"), 28_000);
    let lines = lines_of(&f, &order);
    assert_eq!(lines.len(), 2);
    assert_eq!(int(&lines[0], "amount_minor"), 20_000);
    assert_eq!(text(&lines[1], "label"), "Cola");

    // The block bill settles while the session runs, for the whole total.
    let rev = rev_of(&f, Table::Orders, &order);
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 28_000, None)]),
    )
    .await
    .unwrap();
    // Once settled the bill takes no more.
    assert_eq!(
        code(place(&f, owner(), place_of(&session, &[(&id, 1)])).await),
        ErrorCode::ValidationFailed
    );
}

#[tokio::test]
async fn the_order_total_equals_the_sum_of_its_lines_after_an_undo() {
    let f = bootstrapped().await;
    let id = cola(&f, 20).await;
    let (session, _) = running(&f).await;
    let a = place(&f, owner(), place_of(&session, &[(&id, 3)]))
        .await
        .unwrap();
    place(&f, owner(), place_of(&session, &[(&id, 2)]))
        .await
        .unwrap();
    let order = text(&orders_of(&f, &session)[0], "id");
    unplace(&f, owner(), &order, &a.intent_id).await.unwrap();
    let o = &orders_of(&f, &session)[0];
    let sum: i64 = lines_of(&f, &order)
        .iter()
        .map(|l| int(l, "amount_minor"))
        .sum();
    assert_eq!((int(o, "total_minor"), sum), (8_000, 8_000));
}

#[tokio::test]
async fn the_bill_and_the_kiosk_screen_show_what_was_ordered() {
    let f = bootstrapped().await;
    let id = cola(&f, 10).await;
    let chips = make(&f, priced("Chips", 1_000, true)).await;
    stock_in(&f, &chips, 10, 500).await;
    let (session, _) = running(&f).await;
    let screens = |f: &Fixture| crate::session_screens(&reopen(f), f.clock.now_ms()).unwrap();
    assert_eq!(screens(&f)[0].1.ordered_items, 0);

    place(&f, owner(), place_of(&session, &[(&id, 2), (&chips, 3)]))
        .await
        .unwrap();
    assert_eq!(screens(&f)[0].1.ordered_items, 5);

    // A running session shows its products on its own live bill, not as a second bill.
    f.clock.advance(60 * 60_000);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.bills.len(), 1);
    let b = &snap.bills[0];
    assert_eq!(b.order_id, None);
    assert_eq!(
        b.items
            .iter()
            .map(|i| (i.label.as_str(), i.qty, i.amount_minor))
            .collect::<Vec<_>>(),
        [("Cola", 2, 8_000), ("Chips", 3, 3_000)]
    );
    assert_eq!(b.lines.len(), 1);
    assert_eq!(b.subtotal_minor, 10_000 + 11_000);

    // Once ended, the one open order carries the segments as lines and the products as items.
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.bills.len(), 1);
    let b = &snap.bills[0];
    assert!(b.order_id.is_some());
    assert_eq!((b.lines.len(), b.items.len()), (1, 2));
    assert_eq!(b.subtotal_minor, 21_000);
}

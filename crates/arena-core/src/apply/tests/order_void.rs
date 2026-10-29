//! Story 5.23: void an unsettled order with a reason and an approver.

use super::session::{
    end, end_intent, int, pay, pc, refusal, sessions, settle_intent, start, submit, table, text,
};
use super::stock::{held, intent, make, manager_with_pin, owner, staff, stock_in, up};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    DeviceKind, ElevatedBy, ElevationKind, ElevationMode, KEY_INVENTORY_TAKE_STOCK_AT, OrderPlace,
    OrderPlaceReverted, OrderVoid, PosSaleLine, ProductUpsert,
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

async fn place(f: &Fixture, s: &str, lines: &[(&str, i64)]) -> Applied {
    submit(f, owner(), IntentPayload::OrderPlace(place_of(s, lines)))
        .await
        .unwrap()
}

fn void_intent(
    actor: Actor,
    order: &str,
    reason: Option<ReasonCode>,
    note: Option<&str>,
    approver: Option<&str>,
) -> Intent {
    let mut i = intent(
        actor,
        IntentPayload::OrderVoid(OrderVoid {
            order_id: order.into(),
            note: note.map(str::to_owned),
        }),
    );
    i.reason = reason;
    i.elevated_by = approver.map(|id| ElevatedBy {
        kind: ElevationKind::Pin,
        id: id.into(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i
}

async fn void(f: &Fixture, i: Intent) -> Result<Applied, AppError> {
    let mut i = i;
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn set_mode(f: &Fixture, value: &str) {
    let row = table(f, Table::Settings)
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

fn order_by_id(f: &Fixture, id: &str) -> Row {
    table(f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == id)
        .unwrap()
}

fn lines_of(f: &Fixture, order: &str) -> Vec<Row> {
    table(f, Table::OrderLines)
        .into_iter()
        .filter(|l| text(l, "order_id") == order)
        .collect()
}

fn open_holds(f: &Fixture) -> usize {
    table(f, Table::StockHolds)
        .into_iter()
        .filter(|h| h["released_at"] == RowValue::Null)
        .count()
}

fn moves_of(f: &Fixture, id: &str, reason: &str) -> usize {
    reopen(f)
        .moves(id)
        .unwrap()
        .into_iter()
        .filter(|m| m.reason == reason)
        .count()
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

#[tokio::test]
async fn a_void_closes_holds_returns_taken_stock_and_marks_the_order() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&b, 2), (&c, 1)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    // Served mode: the burger is held, the cola was taken at placement.
    assert_eq!((open_holds(&f), held(&f, &b).0, held(&f, &c).0), (1, 5, 4));

    f.clock.advance(30_000);
    let a = void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::Mistake), None, None),
    )
    .await
    .unwrap();
    let tables: Vec<&str> = a.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert!(tables.contains(&"orders") && tables.contains(&"stock_holds"));

    let o = order_by_id(&f, &order);
    assert_eq!(text(&o, "status"), "void");
    assert_eq!(o["session_id"], RowValue::Null);
    assert_eq!(text(&o, "void_session_id"), session);
    assert_eq!(text(&o, "void_reason"), "mistake");
    assert_eq!(o["void_note"], RowValue::Null);
    assert_eq!(text(&o, "voided_by"), "o");
    assert_eq!(int(&o, "voided_at"), f.clock.now_ms());
    assert_eq!(int(&o, "total_minor"), 22_000, "the voided amount is kept");
    assert_eq!(
        lines_of(&f, &order).len(),
        2,
        "the lines stay for the record"
    );
    assert!(order_of(&f, &session).is_none(), "the session has no bill");

    assert_eq!(open_holds(&f), 0, "the burger hold is closed");
    assert_eq!((held(&f, &b).0, held(&f, &c).0), (5, 5));
    assert_eq!(moves_of(&f, &c, "return"), 1);
    assert_eq!(moves_of(&f, &b, "return"), 0, "a held line took no stock");
}

#[tokio::test]
async fn a_void_returns_a_served_kitchen_line_and_an_order_mode_line() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&b, 2)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let line = text(&lines_of(&f, &order)[0], "id");
    submit(
        &f,
        owner(),
        IntentPayload::OrderLineServed(arena_proto::OrderLineServed { line_id: line }),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &b).0, 3);
    void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::ChangedMind), None, None),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &b).0, 5, "the served units are back");

    // In order mode stock was taken at placement even though the line is unserved.
    set_mode(&f, "order").await;
    place(&f, &session, &[(&b, 1)]).await;
    assert_eq!(held(&f, &b).0, 4);
    let order = text(&order_of(&f, &session).unwrap(), "id");
    void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::Mistake), None, None),
    )
    .await
    .unwrap();
    assert_eq!(held(&f, &b).0, 5);
}

#[tokio::test]
async fn the_reason_is_required_and_other_needs_a_note() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&c, 1)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let before = changes(&f);

    let none = void(&f, void_intent(owner(), &order, None, None, None)).await;
    assert_eq!(refusal(none).0, ErrorCode::ReasonRequired);
    let wrong = void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::Lost), None, None),
    )
    .await;
    assert_eq!(refusal(wrong).0, ErrorCode::ValidationFailed);
    for note in [None, Some("   "), Some("bad\u{7}text")] {
        let r = void(
            &f,
            void_intent(owner(), &order, Some(ReasonCode::Other), note, None),
        )
        .await;
        assert_eq!(
            refusal(r),
            (ErrorCode::ValidationFailed, Some("note".into()))
        );
    }
    let long = "x".repeat(141);
    let r = void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::Other), Some(&long), None),
    )
    .await;
    assert_eq!(
        refusal(r),
        (ErrorCode::ValidationFailed, Some("note".into()))
    );
    assert_eq!(changes(&f), before, "nothing was written");

    void(
        &f,
        void_intent(
            owner(),
            &order,
            Some(ReasonCode::Other),
            Some("  Wrong table  "),
            None,
        ),
    )
    .await
    .unwrap();
    let o = order_by_id(&f, &order);
    assert_eq!(
        (text(&o, "void_reason"), text(&o, "void_note")),
        ("other".into(), "Wrong table".into())
    );
}

#[tokio::test]
async fn a_note_is_kept_only_for_other() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&c, 1)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    void(
        &f,
        void_intent(
            owner(),
            &order,
            Some(ReasonCode::ChangedMind),
            Some("ignored"),
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(order_by_id(&f, &order)["void_note"], RowValue::Null);
}

#[tokio::test]
async fn a_void_needs_pos_void_on_the_actor_or_the_approving_manager() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&c, 1)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    let before = changes(&f);

    let alone = void_intent(
        counter.clone(),
        &order,
        Some(ReasonCode::Mistake),
        None,
        None,
    );
    assert_eq!(
        refusal(void(&f, alone).await).0,
        ErrorCode::PermissionDenied
    );
    // A staff member without `pos.void` cannot approve it either.
    let by_peer = void_intent(
        counter.clone(),
        &order,
        Some(ReasonCode::Mistake),
        None,
        Some(&counter.id),
    );
    assert_eq!(
        refusal(void(&f, by_peer).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(changes(&f), before);

    let approved = void_intent(
        counter.clone(),
        &order,
        Some(ReasonCode::Mistake),
        None,
        Some(&manager),
    );
    void(&f, approved).await.unwrap();
    assert_eq!(text(&order_by_id(&f, &order), "voided_by"), manager);
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(change.actor_id, counter.id);
    assert_eq!(change.elevated_by_id.as_deref(), Some(manager.as_str()));
}

#[tokio::test]
async fn a_settled_a_voided_or_a_missing_order_is_refused_and_nothing_changes() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&c, 1)]).await;
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let mistake = |o: &str| void_intent(owner(), o, Some(ReasonCode::Mistake), None, None);

    // Voided already: STALE.
    void(&f, mistake(&order)).await.unwrap();
    let before = changes(&f);
    assert_eq!(refusal(void(&f, mistake(&order)).await).0, ErrorCode::Stale);

    // Settled: ORDER_SETTLED.
    place(&f, &session, &[(&c, 1)]).await;
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let fresh = text(&order_of(&f, &session).unwrap(), "id");
    let o = order_by_id(&f, &fresh);
    end(
        &f,
        settle_intent(&fresh, Some(int(&o, "rev")), vec![pay(Cash, 14_000, None)]),
    )
    .await
    .unwrap();
    let settled = changes(&f);
    assert_eq!(
        refusal(void(&f, mistake(&fresh)).await).0,
        ErrorCode::OrderSettled
    );
    assert_eq!(changes(&f), settled);
    assert!(settled > before);

    assert_eq!(
        refusal(void(&f, mistake("nope")).await).0,
        ErrorCode::NotFound
    );
    let mut with_rev = mistake(&order);
    with_rev.base_rev = Some(1);
    assert_eq!(
        refusal(void(&f, with_rev).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
}

#[tokio::test]
async fn a_bill_with_play_time_cannot_be_voided() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, &session, &[(&c, 1)]).await;
    f.clock.advance(60 * 60_000);
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let before = changes(&f);
    let r = void(
        &f,
        void_intent(owner(), &order, Some(ReasonCode::Mistake), None, None),
    )
    .await;
    assert_eq!(
        refusal(r),
        (ErrorCode::ValidationFailed, Some("order_id".into()))
    );
    assert_eq!(changes(&f), before);
    assert_eq!(held(&f, &c).0, 4, "the stock was not given back");
}

#[tokio::test]
async fn after_a_void_the_session_opens_a_fresh_bill_and_the_old_placement_cannot_be_undone() {
    let f = bootstrapped().await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    let placed = place(&f, &session, &[(&c, 1)]).await;
    let old = text(&order_of(&f, &session).unwrap(), "id");
    void(
        &f,
        void_intent(owner(), &old, Some(ReasonCode::Mistake), None, None),
    )
    .await
    .unwrap();

    let mut undo = intent(
        owner(),
        IntentPayload::OrderPlaceReverted(OrderPlaceReverted {
            order_id: old.clone(),
            placement_id: placed.intent_id.clone(),
        }),
    );
    undo.base_rev = Some(int(&order_by_id(&f, &old), "rev"));
    undo.issued_at = f.clock.now_ms();
    assert_eq!(
        refusal(f.writer.submit_intent(undo).await).0,
        ErrorCode::ValidationFailed
    );

    place(&f, &session, &[(&c, 2)]).await;
    let fresh = text(&order_of(&f, &session).unwrap(), "id");
    assert_ne!(fresh, old);
    assert_eq!(int(&order_by_id(&f, &fresh), "total_minor"), 8_000);
    assert_eq!(text(&order_by_id(&f, &old), "status"), "void");
}

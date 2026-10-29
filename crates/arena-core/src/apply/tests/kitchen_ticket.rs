//! Story 7.3: every placement with kitchen lines opens a kitchen ticket the Hub can move.

use super::order_line::{
    burger, cancel, code, cola, line, order_of, place, place_of, running, serve,
};
use super::prepaid::sql;
use super::session::{int, pay, pc, rev_of, submit, table, text};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    DeviceKind, KitchenAdvance, KitchenAdvanceReverted, OrderPlaceReverted, OrderReady, PosSale,
    PosSaleLine, PosSaleReverted,
};

pub(super) async fn flags(f: &Fixture, value: &str) {
    sql(
        f,
        format!("UPDATE settings SET value = '{value}' WHERE key = 'licence.flags'"),
    )
    .await;
}

/// A Basic branch with a running session and kitchen stock.
pub(super) async fn basic(f: &Fixture) -> (String, String) {
    flags(f, "kitchen").await;
    let b = burger(f, 10).await;
    let session = running(f).await;
    (b, session)
}

pub(super) fn tickets(f: &Fixture) -> Vec<Row> {
    let mut t = table(f, Table::KitchenTickets);
    t.sort_by_key(|r| int(r, "number"));
    t
}

fn ticket_lines(f: &Fixture, ticket: &str) -> Vec<Row> {
    table(f, Table::KitchenTicketLines)
        .into_iter()
        .filter(|l| text(l, "ticket_id") == ticket)
        .collect()
}

fn stage(f: &Fixture, ticket: &str) -> String {
    text(
        tickets(f).iter().find(|t| text(t, "id") == ticket).unwrap(),
        "stage",
    )
}

async fn advance_with(
    f: &Fixture,
    actor: Actor,
    ticket: &str,
    to: &str,
    rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::KitchenAdvance(KitchenAdvance {
            ticket_id: ticket.into(),
            to: to.into(),
        }),
    );
    i.base_rev = rev;
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

pub(super) async fn advance(f: &Fixture, ticket: &str, to: &str) -> Result<Applied, AppError> {
    let rev = rev_of(f, Table::KitchenTickets, ticket);
    advance_with(f, owner(), ticket, to, Some(rev)).await
}

async fn undo_with(f: &Fixture, actor: Actor, ticket: &str) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::KitchenAdvanceReverted(KitchenAdvanceReverted {
            ticket_id: ticket.into(),
        }),
    );
    i.base_rev = Some(rev_of(f, Table::KitchenTickets, ticket));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn undo(f: &Fixture, ticket: &str) -> Result<Applied, AppError> {
    undo_with(f, owner(), ticket).await
}

fn burger_line(product: &str, qty: i64, notes: &[&str]) -> PosSaleLine {
    PosSaleLine {
        product_id: product.into(),
        qty,
        kitchen_notes: notes.iter().map(|n| (*n).to_owned()).collect(),
    }
}

fn open_holds(f: &Fixture) -> usize {
    table(f, Table::StockHolds)
        .into_iter()
        .filter(|h| h["released_at"] == RowValue::Null)
        .count()
}

fn on_hand(f: &Fixture, product: &str) -> i64 {
    reopen(f).stock(product).unwrap().unwrap().qty
}

fn ready_at(f: &Fixture, session: &str) -> RowValue {
    order_of(f, session).unwrap()["ready_at"].clone()
}

#[tokio::test]
async fn a_placement_with_kitchen_lines_opens_one_ticket_with_only_those_lines() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    let c = cola(&f, 5).await;
    let mut p = place_of(&session, &[(&b, 2), (&c, 1)]);
    p.lines[0].kitchen_notes = vec!["No onion".into()];
    place(&f, owner(), p).await.unwrap();

    let t = tickets(&f);
    assert_eq!(t.len(), 1);
    assert_eq!(
        (
            int(&t[0], "number"),
            text(&t[0], "stage"),
            text(&t[0], "seat")
        ),
        (1, "new".to_owned(), "PC".to_owned())
    );
    assert_eq!(
        text(&t[0], "order_id"),
        text(&order_of(&f, &session).unwrap(), "id")
    );
    let lines = ticket_lines(&f, &text(&t[0], "id"));
    assert_eq!(lines.len(), 1);
    assert_eq!(text(&lines[0], "label"), "Burger");
    assert_eq!(int(&lines[0], "qty"), 2);
    assert_eq!(text(&lines[0], "notes"), r#"["No onion"]"#);
    assert_eq!(
        text(&lines[0], "order_line_id"),
        text(&line(&f, &session, "Burger"), "id")
    );

    // A placement with no kitchen line opens none; the next kitchen one takes the next number.
    place(&f, owner(), place_of(&session, &[(&c, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 1);
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let t = tickets(&f);
    assert_eq!(
        t.iter().map(|r| int(r, "number")).collect::<Vec<_>>(),
        [1, 2]
    );
}

#[tokio::test]
async fn a_walk_up_sale_opens_a_counter_ticket_and_only_its_stage_moves() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let b = burger(&f, 5).await;
    let sale = PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: vec![burger_line(&b, 1, &[])],
        payments: vec![pay(Cash, 9_000, None)],
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    };
    submit(&f, owner(), IntentPayload::PosSale(sale))
        .await
        .unwrap();
    let t = tickets(&f);
    assert_eq!(t.len(), 1);
    let id = text(&t[0], "id");
    assert_eq!(text(&t[0], "seat"), "Counter");
    assert_eq!(on_hand(&f, &b), 4);

    for to in ["preparing", "ready", "served"] {
        let a = advance(&f, &id, to).await.unwrap();
        // Only the ticket (and its order-ready alert, Story 7.4) changes: the sale was served
        // and settled at the counter.
        let expect: &[&str] = if to == "preparing" {
            &["kitchen_tickets"]
        } else {
            &["kitchen_tickets", "alerts"]
        };
        assert_eq!(
            a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
            expect,
            "{to}"
        );
    }
    assert_eq!(stage(&f, &id), "served");
    assert_eq!(on_hand(&f, &b), 4);
}

#[tokio::test]
async fn a_free_branch_opens_no_ticket_and_keeps_the_epic_five_and_six_flow() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert!(tickets(&f).is_empty());

    let order = text(&order_of(&f, &session).unwrap(), "id");
    submit(
        &f,
        owner(),
        IntentPayload::OrderReady(OrderReady { order_id: order }),
    )
    .await
    .unwrap();
    serve(&f, owner(), &text(&line(&f, &session, "Burger"), "id"))
        .await
        .unwrap();

    // Nothing to move: a Free branch refuses with TIER_LOCKED, not NOT_FOUND.
    assert_eq!(
        code(advance_with(&f, owner(), "nope", "preparing", Some(0)).await),
        ErrorCode::TierLocked
    );
    let mut i = intent(
        owner(),
        IntentPayload::KitchenAdvanceReverted(KitchenAdvanceReverted {
            ticket_id: "nope".into(),
        }),
    );
    i.base_rev = Some(0);
    i.issued_at = f.clock.now_ms();
    assert_eq!(code(f.writer.submit_intent(i).await), ErrorCode::TierLocked);
}

#[tokio::test]
async fn a_ticket_moves_one_stage_at_a_time_and_a_stale_or_skipped_move_changes_nothing() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    let rev = rev_of(&f, Table::KitchenTickets, &id);
    let before = reopen(&f).changes().unwrap().len();

    for (to, base, want) in [
        ("ready", Some(rev), ErrorCode::BadStage),
        ("served", Some(rev), ErrorCode::BadStage),
        ("new", Some(rev), ErrorCode::BadStage),
        ("bogus", Some(rev), ErrorCode::BadStage),
        ("preparing", Some(rev + 7), ErrorCode::Stale),
        ("preparing", None, ErrorCode::ValidationFailed),
    ] {
        let r = advance_with(&f, owner(), &id, to, base).await;
        assert_eq!(code(r), want, "{to} {base:?}");
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(stage(&f, &id), "new");

    advance(&f, &id, "preparing").await.unwrap();
    // The same tap again is stale: the board snaps to the Hub's state.
    assert_eq!(
        code(advance_with(&f, owner(), &id, "preparing", Some(rev)).await),
        ErrorCode::Stale
    );
}

#[tokio::test]
async fn ready_stamps_the_order_and_served_serves_the_lines_and_takes_the_stock() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 2)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    assert_eq!((on_hand(&f, &b), open_holds(&f)), (10, 1));
    assert_eq!(ready_at(&f, &session), RowValue::Null);

    advance(&f, &id, "preparing").await.unwrap();
    assert_eq!(ready_at(&f, &session), RowValue::Null);
    let a = advance(&f, &id, "ready").await.unwrap();
    assert!(a.changes.iter().any(|c| c.tbl == "orders"));
    assert_ne!(ready_at(&f, &session), RowValue::Null);

    advance(&f, &id, "served").await.unwrap();
    assert_eq!(stage(&f, &id), "served");
    assert_eq!((on_hand(&f, &b), open_holds(&f)), (8, 0));
    assert_eq!(
        line(&f, &session, "Burger")["served"],
        RowValue::Integer(1),
        "the line is served"
    );
    // Served is the end: nothing follows it.
    assert_eq!(
        code(
            advance_with(
                &f,
                owner(),
                &id,
                "served",
                Some(rev_of(&f, Table::KitchenTickets, &id))
            )
            .await
        ),
        ErrorCode::BadStage
    );
}

#[tokio::test]
async fn undoing_ready_clears_the_mark_and_undoing_served_gives_the_stock_back() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 2)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    advance(&f, &id, "preparing").await.unwrap();
    advance(&f, &id, "ready").await.unwrap();
    undo(&f, &id).await.unwrap();
    assert_eq!(stage(&f, &id), "preparing");
    assert_eq!(ready_at(&f, &session), RowValue::Null);

    advance(&f, &id, "ready").await.unwrap();
    advance(&f, &id, "served").await.unwrap();
    assert_eq!((on_hand(&f, &b), open_holds(&f)), (8, 0));
    undo(&f, &id).await.unwrap();
    assert_eq!(stage(&f, &id), "ready");
    assert_eq!((on_hand(&f, &b), open_holds(&f)), (10, 1));
    assert_eq!(line(&f, &session, "Burger")["served"], RowValue::Integer(0));

    // The undo itself cannot be undone, and the ticket serves again.
    assert_eq!(
        code(undo(&f, &id).await),
        ErrorCode::UndoNotAllowed,
        "the undo left no advance to take back"
    );
    advance(&f, &id, "served").await.unwrap();
    assert_eq!((on_hand(&f, &b), open_holds(&f)), (8, 0));
}

#[tokio::test]
async fn an_undo_needs_the_same_actor_the_window_and_a_step_to_take_back() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    assert_eq!(
        code(undo(&f, &id).await),
        ErrorCode::BadStage,
        "new has no previous stage"
    );

    advance(&f, &id, "preparing").await.unwrap();
    let other = staff(&f, "sam", ROLE_KITCHEN).await;
    assert_eq!(
        code(undo_with(&f, other, &id).await),
        ErrorCode::UndoNotAllowed
    );
    f.clock.advance(20_000);
    assert_eq!(code(undo(&f, &id).await), ErrorCode::UndoNotAllowed);
    assert_eq!(stage(&f, &id), "preparing");
}

#[tokio::test]
async fn mark_ready_and_serve_by_hand_follow_the_ticket() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let line_id = text(&line(&f, &session, "Burger"), "id");

    // While the ticket is New or Preparing the ticket, not the order, says when it is ready.
    let ready = |order: &str| {
        submit(
            &f,
            owner(),
            IntentPayload::OrderReady(OrderReady {
                order_id: order.into(),
            }),
        )
    };
    assert_eq!(code(ready(&order).await), ErrorCode::BadStage);
    assert_eq!(
        code(serve(&f, owner(), &line_id).await),
        ErrorCode::BadStage
    );
    advance(&f, &id, "preparing").await.unwrap();
    assert_eq!(code(ready(&order).await), ErrorCode::BadStage);
    advance(&f, &id, "ready").await.unwrap();

    // Serving the last unserved line of a Ready ticket serves the ticket.
    serve(&f, owner(), &line_id).await.unwrap();
    assert_eq!(stage(&f, &id), "served");
}

#[tokio::test]
async fn cancelling_lines_trims_the_ticket_and_the_last_one_cancels_it() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let b = burger(&f, 10).await;
    let w = super::stock::make(
        &f,
        arena_proto::ProductUpsert {
            price_minor: 5_000,
            kitchen: true,
            ..super::stock::up("Wrap")
        },
    )
    .await;
    super::stock::stock_in(&f, &w, 10, 1_000).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1), (&w, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    assert_eq!(ticket_lines(&f, &id).len(), 2);

    cancel(&f, owner(), &text(&line(&f, &session, "Wrap"), "id"), false)
        .await
        .unwrap();
    assert_eq!(ticket_lines(&f, &id).len(), 1);
    assert_eq!(stage(&f, &id), "new");

    cancel(
        &f,
        owner(),
        &text(&line(&f, &session, "Burger"), "id"),
        false,
    )
    .await
    .unwrap();
    assert!(ticket_lines(&f, &id).is_empty());
    let t = tickets(&f);
    assert_eq!(
        (text(&t[0], "stage"), text(&t[0], "cancel_reason")),
        ("cancelled".to_owned(), "changed_mind".to_owned())
    );
    assert_eq!(
        code(advance(&f, &id, "preparing").await),
        ErrorCode::BadStage
    );
}

#[tokio::test]
async fn undoing_a_placement_removes_its_ticket_until_the_kitchen_starts() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    let placed = place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let unplace = |placement: String| {
        let f = &f;
        let order = order.clone();
        async move {
            let mut i = intent(
                owner(),
                IntentPayload::OrderPlaceReverted(OrderPlaceReverted {
                    order_id: order.clone(),
                    placement_id: placement,
                }),
            );
            i.base_rev = Some(rev_of(f, Table::Orders, &order));
            i.issued_at = f.clock.now_ms();
            f.writer.submit_intent(i).await
        }
    };
    let placement = placed.intent_id.clone();
    let id = text(&tickets(&f)[0], "id");
    advance(&f, &id, "preparing").await.unwrap();
    assert_eq!(
        code(unplace(placement.clone()).await),
        ErrorCode::UndoNotAllowed
    );
    undo(&f, &id).await.unwrap();
    unplace(placement).await.unwrap();
    assert!(tickets(&f).is_empty());
    assert!(table(&f, Table::KitchenTicketLines).is_empty());
}

#[tokio::test]
async fn a_voided_order_cancels_its_live_tickets() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    advance(&f, &id, "preparing").await.unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let mut i = intent(
        owner(),
        IntentPayload::OrderVoid(arena_proto::OrderVoid {
            order_id: order,
            note: None,
        }),
    );
    i.reason = Some(ReasonCode::Mistake);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    let t = tickets(&f);
    assert_eq!(
        (text(&t[0], "stage"), text(&t[0], "cancel_reason")),
        ("cancelled".to_owned(), "mistake".to_owned())
    );
    assert!(ticket_lines(&f, &id).is_empty());
}

#[tokio::test]
async fn undoing_a_sale_removes_its_ticket() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let b = burger(&f, 5).await;
    let sale = PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: vec![burger_line(&b, 1, &[])],
        payments: vec![pay(Cash, 9_000, None)],
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    };
    submit(&f, owner(), IntentPayload::PosSale(sale))
        .await
        .unwrap();
    let order = text(
        table(&f, Table::Orders)
            .iter()
            .find(|o| text(o, "kind") == "pos_sale")
            .unwrap(),
        "id",
    );
    let mut i = intent(
        owner(),
        IntentPayload::PosSaleReverted(PosSaleReverted {
            order_id: order.clone(),
        }),
    );
    i.base_rev = Some(rev_of(&f, Table::Orders, &order));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    assert!(tickets(&f).is_empty());
}

#[tokio::test]
async fn a_ticket_opened_before_a_lapse_still_finishes_but_no_new_one_opens() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    flags(&f, "").await;

    advance(&f, &id, "preparing").await.unwrap();
    advance(&f, &id, "ready").await.unwrap();
    // The branch is Free now: a new kitchen placement opens no ticket.
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 1);
    assert_eq!(
        code(advance_with(&f, owner(), "other", "preparing", Some(0)).await),
        ErrorCode::TierLocked
    );
}

#[tokio::test]
async fn the_kitchen_role_a_counter_and_a_display_may_advance_and_nobody_else() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    let kitchen = staff(&f, "kay", ROLE_KITCHEN).await;
    let counter = staff(&f, "cam", ROLE_COUNTER).await;
    let rev = || rev_of(&f, Table::KitchenTickets, &id);
    advance_with(&f, kitchen, &id, "preparing", Some(rev()))
        .await
        .unwrap();
    advance_with(&f, counter, &id, "ready", Some(rev()))
        .await
        .unwrap();

    // A device acts as a display.
    let display = pc(&f, DeviceKind::Pc, "pc", None).await;
    advance_with(
        &f,
        Actor {
            kind: ActorKind::Device,
            id: display,
        },
        &id,
        "served",
        Some(rev()),
    )
    .await
    .unwrap();
    assert_eq!(stage(&f, &id), "served");
}

// ---- Story 7.4: the order-ready alert ----

fn ready_alerts(f: &Fixture) -> Vec<Row> {
    table(f, Table::Alerts)
        .into_iter()
        .filter(|a| text(a, "kind") == "order_ready" && a["acked_at"] == RowValue::Null)
        .collect()
}

#[tokio::test]
async fn a_device_ticket_reaching_ready_raises_one_alert_on_its_device_and_the_undo_closes_it() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let id = text(&tickets(&f)[0], "id");
    advance(&f, &id, "preparing").await.unwrap();
    assert!(ready_alerts(&f).is_empty());

    advance(&f, &id, "ready").await.unwrap();
    let open = ready_alerts(&f);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["ticket_id"], RowValue::Null);
    assert_eq!(
        text(&open[0], "device_id"),
        text(&tickets(&f)[0], "device_id")
    );
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.alerts[0].kind, arena_proto::AlertKind::OrderReady);

    undo(&f, &id).await.unwrap();
    assert!(ready_alerts(&f).is_empty());
    assert_eq!(stage(&f, &id), "preparing");

    advance(&f, &id, "ready").await.unwrap();
    assert_eq!(ready_alerts(&f).len(), 1);
    advance(&f, &id, "served").await.unwrap();
    assert!(ready_alerts(&f).is_empty(), "serving closes the alert");
}

#[tokio::test]
async fn a_walk_up_ticket_raises_a_branch_alert_per_ticket() {
    let f = bootstrapped().await;
    flags(&f, "kitchen").await;
    let b = burger(&f, 5).await;
    let sale = || PosSale {
        kiosk: None,
        business: None,
        coupon: None,
        discount: None,
        lines: vec![burger_line(&b, 1, &[])],
        payments: vec![pay(Cash, 9_000, None)],
        player_id: None,
        credit_override: None,
        stock_override: None,
        receipt: None,
    };
    submit(&f, owner(), IntentPayload::PosSale(sale()))
        .await
        .unwrap();
    submit(&f, owner(), IntentPayload::PosSale(sale()))
        .await
        .unwrap();
    let ids: Vec<String> = tickets(&f).iter().map(|t| text(t, "id")).collect();
    for id in &ids {
        advance(&f, id, "preparing").await.unwrap();
        advance(&f, id, "ready").await.unwrap();
    }
    let open = ready_alerts(&f);
    assert_eq!(open.len(), 2);
    assert!(open.iter().all(|a| a["device_id"] == RowValue::Null));

    undo(&f, &ids[1]).await.unwrap();
    let open = ready_alerts(&f);
    assert_eq!(open.len(), 1);
    assert_eq!(text(&open[0], "ticket_id"), ids[0]);
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert_eq!(snap.counter_tickets.len(), 2);
    assert_eq!(snap.alerts[0].ticket_number, Some(1));
}

#[tokio::test]
async fn a_device_alert_stays_while_another_ticket_of_the_device_is_ready() {
    let f = bootstrapped().await;
    let (b, session) = basic(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let ids: Vec<String> = tickets(&f).iter().map(|t| text(t, "id")).collect();
    assert_eq!(ids.len(), 2);
    for id in &ids {
        advance(&f, id, "preparing").await.unwrap();
        advance(&f, id, "ready").await.unwrap();
    }
    assert_eq!(ready_alerts(&f).len(), 1, "one alert per device");
    advance(&f, &ids[0], "served").await.unwrap();
    assert_eq!(ready_alerts(&f).len(), 1, "the other ticket is still ready");
    advance(&f, &ids[1], "served").await.unwrap();
    assert!(ready_alerts(&f).is_empty());
}

#[tokio::test]
async fn a_free_branch_raises_no_order_ready_alert_for_the_epic_six_mark_ready() {
    let f = bootstrapped().await;
    let b = burger(&f, 10).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    submit(
        &f,
        owner(),
        IntentPayload::OrderReady(OrderReady { order_id: order }),
    )
    .await
    .unwrap();
    assert!(tickets(&f).is_empty());
    assert!(ready_alerts(&f).is_empty());
    let snap = crate::snapshot::read_snapshot(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(snap.alerts.is_empty() && snap.counter_tickets.is_empty());
}

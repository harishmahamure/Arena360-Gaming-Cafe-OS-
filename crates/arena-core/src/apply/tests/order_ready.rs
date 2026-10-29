use super::order_line::{
    burger, cancel, code, cola, line, order_of, place, place_of, running, serve, set_setting,
};
use super::session::{
    end, end_intent, int, pay, refusal, sessions, settle_intent, submit, table, text,
};
use super::stock::{intent, owner};
use super::*;
use arena_proto::OrderReady;
use arena_proto::PaymentMethod::Cash;

async fn ready(f: &Fixture, order: &str) -> Result<Applied, AppError> {
    submit(
        f,
        owner(),
        IntentPayload::OrderReady(OrderReady {
            order_id: order.into(),
        }),
    )
    .await
}

fn status(f: &Fixture) -> arena_proto::OrderStatus {
    let mut screens = crate::session_screens(&reopen(f), f.clock.now_ms()).unwrap();
    let mut orders = screens.remove(0).1.orders;
    assert_eq!(orders.len(), 1);
    orders.remove(0)
}

#[tokio::test]
async fn an_order_marked_ready_reaches_the_screen_with_the_branch_fulfilment() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 2)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let s = status(&f);
    assert_eq!(
        (s.order_id.as_str(), s.items, s.unserved, s.ready),
        (order.as_str(), 2, 1, None)
    );

    let a = ready(&f, &order).await.unwrap();
    assert_eq!(
        a.changes.iter().map(|c| c.tbl.as_str()).collect::<Vec<_>>(),
        ["orders"]
    );
    // The branch default is pick-up at the counter.
    assert_eq!(status(&f).ready.as_deref(), Some("counter"));
    set_setting(&f, "branch.fulfilment", "seat").await;
    assert_eq!(status(&f).ready.as_deref(), Some("seat"));
}

#[tokio::test]
async fn more_lines_clear_the_mark_and_a_served_order_drops_its_tag() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    ready(&f, &order).await.unwrap();
    assert!(status(&f).ready.is_some());

    place(&f, owner(), place_of(&session, &[(&c, 2)]))
        .await
        .unwrap();
    let s = status(&f);
    assert_eq!((s.items, s.unserved, s.ready), (3, 1, None));
    ready(&f, &order).await.unwrap();

    serve(&f, owner(), &text(&line(&f, &session, "Burger"), "id"))
        .await
        .unwrap();
    let s = status(&f);
    assert_eq!((s.items, s.unserved), (3, 0));
}

#[tokio::test]
async fn a_cancelled_last_line_takes_the_order_off_the_screen() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    cancel(
        &f,
        owner(),
        &text(&line(&f, &session, "Burger"), "id"),
        false,
    )
    .await
    .unwrap();
    let screens = crate::session_screens(&reopen(&f), f.clock.now_ms()).unwrap();
    assert!(screens[0].1.orders.is_empty());
}

#[tokio::test]
async fn a_refused_ready_names_the_order_and_writes_nothing() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let c = cola(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&c, 1)]))
        .await
        .unwrap();
    let plain = text(&order_of(&f, &session).unwrap(), "id");
    // Nothing to serve: a plain line is served at once.
    assert_eq!(
        refusal(ready(&f, &plain).await),
        (ErrorCode::ValidationFailed, Some("order_id".into()))
    );
    assert_eq!(code(ready(&f, "nope").await), ErrorCode::NotFound);

    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    ready(&f, &plain).await.unwrap();
    let before = table(&f, Table::Orders);
    assert_eq!(
        refusal(ready(&f, &plain).await),
        (ErrorCode::ValidationFailed, Some("order_id".into())),
        "already ready"
    );
    assert_eq!(table(&f, Table::Orders), before);

    // A settled bill is closed.
    let s = sessions(&f).pop().unwrap();
    end(&f, end_intent(&session, Some(int(&s, "rev"))))
        .await
        .unwrap();
    let o = table(&f, Table::Orders).pop().unwrap();
    let total = int(&o, "total_minor");
    serve(&f, owner(), &text(&line(&f, &session, "Burger"), "id"))
        .await
        .unwrap();
    let o = table(&f, Table::Orders).pop().unwrap();
    let mut i = settle_intent(
        &text(&o, "id"),
        Some(int(&o, "rev")),
        vec![pay(Cash, total, None)],
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(
        refusal(ready(&f, &plain).await),
        (ErrorCode::ValidationFailed, Some("order_id".into()))
    );
}

#[tokio::test]
async fn ready_takes_no_base_rev_and_needs_the_sell_permission() {
    let f = bootstrapped().await;
    let b = burger(&f, 5).await;
    let session = running(&f).await;
    place(&f, owner(), place_of(&session, &[(&b, 1)]))
        .await
        .unwrap();
    let order = text(&order_of(&f, &session).unwrap(), "id");
    let mut i = intent(
        owner(),
        IntentPayload::OrderReady(OrderReady {
            order_id: order.clone(),
        }),
    );
    i.base_rev = Some(0);
    i.issued_at = f.clock.now_ms();
    let e = f.writer.submit_intent(i).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("base_rev"));
    assert_eq!(
        arena_proto::IntentKind::OrderReady.requirement(),
        arena_proto::Requirement::Perm(arena_proto::Permission::PosSell)
    );
}

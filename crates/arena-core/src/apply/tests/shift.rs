use super::session::{
    billed, end, int, pay, plan_of, player, refusal, sale_order, sell_intent, sell_revert_intent,
    settle_intent, settle_revert_intent, submit, table, text, topup_intent, topup_revert_intent,
};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN};
use arena_proto::PaymentMethod::{Cash, Upi};
use arena_proto::{MAX_FLOAT_MINOR, ShiftOpen};

fn open(float_minor: i64) -> IntentPayload {
    IntentPayload::ShiftOpen(ShiftOpen { float_minor })
}

fn tables(a: &Applied) -> Vec<&str> {
    a.changes.iter().map(|c| c.tbl.as_str()).collect()
}

/// The `cash_events` rows of one kind, oldest first.
fn events(f: &Fixture, kind: &str) -> Vec<Row> {
    table(f, Table::CashEvents)
        .into_iter()
        .filter(|e| text(e, "kind") == kind)
        .collect()
}

fn shift_of(f: &Fixture) -> arena_proto::ShiftState {
    crate::snapshot::read_snapshot(&reopen(f), f.clock.now_ms())
        .unwrap()
        .shift
        .unwrap()
}

/// Closes every open shift behind the Hub's back (closing is Story 5.26).
async fn close_shift(f: &Fixture) {
    super::prepaid::sql(
        f,
        "UPDATE shifts SET state = 'closed', closed_at = 1 WHERE state = 'open'".into(),
    )
    .await;
}

#[tokio::test]
async fn opening_a_shift_writes_the_shift_and_its_float_event() {
    let f = bootstrapped_closed().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let a = submit(&f, counter.clone(), open(5_000)).await.unwrap();
    assert_eq!(tables(&a), ["shifts", "cash_events"]);
    let shifts = table(&f, Table::Shifts);
    assert_eq!(shifts.len(), 1);
    let s = &shifts[0];
    assert_eq!(text(s, "staff_id"), counter.id);
    assert_eq!(text(s, "state"), "open");
    assert_eq!((int(s, "float_minor"), int(s, "carried_minor")), (5_000, 0));
    assert_eq!(int(s, "opened_at"), f.clock.now_ms());
    let float = events(&f, "float_in");
    assert_eq!(float.len(), 1);
    assert_eq!(text(&float[0], "shift_id"), text(s, "id"));
    assert_eq!(int(&float[0], "amount_minor"), 5_000);
    assert_eq!(text(&float[0], "staff_id"), counter.id);
}

#[tokio::test]
async fn the_first_float_may_be_zero() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(0)).await.unwrap();
    assert_eq!(int(&table(&f, Table::Shifts)[0], "float_minor"), 0);
}

#[tokio::test]
async fn the_next_shift_carries_the_last_opening_float_and_the_audit_keeps_both_amounts() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(5_000)).await.unwrap();
    assert_eq!(shift_of(&f).carried_minor, 5_000);
    close_shift(&f).await;
    assert!(shift_of(&f).open.is_none());
    assert_eq!(shift_of(&f).carried_minor, 5_000);

    f.clock.advance(60_000);
    let a = submit(&f, owner(), open(7_500)).await.unwrap();
    let id = a.changes[0].row_id.clone();
    let s = table(&f, Table::Shifts)
        .into_iter()
        .find(|s| text(s, "id") == id)
        .unwrap();
    assert_eq!(
        (int(&s, "float_minor"), int(&s, "carried_minor")),
        (7_500, 5_000)
    );
    // The change-log row carries both, so the audit shows them side by side.
    let change = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.tbl == "shifts" && c.row_id == id)
        .unwrap();
    let row: Row = rmp_serde::from_slice(&change.row).unwrap();
    assert_eq!(
        (int(&row, "float_minor"), int(&row, "carried_minor")),
        (7_500, 5_000)
    );
    // The next one offers the latest opening float.
    close_shift(&f).await;
    assert_eq!(shift_of(&f).carried_minor, 7_500);
}

#[tokio::test]
async fn a_second_open_is_refused_naming_whose_shift_is_open() {
    let f = bootstrapped_closed().await;
    let asha = staff(&f, "asha", ROLE_COUNTER).await;
    let ravi = staff(&f, "ravi", ROLE_COUNTER).await;
    submit(&f, asha, open(1_000)).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    let err = submit(&f, ravi, open(2_000)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ShiftAlreadyOpen);
    assert!(err.message.contains("asha"), "{}", err.message);
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert_eq!(table(&f, Table::Shifts).len(), 1);
}

#[tokio::test]
async fn the_float_and_the_branch_are_checked() {
    let f = bootstrapped_closed().await;
    for bad in [-1, MAX_FLOAT_MINOR + 1] {
        assert_eq!(
            refusal(submit(&f, owner(), open(bad)).await),
            (ErrorCode::ValidationFailed, Some("float_minor".into()))
        );
    }
    let mut with_rev = intent(owner(), open(1_000));
    with_rev.base_rev = Some(0);
    assert_eq!(
        refusal(end(&f, with_rev).await),
        (ErrorCode::ValidationFailed, Some("base_rev".into()))
    );
    assert!(table(&f, Table::Shifts).is_empty());
    assert!(
        submit(&f, owner(), open(MAX_FLOAT_MINOR)).await.is_ok(),
        "the largest float opens"
    );

    let unset = primary_closed();
    assert_eq!(
        refusal(submit(&unset, owner(), open(0)).await),
        (ErrorCode::ValidationFailed, Some("branch".into()))
    );
}

#[tokio::test]
async fn counter_staff_may_open_a_shift_but_kitchen_staff_may_not() {
    let f = bootstrapped_closed().await;
    let cook = staff(&f, "cook", ROLE_KITCHEN).await;
    assert_eq!(
        refusal(submit(&f, cook, open(0)).await).0,
        ErrorCode::PermissionDenied
    );
    assert!(table(&f, Table::Shifts).is_empty());
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter, open(0)).await.unwrap();
}

#[tokio::test]
async fn cash_needs_an_open_shift_and_nothing_else_does() {
    let f = bootstrapped_closed().await;
    let p = player(&f, "asha", "+919876543210").await;
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    let plan = plan_of(&f, "pack", 30_000, true).await;
    let changes = || reopen(&f).changes().unwrap().len();
    let before = changes();

    // A settle with a cash row, a top-up and a plan sale are all refused, and write nothing.
    let cash = settle_intent(
        &order,
        Some(rev),
        vec![pay(Cash, 5_000, None), pay(Upi, 2_500, Some("1234"))],
    );
    assert_eq!(
        refusal(end(&f, cash).await),
        (ErrorCode::ShiftNotOpen, Some("payments".into()))
    );
    assert_eq!(
        refusal(end(&f, topup_intent(&p, 5_000, vec![pay(Cash, 5_000, None)])).await).0,
        ErrorCode::ShiftNotOpen
    );
    assert_eq!(
        refusal(end(&f, sell_intent(&p, &plan, vec![pay(Cash, 30_000, None)])).await).0,
        ErrorCode::ShiftNotOpen
    );
    assert_eq!(changes(), before);
    assert!(table(&f, Table::CashEvents).is_empty());

    // UPI settles the same bill and a UPI top-up works, with no shift and no cash event.
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Upi, 7_500, Some("4321"))]),
    )
    .await
    .unwrap();
    end(
        &f,
        topup_intent(&p, 5_000, vec![pay(Upi, 5_000, Some("1111"))]),
    )
    .await
    .unwrap();
    assert!(table(&f, Table::CashEvents).is_empty());

    // Once a shift opens, cash goes through.
    submit(&f, owner(), open(0)).await.unwrap();
    end(&f, topup_intent(&p, 5_000, vec![pay(Cash, 5_000, None)]))
        .await
        .unwrap();
    assert_eq!(events(&f, "sale").len(), 1);
}

#[tokio::test]
async fn a_cash_payment_writes_a_sale_event_for_the_cash_kept_and_undo_cancels_it() {
    let f = bootstrapped_closed().await;
    let opener = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, opener, open(1_000)).await.unwrap();
    let shift = text(&table(&f, Table::Shifts)[0], "id");
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    // The settling staff member is not the one who opened the shift.
    let a = end(
        &f,
        settle_intent(
            &order,
            Some(rev),
            vec![pay(Cash, 5_000, None), pay(Upi, 2_500, Some("1234"))],
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        tables(&a),
        ["payments", "payments", "cash_events", "orders"]
    );
    let sales = events(&f, "sale");
    assert_eq!(sales.len(), 1);
    let sale = &sales[0];
    assert_eq!(int(sale, "amount_minor"), 5_000);
    assert_eq!(text(sale, "shift_id"), shift);
    assert_eq!(text(sale, "order_id"), order);
    assert_eq!(text(sale, "staff_id"), owner().id);

    f.clock.advance(3_000);
    let rev = int(&table(&f, Table::Orders)[0], "rev");
    end(&f, settle_revert_intent(&order, Some(rev)))
        .await
        .unwrap();
    let back = events(&f, "sale_reverted");
    assert_eq!(back.len(), 1);
    assert_eq!(int(&back[0], "amount_minor"), -5_000);
    assert_eq!(text(&back[0], "reverses"), text(sale, "id"));
    assert_eq!(text(&back[0], "shift_id"), shift);
    // Append-only: the sale row is still there.
    assert_eq!(events(&f, "sale").len(), 1);
}

#[tokio::test]
async fn top_ups_and_plan_sales_write_and_cancel_their_cash_events_too() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(0)).await.unwrap();
    let p = player(&f, "asha", "+919876543210").await;
    let net = |f: &Fixture| -> i64 {
        table(f, Table::CashEvents)
            .iter()
            .filter(|e| text(e, "kind") != "float_in")
            .map(|e| int(e, "amount_minor"))
            .sum()
    };

    end(
        &f,
        topup_intent(
            &p,
            20_000,
            vec![pay(Cash, 12_000, None), pay(Upi, 8_000, Some("1111"))],
        ),
    )
    .await
    .unwrap();
    assert_eq!(net(&f), 12_000);
    let topup = table(&f, Table::Orders)
        .into_iter()
        .rfind(|o| text(o, "kind") == "topup")
        .unwrap();
    f.clock.advance(3_000);
    end(
        &f,
        topup_revert_intent(&text(&topup, "id"), Some(int(&topup, "rev"))),
    )
    .await
    .unwrap();
    assert_eq!(net(&f), 0);
    assert_eq!(events(&f, "sale_reverted").len(), 1);

    let plan = plan_of(&f, "pack", 30_000, true).await;
    end(&f, sell_intent(&p, &plan, vec![pay(Cash, 30_000, None)]))
        .await
        .unwrap();
    assert_eq!(net(&f), 30_000);
    let sale = sale_order(&f);
    f.clock.advance(3_000);
    end(
        &f,
        sell_revert_intent(&text(&sale, "id"), Some(int(&sale, "rev"))),
    )
    .await
    .unwrap();
    assert_eq!(net(&f), 0);
}

#[tokio::test]
async fn the_snapshot_shows_who_opened_it_and_never_the_drawer_total() {
    let f = bootstrapped_closed().await;
    assert!(shift_of(&f).open.is_none());
    assert_eq!(shift_of(&f).carried_minor, 0);
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter.clone(), open(2_500)).await.unwrap();
    let open_shift = shift_of(&f).open.unwrap();
    assert_eq!(open_shift.staff_id, counter.id);
    assert_eq!(open_shift.staff_name, "asha");
    assert_eq!((open_shift.float_minor, open_shift.events), (2_500, 1));
    assert_eq!(open_shift.opened_at, f.clock.now_ms());

    let p = player(&f, "ravi", "+919876543211").await;
    end(&f, topup_intent(&p, 900, vec![pay(Cash, 900, None)]))
        .await
        .unwrap();
    assert_eq!(shift_of(&f).open.unwrap().events, 2);
    // The wire form has no expected-cash field at all.
    let json = serde_json::to_value(shift_of(&f)).unwrap();
    let keys: Vec<_> = json["open"].as_object().unwrap().keys().cloned().collect();
    assert!(
        keys.iter()
            .all(|k| !k.contains("expected") && !k.contains("total")),
        "{keys:?}"
    );
}

#[tokio::test]
async fn a_credit_collection_in_cash_needs_an_open_shift() {
    use super::session::{collect_intent, credit_intent};
    let f = bootstrapped_closed().await;
    let p = player(&f, "asha", "+919876543210").await;
    end(&f, credit_intent(&p, 10_000, &[3, 7, 14]))
        .await
        .unwrap();
    super::prepaid::sql(
        &f,
        "UPDATE credit_balances SET outstanding_minor = 6000".into(),
    )
    .await;
    let before = reopen(&f).changes().unwrap().len();
    assert_eq!(
        refusal(end(&f, collect_intent(&p, 4_000, vec![pay(Cash, 4_000, None)])).await),
        (ErrorCode::ShiftNotOpen, Some("payments".into()))
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    end(
        &f,
        collect_intent(&p, 4_000, vec![pay(Upi, 4_000, Some("2222"))]),
    )
    .await
    .unwrap();
    submit(&f, owner(), open(0)).await.unwrap();
    end(&f, collect_intent(&p, 2_000, vec![pay(Cash, 2_000, None)]))
        .await
        .unwrap();
    assert_eq!(events(&f, "sale").len(), 1);
}

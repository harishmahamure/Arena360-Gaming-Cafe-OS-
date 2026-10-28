//! Cash deposits and expenses (Story 5.25).

use super::session::{
    billed, end, int, pay, refusal, settle_intent, settle_revert_intent, submit, table, text,
};
use super::stock::{intent, manager_with_pin, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    CashDeposit, ElevatedBy, ElevationKind, ElevationMode, ExpenseCategory, ExpenseCreate,
    ExpensePaidFrom, MAX_CASH_MOVE_MINOR, ShiftOpen,
};

fn open(float_minor: i64) -> IntentPayload {
    IntentPayload::ShiftOpen(ShiftOpen { float_minor })
}

fn deposit(amount_minor: i64, reference: &str) -> IntentPayload {
    IntentPayload::CashDeposit(CashDeposit {
        amount_minor,
        reference: reference.into(),
    })
}

fn expense(amount_minor: i64, paid_from: ExpensePaidFrom) -> ExpenseCreate {
    ExpenseCreate {
        category: ExpenseCategory::Supplies,
        amount_minor,
        paid_from,
        receipt_photo: None,
        note: None,
    }
}

fn spend(e: ExpenseCreate) -> IntentPayload {
    IntentPayload::ExpenseCreate(e)
}

/// A JPEG data URI of `bytes` bytes (not a decodable picture: the checks look at the header and
/// the size).
fn photo(bytes: usize) -> String {
    use base64::Engine;
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0];
    b.resize(bytes, 0);
    format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(b)
    )
}

/// The `cash_events` rows of one kind, oldest first.
fn events(f: &Fixture, kind: &str) -> Vec<Row> {
    table(f, Table::CashEvents)
        .into_iter()
        .filter(|e| text(e, "kind") == kind)
        .collect()
}

fn expected(f: &Fixture) -> i64 {
    let shift = table(f, Table::Shifts)
        .into_iter()
        .find(|s| text(s, "state") == "open")
        .map(|s| text(&s, "id"))
        .unwrap();
    table(f, Table::CashEvents)
        .iter()
        .filter(|e| text(e, "shift_id") == shift)
        .map(|e| int(e, "amount_minor"))
        .sum()
}

#[tokio::test]
async fn a_deposit_writes_a_negative_cash_event_with_its_slip_reference() {
    let f = bootstrapped_closed().await;
    let manager = staff(&f, "meena", ROLE_MANAGER).await;
    submit(&f, manager.clone(), open(10_000)).await.unwrap();
    let a = submit(&f, manager.clone(), deposit(4_000, "  SLIP-17 "))
        .await
        .unwrap();
    assert_eq!(a.changes.len(), 1);
    assert_eq!(a.changes[0].tbl, "cash_events");
    let rows = events(&f, "deposit");
    assert_eq!(rows.len(), 1);
    assert_eq!(int(&rows[0], "amount_minor"), -4_000);
    assert_eq!(text(&rows[0], "reference"), "SLIP-17");
    assert_eq!(text(&rows[0], "staff_id"), manager.id);
    assert_eq!(expected(&f), 6_000);
}

#[tokio::test]
async fn a_deposit_needs_a_reference_an_amount_an_open_shift_and_cash_manage() {
    let f = bootstrapped_closed().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    let manager = staff(&f, "meena", ROLE_MANAGER).await;
    assert_eq!(
        refusal(submit(&f, manager.clone(), deposit(100, "S1")).await).0,
        ErrorCode::ShiftNotOpen,
        "no shift"
    );
    submit(&f, manager.clone(), open(10_000)).await.unwrap();
    let long = "x".repeat(41);
    for (amount, reference, field) in [
        (100, "", "reference"),
        (100, "   ", "reference"),
        (100, long.as_str(), "reference"),
        (0, "S1", "amount_minor"),
        (-5, "S1", "amount_minor"),
        (MAX_CASH_MOVE_MINOR + 1, "S1", "amount_minor"),
    ] {
        assert_eq!(
            refusal(submit(&f, manager.clone(), deposit(amount, reference)).await),
            (ErrorCode::ValidationFailed, Some(field.into())),
            "{amount} {reference}"
        );
    }
    assert_eq!(
        refusal(submit(&f, counter.clone(), deposit(100, "S1")).await).0,
        ErrorCode::PermissionDenied,
        "Counter lacks cash.manage"
    );
    assert!(events(&f, "deposit").is_empty());

    // Counter elevates with a manager's PIN.
    let approver = manager_with_pin(&f).await;
    let mut i = intent(counter.clone(), deposit(100, "S1"));
    i.elevated_by = Some(ElevatedBy {
        kind: ElevationKind::Pin,
        id: approver,
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    let rows = events(&f, "deposit");
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "staff_id"), counter.id);
}

#[tokio::test]
async fn a_deposit_or_drawer_expense_above_the_expected_cash_is_drawer_short() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(5_000)).await.unwrap();
    let before = reopen(&f).changes().unwrap().len();
    let short = submit(&f, owner(), deposit(5_001, "S1")).await.unwrap_err();
    assert_eq!(short.code, ErrorCode::DrawerShort);
    assert!(
        !short.message.chars().any(|c| c.is_ascii_digit()),
        "the message never reveals the expected amount: {}",
        short.message
    );
    assert_eq!(
        refusal(submit(&f, owner(), spend(expense(5_001, ExpensePaidFrom::Drawer))).await).0,
        ErrorCode::DrawerShort
    );
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    // The exact amount is fine, and the drawer is then empty.
    submit(&f, owner(), deposit(5_000, "S1")).await.unwrap();
    assert_eq!(expected(&f), 0);
    assert_eq!(
        refusal(submit(&f, owner(), deposit(1, "S2")).await).0,
        ErrorCode::DrawerShort
    );
}

#[tokio::test]
async fn only_a_drawer_expense_lowers_the_expected_cash() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(10_000)).await.unwrap();
    let mut drawer = expense(2_500, ExpensePaidFrom::Drawer);
    drawer.category = ExpenseCategory::FoodStock;
    drawer.note = Some(" tea and milk ".into());
    drawer.receipt_photo = Some(photo(10_000));
    submit(&f, owner(), spend(drawer)).await.unwrap();
    assert_eq!(expected(&f), 7_500);
    // The owner paid for this one, so it can be larger than the drawer holds.
    let mut owner_paid = expense(50_000, ExpensePaidFrom::Owner);
    owner_paid.category = ExpenseCategory::Repairs;
    submit(&f, owner(), spend(owner_paid)).await.unwrap();
    assert_eq!(expected(&f), 7_500);

    let rows = events(&f, "expense");
    assert_eq!(rows.len(), 2);
    let d = &rows[0];
    assert_eq!(int(d, "amount_minor"), -2_500);
    assert_eq!(int(d, "spent_minor"), 2_500);
    assert_eq!(text(d, "category"), "food_stock");
    assert_eq!(text(d, "paid_from"), "drawer");
    assert_eq!(text(d, "note"), "tea and milk");
    assert!(text(d, "receipt_photo").starts_with("data:image/jpeg;base64,"));
    let o = &rows[1];
    assert_eq!(int(o, "amount_minor"), 0);
    assert_eq!(int(o, "spent_minor"), 50_000);
    assert_eq!(text(o, "paid_from"), "owner");
}

#[tokio::test]
async fn an_expense_is_checked() {
    let f = bootstrapped_closed().await;
    let manager = staff(&f, "meena", ROLE_MANAGER).await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    assert_eq!(
        refusal(
            submit(
                &f,
                manager.clone(),
                spend(expense(100, ExpensePaidFrom::Owner))
            )
            .await
        )
        .0,
        ErrorCode::ShiftNotOpen
    );
    submit(&f, manager.clone(), open(10_000)).await.unwrap();
    let mut big = expense(100, ExpensePaidFrom::Drawer);
    big.receipt_photo = Some(photo(200 * 1024 + 1));
    let mut png = expense(100, ExpensePaidFrom::Drawer);
    png.receipt_photo = Some("data:image/png;base64,AAAA".into());
    let mut junk = expense(100, ExpensePaidFrom::Drawer);
    junk.receipt_photo = Some("data:image/jpeg;base64,!!!".into());
    let mut long_note = expense(100, ExpensePaidFrom::Drawer);
    long_note.note = Some("x".repeat(141));
    for (e, field) in [
        (expense(0, ExpensePaidFrom::Drawer), "amount_minor"),
        (big, "receipt_photo"),
        (png, "receipt_photo"),
        (junk, "receipt_photo"),
        (long_note, "note"),
    ] {
        assert_eq!(
            refusal(submit(&f, manager.clone(), spend(e)).await),
            (ErrorCode::ValidationFailed, Some(field.into()))
        );
    }
    let mut at_limit = expense(100, ExpensePaidFrom::Drawer);
    at_limit.receipt_photo = Some(photo(200 * 1024));
    submit(&f, manager.clone(), spend(at_limit)).await.unwrap();
    assert_eq!(
        refusal(submit(&f, counter, spend(expense(100, ExpensePaidFrom::Drawer))).await).0,
        ErrorCode::PermissionDenied
    );
    assert_eq!(events(&f, "expense").len(), 1);
}

#[tokio::test]
async fn expected_cash_nets_float_cash_sales_deposits_drawer_expenses_and_undone_sales() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(10_000)).await.unwrap();
    let (order, _, rev) = billed(&f, 10_000, 41).await;
    end(
        &f,
        settle_intent(&order, Some(rev), vec![pay(Cash, 7_500, None)]),
    )
    .await
    .unwrap();
    assert_eq!(expected(&f), 17_500);
    submit(&f, owner(), deposit(8_000, "S1")).await.unwrap();
    submit(&f, owner(), spend(expense(1_500, ExpensePaidFrom::Drawer)))
        .await
        .unwrap();
    submit(&f, owner(), spend(expense(900, ExpensePaidFrom::Owner)))
        .await
        .unwrap();
    assert_eq!(expected(&f), 8_000);
    // Undo the settle: its sale row is cancelled by a compensating one and nets out.
    let settled = table(&f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "id") == order)
        .unwrap();
    end(&f, settle_revert_intent(&order, Some(int(&settled, "rev"))))
        .await
        .unwrap();
    assert_eq!(expected(&f), 500);
}

//! Closing a shift with a blind count and a signed variance (Story 5.26).

use super::session::{int, refusal, submit, table, text};
use super::stock::{intent, manager_with_pin, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{
    CloseDeposit, ElevatedBy, ElevationKind, ElevationMode, ExpenseCategory, ExpenseCreate,
    ExpensePaidFrom, ShiftClose, ShiftOpen, VarianceReason,
};

fn open(float_minor: i64) -> IntentPayload {
    IntentPayload::ShiftOpen(ShiftOpen { float_minor })
}

fn close(counted_minor: i64) -> ShiftClose {
    ShiftClose {
        counted_minor,
        deposit: None,
        reason: None,
        note: None,
    }
}

fn closing(c: ShiftClose) -> IntentPayload {
    IntentPayload::ShiftClose(c)
}

/// An intent by `actor` elevated by the manager `approver` (a PIN checked at the Hub).
async fn elevated(
    f: &Fixture,
    actor: Actor,
    approver: &str,
    c: ShiftClose,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, closing(c));
    i.elevated_by = Some(ElevatedBy {
        kind: ElevationKind::Pin,
        id: approver.to_owned(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn shifts(f: &Fixture) -> Vec<Row> {
    table(f, Table::Shifts)
}

fn only_shift(f: &Fixture) -> Row {
    let mut all = shifts(f);
    assert_eq!(all.len(), 1);
    all.remove(0)
}

fn opt_text(row: &Row, col: &str) -> Option<String> {
    match &row[col] {
        RowValue::Text(t) => Some(t.clone()),
        RowValue::Null => None,
        other => panic!("{col}: {other:?}"),
    }
}

fn opt_int(row: &Row, col: &str) -> Option<i64> {
    match &row[col] {
        RowValue::Integer(n) => Some(*n),
        RowValue::Null => None,
        other => panic!("{col}: {other:?}"),
    }
}

#[tokio::test]
async fn a_close_within_the_threshold_stores_the_count_and_needs_no_pin() {
    let f = bootstrapped_closed().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter.clone(), open(100_000)).await.unwrap();
    // 100.00 short: exactly the default threshold.
    let applied = submit(&f, counter.clone(), closing(close(90_000)))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "shifts");
    let s = only_shift(&f);
    assert_eq!(text(&s, "state"), "closed");
    assert_eq!(int(&s, "counted_minor"), 90_000);
    assert_eq!(int(&s, "expected_minor"), 100_000);
    assert_eq!(int(&s, "variance_minor"), -10_000);
    assert_eq!(opt_text(&s, "variance_reason"), None);
    assert_eq!(text(&s, "closed_by"), counter.id);
    assert_eq!(opt_text(&s, "approver_id"), None);
    assert_eq!(int(&s, "next_float_minor"), 90_000);
    // The next shift offers the counted cash as its float.
    assert_eq!(reopen(&f).shift_state().unwrap().carried_minor, 90_000);
    assert!(reopen(&f).shift_state().unwrap().open.is_none());
}

#[tokio::test]
async fn an_over_count_is_signed_positive() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(5_000)).await.unwrap();
    submit(&f, owner(), closing(close(5_040))).await.unwrap();
    assert_eq!(int(&only_shift(&f), "variance_minor"), 40);
}

#[tokio::test]
async fn a_close_over_the_threshold_needs_an_approver_a_reason_and_a_note_for_other() {
    let f = bootstrapped_closed().await;
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter.clone(), open(100_000)).await.unwrap();
    let approver = manager_with_pin(&f).await;

    // No approver: the counter alone cannot close 100.01 short.
    let err = submit(&f, counter.clone(), closing(close(89_999)))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::VarianceOverThreshold);
    assert!(
        !err.message.chars().any(|c| c.is_ascii_digit()),
        "the refusal never reveals the amounts: {}",
        err.message
    );
    // Even with a reason.
    let mut with_reason = close(89_999);
    with_reason.reason = Some(VarianceReason::CountingError);
    assert_eq!(
        refusal(submit(&f, counter.clone(), closing(with_reason)).await).0,
        ErrorCode::VarianceOverThreshold
    );
    assert_eq!(text(&only_shift(&f), "state"), "open");

    // An approver but no reason.
    let no_reason = elevated(&f, counter.clone(), &approver, close(89_999)).await;
    assert_eq!(
        refusal(no_reason),
        (ErrorCode::ValidationFailed, Some("reason".into()))
    );
    // `other` needs text.
    let mut other = close(89_999);
    other.reason = Some(VarianceReason::Other);
    assert_eq!(
        refusal(elevated(&f, counter.clone(), &approver, other.clone()).await),
        (ErrorCode::ValidationFailed, Some("note".into()))
    );
    other.note = Some("  coin tray knocked over  ".into());
    elevated(&f, counter.clone(), &approver, other)
        .await
        .unwrap();
    let s = only_shift(&f);
    assert_eq!(text(&s, "state"), "closed");
    assert_eq!(int(&s, "variance_minor"), -10_001);
    assert_eq!(opt_text(&s, "variance_reason").as_deref(), Some("other"));
    assert_eq!(
        opt_text(&s, "variance_note").as_deref(),
        Some("coin tray knocked over")
    );
    assert_eq!(text(&s, "closed_by"), counter.id);
    assert_eq!(
        opt_text(&s, "approver_id").as_deref(),
        Some(approver.as_str())
    );
    // The change log records both actors.
    let last = reopen(&f).changes().unwrap().into_iter().last().unwrap();
    assert_eq!(last.actor_id, counter.id);
    assert_eq!(last.elevated_by_id.as_deref(), Some(approver.as_str()));
}

#[tokio::test]
async fn a_manager_closes_over_the_threshold_with_a_reason_and_no_pin() {
    let f = bootstrapped_closed().await;
    let manager = staff(&f, "meena", ROLE_MANAGER).await;
    submit(&f, manager.clone(), open(100_000)).await.unwrap();
    let mut c = close(120_000);
    c.reason = Some(VarianceReason::UnrecordedExpense);
    c.note = Some("ignored for a listed reason".into());
    submit(&f, manager.clone(), closing(c)).await.unwrap();
    let s = only_shift(&f);
    assert_eq!(int(&s, "variance_minor"), 20_000);
    assert_eq!(
        opt_text(&s, "variance_reason").as_deref(),
        Some("unrecorded_expense")
    );
    assert_eq!(
        opt_text(&s, "variance_note"),
        None,
        "only other keeps a note"
    );
    assert_eq!(
        opt_text(&s, "approver_id").as_deref(),
        Some(manager.id.as_str())
    );
}

#[tokio::test]
async fn the_threshold_is_a_branch_setting() {
    let f = bootstrapped_closed().await;
    let setting = reopen(&f)
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| text(r, "key") == arena_proto::KEY_SHIFT_VARIANCE_THRESHOLD)
        .expect("bootstrap writes the threshold");
    assert_eq!(text(&setting, "value"), "10000");
    let mut set = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: arena_proto::KEY_SHIFT_VARIANCE_THRESHOLD.into(),
            value: "0".into(),
        }),
    );
    set.base_rev = Some(int(&setting, "rev"));
    set.issued_at = f.clock.now_ms();
    f.writer.submit_intent(set).await.unwrap();
    let counter = staff(&f, "asha", ROLE_COUNTER).await;
    submit(&f, counter.clone(), open(1_000)).await.unwrap();
    // With a zero threshold any difference needs a manager.
    assert_eq!(
        refusal(submit(&f, counter, closing(close(999))).await).0,
        ErrorCode::VarianceOverThreshold
    );
}

#[tokio::test]
async fn a_deposit_at_close_is_recorded_after_the_count_and_lowers_the_next_float() {
    let f = bootstrapped_closed().await;
    submit(&f, owner(), open(10_000)).await.unwrap();
    // A drawer expense during the shift, then the close with a deposit.
    submit(
        &f,
        owner(),
        IntentPayload::ExpenseCreate(ExpenseCreate {
            category: ExpenseCategory::Supplies,
            amount_minor: 1_000,
            paid_from: ExpensePaidFrom::Drawer,
            receipt_photo: None,
            note: None,
        }),
    )
    .await
    .unwrap();
    let mut c = close(9_000);
    c.deposit = Some(CloseDeposit {
        amount_minor: 7_000,
        reference: " SLIP-9 ".into(),
    });
    let applied = submit(&f, owner(), closing(c)).await.unwrap();
    assert_eq!(applied.changes.len(), 2);
    let s = only_shift(&f);
    assert_eq!(int(&s, "counted_minor"), 9_000);
    assert_eq!(
        int(&s, "expected_minor"),
        9_000,
        "the deposit is after the count"
    );
    assert_eq!(int(&s, "variance_minor"), 0);
    assert_eq!(int(&s, "next_float_minor"), 2_000);
    let deposits: Vec<Row> = table(&f, Table::CashEvents)
        .into_iter()
        .filter(|e| text(e, "kind") == "deposit")
        .collect();
    assert_eq!(deposits.len(), 1);
    assert_eq!(int(&deposits[0], "amount_minor"), -7_000);
    assert_eq!(text(&deposits[0], "reference"), "SLIP-9");
    assert_eq!(reopen(&f).shift_state().unwrap().carried_minor, 2_000);
}

#[tokio::test]
async fn a_close_is_checked() {
    let f = bootstrapped_closed().await;
    assert_eq!(
        refusal(submit(&f, owner(), closing(close(100))).await).0,
        ErrorCode::ShiftNotOpen
    );
    submit(&f, owner(), open(10_000)).await.unwrap();
    let mut too_much = close(500);
    too_much.deposit = Some(CloseDeposit {
        amount_minor: 501,
        reference: "S1".into(),
    });
    let mut no_slip = close(500);
    no_slip.deposit = Some(CloseDeposit {
        amount_minor: 100,
        reference: "  ".into(),
    });
    for (c, field) in [
        (close(-1), "counted_minor"),
        (too_much, "deposit"),
        (no_slip, "reference"),
    ] {
        assert_eq!(
            refusal(submit(&f, owner(), closing(c)).await),
            (ErrorCode::ValidationFailed, Some(field.into()))
        );
    }
    let mut based = intent(owner(), closing(close(10_000)));
    based.base_rev = Some(0);
    based.issued_at = f.clock.now_ms();
    let err = f.writer.submit_intent(based).await.unwrap_err();
    assert_eq!(err.field.as_deref(), Some("base_rev"));
    assert_eq!(text(&only_shift(&f), "state"), "open");

    // A closed shift cannot close again, and cash needs a new shift.
    submit(&f, owner(), closing(close(10_000))).await.unwrap();
    assert_eq!(
        refusal(submit(&f, owner(), closing(close(10_000))).await).0,
        ErrorCode::ShiftNotOpen
    );
    submit(&f, owner(), open(2_000)).await.unwrap();
    assert_eq!(shifts(&f).len(), 2);
    assert_eq!(opt_int(&shifts(&f)[1], "counted_minor"), None);
}

#[tokio::test]
async fn the_reveal_needs_a_count_and_shows_the_breakdown() {
    let f = bootstrapped_closed().await;
    assert!(
        reopen(&f).shift_count(100).unwrap().is_none(),
        "no shift, nothing to reveal"
    );
    submit(&f, owner(), open(10_000)).await.unwrap();
    submit(
        &f,
        owner(),
        IntentPayload::CashDeposit(arena_proto::CashDeposit {
            amount_minor: 2_500,
            reference: "S1".into(),
        }),
    )
    .await
    .unwrap();
    let reveal = reopen(&f).shift_count(7_000).unwrap().unwrap();
    assert_eq!(reveal.counted_minor, 7_000);
    assert_eq!(reveal.expected_minor, 7_500);
    assert_eq!(reveal.difference_minor, -500);
    assert_eq!(reveal.threshold_minor, 10_000);
    assert!(!reveal.over_threshold);
    assert_eq!(reveal.breakdown.float_minor, 10_000);
    assert_eq!(reveal.breakdown.deposits_minor, -2_500);
    assert_eq!(reveal.breakdown.cash_sales_minor, 0);
    assert!(
        reopen(&f)
            .shift_count(20_000)
            .unwrap()
            .unwrap()
            .over_threshold
    );
}

//! The daily credit reminder (Story 8.31): who is due on which day, the gates, "once per day",
//! what the drain does with the row, and the Hub's overdue list.

use super::prepaid::{ist, sql, system};
use super::receipt::{set_key, whatsapp_branch};
use super::session::{credit_player, end, int, player_bill, settle_on_credit, table, text};
use super::stock::intent;
use super::*;
use crate::credit_reminder::due;
use crate::notify_drain::{DrainAction, plan};
use crate::overdue::overdue_view;
use arena_proto::{KEY_NOTIFY_CREDIT_REMINDER, NotifyRequested, UNDO_WINDOW_MS};

const KEK: [u8; crate::players::KEK_LEN] = super::players::KEK;

/// A paid branch with "Credit reminder" on and a consenting player who owes 60.00 on credit from
/// a bill settled on the business day of the fixture's start (2027-01-15, afternoon IST).
async fn owing() -> (Fixture, String) {
    let f = bootstrapped().await;
    whatsapp_branch(&f).await;
    set_key(&f, KEY_NOTIFY_CREDIT_REMINDER, "true").await;
    let p = credit_player(&f, "asha", "+919876543210", 100_000).await;
    let (order, rev) = player_bill(&f, &p, 6_000, 60).await;
    end(&f, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    (f, p)
}

fn reminders(f: &Fixture, at: &str) -> Vec<NotifyRequested> {
    f.clock.set(ist(at));
    due(&reopen(f), f.clock.now_ms()).unwrap()
}

async fn queue(f: &Fixture, r: NotifyRequested) -> Result<Applied, AppError> {
    let mut i = intent(system(), IntentPayload::NotifyRequested(r));
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn request(player: &str) -> NotifyRequested {
    NotifyRequested {
        kind: "credit_reminder".into(),
        player_id: player.into(),
        session_id: None,
        params: [("amount_minor".to_owned(), "6000".to_owned())].into(),
    }
}

fn rows(f: &Fixture) -> Vec<Row> {
    table(f, Table::NotifyOutbox)
}

#[tokio::test]
async fn a_player_on_a_cycle_day_gets_one_reminder_from_eleven_oclock() {
    let (f, p) = owing().await;
    // Day 3 (2027-01-18) but not yet 11:00 branch time.
    assert!(reminders(&f, "2027-01-18 10:59").is_empty());
    let due = reminders(&f, "2027-01-18 11:00");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, "credit_reminder");
    assert_eq!(due[0].player_id, p);
    assert_eq!(due[0].session_id, None);
    assert_eq!(due[0].params["amount_minor"], "6000");

    queue(&f, due[0].clone()).await.unwrap();
    let row = &rows(&f)[0];
    assert_eq!(text(row, "kind"), "credit_reminder");
    assert_eq!(text(row, "reminder_day"), "2027-01-18");
    assert_eq!(text(row, "state"), "queued");
    assert_eq!(
        int(row, "not_before"),
        ist("2027-01-18 11:00") + UNDO_WINDOW_MS
    );
    assert_eq!(text(row, "params"), r#"{"amount_minor":"6000"}"#);

    // Once a day: the check finds nothing more and a forced repeat is refused.
    assert!(reminders(&f, "2027-01-18 16:00").is_empty());
    assert_eq!(
        queue(&f, due[0].clone()).await.unwrap_err().code,
        ErrorCode::Stale
    );
    assert_eq!(rows(&f).len(), 1);
}

#[tokio::test]
async fn only_the_cycle_days_send_and_a_missed_day_is_not_made_up() {
    let (f, _) = owing().await;
    // Days 1, 2, 4, 5, 6 are not on the default cycle (3, 7, 14): a Hub that was off on day 3
    // and starts on day 4 queues nothing for day 3.
    for day in ["16", "17", "19", "20", "21"] {
        assert!(
            reminders(&f, &format!("2027-01-{day} 12:00")).is_empty(),
            "{day}"
        );
    }
    // Day 7 and day 14 are on the cycle.
    assert_eq!(reminders(&f, "2027-01-22 12:00").len(), 1);
    assert!(reminders(&f, "2027-01-23 12:00").is_empty());
    assert_eq!(reminders(&f, "2027-01-29 11:30").len(), 1);
}

#[tokio::test]
async fn a_hub_that_was_off_at_eleven_catches_up_the_same_day() {
    let (f, _) = owing().await;
    assert!(reminders(&f, "2027-01-18 10:00").is_empty());
    // It starts at 17:40: the day's check runs then.
    assert_eq!(reminders(&f, "2027-01-18 17:40").len(), 1);
}

#[tokio::test]
async fn no_consent_a_free_branch_or_a_switch_that_is_off_queues_nothing() {
    let (f, p) = owing().await;
    assert_eq!(reminders(&f, "2027-01-18 12:00").len(), 1);

    set_key(&f, KEY_NOTIFY_CREDIT_REMINDER, "false").await;
    assert!(reminders(&f, "2027-01-18 12:00").is_empty());
    assert_eq!(
        queue(&f, request(&p)).await.unwrap_err().code,
        ErrorCode::ValidationFailed
    );
    set_key(&f, KEY_NOTIFY_CREDIT_REMINDER, "true").await;

    let at = f.clock.now_ms();
    reopen(&f)
        .write_tx(|tx| {
            arena_store::insert_consent(
                tx,
                "withdrawn",
                &p,
                arena_proto::CONSENT_TRANSACTIONAL,
                "2",
                false,
                at,
            )
        })
        .unwrap();
    assert!(reminders(&f, "2027-01-18 12:00").is_empty());
    assert_eq!(
        queue(&f, request(&p)).await.unwrap_err().code,
        ErrorCode::ConsentRequired
    );
    assert!(rows(&f).is_empty());

    // A Free branch writes none, and its check is empty.
    let free = bootstrapped().await;
    let p = credit_player(&free, "asha", "+919876543210", 100_000).await;
    let (order, rev) = player_bill(&free, &p, 6_000, 60).await;
    end(&free, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    assert!(reminders(&free, "2027-01-18 12:00").is_empty());
    assert_eq!(
        queue(&free, request(&p)).await.unwrap_err().code,
        ErrorCode::FeatureNotEntitled
    );
}

#[tokio::test]
async fn a_player_who_owes_nothing_gets_no_reminder() {
    let (f, p) = owing().await;
    sql(
        &f,
        format!("UPDATE credit_balances SET outstanding_minor = 0 WHERE player_id = '{p}'"),
    )
    .await;
    assert!(reminders(&f, "2027-01-18 12:00").is_empty());
    assert_eq!(
        queue(&f, request(&p)).await.unwrap_err().code,
        ErrorCode::Stale
    );
}

#[tokio::test]
async fn the_drain_sends_what_is_owed_now_and_skips_a_cleared_tab() {
    let (f, p) = owing().await;
    let r = reminders(&f, "2027-01-18 12:00").remove(0);
    queue(&f, r).await.unwrap();
    let drain = |now: i64| plan(&reopen(&f), &KEK, now, 10).unwrap();

    assert!(drain(f.clock.now_ms()).is_empty(), "inside the Undo window");
    let after = f.clock.now_ms() + UNDO_WINDOW_MS;
    let items = drain(after);
    let DrainAction::Send(req) = &items[0].action else {
        panic!("a send, got {:?}", items[0].action);
    };
    assert_eq!(req.kind, "credit_reminder");
    assert_eq!(req.params["name"], "Asha Rao");
    assert!(!req.params.contains_key("amount_minor"));

    sql(
        &f,
        format!("UPDATE credit_balances SET outstanding_minor = 0 WHERE player_id = '{p}'"),
    )
    .await;
    assert_eq!(
        drain(after)[0].action,
        DrainAction::Skip("credit_cleared".into())
    );
}

#[tokio::test]
async fn the_overdue_list_is_oldest_first_with_bucket_consent_and_last_reminder() {
    let (f, p) = owing().await;
    // A second player owes less, from the same day, and withdrew consent.
    let q = credit_player(&f, "ravi", "+919812345678", 100_000).await;
    let (order, rev) = player_bill(&f, &q, 6_000, 30).await;
    end(&f, settle_on_credit(&order, rev, 3_000, false))
        .await
        .unwrap();
    let at = f.clock.now_ms();
    reopen(&f)
        .write_tx(|tx| {
            arena_store::insert_consent(
                tx,
                "ravi-withdrawn",
                &q,
                arena_proto::CONSENT_TRANSACTIONAL,
                "2",
                false,
                at,
            )
        })
        .unwrap();

    f.clock.set(ist("2027-01-25 12:00"));
    let view = overdue_view(&reopen(&f), &KEK, f.clock.now_ms()).unwrap();
    assert!(view.reminders);
    assert_eq!(view.as_of_day, "2027-01-25");
    assert_eq!(view.rows.len(), 2);
    let asha = view.rows.iter().find(|r| r.player_id == p).unwrap();
    assert_eq!(
        (asha.name.as_str(), asha.erased, asha.outstanding_minor),
        ("Asha Rao", false, 6_000)
    );
    assert_eq!((asha.age_days, asha.bucket.as_str()), (10, "8_14"));
    assert_eq!(asha.consent, Some(true));
    assert_eq!(asha.last_reminder_day, None);
    let ravi = view.rows.iter().find(|r| r.player_id == q).unwrap();
    assert_eq!(ravi.consent, Some(false));
    assert_eq!(ravi.outstanding_minor, 3_000);

    // A reminder that was sent shows its day; one still queued does not.
    let r = reminders(&f, "2027-01-29 12:00")
        .into_iter()
        .find(|r| r.player_id == p)
        .unwrap();
    queue(&f, r).await.unwrap();
    let last = |f: &Fixture| {
        overdue_view(&reopen(f), &KEK, f.clock.now_ms())
            .unwrap()
            .rows
            .into_iter()
            .find(|r| r.player_id == p)
            .unwrap()
            .last_reminder_day
    };
    assert_eq!(last(&f), None);
    let id = text(&rows(&f)[0], "id");
    sql(
        &f,
        format!("UPDATE notify_outbox SET state = 'sent' WHERE id = '{id}'"),
    )
    .await;
    assert_eq!(last(&f).as_deref(), Some("2027-01-29"));
}

#[tokio::test]
async fn a_free_branch_lists_its_overdue_players_without_reminders() {
    let free = bootstrapped().await;
    let p = credit_player(&free, "asha", "+919876543210", 100_000).await;
    let (order, rev) = player_bill(&free, &p, 6_000, 60).await;
    end(&free, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    free.clock.set(ist("2027-01-20 12:00"));
    let view = overdue_view(&reopen(&free), &KEK, free.clock.now_ms()).unwrap();
    assert!(!view.reminders);
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].last_reminder_day, None);
}

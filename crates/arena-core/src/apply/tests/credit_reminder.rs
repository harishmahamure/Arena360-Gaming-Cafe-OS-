//! The daily credit reminder (Story 8.31): who is due on which day, the gates, "once per day",
//! what the drain does with the row, and the Hub's overdue list.

use super::prepaid::{ist, sql, system};
use super::receipt::{set_key, whatsapp_branch};
use super::session::{credit_player, end, int, player_bill, settle_on_credit, table, text};
use super::stock::{intent, owner};
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

fn remind(id: i64, player: &str) -> arena_proto::CommandWire {
    arena_proto::CommandWire {
        id,
        idempotency_key: ulid::Ulid::generate().to_string(),
        kind: "credit.remind".into(),
        schema_version: 1,
        payload: serde_json::json!({ "player_id": player }),
        issued_by: "owner-1".into(),
        elevated_by: None,
    }
}

fn refusal(c: &arena_store::AppliedCommand) -> Option<ErrorCode> {
    let e: AppError = serde_json::from_str(c.error.as_deref()?).ok()?;
    Some(e.code)
}

#[tokio::test]
async fn an_owner_reminder_queues_the_credit_reminder_row_with_what_is_owed() {
    let (f, p) = owing().await;
    f.clock.set(ist("2027-01-16 12:00"));
    let done = f.writer.apply_command(remind(1, &p)).await.unwrap();
    assert!(done.applied && done.error.is_none(), "{done:?}");
    let rows = rows(&f);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(text(row, "kind"), "credit_reminder");
    assert_eq!(text(row, "player_id"), p);
    assert_eq!(text(row, "state"), "queued");
    // Not a cycle day: the owner asked. The business day is the one the Hub is on.
    assert_eq!(text(row, "reminder_day"), "2027-01-16");
    assert_eq!(text(row, "params"), r#"{"amount_minor":"6000"}"#);
    assert_eq!(
        int(row, "not_before"),
        ist("2027-01-16 12:00") + UNDO_WINDOW_MS
    );

    // The change is the owner's.
    let change = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.tbl == "notify_outbox")
        .unwrap();
    assert_eq!(change.actor_kind, "owner");
}

#[tokio::test]
async fn a_second_reminder_the_same_day_automatic_or_manual_is_rejected() {
    let (f, p) = owing().await;
    f.clock.set(ist("2027-01-18 12:00"));
    // The automatic check got there first.
    let auto = reminders(&f, "2027-01-18 12:00").remove(0);
    queue(&f, auto).await.unwrap();
    let again = f.writer.apply_command(remind(1, &p)).await.unwrap();
    assert!(!again.applied);
    assert_eq!(refusal(&again), Some(ErrorCode::Stale));
    assert_eq!(rows(&f).len(), 1);

    // The next business day it goes through, and then it is refused a second time.
    f.clock.set(ist("2027-01-19 12:00"));
    assert!(f.writer.apply_command(remind(2, &p)).await.unwrap().applied);
    let twice = f.writer.apply_command(remind(3, &p)).await.unwrap();
    assert_eq!(refusal(&twice), Some(ErrorCode::Stale));
    assert_eq!(rows(&f).len(), 2);
}

#[tokio::test]
async fn an_owner_reminder_is_rejected_without_consent_credit_or_a_paid_branch() {
    let (f, p) = owing().await;
    f.clock.set(ist("2027-01-16 12:00"));
    // A player nobody knows.
    let ghost = f.writer.apply_command(remind(1, "NOPE")).await.unwrap();
    assert!(!ghost.applied);
    assert_eq!(refusal(&ghost), Some(ErrorCode::Stale));

    // The tab was cleared at the Hub.
    sql(
        &f,
        format!("UPDATE credit_balances SET outstanding_minor = 0 WHERE player_id = '{p}'"),
    )
    .await;
    let cleared = f.writer.apply_command(remind(2, &p)).await.unwrap();
    assert_eq!(refusal(&cleared), Some(ErrorCode::Stale));
    sql(
        &f,
        format!("UPDATE credit_balances SET outstanding_minor = 6000 WHERE player_id = '{p}'"),
    )
    .await;

    // The switch is off.
    set_key(&f, KEY_NOTIFY_CREDIT_REMINDER, "false").await;
    let off = f.writer.apply_command(remind(4, &p)).await.unwrap();
    assert_eq!(refusal(&off), Some(ErrorCode::ValidationFailed));
    set_key(&f, KEY_NOTIFY_CREDIT_REMINDER, "true").await;

    // The player withdrew consent.
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
    let no_consent = f.writer.apply_command(remind(3, &p)).await.unwrap();
    assert_eq!(refusal(&no_consent), Some(ErrorCode::ConsentRequired));

    assert!(rows(&f).is_empty());

    // A Free branch.
    let free = bootstrapped().await;
    let q = credit_player(&free, "asha", "+919876543210", 100_000).await;
    let (order, rev) = player_bill(&free, &q, 6_000, 60).await;
    end(&free, settle_on_credit(&order, rev, 6_000, false))
        .await
        .unwrap();
    free.clock.set(ist("2027-01-16 12:00"));
    let blocked = free.writer.apply_command(remind(5, &q)).await.unwrap();
    assert_eq!(refusal(&blocked), Some(ErrorCode::FeatureNotEntitled));
}

#[tokio::test]
async fn only_the_credit_reminder_is_open_to_an_owner_among_the_notify_requests() {
    let (f, p) = owing().await;
    f.clock.set(ist("2027-01-16 12:00"));
    let ask = |kind: &str| {
        let mut i = intent(
            owner(),
            IntentPayload::NotifyRequested(NotifyRequested {
                kind: kind.into(),
                player_id: p.clone(),
                session_id: Some("S1".into()),
                params: [("minutes".to_owned(), "5".to_owned())].into(),
            }),
        );
        i.issued_at = f.clock.now_ms();
        i
    };
    let denied = f.writer.submit_intent(ask("session_ending")).await;
    assert_eq!(denied.unwrap_err().code, ErrorCode::PermissionDenied);
    let mut own = intent(owner(), IntentPayload::NotifyRequested(request(&p)));
    own.issued_at = f.clock.now_ms();
    assert!(f.writer.submit_intent(own).await.is_ok());
}

#[tokio::test]
async fn the_summary_lists_the_players_reminded_today_that_are_queued_or_sent() {
    let (f, p) = owing().await;
    let q = credit_player(&f, "ravi", "+919812345678", 100_000).await;
    let (order, rev) = player_bill(&f, &q, 6_000, 30).await;
    end(&f, settle_on_credit(&order, rev, 3_000, false))
        .await
        .unwrap();
    f.clock.set(ist("2027-01-16 12:00"));
    let summary = |f: &Fixture| {
        crate::summary::build_summary(&reopen(f), f.clock.now_ms())
            .unwrap()
            .unwrap()
    };
    assert!(summary(&f).reminded_today.is_empty());

    f.writer.apply_command(remind(1, &p)).await.unwrap();
    f.writer.apply_command(remind(2, &q)).await.unwrap();
    let mut both = vec![p.clone(), q.clone()];
    both.sort();
    assert_eq!(summary(&f).reminded_today, both);

    // A reminder that failed to send no longer counts; one that was sent still does.
    let ids: Vec<String> = rows(&f).iter().map(|r| text(r, "id")).collect();
    let id_of = |player: &str| {
        rows(&f)
            .iter()
            .find(|r| text(r, "player_id") == player)
            .map(|r| text(r, "id"))
            .unwrap()
    };
    assert_eq!(ids.len(), 2);
    sql(
        &f,
        format!(
            "UPDATE notify_outbox SET state = 'failed' WHERE id = '{}'",
            id_of(&q)
        ),
    )
    .await;
    sql(
        &f,
        format!(
            "UPDATE notify_outbox SET state = 'sent' WHERE id = '{}'",
            id_of(&p)
        ),
    )
    .await;
    assert_eq!(summary(&f).reminded_today, std::slice::from_ref(&p));

    // Yesterday's reminder is not today's.
    f.clock.set(ist("2027-01-17 12:00"));
    assert!(summary(&f).reminded_today.is_empty());
    // And the file never names a phone, name or email.
    let json = serde_json::to_string(&summary(&f)).unwrap();
    assert!(!json.contains("+91") && !json.contains("Asha"));
}

//! Messages a running session asks for (Story 8.30): low balance, session ending and the
//! console's five minutes.

use super::prepaid::{begin, sql, system};
use super::receipt::{set_key, whatsapp_branch};
use super::session::{end, end_intent, int, pc, player, sessions, table, text, topped_up};
use super::stock::intent;
use super::*;
use crate::notify_drain::{DrainAction, plan};
use crate::notify_trigger::{Trigger, triggers};
use arena_proto::{
    DeviceKind, KEY_NOTIFY_LOW_BALANCE, KEY_NOTIFY_SESSION_ENDING, NotifyRearm, NotifyRequested,
    SessionMode, UNDO_WINDOW_MS,
};

const MIN: i64 = 60_000;

/// A paid branch with "session ending" on, a device at 60 per hour and a profiled player who
/// agreed to messages and holds `topup` in the wallet. With 100.00 a wallet session runs 90
/// minutes (10.00 stay free, 90.00 are set aside).
async fn running(f: &Fixture, kind: DeviceKind, topup: i64) -> (String, String) {
    whatsapp_branch(f).await;
    set_key(f, KEY_NOTIFY_SESSION_ENDING, "true").await;
    let group = if kind == DeviceKind::Console {
        "console"
    } else {
        "pc"
    };
    let d = pc(f, kind, group, Some(6_000)).await;
    let p = player(f, "asha", "+919876543210").await;
    topped_up(f, &p, topup).await;
    begin(f, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    (d, p)
}

fn due(f: &Fixture) -> Vec<Trigger> {
    triggers(&reopen(f), f.clock.now_ms()).unwrap()
}

fn kinds(t: &[Trigger]) -> Vec<String> {
    t.iter()
        .map(|t| match t {
            Trigger::Request(r) => r.kind.clone(),
            Trigger::Rearm(_) => "rearm".into(),
        })
        .collect()
}

async fn fire(f: &Fixture, t: Trigger) -> Result<Applied, AppError> {
    let payload = match t {
        Trigger::Request(r) => IntentPayload::NotifyRequested(r),
        Trigger::Rearm(outbox_id) => IntentPayload::NotifyRearm(NotifyRearm { outbox_id }),
    };
    let mut i = intent(system(), payload);
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

async fn fire_all(f: &Fixture) -> usize {
    let t = due(f);
    let n = t.len();
    for t in t {
        fire(f, t).await.unwrap();
    }
    n
}

struct Queued {
    id: String,
    kind: String,
    order_id: Option<String>,
    session_id: Option<String>,
    state: String,
    not_before: i64,
    params: Option<String>,
}

fn opt(r: &arena_store::Row, col: &str) -> Option<String> {
    match &r[col] {
        RowValue::Text(t) => Some(t.clone()),
        _ => None,
    }
}

fn rows(f: &Fixture) -> Vec<Queued> {
    table(f, Table::NotifyOutbox)
        .iter()
        .map(|r| Queued {
            id: text(r, "id"),
            kind: text(r, "kind"),
            order_id: opt(r, "order_id"),
            session_id: opt(r, "session_id"),
            state: text(r, "state"),
            not_before: int(r, "not_before"),
            params: opt(r, "params"),
        })
        .collect()
}

#[tokio::test]
async fn a_pc_session_five_minutes_from_its_end_queues_one_session_ending_row() {
    let f = bootstrapped().await;
    let (_, p) = running(&f, DeviceKind::Pc, 10_000).await;
    f.clock.advance(84 * MIN + 59_000 - (f.clock.now_ms() - T0));
    assert!(due(&f).is_empty(), "more than five minutes left");

    f.clock.advance(1_000);
    let t = due(&f);
    assert_eq!(kinds(&t), ["session_ending"]);
    let Trigger::Request(r) = &t[0] else {
        panic!("a request");
    };
    assert_eq!(r.params["minutes"], "5");
    assert_eq!(r.player_id, p);
    fire(&f, t[0].clone()).await.unwrap();

    let row = &rows(&f)[0];
    assert_eq!(row.kind, "session_ending");
    assert_eq!(row.order_id, None);
    assert_eq!(row.session_id.as_deref(), Some(r.session_id.as_str()));
    assert_eq!(row.state, "queued");
    assert_eq!(row.not_before, f.clock.now_ms() + UNDO_WINDOW_MS);
    assert_eq!(row.params.as_deref(), Some(r#"{"minutes":"5"}"#));

    // One per session: a second pass sees nothing and a forced repeat is refused.
    assert!(due(&f).is_empty());
    let e = fire(&f, t[0].clone()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Stale);
    assert_eq!(rows(&f).len(), 1);
}

#[tokio::test]
async fn an_extend_does_not_repeat_the_message() {
    let f = bootstrapped().await;
    running(&f, DeviceKind::Pc, 10_000).await;
    f.clock.advance(85 * MIN - (f.clock.now_ms() - T0));
    assert_eq!(fire_all(&f).await, 1);

    // The session is extended by 30 minutes, then runs down to five minutes again.
    let s = &sessions(&f)[0];
    let ends = int(s, "ends_at") + 30 * MIN;
    sql(
        &f,
        format!(
            "UPDATE sessions SET ends_at = {ends} WHERE id = '{}'",
            text(s, "id")
        ),
    )
    .await;
    assert!(due(&f).is_empty());
    f.clock.advance(30 * MIN);
    assert!(due(&f).is_empty(), "the session already had its message");
    assert_eq!(rows(&f).len(), 1);
}

#[tokio::test]
async fn a_console_session_names_the_tv() {
    let f = bootstrapped().await;
    running(&f, DeviceKind::Console, 10_000).await;
    f.clock.advance(85 * MIN);
    let t = due(&f);
    assert_eq!(kinds(&t), ["console_5min"]);
    let Trigger::Request(r) = &t[0] else {
        panic!("a request");
    };
    assert_eq!(r.params.keys().collect::<Vec<_>>(), ["console"]);
    assert!(!r.params["console"].is_empty());
    fire(&f, t[0].clone()).await.unwrap();
    assert_eq!(rows(&f)[0].kind, "console_5min");
}

#[tokio::test]
async fn a_guest_a_missing_consent_a_free_branch_or_a_message_turned_off_queues_nothing() {
    // The message is switched off.
    let f = bootstrapped().await;
    running(&f, DeviceKind::Pc, 10_000).await;
    set_key(&f, KEY_NOTIFY_SESSION_ENDING, "false").await;
    f.clock.advance(86 * MIN);
    assert!(due(&f).is_empty());
    // Switched on again, the same moment queues it: the switch was the only thing missing.
    set_key(&f, KEY_NOTIFY_SESSION_ENDING, "true").await;
    assert_eq!(kinds(&due(&f)), ["session_ending"]);

    // The player withdrew consent.
    let (_, p) = (
        String::new(),
        text(&table(&f, Table::Players)[0], "player_id"),
    );
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
    assert!(due(&f).is_empty());
    let e = fire(
        &f,
        Trigger::Request(NotifyRequested {
            kind: "session_ending".into(),
            player_id: p,
            session_id: text(&sessions(&f)[0], "id"),
            params: [("minutes".to_owned(), "4".to_owned())].into(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::ConsentRequired);

    // A guest has no player to message, and a Free branch sends nothing at all.
    let g = bootstrapped().await;
    whatsapp_branch(&g).await;
    set_key(&g, KEY_NOTIFY_SESSION_ENDING, "true").await;
    let d = pc(&g, DeviceKind::Pc, "pc", Some(6_000)).await;
    begin(&g, &d, None, SessionMode::Postpaid, None).await.ok();
    g.clock.advance(86 * MIN);
    assert!(due(&g).is_empty());
    let free = bootstrapped().await;
    let d = pc(&free, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&free, "asha", "+919876543210").await;
    topped_up(&free, &p, 10_000).await;
    begin(&free, &d, Some(&p), SessionMode::Wallet, None)
        .await
        .unwrap();
    free.clock.advance(86 * MIN);
    assert!(due(&free).is_empty());
}

#[tokio::test]
async fn a_low_balance_message_goes_out_once_per_crossing() {
    let f = bootstrapped().await;
    let (_, p) = running(&f, DeviceKind::Pc, 10_000).await;
    set_key(&f, KEY_NOTIFY_LOW_BALANCE, "true").await;
    // 10.00 in the wallet, 9.00 of it set aside for 90 minutes: the balance is above 5.00 until
    // 40 minutes are left (1 000 free + 9 000 * minutes left / 90).
    f.clock.advance(49 * MIN);
    assert!(due(&f).is_empty());
    f.clock.advance(2 * MIN);
    let t = due(&f);
    assert_eq!(kinds(&t), ["low_balance"]);
    let Trigger::Request(r) = &t[0] else {
        panic!("a request");
    };
    let left: i64 = r.params["amount_minor"].parse().unwrap();
    assert!(left <= 5_000 && left > 4_000, "{left}");
    fire(&f, t[0].clone()).await.unwrap();
    assert!(due(&f).is_empty(), "still the same crossing");
    assert_eq!(rows(&f).len(), 1);

    // A top-up lifts the wallet above the level: the crossing is closed (one pass, once).
    topped_up(&f, &p, 100_000).await;
    let t = due(&f);
    assert_eq!(kinds(&t), ["rearm"]);
    fire(&f, t[0].clone()).await.unwrap();
    assert!(due(&f).is_empty());
    assert_eq!(
        fire(&f, t[0].clone()).await.unwrap_err().code,
        ErrorCode::Stale
    );

    // Falling below the level again is a new crossing and a second message.
    set_key(&f, "billing.low_balance_minor", "99999999").await;
    assert_eq!(kinds(&due(&f)), ["low_balance"]);
    fire_all(&f).await;
    assert_eq!(rows(&f).len(), 2);
}

#[tokio::test]
async fn the_drain_skips_a_session_message_when_the_session_ended_or_was_extended() {
    let f = bootstrapped().await;
    running(&f, DeviceKind::Pc, 10_000).await;
    f.clock.advance(85 * MIN);
    fire_all(&f).await;
    let d = |f: &Fixture| {
        plan(&reopen(f), &super::players::KEK, f.clock.now_ms(), 20)
            .unwrap()
            .into_iter()
            .map(|i| i.action)
            .collect::<Vec<_>>()
    };
    assert!(d(&f).is_empty(), "inside the Undo window");

    f.clock.advance(UNDO_WINDOW_MS + 1);
    let DrainAction::Send(req) = &d(&f)[0] else {
        panic!("a running session five minutes from its end is sent");
    };
    assert_eq!(req.kind, "session_ending");
    assert_eq!(req.params["minutes"], "5");
    assert!(req.params.contains_key("name"));
    assert!(!req.params.contains_key("amount_minor"));

    // Extended meanwhile: more than five minutes are left, so nothing is sent.
    let s = &sessions(&f)[0];
    let (id, ends) = (text(s, "id"), int(s, "ends_at"));
    sql(
        &f,
        format!(
            "UPDATE sessions SET ends_at = {} WHERE id = '{id}'",
            ends + 30 * MIN
        ),
    )
    .await;
    assert_eq!(d(&f), [DrainAction::Skip("session_changed".into())]);

    // Back to the original end, then the session is ended by staff.
    sql(
        &f,
        format!("UPDATE sessions SET ends_at = {ends} WHERE id = '{id}'"),
    )
    .await;
    assert!(matches!(d(&f)[0], DrainAction::Send(_)));
    let rev = int(&sessions(&f)[0], "rev");
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(d(&f), [DrainAction::Skip("session_changed".into())]);
}

#[tokio::test]
async fn a_sent_session_message_leaves_no_sale_to_lock() {
    let f = bootstrapped().await;
    running(&f, DeviceKind::Pc, 10_000).await;
    f.clock.advance(85 * MIN);
    fire_all(&f).await;
    let id = rows(&f)[0].id.clone();
    let mut i = intent(
        system(),
        IntentPayload::NotifySent(arena_proto::NotifySent {
            outbox_id: id.clone(),
            channel: arena_proto::NotifyChannel::Whatsapp,
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await.unwrap();
    assert_eq!(rows(&f)[0].state, "sent");
    let mut effects = 0;
    reopen(&f)
        .write_tx(|tx| {
            effects = tx
                .query_row("SELECT COUNT(*) FROM irreversible_effects", [], |r| {
                    r.get(0)
                })
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(effects, 0, "a message about a session locks no sale");
}

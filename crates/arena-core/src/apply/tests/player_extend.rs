//! Story 6.21: a paired device extends the session on its own PC for its player, and the quotes
//! and sources the kiosk's `extend-options` panel shows.

use super::prepaid::{group_of, ist, owned_pack, pack_session, sql, wallet_session};
use super::session::{
    balance, credit_intent, int, pc, player, row_of, sessions, table, text, topped_up,
};
use super::stock::{intent, owner};
use super::*;
use crate::player_extend::{extend_options, extend_target};
use arena_proto::{DeviceKind, ExtendSource, SessionExtend, SessionMode};

const MIN: i64 = 60_000;

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

async fn extend_as(
    f: &Fixture,
    actor: &str,
    session: &str,
    minutes: i64,
    source: ExtendSource,
    over: bool,
) -> Result<Applied, AppError> {
    let mut i = intent(
        device(actor),
        IntentPayload::SessionExtend(SessionExtend {
            session_id: session.into(),
            minutes,
            source,
            pack_id: None,
            credit_override: over.then_some(true),
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn ends_at(f: &Fixture, session: &str) -> i64 {
    int(&row_of(f, Table::Sessions, session), "ends_at")
}

#[tokio::test]
async fn a_device_actor_extends_the_session_on_its_own_pc_and_nowhere_else() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    // Another PC's device may not extend it.
    let denied = extend_as(&f, &other, &s, 60, ExtendSource::Wallet, false)
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    assert_eq!(ends_at(&f, &s), T0 + 60 * MIN);
    // Its own PC may; the player pays from the wallet at the live rate.
    extend_as(&f, &d, &s, 60, ExtendSource::Wallet, false)
        .await
        .unwrap();
    assert_eq!(ends_at(&f, &s), T0 + 120 * MIN);
    assert_eq!(balance(&f, &p), 4_000);
    let order = table(&f, Table::Orders)
        .into_iter()
        .find(|o| matches!(&o["extends_session_id"], RowValue::Text(t) if *t == s))
        .unwrap();
    assert_eq!(text(&order, "settled_by"), d);
}

#[tokio::test]
async fn a_device_actor_has_no_credit_override() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let i = intent(owner(), credit_intent(&p, 5_000, &[3, 7, 14]).payload);
    f.writer.submit_intent(i).await.unwrap();
    // Over the limit without an override: the typed refusal the kiosk shows.
    let e = extend_as(&f, &d, &s, 120, ExtendSource::Credit, false)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::CreditLimit);
    // An override is a manager's, never a kiosk's.
    let e = extend_as(&f, &d, &s, 120, ExtendSource::Credit, true)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(ends_at(&f, &s), T0 + 60 * MIN);
}

#[tokio::test]
async fn a_wallet_that_cannot_cover_the_extend_is_refused_with_the_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 4_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let e = extend_as(&f, &d, &s, 60, ExtendSource::Wallet, false)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert_eq!(e.message, "Balance ₹40.00. Top up at counter.");
    assert_eq!(balance(&f, &p), 4_000);
}

// ---- the panel's quotes and sources ----

fn reopen(f: &Fixture) -> Store {
    Store::open(&f.path, T0).unwrap()
}

#[tokio::test]
async fn quotes_are_the_three_steps_priced_at_the_live_rate_and_match_the_charge() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(5_001)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 60).await;
    let s = pack_session(&f, &d, &p, &pack).await;
    let r = extend_options(&reopen(&f), &d, f.clock.now_ms()).unwrap();
    let quoted: Vec<_> = r
        .quotes
        .iter()
        .map(|q| (q.minutes, q.price_minor))
        .collect();
    // Half-up, exactly as `session.extend` charges (5001 * 30 / 60 = 2500.5).
    assert_eq!(quoted, [(30, 2_501), (60, 5_001), (120, 10_002)]);
    assert_eq!(r.wallet_minor, 20_000);
    assert_eq!(extend_target(&reopen(&f), &d).unwrap(), s);
    extend_as(&f, &d, &s, 30, ExtendSource::Wallet, false)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 20_000 - 2_501);
}

#[tokio::test]
async fn sources_are_the_wallet_net_of_reserved_the_groups_other_packs_and_credit_headroom() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", Some(9_000)).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 30_000).await;
    let own = group_of(&f, &d);
    let running = owned_pack(&f, &p, std::slice::from_ref(&own), 60).await;
    let spare = owned_pack(&f, &p, std::slice::from_ref(&own), 90).await;
    let _foreign = owned_pack(&f, &p, &[group_of(&f, &other)], 90).await;
    let _empty = owned_pack(&f, &p, std::slice::from_ref(&own), 0).await;
    let s = pack_session(&f, &d, &p, &running).await;
    let r = extend_options(&reopen(&f), &d, f.clock.now_ms()).unwrap();
    // The session's own pack, another group's pack and a used-up pack are not offered.
    let packs: Vec<_> = r
        .packs
        .iter()
        .map(|k| (k.pack_id.clone(), k.minutes_left))
        .collect();
    assert_eq!(packs, [(spare, 90)]);
    assert_eq!(r.credit_headroom_minor, None);
    f.writer
        .submit_intent(intent(
            owner(),
            credit_intent(&p, 20_000, &[3, 7, 14]).payload,
        ))
        .await
        .unwrap();
    extend_as(&f, &d, &s, 30, ExtendSource::Credit, false)
        .await
        .unwrap();
    let r = extend_options(&reopen(&f), &d, f.clock.now_ms()).unwrap();
    assert_eq!(r.credit_headroom_minor, Some(20_000 - 3_000));
}

#[tokio::test]
async fn a_wallet_session_quotes_the_wallet_less_what_it_sets_aside() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let s = wallet_session(&f, &d, &p).await;
    let reserved = int(&sessions(&f)[0], "reserved_minor");
    assert!(reserved > 0);
    let r = extend_options(&reopen(&f), &d, f.clock.now_ms()).unwrap();
    assert_eq!(r.wallet_minor, 50_000 - reserved);
    assert_eq!(extend_target(&reopen(&f), &d).unwrap(), s);
}

#[tokio::test]
async fn the_closing_time_is_named_in_the_branch_s_own_words() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 20_000).await;
    let open = r#"{"open_min":600,"close_min":1380}"#;
    let week = [open; 7].join(",");
    sql(
        &f,
        format!("UPDATE settings SET value = '[{week}]' WHERE key = 'branch.opening_hours'"),
    )
    .await;
    f.clock.set(ist("2027-01-15 21:30"));
    wallet_session(&f, &d, &p).await;
    let r = extend_options(&reopen(&f), &d, f.clock.now_ms()).unwrap();
    assert_eq!(r.closing_label.as_deref(), Some("11:00 pm"));
}

#[tokio::test]
async fn a_postpaid_session_a_guest_and_an_empty_pc_have_no_extend() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let guest_pc = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let idle = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    super::prepaid::begin(&f, &d, Some(&p), SessionMode::Postpaid, None)
        .await
        .unwrap();
    super::prepaid::begin(&f, &guest_pc, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    let store = reopen(&f);
    for id in [&d, &guest_pc] {
        let e = extend_options(&store, id, f.clock.now_ms()).unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(
            extend_target(&store, id).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }
    assert_eq!(
        extend_options(&store, &idle, f.clock.now_ms())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

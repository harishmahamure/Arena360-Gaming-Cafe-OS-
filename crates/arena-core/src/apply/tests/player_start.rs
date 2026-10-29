//! Story 6.9: a paired device starts a prepaid session for a logged-in player, a block is paid
//! from the wallet in the same intent, and the plan picker's options.

use super::prepaid::{
    begin_block, group_of, ist, order_of, owned_pack, owned_pass, sql, system, window_plan,
};
use super::session::{
    balance, int, ledger_sum, pc, player, refusal, sessions, state_of, submit, table, text,
    topped_up,
};
use super::stock::{intent, owner};
use super::*;
use crate::player_plans::player_options;
use arena_proto::{DeviceKind, PlanTag, PlayerChoice, SessionMode, SessionStart};

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn start(
    device_id: &str,
    player: Option<&str>,
    mode: SessionMode,
    pack: Option<&str>,
    plan: Option<&str>,
    pay_from_wallet: bool,
) -> IntentPayload {
    IntentPayload::SessionStart(SessionStart {
        device_id: device_id.into(),
        mode,
        player_id: player.map(Into::into),
        pack_id: pack.map(Into::into),
        plan_id: plan.map(Into::into),
        group_with: None,
        pay_from_wallet,
    })
}

async fn as_device(
    f: &Fixture,
    actor_device: &str,
    payload: IntentPayload,
) -> Result<Applied, AppError> {
    submit(f, device(actor_device), payload).await
}

fn rows(f: &Fixture, t: Table) -> usize {
    table(f, t).len()
}

#[tokio::test]
async fn a_device_actor_starts_a_pack_for_its_own_pc_and_the_session_keeps_the_player() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let pack = owned_pack(&f, &p, &[group_of(&f, &d)], 120).await;
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Pack, Some(&pack), None, false),
    )
    .await
    .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(text(s, "player_id"), p);
    assert_eq!(text(s, "mode"), "pack");
    assert_eq!(state_of(&f, &d), "in_session");
    // The change log shows the device actor.
    let changes = reopen(&f).changes().unwrap();
    let session_change = changes.iter().rev().find(|c| c.tbl == "sessions").unwrap();
    assert_eq!(
        (
            session_change.actor_kind.as_str(),
            session_change.actor_id.as_str()
        ),
        ("device", d.as_str())
    );
}

#[tokio::test]
async fn a_device_actor_may_not_start_postpaid_a_guest_or_another_pc() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let denied = |r: Result<Applied, AppError>| r.unwrap_err().code;
    assert_eq!(
        denied(
            as_device(
                &f,
                &d,
                start(&d, Some(&p), SessionMode::Postpaid, None, None, false)
            )
            .await
        ),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        denied(
            as_device(
                &f,
                &d,
                start(&d, None, SessionMode::Wallet, None, None, false)
            )
            .await
        ),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        denied(
            as_device(
                &f,
                &d,
                start(&other, Some(&p), SessionMode::Wallet, None, None, false)
            )
            .await
        ),
        ErrorCode::PermissionDenied
    );
    assert!(sessions(&f).is_empty());
    assert_eq!(state_of(&f, &d), state_of(&f, &other));
    // The same start as the device it names goes through.
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    assert_eq!(sessions(&f).len(), 1);
}

#[tokio::test]
async fn a_device_actor_ends_only_the_session_on_its_own_pc_and_holds_no_other_session_right() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    let id = text(&sessions(&f)[0], "id");
    let end_as = |actor: &str, rev: i64| {
        let end = IntentPayload::SessionEnd(arena_proto::SessionEnd {
            session_id: id.clone(),
        });
        let mut i = intent(device(actor), end);
        i.base_rev = Some(rev);
        i.issued_at = f.clock.now_ms();
        i
    };
    let rev = super::session::rev_of(&f, Table::Sessions, &id);
    // Another PC's device may not end it.
    assert_eq!(
        f.writer
            .submit_intent(end_as(&other, rev))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_ne!(text(&sessions(&f)[0], "state"), "ended");
    // Its own PC may, with the session's own rev.
    f.writer.submit_intent(end_as(&d, rev)).await.unwrap();
    assert_eq!(text(&sessions(&f)[0], "state"), "ended");
}

#[tokio::test]
async fn a_device_actor_may_not_force_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    let id = text(&sessions(&f)[0], "id");
    let force = IntentPayload::SessionForceEnd(arena_proto::SessionForceEnd {
        session_id: id,
        note: None,
    });
    let mut i = intent(device(&d), force);
    i.base_rev = Some(0);
    i.issued_at = f.clock.now_ms();
    assert_eq!(
        f.writer.submit_intent(i).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn a_block_is_paid_from_the_wallet_in_the_same_intent_and_ends_at_the_window_end() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Block, None, Some(&plan), true),
    )
    .await
    .unwrap();
    let s = &sessions(&f)[0];
    assert_eq!(int(s, "ends_at"), ist("2027-01-16 06:00"));
    assert_eq!(text(s, "player_id"), p);
    assert_eq!(balance(&f, &p), 30_000);
    assert_eq!(ledger_sum(&f, &p), 30_000);
    let order = order_of(&f, &text(s, "id"));
    assert_eq!(
        (text(&order, "status").as_str(), int(&order, "total_minor")),
        ("settled", 20_000)
    );
    let pays: Vec<_> = table(&f, Table::Payments)
        .into_iter()
        .filter(|r| text(r, "order_id") == text(&order, "id"))
        .collect();
    assert_eq!(pays.len(), 1);
    assert_eq!(text(&pays[0], "method"), "wallet");
    assert_eq!(int(&pays[0], "amount_minor"), 20_000);
    let bill = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "reason") == "bill")
        .unwrap();
    assert_eq!(int(&bill, "delta_minor"), -20_000);
}

#[tokio::test]
async fn a_block_the_wallet_cannot_cover_is_refused_and_writes_nothing() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 19_999).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let (sessions_before, txns_before) = (rows(&f, Table::Orders), rows(&f, Table::WalletTxns));
    assert_eq!(
        refusal(
            as_device(
                &f,
                &d,
                start(&d, Some(&p), SessionMode::Block, None, Some(&plan), true),
            )
            .await
        ),
        (ErrorCode::InsufficientBalance, Some("player_id".into()))
    );
    assert!(sessions(&f).is_empty());
    assert_eq!(rows(&f, Table::Orders), sessions_before);
    assert_eq!(rows(&f, Table::WalletTxns), txns_before);
    assert_eq!(balance(&f, &p), 19_999);
    // Exactly the price is enough, and the wallet lands on zero, never below.
    topped_up(&f, &p, 1).await;
    as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Block, None, Some(&plan), true),
    )
    .await
    .unwrap();
    assert_eq!(balance(&f, &p), 0);
}

#[tokio::test]
async fn the_block_check_uses_the_wallet_less_what_a_running_wallet_session_sets_aside() {
    let f = bootstrapped().await;
    let (rated, blocked) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", None).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 25_000).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &blocked)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    as_device(
        &f,
        &rated,
        start(&rated, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    // Most of the wallet is set aside for the running session.
    let e = as_device(
        &f,
        &blocked,
        start(
            &blocked,
            Some(&p),
            SessionMode::Block,
            None,
            Some(&plan),
            true,
        ),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert!(e.message.contains("Balance"), "{}", e.message);
}

#[tokio::test]
async fn pay_from_wallet_is_for_a_block_with_a_player_only() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let field = |c, f: &str| (c, Some(f.to_owned()));
    let wrong = [
        start(&d, Some(&p), SessionMode::Wallet, None, None, true),
        start(&d, None, SessionMode::Block, None, Some(&plan), true),
        start(&d, Some(&p), SessionMode::Postpaid, None, None, true),
    ];
    for payload in wrong {
        assert_eq!(
            refusal(submit(&f, owner(), payload).await),
            field(ErrorCode::ValidationFailed, "pay_from_wallet")
        );
    }
    assert!(sessions(&f).is_empty());
}

#[tokio::test]
async fn a_staff_started_block_is_still_billed_unpaid() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 50_000).await;
    let plan = window_plan(&f, "block", &[group_of(&f, &d)], 20_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    begin_block(&f, &d, Some(&p), &plan, SessionMode::Block)
        .await
        .unwrap();
    assert_eq!(balance(&f, &p), 50_000);
    let o = table(&f, Table::Orders)
        .into_iter()
        .find(|o| text(o, "kind") != "topup")
        .unwrap();
    assert_eq!(text(&o, "status"), "open");
}

#[tokio::test]
async fn stale_choices_and_a_busy_pc_are_refused_with_their_codes() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", None).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    let q = player(&f, "bela", "+919876543211").await;
    topped_up(&f, &q, 50_000).await;
    let (own, foreign) = (group_of(&f, &d), group_of(&f, &other));
    let for_other = owned_pack(&f, &p, std::slice::from_ref(&foreign), 60).await;
    let expired = owned_pack(&f, &p, std::slice::from_ref(&own), 60).await;
    sql(
        &f,
        format!("UPDATE player_packs SET expires_at = 1 WHERE id = '{expired}'"),
    )
    .await;
    let pass = owned_pass(&f, &p, std::slice::from_ref(&own)).await;
    f.clock.set(ist("2027-01-15 14:00"));
    let code = |r: Result<Applied, AppError>| r.unwrap_err().code;
    assert_eq!(
        code(
            as_device(
                &f,
                &d,
                start(
                    &d,
                    Some(&p),
                    SessionMode::Pack,
                    Some(&for_other),
                    None,
                    false
                )
            )
            .await
        ),
        ErrorCode::PlanNotForGroup
    );
    assert_eq!(
        code(
            as_device(
                &f,
                &d,
                start(&d, Some(&p), SessionMode::Pack, Some(&expired), None, false)
            )
            .await
        ),
        ErrorCode::PackExpired
    );
    assert_eq!(
        code(
            as_device(
                &f,
                &d,
                start(&d, Some(&p), SessionMode::Pass, Some(&pass), None, false)
            )
            .await
        ),
        ErrorCode::PassOutsideWindow
    );
    as_device(
        &f,
        &d,
        start(&d, Some(&q), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    assert_eq!(
        code(
            as_device(
                &f,
                &d,
                start(&d, Some(&q), SessionMode::Wallet, None, None, false)
            )
            .await
        ),
        ErrorCode::PcBusy
    );
}

#[tokio::test]
async fn a_wallet_start_below_the_minimum_names_the_balance() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let e = as_device(
        &f,
        &d,
        start(&d, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InsufficientBalance);
    assert_eq!(e.field.as_deref(), Some("player_id"));
}

// ---- the plan picker's options ----

#[tokio::test]
async fn options_list_this_groups_packs_passes_blocks_then_the_wallet() {
    let f = bootstrapped().await;
    let (d, other) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", Some(9_000)).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 4_000).await;
    let (own, foreign) = (group_of(&f, &d), group_of(&f, &other));
    let pack = owned_pack(&f, &p, std::slice::from_ref(&own), 120).await;
    let _for_other = owned_pack(&f, &p, std::slice::from_ref(&foreign), 60).await;
    let pass = owned_pass(&f, &p, std::slice::from_ref(&own)).await;
    let block = window_plan(&f, "block", std::slice::from_ref(&own), 20_000).await;
    let _other_block = window_plan(&f, "block", std::slice::from_ref(&foreign), 10_000).await;
    f.clock.set(ist("2027-01-15 23:30"));
    let r = player_options(&reopen(&f), &p, &d, f.clock.now_ms()).unwrap();
    let modes: Vec<_> = r
        .options
        .iter()
        .map(|o| match &o.choice {
            PlayerChoice::Pack { pack_id } => ("pack", pack_id.clone()),
            PlayerChoice::Pass { pack_id } => ("pass", pack_id.clone()),
            PlayerChoice::Block { plan_id } => ("block", plan_id.clone()),
            PlayerChoice::Wallet => ("wallet", String::new()),
        })
        .collect();
    assert_eq!(
        modes,
        [
            ("pack", pack),
            ("pass", pass),
            ("block", block),
            ("wallet", String::new())
        ]
    );
    assert!(r.options.iter().all(|o| o.selectable && o.tag.is_none()));
    let wallet = r.wallet.unwrap();
    assert_eq!((wallet.balance_minor, wallet.rate_minor), (4_000, 6_000));
    let blk = &r.options[2];
    assert_eq!(blk.price_minor, Some(20_000));
    assert_eq!(blk.window.as_deref(), Some("22:00–06:00"));
    assert_eq!(blk.ends_label.as_deref(), Some("06:00"));
    assert_eq!(r.options[0].minutes_left, Some(120));
}

#[tokio::test]
async fn an_expired_pack_and_a_closed_pass_or_block_are_listed_but_not_selectable() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let p = player(&f, "asha", "+919876543210").await;
    let own = vec![group_of(&f, &d)];
    let expired = owned_pack(&f, &p, &own, 45).await;
    let empty = owned_pack(&f, &p, &own, 0).await;
    let _pass = owned_pass(&f, &p, &own).await;
    let _block = window_plan(&f, "block", &own, 20_000).await;
    sql(
        &f,
        format!("UPDATE player_packs SET expires_at = 1 WHERE id = '{expired}'"),
    )
    .await;
    f.clock.set(ist("2027-01-15 14:00"));
    let r = player_options(&reopen(&f), &p, &d, f.clock.now_ms()).unwrap();
    let by = |id: &str| {
        r.options
            .iter()
            .find(|o| matches!(&o.choice, PlayerChoice::Pack { pack_id } if pack_id == id))
            .unwrap()
    };
    let e = by(&expired);
    assert_eq!(
        (e.selectable, e.tag, e.minutes_left),
        (false, Some(PlanTag::Expired), Some(45))
    );
    assert_eq!(
        (by(&empty).selectable, by(&empty).tag),
        (false, Some(PlanTag::Expired))
    );
    for o in r.options.iter().filter(|o| {
        matches!(
            o.choice,
            PlayerChoice::Pass { .. } | PlayerChoice::Block { .. }
        )
    }) {
        assert_eq!((o.selectable, o.tag), (false, Some(PlanTag::OutsideWindow)));
        assert_eq!(o.window.as_deref(), Some("22:00–06:00"));
        assert!(o.ends_label.is_none());
    }
    // The wallet is always offered on a rated group.
    assert!(matches!(
        r.options.last().unwrap().choice,
        PlayerChoice::Wallet
    ));
}

#[tokio::test]
async fn a_pc_without_a_group_rate_offers_no_wallet_and_no_group_offers_nothing() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", None).await;
    let p = player(&f, "asha", "+919876543210").await;
    let r = player_options(&reopen(&f), &p, &d, f.clock.now_ms()).unwrap();
    assert!(r.wallet.is_none() && r.options.is_empty());
    let r = player_options(&reopen(&f), &p, "NOPE", f.clock.now_ms()).unwrap();
    assert!(r.wallet.is_none() && r.options.is_empty());
}

#[tokio::test]
async fn the_wallet_balance_is_net_of_what_a_running_wallet_session_sets_aside() {
    let f = bootstrapped().await;
    let (a, b) = (
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
        pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await,
    );
    let p = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &p, 10_000).await;
    as_device(
        &f,
        &a,
        start(&a, Some(&p), SessionMode::Wallet, None, None, false),
    )
    .await
    .unwrap();
    let reserved = int(&sessions(&f)[0], "reserved_minor");
    assert!(reserved > 0);
    let r = player_options(&reopen(&f), &p, &b, f.clock.now_ms()).unwrap();
    assert_eq!(r.wallet.unwrap().balance_minor, 10_000 - reserved);
    let _ = system();
}

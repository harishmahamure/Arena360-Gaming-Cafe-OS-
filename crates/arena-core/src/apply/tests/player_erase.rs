//! Erasing a player's personal data by crypto-shredding (Story 9.1).

use super::players::{KEK, add, form};
use super::prepaid::sql;
use super::session::{credit_player, int, pc, start, table, text, topped_up};
use super::stock::{intent, owner, staff};
use super::*;
use crate::players::{PlayerIndex, open_field_of, profile};
use crate::rbac::ROLE_KITCHEN;
use arena_proto::{DeviceKind, PlayerErase, WalletDisposal};

pub(super) async fn erase(
    f: &Fixture,
    actor: Actor,
    player: &str,
    wallet: Option<WalletDisposal>,
    note: Option<&str>,
) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::PlayerErased(PlayerErase {
            player_id: player.into(),
            wallet,
            note: note.map(str::to_owned),
        }),
    );
    i.issued_at = f.clock.now_ms();
    f.writer.submit_intent(i).await
}

fn player_row(f: &Fixture, id: &str) -> Row {
    table(f, Table::Players)
        .into_iter()
        .find(|r| text(r, "player_id") == id)
        .unwrap()
}

fn change_count(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn refusal(r: Result<Applied, AppError>) -> (ErrorCode, Option<String>) {
    let e = r.unwrap_err();
    (e.code, e.field)
}

#[tokio::test]
async fn an_erasure_tombstones_the_row_shreds_the_key_and_keeps_consent() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+91 98765-43210"))
        .await
        .unwrap();
    let before = player_row(&f, &id);
    let old_name = text(&before, "name_enc");
    let wrapped = reopen(&f).player_key(&id).unwrap().unwrap();
    assert!(open_field_of(&KEK, &id, Some(&wrapped), "name", &old_name).is_ok());

    let applied = erase(&f, owner(), &id, None, None).await.unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect::<Vec<_>>(),
        [("players", ChangeOp::Update)]
    );
    let row = player_row(&f, &id);
    assert_eq!(text(&row, "username"), "erased");
    for col in ["name_enc", "phone_enc", "password_hash", "phone_lookup"] {
        assert_eq!(text(&row, col), "", "{col}");
    }
    for col in ["email_enc", "dob_enc", "notes_enc"] {
        assert_eq!(row[col], RowValue::Null, "{col}");
    }
    assert_eq!(text(&row, "tags"), "[]");
    assert_eq!(int(&row, "erased_at"), T0);
    assert_eq!(int(&row, "rev"), int(&before, "rev") + 1);

    let store = reopen(&f);
    assert_eq!(store.player_key(&id).unwrap(), None, "the key is shredded");
    assert!(store.player(&id).unwrap().is_none());
    assert!(store.player_erased(&id).unwrap());
    assert_eq!(table(&f, Table::Consents).len(), 1, "consent stays");
    // Old ciphertext no longer opens, and says why.
    let e = open_field_of(&KEK, &id, None, "name", &old_name).unwrap_err();
    assert_eq!(e.code, ErrorCode::KeyErased);

    // The change row carries the id, the time and constants, no personal data.
    let last = store.changes().unwrap().pop().unwrap();
    let log = String::from_utf8_lossy(&last.row).into_owned();
    for gone in [old_name.as_str(), "asha@example.com", "$argon2id$stub"] {
        assert!(!log.contains(gone), "{gone}");
    }
    assert!(log.contains(&id) && log.contains("erased"));
}

#[tokio::test]
async fn an_erased_player_is_never_found_and_their_phone_and_username_are_free() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let mut index = PlayerIndex::default();
    assert_eq!(index.search(&reader, &KEK, "98765").unwrap().len(), 1);
    assert!(profile(&reader, &KEK, &id).is_ok());

    erase(&f, owner(), &id, None, None).await.unwrap();
    assert!(index.search(&reader, &KEK, "98765").unwrap().is_empty());
    assert_eq!(
        profile(&reader, &KEK, &id).unwrap_err().code,
        ErrorCode::KeyErased
    );
    assert_eq!(
        profile(&reader, &KEK, "nope").unwrap_err().code,
        ErrorCode::NotFound
    );
    let store = reopen(&f);
    assert!(store.player_revs().unwrap().is_empty());
    let (lookups, names) = store.player_import_keys().unwrap();
    assert!(lookups.is_empty() && names.is_empty());
    assert!(store.player_by_username("erased").unwrap().is_none());

    // The same phone and username can be registered again, and a second erasure shares the
    // tombstone username with the first.
    let (again, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    assert_ne!(again, id);
    erase(&f, owner(), &again, None, None).await.unwrap();
    assert_eq!(
        table(&f, Table::Players)
            .iter()
            .filter(|p| text(p, "username") == "erased")
            .count(),
        2
    );
}

#[tokio::test]
async fn an_unknown_or_already_erased_player_is_not_found() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    erase(&f, owner(), &id, None, None).await.unwrap();
    let before = change_count(&f);
    for who in [id.as_str(), "nobody"] {
        let e = erase(&f, owner(), who, None, None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{who}");
    }
    assert_eq!(change_count(&f), before);
}

#[tokio::test]
async fn an_open_session_or_credit_owed_blocks_the_erasure_and_changes_nothing() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    start(&f, &d, Some(&id)).await.unwrap();
    let before = (change_count(&f), player_row(&f, &id));
    let e = erase(&f, owner(), &id, None, None).await.unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::EraseBlocked, "End the session first")
    );
    assert_eq!((change_count(&f), player_row(&f, &id)), before);

    let g = bootstrapped().await;
    let owing = credit_player(&g, "ravi", "+919812345678", 100_000).await;
    sql(
        &g,
        format!("UPDATE credit_balances SET outstanding_minor = 500 WHERE player_id = '{owing}'"),
    )
    .await;
    let before = (change_count(&g), player_row(&g, &owing));
    let e = erase(&g, owner(), &owing, None, None).await.unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::EraseBlocked, "Collect or write off credit first")
    );
    assert_eq!((change_count(&g), player_row(&g, &owing)), before);
    assert!(reopen(&g).player_key(&owing).unwrap().is_some());
}

#[tokio::test]
async fn a_wallet_balance_needs_its_disposal_and_a_reason_then_is_zeroed_first() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    topped_up(&f, &id, 500).await;
    let before = (change_count(&f), player_row(&f, &id));

    let invalid = |name: &str| (ErrorCode::ValidationFailed, Some(name.to_owned()));
    assert_eq!(
        refusal(erase(&f, owner(), &id, None, None).await),
        invalid("wallet")
    );
    let long = "x".repeat(121);
    for note in [None, Some("  "), Some(long.as_str())] {
        assert_eq!(
            refusal(erase(&f, owner(), &id, Some(WalletDisposal::Forfeited), note).await),
            invalid("note"),
            "{note:?}"
        );
    }
    assert_eq!((change_count(&f), player_row(&f, &id)), before);

    let applied = erase(
        &f,
        owner(),
        &id,
        Some(WalletDisposal::Forfeited),
        Some("player left the area"),
    )
    .await
    .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .map(|c| c.tbl.as_str())
            .collect::<Vec<_>>(),
        ["wallets", "wallet_txns", "players"]
    );
    let wallet = table(&f, Table::Wallets)
        .into_iter()
        .find(|w| text(w, "player_id") == id)
        .unwrap();
    assert_eq!(int(&wallet, "balance_minor"), 0);
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "reason") == "erase_forfeit")
        .unwrap();
    assert_eq!(int(&txn, "delta_minor"), -500);
    assert_eq!(txn["order_id"], RowValue::Null);
    assert_eq!(text(&txn, "player_id"), id);
    assert!(reopen(&f).player_erased(&id).unwrap());
}

#[tokio::test]
async fn a_refunded_balance_writes_the_refund_row_and_no_balance_takes_no_disposal() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let invalid = |name: &str| (ErrorCode::ValidationFailed, Some(name.to_owned()));
    assert_eq!(
        refusal(
            erase(
                &f,
                owner(),
                &id,
                Some(WalletDisposal::Refunded),
                Some("paid out")
            )
            .await
        ),
        invalid("wallet")
    );
    assert_eq!(
        refusal(erase(&f, owner(), &id, None, Some("why")).await),
        invalid("note")
    );

    let (rich, _) = add(&f, owner(), &form("ravi", "+919000000001"))
        .await
        .unwrap();
    topped_up(&f, &rich, 800).await;
    erase(
        &f,
        owner(),
        &rich,
        Some(WalletDisposal::Refunded),
        Some("paid out in cash"),
    )
    .await
    .unwrap();
    let txn = table(&f, Table::WalletTxns)
        .into_iter()
        .find(|t| text(t, "reason") == "erase_refund")
        .unwrap();
    assert_eq!(int(&txn, "delta_minor"), -800);
}

#[tokio::test]
async fn only_a_member_with_player_manage_can_erase() {
    let f = bootstrapped().await;
    let (id, _) = add(&f, owner(), &form("asha", "+919876543210"))
        .await
        .unwrap();
    let cook = staff(&f, "cook", ROLE_KITCHEN).await;
    let e = erase(&f, cook, &id, None, None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert!(!reopen(&f).player_erased(&id).unwrap());
}

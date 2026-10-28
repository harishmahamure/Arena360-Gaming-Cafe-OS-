use std::collections::HashSet;

use super::stock::{intent, owner, staff};
use super::*;
use crate::import_players::{PlayerLine, seal_rows};
use crate::players::{KEK_LEN, open_field};
use crate::rbac::{ROLE_COUNTER, ROLE_KITCHEN, ROLE_MANAGER};
use arena_proto::{ImportPlayerRow, ImportPlayers};

const KEK: [u8; KEK_LEN] = [9; KEK_LEN];

fn line(n: u32, balance: i64, limit: i64) -> PlayerLine {
    PlayerLine {
        phone: format!("+9198000{n:05}"),
        name: format!("Player {n}"),
        balance_minor: balance,
        credit_limit_minor: limit,
    }
}

fn rows(lines: &[PlayerLine]) -> Vec<ImportPlayerRow> {
    seal_rows(&KEK, lines, &mut HashSet::new()).unwrap()
}

async fn import(
    f: &Fixture,
    actor: Actor,
    rows: Vec<ImportPlayerRow>,
) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(
            actor,
            IntentPayload::ImportPlayers(ImportPlayers { rows }),
        ))
        .await
}

fn state(f: &Fixture) -> (usize, usize, usize, usize, usize, usize) {
    let s = reopen(f);
    (
        s.rows(Table::Players).unwrap().len(),
        s.rows(Table::Consents).unwrap().len(),
        s.rows(Table::Wallets).unwrap().len(),
        s.rows(Table::WalletTxns).unwrap().len(),
        s.rows(Table::CreditBalances).unwrap().len(),
        s.changes().unwrap().len(),
    )
}

#[tokio::test]
async fn a_chunk_creates_sealed_profiles_with_balances_credit_and_no_cash_or_invoice() {
    let f = bootstrapped().await;
    let before = state(&f);
    let sealed = rows(&[line(1, 50_050, 0), line(2, 0, 200_000), line(3, 100, 5_000)]);
    let a = import(&f, owner(), sealed.clone()).await.unwrap();
    let after = state(&f);
    assert_eq!(
        (after.0, after.1, after.2, after.3, after.4),
        (3, 3, 3, 2, 2),
        "3 players, 3 consents, 3 wallets, 2 ledger rows, 2 credit accounts"
    );
    assert_eq!(after.5 - before.5, a.changes.len());
    let s = reopen(&f);
    let first = s
        .rows(Table::Players)
        .unwrap()
        .into_iter()
        .find(|p| p["player_id"] == RowValue::Text(sealed[0].player_id.clone()))
        .unwrap();
    assert_eq!(first["password_hash"], RowValue::Text("!".into()));
    assert_eq!(
        first["username"],
        RowValue::Text(sealed[0].username.clone())
    );
    assert!(
        !format!("{first:?}").contains("Player 1"),
        "the name is ciphertext"
    );
    let name = open_field(
        &KEK,
        &sealed[0].player_id,
        &sealed[0].wrapped_key,
        "name",
        &sealed[0].name_enc,
    )
    .unwrap();
    assert_eq!(name, "Player 1");
    for c in s.rows(Table::Consents).unwrap() {
        assert_eq!(c["granted"], RowValue::Integer(0), "consent is not granted");
        assert_eq!(c["policy_version"], RowValue::Text("import".into()));
    }
    let wallets = s.rows(Table::Wallets).unwrap();
    let balance = |id: &str| {
        let w = wallets
            .iter()
            .find(|w| w["player_id"] == RowValue::Text(id.into()))
            .unwrap();
        w["balance_minor"].clone()
    };
    assert_eq!(balance(&sealed[0].player_id), RowValue::Integer(50_050));
    assert_eq!(balance(&sealed[1].player_id), RowValue::Integer(0));
    let txns = s.rows(Table::WalletTxns).unwrap();
    assert!(txns.iter().all(|t| {
        t["reason"] == RowValue::Text("import".into()) && t["order_id"] == RowValue::Null
    }));
    let credit = s.rows(Table::CreditBalances).unwrap();
    assert!(credit.iter().any(|c| {
        c["player_id"] == RowValue::Text(sealed[1].player_id.clone())
            && c["limit_minor"] == RowValue::Integer(200_000)
            && c["outstanding_minor"] == RowValue::Integer(0)
    }));
    assert_eq!(
        s.rows(Table::Orders).unwrap().len(),
        0,
        "no order, no invoice"
    );
    assert_eq!(s.rows(Table::CashEvents).unwrap().len(), 0, "no cash event");
}

#[tokio::test]
async fn a_retry_with_the_same_key_never_duplicates() {
    let f = bootstrapped().await;
    let it = intent(
        owner(),
        IntentPayload::ImportPlayers(ImportPlayers {
            rows: rows(&[line(1, 500, 0)]),
        }),
    );
    let first = f.writer.submit_intent(it.clone()).await.unwrap();
    let before = state(&f);
    let again = f.writer.submit_intent(it).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn one_bad_row_fails_the_whole_chunk_and_writes_nothing() {
    let f = bootstrapped().await;
    import(&f, owner(), rows(&[line(1, 0, 0)])).await.unwrap();
    let before = state(&f);
    let good = rows(&[line(2, 0, 0), line(3, 0, 0)]);
    let mut bad_ciphertext = good.clone();
    bad_ciphertext[1].name_enc = "nope".into();
    let mut bad_lookup = good.clone();
    bad_lookup[1].phone_lookup = "xyz".into();
    let mut negative = good.clone();
    negative[1].balance_minor = -1;
    let mut huge = good.clone();
    huge[1].credit_limit_minor = i64::MAX;
    let mut same_username = good.clone();
    same_username[1].username = same_username[0].username.clone();
    let mut same_phone = good.clone();
    same_phone[1].phone_lookup = same_phone[0].phone_lookup.clone();
    let mut known_phone = good.clone();
    known_phone[1].phone_lookup = rows(&[line(1, 0, 0)])[0].phone_lookup.clone();
    let mut known_name = good.clone();
    known_name[1].username = reopen(&f)
        .rows(Table::Players)
        .unwrap()
        .remove(0)
        .remove("username")
        .map(|v| match v {
            RowValue::Text(t) => t,
            other => panic!("{other:?}"),
        })
        .unwrap();
    for (chunk, code, field) in [
        (
            bad_ciphertext,
            ErrorCode::ValidationFailed,
            "rows[1].name_enc",
        ),
        (
            bad_lookup,
            ErrorCode::ValidationFailed,
            "rows[1].phone_lookup",
        ),
        (
            negative,
            ErrorCode::ValidationFailed,
            "rows[1].balance_minor",
        ),
        (
            huge,
            ErrorCode::ValidationFailed,
            "rows[1].credit_limit_minor",
        ),
        (same_username, ErrorCode::UsernameTaken, "rows[1].username"),
        (same_phone, ErrorCode::PhoneTaken, "rows[1].phone"),
        (known_phone, ErrorCode::PhoneTaken, "rows[1].phone"),
        (known_name, ErrorCode::UsernameTaken, "rows[1].username"),
        (Vec::new(), ErrorCode::ValidationFailed, "rows"),
    ] {
        let e = import(&f, owner(), chunk).await.unwrap_err();
        assert_eq!((e.code, e.field.as_deref()), (code, Some(field)), "{e:?}");
        assert_eq!(state(&f), before);
    }
    let too_many: Vec<PlayerLine> = (0..501).map(|n| line(100 + n, 0, 0)).collect();
    let e = import(&f, owner(), rows(&too_many)).await.unwrap_err();
    assert_eq!(
        (e.code, e.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("rows"))
    );
    assert_eq!(state(&f), before);
}

#[tokio::test]
async fn player_manage_is_required_and_balances_and_limits_need_their_own_permissions() {
    let f = bootstrapped().await;
    let kitchen = staff(&f, "cook", ROLE_KITCHEN).await;
    let e = import(&f, kitchen, rows(&[line(1, 0, 0)]))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(state(&f).0, 0);

    let counter = staff(&f, "till", ROLE_COUNTER).await;
    import(&f, counter.clone(), rows(&[line(1, 700, 0)]))
        .await
        .unwrap();
    let before = state(&f);
    let e = import(&f, counter, rows(&[line(2, 0, 9_000)]))
        .await
        .unwrap_err();
    assert_eq!(
        e.code,
        ErrorCode::PermissionDenied,
        "credit.manage is needed"
    );
    assert_eq!(state(&f), before);

    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    import(&f, manager, rows(&[line(2, 100, 9_000)]))
        .await
        .unwrap();
    assert_eq!(state(&f).0, 2);
}

#[tokio::test]
async fn a_thousand_player_file_previews_seals_and_commits_in_two_chunks_well_inside_30_seconds() {
    let started = std::time::Instant::now();
    let f = bootstrapped().await;
    let mut csv = String::from("phone,name,balance,credit_limit\r\n");
    for i in 0..1000u32 {
        csv.push_str(&format!(
            "+9197{i:08},Player {i},{}.50,{}\r\n",
            i % 200,
            if i % 4 == 0 { 1000 } else { 0 }
        ));
    }
    let (analysis, mut taken) = {
        let store = reopen(&f);
        let ctx = crate::import_players::context(&store).unwrap();
        let analysis = crate::import_players::analyse(&csv, &ctx, 2, &KEK).unwrap();
        (analysis, ctx.usernames)
    };
    assert_eq!(analysis.preview.failures, Vec::new());
    assert_eq!(
        (analysis.preview.new, analysis.preview.with_credit),
        (1000, 250)
    );
    let sealed = seal_rows(&KEK, &analysis.rows, &mut taken).unwrap();
    for chunk in sealed.chunks(arena_proto::IMPORT_CHUNK_ROWS) {
        import(&f, owner(), chunk.to_vec()).await.unwrap();
    }
    assert_eq!(state(&f).0, 1000);
    let again = {
        let store = reopen(&f);
        let ctx = crate::import_players::context(&store).unwrap();
        crate::import_players::analyse(&csv, &ctx, 2, &KEK).unwrap()
    };
    assert_eq!(
        (again.preview.new, again.preview.matched),
        (0, 1000),
        "a second dry-run finds every phone already imported"
    );
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    eprintln!(
        "1,000 players previewed and committed in {:?}",
        started.elapsed()
    );
}

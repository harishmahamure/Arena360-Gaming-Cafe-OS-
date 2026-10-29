//! Story 6.34: a paired device changes the password of the player on its own PC, and the Account
//! page reads details, wallet and packs, and order history for that player.

use super::players::KEK;
use super::prepaid::{begin, group_of, owned_pack, sql, wallet_session};
use super::session::{balance, pc, player, sessions, submit, table, text, topped_up};
use super::stock::{make, owner, stock_in, up};
use super::*;
use crate::player_account::{account_page, mask_phone};
use arena_proto::{
    DeviceKind, OrderPlace, PlayerPasswordChange, PlayerPasswordReset, PosSaleLine, ProductUpsert,
    SessionMode,
};

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn change(player: &str, hash: &str) -> IntentPayload {
    IntentPayload::PlayerPasswordChange(PlayerPasswordChange {
        player_id: player.into(),
        password_hash: hash.into(),
    })
}

fn hash_of(f: &Fixture, player: &str) -> String {
    reopen(f).player(player).unwrap().unwrap().0.password_hash
}

#[test]
fn a_phone_shows_only_its_last_four_digits() {
    assert_eq!(mask_phone("+919876543210"), "+••••••••3210");
    assert_eq!(mask_phone("+91 98765-43210"), "+•• •••••-•3210");
    assert_eq!(mask_phone("123"), "123");
    assert_eq!(mask_phone(""), "");
}

#[tokio::test]
async fn a_device_changes_the_password_of_the_player_on_its_own_pc_and_no_one_elses() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let other = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    let ravi = player(&f, "ravi", "+919000000001").await;
    topped_up(&f, &asha, 10_000).await;
    // No session on the PC yet: denied.
    let e = submit(&f, device(&d), change(&asha, "$argon2id$new"))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    wallet_session(&f, &d, &asha).await;
    let before = hash_of(&f, &asha);
    // Another PC's device, and another player's password, are denied.
    for (actor, target) in [(&other, &asha), (&d, &ravi)] {
        let e = submit(&f, device(actor), change(target, "$argon2id$new"))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }
    // A plain hash is refused on password_hash and nothing is written.
    let e = submit(&f, device(&d), change(&asha, "plain"))
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("password_hash"));
    assert_eq!(hash_of(&f, &asha), before);
    // A device cannot reset: only change, for its own player.
    let reset = IntentPayload::PlayerPasswordReset(PlayerPasswordReset {
        player_id: asha.clone(),
        password_hash: "$argon2id$new".into(),
    });
    let e = submit(&f, device(&d), reset).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(hash_of(&f, &asha), before);

    let applied = submit(&f, device(&d), change(&asha, "$argon2id$new"))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(hash_of(&f, &asha), "$argon2id$new");
    let store = reopen(&f);
    let logged = store
        .changes()
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.tbl == "players")
        .unwrap();
    assert_eq!(
        (logged.actor_kind.as_str(), logged.actor_id.as_str()),
        ("device", d.as_str())
    );
}

#[tokio::test]
async fn the_account_page_holds_details_wallet_packs_and_orders() {
    let f = bootstrapped().await;
    let d = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let asha = player(&f, "asha", "+919876543210").await;
    topped_up(&f, &asha, 10_000).await;
    let groups = [group_of(&f, &d)];
    owned_pack(&f, &asha, &groups, 90).await;
    let spent = owned_pack(&f, &asha, &groups, 0).await;
    let lapsed = owned_pack(&f, &asha, &groups, 45).await;
    let now = f.clock.now_ms();
    sql(
        &f,
        format!(
            "UPDATE player_packs SET expires_at = {} WHERE id IN ('{spent}', '{lapsed}')",
            now - 1_000
        ),
    )
    .await;

    // An order: two colas on the player's running session, then settled.
    let cola = make(
        &f,
        ProductUpsert {
            price_minor: 4_000,
            track_stock: true,
            ..up("Cola")
        },
    )
    .await;
    stock_in(&f, &cola, 10, 2_000).await;
    begin(&f, &d, Some(&asha), SessionMode::Postpaid, None)
        .await
        .unwrap();
    let session = text(sessions(&f).last().unwrap(), "id");
    submit(
        &f,
        owner(),
        IntentPayload::OrderPlace(OrderPlace {
            counter: None,
            session_id: session.clone(),
            lines: vec![PosSaleLine {
                product_id: cola,
                qty: 2,
            }],
            stock_override: None,
        }),
    )
    .await
    .unwrap();
    let order = table(&f, Table::Orders)
        .iter()
        .find(|o| o["session_id"] == RowValue::Text(session.clone()))
        .map(|o| text(o, "id"))
        .unwrap();
    sql(
        &f,
        format!("UPDATE orders SET invoice_no = 'MUM/26-27/E1/7' WHERE id = '{order}'"),
    )
    .await;

    let store = reopen(&f);
    let page = account_page(&store, &KEK, &asha, now).unwrap();
    assert_eq!(page.name, "Asha Rao");
    assert_eq!(page.username, "asha");
    assert_eq!(page.phone_masked, "+••••••••3210");
    assert_eq!(
        page.balance_minor,
        balance(&f, &asha) - reserved(&store, &asha)
    );
    // The live pack first, then the lapsed one with its unused minutes; the spent one is gone.
    let names: Vec<_> = page
        .packs
        .iter()
        .map(|p| (p.expired, p.minutes_left))
        .collect();
    assert_eq!(names, [(false, Some(90)), (true, Some(45))]);
    assert!(page.packs[0].groups.iter().all(|g| !g.is_empty()));
    assert_eq!(page.orders.len(), 1);
    let o = &page.orders[0];
    assert_eq!(o.order_id, order);
    assert_eq!(o.items.len(), 1);
    assert_eq!((o.items[0].name.as_str(), o.items[0].qty), ("Cola", 2));
    assert_eq!(o.status, "open");
    assert_eq!(o.invoice_no.as_deref(), Some("MUM/26-27/E1/7"));

    // Someone else's page holds none of it.
    let ravi = player(&f, "ravi", "+919000000001").await;
    let theirs = account_page(&reopen(&f), &KEK, &ravi, now).unwrap();
    assert_eq!(
        (
            theirs.packs.len(),
            theirs.orders.len(),
            theirs.balance_minor
        ),
        (0, 0, 0)
    );
    let missing = account_page(&reopen(&f), &KEK, "nobody", now).unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
}

fn reserved(store: &arena_store::Store, player: &str) -> i64 {
    store.reserved_minor(player).unwrap()
}

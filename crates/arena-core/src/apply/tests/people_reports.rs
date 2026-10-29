//! Story 5.30: the Players and Staff reports over rows the Hub really wrote (settles by a staff
//! member, a session start in the change log, a player whose data key is later removed).

use super::prepaid::sql;
use super::session::{pay, pc, start_of, submit, text};
use super::stock::{make, owner, staff, stock_in, up};
use super::*;
use crate::players::KEK_LEN;
use crate::rbac::ROLE_COUNTER;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{DeviceKind, PosSale, PosSaleLine, ProductUpsert};

const KEK: [u8; KEK_LEN] = [9; KEK_LEN];

async fn sell(f: &Fixture, actor: Actor, product: &str, qty: i64, player: Option<&str>) {
    submit(
        f,
        actor,
        IntentPayload::PosSale(PosSale {
            kiosk: None,
            business: None,
            coupon: None,
            discount: None,
            lines: vec![PosSaleLine {
                product_id: product.into(),
                qty,
            }],
            payments: vec![pay(Cash, 5_000 * qty, None)],
            player_id: player.map(Into::into),
            credit_override: None,
            stock_override: None,
            receipt: None,
        }),
    )
    .await
    .unwrap();
}

fn today(f: &Fixture) -> String {
    crate::locale::current_business_day(&reopen(f), f.clock.now_ms())
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn players_and_staff_reports_follow_the_rows_a_counter_shift_wrote() {
    let f = bootstrapped().await;
    let till = staff(&f, "till", ROLE_COUNTER).await;
    let id = make(
        &f,
        ProductUpsert {
            price_minor: 5_000,
            ..up("Burger")
        },
    )
    .await;
    stock_in(&f, &id, 20, 1_000).await;
    let (player, _) = super::players::add(
        &f,
        owner(),
        &super::players::form("Asha_9", "+91 98765-43210"),
    )
    .await
    .unwrap();

    // Two sales to the player and one to a guest, all settled by the counter; the counter also
    // starts a session and the owner starts one that is not the counter's.
    sell(&f, till.clone(), &id, 1, Some(&player)).await;
    sell(&f, till.clone(), &id, 2, Some(&player)).await;
    sell(&f, till.clone(), &id, 1, None).await;
    let d1 = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    let d2 = pc(&f, DeviceKind::Pc, "pc", Some(10_000)).await;
    submit(&f, till.clone(), start_of(&d1, None)).await.unwrap();
    submit(&f, owner(), start_of(&d2, None)).await.unwrap();

    let day = today(&f);
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let now = f.clock.now_ms();

    let players = reader.players_report(&day, &day, now, &KEK).unwrap();
    assert_eq!(players.top.len(), 1);
    let top = &players.top[0];
    assert_eq!(
        (top.name.as_str(), top.erased, top.visits, top.spend_minor),
        ("Asha Rao", false, 1, 15_000),
        "two sales on one business day are one visit"
    );
    assert_eq!((players.guest.bills, players.guest.spend_minor), (1, 5_000));
    assert_eq!(players.new_players.players, 1);
    assert_eq!(players.returning_players.players, 0);
    assert!(
        !serde_json::to_string(&players).unwrap().contains("98765"),
        "no phone in the report"
    );

    let staff_report = reader.staff_report(&day, &day).unwrap();
    let till_row = staff_report
        .staff
        .iter()
        .find(|s| s.staff_id == till.id)
        .unwrap();
    assert_eq!(
        (
            till_row.name.as_str(),
            till_row.sales_count,
            till_row.sales_minor,
            till_row.sessions_started
        ),
        ("till", 3, 20_000, 1),
        "the owner's session is not the counter's"
    );
    assert_eq!(staff_report.from, day);

    // The player's data key goes: the same sales now read as an erased player.
    sql(
        &f,
        format!("DELETE FROM player_keys WHERE player_id = '{player}'"),
    )
    .await;
    let players = reader.players_report(&day, &day, now, &KEK).unwrap();
    assert_eq!(players.top.len(), 1);
    assert!(players.top[0].erased && players.top[0].name.is_empty());
    assert_eq!(players.top[0].spend_minor, 15_000);
    assert_eq!(
        text(&super::session::table(&f, Table::Players)[0], "player_id"),
        player
    );
}

#[tokio::test]
async fn a_range_with_no_activity_reads_as_empty() {
    let f = bootstrapped().await;
    let day = today(&f);
    let reader = crate::SnapshotReader::open(&f.path).unwrap();
    let players = reader
        .players_report(&day, &day, f.clock.now_ms(), &KEK)
        .unwrap();
    assert!(players.top.is_empty() && players.debtors.is_empty());
    assert_eq!(players.credit_buckets.len(), 3);
    assert!(reader.staff_report(&day, &day).unwrap().staff.is_empty());
    assert!(
        reader
            .players_report("2026-10-02", "2026-10-01", 0, &KEK)
            .is_err()
    );
    assert!(reader.staff_report("2026-01-01", "2026-12-31").is_err());
}

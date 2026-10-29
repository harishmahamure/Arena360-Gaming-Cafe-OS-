//! Story 6.31: a player's order from the kiosk joins the running session's bill, and the
//! `kiosk_order` alert it raises closes once the kitchen has served every line.

use super::order_line::serve;
use super::prepaid::sql;
use super::session::{int, pc, refusal, sessions, start, submit, table, text};
use super::stock::{make, owner, stock_in, up};
use super::*;
use arena_proto::{
    AlertKind, AlertRaise, DeviceKind, FLAG_KIOSK_ORDERS, OrderPlace, PosSaleLine, ProductUpsert,
};

fn licence() -> String {
    format!(
        "INSERT INTO settings (branch_id, key, value, rev, updated_at)
         SELECT branch_id, 'licence.flags', '{FLAG_KIOSK_ORDERS}', 0, 0 FROM branch WHERE true
         ON CONFLICT (branch_id, key) DO UPDATE SET value = excluded.value"
    )
}

fn device(id: &str) -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: id.into(),
    }
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub".into(),
    }
}

fn order(session: &str, product: &str, qty: i64, over: Option<bool>) -> IntentPayload {
    IntentPayload::OrderPlace(OrderPlace {
        session_id: session.into(),
        lines: vec![PosSaleLine {
            product_id: product.into(),
            qty,
        }],
        stock_override: over,
    })
}

/// A running postpaid guest session on a fresh PC: its session id and device id.
async fn running(f: &Fixture) -> (String, String) {
    let d = pc(f, DeviceKind::Pc, "pc", Some(10_000)).await;
    start(f, &d, None).await.unwrap();
    (text(sessions(f).last().unwrap(), "id"), d)
}

async fn cola(f: &Fixture, qty: i64, kitchen: bool) -> String {
    let id = make(
        f,
        ProductUpsert {
            price_minor: 4_000,
            track_stock: true,
            kitchen,
            ..up("Cola")
        },
    )
    .await;
    stock_in(f, &id, qty, 2_000).await;
    id
}

#[tokio::test]
async fn a_device_places_an_order_on_the_session_on_its_own_pc() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5, false).await;
    let (session, d) = running(&f).await;

    submit(&f, device(&d), order(&session, &id, 2, None))
        .await
        .unwrap();
    let orders = table(&f, Table::Orders);
    assert_eq!(orders.len(), 1);
    assert_eq!(int(&orders[0], "total_minor"), 8_000);
    let lines = table(&f, Table::OrderLines);
    assert_eq!(text(&lines[0], "placed_by"), d);
}

#[tokio::test]
async fn a_device_cannot_order_on_another_pc_or_override_stock() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5, false).await;
    let (session, mine) = running(&f).await;
    let (_, other) = running(&f).await;

    let (code, _) = refusal(submit(&f, device(&other), order(&session, &id, 1, None)).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let (code, _) = refusal(submit(&f, device(&mine), order(&session, &id, 1, Some(true))).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn a_short_line_is_refused_with_its_index_and_nothing_is_placed() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 1, false).await;
    let (session, d) = running(&f).await;

    let (code, field) = refusal(submit(&f, device(&d), order(&session, &id, 3, None)).await);
    assert_eq!(code, ErrorCode::OutOfStock);
    assert_eq!(field.as_deref(), Some("lines[0]"));
    assert!(table(&f, Table::Orders).is_empty());
}

#[tokio::test]
async fn the_kiosk_order_alert_closes_when_the_last_kitchen_line_is_served() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let id = cola(&f, 5, true).await;
    let (session, d) = running(&f).await;
    submit(&f, device(&d), order(&session, &id, 1, None))
        .await
        .unwrap();
    submit(
        &f,
        system(),
        IntentPayload::AlertRaise(AlertRaise::new(AlertKind::KioskOrder, Some(d.clone()))),
    )
    .await
    .unwrap();
    let open = |f: &Fixture| {
        reopen(f)
            .rows(Table::Alerts)
            .unwrap()
            .into_iter()
            .filter(|a| matches!(&a["acked_at"], RowValue::Null))
            .count()
    };
    assert_eq!(open(&f), 1);

    let line = text(&table(&f, Table::OrderLines)[0], "id");
    serve(&f, owner(), &line).await.unwrap();
    assert_eq!(open(&f), 0);
}

#[tokio::test]
async fn the_menu_says_how_many_can_be_sold_now() {
    let f = bootstrapped().await;
    sql(&f, licence()).await;
    let cola = cola(&f, 5, true).await;
    let untracked = make(
        &f,
        ProductUpsert {
            price_minor: 1_000,
            track_stock: false,
            ..up("Mints")
        },
    )
    .await;
    let empty = make(
        &f,
        ProductUpsert {
            price_minor: 1_000,
            track_stock: true,
            ..up("Water")
        },
    )
    .await;
    let combo = make(
        &f,
        ProductUpsert {
            bundle: true,
            price_minor: 6_000,
            components: vec![arena_proto::BundleComponent {
                product_id: cola.clone(),
                qty: 2,
            }],
            ..up("Combo")
        },
    )
    .await;
    // A kitchen line holds its stock until served: 1 of the 5 is spoken for.
    let (session, d) = running(&f).await;
    submit(&f, device(&d), order(&session, &cola, 1, None))
        .await
        .unwrap();

    let menu = crate::DeviceDirectory::open(&f.path)
        .unwrap()
        .shop_menu(f.clock.now_ms())
        .unwrap();
    let max = |id: &str| menu.items.iter().find(|i| i.id == id).unwrap().max_qty;
    assert_eq!(max(&cola), Some(4));
    assert_eq!(max(&untracked), None);
    assert_eq!(max(&empty), Some(0));
    assert_eq!(max(&combo), Some(2));
}

//! Story 6.30: the kiosk Shop's menu and photos, and the `kiosk_orders` gate on a device's
//! order-capable intents.

use super::prepaid::sql;
use super::session::{pay, submit};
use super::stock::{make, owner, stock_in, up};
use super::*;
use crate::DeviceDirectory;
use arena_proto::PaymentMethod::Cash;
use arena_proto::{
    BundleComponent, FLAG_KIOSK_ORDERS, OrderPlace, PosSale, PosSaleLine, ProductUpsert,
};

/// A JPEG data URL with a header the Hub accepts (not a decodable picture).
fn jpeg() -> String {
    use base64::Engine;
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    b.extend([0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x40, 0x00, 0x40]);
    b.extend([0x01, 0x01, 0x11, 0x00]);
    format!(
        "{}{}",
        arena_proto::IMAGE_PREFIX,
        base64::engine::general_purpose::STANDARD.encode(b)
    )
}

fn licence(flags: &str) -> String {
    format!(
        "INSERT INTO settings (branch_id, key, value, rev, updated_at)
         SELECT branch_id, 'licence.flags', '{flags}', 0, 0 FROM branch WHERE true
         ON CONFLICT (branch_id, key) DO UPDATE SET value = excluded.value"
    )
}

fn device() -> Actor {
    Actor {
        kind: ActorKind::Device,
        id: "01JDEVICEAAAAAAAAAAAAAAAAA".into(),
    }
}

fn place() -> IntentPayload {
    IntentPayload::OrderPlace(OrderPlace {
        counter: None,
        session_id: "S1".into(),
        lines: vec![PosSaleLine {
            product_id: "P1".into(),
            qty: 1,
        }],
        stock_override: None,
    })
}

fn item(name: &str, category: &str, price: i64) -> ProductUpsert {
    ProductUpsert {
        category: category.into(),
        price_minor: price,
        ..up(name)
    }
}

async fn sell(f: &Fixture, id: &str, qty: i64) {
    submit(
        f,
        owner(),
        IntentPayload::PosSale(PosSale {
            kiosk: None,
            business: None,
            coupon: None,
            discount: None,
            lines: vec![PosSaleLine {
                product_id: id.into(),
                qty,
            }],
            payments: vec![pay(Cash, 100 * qty, None)],
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt: None,
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_device_order_on_a_branch_without_kiosk_orders_is_refused_before_the_permission_check() {
    let f = bootstrapped().await;
    // The device is unknown, so the permission step alone would say PERMISSION_DENIED.
    let e = submit(&f, device(), place()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::FeatureNotInTier);
    sql(&f, licence("uploads,kitchen")).await;
    let e = submit(&f, device(), place()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::FeatureNotInTier);

    // With the flag the gate steps aside (the permission step then speaks).
    sql(&f, licence(&format!("kitchen,{FLAG_KIOSK_ORDERS}"))).await;
    let e = submit(&f, device(), place()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
}

#[tokio::test]
async fn counter_staff_are_not_held_to_the_kiosk_orders_gate() {
    let f = bootstrapped().await;
    let e = submit(&f, owner(), place()).await.unwrap_err();
    assert_ne!(e.code, ErrorCode::FeatureNotInTier);
}

#[tokio::test]
async fn a_free_branch_gets_no_menu_and_no_photos() {
    let f = bootstrapped().await;
    let mut photo = item("Cola", "Drinks", 4_000);
    photo.image = jpeg();
    let id = make(&f, photo).await;
    let dir = DeviceDirectory::open(&f.path).unwrap();
    let menu = dir.shop_menu(f.clock.now_ms(), "PC1").unwrap();
    assert!(!menu.enabled);
    assert!(menu.items.is_empty());
    assert_eq!(dir.shop_image(&id).unwrap().image, None);
}

#[tokio::test]
async fn the_menu_lists_active_products_and_bundles_with_live_availability() {
    let f = bootstrapped().await;
    sql(&f, licence(FLAG_KIOSK_ORDERS)).await;
    let mut cola = item("Cola", "Drinks", 4_000);
    cola.image = jpeg();
    let cola = make(&f, cola).await;
    stock_in(&f, &cola, 5, 2_000).await;
    let chips = make(&f, item("Chips", "Snacks", 2_000)).await;
    let hidden = make(
        &f,
        ProductUpsert {
            active: false,
            ..item("Retired", "Snacks", 100)
        },
    )
    .await;
    let _combo = make(
        &f,
        ProductUpsert {
            bundle: true,
            track_stock: false,
            components: vec![
                BundleComponent {
                    product_id: cola.clone(),
                    qty: 2,
                },
                BundleComponent {
                    product_id: chips.clone(),
                    qty: 1,
                },
            ],
            ..item("Combo", "Combos", 9_000)
        },
    )
    .await;
    let dir = DeviceDirectory::open(&f.path).unwrap();
    let now = f.clock.now_ms();

    let menu = dir.shop_menu(now, "PC1").unwrap();
    assert!(menu.enabled);
    let names: Vec<_> = menu.items.iter().map(|i| i.name.as_str()).collect();
    // By category then name; the inactive product is not listed.
    assert_eq!(names, ["Combo", "Cola", "Chips"]);
    let by = |name: &str| menu.items.iter().find(|i| i.name == name).unwrap();
    assert_eq!(by("Cola").price_minor, 4_000);
    assert!(by("Cola").available && by("Cola").has_image);
    // Chips are tracked with no stock: out of stock, and so is the bundle that needs them.
    assert!(!by("Chips").available && !by("Chips").has_image);
    let combo_item = by("Combo");
    assert!(combo_item.bundle && !combo_item.available);
    let parts: Vec<_> = combo_item
        .components
        .iter()
        .map(|c| (c.name.as_str(), c.qty))
        .collect();
    assert_eq!(parts, [("Cola", 2), ("Chips", 1)]);

    // A Hub stock change shows on the next read.
    stock_in(&f, &chips, 3, 500).await;
    let menu = dir.shop_menu(now, "PC1").unwrap();
    assert!(menu.items.iter().all(|i| i.available));

    // The photo comes from the Hub, only for a listed product that has one.
    assert!(
        dir.shop_image(&cola)
            .unwrap()
            .image
            .unwrap()
            .starts_with("data:image/jpeg")
    );
    assert_eq!(dir.shop_image(&chips).unwrap().image, None);
    assert_eq!(dir.shop_image(&hidden).unwrap().image, None);
    assert_eq!(dir.shop_image("nope").unwrap().image, None);
}

#[tokio::test]
async fn the_three_top_sellers_of_the_week_are_popular_and_void_orders_do_not_count() {
    let f = bootstrapped().await;
    sql(&f, licence(FLAG_KIOSK_ORDERS)).await;
    let mut ids = Vec::new();
    for name in ["A", "B", "C", "D", "E"] {
        let id = make(&f, item(name, "Food", 100)).await;
        stock_in(&f, &id, 100, 10).await;
        ids.push(id);
    }
    let [a, b, c, d, e] = &ids[..] else { panic!() };
    for (id, qty) in [(a, 5), (b, 4), (c, 3), (d, 2)] {
        sell(&f, id, qty).await;
    }
    // E sold the most, but on an order that was voided.
    sell(&f, e, 9).await;
    sql(
        &f,
        format!(
            "UPDATE orders SET status = 'void' WHERE id IN (
                 SELECT order_id FROM order_lines WHERE product_id = '{e}')"
        ),
    )
    .await;
    let dir = DeviceDirectory::open(&f.path).unwrap();
    let now = f.clock.now_ms();
    let popular = |now: i64| -> Vec<String> {
        dir.shop_menu(now, "PC1")
            .unwrap()
            .items
            .into_iter()
            .filter(|i| i.popular)
            .map(|i| i.name)
            .collect()
    };
    assert_eq!(popular(now), ["A", "B", "C"]);
    // Eight days on, nothing sold in the last 7 days.
    assert!(popular(now + 8 * 24 * 3_600_000).is_empty());
}

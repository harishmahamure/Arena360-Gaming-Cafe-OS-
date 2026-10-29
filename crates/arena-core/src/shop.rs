//! The kiosk Shop's reads (Story 6.30): the menu with live availability and the popular items,
//! and one product's photo.
//!
//! Both are gated on the `kiosk_orders` licence flag: a branch without it gets
//! `enabled: false`, no items and no photos. Availability is [`crate::catalog`]'s (stock, expiry,
//! a bundle's components), so the Shop and the counter agree. Nothing here writes.

use arena_proto::{
    AppError, KEY_LICENCE_FLAGS, POPULAR_DAYS, POPULAR_LIMIT, ShopComponent, ShopImageResult,
    ShopItem, ShopMenu, has_flag,
};
use arena_store::Store;

const DAY_MS: i64 = 24 * 3_600_000;

/// Whether the branch's licence lets kiosks show the Shop.
pub fn enabled(store: &Store) -> Result<bool, AppError> {
    Ok(store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_LICENCE_FLAGS)
        .is_some_and(|s| has_flag(&s.value, arena_proto::FLAG_KIOSK_ORDERS)))
}

/// The menu as of `now_ms`: every active, non-deleted product and bundle by category then name,
/// with whether it can be sold now, whether it has a photo, and a `popular` mark on the top
/// sellers of the last 7 days (void orders do not count).
pub fn menu(store: &Store, now_ms: i64) -> Result<ShopMenu, AppError> {
    if !enabled(store)? {
        return Ok(ShopMenu::default());
    }
    let today = crate::locale::current_business_day(store, now_ms)?;
    let popular: Vec<String> = store
        .units_sold(now_ms - POPULAR_DAYS * DAY_MS, POPULAR_LIMIT)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let items = store
        .menu_products()?
        .into_iter()
        .map(|(p, has_image)| {
            let status = crate::catalog::status(store, &p, today.as_deref())?;
            Ok(ShopItem {
                popular: popular.contains(&p.product_id),
                id: p.product_id,
                name: p.name,
                category: p.category,
                price_minor: p.price_minor,
                bundle: p.bundle,
                available: status.available,
                has_image,
                components: status
                    .components
                    .into_iter()
                    .map(|c| ShopComponent {
                        name: c.name,
                        qty: c.qty,
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(ShopMenu {
        enabled: true,
        items,
    })
}

/// The photo of product `id` (a JPEG data URL), or `None` when the branch has no Shop, the
/// product is not on the menu (inactive, deleted or unknown) or it has no photo.
pub fn image(store: &Store, id: &str) -> Result<ShopImageResult, AppError> {
    let image = if enabled(store)? {
        store
            .product(id)?
            .filter(|p| p.active && p.deleted_at.is_none() && !p.image.is_empty())
            .map(|p| p.image)
    } else {
        None
    };
    Ok(ShopImageResult {
        id: id.to_owned(),
        image,
    })
}

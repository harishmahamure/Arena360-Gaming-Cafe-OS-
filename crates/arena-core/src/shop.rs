//! The kiosk Shop's reads (Story 6.30): the menu with live availability and the popular items,
//! and one product's photo.
//!
//! Both are gated on the `kiosk_orders` licence flag: a branch without it gets
//! `enabled: false`, no items and no photos. Availability is [`crate::catalog`]'s (stock, expiry,
//! a bundle's components), so the Shop and the counter agree. Nothing here writes.

use arena_proto::{
    AppError, ErrorCode, KEY_LICENCE_FLAGS, POPULAR_DAYS, POPULAR_LIMIT, SessionMode,
    ShopComponent, ShopImageResult, ShopItem, ShopMenu, ShopPayOptions, has_flag,
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
pub fn menu(store: &Store, now_ms: i64, device_id: &str) -> Result<ShopMenu, AppError> {
    if !enabled(store)? {
        return Ok(ShopMenu::default());
    }
    let today = crate::locale::current_business_day(store, now_ms)?;
    let popular: Vec<String> = store
        .units_sold(now_ms - POPULAR_DAYS * DAY_MS, POPULAR_LIMIT)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let kitchen_on = store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_LICENCE_FLAGS)
        .is_some_and(|s| has_flag(&s.value, arena_proto::FLAG_KITCHEN));
    let items = store
        .menu_products()?
        .into_iter()
        .map(|(p, has_image)| {
            let status = crate::catalog::status(store, &p, today.as_deref())?;
            Ok(ShopItem {
                kitchen_notes: if kitchen_on && p.kitchen {
                    p.kitchen_notes.clone()
                } else {
                    Vec::new()
                },
                popular: popular.contains(&p.product_id),
                id: p.product_id,
                name: p.name,
                category: p.category,
                price_minor: p.price_minor,
                bundle: p.bundle,
                available: status.available,
                max_qty: if status.available {
                    status.sellable
                } else {
                    Some(0)
                },
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
        pay: pay_options(store, device_id)?,
    })
}

/// How the player on `device_id` can pay for a cart (Story 6.32). A running postpaid or block
/// session offers its bill, and a profiled player's session also the wallet (the balance less
/// what running wallet sessions set aside). Pay at counter is always offered; it is the only
/// choice once the session has ended.
fn pay_options(store: &Store, device_id: &str) -> Result<ShopPayOptions, AppError> {
    let live = store.live_sessions()?.into_iter().find(|s| {
        s.device_id == device_id && matches!(s.state.as_str(), "active" | "paused" | "grace")
    });
    let Some(s) = live else {
        return Ok(ShopPayOptions {
            bill: false,
            wallet_minor: None,
            counter: true,
        });
    };
    let bill = s.mode == SessionMode::Block.as_str()
        || (s.mode == SessionMode::Postpaid.as_str() && s.group_bill_id.is_none());
    let wallet_minor = match &s.player_id {
        Some(id) => Some((store.wallet_balance(id)? - store.reserved_minor(id)?).max(0)),
        None => None,
    };
    Ok(ShopPayOptions {
        bill,
        wallet_minor,
        counter: true,
    })
}

/// Who a cart from `device_id` is for (Story 6.32), as the Hub reads it when the player confirms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PayTarget {
    /// The player of the session running on the PC (`None` for a guest), when one is running.
    pub running: Option<Option<String>>,
    /// The wallet balance that player may spend now.
    pub wallet_minor: Option<i64>,
    /// The player of the latest session on the PC, running or ended.
    pub last_player: Option<String>,
    /// What the PC owes the counter for unpaid orders (minor units).
    pub unpaid_minor: i64,
}

/// Reads [`PayTarget`] for `device_id`.
pub fn pay_target(store: &Store, device_id: &str) -> Result<PayTarget, AppError> {
    let running = store.live_sessions()?.into_iter().find(|s| {
        s.device_id == device_id && matches!(s.state.as_str(), "active" | "paused" | "grace")
    });
    let wallet_minor = match running.as_ref().and_then(|s| s.player_id.as_deref()) {
        Some(id) => Some((store.wallet_balance(id)? - store.reserved_minor(id)?).max(0)),
        None => None,
    };
    Ok(PayTarget {
        running: running.map(|s| s.player_id),
        wallet_minor,
        last_player: store.last_session_player(device_id)?,
        unpaid_minor: store.unpaid_counter_minor(device_id)?,
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

/// The id of the session an order from the kiosk on `device_id` goes on (Story 6.31): the live
/// session on that PC. Whether it can take an order (a running postpaid or block session) is
/// `apply()`'s rule, so the counter and the kiosk agree.
pub fn order_target(store: &Store, device_id: &str) -> Result<String, AppError> {
    store
        .live_sessions()?
        .into_iter()
        .find(|s| s.device_id == device_id)
        .map(|s| s.id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no session on this PC"))
}

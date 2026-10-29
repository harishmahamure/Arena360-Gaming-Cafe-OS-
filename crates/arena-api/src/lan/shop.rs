//! The kiosk's Shop reads (Story 6.30): `shop.menu` and `shop.image`.
//!
//! Neither names a player or a session and neither writes. A follower answers them from its
//! replicated database as the primary does. The menu carries no photos (a photo can be 200 KB);
//! the kiosk asks for each one with `shop.image`. A branch without the `kiosk_orders` licence flag
//! gets `enabled: false`, an empty menu and no photos.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, ShopImageResult, ShopMenu};

use super::LanHub;

fn failed(what: &str, e: tokio::task::JoinError) -> AppError {
    AppError::new(ErrorCode::Internal, format!("{what} read failed: {e}"))
}

/// `shop.menu`.
pub(super) async fn menu(hub: &LanHub) -> Result<ShopMenu, AppError> {
    let dir = Arc::clone(&hub.devices);
    let now = hub.clock.now_ms();
    tokio::task::spawn_blocking(move || dir.shop_menu(now))
        .await
        .map_err(|e| failed("shop menu", e))?
}

/// `shop.image` for product `id`.
pub(super) async fn image(hub: &LanHub, id: &str) -> Result<ShopImageResult, AppError> {
    let dir = Arc::clone(&hub.devices);
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || dir.shop_image(&id))
        .await
        .map_err(|e| failed("shop image", e))?
}

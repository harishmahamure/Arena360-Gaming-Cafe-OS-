//! The kiosk's Shop frames (Stories 6.30 and 6.31): `shop.menu`, `shop.image` and `shop.place`.
//!
//! The two reads name no player or session and write nothing. A follower answers them from its
//! replicated database as the primary does. The menu carries no photos (a photo can be 200 KB);
//! the kiosk asks for each one with `shop.image`. A branch without the `kiosk_orders` licence flag
//! gets `enabled: false`, an empty menu and no photos. `shop.place` is the one write: the player's
//! cart goes on the bill of the session running on the paired PC.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AlertKind, AlertRaise, AppError, CounterOrder, ErrorCode, IntentKind,
    IntentPayload, KioskSale, OrderPlace, PosSale, PosSaleLine, ShopImageResult, ShopMenu, ShopPay,
    ShopPlace, ShopPlaceResult,
};

use super::LanHub;
use super::player_login::intent;
use super::session::LAN_SYSTEM_ACTOR;

fn failed(what: &str, e: tokio::task::JoinError) -> AppError {
    AppError::new(ErrorCode::Internal, format!("{what} read failed: {e}"))
}

/// `shop.menu`.
pub(super) async fn menu(hub: &LanHub, device: &str) -> Result<ShopMenu, AppError> {
    let dir = Arc::clone(&hub.devices);
    let now = hub.clock.now_ms();
    let device = device.to_owned();
    tokio::task::spawn_blocking(move || dir.shop_menu(now, &device))
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

fn not_primary() -> AppError {
    AppError::new(ErrorCode::NotPrimary, "this hub is not the primary writer")
}

/// The product a refusal's `lines[i]` field names, if it names one of the cart's lines.
fn refused_product(field: Option<&str>, lines: &[PosSaleLine]) -> Option<String> {
    // `lines[i]` or a field of it (`lines[i].kitchen_notes`).
    let at: usize = field?
        .strip_prefix("lines[")?
        .split_once(']')?
        .0
        .parse()
        .ok()?;
    lines.get(at).map(|l| l.product_id.clone())
}

/// `shop.place` (Stories 6.31 and 6.32): the player's cart, paid the way they chose.
///
/// The Hub builds the intent itself (device actor, never a stock override), so the Epic 5 rules
/// apply as they do at the counter and a kiosk without the `kiosk_orders` flag is refused
/// `FEATURE_NOT_IN_TIER`:
/// - `bill`: an `order.place` on the session running on `device`;
/// - `wallet`: a `pos.sale` settled by one wallet payment of the whole total, for the profile
///   running on `device`, tied to the PC for delivery;
/// - `counter`: an `order.place` with `counter`, an unpaid order tied to the PC and the player.
///
/// On success the Hub raises the PC's `kiosk_order` alert (the new-order sound, the badge and the
/// tile's ORDER tag); one already open is as good as a new one. A refusal comes back typed, naming
/// the short product, and the cart stays on the kiosk.
pub(super) async fn place(
    hub: &LanHub,
    device: &str,
    request: ShopPlace,
) -> Result<ShopPlaceResult, AppError> {
    if !hub.is_primary() {
        return Err(not_primary());
    }
    let writer = hub.writer.clone().ok_or_else(not_primary)?;
    let target = {
        let (dir, device) = (Arc::clone(&hub.devices), device.to_owned());
        tokio::task::spawn_blocking(move || dir.pay_target(&device))
            .await
            .map_err(|e| failed("session", e))??
    };
    let (kind, payload) = match request.pay {
        ShopPay::Bill => {
            let session_id = {
                let (dir, device) = (Arc::clone(&hub.devices), device.to_owned());
                tokio::task::spawn_blocking(move || dir.order_target(&device))
                    .await
                    .map_err(|e| failed("session", e))?
            };
            match session_id {
                Ok(session_id) => (
                    IntentKind::OrderPlace,
                    IntentPayload::OrderPlace(OrderPlace {
                        counter: None,
                        session_id,
                        lines: request.lines.clone(),
                        stock_override: None,
                    }),
                ),
                Err(e) if e.code == ErrorCode::NotPrimary => return Err(e),
                Err(e) => return Ok(refusal(e, None, None)),
            }
        }
        ShopPay::Wallet => {
            let Some(Some(player_id)) = target.running.clone() else {
                return Ok(refusal(
                    AppError::new(
                        ErrorCode::ValidationFailed,
                        "Pay now needs a signed-in player on a running session.",
                    ),
                    None,
                    None,
                ));
            };
            (
                IntentKind::PosSale,
                IntentPayload::PosSale(PosSale {
                    kiosk: Some(KioskSale {
                        device_id: device.to_owned(),
                    }),
                    lines: request.lines.clone(),
                    payments: Vec::new(),
                    player_id: Some(player_id),
                    credit_override: None,
                    stock_override: None,
                    receipt: None,
                    business: None,
                    coupon: None,
                    discount: None,
                }),
            )
        }
        ShopPay::Counter => (
            IntentKind::OrderPlace,
            IntentPayload::OrderPlace(OrderPlace {
                counter: Some(CounterOrder {
                    device_id: device.to_owned(),
                    player_id: target.last_player.clone(),
                }),
                session_id: String::new(),
                lines: request.lines.clone(),
                stock_override: None,
            }),
        ),
    };
    let order = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::Device,
            id: device.to_owned(),
        },
        kind,
        payload,
    );
    match writer.submit_intent(order).await {
        Ok(_) => {
            raise_order_alert(hub, &writer, device).await;
            tracing::info!(device_id = %device, lines = request.lines.len(), pay = ?request.pay, "a player ordered from the kiosk");
            Ok(ShopPlaceResult::Ok)
        }
        Err(e) if e.code == ErrorCode::NotPrimary => Err(e),
        Err(e) => {
            tracing::info!(device_id = %device, code = e.code.as_str(), "kiosk order refused");
            let product = refused_product(e.field.as_deref(), &request.lines);
            let balance = (e.code == ErrorCode::InsufficientBalance)
                .then_some(target.wallet_minor)
                .flatten();
            Ok(refusal(e, product, balance))
        }
    }
}

fn refusal(e: AppError, product_id: Option<String>, balance_minor: Option<i64>) -> ShopPlaceResult {
    ShopPlaceResult::Refused {
        code: e.code,
        message: e.message,
        product_id,
        balance_minor,
    }
}

/// Raises the PC's `kiosk_order` alert after an order committed. The order is already on the bill,
/// so a failure here is logged and the player is still told it went through.
async fn raise_order_alert(hub: &LanHub, writer: &arena_core::WriterHandle, device: &str) {
    let raise = intent(
        hub.clock.now_ms(),
        Actor {
            kind: ActorKind::System,
            id: LAN_SYSTEM_ACTOR.to_owned(),
        },
        IntentKind::AlertRaise,
        IntentPayload::AlertRaise(AlertRaise::new(
            AlertKind::KioskOrder,
            Some(device.to_owned()),
        )),
    );
    match writer.submit_intent(raise).await {
        Ok(_) => {}
        Err(e) if e.code == ErrorCode::Stale => {}
        Err(e) => {
            tracing::warn!(device_id = %device, code = e.code.as_str(), "the kiosk order alert was not raised");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_cart_line_it_is_about() {
        let lines = vec![
            PosSaleLine {
                product_id: "A".into(),
                qty: 1,
                kitchen_notes: Vec::new(),
            },
            PosSaleLine {
                product_id: "B".into(),
                qty: 2,
                kitchen_notes: Vec::new(),
            },
        ];
        assert_eq!(
            refused_product(Some("lines[1]"), &lines).as_deref(),
            Some("B")
        );
        for bad in [
            None,
            Some("lines"),
            Some("lines[x]"),
            Some("lines[2]"),
            Some("session_id"),
        ] {
            assert_eq!(refused_product(bad, &lines), None, "{bad:?}");
        }
    }
}

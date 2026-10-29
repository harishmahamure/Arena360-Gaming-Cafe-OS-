//! The kiosk Shop's reads (Story 6.30): `shop.menu` and `shop.image`.
//!
//! The Hub decides everything the Shop shows: whether orders are enabled on this branch (the
//! `kiosk_orders` licence flag), which products and bundles are listed, which are available right
//! now and which are the popular ones. The kiosk only renders. Both reads change no state.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};
use crate::intent::IntentPayload;
use crate::pos::PosSaleLine;

/// The licence flag that lets a kiosk show the Shop and place orders (Basic and above).
pub const FLAG_KIOSK_ORDERS: &str = "kiosk_orders";

/// How many products carry the popular badge.
pub const POPULAR_LIMIT: usize = 3;

/// The popular window: units sold over the last 7 days.
pub const POPULAR_DAYS: i64 = 7;

/// The longest product id a `shop.image` may name.
pub const MAX_SHOP_ID_LEN: usize = 64;

/// Whether `p`, sent by a kiosk device, needs the `kiosk_orders` licence flag. An order-capable
/// intent from a device on a branch without the flag is refused `FEATURE_NOT_IN_TIER` before the
/// permission check.
pub fn kiosk_order_gate(p: &IntentPayload) -> bool {
    matches!(p, IntentPayload::OrderPlace(_))
}

/// One part of a bundle ("2 x Cola").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopComponent {
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
}

/// One card of the Shop: an active product or bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopItem {
    pub id: String,
    pub name: String,
    pub category: String,
    /// Integer minor units.
    #[ts(type = "number")]
    pub price_minor: i64,
    pub bundle: bool,
    /// Whether the Hub can sell it right now (stock, expiry, a bundle's components).
    pub available: bool,
    /// How many units the Hub can sell right now (stock less expired lots and open holds; a
    /// bundle's tightest component), or `null` when the item is not tracked. `0` when it is
    /// unavailable. The cart refuses a step past it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub max_qty: Option<i64>,
    /// Whether `shop.image` has a photo for it (the menu itself carries no photos).
    pub has_image: bool,
    /// One of the top [`POPULAR_LIMIT`] sellers of the last [`POPULAR_DAYS`] days.
    pub popular: bool,
    /// A bundle's parts; empty for a product.
    pub components: Vec<ShopComponent>,
}

/// `shop.menu_result`: whether orders are enabled here and, if so, the menu.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopMenu {
    /// The branch's licence has `kiosk_orders`. When `false` there are no items and the kiosk
    /// shows no Shop tab.
    pub enabled: bool,
    /// Active, non-deleted products and bundles by category then name.
    pub items: Vec<ShopItem>,
}

/// `shop.image` (kiosk -> Hub): the photo of one product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopImageRequest {
    pub id: String,
}

impl ShopImageRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        if (1..=MAX_SHOP_ID_LEN).contains(&self.id.len()) {
            Ok(())
        } else {
            Err(AppError::new(ErrorCode::BadFrame, "malformed shop.image"))
        }
    }
}

/// `shop.image_result`: the product's photo as the data URL the Hub stores (a JPEG of at most
/// 200 KB), or `null` when it has none or is not on the menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopImageResult {
    pub id: String,
    pub image: Option<String>,
}

/// The most lines one kiosk order may hold.
pub const MAX_CART_LINES: usize = 50;

/// The most of one product one cart line may hold.
pub const MAX_CART_QTY: i64 = 99;

/// `shop.place` (kiosk -> Hub, Story 6.31): the player's cart. It names no session and no player:
/// the Hub finds the session running on the paired PC and builds the `order.place` itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShopPlace {
    pub lines: Vec<PosSaleLine>,
}

impl ShopPlace {
    /// A cart of 1 to [`MAX_CART_LINES`] lines, each a sane product id once with 1 to
    /// [`MAX_CART_QTY`] units.
    pub fn validate(&self) -> Result<(), AppError> {
        let bad = |m: &str| Err(AppError::new(ErrorCode::BadFrame, m));
        if self.lines.is_empty() || self.lines.len() > MAX_CART_LINES {
            return bad("malformed shop.place");
        }
        let mut seen = std::collections::BTreeSet::new();
        for l in &self.lines {
            if !(1..=MAX_SHOP_ID_LEN).contains(&l.product_id.len())
                || !(1..=MAX_CART_QTY).contains(&l.qty)
                || !seen.insert(l.product_id.as_str())
            {
                return bad("malformed shop.place");
            }
        }
        Ok(())
    }
}

/// `shop.place_result`: the order joined the session's bill, or the Hub refused it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ShopPlaceResult {
    /// The lines are on the session's bill.
    Ok,
    /// `apply()` refused (`OUT_OF_STOCK`, `BUNDLE_SHORT`, `FEATURE_NOT_IN_TIER`, a session that
    /// cannot take an order, ...). `product_id` names the short line when the refusal is about one.
    Refused {
        code: ErrorCode,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        product_id: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameBody, parse_frame};

    #[test]
    fn the_shop_frames_round_trip() {
        let ask = parse_frame(r#"{"v":1,"kind":"shop.menu","rid":"m"}"#).unwrap();
        assert_eq!(ask.body, FrameBody::ShopMenu);
        let menu = ShopMenu {
            enabled: true,
            items: vec![ShopItem {
                id: "P1".into(),
                name: "Combo".into(),
                category: "Snacks".into(),
                price_minor: 12_000,
                bundle: true,
                available: false,
                max_qty: Some(0),
                has_image: false,
                popular: true,
                components: vec![ShopComponent {
                    name: "Cola".into(),
                    qty: 2,
                }],
            }],
        };
        let reply = Frame::new("m", FrameBody::ShopMenuResult(menu));
        let v: serde_json::Value = serde_json::from_str(&reply.to_text()).unwrap();
        assert_eq!(v["kind"], "shop.menu_result");
        assert_eq!(v["enabled"], true);
        assert_eq!(v["items"][0]["components"][0]["qty"], 2);
        assert_eq!(parse_frame(&reply.to_text()).unwrap(), reply);

        let image = parse_frame(r#"{"v":1,"kind":"shop.image","rid":"i","id":"P1"}"#).unwrap();
        assert_eq!(
            image.body,
            FrameBody::ShopImage(ShopImageRequest { id: "P1".into() })
        );
        let reply = Frame::new(
            "i",
            FrameBody::ShopImageResult(ShopImageResult {
                id: "P1".into(),
                image: None,
            }),
        );
        assert_eq!(parse_frame(&reply.to_text()).unwrap(), reply);
    }

    #[test]
    fn the_place_frames_round_trip_and_name_no_session() {
        let ask = parse_frame(
            r#"{"v":1,"kind":"shop.place","rid":"p","lines":[{"product_id":"P1","qty":2}]}"#,
        )
        .unwrap();
        assert_eq!(
            ask.body,
            FrameBody::ShopPlace(ShopPlace {
                lines: vec![PosSaleLine {
                    product_id: "P1".into(),
                    qty: 2
                }]
            })
        );
        for body in [
            ShopPlaceResult::Ok,
            ShopPlaceResult::Refused {
                code: ErrorCode::OutOfStock,
                message: "Only 1 Cola left.".into(),
                product_id: Some("P1".into()),
            },
        ] {
            let reply = Frame::new("p", FrameBody::ShopPlaceResult(body));
            let v: serde_json::Value = serde_json::from_str(&reply.to_text()).unwrap();
            assert_eq!(v["kind"], "shop.place_result");
            assert_eq!(parse_frame(&reply.to_text()).unwrap(), reply);
        }
    }

    #[test]
    fn a_cart_is_one_to_fifty_lines_of_one_to_ninety_nine_each_product_once() {
        let line = |id: &str, qty| PosSaleLine {
            product_id: id.into(),
            qty,
        };
        let cart = |lines| ShopPlace { lines };
        assert!(
            cart(vec![line("P1", 1), line("P2", MAX_CART_QTY)])
                .validate()
                .is_ok()
        );
        let many: Vec<_> = (0..=MAX_CART_LINES)
            .map(|i| line(&format!("P{i}"), 1))
            .collect();
        for bad in [
            cart(vec![]),
            cart(vec![line("P1", 0)]),
            cart(vec![line("P1", MAX_CART_QTY + 1)]),
            cart(vec![line("", 1)]),
            cart(vec![line(&"x".repeat(MAX_SHOP_ID_LEN + 1), 1)]),
            cart(vec![line("P1", 1), line("P1", 2)]),
            cart(many),
        ] {
            assert_eq!(bad.validate().unwrap_err().code, ErrorCode::BadFrame);
        }
    }

    #[test]
    fn an_image_request_names_a_product_of_sane_length() {
        assert!(ShopImageRequest { id: "P1".into() }.validate().is_ok());
        for id in ["".to_owned(), "x".repeat(MAX_SHOP_ID_LEN + 1)] {
            assert_eq!(
                ShopImageRequest { id }.validate().unwrap_err().code,
                ErrorCode::BadFrame
            );
        }
    }

    #[test]
    fn only_an_order_is_gated_on_the_kiosk_orders_flag() {
        use crate::{OrderPlace, PosSaleLine};
        let order = IntentPayload::OrderPlace(OrderPlace {
            session_id: "S".into(),
            lines: Vec::<PosSaleLine>::new(),
            stock_override: None,
        });
        assert!(kiosk_order_gate(&order));
        assert!(!kiosk_order_gate(&IntentPayload::SettingsSet(
            crate::SettingsSet {
                key: "k".into(),
                value: "v".into()
            }
        )));
    }
}

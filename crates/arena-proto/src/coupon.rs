use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The shortest and longest coupon code (Story 5.21).
pub const COUPON_CODE_MIN_CHARS: usize = 3;
pub const COUPON_CODE_MAX_CHARS: usize = 24;
/// A percent coupon takes 1 to 10 000 basis points (0.01% to 100%).
pub const COUPON_MAX_PERCENT_BP: i64 = 10_000;
/// The most targets one coupon lists.
pub const COUPON_MAX_TARGETS: usize = 100;
/// The target kinds: a device group, a plan or a product.
pub const COUPON_TARGET_KINDS: [&str; 3] = ["group", "plan", "product"];

/// The code as the Hub stores and compares it: trimmed and upper-cased. Whether it is a valid
/// code is [`coupon_code_valid`].
pub fn coupon_code_normal(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

/// Whether `code` is 3 to 24 characters of `A-Z 0-9 - _` (already upper-cased).
pub fn coupon_code_valid(code: &str) -> bool {
    (COUPON_CODE_MIN_CHARS..=COUPON_CODE_MAX_CHARS).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn yes() -> bool {
    true
}

/// What a coupon is limited to: a device group (a time line on a device of that group), a plan or
/// a product (a bundle is a product).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CouponTarget {
    /// `group`, `plan` or `product`.
    pub kind: String,
    pub id: String,
}

/// The payload of `coupon.upsert`: a coupon is a percent (`percent_bp`) or a fixed amount
/// (`amount_minor`), never both, with an optional local-date validity window (inclusive), an
/// optional usage limit, a minimum bill (0 = none) and optional targets. A coupon with an `id` is
/// an edit; a deactivated coupon stays listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CouponUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub percent_bp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub amount_minor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub usage_limit: Option<i64>,
    #[serde(default)]
    #[ts(type = "number")]
    pub min_bill_minor: i64,
    #[serde(default)]
    pub targets: Vec<CouponTarget>,
    #[serde(default = "yes")]
    pub active: bool,
}

/// One coupon as `GET /ui/coupons` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CouponView {
    pub id: String,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub percent_bp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub amount_minor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub usage_limit: Option<i64>,
    #[ts(type = "number")]
    pub used: i64,
    #[ts(type = "number")]
    pub min_bill_minor: i64,
    pub targets: Vec<CouponTarget>,
    pub active: bool,
    #[ts(type = "number")]
    pub rev: i64,
}

/// The body of `POST /ui/coupons/preview`: the code and the bill it is for, exactly one of an open
/// bill (`order_id`), a counter cart (`lines`) or a plan sale (`plan_id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CouponPreviewRequest {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub order_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lines: Option<Vec<crate::PosSaleLine>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_id: Option<String>,
}

/// What a coupon does to a bill: the discount and the total to pay after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CouponPreview {
    /// The code as stored (upper-case).
    pub code: String,
    #[ts(type = "number")]
    pub discount_minor: i64,
    /// The bill total after the coupon.
    #[ts(type = "number")]
    pub total_minor: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_upper_case_and_plain() {
        assert_eq!(coupon_code_normal("  diwali-10 "), "DIWALI-10");
        for ok in ["ABC", "A_B-9", &"X".repeat(24)] {
            assert!(coupon_code_valid(ok), "{ok}");
        }
        for bad in ["", "AB", "a b", "ABC!", "abc", &"X".repeat(25), "ÀBC"] {
            assert!(!coupon_code_valid(bad), "{bad}");
        }
    }

    #[test]
    fn upsert_defaults_to_active_with_no_minimum() {
        let u: CouponUpsert =
            serde_json::from_value(serde_json::json!({"code": "SAVE10", "percent_bp": 1000}))
                .unwrap();
        assert!(u.active);
        assert_eq!(
            (u.min_bill_minor, u.targets.len(), u.usage_limit),
            (0, 0, None)
        );
    }
}

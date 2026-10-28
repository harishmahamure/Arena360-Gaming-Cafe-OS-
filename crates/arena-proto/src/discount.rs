use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest free text of an `other` reason (Story 5.22).
pub const DISCOUNT_NOTE_MAX_CHARS: usize = 200;
/// A percent discount is 1 to 10 000 basis points (0.01% to 100%).
pub const DISCOUNT_MAX_PERCENT_BP: i64 = 10_000;

/// Why a manual discount was given: the four reasons the Hub offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum DiscountReason {
    RegularCustomer,
    ServiceIssue,
    StaffMeal,
    /// Needs the free text in `note`.
    Other,
}

impl DiscountReason {
    /// Every reason, in the order the Hub offers them.
    pub const ALL: [DiscountReason; 4] = [
        Self::RegularCustomer,
        Self::ServiceIssue,
        Self::StaffMeal,
        Self::Other,
    ];

    /// The code stored on the order.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RegularCustomer => "regular_customer",
            Self::ServiceIssue => "service_issue",
            Self::StaffMeal => "staff_meal",
            Self::Other => "other",
        }
    }
}

/// A discount staff give by hand (Story 5.22): a percent (`percent_bp`) or a fixed amount
/// (`amount_minor`), never both, with the reason they picked. It rides on `session.settle`,
/// `pos.sale` and `plan.sell`, is applied after a coupon, and needs `discount.manual` on the actor
/// or on the manager whose PIN approved the intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ManualDiscount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub percent_bp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub amount_minor: Option<i64>,
    pub reason: DiscountReason,
    /// The text of an `other` reason (required then, at most 200 characters); ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

/// The body of `POST /ui/discount/preview`: the discount, the coupon already applied to the bill
/// (if any) and the bill, exactly one of an open bill (`order_id`), a counter cart (`lines`) or a
/// plan sale (`plan_id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DiscountPreviewRequest {
    pub discount: ManualDiscount,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coupon: Option<String>,
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

/// What a manual discount does to a bill: the amount taken off and the total to pay after the
/// coupon and the discount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DiscountPreview {
    #[ts(type = "number")]
    pub discount_minor: i64,
    /// The bill total after the coupon (when one was named) and the discount.
    #[ts(type = "number")]
    pub total_minor: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_discount_reads_with_snake_case_reasons() {
        let d: ManualDiscount = serde_json::from_value(serde_json::json!({
            "percent_bp": 500,
            "reason": "staff_meal"
        }))
        .unwrap();
        assert_eq!(d.reason, DiscountReason::StaffMeal);
        assert_eq!(
            (d.percent_bp, d.amount_minor, d.note),
            (Some(500), None, None)
        );
        for r in DiscountReason::ALL {
            assert_eq!(serde_json::to_value(r).unwrap(), r.as_str());
        }
    }
}

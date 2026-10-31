//! The price book's vocabulary (Story 10.1): the PPP bands, the billable items and the two
//! billing cycles, and the answer of `GET /v1/pricing`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A purchasing-power band. It fixes the currency a branch is billed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum Band {
    In,
    My,
    Sg,
    Us,
}

impl Band {
    pub const ALL: [Band; 4] = [Self::In, Self::My, Self::Sg, Self::Us];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "IN",
            Self::My => "MY",
            Self::Sg => "SG",
            Self::Us => "US",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.as_str() == s)
    }

    /// The ISO 4217 currency the band is billed in.
    pub fn currency(self) -> &'static str {
        match self {
            Self::In => "INR",
            Self::My => "MYR",
            Self::Sg => "SGD",
            Self::Us => "USD",
        }
    }

    /// GST is charged on band IN only.
    pub fn gst_applicable(self) -> bool {
        self == Self::In
    }
}

/// What a price line sells. Free is never billed, so it has no line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PriceItem {
    Basic,
    Premium,
    #[serde(rename = "addon_devices_20")]
    AddonDevices20,
    AddonTunnel,
    AddonOwnerLogin,
}

impl PriceItem {
    pub const ALL: [PriceItem; 5] = [
        Self::Basic,
        Self::Premium,
        Self::AddonDevices20,
        Self::AddonTunnel,
        Self::AddonOwnerLogin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Premium => "premium",
            Self::AddonDevices20 => "addon_devices_20",
            Self::AddonTunnel => "addon_tunnel",
            Self::AddonOwnerLogin => "addon_owner_login",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.as_str() == s)
    }
}

/// How often a subscription is charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum BillingCycle {
    Monthly,
    Yearly,
}

impl BillingCycle {
    pub const ALL: [BillingCycle; 2] = [Self::Monthly, Self::Yearly];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// One price of the live book. `amount_minor` is in minor units of the band's currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PriceLine {
    pub item: PriceItem,
    pub cycle: BillingCycle,
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub gst_applicable: bool,
}

/// `GET /v1/pricing?band=`: the live book's lines for one band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Pricing {
    pub band: Band,
    pub currency: String,
    pub price_book_version: u32,
    pub lines: Vec<PriceLine>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_the_band_fixes_the_currency() {
        for b in Band::ALL {
            assert_eq!(Band::parse(b.as_str()), Some(b));
            assert_eq!(
                serde_json::to_string(&b).unwrap(),
                format!("\"{}\"", b.as_str())
            );
        }
        for i in PriceItem::ALL {
            assert_eq!(PriceItem::parse(i.as_str()), Some(i));
            assert_eq!(
                serde_json::to_string(&i).unwrap(),
                format!("\"{}\"", i.as_str())
            );
        }
        for c in BillingCycle::ALL {
            assert_eq!(BillingCycle::parse(c.as_str()), Some(c));
        }
        assert_eq!(Band::parse("in"), None);
        assert_eq!(
            Band::ALL.map(|b| (b.currency(), b.gst_applicable())),
            [
                ("INR", true),
                ("MYR", false),
                ("SGD", false),
                ("USD", false)
            ]
        );
    }
}

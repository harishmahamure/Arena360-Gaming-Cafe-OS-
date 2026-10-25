use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum Tier {
    Free,
    Basic,
    Premium,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Self::Free, Self::Basic, Self::Premium];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Basic => "basic",
            Self::Premium => "premium",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn defaults(self) -> &'static TierDefaults {
        TIERS
            .iter()
            .find(|d| d.tier == self)
            .expect("every tier has a TIERS row")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum EntitlementSource {
    Free,
    Comp,
    Paid,
}

impl EntitlementSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Comp => "comp",
            Self::Paid => "paid",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "free" => Some(Self::Free),
            "comp" => Some(Self::Comp),
            "paid" => Some(Self::Paid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TierLimits {
    pub devices: u32,
    pub users: u32,
    pub branches: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierDefaults {
    pub tier: Tier,
    pub flags: &'static [&'static str],
    pub features: &'static [&'static str],
    pub upload_interval_s: u32,
    pub summary_interval_s: u32,
    pub rollup_schema: u32,
    pub retention_days: u32,
    pub limits: TierLimits,
}

pub const TIERS: &[TierDefaults] = &[
    TierDefaults {
        tier: Tier::Free,
        flags: &[],
        features: &[],
        upload_interval_s: 0,
        summary_interval_s: 0,
        rollup_schema: 1,
        retention_days: 0,
        limits: TierLimits {
            devices: 20,
            users: 3,
            branches: 1,
        },
    },
    TierDefaults {
        tier: Tier::Basic,
        flags: &["uploads", "owner_reports", "kitchen"],
        features: &[],
        upload_interval_s: 300,
        summary_interval_s: 900,
        rollup_schema: 1,
        retention_days: 365,
        limits: TierLimits {
            devices: 60,
            users: 10,
            branches: 1,
        },
    },
    TierDefaults {
        tier: Tier::Premium,
        flags: &[
            "uploads",
            "owner_reports",
            "kiosk_orders",
            "kitchen",
            "kitchen_display",
            "standby",
        ],
        features: &[],
        upload_interval_s: 300,
        summary_interval_s: 900,
        rollup_schema: 1,
        retention_days: 1095,
        limits: TierLimits {
            devices: 200,
            users: 50,
            branches: 1,
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tier_table_pins_every_value() {
        assert_eq!(
            TIERS.iter().map(|d| d.tier).collect::<Vec<_>>(),
            Tier::ALL.to_vec()
        );
        let free = Tier::Free.defaults();
        assert!(free.flags.is_empty() && free.features.is_empty());
        assert_eq!(
            (
                free.upload_interval_s,
                free.summary_interval_s,
                free.rollup_schema,
                free.retention_days
            ),
            (0, 0, 1, 0)
        );
        assert_eq!(
            free.limits,
            TierLimits {
                devices: 20,
                users: 3,
                branches: 1
            }
        );
        let basic = Tier::Basic.defaults();
        assert_eq!(basic.flags, ["uploads", "owner_reports", "kitchen"]);
        assert!(basic.features.is_empty());
        assert_eq!(
            (
                basic.upload_interval_s,
                basic.summary_interval_s,
                basic.rollup_schema,
                basic.retention_days
            ),
            (300, 900, 1, 365)
        );
        assert_eq!(
            basic.limits,
            TierLimits {
                devices: 60,
                users: 10,
                branches: 1
            }
        );
        let premium = Tier::Premium.defaults();
        assert_eq!(
            premium.flags,
            [
                "uploads",
                "owner_reports",
                "kiosk_orders",
                "kitchen",
                "kitchen_display",
                "standby"
            ]
        );
        assert!(premium.features.is_empty());
        assert_eq!(
            (
                premium.upload_interval_s,
                premium.summary_interval_s,
                premium.rollup_schema,
                premium.retention_days
            ),
            (300, 900, 1, 1095)
        );
        assert_eq!(
            premium.limits,
            TierLimits {
                devices: 200,
                users: 50,
                branches: 1
            }
        );
    }

    #[test]
    fn tiers_and_sources_round_trip() {
        for t in Tier::ALL {
            assert_eq!(Tier::parse(t.as_str()), Some(t));
            assert_eq!(
                serde_json::to_string(&t).unwrap(),
                format!("\"{}\"", t.as_str())
            );
        }
        assert_eq!(Tier::parse("gold"), None);
        for s in [
            EntitlementSource::Free,
            EntitlementSource::Comp,
            EntitlementSource::Paid,
        ] {
            assert_eq!(EntitlementSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(EntitlementSource::parse("trial"), None);
    }
}

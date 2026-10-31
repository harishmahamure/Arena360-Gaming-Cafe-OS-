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

/// A limit a paid tier does not cap (users, branches). It rides the licence as a plain number.
pub const UNLIMITED: u32 = u32::MAX;

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
        flags: &["owner_app", "kiosk_orders", "kitchen", "standby"],
        features: &[],
        upload_interval_s: 900,
        summary_interval_s: 900,
        rollup_schema: 1,
        retention_days: 365,
        limits: TierLimits {
            devices: 60,
            users: UNLIMITED,
            branches: UNLIMITED,
        },
    },
    TierDefaults {
        tier: Tier::Premium,
        flags: &[
            "owner_app",
            "kiosk_orders",
            "kitchen",
            "standby",
            "tunnel",
            "live_view",
        ],
        features: &[],
        upload_interval_s: 300,
        summary_interval_s: 300,
        rollup_schema: 1,
        retention_days: 1095,
        limits: TierLimits {
            devices: 200,
            users: UNLIMITED,
            branches: UNLIMITED,
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
        assert_eq!(
            basic.flags,
            ["owner_app", "kiosk_orders", "kitchen", "standby"]
        );
        assert!(basic.features.is_empty());
        assert_eq!(
            (
                basic.upload_interval_s,
                basic.summary_interval_s,
                basic.rollup_schema,
                basic.retention_days
            ),
            (900, 900, 1, 365)
        );
        assert_eq!(
            basic.limits,
            TierLimits {
                devices: 60,
                users: UNLIMITED,
                branches: UNLIMITED
            }
        );
        let premium = Tier::Premium.defaults();
        assert_eq!(
            premium.flags,
            [
                "owner_app",
                "kiosk_orders",
                "kitchen",
                "standby",
                "tunnel",
                "live_view"
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
            (300, 300, 1, 1095)
        );
        assert_eq!(
            premium.limits,
            TierLimits {
                devices: 200,
                users: UNLIMITED,
                branches: UNLIMITED
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
    const SNAPSHOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/ui/src/tiers.snapshot.ts"
    );

    const HEADER: &str = "// Generated from the tier table in crates/arena-proto/src/tier.rs by\n\
// `UPDATE_TIERS_SNAPSHOT=1 cargo test -p arena-proto tier_snapshot` (Story 11.3).\n\
// Do not edit: CI fails when it differs from the table. `null` is no cap.\n";

    /// The tier table as the marketing Pricing page reads it.
    fn snapshot() -> String {
        let cap = |n: u32| (n != UNLIMITED).then_some(n);
        let tiers: Vec<_> = TIERS
            .iter()
            .map(|d| {
                serde_json::json!({
                    "tier": d.tier.as_str(),
                    "devices": d.limits.devices,
                    "users": cap(d.limits.users),
                    "branches": cap(d.limits.branches),
                    "flags": d.flags,
                    "upload_interval_s": d.upload_interval_s,
                    "retention_days": d.retention_days,
                })
            })
            .collect();
        format!(
            "{HEADER}export const TIERS_SNAPSHOT = {} as const;\n",
            serde_json::to_string_pretty(&tiers).unwrap()
        )
    }

    #[test]
    fn tier_snapshot_matches_the_tier_table() {
        let want = snapshot();
        if std::env::var_os("UPDATE_TIERS_SNAPSHOT").is_some() {
            std::fs::write(SNAPSHOT, &want).unwrap();
            return;
        }
        let have = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
        assert!(
            have.replace("\r\n", "\n") == want,
            "packages/ui/src/tiers.snapshot.ts differs from the tier table; regenerate it with \
             UPDATE_TIERS_SNAPSHOT=1 cargo test -p arena-proto tier_snapshot"
        );
    }
}

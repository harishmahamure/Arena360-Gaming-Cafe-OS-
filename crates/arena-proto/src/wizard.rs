use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tier::{Tier, TierLimits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum WizardStep {
    Cafe,
    Tier,
    InstallHub,
    Devices,
    Rates,
    Products,
    Staff,
    PrinterPayments,
    TestSession,
    GoLive,
}

impl WizardStep {
    pub const ALL: [WizardStep; 10] = [
        Self::Cafe,
        Self::Tier,
        Self::InstallHub,
        Self::Devices,
        Self::Rates,
        Self::Products,
        Self::Staff,
        Self::PrinterPayments,
        Self::TestSession,
        Self::GoLive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cafe => "cafe",
            Self::Tier => "tier",
            Self::InstallHub => "install_hub",
            Self::Devices => "devices",
            Self::Rates => "rates",
            Self::Products => "products",
            Self::Staff => "staff",
            Self::PrinterPayments => "printer_payments",
            Self::TestSession => "test_session",
            Self::GoLive => "go_live",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.as_str() == s)
    }

    pub fn can_skip(self) -> bool {
        matches!(self, Self::Products | Self::Staff | Self::PrinterPayments)
    }

    pub fn number(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum StepState {
    Pending,
    Done,
    Skipped,
}

impl StepState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Done => "done",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "done" => Some(Self::Done),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

pub fn current_step(states: &[(WizardStep, StepState)]) -> WizardStep {
    WizardStep::ALL
        .into_iter()
        .find(|step| {
            !states
                .iter()
                .any(|(s, state)| s == step && *state != StepState::Pending)
        })
        .unwrap_or(WizardStep::GoLive)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountryRow {
    pub code: &'static str,
    pub name: &'static str,
    pub currency: &'static str,
    pub timezone: &'static str,
    pub gst: bool,
}

pub const COUNTRIES: &[CountryRow] = &[
    CountryRow {
        code: "IN",
        name: "India",
        currency: "INR",
        timezone: "Asia/Kolkata",
        gst: true,
    },
    CountryRow {
        code: "NP",
        name: "Nepal",
        currency: "NPR",
        timezone: "Asia/Kathmandu",
        gst: false,
    },
    CountryRow {
        code: "LK",
        name: "Sri Lanka",
        currency: "LKR",
        timezone: "Asia/Colombo",
        gst: false,
    },
    CountryRow {
        code: "BD",
        name: "Bangladesh",
        currency: "BDT",
        timezone: "Asia/Dhaka",
        gst: false,
    },
    CountryRow {
        code: "AE",
        name: "United Arab Emirates",
        currency: "AED",
        timezone: "Asia/Dubai",
        gst: false,
    },
    CountryRow {
        code: "SG",
        name: "Singapore",
        currency: "SGD",
        timezone: "Asia/Singapore",
        gst: false,
    },
    CountryRow {
        code: "MY",
        name: "Malaysia",
        currency: "MYR",
        timezone: "Asia/Kuala_Lumpur",
        gst: false,
    },
    CountryRow {
        code: "GB",
        name: "United Kingdom",
        currency: "GBP",
        timezone: "Europe/London",
        gst: false,
    },
    CountryRow {
        code: "US",
        name: "United States",
        currency: "USD",
        timezone: "America/New_York",
        gst: false,
    },
];

pub fn country(code: &str) -> Option<&'static CountryRow> {
    COUNTRIES.iter().find(|c| c.code == code)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Country {
    pub code: String,
    pub name: String,
    pub currency: String,
    pub timezone: String,
    pub gst: bool,
}

impl From<&CountryRow> for Country {
    fn from(c: &CountryRow) -> Self {
        Self {
            code: c.code.to_owned(),
            name: c.name.to_owned(),
            currency: c.currency.to_owned(),
            timezone: c.timezone.to_owned(),
            gst: c.gst,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WizardStepInfo {
    pub key: WizardStep,
    pub state: StepState,
    pub can_skip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WizardTenant {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WizardBranch {
    pub id: String,
    pub name: String,
    pub city: String,
    pub country: String,
    pub currency: String,
    pub timezone: String,
    pub gst_registered: bool,
    pub gstin: Option<String>,
    pub branch_code: String,
    pub code_editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WizardState {
    pub current: WizardStep,
    pub steps: Vec<WizardStepInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tenant: Option<WizardTenant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch: Option<WizardBranch>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CafeDetails {
    pub cafe_name: String,
    pub city: String,
    pub country: String,
    pub currency: String,
    pub timezone: String,
    pub gst_registered: bool,
    #[serde(default)]
    #[ts(optional)]
    pub gstin: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub branch_id: Option<String>,
}

impl std::fmt::Debug for CafeDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CafeDetails")
            .field("country", &self.country)
            .field("currency", &self.currency)
            .field("timezone", &self.timezone)
            .field("gst_registered", &self.gst_registered)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetBranchCode {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchCodeSet {
    pub branch_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum TierUnavailable {
    OneFreeBranch,
    BillingDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TierOption {
    pub key: Tier,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<TierUnavailable>,
    pub limits: TierLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TierChoices {
    pub tiers: Vec<TierOption>,
    pub billing_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current: Option<Tier>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ChooseTier {
    pub tier: Tier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchHub {
    pub paired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub last_seen_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerBranch {
    pub id: String,
    pub name: String,
    pub branch_code: String,
    pub timezone: String,
    pub tier: Tier,
    pub flags: Vec<String>,
    pub live: bool,
    pub hub: BranchHub,
    pub waiting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UpdateBranch {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AddBranch {
    pub name: String,
}

pub const KEY_BRANCH_NAME: &str = "branch.name";
pub const KEY_BRANCH_CODE: &str = "branch.code";
pub const KEY_BRANCH_CODE_LOCKED_AT: &str = "branch.code_locked_at";
pub const BRANCH_CODE_MIN: usize = 3;
pub const BRANCH_CODE_MAX: usize = 6;

pub fn valid_branch_code(code: &str) -> bool {
    (BRANCH_CODE_MIN..=BRANCH_CODE_MAX).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum GoLiveItem {
    OpeningHours,
    BackupLocation,
    Standby,
    AdConsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum GoLiveBlocker {
    Step(WizardStep),
    Item(GoLiveItem),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GoLiveItemView {
    pub key: GoLiveItem,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GoLiveView {
    pub items: Vec<GoLiveItemView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocker: Option<GoLiveBlocker>,
    pub free: bool,
    pub live: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_ordered_and_round_trip() {
        let keys: Vec<&str> = WizardStep::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            [
                "cafe",
                "tier",
                "install_hub",
                "devices",
                "rates",
                "products",
                "staff",
                "printer_payments",
                "test_session",
                "go_live"
            ]
        );
        for (i, s) in WizardStep::ALL.into_iter().enumerate() {
            assert_eq!(WizardStep::parse(s.as_str()), Some(s));
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!("\"{}\"", s.as_str())
            );
            assert_eq!(s.number(), i + 1);
        }
        assert_eq!(WizardStep::parse("nope"), None);
        let skippable: Vec<WizardStep> = WizardStep::ALL
            .into_iter()
            .filter(|s| s.can_skip())
            .collect();
        assert_eq!(
            skippable,
            [
                WizardStep::Products,
                WizardStep::Staff,
                WizardStep::PrinterPayments
            ]
        );
    }

    #[test]
    fn current_is_the_first_step_not_done() {
        assert_eq!(current_step(&[]), WizardStep::Cafe);
        assert_eq!(
            current_step(&[
                (WizardStep::Cafe, StepState::Done),
                (WizardStep::Tier, StepState::Pending),
                (WizardStep::InstallHub, StepState::Done),
            ]),
            WizardStep::Tier
        );
        let all: Vec<_> = WizardStep::ALL
            .into_iter()
            .map(|s| (s, StepState::Done))
            .collect();
        assert_eq!(current_step(&all), WizardStep::GoLive);
        assert_eq!(
            current_step(&[
                (WizardStep::Cafe, StepState::Done),
                (WizardStep::Tier, StepState::Skipped),
            ]),
            WizardStep::InstallHub
        );
    }

    #[test]
    fn countries_use_known_currencies_and_zones() {
        assert_eq!(
            COUNTRIES.iter().map(|c| c.code).collect::<Vec<_>>(),
            ["IN", "NP", "LK", "BD", "AE", "SG", "MY", "GB", "US"]
        );
        for c in COUNTRIES {
            assert!(crate::currency::is_known_currency(c.currency), "{}", c.code);
            assert!(
                crate::currency::is_canonical_timezone(c.timezone),
                "{}",
                c.code
            );
            assert_eq!(c.gst, c.code == "IN");
        }
        assert_eq!(country("IN").map(|c| c.currency), Some("INR"));
        assert_eq!(country("in"), None);
    }

    #[test]
    fn branch_codes_are_three_to_six_upper_alnum() {
        for ok in ["PAG", "ARENA1", "A1B2"] {
            assert!(valid_branch_code(ok), "{ok}");
        }
        for bad in ["ab", "AB", "ABCDEFG", "AB-C", "abc", "ÄBC"] {
            assert!(!valid_branch_code(bad), "{bad}");
        }
    }

    #[test]
    fn wizard_state_omits_absent_tenant_and_branch() {
        let s = WizardState {
            current: WizardStep::Cafe,
            steps: vec![],
            tenant: None,
            branch: None,
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"current":"cafe","steps":[]}"#
        );
    }
}

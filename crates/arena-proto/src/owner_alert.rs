//! Owner alerts (Story 8.33): what the cloud raises for the café owner, keeps in a list, pushes to
//! the owner's phones and sends on WhatsApp. An alert is a kind plus its display parameters; the
//! plain-text line (staff register, English) and the deep link are derived from them, so a later
//! kind is one more arm of [`OwnerAlertKind`]. Alerts carry player ids at most: never a phone, a
//! name or an email.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest value of one alert parameter, in characters. Three of them and the longest line
/// stay inside one WhatsApp template parameter ([`crate::NOTIFY_PARAM_MAX_CHARS`]).
pub const OWNER_ALERT_PARAM_MAX_CHARS: usize = 40;
/// Alerts a page of `GET /v1/alerts` holds unless the call says otherwise.
pub const OWNER_ALERTS_PAGE: usize = 30;
/// The most alerts one page holds.
pub const OWNER_ALERTS_PAGE_MAX: usize = 100;
/// The setting that holds a branch's [`OwnerAlertThresholds`] (Story 8.34).
pub const KEY_OWNER_ALERT_THRESHOLDS: &str = "owner_alert.thresholds";
/// The default value of [`KEY_OWNER_ALERT_THRESHOLDS`]: [`OwnerAlertThresholds::default`] as JSON.
pub const DEFAULT_OWNER_ALERT_THRESHOLDS: &str = r#"{"cash_variance_minor":null,"large_refund_minor":50000,"large_discount_minor":20000,"large_discount_percent":20}"#;
/// The largest money threshold, in minor units.
pub const OWNER_ALERT_MAX_MINOR: i64 = 100_000_000;
/// Push subscriptions one owner may keep.
pub const PUSH_SUBSCRIPTIONS_MAX: u32 = 10;
/// How long a push service keeps a message it cannot deliver yet, in seconds.
pub const PUSH_TTL_S: u32 = 86_400;

/// What an alert is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum OwnerAlertKind {
    /// A stand-in that tests and the dev fixture raise.
    Test,
    /// A shift closed with the cash off by more than the branch's threshold.
    CashVariance,
    /// A tracked product fell to its low-stock level.
    LowStock,
    /// A player's credit is past its due days.
    CreditOverdue,
    /// A refund above the branch's limit.
    LargeRefund,
    /// A discount above the branch's limit.
    LargeDiscount,
    /// The branch's Hub stopped reporting.
    BranchOffline,
    /// The branch's Hub reports but has stopped backing up (Story 8.35).
    BranchNotUploading,
    /// The branch's Hub reports again after an offline alert (Story 8.35).
    BranchOnline,
    /// A Manager forced the standby Hub to main (Story 9.22).
    StandbyForced,
    /// A subscription charge's GST tax invoice is ready (Story 10.7). Raised by the cloud's jobs
    /// role, never by a Hub.
    InvoiceReady,
}

/// Why a set of parameters does not fit its kind: the offending name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerAlertParamError {
    pub field: String,
    pub message: String,
}

impl OwnerAlertKind {
    /// Every kind.
    pub const ALL: [Self; 11] = [
        Self::Test,
        Self::CashVariance,
        Self::LowStock,
        Self::CreditOverdue,
        Self::LargeRefund,
        Self::LargeDiscount,
        Self::BranchOffline,
        Self::BranchNotUploading,
        Self::BranchOnline,
        Self::StandbyForced,
        Self::InvoiceReady,
    ];

    /// The kinds a Hub raises through `POST /v1/notify` (Story 8.34).
    pub const HUB_SOURCED: [Self; 6] = [
        Self::CashVariance,
        Self::LowStock,
        Self::CreditOverdue,
        Self::LargeRefund,
        Self::LargeDiscount,
        Self::StandbyForced,
    ];

    /// Whether a Hub may raise this kind.
    pub fn hub_sourced(self) -> bool {
        Self::HUB_SOURCED.contains(&self)
    }

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::CashVariance => "cash_variance",
            Self::LowStock => "low_stock",
            Self::CreditOverdue => "credit_overdue",
            Self::LargeRefund => "large_refund",
            Self::LargeDiscount => "large_discount",
            Self::BranchOffline => "branch_offline",
            Self::BranchNotUploading => "branch_not_uploading",
            Self::BranchOnline => "branch_online",
            Self::StandbyForced => "standby_forced",
            Self::InvoiceReady => "invoice_ready",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The display parameters every alert of this kind carries.
    pub fn required_params(self) -> &'static [&'static str] {
        match self {
            Self::Test => &[],
            Self::CashVariance => &["amount", "shift"],
            Self::LowStock => &["product", "qty"],
            Self::CreditOverdue => &["amount", "days"],
            Self::LargeRefund | Self::LargeDiscount => &["amount"],
            Self::BranchOffline | Self::BranchNotUploading | Self::BranchOnline => &["minutes"],
            Self::StandbyForced => &["manager"],
            Self::InvoiceReady => &["number", "amount"],
        }
    }

    /// Parameters an alert of this kind may also carry (ids, never personal details).
    pub fn optional_params(self) -> &'static [&'static str] {
        match self {
            Self::CreditOverdue => &["player_id"],
            Self::CashVariance => &["reason"],
            _ => &[],
        }
    }

    /// Checks `params` against the kind: every required one present, none that the kind does not
    /// name, each 1 to [`OWNER_ALERT_PARAM_MAX_CHARS`] characters of plain text.
    pub fn check_params(
        self,
        params: &BTreeMap<String, String>,
    ) -> Result<(), OwnerAlertParamError> {
        let fail = |field: &str, message: String| OwnerAlertParamError {
            field: field.to_owned(),
            message,
        };
        for name in self.required_params() {
            if !params.contains_key(*name) {
                return Err(fail(name, format!("Send the parameter {name}.")));
            }
        }
        for (name, value) in params {
            let known = self.required_params().contains(&name.as_str())
                || self.optional_params().contains(&name.as_str());
            if !known {
                return Err(fail(name, format!("This alert has no parameter {name}.")));
            }
            if value.trim().is_empty()
                || value.chars().count() > OWNER_ALERT_PARAM_MAX_CHARS
                || value.chars().any(char::is_control)
            {
                return Err(fail(
                    name,
                    format!(
                        "The parameter {name} must be 1 to {OWNER_ALERT_PARAM_MAX_CHARS} characters of plain text."
                    ),
                ));
            }
        }
        Ok(())
    }

    /// The alert's plain-text line, without the branch name. `params` must have passed
    /// [`Self::check_params`]; a missing one reads as a dash.
    pub fn text(self, params: &BTreeMap<String, String>) -> String {
        let p = |name: &str| params.get(name).map_or("-", String::as_str);
        match self {
            Self::Test => "Test alert. Alerts from your branches will reach you here.".to_owned(),
            Self::CashVariance => {
                let reason = params
                    .get("reason")
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default();
                format!(
                    "Cash is off by {} at the close of the {} shift{reason}.",
                    p("amount"),
                    p("shift")
                )
            }
            Self::LowStock => format!("{} is running low: {} left.", p("product"), p("qty")),
            Self::CreditOverdue => {
                let days = p("days");
                let unit = if days == "1" { "day" } else { "days" };
                format!("Credit of {} is overdue by {days} {unit}.", p("amount"))
            }
            Self::LargeRefund => format!("A large refund of {} was given.", p("amount")),
            Self::LargeDiscount => format!("A large discount of {} was given.", p("amount")),
            Self::BranchOffline => {
                let minutes = p("minutes");
                let unit = if minutes == "1" { "minute" } else { "minutes" };
                format!("The branch has been offline for {minutes} {unit}.")
            }
            Self::BranchNotUploading => {
                let minutes = p("minutes");
                let unit = if minutes == "1" { "minute" } else { "minutes" };
                format!("The branch is online but has not backed up for {minutes} {unit}.")
            }
            Self::BranchOnline => {
                let minutes = p("minutes");
                let unit = if minutes == "1" { "minute" } else { "minutes" };
                format!("The branch is back online after {minutes} {unit}.")
            }
            Self::StandbyForced => format!(
                "The standby Hub was forced to main by {}. Data since its last sync may be lost.",
                p("manager")
            ),
            Self::InvoiceReady => {
                format!("Invoice ready: {} for {}.", p("number"), p("amount"))
            }
        }
    }

    /// The path in the Owner app that opens what the alert is about.
    pub fn url(self, branch_id: &str) -> String {
        match self {
            Self::Test | Self::InvoiceReady => "/alerts".to_owned(),
            Self::BranchOffline
            | Self::BranchNotUploading
            | Self::BranchOnline
            | Self::StandbyForced => {
                format!("/#today-{branch_id}")
            }
            Self::CashVariance => format!("/reports?tab=cash&branch={branch_id}"),
            Self::LowStock => format!("/reports?tab=stock&branch={branch_id}"),
            Self::CreditOverdue => format!("/reports?tab=credit&branch={branch_id}"),
            Self::LargeRefund | Self::LargeDiscount => format!("/reports?branch={branch_id}"),
        }
    }
}

/// The per-branch limits above which the Hub raises an alert (setting
/// [`KEY_OWNER_ALERT_THRESHOLDS`], a JSON object; Story 8.34).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertThresholds {
    /// A shift closed with the cash off by more than this; `None` is the branch's own variance
    /// threshold.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub cash_variance_minor: Option<i64>,
    /// A refund of more than this.
    #[ts(type = "number")]
    pub large_refund_minor: i64,
    /// A manual discount of more than this.
    #[ts(type = "number")]
    pub large_discount_minor: i64,
    /// A manual discount of more than this percent of the bill before it.
    #[ts(type = "number")]
    pub large_discount_percent: i64,
}

impl Default for OwnerAlertThresholds {
    fn default() -> Self {
        Self {
            cash_variance_minor: None,
            large_refund_minor: 50_000,
            large_discount_minor: 20_000,
            large_discount_percent: 20,
        }
    }
}

impl OwnerAlertThresholds {
    /// Reads the setting's JSON: every field but `cash_variance_minor` present, nothing unknown,
    /// money in `0..=`[`OWNER_ALERT_MAX_MINOR`] and the percent in `1..=100`.
    pub fn parse(json: &str) -> Option<Self> {
        let t: Self = serde_json::from_str(json).ok()?;
        let money = |v: i64| (0..=OWNER_ALERT_MAX_MINOR).contains(&v);
        (t.cash_variance_minor.is_none_or(money)
            && money(t.large_refund_minor)
            && money(t.large_discount_minor)
            && (1..=100).contains(&t.large_discount_percent))
        .then_some(t)
    }

    /// The setting's JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// One alert as the Alerts tab lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertView {
    pub id: String,
    pub branch_id: String,
    pub branch_name: String,
    pub kind: OwnerAlertKind,
    /// The plain-text line, without the branch name.
    pub text: String,
    /// A path in the Owner app.
    pub url: String,
    /// Unix ms.
    #[ts(type = "number")]
    pub created_at: i64,
}

/// `GET /v1/alerts`: newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertList {
    pub alerts: Vec<OwnerAlertView>,
    /// Pass as `before` for the next (older) page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_before: Option<String>,
}

/// `GET /v1/alerts/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertSettings {
    /// The VAPID public key (base64url) a browser subscribes with; null when push is not set up.
    pub vapid_public_key: Option<String>,
    /// Whether alerts also go out on WhatsApp.
    pub whatsapp: bool,
    /// How many browsers the owner has turned push on in.
    pub subscriptions: u32,
}

/// `PATCH /v1/alerts/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertSettingsPatch {
    pub whatsapp: bool,
}

/// The keys of a browser's push subscription (base64url).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

impl std::fmt::Debug for PushKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushKeys(..)")
    }
}

/// `POST /v1/alerts/push`: a browser's `PushSubscription.toJSON()` (without the expiry).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PushSubscribe {
    pub endpoint: String,
    pub keys: PushKeys,
}

impl std::fmt::Debug for PushSubscribe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushSubscribe(..)")
    }
}

/// `POST /v1/alerts/push/remove`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PushRemove {
    pub endpoint: String,
}

impl std::fmt::Debug for PushRemove {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushRemove(..)")
    }
}

/// What a push carries (the service worker shows it): the branch name, the line, the path to open
/// and the alert id as the notification tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerAlertPush {
    pub title: String,
    pub body: String,
    pub url: String,
    pub tag: String,
}

/// `POST /v1/dev/owner-alerts` (`--dev` only): raises an alert for a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DevOwnerAlert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub branch_id: String,
    /// An [`OwnerAlertKind`] name.
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// A full set of parameters for `kind`.
    fn sample(kind: OwnerAlertKind) -> BTreeMap<String, String> {
        let values = [
            ("amount", "Rs 250"),
            ("shift", "evening"),
            ("product", "Cola"),
            ("qty", "3"),
            ("days", "9"),
            ("minutes", "20"),
            ("manager", "Asha"),
            ("number", "A360/2026-27/000001"),
        ];
        params(
            &values
                .into_iter()
                .filter(|(k, _)| kind.required_params().contains(k))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn every_kind_has_its_line() {
        let line = |k| OwnerAlertKind::text(k, &sample(k));
        assert_eq!(
            line(OwnerAlertKind::Test),
            "Test alert. Alerts from your branches will reach you here."
        );
        assert_eq!(
            line(OwnerAlertKind::CashVariance),
            "Cash is off by Rs 250 at the close of the evening shift."
        );
        assert_eq!(
            line(OwnerAlertKind::LowStock),
            "Cola is running low: 3 left."
        );
        assert_eq!(
            line(OwnerAlertKind::CreditOverdue),
            "Credit of Rs 250 is overdue by 9 days."
        );
        assert_eq!(
            line(OwnerAlertKind::LargeRefund),
            "A large refund of Rs 250 was given."
        );
        assert_eq!(
            line(OwnerAlertKind::LargeDiscount),
            "A large discount of Rs 250 was given."
        );
        assert_eq!(
            line(OwnerAlertKind::BranchOffline),
            "The branch has been offline for 20 minutes."
        );
        assert_eq!(
            line(OwnerAlertKind::BranchNotUploading),
            "The branch is online but has not backed up for 20 minutes."
        );
        assert_eq!(
            line(OwnerAlertKind::BranchOnline),
            "The branch is back online after 20 minutes."
        );
    }

    #[test]
    fn one_day_and_one_minute_are_singular() {
        let p = params(&[("amount", "Rs 5"), ("days", "1")]);
        assert_eq!(
            OwnerAlertKind::CreditOverdue.text(&p),
            "Credit of Rs 5 is overdue by 1 day."
        );
        let p = params(&[("minutes", "1")]);
        assert_eq!(
            OwnerAlertKind::BranchOffline.text(&p),
            "The branch has been offline for 1 minute."
        );
    }

    #[test]
    fn every_kind_has_its_link() {
        let url = |k: OwnerAlertKind| k.url("B1");
        assert_eq!(url(OwnerAlertKind::Test), "/alerts");
        assert_eq!(url(OwnerAlertKind::BranchOffline), "/#today-B1");
        assert_eq!(url(OwnerAlertKind::BranchNotUploading), "/#today-B1");
        assert_eq!(url(OwnerAlertKind::BranchOnline), "/#today-B1");
        assert_eq!(url(OwnerAlertKind::StandbyForced), "/#today-B1");
        assert_eq!(
            url(OwnerAlertKind::CashVariance),
            "/reports?tab=cash&branch=B1"
        );
        assert_eq!(
            url(OwnerAlertKind::LowStock),
            "/reports?tab=stock&branch=B1"
        );
        assert_eq!(
            url(OwnerAlertKind::CreditOverdue),
            "/reports?tab=credit&branch=B1"
        );
        assert_eq!(url(OwnerAlertKind::LargeRefund), "/reports?branch=B1");
        assert_eq!(url(OwnerAlertKind::LargeDiscount), "/reports?branch=B1");
    }

    #[test]
    fn kind_names_round_trip_and_each_sample_passes() {
        for k in OwnerAlertKind::ALL {
            assert_eq!(OwnerAlertKind::parse(k.as_str()), Some(k));
            assert_eq!(k.check_params(&sample(k)), Ok(()), "{}", k.as_str());
            assert!(k.text(&sample(k)).chars().count() <= 200);
        }
        assert_eq!(OwnerAlertKind::parse("nope"), None);
        assert_eq!(
            serde_json::to_string(&OwnerAlertKind::BranchOffline).unwrap(),
            "\"branch_offline\""
        );
    }

    #[test]
    fn a_missing_unknown_empty_or_long_parameter_is_refused() {
        let k = OwnerAlertKind::LowStock;
        let err = |p: &BTreeMap<String, String>| k.check_params(p).unwrap_err().field;
        assert_eq!(err(&params(&[("product", "Cola")])), "qty");
        assert_eq!(
            err(&params(&[
                ("product", "Cola"),
                ("qty", "3"),
                ("phone", "+91")
            ])),
            "phone"
        );
        assert_eq!(err(&params(&[("product", " "), ("qty", "3")])), "product");
        let long = "x".repeat(OWNER_ALERT_PARAM_MAX_CHARS + 1);
        assert_eq!(
            err(&params(&[("product", long.as_str()), ("qty", "3")])),
            "product"
        );
        assert_eq!(
            err(&params(&[("product", "Co\nla"), ("qty", "3")])),
            "product"
        );
    }

    #[test]
    fn a_forced_standby_line_names_the_manager() {
        assert_eq!(
            OwnerAlertKind::StandbyForced.text(&sample(OwnerAlertKind::StandbyForced)),
            "The standby Hub was forced to main by Asha. Data since its last sync may be lost."
        );
    }

    #[test]
    fn an_invoice_ready_line_names_the_number_and_amount_and_no_hub_raises_it() {
        let k = OwnerAlertKind::InvoiceReady;
        let p = params(&[("number", "A360/2026-27/000001"), ("amount", "Rs 1180.00")]);
        assert_eq!(k.check_params(&p), Ok(()));
        assert_eq!(
            k.text(&p),
            "Invoice ready: A360/2026-27/000001 for Rs 1180.00."
        );
        assert_eq!(k.as_str(), "invoice_ready");
        assert!(!k.hub_sourced());
        assert_eq!(
            k.check_params(&params(&[("amount", "Rs 1")]))
                .unwrap_err()
                .field,
            "number"
        );
    }

    #[test]
    fn a_cash_variance_line_carries_its_reason() {
        let mut p = sample(OwnerAlertKind::CashVariance);
        p.insert("reason".into(), "counting error".into());
        assert_eq!(OwnerAlertKind::CashVariance.check_params(&p), Ok(()));
        assert_eq!(
            OwnerAlertKind::CashVariance.text(&p),
            "Cash is off by Rs 250 at the close of the evening shift (counting error)."
        );
    }

    #[test]
    fn the_hub_raises_six_kinds() {
        assert_eq!(OwnerAlertKind::HUB_SOURCED.len(), 6);
        assert!(OwnerAlertKind::StandbyForced.hub_sourced());
        assert!(OwnerAlertKind::LowStock.hub_sourced());
        assert!(!OwnerAlertKind::Test.hub_sourced());
        assert!(!OwnerAlertKind::BranchOffline.hub_sourced());
        assert!(!OwnerAlertKind::BranchNotUploading.hub_sourced());
        assert!(!OwnerAlertKind::BranchOnline.hub_sourced());
    }

    #[test]
    fn thresholds_parse_strictly() {
        let d = OwnerAlertThresholds::default();
        assert_eq!(OwnerAlertThresholds::parse(&d.to_json()), Some(d));
        assert_eq!(d.to_json(), DEFAULT_OWNER_ALERT_THRESHOLDS);
        let ok = |j: &str| OwnerAlertThresholds::parse(j).is_some();
        assert!(ok(
            r#"{"large_refund_minor":0,"large_discount_minor":1,"large_discount_percent":100}"#
        ));
        assert!(ok(
            r#"{"cash_variance_minor":0,"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":1}"#
        ));
        assert!(!ok(r#"{"large_refund_minor":1}"#));
        assert!(!ok(
            r#"{"large_refund_minor":-1,"large_discount_minor":1,"large_discount_percent":1}"#
        ));
        assert!(!ok(
            r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":0}"#
        ));
        assert!(!ok(
            r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":101}"#
        ));
        assert!(!ok(
            r#"{"cash_variance_minor":-5,"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":1}"#
        ));
        assert!(!ok(
            r#"{"large_refund_minor":100000001,"large_discount_minor":1,"large_discount_percent":1}"#
        ));
        assert!(!ok(
            r#"{"large_refund_minor":1,"large_discount_minor":1,"large_discount_percent":1,"x":1}"#
        ));
        assert!(!ok("nope"));
    }

    #[test]
    fn a_player_id_is_allowed_only_where_named() {
        let mut p = sample(OwnerAlertKind::CreditOverdue);
        p.insert("player_id".into(), "01J9PLAYER".into());
        assert_eq!(OwnerAlertKind::CreditOverdue.check_params(&p), Ok(()));
        let mut p = sample(OwnerAlertKind::LargeRefund);
        p.insert("player_id".into(), "01J9PLAYER".into());
        assert_eq!(
            OwnerAlertKind::LargeRefund
                .check_params(&p)
                .unwrap_err()
                .field,
            "player_id"
        );
    }
}

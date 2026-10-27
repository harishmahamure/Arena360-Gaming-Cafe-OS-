use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const RULE_KINDS: [&str; 2] = ["weekly", "date"];
pub const RULE_ADJUST_KINDS: [&str; 2] = ["override_minor", "percent_bp"];
pub const RULE_ALL_DAYS: i64 = 0b111_1111;
pub const RULE_MIN_PERCENT_BP: i64 = -10_000;
pub const RULE_MAX_PERCENT_BP: i64 = 100_000;
pub const RULE_MAX_PRIORITY: i64 = 1000;
pub const RULE_MAX_DATE_SPAN_DAYS: i64 = 366;

fn yes() -> bool {
    true
}

/// The payload of `rule.upsert`: a weekly rule (`days_mask`, bit 0 = Monday, and an hour range
/// in minutes from local midnight that may wrap midnight) or a date rule (`date_from` to
/// `date_to`, inclusive local `YYYY-MM-DD`), for one device group, that either replaces the
/// group's rate (`override_minor`, minor units per hour) or moves it by a percentage
/// (`percent_bp`, basis points, negative = discount). A rule with an `id` is an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RuleUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub name: String,
    pub group_id: String,
    pub rule_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub days_mask: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub end_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_to: Option<String>,
    pub adjust_kind: String,
    #[ts(type = "number")]
    pub adjust_value: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub priority: i64,
    #[serde(default = "yes")]
    pub active: bool,
}

/// Another active rule that applies to the same group at the same time as the one it is listed
/// under, and whether that other rule is the one that wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RuleConflict {
    pub rule_id: String,
    pub name: String,
    pub other_wins: bool,
}

/// One rule as `GET /ui/rules` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RuleView {
    pub id: String,
    pub name: String,
    pub group_id: String,
    pub rule_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub days_mask: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub end_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_to: Option<String>,
    pub adjust_kind: String,
    #[ts(type = "number")]
    pub adjust_value: i64,
    #[ts(type = "number")]
    pub priority: i64,
    pub active: bool,
    #[ts(type = "number")]
    pub rev: i64,
    pub conflicts: Vec<RuleConflict>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_defaults_to_active_with_priority_zero() {
        let u: RuleUpsert = serde_json::from_value(serde_json::json!({
            "name": "Happy hour", "group_id": "G", "rule_kind": "weekly", "days_mask": 31,
            "start_min": 960, "end_min": 1080, "adjust_kind": "percent_bp", "adjust_value": -2000
        }))
        .unwrap();
        assert!(u.active);
        assert_eq!((u.priority, u.id, u.date_from), (0, None, None));
    }
}

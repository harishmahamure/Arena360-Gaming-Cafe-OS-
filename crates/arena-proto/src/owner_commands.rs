//! The owner command catalogue (Story 8.18, AD-5): the kinds an owner may send to a branch, the
//! payload each one carries, the tier and flag it needs and whether it is sensitive. The cloud
//! checks a post against this list before it queues a row; the Hub runs the `handled` kinds (Story
//! 8.19, `arena_core::HANDLED_COMMAND_KINDS`) and the later stories add kinds.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::commands::COMMAND_SCHEMA_VERSION;
use crate::error::{AppError, ErrorCode};
use crate::{
    CouponUpsert, PlanUpsert, ProductUpsert, RateUpsert, RuleUpsert, StockThreshold, Tier,
};

/// Owner command posts a tenant may make in a minute.
pub const OWNER_COMMAND_POSTS_PER_MIN: u64 = 60;
pub const OWNER_COMMANDS_PAGE: usize = 50;
pub const OWNER_COMMANDS_PAGE_MAX: usize = 200;

/// One entry of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerCommandKind {
    pub kind: &'static str,
    /// The command `schema_version` this kind is sent at; a branch must report at least this.
    pub schema_version: u32,
    /// The lowest tier of the branch that may be sent this kind.
    pub min_tier: Tier,
    /// A licence flag the branch must also hold, when the kind needs one.
    pub flag: Option<&'static str>,
    /// Needs a step-up proof younger than [`crate::STEPUP_TTL_MS`] on the owner's session (Story
    /// 8.25); the cloud refuses a post `MFA_REQUIRED` without one.
    pub sensitive: bool,
    /// The Hub runs this kind. A name reserved without a handler is refused `UNSUPPORTED`.
    pub handled: bool,
}

const fn handled(kind: &'static str) -> OwnerCommandKind {
    OwnerCommandKind {
        kind,
        schema_version: COMMAND_SCHEMA_VERSION,
        min_tier: Tier::Basic,
        flag: None,
        sensitive: false,
        handled: true,
    }
}

const fn reserved(kind: &'static str) -> OwnerCommandKind {
    OwnerCommandKind {
        kind,
        schema_version: COMMAND_SCHEMA_VERSION,
        min_tier: Tier::Basic,
        flag: None,
        sensitive: true,
        handled: false,
    }
}

pub const OWNER_COMMAND_KINDS: [OwnerCommandKind; 9] = [
    handled("rate.upsert"),
    handled("plan.upsert"),
    handled("rule.upsert"),
    handled("coupon.upsert"),
    handled("product.upsert"),
    handled("stock.threshold"),
    handled("backup.now"),
    reserved("hub.revoke"),
    reserved("backup.restore"),
];

pub fn owner_command_kind(kind: &str) -> Option<&'static OwnerCommandKind> {
    OWNER_COMMAND_KINDS.iter().find(|k| k.kind == kind)
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// The kind the cloud accepts, or `UNSUPPORTED` for a name outside the catalogue or one reserved
/// without a handler.
pub fn accepted_owner_kind(kind: &str) -> Result<&'static OwnerCommandKind, AppError> {
    owner_command_kind(kind)
        .filter(|k| k.handled)
        .ok_or_else(|| {
            AppError::with_field(
                ErrorCode::Unsupported,
                "kind",
                "This change can't be sent to the branch yet.",
            )
        })
}

fn parses<T: serde::de::DeserializeOwned>(payload: &serde_json::Value) -> bool {
    serde_json::from_value::<T>(payload.clone()).is_ok()
}

/// Check a payload against its kind's schema. `base_rev` rides beside the fields and is not part
/// of them; `backup.now` takes no fields.
pub fn validate_owner_payload(kind: &str, payload: &serde_json::Value) -> Result<(), AppError> {
    let Some(fields) = payload.as_object() else {
        return Err(invalid("payload", "Send the payload as an object."));
    };
    let mut fields = fields.clone();
    fields.remove("base_rev");
    let fields = serde_json::Value::Object(fields);
    let fits = match kind {
        "rate.upsert" => parses::<RateUpsert>(&fields),
        "plan.upsert" => parses::<PlanUpsert>(&fields),
        "rule.upsert" => parses::<RuleUpsert>(&fields),
        "coupon.upsert" => parses::<CouponUpsert>(&fields),
        "product.upsert" => parses::<ProductUpsert>(&fields),
        "stock.threshold" => parses::<StockThreshold>(&fields),
        "backup.now" => fields.as_object().is_some_and(|f| f.is_empty()),
        _ => false,
    };
    if fits {
        Ok(())
    } else {
        Err(invalid(
            "payload",
            "The payload does not match this kind of change.",
        ))
    }
}

/// The kinds whose owner command raises `updated_by_owner` on the Hub's UI event stream (Story
/// 8.19), with the entity the event names, the change-log table of the row and the key of the toast
/// line in the Hub's strings catalogue. Later kinds add a row here and a line to the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerUpdate {
    pub kind: &'static str,
    pub entity: &'static str,
    pub tbl: &'static str,
    pub copy_key: &'static str,
}

pub const OWNER_UPDATES: [OwnerUpdate; 5] = [
    OwnerUpdate {
        kind: "rate.upsert",
        entity: "rate",
        tbl: "rates",
        copy_key: "hub.owner_update.rate",
    },
    OwnerUpdate {
        kind: "plan.upsert",
        entity: "plan",
        tbl: "plans",
        copy_key: "hub.owner_update.plan",
    },
    OwnerUpdate {
        kind: "rule.upsert",
        entity: "rule",
        tbl: "pricing_rules",
        copy_key: "hub.owner_update.rule",
    },
    OwnerUpdate {
        kind: "coupon.upsert",
        entity: "coupon",
        tbl: "coupons",
        copy_key: "hub.owner_update.coupon",
    },
    OwnerUpdate {
        kind: "product.upsert",
        entity: "product",
        tbl: "products",
        copy_key: "hub.owner_update.product",
    },
];

pub fn owner_update_for_table(tbl: &str) -> Option<&'static OwnerUpdate> {
    OWNER_UPDATES.iter().find(|u| u.tbl == tbl)
}

/// The change an owner command overwrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OverwrittenChange {
    pub intent_id: String,
    /// `staff`, `owner`, `admin`, `system` or `device`.
    pub actor_kind: String,
    pub actor_id: String,
    /// When it was made (Hub clock, ms).
    #[ts(type = "number")]
    pub at: i64,
}

/// `event: updated_by_owner` on `GET /ui/events`: an owner command of one of [`OWNER_UPDATES`]
/// committed. `overwritten` is present only when it replaced a change made after the owner's
/// `base_rev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UpdatedByOwnerEvent {
    pub kind: String,
    pub entity: String,
    pub row_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub overwritten: Option<OverwrittenChange>,
}

/// `POST /v1/commands`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandPost {
    /// A branch id, or `all` for every branch the owner may send this kind to.
    pub branch_id: String,
    pub idempotency_key: String,
    pub kind: String,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub base_rev: Option<i64>,
}

/// One queued command as the owner sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandView {
    #[ts(type = "number")]
    pub id: i64,
    pub branch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_id: Option<String>,
    pub kind: String,
    pub status: crate::CommandStatus,
    /// What was sent (with `base_rev` beside its fields), so Retry and Resend can send it again.
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    /// Why the Hub refused it, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<AppError>,
    /// When the cloud queued it (cloud clock, ms).
    #[ts(type = "number")]
    pub queued_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub done_at: Option<i64>,
}

/// `POST /v1/commands/preview`: which branches a change of this kind would reach (Story 8.21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandPreviewRequest {
    /// A branch id, or `all`.
    pub branch_id: String,
    pub kind: String,
}

/// One branch of a preview: whether its Hub was heard from lately and whether it can take the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandTarget {
    pub branch_id: String,
    pub name: String,
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_heartbeat_at: Option<i64>,
    /// `false` when the branch cannot take the kind; `error` says why (`FEATURE_NOT_ENTITLED`,
    /// `BRANCH_UPDATE_REQUIRED`).
    pub offered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandPreview {
    pub targets: Vec<OwnerCommandTarget>,
}

/// A branch an "all branches" post did not reach, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandSkip {
    pub branch_id: String,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandPosted {
    pub commands: Vec<OwnerCommandView>,
    pub skipped: Vec<OwnerCommandSkip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerCommandList {
    pub commands: Vec<OwnerCommandView>,
    /// Pass as `before` for the next (older) page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub next_before: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_kinds_are_handled_and_the_reserved_ones_are_not() {
        for k in [
            "rate.upsert",
            "plan.upsert",
            "rule.upsert",
            "coupon.upsert",
            "product.upsert",
            "backup.now",
        ] {
            let kind = accepted_owner_kind(k).unwrap();
            assert!(!kind.sensitive && kind.handled, "{k}");
            assert_eq!(kind.schema_version, COMMAND_SCHEMA_VERSION);
        }
        for k in ["hub.revoke", "backup.restore"] {
            assert!(owner_command_kind(k).unwrap().sensitive, "{k}");
            assert_eq!(
                accepted_owner_kind(k).unwrap_err().code,
                ErrorCode::Unsupported
            );
        }
        assert_eq!(
            accepted_owner_kind("pricing.set").unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn payloads_are_checked_against_their_kind() {
        let rate = json!({"group_id": "g1", "price_per_hour_minor": 5000, "base_rev": 3});
        assert!(validate_owner_payload("rate.upsert", &rate).is_ok());
        assert!(validate_owner_payload("rate.upsert", &json!({"group_id": "g1"})).is_err());
        assert!(validate_owner_payload("rate.upsert", &json!([1])).is_err());
        assert!(validate_owner_payload("backup.now", &json!({})).is_ok());
        assert!(validate_owner_payload("backup.now", &json!({"x": 1})).is_err());
        assert!(
            validate_owner_payload(
                "coupon.upsert",
                &json!({"code": "WELCOME", "percent_bp": 1000})
            )
            .is_ok()
        );
        assert!(validate_owner_payload("coupon.upsert", &rate).is_err());
    }
}

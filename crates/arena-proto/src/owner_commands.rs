//! The owner command catalogue (Story 8.18, AD-5): the kinds an owner may send to a branch, the
//! payload each one carries, the tier and flag it needs and whether it is sensitive. The cloud
//! checks a post against this list before it queues a row; the Hub runs the `handled` kinds (Story
//! 8.19, `arena_core::HANDLED_COMMAND_KINDS`) and the later stories add kinds.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::commands::COMMAND_SCHEMA_VERSION;
use crate::error::{AppError, ErrorCode};
use crate::{
    CouponUpsert, DeviceAssign, GroupUpsert, KEY_OWNER_ALERT_THRESHOLDS, OwnerAlertThresholds,
    Permission, PlanUpsert, ProductUpsert, RateUpsert, ReasonCode, RoleUpsert, RuleUpsert,
    SettingsSet, StaffDeactivate, StockThreshold, Tier,
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

/// A kind the Hub runs that needs a step-up proof (Story 8.26).
const fn sensitive(kind: &'static str) -> OwnerCommandKind {
    OwnerCommandKind {
        sensitive: true,
        ..handled(kind)
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

pub const OWNER_COMMAND_KINDS: [OwnerCommandKind; 17] = [
    handled("rate.upsert"),
    handled("plan.upsert"),
    handled("rule.upsert"),
    handled("coupon.upsert"),
    handled("product.upsert"),
    handled("stock.threshold"),
    handled("backup.now"),
    sensitive("staff.upsert"),
    sensitive("staff.deactivate"),
    sensitive("role.upsert"),
    handled("group.upsert"),
    handled("device.assign"),
    sensitive("device.revoke"),
    handled("credit.remind"),
    // The alert limits (Story 8.34): `settings.set` of `owner_alert.thresholds` only.
    handled("settings.set"),
    reserved("hub.revoke"),
    reserved("backup.restore"),
];

pub fn owner_command_kind(kind: &str) -> Option<&'static OwnerCommandKind> {
    OWNER_COMMAND_KINDS.iter().find(|k| k.kind == kind)
}

/// The payload of an owner `staff.upsert` (Story 8.26): who, where in the team and whether active.
/// It never carries a credential: the cloud adds `setup_code_hash` and `setup_expires_at` for a new
/// member (or when `setup_code` asks for a new code) and the Hub keeps the member's own password
/// and PIN. Without `staff_id` it adds a member at the branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerStaffUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub staff_id: Option<String>,
    /// The member's mobile number with the country code: the login the setup code is for.
    pub login: String,
    pub display_name: String,
    /// A role id of the branch, or the name of a built-in role.
    pub role_id: String,
    pub active: bool,
    /// Issue a new setup code for a member who has not chosen a password yet. A new member always
    /// gets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub setup_code: Option<bool>,
}

/// The reasons an owner gives for revoking a device (the Hub's `device.revoke` list).
pub const DEVICE_REVOKE_REASONS: [ReasonCode; 4] = [
    ReasonCode::Lost,
    ReasonCode::Stolen,
    ReasonCode::Replaced,
    ReasonCode::Other,
];

/// The payload of an owner `device.revoke` (Story 8.27): the device and why. The Hub applies its
/// own `device.revoke` intent with `reason`, so the reason rides beside the device id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OwnerDeviceRevoke {
    pub device_id: String,
    pub reason: ReasonCode,
}

/// The payload of an owner `credit.remind` (Story 8.32): the player to remind of what they owe. The
/// Hub turns it into a `notify.requested` of kind `credit_reminder` and puts what the player owes in
/// the message itself, so the amount is never sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditRemind {
    pub player_id: String,
}

/// One setup code the cloud issued for a new staff member of a branch. It is shown to the owner
/// once, in the answer to the post, and kept nowhere else (only its hash goes to the Hub).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffSetupCode {
    pub branch_id: String,
    pub staff_id: String,
    pub code: String,
}

impl std::fmt::Debug for StaffSetupCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaffSetupCode")
            .field("branch_id", &self.branch_id)
            .field("staff_id", &self.staff_id)
            .field("code", &"[redacted]")
            .finish()
    }
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
        "settings.set" => return validate_alert_limits(fields),
        "staff.upsert" => parses::<OwnerStaffUpsert>(&fields),
        "staff.deactivate" => parses::<StaffDeactivate>(&fields),
        "group.upsert" => parses::<GroupUpsert>(&fields),
        "device.assign" => parses::<DeviceAssign>(&fields),
        "credit.remind" => serde_json::from_value::<CreditRemind>(fields)
            .is_ok_and(|c| !c.player_id.trim().is_empty()),
        "device.revoke" => serde_json::from_value::<OwnerDeviceRevoke>(fields)
            .is_ok_and(|d| DEVICE_REVOKE_REASONS.contains(&d.reason)),
        "role.upsert" => match serde_json::from_value::<RoleUpsert>(fields) {
            // Only permissions of the fixed catalogue go to a Hub.
            Ok(role) => {
                if let Some(i) = role
                    .perms
                    .iter()
                    .position(|p| Permission::parse(&p.permission).is_none())
                {
                    return Err(invalid(
                        &format!("perms[{i}]"),
                        "That permission is not in the catalogue.",
                    ));
                }
                true
            }
            Err(_) => false,
        },
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

/// An owner `settings.set` sets the alert limits and nothing else: the key must be
/// [`KEY_OWNER_ALERT_THRESHOLDS`] and the value a valid [`OwnerAlertThresholds`] JSON.
fn validate_alert_limits(fields: serde_json::Value) -> Result<(), AppError> {
    let Ok(set) = serde_json::from_value::<SettingsSet>(fields) else {
        return Err(invalid(
            "payload",
            "The payload does not match this kind of change.",
        ));
    };
    if set.key != KEY_OWNER_ALERT_THRESHOLDS {
        return Err(invalid(
            "key",
            "Only the alert limits can be set from here.",
        ));
    }
    if OwnerAlertThresholds::parse(&set.value).is_none() {
        return Err(invalid(
            "value",
            "Enter amounts from 0 and a percent from 1 to 100.",
        ));
    }
    Ok(())
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

impl OwnerUpdate {
    /// Whether an owner command of `kind` raises this update: its own kind, and a deactivated
    /// staff member counts as a staff update (Story 8.26).
    pub fn covers(&self, kind: &str) -> bool {
        kind == self.kind || (self.kind == "staff.upsert" && kind == "staff.deactivate")
    }
}

pub const OWNER_UPDATES: [OwnerUpdate; 7] = [
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
    OwnerUpdate {
        kind: "staff.upsert",
        entity: "staff",
        tbl: "staff",
        copy_key: "hub.owner_update.staff",
    },
    OwnerUpdate {
        kind: "role.upsert",
        entity: "role",
        tbl: "roles",
        copy_key: "hub.owner_update.role",
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
    /// The setup codes of the staff members this post added, one per branch (Story 8.26): shown
    /// to the owner once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub setup_codes: Option<Vec<StaffSetupCode>>,
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
    fn staff_and_role_kinds_are_handled_and_sensitive() {
        for k in ["staff.upsert", "staff.deactivate", "role.upsert"] {
            let kind = accepted_owner_kind(k).unwrap();
            assert!(kind.sensitive && kind.handled, "{k}");
        }
    }

    #[test]
    fn a_staff_payload_never_carries_a_credential_and_a_role_only_catalogue_permissions() {
        let member = json!({"login": "+919876500001", "display_name": "Neha", "role_id": "Counter", "active": true});
        assert!(validate_owner_payload("staff.upsert", &member).is_ok());
        let mut with_code = member.clone();
        with_code["setup_code"] = true.into();
        assert!(validate_owner_payload("staff.upsert", &with_code).is_ok());
        for field in ["password_hash", "pin_hash", "setup_code_hash"] {
            let mut bad = member.clone();
            bad[field] = "x".into();
            assert!(
                validate_owner_payload("staff.upsert", &bad).is_err(),
                "{field}"
            );
        }
        assert!(validate_owner_payload("staff.deactivate", &json!({"staff_id": "S1"})).is_ok());
        let role = |perm: &str| json!({"name": "Lead", "perms": [{"permission": perm, "effect": "allow"}]});
        assert!(validate_owner_payload("role.upsert", &role("session.start")).is_ok());
        let e = validate_owner_payload("role.upsert", &role("session.destroy")).unwrap_err();
        assert_eq!(e.field.as_deref(), Some("perms[0]"));
        assert!(validate_owner_payload("role.upsert", &json!({"perms": []})).is_err());
    }

    #[test]
    fn group_and_assign_are_plain_and_a_device_revoke_is_sensitive_with_a_reason() {
        for k in ["group.upsert", "device.assign"] {
            let kind = accepted_owner_kind(k).unwrap();
            assert!(!kind.sensitive && kind.handled, "{k}");
        }
        let kind = accepted_owner_kind("device.revoke").unwrap();
        assert!(kind.sensitive && kind.handled);
        // The Hub itself is revoked only by Epic 9.
        assert!(owner_command_kind("hub.revoke").is_some_and(|k| !k.handled));

        let group = json!({"name": "Row A", "group_kind": "pc", "colour": "teal", "sort_order": 1});
        assert!(validate_owner_payload("group.upsert", &group).is_ok());
        assert!(validate_owner_payload("group.upsert", &json!({"name": "Row A"})).is_err());
        assert!(
            validate_owner_payload(
                "device.assign",
                &json!({"device_id": "D", "group_id": null})
            )
            .is_ok()
        );
        assert!(validate_owner_payload("device.assign", &json!({"group_id": "G"})).is_err());

        let revoke = |reason: &str| json!({"device_id": "D", "reason": reason});
        for reason in ["lost", "stolen", "replaced", "other"] {
            assert!(
                validate_owner_payload("device.revoke", &revoke(reason)).is_ok(),
                "{reason}"
            );
        }
        for bad in [revoke("closing"), revoke("nope"), json!({"device_id": "D"})] {
            assert!(
                validate_owner_payload("device.revoke", &bad).is_err(),
                "{bad}"
            );
        }
        let mut extra = revoke("lost");
        extra["x"] = 1.into();
        assert!(validate_owner_payload("device.revoke", &extra).is_err());
    }

    #[test]
    fn a_credit_reminder_is_a_plain_handled_kind_that_names_only_the_player() {
        let kind = accepted_owner_kind("credit.remind").unwrap();
        assert!(!kind.sensitive && kind.handled);
        assert_eq!(kind.min_tier, Tier::Basic);
        assert_eq!(OWNER_COMMAND_KINDS.len(), 17);
        assert!(validate_owner_payload("credit.remind", &json!({"player_id": "P1"})).is_ok());
        for bad in [
            json!({}),
            json!({"player_id": ""}),
            json!({"player_id": " "}),
            json!({"player_id": 5}),
            json!({"player_id": "P1", "phone": "+919876543210"}),
            json!({"player_id": "P1", "amount_minor": "5"}),
        ] {
            assert!(
                validate_owner_payload("credit.remind", &bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_owner_settings_set_takes_only_the_alert_limits() {
        let kind = accepted_owner_kind("settings.set").unwrap();
        assert!(!kind.sensitive && kind.handled);
        assert_eq!(kind.min_tier, Tier::Basic);
        let limits = crate::OwnerAlertThresholds::default().to_json();
        let ok = json!({"key": "owner_alert.thresholds", "value": limits, "base_rev": 2});
        assert!(validate_owner_payload("settings.set", &ok).is_ok());
        let field = |p: serde_json::Value| {
            validate_owner_payload("settings.set", &p)
                .unwrap_err()
                .field
        };
        assert_eq!(
            field(json!({"key": "branch.name", "value": "X"})).as_deref(),
            Some("key")
        );
        assert_eq!(
            field(json!({"key": "owner_alert.thresholds", "value": "{}"})).as_deref(),
            Some("value")
        );
        assert_eq!(
            field(json!({"key": "owner_alert.thresholds"})).as_deref(),
            Some("payload")
        );
    }

    #[test]
    fn a_setup_code_never_prints() {
        let code = StaffSetupCode {
            branch_id: "b".into(),
            staff_id: "s".into(),
            code: "K7Q49MZD".into(),
        };
        assert!(!format!("{code:?}").contains("K7Q49MZD"));
    }

    #[test]
    fn a_deactivation_toasts_as_a_staff_update() {
        let staff = OWNER_UPDATES.iter().find(|u| u.tbl == "staff").unwrap();
        assert!(staff.covers("staff.upsert") && staff.covers("staff.deactivate"));
        assert!(!staff.covers("role.upsert"));
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

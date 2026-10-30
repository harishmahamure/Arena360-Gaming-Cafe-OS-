use arena_proto::{
    Actor, ActorKind, AppError, COMMAND_SCHEMA_VERSION, CommandWire, ElevatedBy, ElevationKind,
    ElevationMode, ErrorCode, Intent, IntentPayload, KEY_SETUP_LIVE, NOTIFY_KIND_CREDIT_REMINDER,
    NotifyRequested, ReasonCode, SETTINGS_COMMAND_KEYS,
};

use arena_store::{
    StoreTx, find_coupon, find_device, find_device_meta, find_plan, find_pricing_rule,
    find_product, find_role, find_setting, find_staff, newest_change_of,
};

use crate::apply::is_ulid;

/// The `image` an owner `product.upsert` carries when the owner sent none (the summary holds no
/// images, so the Owner app cannot send the stored one back): `owner_wins` swaps it for the
/// product's stored image. It is not a JPEG data URL, so it can never be stored by mistake.
const IMAGE_KEEP: &str = "keep";

/// The command kind that runs outside `apply()`: an upload and summary cycle (Story 8.19).
pub const BACKUP_NOW: &str = "backup.now";

/// The command kind that asks for a credit reminder to a player (Story 8.32).
pub const CREDIT_REMIND: &str = "credit.remind";

/// Every command kind this Hub runs, at [`COMMAND_SCHEMA_VERSION`]: the setup kinds (Story 2.15)
/// the owner kinds (Story 8.19), the device revoke (Story 8.27) and the credit reminder (Story
/// 8.32). `arena_proto::OWNER_COMMAND_KINDS` lists the same owner kinds as handled; a test keeps the
/// two in agreement.
pub const HANDLED_COMMAND_KINDS: [&str; 18] = [
    "group.upsert",
    "device.assign",
    "device.register",
    "staff.upsert",
    "settings.set",
    "setup.test_session",
    "branch.go_live",
    "rate.upsert",
    "plan.upsert",
    "rule.upsert",
    "coupon.upsert",
    "product.upsert",
    "stock.threshold",
    "staff.deactivate",
    "role.upsert",
    "device.revoke",
    CREDIT_REMIND,
    BACKUP_NOW,
];

fn refused(code: ErrorCode, field: &str, message: &str) -> AppError {
    AppError::with_field(code, field, message)
}

fn unsupported() -> AppError {
    refused(
        ErrorCode::Unsupported,
        "kind",
        "this hub does not run that command",
    )
}

fn handled(cmd: &CommandWire) -> Result<(), AppError> {
    if !HANDLED_COMMAND_KINDS.contains(&cmd.kind.as_str())
        || cmd.schema_version != COMMAND_SCHEMA_VERSION
    {
        return Err(unsupported());
    }
    if !is_ulid(&cmd.idempotency_key) {
        return Err(refused(
            ErrorCode::BadFrame,
            "idempotency_key",
            "idempotency_key must be a ULID",
        ));
    }
    Ok(())
}

/// Checks a `backup.now` command: it carries no fields (a `base_rev` is ignored).
pub fn backup_now_check(cmd: &CommandWire) -> Result<(), AppError> {
    handled(cmd)?;
    if cmd.kind != BACKUP_NOW {
        return Err(unsupported());
    }
    match cmd.payload.as_object() {
        Some(fields) if fields.keys().all(|k| k == "base_rev") => Ok(()),
        _ => Err(refused(
            ErrorCode::ValidationFailed,
            "payload",
            "backup.now takes no fields",
        )),
    }
}

pub fn command_intent(cmd: &CommandWire, now: i64) -> Result<Intent, AppError> {
    handled(cmd)?;
    if cmd.kind == BACKUP_NOW {
        return Err(unsupported());
    }
    let Some(fields) = cmd.payload.as_object() else {
        return Err(refused(
            ErrorCode::ValidationFailed,
            "payload",
            "the payload must be an object",
        ));
    };
    if cmd.kind == CREDIT_REMIND {
        return credit_remind_intent(cmd, fields, now);
    }
    let mut fields = fields.clone();
    let base_rev = match fields.remove("base_rev") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| {
            refused(
                ErrorCode::ValidationFailed,
                "base_rev",
                "base_rev must be a number",
            )
        })?),
    };
    let kind = match cmd.kind.as_str() {
        "setup.test_session" => "device.test_session.start",
        "branch.go_live" => "settings.set",
        other => other,
    };
    fields.insert("kind".into(), kind.into());
    // A revoke carries its reason beside the device id; the intent holds it apart from the payload.
    let reason = if cmd.kind == "device.revoke" {
        let reason = fields
            .remove("reason")
            .and_then(|r| serde_json::from_value::<ReasonCode>(r).ok())
            .filter(|r| arena_proto::DEVICE_REVOKE_REASONS.contains(r));
        Some(reason.ok_or_else(|| {
            refused(
                ErrorCode::ReasonRequired,
                "reason",
                "choose a reason for revoking the device",
            )
        })?)
    } else {
        None
    };
    if cmd.kind == "product.upsert" && !fields.contains_key("image") {
        fields.insert("image".into(), IMAGE_KEEP.into());
    }
    if cmd.kind == "branch.go_live" {
        fields.insert("key".into(), KEY_SETUP_LIVE.into());
        fields.insert("value".into(), "true".into());
    }
    let payload: IntentPayload = serde_json::from_value(serde_json::Value::Object(fields))
        .map_err(|_| {
            refused(
                ErrorCode::ValidationFailed,
                "payload",
                "the payload does not match the command",
            )
        })?;
    if let IntentPayload::SettingsSet(s) = &payload
        && cmd.kind != "branch.go_live"
        && !SETTINGS_COMMAND_KEYS.contains(&s.key.as_str())
    {
        return Err(refused(
            ErrorCode::ValidationFailed,
            "key",
            "only the branch name, timezone and alert limits are set by command",
        ));
    }
    Ok(Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: cmd.idempotency_key.clone(),
        issued_at: now,
        base_rev,
        actor: Actor {
            kind: ActorKind::Owner,
            id: cmd.issued_by.clone(),
        },
        // A step-up the cloud checked (Story 8.25): `ElevationMode` has no cloud value and `changes`
        // constrains it, so it is recorded as mode `hub`; the kind `mfa` marks it as a step-up.
        elevated_by: cmd.elevated_by.as_ref().map(|id| ElevatedBy {
            kind: ElevationKind::Mfa,
            id: id.clone(),
            mode: ElevationMode::Hub,
            verifier_version: 0,
        }),
        reason,
        payload,
    })
}

/// An owner `credit.remind`: the Hub's own `notify.requested` of kind `credit_reminder` for the
/// player, issued by the owner. The command carries only the player; the placeholder
/// `amount_minor` satisfies the request's parameter check and `notify_request::mutate` replaces it
/// with what the player owes when the row is queued. A `base_rev` is ignored: nothing is versioned.
fn credit_remind_intent(
    cmd: &CommandWire,
    fields: &serde_json::Map<String, serde_json::Value>,
    now: i64,
) -> Result<Intent, AppError> {
    let player_id = match (
        fields.get("player_id"),
        fields.keys().all(|k| k == "player_id" || k == "base_rev"),
    ) {
        (Some(serde_json::Value::String(id)), true) if !id.trim().is_empty() => id.clone(),
        _ => {
            return Err(refused(
                ErrorCode::ValidationFailed,
                "player_id",
                "credit.remind takes one player",
            ));
        }
    };
    let payload = IntentPayload::NotifyRequested(NotifyRequested {
        kind: NOTIFY_KIND_CREDIT_REMINDER.to_owned(),
        player_id,
        session_id: None,
        params: std::collections::BTreeMap::from([("amount_minor".to_owned(), "0".to_owned())]),
    });
    Ok(Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: cmd.idempotency_key.clone(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Owner,
            id: cmd.issued_by.clone(),
        },
        elevated_by: None,
        reason: None,
        payload,
    })
}

/// An owner `device.revoke` of a device that is already revoked (Story 8.27): reported `applied`
/// with nothing changed, so a repeat or a retry does not fail on the Hub's own refusal.
pub(crate) fn already_revoked(tx: &StoreTx<'_>, cmd: &CommandWire) -> Result<bool, AppError> {
    if cmd.kind != "device.revoke" {
        return Ok(false);
    }
    let Some(id) = cmd.payload.get("device_id").and_then(|v| v.as_str()) else {
        return Ok(false);
    };
    Ok(find_device(tx, id)?.is_some_and(|d| d.revoked_at.is_some()))
}

/// The change an owner command is about to overwrite: the newest change of the row the owner's
/// `base_rev` no longer matches (Story 8.19).
pub(crate) struct Overwrote {
    pub tbl: &'static str,
    pub row_id: String,
    pub intent_id: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub at: i64,
}

/// The row an owner edit of a versioned catalogue kind works on: its change-log table, id and
/// current `rev`. `None` for a create, a row that is not there and every other kind.
fn edited_row(
    tx: &StoreTx<'_>,
    payload: &IntentPayload,
) -> Result<Option<(&'static str, String, i64)>, AppError> {
    Ok(match payload {
        IntentPayload::PlanUpsert(p) => match &p.id {
            Some(id) => find_plan(tx, id)?.map(|r| ("plans", id.clone(), r.rev)),
            None => None,
        },
        IntentPayload::RuleUpsert(p) => match &p.id {
            Some(id) => find_pricing_rule(tx, id)?.map(|r| ("pricing_rules", id.clone(), r.rev)),
            None => None,
        },
        IntentPayload::ProductUpsert(p) => match &p.id {
            Some(id) => find_product(tx, id)?.map(|r| ("products", id.clone(), r.rev)),
            None => None,
        },
        IntentPayload::CouponUpsert(p) => match &p.id {
            Some(id) => find_coupon(tx, id)?.map(|r| ("coupons", id.clone(), r.rev)),
            None => None,
        },
        IntentPayload::StaffUpsert(p) => match &p.staff_id {
            Some(id) => find_staff(tx, id)?.map(|r| ("staff", id.clone(), r.rev)),
            None => None,
        },
        IntentPayload::RoleUpsert(p) => match &p.role_id {
            Some(id) => find_role(tx, id)?.map(|r| ("roles", id.clone(), r.rev)),
            None => None,
        },
        _ => None,
    })
}

/// Makes the owner's edit beat a newer row: `base_rev` becomes the row's current `rev` (a rate has
/// none and always applies). When the command carried a `base_rev` below that `rev`, answers the
/// change it is about to overwrite.
pub(crate) fn owner_wins(
    tx: &StoreTx<'_>,
    intent: &mut Intent,
) -> Result<Option<Overwrote>, AppError> {
    let mut overwrote = None;
    if let IntentPayload::ProductUpsert(p) = &mut intent.payload
        && p.image == IMAGE_KEEP
    {
        p.image = match &p.id {
            Some(id) => find_product(tx, id)?.map(|r| r.image).unwrap_or_default(),
            None => String::new(),
        };
    }
    let rev = match &intent.payload {
        IntentPayload::SettingsSet(s) => find_setting(tx, &s.key)?.map(|r| r.rev),
        IntentPayload::DeviceRegister(d) => match &d.device_id {
            Some(id) => find_device_meta(tx, id)?.map(|m| m.rev),
            None => None,
        },
        payload => match edited_row(tx, payload)? {
            Some((tbl, row_id, rev)) => {
                if intent.base_rev.is_some_and(|base| base < rev)
                    && let Some(c) = newest_change_of(tx, tbl, &row_id)?
                {
                    overwrote = Some(Overwrote {
                        tbl,
                        row_id,
                        intent_id: c.intent_id,
                        actor_kind: c.actor_kind,
                        actor_id: c.actor_id,
                        at: c.at,
                    });
                }
                Some(rev)
            }
            None => None,
        },
    };
    if rev.is_some() {
        intent.base_rev = rev;
    }
    Ok(overwrote)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cmd(kind: &str, payload: serde_json::Value) -> CommandWire {
        CommandWire {
            id: 1,
            idempotency_key: ulid::Ulid::generate().to_string(),
            kind: kind.into(),
            schema_version: 1,
            payload,
            issued_by: "owner-1".into(),
            elevated_by: None,
        }
    }

    #[test]
    fn a_group_command_becomes_an_owner_intent_with_its_base_rev() {
        let c = cmd(
            "group.upsert",
            json!({"name": "Row A", "group_kind": "pc", "colour": "teal", "sort_order": 1, "base_rev": 3}),
        );
        let i = command_intent(&c, 99).unwrap();
        assert_eq!(i.actor.kind, ActorKind::Owner);
        assert_eq!(i.actor.id, "owner-1");
        assert_eq!((i.base_rev, i.issued_at), (Some(3), 99));
        assert_eq!(i.idempotency_key, c.idempotency_key);
        assert!(matches!(i.payload, IntentPayload::GroupUpsert(_)));
    }

    #[test]
    fn the_hubs_handled_kinds_are_the_setup_kinds_and_the_catalogues_handled_kinds() {
        let mut ours: Vec<&str> = HANDLED_COMMAND_KINDS.to_vec();
        ours.sort_unstable();
        let mut theirs: Vec<&str> = arena_proto::SETUP_COMMAND_KINDS
            .iter()
            .copied()
            .chain(
                arena_proto::OWNER_COMMAND_KINDS
                    .iter()
                    .filter(|k| k.handled)
                    .map(|k| k.kind),
            )
            .collect();
        theirs.sort_unstable();
        // The wizard and the owner both send `staff.upsert`.
        theirs.dedup();
        assert_eq!(ours, theirs);
        for k in arena_proto::OWNER_COMMAND_KINDS
            .iter()
            .filter(|k| !k.handled)
        {
            assert!(!HANDLED_COMMAND_KINDS.contains(&k.kind), "{}", k.kind);
        }
    }

    #[test]
    fn the_owner_kinds_become_owner_intents_with_their_base_rev() {
        let cases = [
            (
                "rate.upsert",
                json!({"group_id": "g", "price_per_hour_minor": 5}),
            ),
            (
                "plan.upsert",
                json!({"plan_kind": "pack", "name": "x", "price_minor": 1, "group_ids": []}),
            ),
            (
                "rule.upsert",
                json!({"name": "x", "group_id": "g", "rule_kind": "weekly", "adjust_kind": "percent_bp", "adjust_value": 1}),
            ),
            ("coupon.upsert", json!({"code": "ABC", "percent_bp": 10})),
        ];
        for (kind, mut body) in cases {
            body["base_rev"] = 4.into();
            let i = command_intent(&cmd(kind, body), 5).unwrap();
            assert_eq!(i.kind.as_str(), kind);
            assert_eq!((i.actor.kind, i.base_rev), (ActorKind::Owner, Some(4)));
        }
        assert!(
            command_intent(&cmd("rate.upsert", json!({"group_id": "g"})), 1)
                .is_err_and(|e| e.code == ErrorCode::ValidationFailed)
        );
    }

    #[test]
    fn a_credit_remind_becomes_an_owner_credit_reminder_request() {
        let mut c = cmd("credit.remind", json!({"player_id": "P1", "base_rev": 4}));
        c.elevated_by = None;
        let i = command_intent(&c, 9).unwrap();
        assert_eq!(i.kind, arena_proto::IntentKind::NotifyRequested);
        assert_eq!(
            (i.actor.kind, i.base_rev, i.issued_at),
            (ActorKind::Owner, None, 9)
        );
        assert_eq!(i.idempotency_key, c.idempotency_key);
        let IntentPayload::NotifyRequested(n) = i.payload else {
            panic!("not a notify.requested")
        };
        assert_eq!(n.kind, "credit_reminder");
        assert_eq!((n.player_id.as_str(), n.session_id), ("P1", None));
        assert_eq!(n.params.get("amount_minor").map(String::as_str), Some("0"));
        for bad in [
            json!({}),
            json!({"player_id": ""}),
            json!({"player_id": 3}),
            json!({"player_id": "P1", "phone": "+91"}),
            json!([1]),
        ] {
            let e = command_intent(&cmd("credit.remind", bad), 9).unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
        }
    }

    #[test]
    fn backup_now_takes_no_fields_and_never_becomes_an_intent() {
        assert!(backup_now_check(&cmd(BACKUP_NOW, json!({}))).is_ok());
        assert!(backup_now_check(&cmd(BACKUP_NOW, json!({"base_rev": 3}))).is_ok());
        for bad in [json!({"x": 1}), json!([1]), json!(null)] {
            let e = backup_now_check(&cmd(BACKUP_NOW, bad)).unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
        }
        let mut newer = cmd(BACKUP_NOW, json!({}));
        newer.schema_version = 2;
        assert_eq!(
            backup_now_check(&newer).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            backup_now_check(&cmd("group.upsert", json!({})))
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            command_intent(&cmd(BACKUP_NOW, json!({})), 1)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn kinds_without_a_handler_or_outside_the_catalogue_are_unsupported() {
        for kind in [
            "pricing.set",
            "branch.bootstrap",
            "rule.delete",
            "hub.revoke",
        ] {
            let e = command_intent(&cmd(kind, json!({})), 1).unwrap_err();
            assert_eq!(e.code, ErrorCode::Unsupported, "{kind}");
        }
        let mut old = cmd("group.upsert", json!({}));
        old.schema_version = 2;
        assert_eq!(
            command_intent(&old, 1).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn bad_keys_payloads_and_settings_are_refused() {
        let mut c = cmd("group.upsert", json!({"name": "x"}));
        c.idempotency_key = "nope".into();
        assert_eq!(command_intent(&c, 1).unwrap_err().code, ErrorCode::BadFrame);
        for p in [json!([1]), json!({"name": 5}), json!({"base_rev": "x"})] {
            let e = command_intent(&cmd("group.upsert", p), 1).unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
        }
        let e = command_intent(
            &cmd(
                "settings.set",
                json!({"key": "branch.currency", "value": "USD"}),
            ),
            1,
        )
        .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("key"));
        assert!(
            command_intent(
                &cmd(
                    "settings.set",
                    json!({"key": "branch.tz", "value": "Asia/Kolkata", "base_rev": 1})
                ),
                1
            )
            .is_ok()
        );
    }

    #[test]
    fn go_live_becomes_a_settings_set_of_the_live_flag() {
        let i = command_intent(&cmd("branch.go_live", json!({"key": "branch.name"})), 7).unwrap();
        assert_eq!(i.kind, arena_proto::IntentKind::SettingsSet);
        let IntentPayload::SettingsSet(s) = i.payload else {
            panic!("not a settings.set")
        };
        assert_eq!((s.key.as_str(), s.value.as_str()), ("setup.live", "true"));
    }

    #[test]
    fn the_test_session_command_becomes_a_start_intent() {
        let i = command_intent(&cmd("setup.test_session", json!({"device_id": "D"})), 7).unwrap();
        assert_eq!(i.kind, arena_proto::IntentKind::DeviceTestSessionStart);
        assert_eq!(i.actor.kind, ActorKind::Owner);
        assert!(
            command_intent(&cmd("setup.test_session", json!({})), 7)
                .is_err_and(|e| e.code == ErrorCode::ValidationFailed)
        );
    }

    #[test]
    fn a_payload_cannot_swap_the_kind() {
        let c = cmd(
            "group.upsert",
            json!({"kind": "device.revoke", "name": "G", "group_kind": "pc", "colour": "teal", "sort_order": 0}),
        );
        let i = command_intent(&c, 1).unwrap();
        assert!(matches!(i.payload, IntentPayload::GroupUpsert(_)));
    }
}

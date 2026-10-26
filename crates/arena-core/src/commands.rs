use arena_proto::{
    Actor, ActorKind, AppError, COMMAND_SCHEMA_VERSION, CommandWire, ErrorCode, Intent,
    IntentPayload, KEY_SETUP_LIVE, SETTINGS_COMMAND_KEYS,
};

use arena_store::{StoreTx, find_device_meta, find_setting, find_staff};

use crate::apply::is_ulid;

const HANDLED: [&str; 7] = [
    "group.upsert",
    "device.assign",
    "device.register",
    "staff.upsert",
    "settings.set",
    "setup.test_session",
    "branch.go_live",
];

fn refused(code: ErrorCode, field: &str, message: &str) -> AppError {
    AppError::with_field(code, field, message)
}

pub fn command_intent(cmd: &CommandWire, now: i64) -> Result<Intent, AppError> {
    if !HANDLED.contains(&cmd.kind.as_str()) || cmd.schema_version != COMMAND_SCHEMA_VERSION {
        return Err(refused(
            ErrorCode::Unsupported,
            "kind",
            "this hub does not run that command",
        ));
    }
    if !is_ulid(&cmd.idempotency_key) {
        return Err(refused(
            ErrorCode::BadFrame,
            "idempotency_key",
            "idempotency_key must be a ULID",
        ));
    }
    let Some(fields) = cmd.payload.as_object() else {
        return Err(refused(
            ErrorCode::ValidationFailed,
            "payload",
            "the payload must be an object",
        ));
    };
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
            "only the branch name and timezone are set by command",
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
        elevated_by: None,
        reason: None,
        payload,
    })
}

pub(crate) fn owner_wins(tx: &StoreTx<'_>, intent: &mut Intent) -> Result<(), AppError> {
    let rev = match &intent.payload {
        IntentPayload::SettingsSet(s) => find_setting(tx, &s.key)?.map(|r| r.rev),
        IntentPayload::DeviceRegister(d) => match &d.device_id {
            Some(id) => find_device_meta(tx, id)?.map(|m| m.rev),
            None => None,
        },
        IntentPayload::StaffUpsert(s) => match &s.staff_id {
            Some(id) => find_staff(tx, id)?.map(|r| r.rev),
            None => None,
        },
        _ => None,
    };
    if rev.is_some() {
        intent.base_rev = rev;
    }
    Ok(())
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
    fn kinds_without_a_handler_or_outside_the_catalogue_are_unsupported() {
        for kind in ["pricing.set", "branch.bootstrap"] {
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

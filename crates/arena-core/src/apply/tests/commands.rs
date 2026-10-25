use super::*;
use arena_proto::CommandWire;
use serde_json::json;

fn cmd(id: i64, kind: &str, payload: serde_json::Value) -> CommandWire {
    CommandWire {
        id,
        idempotency_key: ulid::Ulid::generate().to_string(),
        kind: kind.into(),
        schema_version: 1,
        payload,
        issued_by: "owner-1".into(),
    }
}

fn group(id: i64, name: &str) -> CommandWire {
    cmd(
        id,
        "group.upsert",
        json!({"name": name, "group_kind": "pc", "colour": "teal", "sort_order": 1}),
    )
}

fn groups(f: &Fixture) -> usize {
    reopen(f).rows(Table::DeviceGroups).unwrap().len()
}

#[tokio::test]
async fn a_command_applies_once_as_the_owner_and_a_repeat_returns_the_stored_row() {
    let mut f = bootstrapped().await;
    let c = group(7, "Row A");
    let first = f.writer.apply_command(c.clone()).await.unwrap();
    assert!(first.applied && first.error.is_none());
    assert_eq!((first.command_id, first.kind.as_str()), (7, "group.upsert"));
    assert!(first.applied_seq.is_some());
    assert_eq!(groups(&f), 1);
    let applied = f.events.try_recv().unwrap();
    assert_eq!(applied.changes.last().map(|c| c.seq), first.applied_seq);
    let change = reopen(&f)
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| Some(c.seq) == first.applied_seq)
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("owner", "owner-1")
    );
    let again = f.writer.apply_command(c.clone()).await.unwrap();
    assert_eq!(again.applied_seq, first.applied_seq);
    let other_id = CommandWire { id: 8, ..c };
    let by_key = f.writer.apply_command(other_id).await.unwrap();
    assert_eq!(by_key.command_id, 7);
    assert_eq!(groups(&f), 1);
    assert!(f.events.try_recv().is_err());
}

#[tokio::test]
async fn a_rejected_command_writes_nothing_but_its_record_and_is_not_retried() {
    let mut f = bootstrapped().await;
    let before = reopen(&f).changes().unwrap().len();
    let c = cmd(
        3,
        "device.assign",
        json!({"device_id": "01JNOSUCHDEVICE", "group_id": null}),
    );
    let row = f.writer.apply_command(c.clone()).await.unwrap();
    assert!(!row.applied && row.applied_seq.is_none());
    let err: AppError = serde_json::from_str(row.error.as_deref().unwrap()).unwrap();
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(f.events.try_recv().is_err());
    let again = f.writer.apply_command(c).await.unwrap();
    assert!(!again.applied);
    assert_eq!(f.writer.command_cursor().await.unwrap(), 3);
}

#[tokio::test]
async fn commands_the_hub_cannot_run_are_recorded_as_unsupported() {
    let f = bootstrapped().await;
    for (id, kind) in [
        (1, "branch.go_live"),
        (2, "setup.test_session"),
        (3, "pricing.set"),
    ] {
        let row = f
            .writer
            .apply_command(cmd(id, kind, json!({})))
            .await
            .unwrap();
        let err: AppError = serde_json::from_str(row.error.as_deref().unwrap()).unwrap();
        assert_eq!(
            (row.applied, err.code),
            (false, ErrorCode::Unsupported),
            "{kind}"
        );
    }
}

#[tokio::test]
async fn the_owner_wins_over_a_stale_base_rev() {
    let f = bootstrapped().await;
    let rev = match setting(&reopen(&f), "branch.tz")["rev"] {
        RowValue::Integer(r) => r,
        ref other => panic!("{other:?}"),
    };
    let stale = cmd(
        1,
        "settings.set",
        json!({"key": "branch.tz", "value": "Asia/Dubai", "base_rev": rev + 40}),
    );
    let row = f.writer.apply_command(stale).await.unwrap();
    assert!(row.applied, "{:?}", row.error);
    assert_eq!(
        setting(&reopen(&f), "branch.tz")["value"],
        RowValue::Text("Asia/Dubai".into())
    );
    let none = cmd(
        2,
        "settings.set",
        json!({"key": "branch.tz", "value": "Asia/Kolkata"}),
    );
    assert!(f.writer.apply_command(none).await.unwrap().applied);
    let other_key = cmd(
        3,
        "settings.set",
        json!({"key": "branch.currency", "value": "USD"}),
    );
    assert!(!f.writer.apply_command(other_key).await.unwrap().applied);
}

#[tokio::test]
async fn not_primary_records_nothing_so_the_next_poll_retries() {
    let f = bootstrapped().await;
    f.writer.close().await;
    let e = f.writer.apply_command(group(1, "Row A")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotPrimary);
    let max = reopen(&f)
        .write_tx(|tx| arena_store::max_applied_command_id(tx))
        .unwrap();
    assert_eq!(max, 0);
}

#[tokio::test]
async fn results_are_reported_in_order_until_marked() {
    let f = bootstrapped().await;
    f.writer.apply_command(group(5, "A")).await.unwrap();
    f.writer.apply_command(group(6, "B")).await.unwrap();
    let ids = |rows: Vec<arena_store::AppliedCommand>| {
        rows.into_iter().map(|r| r.command_id).collect::<Vec<_>>()
    };
    assert_eq!(ids(f.writer.unreported_commands(10).await.unwrap()), [5, 6]);
    f.writer.mark_command_reported(5).await.unwrap();
    assert_eq!(ids(f.writer.unreported_commands(10).await.unwrap()), [6]);
    assert_eq!(f.writer.command_cursor().await.unwrap(), 6);
}

fn text(row: &std::collections::BTreeMap<String, RowValue>, k: &str) -> String {
    match &row[k] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{k} is {other:?}"),
    }
}

#[tokio::test]
async fn owner_staff_and_device_edits_win_over_a_stale_base_rev() {
    let f = bootstrapped().await;
    let db = reopen(&f);
    let role = db
        .rows(Table::Roles)
        .unwrap()
        .iter()
        .map(|r| text(r, "role_id"))
        .next()
        .unwrap();
    let pw = crate::staff::hash_secret("pw").unwrap();
    let staff = |id: i64, extra: serde_json::Value| {
        let mut p = json!({"login": "asha", "display_name": "Asha", "role_id": role, "password_hash": pw, "active": true});
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        cmd(id, "staff.upsert", p)
    };
    assert!(
        f.writer
            .apply_command(staff(1, json!({})))
            .await
            .unwrap()
            .applied
    );
    let staff_id = text(&reopen(&f).rows(Table::Staff).unwrap()[0], "staff_id");
    let edit = staff(
        2,
        json!({"staff_id": staff_id, "display_name": "Asha K", "base_rev": 99}),
    );
    let row = f.writer.apply_command(edit).await.unwrap();
    assert!(row.applied, "{:?}", row.error);
    assert_eq!(
        text(&reopen(&f).rows(Table::Staff).unwrap()[0], "display_name"),
        "Asha K"
    );

    let secret = crate::devices::new_token_secret().unwrap();
    let hash = crate::devices::hash_token_secret(&secret).unwrap();
    let register = |id: i64, extra: serde_json::Value| {
        let mut p = json!({"class": "paired", "label": "Kiosk 1", "token_hash": hash});
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        cmd(id, "device.register", p)
    };
    let row = f
        .writer
        .apply_command(register(3, json!({})))
        .await
        .unwrap();
    assert!(row.applied, "{:?}", row.error);
    let device_id = text(&reopen(&f).rows(Table::Devices).unwrap()[0], "device_id");
    let edit = register(
        4,
        json!({"device_id": device_id, "label": "Kiosk 2", "token_hash": "", "base_rev": 99}),
    );
    let row = f.writer.apply_command(edit).await.unwrap();
    assert!(row.applied, "{:?}", row.error);
    assert_eq!(
        text(&reopen(&f).rows(Table::Devices).unwrap()[0], "label"),
        "Kiosk 2"
    );

    let assign = cmd(
        5,
        "device.assign",
        json!({"device_id": device_id, "group_id": null}),
    );
    let row = f.writer.apply_command(assign).await.unwrap();
    assert!(row.applied, "{:?}", row.error);
    let changes = reopen(&f).changes().unwrap();
    assert!(changes.iter().filter(|c| c.actor_kind == "owner").count() >= 5);
}

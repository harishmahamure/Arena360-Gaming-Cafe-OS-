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

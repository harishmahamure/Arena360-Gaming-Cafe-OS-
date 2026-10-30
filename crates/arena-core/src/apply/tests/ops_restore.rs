//! Story 9.7: the `ops.restore` record and `backup.last_restore`.

use super::session::{refusal, text};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER, ROLE_OWNER};
use arena_proto::{ExportHeader, OpsRestore, RESTORE_SYSTEM_ACTOR};

const KEY: &str = "backup.last_restore";

fn branch_id(f: &Fixture) -> String {
    text(&setting(&reopen(f), KEY), "branch_id")
}

fn facts(f: &Fixture) -> OpsRestore {
    OpsRestore {
        file_name: "DEV01-2027-01-15-1330.a360".into(),
        header: ExportHeader {
            format_version: arena_proto::EXPORT_FORMAT_VERSION,
            branch_id: branch_id(f),
            branch_code: "DEV01".into(),
            epoch: "3:abc".into(),
            last_seq: 120,
            schema_version: 84,
            exported_at: T0 - 60_000,
            sha256: "ab".repeat(32),
        },
        old_epoch: "4:abc".into(),
        new_epoch: "5:abc".into(),
        rows_set_aside: 31,
        erasures_reapplied: 2,
        orphaned_file: "4-abc-20270115T133000.db".into(),
        restored_at: T0,
        by: None,
    }
}

fn record(actor: Actor, facts: OpsRestore) -> Intent {
    intent(actor, IntentPayload::OpsRestore(facts))
}

fn stored(f: &Fixture) -> String {
    text(&setting(&reopen(f), KEY), "value")
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

#[tokio::test]
async fn the_setting_starts_empty_and_is_not_set_by_hand() {
    let f = bootstrapped().await;
    assert_eq!(stored(&f), "");
    let rev = match setting(&reopen(&f), KEY)["rev"] {
        RowValue::Integer(n) => n,
        ref other => panic!("{other:?}"),
    };
    let before = changes(&f);
    let mut set = intent(
        owner(),
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: KEY.into(),
            value: "{}".into(),
        }),
    );
    set.base_rev = Some(rev);
    let (code, field) = refusal(f.writer.submit_intent(set).await);
    assert_eq!(code, ErrorCode::ValidationFailed);
    assert_eq!(field.as_deref(), Some("key"));
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn the_owner_records_a_restore_and_the_change_row_is_the_audit_entry() {
    let f = bootstrapped().await;
    let who = staff(&f, "owner-direct", ROLE_OWNER).await;
    let before = changes(&f);
    let applied = f
        .writer
        .submit_intent(record(who.clone(), facts(&f)))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "settings");
    assert!(applied.changes[0].row_id.ends_with("/backup.last_restore"));
    let kept = OpsRestore::parse(&stored(&f)).unwrap();
    assert_eq!(
        kept,
        OpsRestore {
            by: Some("owner-direct".into()),
            ..facts(&f)
        }
    );
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].actor_id, who.id);
    assert_eq!(rows[before].intent_kind, "ops.restore");
}

#[tokio::test]
async fn only_the_owner_may_record_a_restore() {
    let f = bootstrapped().await;
    let mut people = Vec::new();
    for role in [ROLE_MANAGER, ROLE_COUNTER] {
        people.push((
            role,
            staff(&f, &format!("x-{}", role.to_lowercase()), role).await,
        ));
    }
    let before = changes(&f);
    for (role, who) in people {
        let (code, _) = refusal(f.writer.submit_intent(record(who, facts(&f))).await);
        assert_eq!(code, ErrorCode::PermissionDenied, "{role}");
    }
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn the_hub_records_it_as_itself_only_under_the_restore_actor() {
    let f = bootstrapped().await;
    let hub = Actor {
        kind: ActorKind::System,
        id: RESTORE_SYSTEM_ACTOR.into(),
    };
    let other = Actor {
        kind: ActorKind::System,
        id: "notify".into(),
    };
    let (code, _) = refusal(f.writer.submit_intent(record(other, facts(&f))).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    // The owner's cloud actor is not a staff member who confirmed on this PC.
    let (code, field) = refusal(f.writer.submit_intent(record(owner(), facts(&f))).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("actor"))
    );
    f.writer
        .submit_intent(record(hub, facts(&f)))
        .await
        .unwrap();
    let kept = OpsRestore::parse(&stored(&f)).unwrap();
    assert_eq!(kept.by, None);
}

#[tokio::test]
async fn a_file_of_another_branch_or_implausible_facts_are_refused() {
    let f = bootstrapped().await;
    let who = staff(&f, "owner-direct", ROLE_OWNER).await;
    let before = changes(&f);
    let mut other = facts(&f);
    other.header.branch_id = "01JOTHER".into();
    let mut path = facts(&f);
    path.file_name = "../x.a360".into();
    let mut neg = facts(&f);
    neg.rows_set_aside = -1;
    for bad in [other, path, neg] {
        let (code, field) = refusal(f.writer.submit_intent(record(who.clone(), bad)).await);
        assert_eq!(
            (code, field.as_deref()),
            (ErrorCode::ValidationFailed, Some("header"))
        );
    }
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn recording_the_same_restore_twice_applies_it_once() {
    let f = bootstrapped().await;
    let who = staff(&f, "owner-direct", ROLE_OWNER).await;
    let first = record(who, facts(&f));
    f.writer.submit_intent(first.clone()).await.unwrap();
    let before = changes(&f);
    let again = f.writer.submit_intent(first).await;
    assert!(again.is_ok(), "{again:?}");
    assert_eq!(changes(&f), before, "the repeat writes nothing");
}

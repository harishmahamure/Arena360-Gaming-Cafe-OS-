//! Story 9.21: the `lease.promote` record and `ops.last_promote`.

use super::session::{refusal, text};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::LeasePromote;

const KEY: &str = "ops.last_promote";

fn facts() -> LeasePromote {
    LeasePromote {
        old_epoch: "3:01JMAIN".into(),
        fork_seq: 120,
        new_epoch: "4:01JSTBY".into(),
        provisional: true,
        lag_rows: 2,
        force: false,
        erasures_reapplied: 1,
        kitchen_displays_to_repair: 2,
        promoted_at: T0,
        by: None,
    }
}

fn record(actor: Actor, facts: LeasePromote) -> Intent {
    intent(actor, IntentPayload::LeasePromote(facts))
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
async fn a_manager_records_a_promote_and_the_change_row_is_the_audit_entry() {
    let f = bootstrapped().await;
    let who = staff(&f, "manager-asha", ROLE_MANAGER).await;
    let before = changes(&f);
    let applied = f
        .writer
        .submit_intent(record(who.clone(), facts()))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "settings");
    assert!(applied.changes[0].row_id.ends_with("/ops.last_promote"));
    let kept = LeasePromote::parse(&stored(&f)).unwrap();
    assert_eq!(
        kept,
        LeasePromote {
            by: Some("manager-asha".into()),
            ..facts()
        }
    );
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].actor_id, who.id);
    assert_eq!(rows[before].intent_kind, "lease.promote");
}

#[tokio::test]
async fn the_owner_may_record_one_and_a_counter_may_not() {
    let f = bootstrapped().await;
    let counter = staff(&f, "x-counter", ROLE_COUNTER).await;
    let before = changes(&f);
    let (code, _) = refusal(f.writer.submit_intent(record(counter, facts())).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
    let owner_staff = staff(&f, "owner-direct", crate::rbac::ROLE_OWNER).await;
    f.writer
        .submit_intent(record(owner_staff, facts()))
        .await
        .unwrap();
    assert!(LeasePromote::parse(&stored(&f)).is_some());
}

#[tokio::test]
async fn inconsistent_facts_are_refused() {
    let f = bootstrapped().await;
    let who = staff(&f, "manager-asha", ROLE_MANAGER).await;
    let before = changes(&f);
    for bad in [
        LeasePromote {
            new_epoch: String::new(),
            ..facts()
        },
        LeasePromote {
            new_epoch: "3:01JMAIN".into(),
            ..facts()
        },
        LeasePromote {
            lag_rows: -1,
            ..facts()
        },
    ] {
        let (code, field) = refusal(f.writer.submit_intent(record(who.clone(), bad)).await);
        assert_eq!(
            (code, field.as_deref()),
            (ErrorCode::ValidationFailed, Some("promote"))
        );
    }
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn the_cloud_actor_does_not_record_a_promote() {
    let f = bootstrapped().await;
    let (code, field) = refusal(f.writer.submit_intent(record(owner(), facts())).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("actor"))
    );
}

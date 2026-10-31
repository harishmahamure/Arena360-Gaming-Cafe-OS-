//! Story 9.23: the `ops.orphan_sweep` record and `ops.last_orphan_sweep`.

use super::session::{refusal, text};
use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::ROLE_MANAGER;
use arena_proto::{ORPHAN_SWEEP_SYSTEM_ACTOR, OrphanSweep, OrphanSweepFile, OrphanSweepInvoice};

const KEY: &str = "ops.last_orphan_sweep";

fn facts() -> OrphanSweep {
    OrphanSweep {
        files: vec![OrphanSweepFile {
            file: "3-01JOLD-20270115T133000.db".into(),
            set_aside_changes: Some(12),
            invoices: vec![
                OrphanSweepInvoice {
                    invoice_no: "INV/26-27/0042".into(),
                    amount_minor: 11_800,
                },
                OrphanSweepInvoice {
                    invoice_no: "INV/26-27/0043".into(),
                    amount_minor: 500,
                },
            ],
        }],
        swept_at: T0,
    }
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: ORPHAN_SWEEP_SYSTEM_ACTOR.into(),
    }
}

fn stored(f: &Fixture) -> String {
    text(&setting(&reopen(f), KEY), "value")
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

#[tokio::test]
async fn the_hub_records_a_sweep_and_the_change_row_lists_the_invoices() {
    let f = bootstrapped().await;
    assert_eq!(stored(&f), "");
    let before = changes(&f);
    let applied = f
        .writer
        .submit_intent(intent(system(), IntentPayload::OpsOrphanSweep(facts())))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert!(
        applied.changes[0]
            .row_id
            .ends_with("/ops.last_orphan_sweep")
    );
    assert_eq!(OrphanSweep::parse(&stored(&f)), Some(facts()));
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].intent_kind, "ops.orphan_sweep");
    assert_eq!(rows[before].actor_kind, "system");
}

#[tokio::test]
async fn a_person_cannot_record_a_sweep_and_another_system_actor_cannot_either() {
    let f = bootstrapped().await;
    let manager = staff(&f, "manager-asha", ROLE_MANAGER).await;
    let before = changes(&f);
    for actor in [
        manager,
        owner(),
        Actor {
            kind: ActorKind::System,
            id: "someone-else".into(),
        },
    ] {
        let (code, _) = refusal(
            f.writer
                .submit_intent(intent(actor, IntentPayload::OpsOrphanSweep(facts())))
                .await,
        );
        assert!(
            matches!(
                code,
                ErrorCode::PermissionDenied | ErrorCode::ValidationFailed
            ),
            "{code:?}"
        );
    }
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn an_inconsistent_sweep_is_refused() {
    let f = bootstrapped().await;
    let before = changes(&f);
    let mut empty = facts();
    empty.files.clear();
    let mut path = facts();
    path.files[0].file = "orphaned/x.db".into();
    for bad in [empty, path] {
        let (code, field) = refusal(
            f.writer
                .submit_intent(intent(system(), IntentPayload::OpsOrphanSweep(bad)))
                .await,
        );
        assert_eq!(code, ErrorCode::ValidationFailed);
        assert_eq!(field.as_deref(), Some("sweep"));
    }
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn the_setting_is_not_set_by_hand() {
    let f = bootstrapped().await;
    let rev = match setting(&reopen(&f), KEY)["rev"] {
        RowValue::Integer(n) => n,
        ref other => panic!("{other:?}"),
    };
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
}

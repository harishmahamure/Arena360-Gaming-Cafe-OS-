//! One test per row of the Story 1.7 apply matrix (the append-only and replay rows live in
//! `arena-store` and `arena-replay`).

use std::path::PathBuf;
use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ElevatedBy, ElevationMode, ErrorCode, Intent,
    IntentKind, IntentPayload,
};
use arena_store::{ChangeOp, LeaseRow, Row, RowValue, Store, Table, read_lease, write_lease};
use tokio::sync::{broadcast, watch};

use super::*;
use crate::DEFAULT_SETTINGS;
use crate::clock::ManualClock;
use crate::status::{HubStatus, LeaseInfo, status_channel};
use crate::writer::{Writer, WriterHandle};

const T0: i64 = 1_800_000_000_000;
const HUB: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

fn lease_row(counter: i64, id: &str) -> LeaseRow {
    LeaseRow {
        counter,
        hub_instance_id: id.into(),
        provisional: true,
        renewed_at: T0,
        expires_at: T0 + 15_000,
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    clock: Arc<ManualClock>,
    status: watch::Sender<HubStatus>,
    writer: WriterHandle,
    events: broadcast::Receiver<Applied>,
}

/// A primary writer holding the lease `(1, HUB)`.
fn primary() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    let row = lease_row(1, HUB);
    store.write_tx(|tx| write_lease(tx, &row)).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB);
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&row));
    let (status, rx) = status_channel(initial);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
    let events = writer.subscribe();
    Fixture {
        _dir: dir,
        path,
        clock,
        status,
        writer,
        events,
    }
}

fn reopen(f: &Fixture) -> Store {
    Store::open(&f.path, T0).unwrap()
}

fn payload() -> BranchBootstrap {
    BranchBootstrap {
        name: "Dev Branch".into(),
        branch_code: "DEV01".into(),
        currency: "INR".into(),
        tz: "Asia/Kolkata".into(),
        day_start_min: 360,
        fy_start_month: 4,
    }
}

fn intent_with(actor: ActorKind, b: BranchBootstrap) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::BranchBootstrap,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev: None,
        actor: Actor {
            kind: actor,
            id: "test".into(),
        },
        elevated_by: None,
        payload: IntentPayload::BranchBootstrap(b),
    }
}

fn bootstrap() -> Intent {
    intent_with(ActorKind::System, payload())
}

fn assert_nothing_written(store: &Store) {
    assert!(store.rows(Table::Branch).unwrap().is_empty());
    assert!(store.rows(Table::Settings).unwrap().is_empty());
    assert!(store.changes().unwrap().is_empty());
}

async fn expect_err(f: &Fixture, intent: Intent) -> AppError {
    f.writer.submit_intent(intent).await.unwrap_err()
}

#[tokio::test]
async fn bootstrap_writes_branch_settings_and_gap_free_changes() {
    let mut f = primary();
    let intent = bootstrap();
    let applied = f.writer.submit_intent(intent.clone()).await.unwrap();

    let store = reopen(&f);
    let branches = store.rows(Table::Branch).unwrap();
    assert_eq!(branches.len(), 1);
    let b = &branches[0];
    let RowValue::Text(branch_id) = &b["branch_id"] else {
        panic!("branch_id is text")
    };
    assert!(branch_id.parse::<ulid::Ulid>().is_ok());
    assert_eq!(b["name"], RowValue::Text("Dev Branch".into()));
    assert_eq!(b["currency"], RowValue::Text("INR".into()));
    assert_eq!(b["day_start_min"], RowValue::Integer(360));
    assert_eq!(b["fy_start_month"], RowValue::Integer(4));
    assert_eq!(b["rev"], RowValue::Integer(1));
    assert_eq!(b["created_at"], RowValue::Integer(T0));
    assert_eq!(b["updated_at"], RowValue::Integer(T0));

    let settings = store.rows(Table::Settings).unwrap();
    assert_eq!(settings.len(), DEFAULT_SETTINGS.len());
    for (key, value) in DEFAULT_SETTINGS {
        let s = settings
            .iter()
            .find(|s| s["key"] == RowValue::Text((*key).into()))
            .unwrap();
        assert_eq!(s["value"], RowValue::Text((*value).into()));
        assert_eq!(s["branch_id"], RowValue::Text(branch_id.clone()));
        assert_eq!(s["rev"], RowValue::Integer(1));
        assert_eq!(s["updated_at"], RowValue::Integer(T0));
    }

    let changes = store.changes().unwrap();
    assert_eq!(changes.len(), 1 + DEFAULT_SETTINGS.len());
    let epoch = format!("1:{HUB}");
    for (i, c) in changes.iter().enumerate() {
        assert_eq!(c.epoch, epoch);
        assert_eq!(c.seq, i as i64 + 1, "seq is gap-free from 1");
        assert_eq!(c.op, ChangeOp::Insert);
        assert_eq!(c.intent_id, intent.intent_id);
        assert_eq!(c.intent_kind, "branch.bootstrap");
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("system", "test")
        );
        assert_eq!(c.elevated_by_kind, None);
        assert_eq!(c.at, T0);
        // The full row after the write, as a MessagePack map.
        let row: Row = rmp_serde::from_slice(&c.row).unwrap();
        let table = Table::from_name(&c.tbl).unwrap();
        assert!(store.rows(table).unwrap().contains(&row), "{}", c.row_id);
        // A MessagePack map marker: fixmap 0x80–0x8f, map16 0xde or map32 0xdf.
        assert!(matches!(c.row[0], 0x80..=0x8f | 0xde | 0xdf));
    }
    assert_eq!(changes[0].tbl, "branch");
    assert_eq!(&changes[0].row_id, branch_id);
    assert_eq!(changes[1].row_id, format!("{branch_id}/idle_lock_minutes"));

    // emit fired exactly once, with what was committed.
    let event = f.events.try_recv().unwrap();
    assert_eq!(event, applied);
    assert_eq!(event.intent_id, intent.intent_id);
    assert_eq!(
        event.changes.iter().map(|c| c.seq).collect::<Vec<_>>(),
        (1..=changes.len() as i64).collect::<Vec<_>>()
    );
    assert!(f.events.try_recv().is_err());
}

#[tokio::test]
async fn second_bootstrap_fails_already_bootstrapped_and_writes_nothing() {
    let mut f = primary();
    f.writer.submit_intent(bootstrap()).await.unwrap();
    f.events.try_recv().unwrap();
    let before = {
        let s = reopen(&f);
        (
            s.rows(Table::Branch).unwrap(),
            s.rows(Table::Settings).unwrap(),
            s.changes().unwrap(),
        )
    };
    f.clock.advance(1_000);
    let err = expect_err(&f, bootstrap()).await;
    assert_eq!(err.code, ErrorCode::AlreadyBootstrapped);
    let s = reopen(&f);
    assert_eq!(
        (
            s.rows(Table::Branch).unwrap(),
            s.rows(Table::Settings).unwrap(),
            s.changes().unwrap()
        ),
        before
    );
    assert!(
        f.events.try_recv().is_err(),
        "a failed intent emits nothing"
    );
}

#[tokio::test]
async fn non_system_actor_is_forbidden() {
    let mut f = primary();
    for kind in [
        ActorKind::Staff,
        ActorKind::Owner,
        ActorKind::Admin,
        ActorKind::Device,
    ] {
        let err = expect_err(&f, intent_with(kind, payload())).await;
        assert_eq!(err.code, ErrorCode::Forbidden, "{kind:?}");
    }
    let mut anonymous = bootstrap();
    anonymous.actor.id = " ".into();
    assert_eq!(expect_err(&f, anonymous).await.code, ErrorCode::Forbidden);
    assert_nothing_written(&reopen(&f));
    assert!(f.events.try_recv().is_err());
}

#[tokio::test]
async fn follower_role_is_not_primary() {
    let f = primary();
    f.status.send_modify(|s| s.role = Role::Follower);
    let err = expect_err(&f, bootstrap()).await;
    assert_eq!(err.code, ErrorCode::NotPrimary);
    assert_nothing_written(&reopen(&f));

    f.status.send_modify(|s| {
        s.role = Role::Primary;
        s.lease = None;
    });
    assert_eq!(
        expect_err(&f, bootstrap()).await.code,
        ErrorCode::NotPrimary
    );
    assert_nothing_written(&reopen(&f));
}

#[tokio::test]
async fn superseded_lease_is_not_primary() {
    let f = primary();
    reopen(&f)
        .write_tx(|tx| write_lease(tx, &lease_row(2, "ZZZZZZZZZZZZZZZZZZZZZZZZZZ")))
        .unwrap();
    let err = expect_err(&f, bootstrap()).await;
    assert_eq!(err.code, ErrorCode::NotPrimary);
    let s = reopen(&f);
    assert_nothing_written(&s);
    assert_eq!(s.lease().unwrap().unwrap().counter, 2);
}

#[tokio::test]
async fn permission_is_checked_before_epoch() {
    let f = primary();
    f.status.send_modify(|s| s.role = Role::Follower);
    let err = expect_err(&f, intent_with(ActorKind::Staff, payload())).await;
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[tokio::test]
async fn bad_currency_fails_validation_on_currency() {
    let f = primary();
    let err = expect_err(
        &f,
        intent_with(
            ActorKind::System,
            BranchBootstrap {
                currency: "XXX1".into(),
                ..payload()
            },
        ),
    )
    .await;
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    assert_eq!(err.field.as_deref(), Some("currency"));
    assert_nothing_written(&reopen(&f));
}

#[tokio::test]
async fn every_validation_names_its_field() {
    let f = primary();
    let cases: Vec<(&str, BranchBootstrap)> = vec![
        (
            "currency",
            BranchBootstrap {
                currency: "inr".into(),
                ..payload()
            },
        ),
        (
            "currency",
            BranchBootstrap {
                currency: "".into(),
                ..payload()
            },
        ),
        (
            "tz",
            BranchBootstrap {
                tz: "".into(),
                ..payload()
            },
        ),
        (
            "tz",
            BranchBootstrap {
                tz: "IST".into(),
                ..payload()
            },
        ),
        (
            "day_start_min",
            BranchBootstrap {
                day_start_min: 1440,
                ..payload()
            },
        ),
        (
            "fy_start_month",
            BranchBootstrap {
                fy_start_month: 0,
                ..payload()
            },
        ),
        (
            "fy_start_month",
            BranchBootstrap {
                fy_start_month: 13,
                ..payload()
            },
        ),
        (
            "name",
            BranchBootstrap {
                name: "  ".into(),
                ..payload()
            },
        ),
        (
            "branch_code",
            BranchBootstrap {
                branch_code: "".into(),
                ..payload()
            },
        ),
    ];
    for (field, b) in cases {
        let err = expect_err(&f, intent_with(ActorKind::System, b)).await;
        assert_eq!(err.code, ErrorCode::ValidationFailed, "{field}");
        assert_eq!(err.field.as_deref(), Some(field));
    }
    assert_nothing_written(&reopen(&f));

    // Range edges are accepted.
    for currency in ["USD", "EUR", "GBP", "AED", "SGD"] {
        assert!(KNOWN_CURRENCIES.contains(&currency));
    }
    f.writer
        .submit_intent(intent_with(
            ActorKind::System,
            BranchBootstrap {
                day_start_min: 1439,
                fy_start_month: 12,
                ..payload()
            },
        ))
        .await
        .unwrap();
}

#[test]
fn failure_after_mutate_rolls_everything_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    store
        .write_tx(|tx| write_lease(tx, &lease_row(1, HUB)))
        .unwrap();
    let clock = ManualClock::new(T0);
    let epoch = Some(LeaseInfo::from(&lease_row(1, HUB)).epoch());
    let ctx = ApplyCtx {
        fail_after_mutate: true,
        ..ApplyCtx::new(&clock, epoch.clone(), Role::Primary)
    };
    let err = store
        .write_tx_mut(|tx| apply(tx, &ctx, &bootstrap()))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("after mutate"));
    assert_nothing_written(&store);

    // The same intent without the hook succeeds on the same store.
    let ctx = ApplyCtx::new(&clock, epoch, Role::Primary);
    store
        .write_tx_mut(|tx| {
            assert!(read_lease(tx)?.is_some());
            apply(tx, &ctx, &bootstrap())
        })
        .unwrap();
    assert_eq!(store.rows(Table::Branch).unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_envelope_kind_does_not_decode() {
    let f = primary();
    let json = serde_json::json!({
        "intent_id": "01J", "kind": "branch.rename", "idempotency_key": "01J",
        "issued_at": 0, "actor": {"kind": "system", "id": "x"},
        "payload": {"kind": "branch.bootstrap", "name": "a", "branch_code": "b",
                    "currency": "INR", "tz": "A/B", "day_start_min": 0, "fy_start_month": 1}
    });
    assert!(serde_json::from_value::<Intent>(json).is_err());
    assert_nothing_written(&reopen(&f));
}

#[tokio::test]
async fn unverified_elevation_is_forbidden() {
    let f = primary();
    let mut intent = bootstrap();
    intent.elevated_by = Some(ElevatedBy {
        kind: ActorKind::Staff,
        id: "mgr".into(),
        mode: ElevationMode::CachedVerifier,
        verifier_version: 3,
    });
    let err = expect_err(&f, intent).await;
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(err.message, "elevation not verified");
    assert_nothing_written(&reopen(&f));
}

#[tokio::test]
async fn expired_own_lease_is_not_primary() {
    let f = primary();
    // The lease row is still ours, but the clock is past its expiry.
    f.clock.set(T0 + 15_000);
    let err = expect_err(&f, bootstrap()).await;
    assert_eq!(err.code, ErrorCode::NotPrimary);
    assert!(err.message.contains("expired"), "{err}");
    assert_nothing_written(&reopen(&f));
    f.clock.set(T0 + 14_999);
    f.writer.submit_intent(bootstrap()).await.unwrap();
}

#[tokio::test]
async fn envelope_and_field_shape_checks() {
    let f = primary();
    let mut bad_id = bootstrap();
    bad_id.intent_id = "not-a-ulid".into();
    let err = expect_err(&f, bad_id).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("intent_id"))
    );
    let mut with_rev = bootstrap();
    with_rev.base_rev = Some(1);
    let err = expect_err(&f, with_rev).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    let field_cases: Vec<(&str, BranchBootstrap)> = vec![
        (
            "tz",
            BranchBootstrap {
                tz: "A/B".into(),
                ..payload()
            },
        ),
        (
            "tz",
            BranchBootstrap {
                tz: "/".into(),
                ..payload()
            },
        ),
        (
            "tz",
            BranchBootstrap {
                tz: "asia/kolkata".into(),
                ..payload()
            },
        ),
        (
            "name",
            BranchBootstrap {
                name: "x".repeat(81),
                ..payload()
            },
        ),
        (
            "name",
            BranchBootstrap {
                name: "Dev\u{7}Branch".into(),
                ..payload()
            },
        ),
        (
            "name",
            BranchBootstrap {
                name: "Dev\nBranch".into(),
                ..payload()
            },
        ),
        (
            "branch_code",
            BranchBootstrap {
                branch_code: "DEV 01".into(),
                ..payload()
            },
        ),
        (
            "branch_code",
            BranchBootstrap {
                branch_code: "DEV_01".into(),
                ..payload()
            },
        ),
        (
            "branch_code",
            BranchBootstrap {
                branch_code: "D".repeat(17),
                ..payload()
            },
        ),
        (
            "branch_code",
            BranchBootstrap {
                branch_code: "DÉV".into(),
                ..payload()
            },
        ),
    ];
    for (field, b) in field_cases {
        let err = expect_err(&f, intent_with(ActorKind::System, b.clone())).await;
        assert_eq!(err.code, ErrorCode::ValidationFailed, "{b:?}");
        assert_eq!(err.field.as_deref(), Some(field), "{b:?}");
    }
    assert_nothing_written(&reopen(&f));

    // Edges that are accepted: UTC, an 80-char trimmed name, a 16-char code with '-'.
    f.writer
        .submit_intent(intent_with(
            ActorKind::System,
            BranchBootstrap {
                tz: "UTC".into(),
                name: format!("  {}  ", "é".repeat(80)),
                branch_code: "AB-12-cd-EF-3456".into(),
                ..payload()
            },
        ))
        .await
        .unwrap();
    let b = &reopen(&f).rows(Table::Branch).unwrap()[0];
    assert_eq!(b["name"], RowValue::Text("é".repeat(80)));
    assert_eq!(b["tz"], RowValue::Text("UTC".into()));
}

#[tokio::test]
async fn submit_intent_after_close_is_not_primary() {
    let f = primary();
    f.writer.close().await;
    assert_eq!(
        expect_err(&f, bootstrap()).await.code,
        ErrorCode::NotPrimary
    );
}

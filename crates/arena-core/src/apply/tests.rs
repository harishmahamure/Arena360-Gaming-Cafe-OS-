//! One test per row of the Story 1.7 apply matrix (the append-only and replay rows live in
//! `arena-store` and `arena-replay`), then one per row of the Story 1.8 dedupe/TTL/base_rev
//! matrix (the prune row lives in `arena-tasks`, the property row in `tests/proptest_apply.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ElevatedBy, ElevationKind, ElevationMode,
    ErrorCode, Intent, IntentKind, IntentPayload, ReasonCode, SettingsSet,
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
        reason: None,
        payload: IntentPayload::BranchBootstrap(b),
    }
}

fn bootstrap() -> Intent {
    intent_with(ActorKind::System, payload())
}

/// How many change rows `branch.bootstrap` writes: the branch, its settings, the built-in
/// roles and their permission rows.
fn bootstrap_changes() -> usize {
    1 + DEFAULT_SETTINGS.len()
        + crate::LOCALE_KEYS.len()
        + crate::rbac::BUILTIN_ROLES
            .iter()
            .map(|r| 1 + crate::rbac::builtin_perms(r).len())
            .sum::<usize>()
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
    assert_eq!(
        settings.len(),
        DEFAULT_SETTINGS.len() + crate::LOCALE_KEYS.len()
    );
    let seeded: Vec<(String, String)> = DEFAULT_SETTINGS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .chain([
            ("branch.currency".into(), "INR".into()),
            ("branch.symbol_position".into(), "before".into()),
            ("branch.number_format".into(), "indian".into()),
            ("branch.tz".into(), "Asia/Kolkata".into()),
            ("branch.day_start_min".into(), "360".into()),
            ("branch.fy_start_month".into(), "4".into()),
        ])
        .collect();
    for (key, value) in &seeded {
        let s = settings
            .iter()
            .find(|s| s["key"] == RowValue::Text(key.clone()))
            .unwrap();
        assert_eq!(s["value"], RowValue::Text(value.clone()));
        assert_eq!(s["prior_value"], RowValue::Null);
        assert_eq!(s["effective_from"], RowValue::Null);
        assert_eq!(s["branch_id"], RowValue::Text(branch_id.clone()));
        assert_eq!(s["rev"], RowValue::Integer(1));
        assert_eq!(s["updated_at"], RowValue::Integer(T0));
    }

    let changes = store.changes().unwrap();
    assert_eq!(changes.len(), bootstrap_changes());
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
        assert_eq!(err.code, ErrorCode::PermissionDenied, "{kind:?}");
    }
    let mut anonymous = bootstrap();
    anonymous.actor.id = " ".into();
    assert_eq!(
        expect_err(&f, anonymous).await.code,
        ErrorCode::PermissionDenied
    );
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
    assert_eq!(err.code, ErrorCode::PermissionDenied);
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
        assert!(arena_proto::currency::is_known_currency(currency));
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
async fn an_elevated_system_intent_is_forbidden() {
    // Only a permission can be lent by a PIN: a System requirement cannot be elevated.
    let f = primary();
    let mut intent = bootstrap();
    intent.elevated_by = Some(ElevatedBy {
        kind: ElevationKind::Pin,
        id: "mgr".into(),
        mode: ElevationMode::Hub,
        verifier_version: 1,
    });
    let err = expect_err(&f, intent).await;
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert_eq!(err.message, crate::rbac::denied().message);
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

// ---- Story 1.8: idempotency, TTL and base_rev ----

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A primary writer whose lease never expires (the clock moves by days in these tests).
fn primary_long_lease() -> Fixture {
    let f = primary();
    let row = LeaseRow {
        expires_at: i64::MAX,
        ..lease_row(1, HUB)
    };
    reopen(&f).write_tx(|tx| write_lease(tx, &row)).unwrap();
    f.status
        .send_modify(|s| s.lease = Some(LeaseInfo::from(&row)));
    f
}

/// A bootstrapped branch (its bootstrap event already drained).
async fn bootstrapped() -> Fixture {
    let mut f = primary_long_lease();
    f.writer.submit_intent(bootstrap()).await.unwrap();
    f.events.try_recv().unwrap();
    f
}

fn set(key: &str, value: &str, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::SettingsSet,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev,
        actor: Actor {
            kind: ActorKind::Owner,
            id: "owner-1".into(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::SettingsSet(SettingsSet {
            key: key.into(),
            value: value.into(),
        }),
    }
}

fn setting(store: &Store, key: &str) -> Row {
    store
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text(key.into()))
        .unwrap()
}

/// Everything a rejected or deduped intent must leave untouched.
fn snapshot(f: &Fixture) -> (Vec<Row>, Vec<Row>, Vec<arena_store::ChangeRecord>, i64) {
    let s = reopen(f);
    (
        s.rows(Table::Branch).unwrap(),
        s.rows(Table::Settings).unwrap(),
        s.changes().unwrap(),
        s.applied_intent_count().unwrap(),
    )
}

#[tokio::test]
async fn fresh_edit_bumps_rev_by_one_with_one_update_change() {
    let mut f = bootstrapped().await;
    let before_changes = reopen(&f).changes().unwrap().len();
    f.clock.advance(1_000);
    let intent = set("idle_lock_minutes", "9", Some(1));
    let applied = f.writer.submit_intent(intent.clone()).await.unwrap();

    let s = reopen(&f);
    let row = setting(&s, "idle_lock_minutes");
    assert_eq!(row["value"], RowValue::Text("9".into()));
    assert_eq!(row["rev"], RowValue::Integer(2));
    assert_eq!(row["updated_at"], RowValue::Integer(T0 + 1_000));
    let changes = s.changes().unwrap();
    assert_eq!(changes.len(), before_changes + 1);
    let c = changes.last().unwrap();
    assert_eq!(c.seq, before_changes as i64 + 1, "seq stays gap-free");
    assert_eq!((c.op, c.tbl.as_str()), (ChangeOp::Update, "settings"));
    assert_eq!(c.intent_kind, "settings.set");
    assert_eq!(
        (c.actor_kind.as_str(), c.actor_id.as_str()),
        ("owner", "owner-1")
    );
    let logged: Row = rmp_serde::from_slice(&c.row).unwrap();
    assert_eq!(
        logged, row,
        "the change carries the full row after the write"
    );
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(f.events.try_recv().unwrap(), applied);
    assert_eq!(
        s.applied_intent_count().unwrap(),
        2,
        "bootstrap and the edit"
    );
}

#[tokio::test]
async fn duplicate_success_returns_the_same_applied_and_writes_nothing() {
    let mut f = bootstrapped().await;
    let first = set("hub.locale", "en-GB", Some(1));
    let applied = f.writer.submit_intent(first.clone()).await.unwrap();
    f.events.try_recv().unwrap();
    let before = snapshot(&f);

    f.clock.advance(60_000);
    assert_eq!(
        f.writer.submit_intent(first.clone()).await.unwrap(),
        applied
    );
    // A new intent_id and issued_at with the same body is still a retry.
    let mut retried = first.clone();
    retried.intent_id = ulid::Ulid::generate().to_string();
    retried.issued_at += 1;
    assert_eq!(f.writer.submit_intent(retried).await.unwrap(), applied);
    // Keys are case-insensitive.
    let mut lower = first;
    lower.idempotency_key = lower.idempotency_key.to_ascii_lowercase();
    assert_eq!(f.writer.submit_intent(lower).await.unwrap(), applied);

    assert_eq!(snapshot(&f), before);
    assert_eq!(
        setting(&reopen(&f), "hub.locale")["value"],
        RowValue::Text("en-GB".into())
    );
    assert!(f.events.try_recv().is_err(), "a dedupe hit emits nothing");
}

#[tokio::test]
async fn duplicate_rejection_returns_the_same_stale_and_writes_nothing() {
    let f = bootstrapped().await;
    // base_rev 2 is ahead of the row (rev 1): STALE, recorded.
    let stale = set("hub.locale", "en-GB", Some(2));
    let err = expect_err(&f, stale.clone()).await;
    assert_eq!(err.code, ErrorCode::Stale);
    // Another edit moves the row to rev 2, so the retry would now pass on its own merits...
    f.writer
        .submit_intent(set("hub.locale", "en-US", Some(1)))
        .await
        .unwrap();
    let before = snapshot(&f);
    // ...but the retried key gets its stored STALE back, and nothing is written.
    assert_eq!(expect_err(&f, stale).await, err);
    assert_eq!(snapshot(&f), before);
    assert_eq!(
        setting(&reopen(&f), "hub.locale")["value"],
        RowValue::Text("en-US".into())
    );
}

#[tokio::test]
async fn stale_base_rev_is_rejected_and_the_row_is_unchanged() {
    let f = bootstrapped().await;
    f.writer
        .submit_intent(set("receipt.footer", "thanks", Some(1)))
        .await
        .unwrap();
    let before = setting(&reopen(&f), "receipt.footer");
    assert_eq!(before["rev"], RowValue::Integer(2));
    for base_rev in [1, 3] {
        let err = expect_err(&f, set("receipt.footer", "bye", Some(base_rev))).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::Stale, Some("base_rev"))
        );
    }
    let s = reopen(&f);
    assert_eq!(setting(&s, "receipt.footer"), before);
    assert_eq!(s.changes().unwrap().len(), bootstrap_changes() + 1);
}

#[tokio::test]
async fn expired_intent_fails_and_is_not_recorded() {
    let f = bootstrapped().await;
    let now = T0 + 30 * DAY_MS;
    f.clock.set(now);
    let mut old = set("hub.locale", "en-GB", Some(1));
    old.issued_at = now - INTENT_TTL_MS - 1;
    let before = snapshot(&f);
    let err = expect_err(&f, old.clone()).await;
    assert_eq!(err.code, ErrorCode::Expired);
    assert_eq!(snapshot(&f), before, "EXPIRED is never recorded");
    assert_eq!(expect_err(&f, old.clone()).await.code, ErrorCode::Expired);

    // Exactly at the TTL is still accepted, under the same key.
    old.issued_at = now - INTENT_TTL_MS;
    f.writer.submit_intent(old).await.unwrap();
}

#[tokio::test]
async fn future_issued_at_beyond_skew_fails_validation_and_is_not_recorded() {
    let f = bootstrapped().await;
    let mut ahead = set("hub.locale", "en-GB", Some(1));
    ahead.issued_at = T0 + 10 * 60 * 1000;
    let before = snapshot(&f);
    let err = expect_err(&f, ahead.clone()).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("issued_at"))
    );
    assert_eq!(snapshot(&f), before);
    // Within the 5-minute skew the same key goes through.
    ahead.issued_at = T0 + MAX_CLOCK_SKEW_MS;
    f.writer.submit_intent(ahead).await.unwrap();
}

#[tokio::test]
async fn malformed_idempotency_key_is_bad_frame_before_any_job() {
    let f = bootstrapped().await;
    let jobs = f.writer.jobs_submitted();
    let before = snapshot(&f);
    for key in ["", "not-a-ulid", "01JAAAAAAAAAAAAAAAAAAAAAAI"] {
        let mut intent = set("hub.locale", "en-GB", Some(1));
        intent.idempotency_key = key.into();
        let err = expect_err(&f, intent).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::BadFrame, Some("idempotency_key")),
            "{key:?}"
        );
    }
    assert_eq!(f.writer.jobs_submitted(), jobs, "no transaction was opened");
    assert_eq!(snapshot(&f), before);
    // A missing key does not even decode as an intent frame.
    let mut json = serde_json::to_value(set("hub.locale", "x", Some(1))).unwrap();
    json.as_object_mut().unwrap().remove("idempotency_key");
    assert!(serde_json::from_value::<Intent>(json).is_err());
}

#[tokio::test]
async fn settings_set_requires_base_rev_and_an_existing_key() {
    let f = bootstrapped().await;
    let err = expect_err(&f, set("hub.locale", "en-GB", None)).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    let err = expect_err(&f, set("no.such.key", "x", Some(1))).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("key"))
    );
    let err = expect_err(
        &f,
        set(
            "hub.locale",
            &"x".repeat(MAX_SETTING_VALUE_CHARS + 1),
            Some(1),
        ),
    )
    .await;
    assert_eq!(err.field.as_deref(), Some("value"));
    let mut mislabelled = set("hub.locale", "en-GB", Some(1));
    mislabelled.kind = IntentKind::BranchBootstrap;
    let err = expect_err(&f, mislabelled).await;
    assert_eq!(err.field.as_deref(), Some("kind"));
    assert_eq!(
        setting(&reopen(&f), "hub.locale")["rev"],
        RowValue::Integer(1)
    );
}

#[tokio::test]
async fn settings_set_needs_settings_edit() {
    let f = bootstrapped().await;
    // A staff actor with no staff row, the system and admin hold no permission.
    for kind in [
        ActorKind::Device,
        ActorKind::Admin,
        ActorKind::System,
        ActorKind::Staff,
    ] {
        let mut intent = set("hub.locale", "en-GB", Some(1));
        intent.actor.kind = kind;
        assert_eq!(
            expect_err(&f, intent).await.code,
            ErrorCode::PermissionDenied,
            "{kind:?}"
        );
    }
    f.writer
        .submit_intent(set("hub.locale", "en-GB", Some(1)))
        .await
        .unwrap();
}

#[tokio::test]
async fn permission_denials_and_not_primary_are_not_recorded() {
    let f = bootstrapped().await;
    let mut device = set("hub.locale", "en-GB", Some(1));
    device.actor.kind = ActorKind::Device;
    let count = |f: &Fixture| reopen(f).applied_intent_count().unwrap();
    let n = count(&f);
    assert_eq!(
        expect_err(&f, device.clone()).await.code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(count(&f), n, "a permission denial is never recorded");
    // Once permitted (here: the same key from an allowed actor), the key runs for real.
    device.actor.kind = ActorKind::Owner;
    f.writer.submit_intent(device).await.unwrap();
    let n = count(&f);

    f.status.send_modify(|s| s.role = Role::Follower);
    let follower = set("hub.locale", "en-US", Some(2));
    assert_eq!(
        expect_err(&f, follower.clone()).await.code,
        ErrorCode::NotPrimary
    );
    // A forbidden intent on a follower is not recorded either (the record needs the epoch).
    let mut forbidden = set("hub.locale", "en-US", Some(2));
    forbidden.actor.kind = ActorKind::Admin;
    assert_eq!(
        expect_err(&f, forbidden.clone()).await.code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(count(&f), n);

    // Back to primary, the NOT_PRIMARY key runs for real.
    f.status.send_modify(|s| s.role = Role::Primary);
    f.writer.submit_intent(follower).await.unwrap();
}

#[tokio::test]
async fn dedupe_wins_over_ttl_for_a_recorded_key() {
    let f = bootstrapped().await;
    let intent = set("hub.locale", "en-GB", Some(1));
    let applied = f.writer.submit_intent(intent.clone()).await.unwrap();
    f.clock.advance(8 * DAY_MS);
    assert_eq!(f.writer.submit_intent(intent).await.unwrap(), applied);
}

#[tokio::test]
async fn settings_set_before_bootstrap_fails_on_key_and_is_not_recorded() {
    let mut f = primary_long_lease();
    let early = set("hub.locale", "en-GB", Some(1));
    let err = expect_err(&f, early.clone()).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("key"))
    );
    let s = reopen(&f);
    assert!(s.changes().unwrap().is_empty());
    assert_eq!(
        s.applied_intent_count().unwrap(),
        0,
        "key-not-found is not recorded"
    );
    // After bootstrap the same key is evaluated afresh and succeeds.
    f.writer.submit_intent(bootstrap()).await.unwrap();
    f.events.try_recv().unwrap();
    f.writer.submit_intent(early).await.unwrap();
}

#[tokio::test]
async fn reused_key_with_a_different_body_is_an_unrecorded_conflict() {
    let f = bootstrapped().await;
    let first = set("hub.locale", "en-GB", Some(1));
    f.writer.submit_intent(first.clone()).await.unwrap();
    let stale = set("hub.locale", "en-GB", Some(9));
    expect_err(&f, stale.clone()).await;
    let before = snapshot(&f);
    let perturbations: Vec<Intent> = vec![
        {
            let mut i = first.clone();
            i.payload = IntentPayload::SettingsSet(SettingsSet {
                key: "hub.locale".into(),
                value: "fr-FR".into(),
            });
            i
        },
        {
            let mut i = first.clone();
            i.base_rev = Some(2);
            i
        },
        {
            let mut i = first.clone();
            i.actor.id = "someone-else".into();
            i
        },
        {
            // A different kind under the same key.
            let mut i = bootstrap();
            i.idempotency_key = first.idempotency_key.clone();
            i
        },
        {
            let mut i = stale.clone();
            i.base_rev = Some(2);
            i
        },
    ];
    for intent in perturbations {
        let err = expect_err(&f, intent).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::IdempotencyConflict, Some("idempotency_key"))
        );
    }
    assert_eq!(snapshot(&f), before, "a conflict writes nothing");
    // The original retry still gets its outcome.
    assert!(f.writer.submit_intent(first).await.is_ok());
}

#[tokio::test]
async fn values_are_validated_per_key() {
    let f = bootstrapped().await;
    for (key, value) in [
        ("idle_lock_minutes", "0"),
        ("idle_lock_minutes", "31"),
        ("idle_lock_minutes", "five"),
        ("idle_lock_minutes", ""),
        ("hub.locale", ""),
        ("hub.locale", "en_IN"),
        ("hub.locale", "x"),
    ] {
        let err = expect_err(&f, set(key, value, Some(1))).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("value")),
            "{key}={value:?}"
        );
    }
    let before = snapshot(&f);
    assert_eq!(before.3, 1 + 7, "value rejections are recorded");
    for (key, value) in [
        ("idle_lock_minutes", "1"),
        ("idle_lock_minutes", "30"),
        ("hub.locale", "zh-Hant-TW"),
        ("receipt.footer", "Thank you! ✓"),
        ("receipt.footer", ""),
    ] {
        let rev = match setting(&reopen(&f), key)["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        };
        f.writer
            .submit_intent(set(key, value, Some(rev)))
            .await
            .unwrap();
    }
}

// ---- Story 1.9: device intents ----

fn device_intent(actor: ActorKind, body: IntentPayload) -> Intent {
    Intent {
        kind: body.kind(),
        // A revoke needs a reason; the reason rows have their own tests.
        reason: body.kind().reasons().map(|r| r[0]),
        payload: body,
        ..intent_with(actor, payload())
    }
}

fn register(hash: &str) -> IntentPayload {
    IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
        class: arena_proto::ClientClass::Paired,
        label: "Dev Kiosk".into(),
        token_hash: hash.into(),
    })
}

/// A bootstrapped branch with one registered paired device; returns its id.
async fn with_device(f: &mut Fixture) -> (String, String) {
    f.writer.submit_intent(bootstrap()).await.unwrap();
    let secret = crate::devices::new_token_secret().unwrap();
    let hash = crate::devices::hash_token_secret(&secret).unwrap();
    let applied = f
        .writer
        .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
        .await
        .unwrap();
    // The device row, then its `display.hc.<id>` setting (Story 1.21), in one change.
    assert_eq!(applied.changes.len(), 2);
    assert_eq!(applied.changes[0].tbl, "devices");
    assert_eq!(applied.changes[0].op, ChangeOp::Insert);
    assert_eq!(applied.changes[1].tbl, "settings");
    assert_eq!(applied.changes[1].op, ChangeOp::Insert);
    while f.events.try_recv().is_ok() {}
    (applied.changes[0].row_id.clone(), secret)
}

#[tokio::test]
async fn device_register_writes_a_row_and_an_insert_change() {
    let mut f = primary();
    let (id, secret) = with_device(&mut f).await;
    let store = reopen(&f);
    let row = store.device(&id).unwrap().unwrap();
    assert_eq!(row.class, "paired");
    assert_eq!(row.label, "Dev Kiosk");
    assert_eq!(row.rev, 1);
    assert_eq!(row.revoked_at, None);
    assert!(crate::devices::verify_token_secret(
        &secret,
        &row.token_hash
    ));
    let devices = store.rows(Table::Devices).unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["created_at"], RowValue::Integer(T0));
    let mut changes = store.changes().unwrap();
    let hc = changes.pop().unwrap();
    let change = changes.pop().unwrap();
    assert_eq!(change.intent_kind, "device.register");
    let logged: Row = rmp_serde::from_slice(&change.row).unwrap();
    assert_eq!(logged, devices[0]);
    // Story 1.21: the device's high-contrast row starts off, in the same intent.
    assert_eq!(hc.intent_kind, "device.register");
    assert_eq!(hc.intent_id, change.intent_id);
    assert_eq!(hc.tbl, "settings");
    let hc_row = setting(&store, &arena_proto::hc_key(&id));
    assert_eq!(hc_row["value"], RowValue::Text("false".into()));
    assert_eq!(hc_row["rev"], RowValue::Integer(1));
}

#[tokio::test]
async fn device_register_checks_actor_hash_class_and_branch() {
    let f = primary();
    // Before bootstrap there is no branch to join.
    let hash = crate::devices::hash_token_secret("s").unwrap();
    let err = expect_err(&f, device_intent(ActorKind::Owner, register(&hash))).await;
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    f.writer.submit_intent(bootstrap()).await.unwrap();
    for actor in [ActorKind::Device, ActorKind::Staff, ActorKind::System] {
        let err = expect_err(&f, device_intent(actor, register(&hash))).await;
        assert_eq!(err.code, ErrorCode::PermissionDenied, "{actor:?}");
    }
    let err = expect_err(&f, device_intent(ActorKind::Owner, register("plain"))).await;
    assert_eq!(err.field.as_deref(), Some("token_hash"));
    let phone = IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
        class: arena_proto::ClientClass::Phone,
        label: "x".into(),
        token_hash: hash.clone(),
    });
    let err = expect_err(&f, device_intent(ActorKind::Owner, phone)).await;
    assert_eq!(err.field.as_deref(), Some("class"));
    assert!(reopen(&f).devices().unwrap().is_empty());
}

#[tokio::test]
async fn device_revoke_sets_revoked_at_once_and_needs_device_remote() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    let revoke = || {
        IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
            device_id: id.clone(),
        })
    };
    for actor in [ActorKind::Device, ActorKind::System] {
        let err = expect_err(&f, device_intent(actor, revoke())).await;
        assert_eq!(err.code, ErrorCode::PermissionDenied, "{actor:?}");
    }
    let mut stale = device_intent(ActorKind::Owner, revoke());
    stale.base_rev = Some(7);
    assert_eq!(expect_err(&f, stale).await.code, ErrorCode::Stale);

    f.clock.advance(5);
    let applied = f
        .writer
        .submit_intent(device_intent(ActorKind::Owner, revoke()))
        .await
        .unwrap();
    assert_eq!(applied.changes[0].op, ChangeOp::Update);
    assert_eq!(applied.changes[0].row_id, id);
    assert_eq!(f.events.try_recv().unwrap(), applied);
    let row = reopen(&f).device(&id).unwrap().unwrap();
    assert_eq!((row.revoked_at, row.rev), (Some(T0 + 5), 2));

    let err = expect_err(&f, device_intent(ActorKind::Owner, revoke())).await;
    assert_eq!(
        (err.code, err.field.as_deref()),
        (ErrorCode::ValidationFailed, Some("device_id"))
    );
    let missing = IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
        device_id: "01JNOPEAAAAAAAAAAAAAAAAAAA".into(),
    });
    let err = expect_err(&f, device_intent(ActorKind::Owner, missing)).await;
    assert_eq!(err.field.as_deref(), Some("device_id"));
}

#[tokio::test]
async fn device_version_reported_records_the_version() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    let report = |v: &str| {
        IntentPayload::DeviceVersionReported(arena_proto::DeviceVersionReported {
            device_id: id.clone(),
            version: v.into(),
        })
    };
    let err = expect_err(&f, device_intent(ActorKind::System, report("1.2"))).await;
    assert_eq!(err.field.as_deref(), Some("version"));
    let err = expect_err(&f, device_intent(ActorKind::Device, report("0.1.0"))).await;
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    f.writer
        .submit_intent(device_intent(ActorKind::System, report("0.1.0")))
        .await
        .unwrap();
    let row = reopen(&f).device(&id).unwrap().unwrap();
    assert_eq!(row.reported_version.as_deref(), Some("0.1.0"));
    assert_eq!(row.rev, 2);
    let c = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(c.intent_kind, "device.version_reported");
}

#[tokio::test]
async fn device_directory_verifies_and_sees_revocation() {
    let mut f = primary();
    let (id, secret) = with_device(&mut f).await;
    let dir = crate::DeviceDirectory::open(&f.path).unwrap();
    let token = crate::devices::format_token(&id, &secret);
    let v = dir
        .verify(arena_proto::ClientClass::Paired, &token)
        .unwrap();
    assert_eq!(v.device_id, id);
    assert_eq!(
        dir.verify(arena_proto::ClientClass::Kds, &token)
            .unwrap_err()
            .code,
        ErrorCode::TokenInvalid
    );
    assert!(!dir.is_revoked(&id).unwrap());
    f.writer
        .submit_intent(device_intent(
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: id.clone(),
            }),
        ))
        .await
        .unwrap();
    assert!(dir.is_revoked(&id).unwrap());
    assert_eq!(
        dir.verify(arena_proto::ClientClass::Paired, &token)
            .unwrap_err()
            .code,
        ErrorCode::TokenRevoked
    );
}

#[tokio::test]
async fn a_revoked_or_unknown_device_actor_is_forbidden_before_anything_else() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    let as_device = |device_id: &str| {
        let mut i = device_intent(
            ActorKind::Device,
            IntentPayload::SettingsSet(SettingsSet {
                key: "receipt.footer".into(),
                value: "x".into(),
            }),
        );
        i.actor.id = device_id.into();
        i.base_rev = Some(1);
        i
    };
    // A live device reaches the kind's own check (settings.edit, which no device holds).
    let err = expect_err(&f, as_device(&id)).await;
    assert_eq!(
        (err.code, err.message.as_str()),
        (
            ErrorCode::PermissionDenied,
            "you don't have permission to do this"
        )
    );
    let err = expect_err(&f, as_device("01JNOPEAAAAAAAAAAAAAAAAAAA")).await;
    assert_eq!(
        (err.code, err.message.as_str()),
        (
            ErrorCode::PermissionDenied,
            "the device is revoked or unknown"
        )
    );
    f.writer
        .submit_intent(device_intent(
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: id.clone(),
            }),
        ))
        .await
        .unwrap();
    let err = expect_err(&f, as_device(&id)).await;
    assert_eq!(err.message, "the device is revoked or unknown");
}

// ---- Story 1.16: kiosk.log ----

fn kiosk_log(device_id: &str, code: &str, detail: &str) -> Intent {
    let mut i = device_intent(
        ActorKind::Device,
        IntentPayload::KioskLog(arena_proto::KioskLog {
            level: arena_proto::KioskLogLevel::Warn,
            code: code.into(),
            detail: detail.into(),
        }),
    );
    i.actor.id = device_id.into();
    i
}

#[tokio::test]
async fn kiosk_log_writes_a_device_event_once_from_a_live_device_only() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    // Recorded offline a minute ago: `at` is when the kiosk recorded it.
    let mut first = kiosk_log(&id, "app_restarted", "3 missed pings");
    first.issued_at = T0 - 60_000;
    let applied = f.writer.submit_intent(first.clone()).await.unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(
        (applied.changes[0].tbl.as_str(), applied.changes[0].op),
        ("device_events", ChangeOp::Insert)
    );
    // A replay after a crash (same key, same body) returns the stored outcome, no new row.
    assert_eq!(f.writer.submit_intent(first).await.unwrap(), applied);
    let store = reopen(&f);
    let rows = store.rows(Table::DeviceEvents).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["device_id"], RowValue::Text(id.clone()));
    assert_eq!(rows[0]["at"], RowValue::Integer(T0 - 60_000));
    assert_eq!(rows[0]["level"], RowValue::Text("warn".into()));
    assert_eq!(rows[0]["code"], RowValue::Text("app_restarted".into()));
    assert_eq!(rows[0]["rev"], RowValue::Integer(1));
    let change = store.changes().unwrap().pop().unwrap();
    assert_eq!(
        (change.intent_kind.as_str(), change.actor_kind.as_str()),
        ("kiosk.log", "device")
    );

    // Only a device may log, and only a live one.
    for actor in [ActorKind::Owner, ActorKind::System, ActorKind::Staff] {
        let mut i = kiosk_log(&id, "x", "");
        i.actor.kind = actor;
        assert_eq!(
            expect_err(&f, i).await.code,
            ErrorCode::PermissionDenied,
            "{actor:?}"
        );
    }
    let err = expect_err(&f, kiosk_log("01JNOPEAAAAAAAAAAAAAAAAAAA", "x", "")).await;
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    for (code, detail, field) in [
        ("", "", "code"),
        ("Has Caps", "", "code"),
        (&*"x".repeat(65), "", "code"),
        ("ok", &*"d".repeat(501), "detail"),
        ("ok", "bell\u{7}", "detail"),
    ] {
        let err = expect_err(&f, kiosk_log(&id, code, detail)).await;
        assert_eq!(err.field.as_deref(), Some(field), "{code:?} {detail:?}");
    }
    let mut edit = kiosk_log(&id, "ok", "");
    edit.base_rev = Some(1);
    assert_eq!(
        expect_err(&f, edit).await.field.as_deref(),
        Some("base_rev")
    );
    assert_eq!(reopen(&f).rows(Table::DeviceEvents).unwrap().len(), 1);
}

// ---- Story 1.11: roles, staff and the permission step ----

mod rbac_matrix {
    use super::*;
    use crate::rbac::{
        BUILTIN_ROLES, ROLE_COUNTER, ROLE_KIOSK_DEVICE, ROLE_KITCHEN, ROLE_MANAGER, ROLE_OWNER,
        builtin_perms,
    };
    use crate::staff::{StaffDirectory, hash_secret};
    use arena_proto::{PermEffect, Permission, RolePerm, RoleUpsert, StaffUpsert};

    fn owner() -> Actor {
        Actor {
            kind: ActorKind::Owner,
            id: "owner-1".into(),
        }
    }

    fn staff_actor(id: &str) -> Actor {
        Actor {
            kind: ActorKind::Staff,
            id: id.into(),
        }
    }

    fn as_actor(actor: Actor, body: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            kind: body.kind(),
            payload: body,
            actor,
            base_rev,
            ..bootstrap()
        }
    }

    fn role_id(f: &Fixture, name: &str) -> String {
        reopen(f)
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == RowValue::Text(name.into()))
            .map(|r| match &r["role_id"] {
                RowValue::Text(t) => t.clone(),
                other => panic!("role_id is {other:?}"),
            })
            .unwrap()
    }

    fn staff_body(login: &str, role_id: &str, password: Option<&str>) -> StaffUpsert {
        StaffUpsert {
            staff_id: None,
            login: login.into(),
            display_name: login.into(),
            role_id: role_id.into(),
            password_hash: password.map(|p| hash_secret(p).unwrap()),
            pin_hash: None,
            active: true,
        }
    }

    /// Registers a staff member as the owner; returns the staff id.
    async fn add_staff(f: &Fixture, login: &str, role: &str) -> String {
        let rid = role_id(f, role);
        let applied = f
            .writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(staff_body(login, &rid, Some("pw"))),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(applied.changes.len(), 1);
        assert_eq!(applied.changes[0].tbl, "staff");
        applied.changes[0].row_id.clone()
    }

    fn custom_role(name: &str, perms: &[(&str, PermEffect)]) -> IntentPayload {
        IntentPayload::RoleUpsert(RoleUpsert {
            role_id: None,
            name: name.into(),
            perms: perms
                .iter()
                .map(|(p, e)| RolePerm {
                    permission: (*p).into(),
                    effect: *e,
                })
                .collect(),
        })
    }

    fn footer_rev(f: &Fixture) -> i64 {
        match setting(&reopen(f), "receipt.footer")["rev"] {
            RowValue::Integer(n) => n,
            ref other => panic!("rev is {other:?}"),
        }
    }

    #[tokio::test]
    async fn bootstrap_seeds_the_builtin_roles_as_allow_rows() {
        let f = bootstrapped().await;
        let s = reopen(&f);
        let roles = s.rows(Table::Roles).unwrap();
        assert_eq!(roles.len(), BUILTIN_ROLES.len());
        let perms = s.rows(Table::RolePerms).unwrap();
        for name in BUILTIN_ROLES {
            let role = roles
                .iter()
                .find(|r| r["name"] == RowValue::Text(name.into()))
                .unwrap();
            assert_eq!(role["builtin"], RowValue::Integer(1), "{name}");
            assert_eq!(role["rev"], RowValue::Integer(1), "{name}");
            let mine: Vec<&Row> = perms
                .iter()
                .filter(|p| p["role_id"] == role["role_id"])
                .collect();
            assert_eq!(mine.len(), builtin_perms(name).len(), "{name}");
            assert!(
                mine.iter()
                    .all(|p| p["effect"] == RowValue::Text("allow".into()))
            );
        }
        assert_eq!(builtin_perms(ROLE_KIOSK_DEVICE).len(), 0);
        // Every role and permission row has its own change row.
        let changes = s.changes().unwrap();
        assert_eq!(
            changes.iter().filter(|c| c.tbl == "roles").count(),
            BUILTIN_ROLES.len()
        );
        assert_eq!(
            changes.iter().filter(|c| c.tbl == "role_perms").count(),
            perms.len()
        );
    }

    #[tokio::test]
    async fn each_builtin_role_against_settings_set_and_role_upsert() {
        let f = bootstrapped().await;
        // Kiosk-device is never assigned to a person (its rights are the implicit Device).
        let mut ids = Vec::new();
        for (i, name) in BUILTIN_ROLES.iter().enumerate() {
            if *name == ROLE_KIOSK_DEVICE {
                continue;
            }
            ids.push((*name, add_staff(&f, &format!("staff{i}"), name).await));
        }
        for (i, (name, staff_id)) in ids.iter().enumerate() {
            let allowed = matches!(*name, ROLE_OWNER | ROLE_MANAGER);
            let before = snapshot(&f);
            let set = as_actor(
                staff_actor(staff_id),
                IntentPayload::SettingsSet(SettingsSet {
                    key: "receipt.footer".into(),
                    value: format!("by {name}"),
                }),
                Some(footer_rev(&f)),
            );
            let role = as_actor(
                staff_actor(staff_id),
                custom_role(&format!("Custom {i}"), &[("pos.sell", PermEffect::Allow)]),
                None,
            );
            for intent in [set, role] {
                let kind = intent.kind.as_str();
                let result = f.writer.submit_intent(intent).await;
                if allowed {
                    let applied = result.unwrap_or_else(|e| panic!("{name} {kind}: {e}"));
                    let changes = reopen(&f).changes().unwrap();
                    let c = changes
                        .iter()
                        .find(|c| c.intent_id == applied.intent_id)
                        .unwrap();
                    assert_eq!(
                        (c.actor_kind.as_str(), c.actor_id.as_str()),
                        ("staff", staff_id.as_str())
                    );
                } else {
                    let err = result.unwrap_err();
                    assert_eq!(err.code, ErrorCode::PermissionDenied, "{name} {kind}");
                    assert!(!err.message.contains(staff_id.as_str()));
                    // No rows and no change rows (`changes` covers every table).
                    assert_eq!(snapshot(&f), before, "{name} {kind}: nothing written");
                }
            }
        }
        // An owner actor holds every permission; an admin actor none yet.
        let rev = footer_rev(&f);
        let set = |actor: Actor, rev| {
            as_actor(
                actor,
                IntentPayload::SettingsSet(SettingsSet {
                    key: "receipt.footer".into(),
                    value: "x".into(),
                }),
                Some(rev),
            )
        };
        let admin = Actor {
            kind: ActorKind::Admin,
            id: "admin-1".into(),
        };
        assert_eq!(
            expect_err(&f, set(admin, rev)).await.code,
            ErrorCode::PermissionDenied
        );
        f.writer.submit_intent(set(owner(), rev)).await.unwrap();
    }

    #[tokio::test]
    async fn a_counter_settings_set_is_denied_and_writes_nothing() {
        let f = bootstrapped().await;
        let counter = add_staff(&f, "counter", ROLE_COUNTER).await;
        let before = snapshot(&f);
        let roles_before = reopen(&f).rows(Table::Staff).unwrap();
        let err = expect_err(
            &f,
            as_actor(
                staff_actor(&counter),
                IntentPayload::SettingsSet(SettingsSet {
                    key: "receipt.footer".into(),
                    value: "nope".into(),
                }),
                Some(1),
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(snapshot(&f), before);
        assert_eq!(reopen(&f).rows(Table::Staff).unwrap(), roles_before);
    }

    #[tokio::test]
    async fn custom_roles_allow_deny_and_inactive_staff() {
        let f = bootstrapped().await;
        f.writer
            .submit_intent(as_actor(
                owner(),
                custom_role(
                    "Settings lead",
                    &[
                        ("settings.edit", PermEffect::Allow),
                        ("pos.sell", PermEffect::Deny),
                    ],
                ),
                None,
            ))
            .await
            .unwrap();
        let lead = add_staff(&f, "lead", "Settings lead").await;
        let role = reopen(&f)
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == RowValue::Text("Settings lead".into()))
            .unwrap();
        assert_eq!(role["builtin"], RowValue::Integer(0));
        let set = |rev| {
            as_actor(
                staff_actor(&lead),
                IntentPayload::SettingsSet(SettingsSet {
                    key: "receipt.footer".into(),
                    value: "lead".into(),
                }),
                Some(rev),
            )
        };
        f.writer.submit_intent(set(footer_rev(&f))).await.unwrap();

        // Flip the row to deny: the same staff member is now refused.
        let rid = role_id(&f, "Settings lead");
        let RowValue::Integer(role_rev) = role["rev"] else {
            panic!()
        };
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::RoleUpsert(RoleUpsert {
                    role_id: Some(rid.clone()),
                    name: "Settings lead".into(),
                    perms: vec![RolePerm {
                        permission: "settings.edit".into(),
                        effect: PermEffect::Deny,
                    }],
                }),
                Some(role_rev),
            ))
            .await
            .unwrap();
        let perms: Vec<Row> = reopen(&f)
            .rows(Table::RolePerms)
            .unwrap()
            .into_iter()
            .filter(|p| p["role_id"] == RowValue::Text(rid.clone()))
            .collect();
        assert_eq!(perms.len(), 1, "the rows are replaced");
        assert_eq!(
            expect_err(&f, set(footer_rev(&f))).await.code,
            ErrorCode::PermissionDenied
        );

        // An inactive Manager holds nothing.
        let manager = add_staff(&f, "mgr", ROLE_MANAGER).await;
        let mgr_role = role_id(&f, ROLE_MANAGER);
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(manager.clone()),
                    active: false,
                    ..staff_body("mgr", &mgr_role, None)
                }),
                Some(1),
            ))
            .await
            .unwrap();
        let mut inactive = set(footer_rev(&f));
        inactive.actor = staff_actor(&manager);
        assert_eq!(
            expect_err(&f, inactive).await.code,
            ErrorCode::PermissionDenied
        );
    }

    #[tokio::test]
    async fn builtin_roles_cannot_be_changed() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        let roles_before = reopen(&f).rows(Table::RolePerms).unwrap();
        for name in BUILTIN_ROLES {
            let err = expect_err(
                &f,
                as_actor(
                    owner(),
                    IntentPayload::RoleUpsert(RoleUpsert {
                        role_id: Some(role_id(&f, name)),
                        name: name.into(),
                        perms: vec![],
                    }),
                    Some(1),
                ),
            )
            .await;
            assert_eq!(
                (err.code, err.message.as_str()),
                (
                    ErrorCode::PermissionDenied,
                    "built-in roles cannot be changed"
                ),
                "{name}"
            );
        }
        assert_eq!(snapshot(&f), before);
        assert_eq!(reopen(&f).rows(Table::RolePerms).unwrap(), roles_before);
    }

    #[tokio::test]
    async fn an_unknown_permission_fails_on_its_index() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        let err = expect_err(
            &f,
            as_actor(
                owner(),
                custom_role(
                    "Bad",
                    &[
                        ("pos.sell", PermEffect::Allow),
                        ("nope.x", PermEffect::Allow),
                    ],
                ),
                None,
            ),
        )
        .await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("perms[1]"))
        );
        let err = expect_err(
            &f,
            as_actor(
                owner(),
                custom_role(
                    "Twice",
                    &[
                        ("pos.sell", PermEffect::Allow),
                        ("pos.sell", PermEffect::Deny),
                    ],
                ),
                None,
            ),
        )
        .await;
        assert_eq!(err.field.as_deref(), Some("perms[1]"));
        let s = reopen(&f);
        assert_eq!(
            (s.rows(Table::Branch).unwrap(), s.changes().unwrap()),
            (before.0, before.2)
        );
        assert_eq!(s.rows(Table::Roles).unwrap().len(), BUILTIN_ROLES.len());
        assert_eq!(Permission::parse("nope.x"), None);
    }

    #[tokio::test]
    async fn a_manager_assigns_only_counter_and_kitchen() {
        let f = bootstrapped().await;
        let manager = add_staff(&f, "mgr", ROLE_MANAGER).await;
        let owner_staff = add_staff(&f, "boss", ROLE_OWNER).await;
        for role in [ROLE_OWNER, ROLE_MANAGER] {
            let before = reopen(&f).rows(Table::Staff).unwrap();
            let err = expect_err(
                &f,
                as_actor(
                    staff_actor(&manager),
                    IntentPayload::StaffUpsert(staff_body("new", &role_id(&f, role), Some("pw"))),
                    None,
                ),
            )
            .await;
            assert_eq!(err.code, ErrorCode::PermissionDenied, "{role}");
            assert_eq!(reopen(&f).rows(Table::Staff).unwrap(), before);
        }
        for (i, role) in [ROLE_COUNTER, ROLE_KITCHEN].into_iter().enumerate() {
            f.writer
                .submit_intent(as_actor(
                    staff_actor(&manager),
                    IntentPayload::StaffUpsert(staff_body(
                        &format!("crew{i}"),
                        &role_id(&f, role),
                        Some("pw"),
                    )),
                    None,
                ))
                .await
                .unwrap();
        }
        // Nor may a Manager demote someone who holds another role.
        let err = expect_err(
            &f,
            as_actor(
                staff_actor(&manager),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(owner_staff.clone()),
                    ..staff_body("boss", &role_id(&f, ROLE_COUNTER), None)
                }),
                Some(1),
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        // A staff member with the built-in Owner role may assign any role.
        f.writer
            .submit_intent(as_actor(
                staff_actor(&owner_staff),
                IntentPayload::StaffUpsert(staff_body(
                    "mgr2",
                    &role_id(&f, ROLE_MANAGER),
                    Some("pw"),
                )),
                None,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn staff_upsert_validates_and_rotates_the_pin_version() {
        let f = bootstrapped().await;
        let counter = role_id(&f, ROLE_COUNTER);
        let id = add_staff(&f, "Counter", ROLE_COUNTER).await;
        let fail =
            |body: StaffUpsert, rev| as_actor(owner(), IntentPayload::StaffUpsert(body), rev);
        for (body, rev, field) in [
            (staff_body("counter", &counter, Some("pw")), None, "login"),
            (staff_body("two words", &counter, Some("pw")), None, "login"),
            (staff_body("xx", "nope", Some("pw")), None, "role_id"),
            (staff_body("xx", &counter, None), None, "password_hash"),
            (staff_body("x", &counter, Some("pw")), None, "login"),
            (staff_body("ünï", &counter, Some("pw")), None, "login"),
            (
                staff_body("kiosk", &role_id(&f, ROLE_KIOSK_DEVICE), Some("pw")),
                None,
                "role_id",
            ),
            (
                StaffUpsert {
                    pin_hash: Some("1111".into()),
                    ..staff_body("xx", &counter, Some("pw"))
                },
                None,
                "pin_hash",
            ),
            (
                StaffUpsert {
                    display_name: " ".into(),
                    ..staff_body("xx", &counter, Some("pw"))
                },
                None,
                "display_name",
            ),
            (
                StaffUpsert {
                    staff_id: Some(id.clone()),
                    ..staff_body("Counter", &counter, None)
                },
                None,
                "base_rev",
            ),
        ] {
            let err = expect_err(&f, fail(body, rev)).await;
            assert_eq!(
                (err.code, err.field.as_deref()),
                (ErrorCode::ValidationFailed, Some(field))
            );
        }
        let stale = expect_err(
            &f,
            fail(
                StaffUpsert {
                    staff_id: Some(id.clone()),
                    ..staff_body("Counter", &counter, None)
                },
                Some(9),
            ),
        )
        .await;
        assert_eq!(stale.code, ErrorCode::Stale);
        f.writer
            .submit_intent(fail(
                StaffUpsert {
                    staff_id: Some(id.clone()),
                    pin_hash: Some(hash_secret("3333").unwrap()),
                    ..staff_body("Counter", &counter, None)
                },
                Some(1),
            ))
            .await
            .unwrap();
        let row = reopen(&f).rows(Table::Staff).unwrap().pop().unwrap();
        assert_eq!(row["pin_verifier_version"], RowValue::Integer(2));
        assert_eq!(row["rev"], RowValue::Integer(2));
        assert!(matches!(&row["password_hash"], RowValue::Text(h) if h.starts_with("$argon2id$")));
    }

    #[tokio::test]
    async fn sign_in_fails_the_same_way_for_every_cause() {
        let f = bootstrapped().await;
        let id = add_staff(&f, "counter", ROLE_COUNTER).await;
        let dir = StaffDirectory::open(&f.path).unwrap();
        let ok = dir.sign_in("COUNTER", "pw").unwrap();
        assert_eq!(
            (
                ok.staff_id.as_str(),
                ok.display_name.as_str(),
                ok.role_name.as_str()
            ),
            (id.as_str(), "counter", ROLE_COUNTER)
        );
        let wrong = dir.sign_in("counter", "bad").unwrap_err();
        let unknown = dir.sign_in("nobody", "x").unwrap_err();
        assert_eq!(wrong, unknown);
        assert_eq!(wrong.code, ErrorCode::AuthFailed);
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(id),
                    active: false,
                    ..staff_body("counter", &role_id(&f, ROLE_COUNTER), None)
                }),
                Some(1),
            ))
            .await
            .unwrap();
        assert_eq!(dir.sign_in("counter", "pw").unwrap_err(), unknown);
    }

    #[tokio::test]
    async fn a_manager_cannot_grant_what_it_lacks() {
        let f = bootstrapped().await;
        let manager = add_staff(&f, "mgr", ROLE_MANAGER).await;
        let before = snapshot(&f);
        let err = expect_err(
            &f,
            as_actor(
                staff_actor(&manager),
                custom_role("Restorer", &[("ops.restore", PermEffect::Allow)]),
                None,
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(snapshot(&f), before);
        // Within its own permissions (and a deny of anything) it may.
        f.writer
            .submit_intent(as_actor(
                staff_actor(&manager),
                custom_role(
                    "Till",
                    &[
                        ("pos.sell", PermEffect::Allow),
                        ("ops.restore", PermEffect::Deny),
                    ],
                ),
                None,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_custom_user_manage_role_is_restricted_like_a_manager() {
        let f = bootstrapped().await;
        f.writer
            .submit_intent(as_actor(
                owner(),
                custom_role("Staffer", &[("user.manage", PermEffect::Allow)]),
                None,
            ))
            .await
            .unwrap();
        let staffer = add_staff(&f, "staffer", "Staffer").await;
        let own = role_id(&f, "Staffer");
        // It cannot widen its own role, nor edit it at all.
        for perms in [
            vec![
                ("user.manage", PermEffect::Allow),
                ("settings.edit", PermEffect::Allow),
            ],
            vec![("user.manage", PermEffect::Allow)],
        ] {
            let err = expect_err(
                &f,
                as_actor(
                    staff_actor(&staffer),
                    IntentPayload::RoleUpsert(RoleUpsert {
                        role_id: Some(own.clone()),
                        name: "Staffer".into(),
                        perms: perms
                            .into_iter()
                            .map(|(p, e)| RolePerm {
                                permission: p.into(),
                                effect: e,
                            })
                            .collect(),
                    }),
                    Some(1),
                ),
            )
            .await;
            assert_eq!(err.code, ErrorCode::PermissionDenied);
        }
        // Nor create a role granting more than it holds.
        let err = expect_err(
            &f,
            as_actor(
                staff_actor(&staffer),
                custom_role("Wider", &[("settings.edit", PermEffect::Allow)]),
                None,
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        // It assigns only Counter and Kitchen: not Owner, Manager or a custom role.
        for (i, role) in [ROLE_OWNER, ROLE_MANAGER, "Staffer"]
            .into_iter()
            .enumerate()
        {
            let err = expect_err(
                &f,
                as_actor(
                    staff_actor(&staffer),
                    IntentPayload::StaffUpsert(staff_body(
                        &format!("new{i}"),
                        &role_id(&f, role),
                        Some("pw"),
                    )),
                    None,
                ),
            )
            .await;
            assert_eq!(err.code, ErrorCode::PermissionDenied, "{role}");
        }
        f.writer
            .submit_intent(as_actor(
                staff_actor(&staffer),
                IntentPayload::StaffUpsert(staff_body(
                    "crew",
                    &role_id(&f, ROLE_COUNTER),
                    Some("pw"),
                )),
                None,
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_last_active_owner_stays_an_active_owner() {
        let f = bootstrapped().await;
        let boss = add_staff(&f, "boss", ROLE_OWNER).await;
        let owner_role = role_id(&f, ROLE_OWNER);
        let edit = |role: &str, active: bool, rev| {
            as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(boss.clone()),
                    active,
                    ..staff_body("boss", role, None)
                }),
                Some(rev),
            )
        };
        let count = |f: &Fixture| reopen(f).applied_intent_count().unwrap();
        let n = count(&f);
        let err = expect_err(&f, edit(&owner_role, false, 1)).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("active"))
        );
        let err = expect_err(&f, edit(&role_id(&f, ROLE_MANAGER), true, 1)).await;
        assert_eq!(err.field.as_deref(), Some("role_id"));
        assert_eq!(count(&f), n, "never recorded");
        // With a second active Owner, the first may step down.
        add_staff(&f, "boss2", ROLE_OWNER).await;
        f.writer
            .submit_intent(edit(&owner_role, false, 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn role_names_are_unique_without_case_and_never_built_in() {
        let f = bootstrapped().await;
        f.writer
            .submit_intent(as_actor(owner(), custom_role("Floor lead", &[]), None))
            .await
            .unwrap();
        let n = reopen(&f).applied_intent_count().unwrap();
        let err = expect_err(&f, as_actor(owner(), custom_role("FLOOR LEAD", &[]), None)).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("name_taken"))
        );
        assert_eq!(reopen(&f).applied_intent_count().unwrap(), n);
        for name in ["owner", "KITCHEN", "Kiosk-device"] {
            let err = expect_err(&f, as_actor(owner(), custom_role(name, &[]), None)).await;
            assert_eq!(err.field.as_deref(), Some("name"), "{name}");
        }
    }

    // ---- Story 1.13: inline manager PIN elevation (the apply() rows of the matrix) ----

    /// Registers a staff member with a PIN as the owner; returns the staff id.
    async fn add_staff_pin(f: &Fixture, login: &str, role: &str, pin: &str) -> String {
        let rid = role_id(f, role);
        let applied = f
            .writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    ..staff_body(login, &rid, Some("pw"))
                }),
                None,
            ))
            .await
            .unwrap();
        applied.changes[0].row_id.clone()
    }

    fn pin_by(id: &str, version: i64) -> ElevatedBy {
        ElevatedBy {
            kind: ElevationKind::Pin,
            id: id.into(),
            mode: ElevationMode::Hub,
            verifier_version: version,
        }
    }

    fn currency_rev(f: &Fixture) -> i64 {
        match setting(&reopen(f), "branch.currency")["rev"] {
            RowValue::Integer(n) => n,
            ref other => panic!("rev is {other:?}"),
        }
    }

    /// The Counter's `settings.set` of the currency, elevated by `by`.
    fn counter_sets_currency(f: &Fixture, counter: &str, by: Option<ElevatedBy>) -> Intent {
        Intent {
            elevated_by: by,
            ..as_actor(
                staff_actor(counter),
                IntentPayload::SettingsSet(SettingsSet {
                    key: "branch.currency".into(),
                    value: "USD".into(),
                }),
                Some(currency_rev(f)),
            )
        }
    }

    struct Crew {
        f: Fixture,
        manager: String,
        counter: String,
        kitchen: String,
    }

    async fn crew() -> Crew {
        let f = bootstrapped().await;
        let manager = add_staff_pin(&f, "manager", ROLE_MANAGER, "2222").await;
        let counter = add_staff_pin(&f, "counter", ROLE_COUNTER, "3333").await;
        let kitchen = add_staff_pin(&f, "kitchen", ROLE_KITCHEN, "4444").await;
        Crew {
            f,
            manager,
            counter,
            kitchen,
        }
    }

    #[tokio::test]
    async fn approve_records_the_counter_as_actor_and_the_manager_as_elevator() {
        let c = crew().await;
        let applied =
            c.f.writer
                .submit_intent(counter_sets_currency(
                    &c.f,
                    &c.counter,
                    Some(pin_by(&c.manager, 1)),
                ))
                .await
                .unwrap();
        assert_eq!(applied.changes.len(), 1);
        let change = reopen(&c.f).changes().unwrap().pop().unwrap();
        assert_eq!(
            (change.actor_kind.as_str(), change.actor_id.as_str()),
            ("staff", c.counter.as_str())
        );
        assert_eq!(change.elevated_by_kind.as_deref(), Some("pin"));
        assert_eq!(change.elevated_by_id.as_deref(), Some(c.manager.as_str()));
        assert_eq!(change.elevated_by_mode.as_deref(), Some("hub"));
        assert_eq!(change.elevated_by_verifier_version, Some(1));
        assert_eq!(change.reason, None);
        assert_eq!(
            setting(&reopen(&c.f), "branch.currency")["value"],
            RowValue::Text("USD".into())
        );
    }

    #[tokio::test]
    async fn without_elevation_the_counter_is_denied() {
        let c = crew().await;
        let before = snapshot(&c.f);
        let err = expect_err(&c.f, counter_sets_currency(&c.f, &c.counter, None)).await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(snapshot(&c.f), before);
    }

    #[tokio::test]
    async fn an_elevator_lacking_the_permission_self_or_inactive_is_denied() {
        let c = crew().await;
        for by in [
            pin_by(&c.kitchen, 1),
            pin_by(&c.counter, 1),
            pin_by("01JNOBODYAAAAAAAAAAAAAAAAA", 1),
            ElevatedBy {
                mode: ElevationMode::CachedVerifier,
                ..pin_by(&c.manager, 1)
            },
            ElevatedBy {
                kind: ElevationKind::Mfa,
                ..pin_by(&c.manager, 1)
            },
        ] {
            let err = expect_err(
                &c.f,
                counter_sets_currency(&c.f, &c.counter, Some(by.clone())),
            )
            .await;
            assert_eq!(err.code, ErrorCode::PermissionDenied, "{by:?}");
            assert_eq!(err.message, crate::rbac::denied().message);
        }
        // A manager may not elevate themself either.
        let mut own = counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1)));
        own.actor = staff_actor(&c.manager);
        assert_eq!(
            expect_err(&c.f, own).await.code,
            ErrorCode::PermissionDenied
        );
        // A deactivated manager approves nothing.
        let manager_role = role_id(&c.f, ROLE_MANAGER);
        c.f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(c.manager.clone()),
                    active: false,
                    ..staff_body("manager", &manager_role, None)
                }),
                Some(1),
            ))
            .await
            .unwrap();
        let err = expect_err(
            &c.f,
            counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1))),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(
            setting(&reopen(&c.f), "branch.currency")["value"],
            RowValue::Text("INR".into())
        );
    }

    #[tokio::test]
    async fn a_device_or_deactivated_actor_is_never_elevated() {
        let c = crew().await;
        let secret = crate::devices::new_token_secret().unwrap();
        let hash = crate::devices::hash_token_secret(&secret).unwrap();
        let device =
            c.f.writer
                .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
        let before = snapshot(&c.f);
        // A live device actor with a manager's elevation.
        let mut by_device = counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1)));
        by_device.actor = Actor {
            kind: ActorKind::Device,
            id: device,
        };
        let err = expect_err(&c.f, by_device).await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        // A deactivated Counter with a valid manager elevation.
        let counter_role = role_id(&c.f, ROLE_COUNTER);
        c.f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(c.counter.clone()),
                    active: false,
                    ..staff_body("counter", &counter_role, None)
                }),
                Some(1),
            ))
            .await
            .unwrap();
        let after_deactivate = snapshot(&c.f);
        let err = expect_err(
            &c.f,
            counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1))),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(snapshot(&c.f), after_deactivate);
        assert_ne!(before, after_deactivate);
    }

    #[tokio::test]
    async fn a_rotated_pin_needs_entering_again() {
        let c = crew().await;
        let manager_role = role_id(&c.f, ROLE_MANAGER);
        // The manager's PIN rotates between the elevation and the intent: version 1 -> 2.
        c.f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(c.manager.clone()),
                    pin_hash: Some(hash_secret("5555").unwrap()),
                    ..staff_body("manager", &manager_role, None)
                }),
                Some(1),
            ))
            .await
            .unwrap();
        let before = snapshot(&c.f);
        let err = expect_err(
            &c.f,
            counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1))),
        )
        .await;
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::PinRequired, "enter the PIN again")
        );
        assert_eq!(snapshot(&c.f), before);
        c.f.writer
            .submit_intent(counter_sets_currency(
                &c.f,
                &c.counter,
                Some(pin_by(&c.manager, 2)),
            ))
            .await
            .unwrap();
        // An edit that keeps the PIN keeps the version.
        c.f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(c.manager.clone()),
                    display_name: "Manager Two".into(),
                    ..staff_body("manager", &manager_role, None)
                }),
                Some(2),
            ))
            .await
            .unwrap();
        let row = reopen(&c.f)
            .rows(Table::Staff)
            .unwrap()
            .into_iter()
            .find(|r| r["staff_id"] == RowValue::Text(c.manager.clone()))
            .unwrap();
        assert_eq!(row["pin_verifier_version"], RowValue::Integer(2));
    }

    #[tokio::test]
    async fn a_revoke_needs_a_reason_and_records_it() {
        let c = crew().await;
        let secret = crate::devices::new_token_secret().unwrap();
        let hash = crate::devices::hash_token_secret(&secret).unwrap();
        let device =
            c.f.writer
                .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
                .await
                .unwrap()
                .changes[0]
                .row_id
                .clone();
        let revoke = |reason: Option<ReasonCode>| Intent {
            elevated_by: Some(pin_by(&c.manager, 1)),
            reason,
            ..as_actor(
                staff_actor(&c.counter),
                IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                    device_id: device.clone(),
                }),
                None,
            )
        };
        let before = snapshot(&c.f);
        let err = expect_err(&c.f, revoke(None)).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ReasonRequired, Some("reason"))
        );
        assert_eq!(snapshot(&c.f), before);
        c.f.writer
            .submit_intent(revoke(Some(ReasonCode::Stolen)))
            .await
            .unwrap();
        let change = reopen(&c.f).changes().unwrap().pop().unwrap();
        assert_eq!(change.intent_kind, "device.revoke");
        assert_eq!(change.reason.as_deref(), Some("stolen"));
        assert_eq!(change.actor_id, c.counter);
        assert_eq!(change.elevated_by_id.as_deref(), Some(c.manager.as_str()));
    }

    #[tokio::test]
    async fn match_pin_finds_only_other_staff_holding_the_permission() {
        let c = crew().await;
        let dir = StaffDirectory::open(&c.f.path).unwrap();
        let p = &[Permission::SettingsEdit][..];
        assert_eq!(
            dir.match_pin(p, &c.counter, "2222").unwrap(),
            Some((c.manager.clone(), 1))
        );
        // Wrong PIN, a PIN whose role lacks the permission, the member's own PIN, junk.
        for pin in ["9999", "4444", "12", "abcd", "2222222"] {
            assert_eq!(dir.match_pin(p, &c.counter, pin).unwrap(), None, "{pin}");
        }
        assert_eq!(dir.match_pin(p, &c.manager, "2222").unwrap(), None);
        // Kitchen holds kitchen.use, but nobody else's role does: no candidates at all.
        assert_eq!(
            dir.match_pin(&[Permission::KitchenUse], &c.kitchen, "4444")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_reason_on_a_kind_without_reasons_fails_on_reason() {
        let c = crew().await;
        let mut intent = counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1)));
        intent.reason = Some(ReasonCode::Other);
        let err = expect_err(&c.f, intent).await;
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("reason"))
        );
    }

    // ---- Story 1.20: kiosk admin exit ----

    mod admin_exit_matrix {
        use super::*;
        use arena_proto::{
            ADMIN_EXIT_MS, AdminExitMode, DeviceAdminExit, DeviceAdminRelock, DeviceAdminUnlock,
        };

        const MIN: i64 = 60_000;

        /// The crew plus a registered paired kiosk.
        async fn kiosk_crew() -> (Crew, String) {
            let c = crew().await;
            let hash = crate::devices::hash_token_secret("s".repeat(64).as_str()).unwrap();
            let applied =
                c.f.writer
                    .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
                    .await
                    .unwrap();
            let id = applied.changes[0].row_id.clone();
            (c, id)
        }

        fn verifier(f: &Fixture) -> (i64, Vec<String>) {
            let dir = crate::devices::DeviceDirectory::open(&f.path).unwrap();
            let v = dir.manager_verifier().unwrap();
            (
                v.version,
                v.entries.into_iter().map(|e| e.staff_id).collect(),
            )
        }

        fn exit_by(
            device: &str,
            staff: &str,
            mode: AdminExitMode,
            version: i64,
            at: i64,
        ) -> (Intent, String) {
            let exit_id = ulid::Ulid::generate().to_string();
            let mut i = device_intent(
                ActorKind::Device,
                IntentPayload::DeviceAdminExit(DeviceAdminExit {
                    exit_id: exit_id.clone(),
                    mode,
                }),
            );
            i.actor.id = device.into();
            i.issued_at = at;
            i.elevated_by = Some(ElevatedBy {
                kind: ElevationKind::Pin,
                id: staff.into(),
                mode: match mode {
                    AdminExitMode::CachedVerifier => ElevationMode::CachedVerifier,
                    _ => ElevationMode::Hub,
                },
                verifier_version: version,
            });
            (i, exit_id)
        }

        fn relock(device: &str, exit_id: &str, at: i64) -> Intent {
            let mut i = device_intent(
                ActorKind::Device,
                IntentPayload::DeviceAdminRelock(DeviceAdminRelock {
                    exit_id: exit_id.into(),
                }),
            );
            i.actor.id = device.into();
            i.issued_at = at;
            i
        }

        fn exit_row(f: &Fixture, exit_id: &str) -> arena_store::AdminExitRow {
            crate::devices::DeviceDirectory::open(&f.path)
                .unwrap()
                .admin_exit(exit_id)
                .unwrap()
                .unwrap()
        }

        /// Rotates `staff`'s PIN (a new verifier version), at the clock's time.
        async fn rotate_pin(f: &Fixture, staff: &str, pin: &str) {
            let store = reopen(f);
            let row = store
                .rows(Table::Staff)
                .unwrap()
                .into_iter()
                .find(|r| r["staff_id"] == RowValue::Text(staff.into()))
                .unwrap();
            let text = |c: &str| match &row[c] {
                RowValue::Text(t) => t.clone(),
                other => panic!("{c} is {other:?}"),
            };
            let RowValue::Integer(rev) = row["rev"] else {
                panic!("rev")
            };
            let mut i = as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(staff.into()),
                    login: text("login"),
                    display_name: text("display_name"),
                    role_id: text("role_id"),
                    password_hash: None,
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    active: true,
                }),
                Some(rev),
            );
            i.issued_at = f.clock.now_ms();
            f.writer.submit_intent(i).await.unwrap();
        }

        #[tokio::test]
        async fn the_verifier_lists_device_remote_holders_and_rises_on_every_change() {
            let (c, _) = kiosk_crew().await;
            let (v1, entries) = verifier(&c.f);
            // Owner and Manager hold device.remote; only the manager has a staff row and PIN.
            assert_eq!(entries, vec![c.manager.clone()]);
            assert!(v1 >= 1);
            // An unrelated change keeps the version.
            let rev = match setting(&reopen(&c.f), "receipt.footer")["rev"] {
                RowValue::Integer(n) => n,
                ref o => panic!("{o:?}"),
            };
            c.f.writer
                .submit_intent(set("receipt.footer", "hi", Some(rev)))
                .await
                .unwrap();
            assert_eq!(verifier(&c.f).0, v1);
            // A PIN rotation raises it.
            c.f.clock.advance(MIN);
            rotate_pin(&c.f, &c.manager, "5555").await;
            let (v2, _) = verifier(&c.f);
            assert!(v2 > v1, "{v2} > {v1}");
            // Another manager joins: it rises again and lists both.
            c.f.clock.advance(MIN);
            let second = add_staff_pin(&c.f, "manager2", ROLE_MANAGER, "6666").await;
            let (v3, entries) = verifier(&c.f);
            assert!(v3 > v2);
            assert_eq!(entries.len(), 2);
            assert!(entries.contains(&second));
            // A counter joining changes nothing.
            add_staff_pin(&c.f, "counter2", ROLE_COUNTER, "7777").await;
            assert_eq!(verifier(&c.f).0, v3);
            // The Hub's start brings an out-of-date history up to the staff rows.
            reopen(&c.f)
                .write_tx(|tx| {
                    tx.execute("DELETE FROM verifier_history", [])
                        .map_err(arena_store::db_error)?;
                    Ok(())
                })
                .unwrap();
            assert_eq!(verifier(&c.f).0, 0);
            c.f.writer.seed_defaults().await.unwrap();
            assert!(verifier(&c.f).0 > v3, "never goes down");
        }

        #[tokio::test]
        async fn an_online_exit_is_recorded_with_its_approver_and_relocks() {
            let (c, kiosk) = kiosk_crew().await;
            let (v, _) = verifier(&c.f);
            let (i, exit_id) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0 + MIN);
            let applied = c.f.writer.submit_intent(i).await.unwrap();
            assert_eq!(applied.changes.len(), 1);
            assert_eq!(applied.changes[0].tbl, "device_admin_exits");
            let row = exit_row(&c.f, &exit_id);
            assert_eq!(
                (
                    row.device_id.as_str(),
                    row.staff_id.as_str(),
                    row.mode.as_str()
                ),
                (kiosk.as_str(), c.manager.as_str(), "hub")
            );
            assert_eq!(
                (row.started_at, row.relock_at),
                (T0 + MIN, T0 + MIN + ADMIN_EXIT_MS)
            );
            assert!(!row.flagged);
            let change = reopen(&c.f).changes().unwrap().pop().unwrap();
            assert_eq!(
                (
                    change.actor_kind.as_str(),
                    change.elevated_by_id.as_deref(),
                    change.elevated_by_mode.as_deref()
                ),
                ("device", Some(c.manager.as_str()), Some("hub"))
            );
            // Re-lock (fake clock: 15 minutes later).
            let at = T0 + MIN + ADMIN_EXIT_MS;
            c.f.clock.advance(MIN + ADMIN_EXIT_MS);
            let applied =
                c.f.writer
                    .submit_intent(relock(&kiosk, &exit_id, at))
                    .await
                    .unwrap();
            assert_eq!(applied.changes[0].op, ChangeOp::Update);
            assert_eq!(exit_row(&c.f, &exit_id).relocked_at, Some(at));
            // Twice is refused.
            let err = expect_err(&c.f, relock(&kiosk, &exit_id, at)).await;
            assert_eq!(err.field.as_deref(), Some("exit_id"));
        }

        #[tokio::test]
        async fn an_offline_exit_with_the_current_verifier_is_not_flagged() {
            let (c, kiosk) = kiosk_crew().await;
            let (v, _) = verifier(&c.f);
            let (i, exit_id) = exit_by(
                &kiosk,
                &c.manager,
                AdminExitMode::CachedVerifier,
                v,
                T0 + MIN,
            );
            c.f.writer.submit_intent(i).await.unwrap();
            let row = exit_row(&c.f, &exit_id);
            assert_eq!(row.mode, "cached_verifier");
            assert_eq!((row.flagged, row.flag_reason), (false, None));
            let change = reopen(&c.f).changes().unwrap().pop().unwrap();
            assert_eq!(change.elevated_by_mode.as_deref(), Some("cached_verifier"));
            assert_eq!(change.elevated_by_verifier_version, Some(v));
        }

        #[tokio::test]
        async fn an_offline_exit_with_a_stale_verifier_is_applied_and_flagged() {
            let (c, kiosk) = kiosk_crew().await;
            let (v2, _) = verifier(&c.f);
            // The Hub moves to a newer verifier at T0 + 10 min.
            c.f.clock.advance(10 * MIN);
            add_staff_pin(&c.f, "manager2", ROLE_MANAGER, "6666").await;
            let (v3, _) = verifier(&c.f);
            assert!(v3 > v2);
            // An exit before the change, checked against v2: fine.
            let (early, early_id) = exit_by(
                &kiosk,
                &c.manager,
                AdminExitMode::CachedVerifier,
                v2,
                T0 + 5 * MIN,
            );
            c.f.writer.submit_intent(early).await.unwrap();
            assert!(!exit_row(&c.f, &early_id).flagged);
            // After it, still checked against v2: applied, flagged.
            let (late, late_id) = exit_by(
                &kiosk,
                &c.manager,
                AdminExitMode::CachedVerifier,
                v2,
                T0 + 11 * MIN,
            );
            c.f.writer.submit_intent(late).await.unwrap();
            let row = exit_row(&c.f, &late_id);
            assert!(row.flagged);
            assert_eq!(row.flag_reason.as_deref(), Some("stale_verifier"));
        }

        #[tokio::test]
        async fn a_removed_manager_passes_only_on_a_stale_offline_check() {
            let (c, kiosk) = kiosk_crew().await;
            let (v, _) = verifier(&c.f);
            // The manager is demoted to Counter at T0 + 10 min.
            c.f.clock.advance(10 * MIN);
            let store = reopen(&c.f);
            let row = store
                .rows(Table::Staff)
                .unwrap()
                .into_iter()
                .find(|r| r["staff_id"] == RowValue::Text(c.manager.clone()))
                .unwrap();
            let RowValue::Integer(rev) = row["rev"] else {
                panic!()
            };
            let mut demote = as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: Some(c.manager.clone()),
                    ..staff_body("manager", &role_id(&c.f, ROLE_COUNTER), None)
                }),
                Some(rev),
            );
            demote.issued_at = c.f.clock.now_ms();
            c.f.writer.submit_intent(demote).await.unwrap();
            assert!(verifier(&c.f).0 > v);
            // Offline, with the verifier that still listed them: applied and flagged.
            let (i, id) = exit_by(
                &kiosk,
                &c.manager,
                AdminExitMode::CachedVerifier,
                v,
                T0 + 11 * MIN,
            );
            c.f.writer.submit_intent(i).await.unwrap();
            assert!(exit_row(&c.f, &id).flagged);
            // Checked by the Hub: refused.
            let (now, _) = verifier(&c.f);
            let (i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, now, T0 + 11 * MIN);
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
        }

        #[tokio::test]
        async fn a_stale_offline_check_never_admits_someone_the_verifier_did_not_list() {
            let (c, kiosk) = kiosk_crew().await;
            let (v, _) = verifier(&c.f);
            c.f.clock.advance(10 * MIN);
            add_staff_pin(&c.f, "manager2", ROLE_MANAGER, "6666").await;
            assert!(verifier(&c.f).0 > v);
            // A PIN-less staff member, created after the verifier the kiosk used.
            let rid = role_id(&c.f, ROLE_MANAGER);
            let mut i = as_actor(
                owner(),
                IntentPayload::StaffUpsert(staff_body("nopin", &rid, Some("pw"))),
                None,
            );
            i.issued_at = c.f.clock.now_ms();
            let nopin = c.f.writer.submit_intent(i).await.unwrap().changes[0]
                .row_id
                .clone();
            for who in [&c.counter, &nopin] {
                let (i, _) = exit_by(&kiosk, who, AdminExitMode::CachedVerifier, v, T0 + 11 * MIN);
                assert_eq!(
                    expect_err(&c.f, i).await.code,
                    ErrorCode::PermissionDenied,
                    "{who}"
                );
            }
            // A version that was never recorded admits nobody that way either.
            let (i, _) = exit_by(
                &kiosk,
                &c.counter,
                AdminExitMode::CachedVerifier,
                0,
                T0 + 11 * MIN,
            );
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
        }

        #[tokio::test]
        async fn only_device_remote_holders_approve_and_only_the_kiosk_acts() {
            let (c, kiosk) = kiosk_crew().await;
            let (v, _) = verifier(&c.f);
            // A Counter's PIN (3333) cannot approve.
            for mode in [AdminExitMode::Hub, AdminExitMode::CachedVerifier] {
                let (i, _) = exit_by(&kiosk, &c.counter, mode, v, T0);
                assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            }
            // No elevation, a mismatched mode, or a staff actor: refused.
            let (mut i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0);
            i.elevated_by = None;
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            let (mut i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::CachedVerifier, v, T0);
            if let Some(e) = &mut i.elevated_by {
                e.mode = ElevationMode::Hub;
            }
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            let (mut i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0);
            i.actor = staff_actor(&c.counter);
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            // A remote mode is never the kiosk's own.
            let (mut i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0);
            if let IntentPayload::DeviceAdminExit(p) = &mut i.payload {
                p.mode = AdminExitMode::Remote;
            }
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            // The cached verifier elevates nothing else.
            let mut i = counter_sets_currency(&c.f, &c.counter, Some(pin_by(&c.manager, 1)));
            if let Some(e) = &mut i.elevated_by {
                e.mode = ElevationMode::CachedVerifier;
            }
            assert_eq!(expect_err(&c.f, i).await.code, ErrorCode::PermissionDenied);
            // Another device cannot re-lock this kiosk's exit.
            let (i, exit_id) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0);
            c.f.writer.submit_intent(i).await.unwrap();
            let hash = crate::devices::hash_token_secret(&"t".repeat(64)).unwrap();
            let other =
                c.f.writer
                    .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
                    .await
                    .unwrap()
                    .changes[0]
                    .row_id
                    .clone();
            assert_eq!(
                expect_err(&c.f, relock(&other, &exit_id, T0)).await.code,
                ErrorCode::PermissionDenied
            );
            // An exit id already used is refused.
            let (mut i, _) = exit_by(&kiosk, &c.manager, AdminExitMode::Hub, v, T0);
            if let IntentPayload::DeviceAdminExit(p) = &mut i.payload {
                p.exit_id = exit_id.clone();
            }
            assert_eq!(expect_err(&c.f, i).await.field.as_deref(), Some("exit_id"));
        }

        #[tokio::test]
        async fn a_remote_unlock_records_a_remote_exit_for_a_live_paired_device() {
            let (c, kiosk) = kiosk_crew().await;
            let unlock = |actor: Actor, device: &str| {
                as_actor(
                    actor,
                    IntentPayload::DeviceAdminUnlock(DeviceAdminUnlock {
                        device_id: device.into(),
                    }),
                    None,
                )
            };
            let applied =
                c.f.writer
                    .submit_intent(unlock(staff_actor(&c.manager), &kiosk))
                    .await
                    .unwrap();
            let id = applied.changes[0].row_id.clone();
            let row = exit_row(&c.f, &id);
            assert_eq!(
                (
                    row.mode.as_str(),
                    row.staff_id.as_str(),
                    row.device_id.as_str()
                ),
                ("remote", c.manager.as_str(), kiosk.as_str())
            );
            assert_eq!(row.relock_at - row.started_at, ADMIN_EXIT_MS);
            // A Counter is refused, unless a manager's PIN elevates them.
            assert_eq!(
                expect_err(&c.f, unlock(staff_actor(&c.counter), &kiosk))
                    .await
                    .code,
                ErrorCode::PermissionDenied
            );
            let mut elevated = unlock(staff_actor(&c.counter), &kiosk);
            elevated.elevated_by = Some(pin_by(&c.manager, 1));
            let applied = c.f.writer.submit_intent(elevated).await.unwrap();
            let row = exit_row(&c.f, &applied.changes[0].row_id);
            assert_eq!(row.staff_id, c.manager);
            // The kiosk re-locks a remote exit like any other.
            c.f.clock.advance(ADMIN_EXIT_MS);
            c.f.writer
                .submit_intent(relock(&kiosk, &id, T0 + ADMIN_EXIT_MS))
                .await
                .unwrap();
            // An unknown device is refused.
            let err = expect_err(&c.f, unlock(staff_actor(&c.manager), "01JNOPE")).await;
            assert_eq!(err.field.as_deref(), Some("device_id"));
        }
    }
}

// ---- Story 1.12: branch locale settings, deferred tz/day start, business_day and fy ----

mod locale_matrix {
    use super::*;
    use crate::snapshot::SnapshotReader;
    use chrono::{NaiveDateTime, TimeZone};
    use chrono_tz::Tz;

    /// UTC ms of the wall-clock time `local` in `tz`.
    fn at(tz: Tz, local: &str) -> i64 {
        let naive = NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
        tz.from_local_datetime(&naive).unwrap().timestamp_millis()
    }

    const IST: Tz = Tz::Asia__Kolkata;
    const GST: Tz = Tz::Asia__Dubai;

    fn rev_of(f: &Fixture, key: &str) -> i64 {
        match setting(&reopen(f), key)["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        }
    }

    /// `settings.set` by the owner, issued now, with the row's current `rev`.
    async fn save(f: &Fixture, key: &str, value: &str) -> Result<Applied, AppError> {
        let mut intent = set(key, value, Some(rev_of(f, key)));
        intent.issued_at = f.clock.now_ms();
        f.writer.submit_intent(intent).await
    }

    /// The (business_day, fy) of the newest change row.
    fn last_stamp(f: &Fixture) -> (Option<String>, Option<String>) {
        let c = reopen(f).changes().unwrap().pop().unwrap();
        (c.business_day, c.fy)
    }

    fn day(s: &str) -> (Option<String>, Option<String>) {
        (Some(s.to_owned()), Some("FY2026-27".to_owned()))
    }

    fn text(v: &RowValue) -> Option<&str> {
        match v {
            RowValue::Text(t) => Some(t),
            _ => None,
        }
    }

    #[tokio::test]
    async fn bootstrap_stamps_every_change_with_business_day_and_fy() {
        let f = bootstrapped().await;
        // T0 is 2027-01-15 13:30 in Kolkata.
        let changes = reopen(&f).changes().unwrap();
        assert_eq!(changes.len(), bootstrap_changes());
        for c in &changes {
            assert_eq!(
                (c.business_day.clone(), c.fy.clone()),
                day("2027-01-15"),
                "{}",
                c.row_id
            );
        }
    }

    #[tokio::test]
    async fn bad_currency_and_bad_tz_fail_on_the_value_and_write_nothing() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        for (key, value) in [
            ("branch.currency", "XYZ"),
            ("branch.tz", "Mars/Base"),
            ("branch.symbol_position", "middle"),
            ("branch.number_format", "roman"),
            ("branch.day_start_min", "1440"),
            ("branch.fy_start_month", "13"),
        ] {
            let err = save(&f, key, value).await.unwrap_err();
            assert_eq!(
                (err.code, err.field.as_deref()),
                (ErrorCode::ValidationFailed, Some("value")),
                "{key}={value}"
            );
        }
        let after = snapshot(&f);
        assert_eq!(
            (&after.0, &after.1, &after.2),
            (&before.0, &before.1, &before.2),
            "no rows and no change rows"
        );
    }

    #[tokio::test]
    async fn other_locale_keys_apply_at_once() {
        let f = bootstrapped().await;
        save(&f, "branch.currency", "KWD").await.unwrap();
        save(&f, "branch.symbol_position", "after").await.unwrap();
        save(&f, "branch.number_format", "international")
            .await
            .unwrap();
        let row = setting(&reopen(&f), "branch.currency");
        assert_eq!(row["value"], RowValue::Text("KWD".into()));
        assert_eq!(
            (&row["prior_value"], &row["effective_from"]),
            (&RowValue::Null, &RowValue::Null)
        );
        let locale = SnapshotReader::open(&f.path)
            .unwrap()
            .locale(f.clock.now_ms())
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                locale.currency.as_str(),
                locale.exponent,
                locale.symbol.as_str()
            ),
            ("KWD", 3, "KD")
        );
        assert_eq!(locale.symbol_position, arena_proto::SymbolPosition::After);
        assert_eq!(
            locale.number_format,
            arena_proto::NumberFormat::International
        );
        assert_eq!(locale.pending, None);
    }

    #[tokio::test]
    async fn a_tz_change_during_d_stamps_d_under_the_old_rules_until_d_plus_one() {
        let f = bootstrapped().await;
        let reader = SnapshotReader::open(&f.path).unwrap();
        f.clock.set(at(IST, "2027-01-15 14:00"));
        save(&f, "branch.tz", "Asia/Dubai").await.unwrap();
        let row = setting(&reopen(&f), "branch.tz");
        assert_eq!(text(&row["value"]), Some("Asia/Dubai"));
        assert_eq!(text(&row["prior_value"]), Some("Asia/Kolkata"));
        assert_eq!(text(&row["effective_from"]), Some("2027-01-16"));
        assert_eq!(last_stamp(&f), day("2027-01-15"));
        let now = reader.locale(f.clock.now_ms()).unwrap().unwrap();
        assert_eq!(now.tz, "Asia/Kolkata");
        let pending = now.pending.unwrap();
        assert_eq!(
            pending.tz.map(|t| (t.value, t.effective_from)),
            Some(("Asia/Dubai".to_owned(), "2027-01-16".to_owned()))
        );

        // The rest of D, up to Kolkata's 06:00 day start, is still D.
        for local in ["2027-01-15 23:59", "2027-01-16 05:59"] {
            f.clock.set(at(IST, local));
            save(&f, "receipt.footer", local).await.unwrap();
            assert_eq!(last_stamp(&f), day("2027-01-15"), "{local}");
        }
        // 06:00 in Kolkata (04:30 in Dubai) starts D+1, now under Dubai rules.
        f.clock.set(at(IST, "2027-01-16 06:00"));
        save(&f, "receipt.footer", "d+1").await.unwrap();
        assert_eq!(last_stamp(&f), day("2027-01-16"));
        let now = reader.locale(f.clock.now_ms()).unwrap().unwrap();
        assert_eq!((now.tz.as_str(), now.pending), ("Asia/Dubai", None));
        // Dubai's day start decides from here on.
        f.clock.set(at(GST, "2027-01-17 05:59"));
        save(&f, "receipt.footer", "late").await.unwrap();
        assert_eq!(last_stamp(&f), day("2027-01-16"));
        f.clock.set(at(GST, "2027-01-17 06:00"));
        save(&f, "receipt.footer", "d+2").await.unwrap();
        assert_eq!(last_stamp(&f), day("2027-01-17"));
        // The earlier rows were not restamped.
        let days: Vec<_> = reopen(&f)
            .changes()
            .unwrap()
            .into_iter()
            .rev()
            .take(5)
            .map(|c| c.business_day.unwrap())
            .collect();
        assert_eq!(
            days,
            [
                "2027-01-17",
                "2027-01-16",
                "2027-01-16",
                "2027-01-15",
                "2027-01-15"
            ]
        );
    }

    #[tokio::test]
    async fn a_second_change_while_one_waits_keeps_the_prior_value() {
        let f = bootstrapped().await;
        save(&f, "branch.tz", "Asia/Dubai").await.unwrap();
        save(&f, "branch.tz", "UTC").await.unwrap();
        let row = setting(&reopen(&f), "branch.tz");
        assert_eq!(text(&row["value"]), Some("UTC"));
        assert_eq!(text(&row["prior_value"]), Some("Asia/Kolkata"));
        assert_eq!(text(&row["effective_from"]), Some("2027-01-16"));
        // Back to the value in effect: nothing waits.
        save(&f, "branch.tz", "Asia/Kolkata").await.unwrap();
        let row = setting(&reopen(&f), "branch.tz");
        assert_eq!(text(&row["value"]), Some("Asia/Kolkata"));
        assert_eq!(
            (&row["prior_value"], &row["effective_from"]),
            (&RowValue::Null, &RowValue::Null)
        );
        // A day start change waits as well; once its day has come, a new change waits for
        // the day after that, with the then-current value as its prior.
        save(&f, "branch.day_start_min", "0").await.unwrap();
        let row = setting(&reopen(&f), "branch.day_start_min");
        assert_eq!(text(&row["prior_value"]), Some("360"));
        assert_eq!(text(&row["effective_from"]), Some("2027-01-16"));
        f.clock.set(at(IST, "2027-01-16 12:00"));
        save(&f, "branch.day_start_min", "120").await.unwrap();
        let row = setting(&reopen(&f), "branch.day_start_min");
        assert_eq!(text(&row["value"]), Some("120"));
        assert_eq!(text(&row["prior_value"]), Some("0"));
        assert_eq!(text(&row["effective_from"]), Some("2027-01-17"));
    }

    #[tokio::test]
    async fn a_minute_before_day_start_belongs_to_the_previous_date() {
        let f = bootstrapped().await;
        f.clock.set(at(IST, "2027-01-16 05:59"));
        save(&f, "receipt.footer", "x").await.unwrap();
        assert_eq!(last_stamp(&f), day("2027-01-15"));
        f.clock.set(at(IST, "2027-01-16 06:00"));
        save(&f, "receipt.footer", "y").await.unwrap();
        assert_eq!(last_stamp(&f), day("2027-01-16"));
    }

    #[tokio::test]
    async fn the_financial_year_turns_on_the_start_month() {
        let f = bootstrapped().await;
        f.clock.set(at(IST, "2027-03-31 12:00"));
        save(&f, "receipt.footer", "march").await.unwrap();
        assert_eq!(
            last_stamp(&f),
            (Some("2027-03-31".into()), Some("FY2026-27".into()))
        );
        f.clock.set(at(IST, "2027-04-01 12:00"));
        save(&f, "receipt.footer", "april").await.unwrap();
        assert_eq!(
            last_stamp(&f),
            (Some("2027-04-01".into()), Some("FY2027-28".into()))
        );
        // A new start month waits for the next business day: the 1st keeps one FY label.
        save(&f, "branch.fy_start_month", "1").await.unwrap();
        assert_eq!(
            last_stamp(&f),
            (Some("2027-04-01".into()), Some("FY2027-28".into()))
        );
        let row = setting(&reopen(&f), "branch.fy_start_month");
        assert_eq!(text(&row["effective_from"]), Some("2027-04-02"));
        f.clock.set(at(IST, "2027-04-02 12:00"));
        save(&f, "receipt.footer", "next day").await.unwrap();
        assert_eq!(
            last_stamp(&f),
            (Some("2027-04-02".into()), Some("FY2027".into()))
        );
    }

    #[tokio::test]
    async fn a_stale_locale_save_is_refused_and_changes_nothing() {
        let f = bootstrapped().await;
        save(&f, "branch.currency", "USD").await.unwrap();
        let before = snapshot(&f);
        let mut intent = set("branch.currency", "EUR", Some(1));
        intent.issued_at = f.clock.now_ms();
        let err = f.writer.submit_intent(intent).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Stale);
        let after = snapshot(&f);
        assert_eq!((&after.1, &after.2), (&before.1, &before.2));
    }
}

// ---- branch.seed_defaults: upgraded branches get the rows they lack ----

mod seed_defaults {
    use super::*;

    /// Makes the branch look like one bootstrapped before 0005/0006: no `branch.*` settings and
    /// no roles (or, with `partial`, one built-in permission row gone).
    async fn strip(f: &Fixture, partial: bool) {
        f.writer
            .submit(move |tx| {
                let sql = if partial {
                    "DELETE FROM role_perms WHERE permission = 'settings.edit' AND role_id =
                         (SELECT role_id FROM roles WHERE name = 'Manager')"
                } else {
                    "DELETE FROM settings WHERE key LIKE 'branch.%';
                     DELETE FROM role_perms; DELETE FROM roles;"
                };
                tx.execute_batch(sql).map_err(arena_store::db_error)
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_upgraded_branch_gets_its_locale_keys_and_roles_once() {
        let mut f = bootstrapped().await;
        strip(&f, false).await;
        let before = reopen(&f).changes().unwrap().len();
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        assert_eq!(f.events.try_recv().unwrap(), applied);
        let roles: usize = crate::rbac::BUILTIN_ROLES
            .iter()
            .map(|r| 1 + crate::rbac::builtin_perms(r).len())
            .sum();
        let branch_defaults = crate::DEFAULT_SETTINGS
            .iter()
            .filter(|(k, _)| k.starts_with("branch."))
            .count();
        assert_eq!(branch_defaults, 4);
        assert_eq!(
            applied.changes.len(),
            crate::LOCALE_KEYS.len() + branch_defaults + roles
        );
        let s = reopen(&f);
        let changes = s.changes().unwrap();
        assert_eq!(changes.len(), before + applied.changes.len());
        for c in &changes[before..] {
            assert_eq!(c.intent_kind, "branch.seed_defaults");
            assert_eq!(c.actor_kind, "system");
            assert_eq!(c.op, ChangeOp::Insert);
            assert_eq!(c.business_day.as_deref(), Some("2027-01-15"));
        }
        // The values come from the branch row's columns.
        for (key, value) in [
            ("branch.currency", "INR"),
            ("branch.number_format", "indian"),
            ("branch.tz", "Asia/Kolkata"),
            ("branch.day_start_min", "360"),
            ("branch.fy_start_month", "4"),
        ] {
            assert_eq!(
                setting(&s, key)["value"],
                RowValue::Text(value.into()),
                "{key}"
            );
        }
        assert_eq!(s.rows(Table::Roles).unwrap().len(), 5);
        // Nothing missing now: no intent, no change rows.
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
        assert_eq!(reopen(&f).changes().unwrap().len(), changes.len());
        assert!(f.events.try_recv().is_err());
        // And the seeded keys are editable.
        let mut edit = set("branch.currency", "USD", Some(1));
        edit.issued_at = f.clock.now_ms();
        f.writer.submit_intent(edit).await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_builtin_permission_row_is_added_back() {
        let f = bootstrapped().await;
        strip(&f, true).await;
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        assert_eq!(applied.changes.len(), 1);
        assert_eq!(applied.changes[0].tbl, "role_perms");
        assert!(applied.changes[0].row_id.ends_with("settings.edit"));
    }

    #[tokio::test]
    async fn nothing_missing_or_no_branch_submits_nothing() {
        let f = primary_long_lease();
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
        let f = bootstrapped().await;
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
        // Submitted by hand with nothing missing, the intent is refused and writes nothing.
        let before = snapshot(&f);
        let mut intent = intent_with(ActorKind::System, payload());
        intent.kind = IntentKind::BranchSeedDefaults;
        intent.payload =
            IntentPayload::BranchSeedDefaults(arena_proto::BranchSeedDefaults::default());
        let err = expect_err(&f, intent).await;
        assert_eq!(err.code, ErrorCode::AlreadyBootstrapped);
        let after = snapshot(&f);
        assert_eq!((&after.1, &after.2), (&before.1, &before.2));
        // Only the system may run it.
        let mut intent = intent_with(ActorKind::Owner, payload());
        intent.kind = IntentKind::BranchSeedDefaults;
        intent.payload =
            IntentPayload::BranchSeedDefaults(arena_proto::BranchSeedDefaults::default());
        assert_eq!(
            expect_err(&f, intent).await.code,
            ErrorCode::PermissionDenied
        );
    }
}

// ---- Story 1.18: the kiosk process lists and kiosk.process_blocked ----

mod process_lists {
    use super::*;
    use arena_proto::{
        DEFAULT_PROCESS_ALLOW, DEFAULT_PROCESS_BLOCK, KioskProcessBlocked, ProcessBlockReason,
        ProcessMode,
    };

    fn blocked(device_id: &str, exe: &str, path: &str, reason: ProcessBlockReason) -> Intent {
        let mut i = device_intent(
            ActorKind::Device,
            IntentPayload::KioskProcessBlocked(KioskProcessBlocked {
                exe_name: exe.into(),
                path: path.into(),
                reason,
                mode: ProcessMode::Enforce,
            }),
        );
        i.actor.id = device_id.into();
        i
    }

    #[tokio::test]
    async fn bootstrap_seeds_both_lists_with_the_defaults() {
        let f = bootstrapped().await;
        let s = reopen(&f);
        assert_eq!(
            setting(&s, "kiosk.process_allow")["value"],
            RowValue::Text(DEFAULT_PROCESS_ALLOW.into())
        );
        assert_eq!(
            setting(&s, "kiosk.process_block")["value"],
            RowValue::Text(DEFAULT_PROCESS_BLOCK.into())
        );
        // A new branch enforces at once.
        assert_eq!(
            setting(&s, "kiosk.process_mode")["value"],
            RowValue::Text("enforce".into())
        );
    }

    #[tokio::test]
    async fn an_upgraded_branch_gets_the_lists_from_seed_defaults_once() {
        let mut f = bootstrapped().await;
        f.writer
            .submit(|tx| {
                tx.execute_batch("DELETE FROM settings WHERE key LIKE 'kiosk.%'")
                    .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
        while f.events.try_recv().is_ok() {}
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        let mut ids: Vec<&str> = applied.changes.iter().map(|c| c.row_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids.len(), 4);
        assert!(ids[0].ends_with("/kiosk.process_allow"), "{ids:?}");
        assert!(ids[1].ends_with("/kiosk.process_block"), "{ids:?}");
        assert!(ids[2].ends_with("/kiosk.process_mode"), "{ids:?}");
        assert!(ids[3].ends_with("/kiosk.volume_default"), "{ids:?}");
        assert_eq!(
            setting(&reopen(&f), "kiosk.process_block")["value"],
            RowValue::Text(DEFAULT_PROCESS_BLOCK.into())
        );
        // An upgraded branch only reports until an operator switches it to enforce.
        assert_eq!(
            setting(&reopen(&f), "kiosk.process_mode")["value"],
            RowValue::Text("report".into())
        );
        let mut edit = set("kiosk.process_mode", "enforce", Some(1));
        edit.issued_at = f.clock.now_ms();
        f.writer.submit_intent(edit).await.unwrap();
        let mut edit = set("kiosk.process_mode", "off", Some(2));
        edit.issued_at = f.clock.now_ms();
        assert_eq!(expect_err(&f, edit).await.code, ErrorCode::ValidationFailed);
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
    }

    #[tokio::test]
    async fn settings_set_takes_a_valid_list_and_names_the_bad_rule() {
        let f = bootstrapped().await;
        let games = r#"[{"path_glob": "C:\\Games\\*.exe"}]"#;
        let mut edit = set("kiosk.process_allow", games, Some(1));
        edit.issued_at = f.clock.now_ms();
        f.writer.submit_intent(edit).await.unwrap();
        assert_eq!(
            setting(&reopen(&f), "kiosk.process_allow")["value"],
            RowValue::Text(games.into())
        );
        for bad in [
            "nope",
            "{}",
            "[]",
            r#"[{"path_glob": "game.exe"}]"#,
            r#"[{"path_glob": "C:\\x.exe", "extra": 1}]"#,
            r#"[{"path_glob": "C:\\Users\\*\\x.exe"}]"#,
            r#"[{"path_glob": "C:\\x.exe", "signer": " Valve"}]"#,
            r#"[{"path_glob": "C:\\Kiosk\\{install}\\x.exe"}]"#,
        ] {
            let mut edit = set("kiosk.process_allow", bad, Some(2));
            edit.issued_at = f.clock.now_ms();
            let err = expect_err(&f, edit).await;
            assert_eq!(
                (err.code, err.field.as_deref()),
                (ErrorCode::ValidationFailed, Some("value")),
                "{bad}"
            );
        }
        for bad in [r#"[{"path_glob": "*.exe"}]"#, r#"[{"path_glob": "*"}]"#] {
            let mut edit = set("kiosk.process_block", bad, Some(1));
            edit.issued_at = f.clock.now_ms();
            assert_eq!(
                expect_err(&f, edit).await.code,
                ErrorCode::ValidationFailed,
                "{bad}"
            );
        }
        let one = r#"{"path_glob": "x.exe"}"#;
        let too_many = format!("[{}]", vec![one; 501].join(","));
        let mut edit = set("kiosk.process_block", &too_many, Some(1));
        edit.issued_at = f.clock.now_ms();
        let err = expect_err(&f, edit).await;
        assert!(err.message.contains("at most 500"), "{}", err.message);
        // A list longer than the global 4096-character limit is fine.
        let many = format!("[{}]", vec![one; 400].join(","));
        assert!(many.len() > MAX_SETTING_VALUE_CHARS);
        let mut edit = set("kiosk.process_block", &many, Some(1));
        edit.issued_at = f.clock.now_ms();
        f.writer.submit_intent(edit).await.unwrap();
    }

    #[tokio::test]
    async fn process_blocked_writes_one_device_event_from_a_live_device_only() {
        let mut f = primary();
        let (id, _) = with_device(&mut f).await;
        let mut first = blocked(
            &id,
            "cmd.exe",
            r"C:\Windows\System32\cmd.exe",
            ProcessBlockReason::Blocked,
        );
        first.issued_at = T0 - 30_000;
        let applied = f.writer.submit_intent(first.clone()).await.unwrap();
        assert_eq!(applied.changes.len(), 1);
        assert_eq!(applied.changes[0].tbl, "device_events");
        // Replayed after reconnect: the stored outcome, no second row.
        assert_eq!(f.writer.submit_intent(first).await.unwrap(), applied);
        let store = reopen(&f);
        let rows = store.rows(Table::DeviceEvents).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["code"], RowValue::Text("process_blocked".into()));
        assert_eq!(rows[0]["level"], RowValue::Text("warn".into()));
        assert_eq!(
            rows[0]["detail"],
            RowValue::Text(r"cmd.exe (blocked): C:\Windows\System32\cmd.exe".into())
        );
        assert_eq!(rows[0]["at"], RowValue::Integer(T0 - 30_000));
        let change = store.changes().unwrap().pop().unwrap();
        assert_eq!(
            (change.intent_kind.as_str(), change.actor_kind.as_str()),
            ("kiosk.process_blocked", "device")
        );

        for actor in [ActorKind::Owner, ActorKind::System, ActorKind::Staff] {
            let mut i = blocked(&id, "x.exe", r"C:\x.exe", ProcessBlockReason::NotAllowed);
            i.actor.kind = actor;
            assert_eq!(
                expect_err(&f, i).await.code,
                ErrorCode::PermissionDenied,
                "{actor:?}"
            );
        }
        let err = expect_err(
            &f,
            blocked(
                "01JNOPEAAAAAAAAAAAAAAAAAAA",
                "x.exe",
                r"C:\x.exe",
                ProcessBlockReason::NotAllowed,
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let err = expect_err(
            &f,
            blocked(&id, "", r"C:\x.exe", ProcessBlockReason::Signature),
        )
        .await;
        assert_eq!(err.field.as_deref(), Some("exe_name"));
        let mut edit = blocked(&id, "x.exe", r"C:\x.exe", ProcessBlockReason::Signature);
        edit.base_rev = Some(1);
        assert_eq!(
            expect_err(&f, edit).await.field.as_deref(),
            Some("base_rev")
        );
        assert_eq!(reopen(&f).rows(Table::DeviceEvents).unwrap().len(), 1);

        // Report mode is stored as such; a long path is shortened to the kiosk.log cap.
        let long = format!(r"C:\{}\game.exe", "d".repeat(900));
        let mut report = blocked(&id, "game.exe", &long, ProcessBlockReason::NotAllowed);
        if let IntentPayload::KioskProcessBlocked(p) = &mut report.payload {
            p.mode = ProcessMode::Report;
        }
        f.writer.submit_intent(report).await.unwrap();
        let rows = reopen(&f).rows(Table::DeviceEvents).unwrap();
        let RowValue::Text(detail) = &rows[1]["detail"] else {
            panic!("text detail")
        };
        assert_eq!(detail.chars().count(), crate::MAX_KIOSK_LOG_DETAIL_CHARS);
        assert!(
            detail.starts_with("game.exe (not_allowed, report only): …"),
            "{detail}"
        );
    }
}

// ---- Story 1.21: high contrast per device ----

mod high_contrast {
    use super::*;

    fn value(s: &Store, key: &str) -> Option<RowValue> {
        s.rows(Table::Settings)
            .unwrap()
            .into_iter()
            .find(|r| r["key"] == RowValue::Text(key.into()))
            .map(|r| r["value"].clone())
    }

    #[tokio::test]
    async fn bootstrap_seeds_the_hub_row_off() {
        let f = bootstrapped().await;
        assert_eq!(
            value(&reopen(&f), "display.hc.hub"),
            Some(RowValue::Text("false".into()))
        );
    }

    #[tokio::test]
    async fn settings_set_turns_a_device_and_the_hub_on_with_base_rev() {
        let mut f = primary();
        let (id, _) = with_device(&mut f).await;
        let key = arena_proto::hc_key(&id);
        let mut on = set(&key, "true", Some(1));
        on.issued_at = f.clock.now_ms();
        let applied = f.writer.submit_intent(on).await.unwrap();
        assert_eq!(applied.changes.len(), 1);
        assert_eq!(applied.changes[0].row_id.split_once('/').unwrap().1, key);
        let s = reopen(&f);
        assert_eq!(value(&s, &key), Some(RowValue::Text("true".into())));
        assert_eq!(
            value(&s, "display.hc.hub"),
            Some(RowValue::Text("false".into())),
            "the Hub is unchanged"
        );
        // A stale base_rev, a non-boolean, or a device with no row are refused.
        let mut stale = set(&key, "false", Some(1));
        stale.issued_at = f.clock.now_ms();
        assert_eq!(expect_err(&f, stale).await.code, ErrorCode::Stale);
        let mut bad = set(&key, "yes", Some(2));
        bad.issued_at = f.clock.now_ms();
        assert_eq!(expect_err(&f, bad).await.field.as_deref(), Some("value"));
        let mut unknown = set("display.hc.01JABCDEFGHJKMNPQRSTVWXYZ0", "true", Some(1));
        unknown.issued_at = f.clock.now_ms();
        assert_eq!(expect_err(&f, unknown).await.field.as_deref(), Some("key"));
        let mut hub = set("display.hc.hub", "true", Some(1));
        hub.issued_at = f.clock.now_ms();
        f.writer.submit_intent(hub).await.unwrap();
        assert_eq!(
            value(&reopen(&f), "display.hc.hub"),
            Some(RowValue::Text("true".into()))
        );
    }

    #[tokio::test]
    async fn seed_defaults_adds_rows_for_live_paired_devices_only() {
        let mut f = primary();
        let (live, _) = with_device(&mut f).await;
        let secret = crate::devices::new_token_secret().unwrap();
        let hash = crate::devices::hash_token_secret(&secret).unwrap();
        let gone = f
            .writer
            .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let revoke = IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
            device_id: gone.clone(),
        });
        f.writer
            .submit_intent(device_intent(ActorKind::Owner, revoke))
            .await
            .unwrap();
        // A branch from before Story 1.21: no display.hc.* rows at all.
        f.writer
            .submit(|tx| {
                tx.execute_batch("DELETE FROM settings WHERE key LIKE 'display.hc.%'")
                    .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
        while f.events.try_recv().is_ok() {}
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        let mut keys: Vec<&str> = applied
            .changes
            .iter()
            .map(|c| c.row_id.split_once('/').unwrap().1)
            .collect();
        keys.sort_unstable();
        let live_key = arena_proto::hc_key(&live);
        assert_eq!(keys, [live_key.as_str(), "display.hc.hub"]);
        let s = reopen(&f);
        assert_eq!(value(&s, &live_key), Some(RowValue::Text("false".into())));
        assert_eq!(value(&s, &arena_proto::hc_key(&gone)), None);
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
    }
}

mod operating {
    use super::*;
    use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
    use crate::snapshot::SnapshotReader;
    use arena_proto::{AdConsentState, AdsConsent, AdsConsentSet, SetupStatus, StaffUpsert};

    const HOURS: &str = "branch.opening_hours";
    const CLOSED: &str = r#"{"closed":true}"#;

    fn week(days: [&str; 7]) -> String {
        format!("[{}]", days.join(","))
    }

    fn text(s: &Store, key: &str) -> String {
        match &setting(s, key)["value"] {
            RowValue::Text(t) => t.clone(),
            other => panic!("{key} is {other:?}"),
        }
    }

    fn rev(f: &Fixture, key: &str) -> i64 {
        match setting(&reopen(f), key)["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        }
    }

    fn as_actor(actor: Actor, payload: IntentPayload, base_rev: Option<i64>, now: i64) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: now,
            base_rev,
            actor,
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    async fn save(f: &Fixture, key: &str, value: &str) -> Result<Applied, AppError> {
        let mut intent = set(key, value, Some(rev(f, key)));
        intent.issued_at = f.clock.now_ms();
        f.writer.submit_intent(intent).await
    }

    async fn staff(f: &Fixture, login: &str, role: &str) -> Actor {
        let role_id = reopen(f)
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == RowValue::Text(role.into()))
            .map(|r| match &r["role_id"] {
                RowValue::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap();
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner-1".into(),
        };
        let applied = f
            .writer
            .submit_intent(as_actor(
                owner,
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id,
                    password_hash: Some(crate::staff::hash_secret("pw").unwrap()),
                    pin_hash: None,
                    active: true,
                }),
                None,
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
        Actor {
            kind: ActorKind::Staff,
            id: applied.changes[0].row_id.clone(),
        }
    }

    fn setup(f: &Fixture) -> SetupStatus {
        SnapshotReader::open(&f.path)
            .unwrap()
            .snapshot(f.clock.now_ms())
            .unwrap()
            .setup
            .unwrap()
    }

    #[tokio::test]
    async fn bootstrap_seeds_every_operating_default() {
        let f = bootstrapped().await;
        let s = reopen(&f);
        assert_eq!(text(&s, HOURS), arena_proto::DEFAULT_OPENING_HOURS);
        assert_eq!(text(&s, "branch.opening_hours_set"), "false");
        assert_eq!(text(&s, "branch.fulfilment"), "counter");
        assert_eq!(text(&s, "kiosk.volume_default"), "50");
        assert_eq!(text(&s, "system.backup_dir"), "");
        assert_eq!(
            AdsConsent::parse(&text(&s, "branch.ads_consent")),
            Some(AdsConsent {
                given: false,
                at: None,
                staff_id: None,
                policy_version: None,
            })
        );
    }

    #[tokio::test]
    async fn an_existing_branch_gets_the_operating_rows_from_seed_defaults() {
        let mut f = bootstrapped().await;
        f.writer
            .submit(|tx| {
                tx.execute_batch(
                    "DELETE FROM settings WHERE key IN ('branch.opening_hours',
                     'branch.opening_hours_set', 'branch.fulfilment', 'kiosk.volume_default',
                     'system.backup_dir', 'branch.ads_consent')",
                )
                .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
        while f.events.try_recv().is_ok() {}
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        assert_eq!(applied.changes.len(), 6);
        let s = reopen(&f);
        assert_eq!(text(&s, "branch.fulfilment"), "counter");
        assert_eq!(text(&s, "kiosk.volume_default"), "50");
        assert_eq!(text(&s, HOURS), arena_proto::DEFAULT_OPENING_HOURS);
    }

    #[tokio::test]
    async fn valid_hours_are_stored_and_the_first_save_marks_them_set() {
        let f = bootstrapped().await;
        let mon = week([
            r#"{"open_min":600,"close_min":1380}"#,
            CLOSED,
            CLOSED,
            CLOSED,
            r#"{ "open_min": 1080, "close_min": 120 }"#,
            CLOSED,
            CLOSED,
        ]);
        let applied = save(&f, HOURS, &mon).await.unwrap();
        let keys: Vec<&str> = applied
            .changes
            .iter()
            .map(|c| c.row_id.split_once('/').unwrap().1)
            .collect();
        assert_eq!(keys, [HOURS, "branch.opening_hours_set"]);
        let s = reopen(&f);
        assert_eq!(
            text(&s, HOURS),
            week([
                r#"{"open_min":600,"close_min":1380}"#,
                CLOSED,
                CLOSED,
                CLOSED,
                r#"{"open_min":1080,"close_min":120}"#,
                CLOSED,
                CLOSED,
            ])
        );
        assert_eq!(text(&s, "branch.opening_hours_set"), "true");
        let mut sunday = [CLOSED; 7];
        sunday[6] = r#"{"open_min":0,"close_min":60}"#;
        let again = save(&f, HOURS, &week(sunday)).await.unwrap();
        assert_eq!(again.changes.len(), 1, "the flag is already set");
        assert_eq!(text(&reopen(&f), "branch.opening_hours_set"), "true");
    }

    #[tokio::test]
    async fn bad_hours_name_the_day_or_the_week() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        let mut equal = [CLOSED; 7];
        equal[0] = r#"{"open_min":600,"close_min":600}"#;
        let mut out_of_range = [CLOSED; 7];
        out_of_range[3] = r#"{"open_min":600,"close_min":1440}"#;
        for (value, field) in [
            (week(equal), "branch.opening_hours[0]"),
            (week(out_of_range), "branch.opening_hours[3]"),
            (
                format!("[{}]", [CLOSED; 6].join(",")),
                "branch.opening_hours",
            ),
            ("nope".into(), "branch.opening_hours"),
            (week([CLOSED; 7]), "branch.opening_hours"),
        ] {
            let e = save(&f, HOURS, &value).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{value}");
            assert_eq!(e.field.as_deref(), Some(field), "{value}");
        }
        assert_eq!(snapshot(&f), before);
    }

    #[tokio::test]
    async fn fulfilment_and_volume_bounds() {
        let f = bootstrapped().await;
        save(&f, "branch.fulfilment", "seat").await.unwrap();
        save(&f, "kiosk.volume_default", "0").await.unwrap();
        save(&f, "kiosk.volume_default", "100").await.unwrap();
        for (key, bad) in [
            ("kiosk.volume_default", "101"),
            ("kiosk.volume_default", "-1"),
            ("branch.fulfilment", "table"),
        ] {
            let e = save(&f, key, bad).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{key}={bad}");
            assert_eq!(e.field.as_deref(), Some(key));
        }
        let s = reopen(&f);
        assert_eq!(text(&s, "branch.fulfilment"), "seat");
        assert_eq!(text(&s, "kiosk.volume_default"), "100");
    }

    #[tokio::test]
    async fn consent_and_the_hours_flag_are_never_raw_settings() {
        let f = bootstrapped().await;
        for (key, value) in [
            (
                "branch.ads_consent",
                r#"{"given":true,"at":1,"staff_id":"x"}"#,
            ),
            ("branch.opening_hours_set", "true"),
        ] {
            let e = save(&f, key, value).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.field.as_deref(), Some("key"), "{key}");
        }
    }

    #[tokio::test]
    async fn ads_consent_records_the_staff_actor_and_the_time() {
        let f = bootstrapped().await;
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        f.clock.set(T0 + 5_000);
        let r = rev(&f, "branch.ads_consent");
        let applied = f
            .writer
            .submit_intent(as_actor(
                manager.clone(),
                IntentPayload::AdsConsent(AdsConsentSet {
                    given: true,
                    policy_version: arena_proto::ADS_POLICY_VERSION,
                }),
                Some(r),
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
        assert_eq!(applied.changes.len(), 1);
        let s = reopen(&f);
        assert_eq!(
            AdsConsent::parse(&text(&s, "branch.ads_consent")),
            Some(AdsConsent {
                given: true,
                at: Some(T0 + 5_000),
                staff_id: Some(manager.id.clone()),
                policy_version: Some(arena_proto::ADS_POLICY_VERSION),
            })
        );
        let c = s.changes().unwrap().pop().unwrap();
        assert_eq!(c.intent_kind, "ads.consent");
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("staff", manager.id.as_str())
        );
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner-1".into(),
        };
        let e = f
            .writer
            .submit_intent(as_actor(
                owner,
                IntentPayload::AdsConsent(AdsConsentSet {
                    given: true,
                    policy_version: arena_proto::ADS_POLICY_VERSION,
                }),
                Some(r + 1),
                f.clock.now_ms(),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("actor"));
        let old_policy = f
            .writer
            .submit_intent(as_actor(
                manager.clone(),
                IntentPayload::AdsConsent(AdsConsentSet {
                    given: true,
                    policy_version: arena_proto::ADS_POLICY_VERSION + 1,
                }),
                Some(r + 1),
                f.clock.now_ms(),
            ))
            .await
            .unwrap_err();
        assert_eq!(old_policy.field.as_deref(), Some("policy_version"));
        let stale = f
            .writer
            .submit_intent(as_actor(
                manager,
                IntentPayload::AdsConsent(AdsConsentSet {
                    given: false,
                    policy_version: arena_proto::ADS_POLICY_VERSION,
                }),
                Some(r),
                f.clock.now_ms(),
            ))
            .await
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::Stale);
    }

    #[tokio::test]
    async fn a_counter_cannot_change_operating_settings_or_consent() {
        let f = bootstrapped().await;
        let counter = staff(&f, "counter", ROLE_COUNTER).await;
        let before = snapshot(&f);
        for payload in [
            IntentPayload::SettingsSet(SettingsSet {
                key: "branch.fulfilment".into(),
                value: "seat".into(),
            }),
            IntentPayload::SettingsSet(SettingsSet {
                key: "kiosk.volume_default".into(),
                value: "10".into(),
            }),
            IntentPayload::SettingsSet(SettingsSet {
                key: HOURS.into(),
                value: week([CLOSED; 7]),
            }),
            IntentPayload::AdsConsent(AdsConsentSet {
                given: true,
                policy_version: arena_proto::ADS_POLICY_VERSION,
            }),
        ] {
            let e = f
                .writer
                .submit_intent(as_actor(
                    counter.clone(),
                    payload,
                    Some(1),
                    f.clock.now_ms(),
                ))
                .await
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::PermissionDenied);
        }
        assert_eq!(snapshot(&f), before);
    }

    #[tokio::test]
    async fn setup_status_goes_from_defaults_to_done() {
        let f = bootstrapped().await;
        assert_eq!(
            setup(&f),
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: false,
                ad_consent: AdConsentState::Missing,
            }
        );
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        save(
            &f,
            HOURS,
            &week([r#"{"open_min":600,"close_min":1380}"#; 7]),
        )
        .await
        .unwrap();
        save(&f, "system.backup_dir", "/mnt/backup").await.unwrap();
        assert_eq!(
            setup(&f),
            SetupStatus {
                opening_hours_set: true,
                backup_location_set: true,
                ad_consent: AdConsentState::Missing,
            }
        );
        f.writer
            .submit_intent(as_actor(
                manager,
                IntentPayload::AdsConsent(AdsConsentSet {
                    given: true,
                    policy_version: arena_proto::ADS_POLICY_VERSION,
                }),
                Some(rev(&f, "branch.ads_consent")),
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
        assert_eq!(
            setup(&f),
            SetupStatus {
                opening_hours_set: true,
                backup_location_set: true,
                ad_consent: AdConsentState::Given,
            }
        );
        save(&f, "system.backup_dir", "").await.unwrap();
        assert!(!setup(&f).backup_location_set);
        let relative = save(&f, "system.backup_dir", "backups").await.unwrap_err();
        assert_eq!(relative.code, ErrorCode::ValidationFailed);
        assert_eq!(relative.field.as_deref(), Some("system.backup_dir"));
        assert_eq!(relative.message, "not_found");
    }

    #[tokio::test]
    async fn the_published_setup_status_follows_settings_changes() {
        let f = bootstrapped().await;
        let reader = std::sync::Arc::new(SnapshotReader::open(&f.path).unwrap());
        let job = tokio::spawn(crate::setup::publish_setup(
            reader,
            Some(f.writer.subscribe()),
            f.status.clone(),
        ));
        let mut rx = f.status.subscribe();
        let wait = |rx: &mut watch::Receiver<HubStatus>, want: SetupStatus| {
            let mut rx = rx.clone();
            async move {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    rx.wait_for(|s| s.setup == Some(want)),
                )
                .await
                .expect("published")
                .unwrap();
            }
        };
        wait(
            &mut rx,
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: false,
                ad_consent: AdConsentState::Missing,
            },
        )
        .await;
        save(&f, "system.backup_dir", "/mnt/backup").await.unwrap();
        wait(
            &mut rx,
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: true,
                ad_consent: AdConsentState::Missing,
            },
        )
        .await;
        job.abort();
    }
}

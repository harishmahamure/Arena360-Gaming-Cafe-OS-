//! One test per row of the Story 1.7 apply matrix (the append-only and replay rows live in
//! `arena-store` and `arena-replay`), then one per row of the Story 1.8 dedupe/TTL/base_rev
//! matrix (the prune row lives in `arena-tasks`, the property row in `tests/proptest_apply.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ElevatedBy, ElevationMode, ErrorCode, Intent,
    IntentKind, IntentPayload, SettingsSet,
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

/// How many change rows `branch.bootstrap` writes: the branch, its settings, the built-in
/// roles and their permission rows.
fn bootstrap_changes() -> usize {
    1 + DEFAULT_SETTINGS.len()
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
    assert_eq!(err.code, ErrorCode::PermissionDenied);
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
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "devices");
    assert_eq!(applied.changes[0].op, ChangeOp::Insert);
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
    let change = store.changes().unwrap().pop().unwrap();
    assert_eq!(change.intent_kind, "device.register");
    let logged: Row = rmp_serde::from_slice(&change.row).unwrap();
    assert_eq!(logged, devices[0]);
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
}

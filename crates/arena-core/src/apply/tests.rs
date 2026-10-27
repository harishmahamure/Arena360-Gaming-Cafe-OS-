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

mod commands;
mod plans;
mod players;
mod prepaid;
mod rates;
mod session;
mod session_expire;
mod stock;
mod test_session;

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
        onboarding: None,
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
        + 3
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
        DEFAULT_SETTINGS.len() + crate::LOCALE_KEYS.len() + 3
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
            ("branch.code".into(), "DEV01".into()),
            ("branch.code_locked_at".into(), "".into()),
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
    let f = bootstrapped_free().await;
    user_cap(&f, 50).await;
    f
}

async fn bootstrapped_free() -> Fixture {
    let mut f = primary_long_lease();
    f.writer.submit_intent(bootstrap()).await.unwrap();
    f.events.try_recv().unwrap();
    f
}

async fn user_cap(f: &Fixture, n: i64) {
    let sql = format!(
        "INSERT INTO settings (branch_id, key, value, rev, updated_at)
         SELECT branch_id, 'licence.user_cap', '{n}', 0, 0 FROM branch WHERE true
         ON CONFLICT (branch_id, key) DO UPDATE SET value = excluded.value"
    );
    f.writer
        .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
        .await
        .unwrap();
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
        device_id: None,
        device_kind: Default::default(),
        class: arena_proto::ClientClass::Paired,
        label: "Dev Kiosk".into(),
        token_hash: hash.into(),
        tv_number: None,
        group_id: None,
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
    assert_eq!(applied.changes.len(), 3);
    assert_eq!(applied.changes[0].tbl, "devices");
    assert_eq!(applied.changes[0].op, ChangeOp::Insert);
    assert_eq!(applied.changes[1].tbl, "devices_meta");
    assert_eq!(applied.changes[1].op, ChangeOp::Insert);
    assert_eq!(applied.changes[2].tbl, "settings");
    assert_eq!(applied.changes[2].op, ChangeOp::Insert);
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
    let meta = changes.pop().unwrap();
    let change = changes.pop().unwrap();
    assert_eq!(change.intent_kind, "device.register");
    assert_eq!(
        (meta.tbl.as_str(), meta.row_id.as_str()),
        ("devices_meta", id.as_str())
    );
    let meta_rows = store.rows(Table::DevicesMeta).unwrap();
    let logged_meta: Row = rmp_serde::from_slice(&meta.row).unwrap();
    assert_eq!(logged_meta, meta_rows[0]);
    assert_eq!(meta_rows[0]["kind"], RowValue::Text("pc".into()));
    assert_eq!(meta_rows[0]["label"], RowValue::Text("Dev Kiosk".into()));
    assert_eq!(meta_rows[0]["group_id"], RowValue::Null);
    assert_eq!(meta_rows[0]["rev"], RowValue::Integer(1));
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
    for actor in [
        ActorKind::Device,
        ActorKind::Staff,
        ActorKind::Admin,
        ActorKind::System,
    ] {
        let err = expect_err(&f, device_intent(actor, register(&hash))).await;
        assert_eq!(err.code, ErrorCode::PermissionDenied, "{actor:?}");
    }
    let err = expect_err(&f, device_intent(ActorKind::Owner, register("plain"))).await;
    assert_eq!(err.field.as_deref(), Some("token_hash"));
    let phone = IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
        device_id: None,
        device_kind: Default::default(),
        class: arena_proto::ClientClass::Phone,
        label: "x".into(),
        token_hash: hash.clone(),
        tv_number: None,
        group_id: None,
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
            setup_code_hash: None,
            setup_expires_at: None,
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
            let taken = body.login == "counter";
            let err = expect_err(&f, fail(body, rev)).await;
            let code = if taken {
                ErrorCode::LoginTaken
            } else {
                ErrorCode::ValidationFailed
            };
            assert_eq!((err.code, err.field.as_deref()), (code, Some(field)));
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
    async fn a_counter_saves_billing_rules_only_with_a_managers_pin() {
        let c = crew().await;
        let rule = |key: &str| {
            let r = match setting(&reopen(&c.f), key)["rev"] {
                RowValue::Integer(n) => n,
                _ => unreachable!(),
            };
            (key.to_owned(), r)
        };
        let edit = |key: &str, value: &str, by: Option<ElevatedBy>| {
            let (key, r) = rule(key);
            Intent {
                elevated_by: by,
                ..as_actor(
                    staff_actor(&c.counter),
                    IntentPayload::SettingsSet(SettingsSet {
                        key,
                        value: value.into(),
                    }),
                    Some(r),
                )
            }
        };
        let values = [
            ("billing.rounding_min", "10"),
            ("billing.minimum_min", "0"),
            ("billing.grace_min", "10"),
            ("billing.time_up_mode", "staff_handles"),
        ];
        let before = snapshot(&c.f);
        for (key, value) in values {
            let e = expect_err(&c.f, edit(key, value, None)).await;
            assert_eq!(e.code, ErrorCode::PermissionDenied, "{key}");
        }
        assert_eq!(snapshot(&c.f), before);
        for (key, value) in values {
            c.f.writer
                .submit_intent(edit(key, value, Some(pin_by(&c.manager, 1))))
                .await
                .unwrap();
        }
        let s = reopen(&c.f);
        let changes = s.changes().unwrap();
        for (key, value) in values {
            assert_eq!(setting(&s, key)["value"], RowValue::Text(value.into()));
        }
        for change in &changes[changes.len() - values.len()..] {
            assert_eq!(change.intent_kind, "settings.set");
            assert_eq!(
                (change.actor_kind.as_str(), change.actor_id.as_str()),
                ("staff", c.counter.as_str())
            );
            assert_eq!(change.elevated_by_id.as_deref(), Some(c.manager.as_str()));
        }
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

    async fn pc(c: &Crew) -> String {
        let hash = crate::devices::hash_token_secret("s").unwrap();
        c.f.writer
            .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    fn pc_cmd(c: &Crew, body: IntentPayload, by: Option<ElevatedBy>) -> Intent {
        Intent {
            elevated_by: by,
            ..as_actor(staff_actor(&c.counter), body, None)
        }
    }

    fn screen(device: &str, action: arena_proto::PcAction, text: Option<&str>) -> IntentPayload {
        IntentPayload::DeviceScreen(arena_proto::DeviceScreen {
            device_id: device.into(),
            action,
            text: text.map(Into::into),
        })
    }

    fn power(device: &str, action: arena_proto::PcAction) -> IntentPayload {
        IntentPayload::DevicePower(arena_proto::DevicePower {
            device_id: device.into(),
            action,
        })
    }

    #[tokio::test]
    async fn a_counter_locks_and_messages_a_pc_and_the_audit_row_names_the_command() {
        use arena_proto::PcAction::{Lock, Message};
        let c = crew().await;
        let d = pc(&c).await;
        let lock =
            c.f.writer
                .submit_intent(pc_cmd(&c, screen(&d, Lock, None), None))
                .await
                .unwrap();
        assert_eq!(lock.changes[0].tbl, "device_events");
        c.f.writer
            .submit_intent(pc_cmd(
                &c,
                screen(&d, Message, Some(&"é".repeat(140))),
                None,
            ))
            .await
            .unwrap();
        let events = reopen(&c.f).rows(Table::DeviceEvents).unwrap();
        let codes: Vec<_> = events.iter().map(|e| e["code"].clone()).collect();
        assert_eq!(
            codes,
            [
                RowValue::Text("pc.lock".into()),
                RowValue::Text("pc.message".into())
            ]
        );
        let change = reopen(&c.f).changes().unwrap().pop().unwrap();
        assert_eq!(change.intent_kind, "device.screen");
        assert_eq!(change.actor_id, c.counter);
    }

    #[tokio::test]
    async fn a_message_is_one_to_140_characters_and_a_pc_is_required() {
        use arena_proto::PcAction::{Lock, Message, Restart};
        let c = crew().await;
        let d = pc(&c).await;
        for (body, field) in [
            (screen(&d, Message, None), "text"),
            (screen(&d, Message, Some("  ")), "text"),
            (screen(&d, Message, Some(&"x".repeat(141))), "text"),
            (screen(&d, Lock, Some("hi")), "text"),
            (screen(&d, Restart, None), "action"),
            (screen("nope", Lock, None), "device_id"),
        ] {
            let e = expect_err(&c.f, pc_cmd(&c, body, None)).await;
            assert_eq!(
                (e.code, e.field.as_deref()),
                (ErrorCode::ValidationFailed, Some(field))
            );
        }
        let mut with_rev = pc_cmd(&c, screen(&d, Lock, None), None);
        with_rev.base_rev = Some(1);
        let e = expect_err(&c.f, with_rev).await;
        assert_eq!(e.field.as_deref(), Some("base_rev"));
        assert!(reopen(&c.f).rows(Table::DeviceEvents).unwrap().is_empty());
    }

    #[tokio::test]
    async fn restart_needs_device_remote_so_a_counter_elevates_and_both_actors_are_recorded() {
        use arena_proto::PcAction::{Lock, Restart};
        let c = crew().await;
        let d = pc(&c).await;
        let e = expect_err(&c.f, pc_cmd(&c, power(&d, Restart), None)).await;
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let e = expect_err(
            &c.f,
            pc_cmd(&c, power(&d, Lock), Some(pin_by(&c.manager, 1))),
        )
        .await;
        assert_eq!(e.field.as_deref(), Some("action"));
        let e = expect_err(
            &c.f,
            pc_cmd(&c, power(&d, Restart), Some(pin_by(&c.kitchen, 1))),
        )
        .await;
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let mut ok = pc_cmd(&c, power(&d, Restart), Some(pin_by(&c.manager, 1)));
        c.f.writer.submit_intent(ok.clone()).await.unwrap();
        let change = reopen(&c.f).changes().unwrap().pop().unwrap();
        assert_eq!(change.intent_kind, "device.power");
        assert_eq!(change.actor_id, c.counter);
        assert_eq!(change.elevated_by_id.as_deref(), Some(c.manager.as_str()));
        let again = c.f.writer.submit_intent(ok.clone()).await.unwrap();
        assert_eq!(again.changes.len(), 1);
        assert_eq!(reopen(&c.f).rows(Table::DeviceEvents).unwrap().len(), 1);
        ok.elevated_by = None;
        let e = expect_err(&c.f, ok).await;
        assert_eq!(e.code, ErrorCode::PermissionDenied);
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
                    setup_code_hash: None,
                    setup_expires_at: None,
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

mod pairing_register {
    use super::*;
    use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
    use crate::staff::hash_secret;
    use arena_proto::{ClientClass, DeviceKind, DeviceRegister, StaffUpsert};

    fn actor(kind: ActorKind, id: &str) -> Actor {
        Actor {
            kind,
            id: id.into(),
        }
    }

    fn intent(actor: Actor, body: DeviceRegister, base_rev: Option<i64>) -> Intent {
        Intent {
            kind: IntentKind::DeviceRegister,
            payload: IntentPayload::DeviceRegister(body),
            actor,
            base_rev,
            ..bootstrap()
        }
    }

    fn pc(label: &str) -> DeviceRegister {
        DeviceRegister::new(
            ClientClass::Paired,
            label,
            crate::devices::hash_token_secret("s").unwrap(),
        )
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

    async fn add_staff(f: &Fixture, login: &str, role: &str, pin: &str) -> String {
        let applied = f
            .writer
            .submit_intent(Intent {
                kind: IntentKind::StaffUpsert,
                payload: IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: role_id(f, role),
                    password_hash: Some(hash_secret("pw").unwrap()),
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                actor: actor(ActorKind::Owner, "owner-1"),
                ..bootstrap()
            })
            .await
            .unwrap();
        applied.changes[0].row_id.clone()
    }

    fn pin_of(id: &str) -> ElevatedBy {
        ElevatedBy {
            kind: ElevationKind::Pin,
            id: id.into(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        }
    }

    #[tokio::test]
    async fn the_system_pairs_a_pc_and_a_manager_pin_is_recorded_as_elevated_by() {
        let f = bootstrapped().await;
        let manager = add_staff(&f, "mgr", ROLE_MANAGER, "2222").await;
        let counter = add_staff(&f, "ctr", ROLE_COUNTER, "3333").await;

        let by_code = f
            .writer
            .submit_intent(intent(
                actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
                pc("PC-01"),
                None,
            ))
            .await
            .unwrap();
        let id = by_code.changes[0].row_id.clone();
        let store = reopen(&f);
        let meta = store.device_meta(&id).unwrap().unwrap();
        assert_eq!(
            (
                meta.kind.as_str(),
                meta.label.as_str(),
                meta.group_id,
                meta.rev
            ),
            ("pc", "PC-01", None, 1)
        );
        let change = store
            .changes()
            .unwrap()
            .into_iter()
            .find(|c| c.tbl == "devices_meta")
            .unwrap();
        assert_eq!(
            (change.actor_kind.as_str(), change.actor_id.as_str()),
            ("system", arena_proto::PAIR_SYSTEM_ACTOR)
        );
        assert_eq!(change.elevated_by_id, None);

        let mut by_pin = intent(
            actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
            pc("PC-02"),
            None,
        );
        by_pin.elevated_by = Some(pin_of(&manager));
        let applied = f.writer.submit_intent(by_pin).await.unwrap();
        let change = reopen(&f)
            .changes()
            .unwrap()
            .into_iter()
            .find(|c| c.tbl == "devices_meta" && c.intent_id == applied.intent_id)
            .unwrap();
        assert_eq!(change.elevated_by_kind.as_deref(), Some("pin"));
        assert_eq!(change.elevated_by_id.as_deref(), Some(manager.as_str()));
        assert_eq!(change.elevated_by_mode.as_deref(), Some("hub"));

        let mut by_counter = intent(
            actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
            pc("PC-03"),
            None,
        );
        by_counter.elevated_by = Some(pin_of(&counter));
        assert_eq!(
            expect_err(&f, by_counter).await.code,
            ErrorCode::PermissionDenied
        );
        let mut stale_pin = intent(
            actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
            pc("PC-03"),
            None,
        );
        stale_pin.elevated_by = Some(ElevatedBy {
            verifier_version: 9,
            ..pin_of(&manager)
        });
        assert_eq!(expect_err(&f, stale_pin).await.code, ErrorCode::PinRequired);
    }

    #[tokio::test]
    async fn device_manage_is_needed_and_edits_check_base_rev_and_log_a_change() {
        let f = bootstrapped().await;
        let manager = add_staff(&f, "mgr", ROLE_MANAGER, "2222").await;
        let counter = add_staff(&f, "ctr", ROLE_COUNTER, "3333").await;
        let err = expect_err(
            &f,
            intent(actor(ActorKind::Staff, &counter), pc("PC-09"), None),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let created = f
            .writer
            .submit_intent(intent(actor(ActorKind::Staff, &manager), pc("PC-09"), None))
            .await
            .unwrap();
        let id = created.changes[0].row_id.clone();

        let pairing_edit = DeviceRegister {
            device_id: Some(id.clone()),
            device_kind: DeviceKind::Pc,
            label: "Taken over".into(),
            token_hash: String::new(),
            tv_number: None,
            group_id: None,
            class: ClientClass::Paired,
        };
        let err = expect_err(
            &f,
            intent(
                actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
                pairing_edit,
                Some(1),
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let err = expect_err(
            &f,
            intent(actor(ActorKind::System, "hub-lan"), pc("PC-10"), None),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);

        let edit = |label: &str, kind: DeviceKind| DeviceRegister {
            device_id: Some(id.clone()),
            device_kind: kind,
            label: label.into(),
            token_hash: String::new(),
            tv_number: None,
            group_id: None,
            class: ClientClass::Paired,
        };
        let err = expect_err(
            &f,
            intent(
                actor(ActorKind::Staff, &manager),
                edit("Desk", DeviceKind::Pc),
                None,
            ),
        )
        .await;
        assert_eq!(err.field.as_deref(), Some("base_rev"));
        let err = expect_err(
            &f,
            intent(
                actor(ActorKind::Staff, &manager),
                edit("Desk", DeviceKind::Pc),
                Some(7),
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::Stale);
        let err = expect_err(
            &f,
            intent(
                actor(ActorKind::Staff, &counter),
                edit("Desk", DeviceKind::Pc),
                Some(1),
            ),
        )
        .await;
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let mut with_token = edit("Desk", DeviceKind::Pc);
        with_token.token_hash = crate::devices::hash_token_secret("x").unwrap();
        let err = expect_err(
            &f,
            intent(actor(ActorKind::Staff, &manager), with_token, Some(1)),
        )
        .await;
        assert_eq!(err.field.as_deref(), Some("token_hash"));

        let stamps = |f: &Fixture| {
            let row = reopen(f)
                .rows(Table::DevicesMeta)
                .unwrap()
                .into_iter()
                .find(|r| r["device_id"] == RowValue::Text(id.clone()))
                .unwrap();
            (
                row["rev"].clone(),
                row["created_at"].clone(),
                row["updated_at"].clone(),
            )
        };
        let now = f.clock.now_ms();
        assert_eq!(
            stamps(&f),
            (
                RowValue::Integer(1),
                RowValue::Integer(now),
                RowValue::Integer(now)
            )
        );
        f.clock.advance(5);
        let applied = f
            .writer
            .submit_intent(intent(
                actor(ActorKind::Staff, &manager),
                edit("Desk", DeviceKind::Console),
                Some(1),
            ))
            .await
            .unwrap();
        assert_eq!(
            stamps(&f),
            (
                RowValue::Integer(2),
                RowValue::Integer(now),
                RowValue::Integer(now + 5)
            )
        );
        let tables: Vec<(&str, ChangeOp)> = applied
            .changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op))
            .collect();
        assert_eq!(
            tables,
            [
                ("devices_meta", ChangeOp::Update),
                ("devices", ChangeOp::Update)
            ]
        );
        let store = reopen(&f);
        let meta = store.device_meta(&id).unwrap().unwrap();
        assert_eq!(
            (meta.kind.as_str(), meta.label.as_str(), meta.rev),
            ("console", "Desk", 2)
        );
        assert_eq!(store.device(&id).unwrap().unwrap().label, "Desk");
        let logged = store
            .changes()
            .unwrap()
            .into_iter()
            .rev()
            .find(|c| c.tbl == "devices_meta")
            .unwrap();
        assert_eq!(logged.op, ChangeOp::Update);
        assert_eq!(logged.actor_id, manager);
        let row: Row = rmp_serde::from_slice(&logged.row).unwrap();
        assert_eq!(row["kind"], RowValue::Text("console".into()));
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
    async fn a_branch_name_change_applies_at_once_and_bad_names_are_refused() {
        let f = bootstrapped().await;
        save(&f, "branch.name", "Pixel Arena 2").await.unwrap();
        let summary = reopen(&f).branch_summary().unwrap().unwrap();
        assert_eq!(summary.name, "Pixel Arena 2");
        for bad in ["", " x", "x ", "a\nb", &"x".repeat(81)] {
            let e = save(&f, "branch.name", bad).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{bad:?}");
        }
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
        assert_eq!(branch_defaults, 5);
        assert_eq!(
            applied.changes.len(),
            crate::LOCALE_KEYS.len() + 3 + branch_defaults + roles
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
                    setup_code_hash: None,
                    setup_expires_at: None,
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
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
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
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: true,
                test_session: Default::default(),
                live: false,
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
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: true,
                test_session: Default::default(),
                live: false,
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
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
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
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
            },
        )
        .await;
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        f.writer
            .submit_intent(as_actor(
                manager,
                IntentPayload::PrinterStatus(arena_proto::PrinterStatusSet {
                    state: arena_proto::PrinterState::Skipped,
                    error_kind: None,
                    error: None,
                }),
                None,
                f.clock.now_ms(),
            ))
            .await
            .unwrap();
        wait(
            &mut rx,
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: true,
                ad_consent: AdConsentState::Missing,
                printer: arena_proto::PrinterState::Skipped,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: true,
                test_session: Default::default(),
                live: false,
            },
        )
        .await;
        save(&f, "printer.paper_mm", "58").await.unwrap();
        wait(
            &mut rx,
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: true,
                ad_consent: AdConsentState::Missing,
                printer: arena_proto::PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: true,
                test_session: Default::default(),
                live: false,
            },
        )
        .await;
        save(&f, "setup.live", "true").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.wait_for(|s| s.live))
            .await
            .expect("published")
            .unwrap();
        job.abort();
    }
}

mod printer {
    use super::*;
    use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
    use crate::snapshot::SnapshotReader;
    use arena_proto::{
        PrinterErrorKind, PrinterState, PrinterStatus, PrinterStatusSet, StaffUpsert,
    };

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
                    setup_code_hash: None,
                    setup_expires_at: None,
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

    fn record_as(
        actor: &Actor,
        state: PrinterState,
        error_kind: Option<PrinterErrorKind>,
        error: Option<&str>,
        now: i64,
    ) -> Intent {
        as_actor(
            actor.clone(),
            IntentPayload::PrinterStatus(PrinterStatusSet {
                state,
                error_kind,
                error: error.map(str::to_owned),
            }),
            None,
            now,
        )
    }

    fn record(actor: &Actor, state: PrinterState, now: i64) -> Intent {
        record_as(actor, state, None, None, now)
    }

    fn stored(f: &Fixture) -> PrinterStatus {
        PrinterStatus::parse(&text(&reopen(f), "printer.status")).unwrap()
    }

    fn setup_printer(f: &Fixture) -> PrinterState {
        SnapshotReader::open(&f.path)
            .unwrap()
            .snapshot(f.clock.now_ms())
            .unwrap()
            .setup
            .unwrap()
            .printer
    }

    #[tokio::test]
    async fn bootstrap_seeds_the_printer_defaults() {
        let f = bootstrapped().await;
        let s = reopen(&f);
        assert_eq!(text(&s, "printer.selected"), "");
        assert_eq!(text(&s, "printer.paper_mm"), "80");
        assert_eq!(
            text(&s, "printer.status"),
            arena_proto::DEFAULT_PRINTER_STATUS
        );
        assert_eq!(setup_printer(&f), PrinterState::Unset);
    }

    #[tokio::test]
    async fn an_existing_branch_gets_the_printer_rows_from_seed_defaults() {
        let mut f = bootstrapped().await;
        f.writer
            .submit(|tx| {
                tx.execute_batch(
                    "DELETE FROM settings WHERE key IN ('printer.selected', 'printer.paper_mm',
                     'printer.status')",
                )
                .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
        while f.events.try_recv().is_ok() {}
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        assert_eq!(applied.changes.len(), 3);
        let s = reopen(&f);
        assert_eq!(text(&s, "printer.selected"), "");
        assert_eq!(text(&s, "printer.paper_mm"), "80");
        assert_eq!(
            text(&s, "printer.status"),
            arena_proto::DEFAULT_PRINTER_STATUS
        );
    }

    #[tokio::test]
    async fn the_selection_normalises_a_lan_address_and_names_a_bad_ip() {
        let f = bootstrapped().await;
        let r = rev(&f, "printer.selected");
        f.writer
            .submit_intent(set(
                "printer.selected",
                r#"{"kind":"lan","id_or_addr":"192.168.1.50","name":"192.168.1.50"}"#,
                Some(r),
            ))
            .await
            .unwrap();
        assert_eq!(
            text(&reopen(&f), "printer.selected"),
            r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"192.168.1.50"}"#
        );
        let before = snapshot(&f);
        let e = f
            .writer
            .submit_intent(set(
                "printer.selected",
                r#"{"kind":"lan","id_or_addr":"abc","name":"abc"}"#,
                Some(rev(&f, "printer.selected")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some("ip"));
        let e = f
            .writer
            .submit_intent(set(
                "printer.paper_mm",
                "57",
                Some(rev(&f, "printer.paper_mm")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("printer.paper_mm"));
        assert_eq!(snapshot(&f), before);
        f.writer
            .submit_intent(set(
                "printer.paper_mm",
                "58",
                Some(rev(&f, "printer.paper_mm")),
            ))
            .await
            .unwrap();
        assert_eq!(text(&reopen(&f), "printer.paper_mm"), "58");
    }

    #[tokio::test]
    async fn the_status_is_never_a_raw_setting() {
        let f = bootstrapped().await;
        let e = f
            .writer
            .submit_intent(set(
                "printer.status",
                r#"{"state":"ok","error":null,"at":1}"#,
                Some(rev(&f, "printer.status")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some("key"));
    }

    #[tokio::test]
    async fn test_and_skip_move_the_status_with_the_time_and_the_staff() {
        let f = bootstrapped().await;
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        let now = f.clock.now_ms();
        f.writer
            .submit_intent(record_as(
                &manager,
                PrinterState::Failed,
                Some(PrinterErrorKind::Refused),
                Some("connection refused"),
                now,
            ))
            .await
            .unwrap();
        assert_eq!(
            stored(&f),
            PrinterStatus {
                state: PrinterState::Failed,
                error_kind: Some(PrinterErrorKind::Refused),
                error: Some("connection refused".into()),
                at: Some(now),
            }
        );
        assert_eq!(setup_printer(&f), PrinterState::Failed);
        f.writer
            .submit_intent(record(&manager, PrinterState::Ok, now))
            .await
            .unwrap();
        assert_eq!(stored(&f).state, PrinterState::Ok);
        assert_eq!(stored(&f).error, None);
        f.writer
            .submit_intent(record(&manager, PrinterState::Skipped, now))
            .await
            .unwrap();
        assert_eq!(setup_printer(&f), PrinterState::Skipped);
        let c = reopen(&f).changes().unwrap().pop().unwrap();
        assert_eq!(c.intent_kind, "printer.status");
        assert_eq!(
            (c.actor_kind.as_str(), c.actor_id.as_str()),
            ("staff", manager.id.as_str())
        );
    }

    #[tokio::test]
    async fn changing_the_printer_or_the_width_resets_the_status_and_resaving_does_not() {
        let f = bootstrapped().await;
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        let now = f.clock.now_ms();
        let lan = r#"{"kind":"lan","id_or_addr":"192.168.1.50","name":"P"}"#;
        f.writer
            .submit_intent(set(
                "printer.selected",
                lan,
                Some(rev(&f, "printer.selected")),
            ))
            .await
            .unwrap();
        for (key, same, other) in [
            (
                "printer.selected",
                r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"P"}"#,
                r#"{"kind":"lan","id_or_addr":"192.168.1.51","name":"P"}"#,
            ),
            ("printer.paper_mm", "80", "58"),
        ] {
            f.writer
                .submit_intent(record(&manager, PrinterState::Ok, now))
                .await
                .unwrap();
            let applied = f
                .writer
                .submit_intent(set(key, same, Some(rev(&f, key))))
                .await
                .unwrap();
            assert_eq!(applied.changes.len(), 1, "{key}");
            assert_eq!(stored(&f).state, PrinterState::Ok, "{key} unchanged");
            let applied = f
                .writer
                .submit_intent(set(key, other, Some(rev(&f, key))))
                .await
                .unwrap();
            assert_eq!(applied.changes.len(), 2, "{key}");
            assert_eq!(stored(&f), PrinterStatus::unset(), "{key} changed");
            assert_eq!(setup_printer(&f), PrinterState::Unset);
            let applied = f
                .writer
                .submit_intent(set(key, same, Some(rev(&f, key))))
                .await
                .unwrap();
            assert_eq!(applied.changes.len(), 1, "{key}: already unset");
        }
        f.writer
            .submit_intent(record(&manager, PrinterState::Skipped, now))
            .await
            .unwrap();
        f.writer
            .submit_intent(set(
                "printer.selected",
                "",
                Some(rev(&f, "printer.selected")),
            ))
            .await
            .unwrap();
        assert_eq!(stored(&f).state, PrinterState::Unset);
    }

    #[tokio::test]
    async fn a_status_must_fit_its_state() {
        let f = bootstrapped().await;
        let manager = staff(&f, "manager", ROLE_MANAGER).await;
        let now = f.clock.now_ms();
        let long = "e".repeat(201);
        let k = Some(PrinterErrorKind::Other);
        for (state, kind, error, field) in [
            (PrinterState::Unset, None, None, "state"),
            (PrinterState::Failed, None, None, "error_kind"),
            (PrinterState::Failed, None, Some("x"), "error_kind"),
            (PrinterState::Failed, k, Some(" "), "error"),
            (PrinterState::Failed, k, Some(long.as_str()), "error"),
            (PrinterState::Ok, None, Some("x"), "error"),
            (PrinterState::Ok, k, None, "error_kind"),
            (PrinterState::Skipped, None, Some("x"), "error"),
        ] {
            let e = f
                .writer
                .submit_intent(record_as(&manager, state, kind, error, now))
                .await
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{state:?}");
            assert_eq!(e.field.as_deref(), Some(field), "{state:?}");
        }
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner-1".into(),
        };
        let e = f
            .writer
            .submit_intent(record(&owner, PrinterState::Skipped, now))
            .await
            .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("actor"));
        assert_eq!(setup_printer(&f), PrinterState::Unset);
    }

    #[tokio::test]
    async fn a_counter_cannot_record_or_select() {
        let f = bootstrapped().await;
        let counter = staff(&f, "counter", ROLE_COUNTER).await;
        let before = snapshot(&f);
        let now = f.clock.now_ms();
        for payload in [
            IntentPayload::PrinterStatus(PrinterStatusSet {
                state: PrinterState::Skipped,
                error_kind: None,
                error: None,
            }),
            IntentPayload::SettingsSet(SettingsSet {
                key: "printer.paper_mm".into(),
                value: "58".into(),
            }),
            IntentPayload::SettingsSet(SettingsSet {
                key: "printer.selected".into(),
                value: r#"{"kind":"lan","id_or_addr":"10.0.0.9","name":"P"}"#.into(),
            }),
        ] {
            let e = f
                .writer
                .submit_intent(as_actor(counter.clone(), payload, Some(1), now))
                .await
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::PermissionDenied);
        }
        assert_eq!(snapshot(&f), before);
    }
}

mod device_groups {
    use super::*;
    use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
    use crate::staff::hash_secret;
    use arena_proto::{
        ClientClass, DeviceAssign, DeviceKind, DeviceRegister, GroupDelete, GroupUpsert,
        StaffUpsert,
    };

    fn actor(kind: ActorKind, id: &str) -> Actor {
        Actor {
            kind,
            id: id.into(),
        }
    }

    fn as_actor(a: Actor, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            kind: payload.kind(),
            payload,
            actor: a,
            base_rev,
            ..bootstrap()
        }
    }

    fn upsert(id: Option<&str>, name: &str, kind: &str, colour: &str) -> IntentPayload {
        IntentPayload::GroupUpsert(GroupUpsert {
            id: id.map(Into::into),
            name: name.into(),
            group_kind: kind.into(),
            colour: colour.into(),
            sort_order: 1,
        })
    }

    fn assign(device: &str, group: Option<&str>) -> IntentPayload {
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: device.into(),
            group_id: group.map(Into::into),
        })
    }

    async fn owner(
        f: &Fixture,
        payload: IntentPayload,
        base_rev: Option<i64>,
    ) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(as_actor(actor(ActorKind::Owner, "o"), payload, base_rev))
            .await
    }

    async fn group(f: &Fixture, name: &str, kind: &str) -> String {
        owner(f, upsert(None, name, kind, "teal"), None)
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    async fn device(f: &Fixture, kind: DeviceKind) -> String {
        let mut d = DeviceRegister::new(
            ClientClass::Paired,
            "D",
            crate::devices::hash_token_secret("s").unwrap(),
        );
        d.device_kind = kind;
        f.writer
            .submit_intent(as_actor(
                actor(ActorKind::System, arena_proto::PAIR_SYSTEM_ACTOR),
                IntentPayload::DeviceRegister(d),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    fn group_rev(f: &Fixture, id: &str) -> i64 {
        reopen(f)
            .groups()
            .unwrap()
            .iter()
            .find(|g| g.group_id == id)
            .unwrap()
            .rev
    }

    fn device_rev(f: &Fixture, id: &str) -> i64 {
        reopen(f).device_meta(id).unwrap().unwrap().rev
    }

    async fn field(
        f: &Fixture,
        payload: IntentPayload,
        base_rev: Option<i64>,
    ) -> (ErrorCode, Option<String>) {
        let e = owner(f, payload, base_rev).await.unwrap_err();
        (e.code, e.field)
    }

    #[tokio::test]
    async fn create_edit_and_validate() {
        let f = bootstrapped().await;
        let applied = owner(&f, upsert(None, "PCs", "pc", "teal"), None)
            .await
            .unwrap();
        let id = applied.changes[0].row_id.clone();
        let c = &applied.changes[0];
        assert_eq!(
            reopen(&f)
                .change_actor_kind(&c.epoch, c.seq)
                .unwrap()
                .as_deref(),
            Some("owner")
        );
        let stored = reopen(&f).groups().unwrap();
        assert_eq!(
            (
                stored[0].name.as_str(),
                stored[0].colour.as_str(),
                stored[0].rev
            ),
            ("PCs", "teal", 1)
        );

        let invalid = |field: &str| (ErrorCode::ValidationFailed, Some(field.to_owned()));
        assert_eq!(
            field(&f, upsert(None, "pcs", "pc", "teal"), None).await,
            invalid("name")
        );
        assert_eq!(
            field(&f, upsert(None, "X", "pc", "purple"), None).await,
            invalid("colour")
        );
        assert_eq!(
            field(&f, upsert(None, "X", "tv", "teal"), None).await,
            invalid("kind")
        );
        assert_eq!(
            field(&f, upsert(None, " ", "pc", "teal"), None).await,
            invalid("name")
        );
        let staff = actor(ActorKind::Staff, "x");
        let e = f
            .writer
            .submit_intent(as_actor(
                staff,
                upsert(Some(&id), "PCs", "pc", "lime"),
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert_eq!(
            field(&f, upsert(Some("nope"), "Y", "pc", "lime"), Some(1)).await,
            invalid("id")
        );

        owner(&f, upsert(Some(&id), "Gaming PCs", "pc", "lime"), Some(1))
            .await
            .unwrap();
        let g = &reopen(&f).groups().unwrap()[0];
        assert_eq!(
            (g.name.as_str(), g.colour.as_str(), g.rev),
            ("Gaming PCs", "lime", 2)
        );
    }

    #[tokio::test]
    async fn kind_matrix() {
        let f = bootstrapped().await;
        let groups = [
            ("pc", group(&f, "G-pc", "pc").await),
            ("console", group(&f, "G-console", "console").await),
            ("vr", group(&f, "G-vr", "vr").await),
            ("other", group(&f, "G-other", "other").await),
        ];
        let kinds = [
            (DeviceKind::Pc, "pc"),
            (DeviceKind::Console, "console"),
            (DeviceKind::Vr, "vr"),
            (DeviceKind::Sim, "other"),
        ];
        for (dk, ok) in kinds {
            let dev = device(&f, dk).await;
            for (gk, gid) in &groups {
                let rev = device_rev(&f, &dev);
                let r = owner(&f, assign(&dev, Some(gid)), Some(rev)).await;
                if *gk == ok {
                    r.unwrap();
                    assert_eq!(
                        reopen(&f)
                            .device_meta(&dev)
                            .unwrap()
                            .unwrap()
                            .group_id
                            .as_deref(),
                        Some(gid.as_str())
                    );
                } else {
                    assert_eq!(
                        r.unwrap_err().code,
                        ErrorCode::GroupKindMismatch,
                        "{ok} -> {gk}"
                    );
                }
            }
            let rev = device_rev(&f, &dev);
            owner(&f, assign(&dev, None), Some(rev)).await.unwrap();
            assert_eq!(
                reopen(&f).device_meta(&dev).unwrap().unwrap().group_id,
                None
            );
        }
    }

    #[tokio::test]
    async fn delete_needs_an_empty_group_and_frees_the_name() {
        let f = bootstrapped().await;
        let id = group(&f, "PCs", "pc").await;
        let dev = device(&f, DeviceKind::Pc).await;
        owner(&f, assign(&dev, Some(&id)), Some(1)).await.unwrap();
        let del = || IntentPayload::GroupDelete(GroupDelete { id: id.clone() });
        let e = owner(&f, del(), Some(1)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::GroupNotEmpty);
        assert_eq!(
            field(&f, upsert(Some(&id), "PCs", "console", "teal"), Some(1)).await,
            (ErrorCode::ValidationFailed, Some("kind".into()))
        );
        owner(&f, assign(&dev, None), Some(2)).await.unwrap();
        owner(&f, del(), Some(1)).await.unwrap();
        assert!(reopen(&f).groups().unwrap().is_empty());
        assert_eq!(
            field(&f, del(), Some(2)).await.0,
            ErrorCode::ValidationFailed
        );
        let again = group(&f, "pcs", "pc").await;
        assert_ne!(again, id);
    }

    #[tokio::test]
    async fn stale_staff_edits_fail_but_an_owner_command_wins() {
        let f = bootstrapped().await;
        let id = group(&f, "PCs", "pc").await;
        owner(&f, upsert(Some(&id), "PCs", "pc", "lime"), Some(1))
            .await
            .unwrap();
        let role = |name: &str| -> String {
            match reopen(&f)
                .rows(Table::Roles)
                .unwrap()
                .into_iter()
                .find(|r| r["name"] == RowValue::Text(name.into()))
                .unwrap()["role_id"]
                .clone()
            {
                RowValue::Text(t) => t,
                other => panic!("{other:?}"),
            }
        };
        let mut staff = Vec::new();
        for (login, name) in [("mgr", ROLE_MANAGER), ("ctr", ROLE_COUNTER)] {
            let applied = owner(
                &f,
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: role(name),
                    password_hash: Some(hash_secret("pw").unwrap()),
                    pin_hash: Some(hash_secret("2222").unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                None,
            )
            .await
            .unwrap();
            staff.push(applied.changes[0].row_id.clone());
        }
        let by = |id: &str, p| as_actor(actor(ActorKind::Staff, id), p, Some(1));
        let e = f
            .writer
            .submit_intent(by(&staff[0], upsert(Some(&id), "PCs", "pc", "coral")))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
        assert_eq!(group_rev(&f, &id), 2);
        f.writer
            .submit_intent(as_actor(
                actor(ActorKind::Staff, &staff[0]),
                upsert(Some(&id), "PCs", "pc", "coral"),
                Some(2),
            ))
            .await
            .unwrap();
        owner(&f, upsert(Some(&id), "PCs", "pc", "sand"), Some(1))
            .await
            .unwrap();
        assert_eq!(reopen(&f).groups().unwrap()[0].colour, "sand");

        let e = f
            .writer
            .submit_intent(by(&staff[1], upsert(None, "New", "pc", "lime")))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let mut approved = by(&staff[1], upsert(None, "New", "pc", "lime"));
        approved.base_rev = None;
        approved.elevated_by = Some(ElevatedBy {
            kind: ElevationKind::Pin,
            id: staff[0].clone(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        });
        f.writer.submit_intent(approved).await.unwrap();
        assert_eq!(reopen(&f).groups().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn assign_and_delete_checks() {
        let f = bootstrapped().await;
        let id = group(&f, "PCs", "pc").await;
        let dev = device(&f, DeviceKind::Pc).await;
        let bad = |field: &str| (ErrorCode::ValidationFailed, Some(field.to_owned()));
        assert_eq!(
            field(&f, assign(&dev, Some("nope")), Some(1)).await,
            bad("group_id")
        );
        assert_eq!(
            field(&f, assign("nope", None), Some(1)).await,
            bad("device_id")
        );
        let mgr = actor(ActorKind::Staff, "x");
        let e = f
            .writer
            .submit_intent(as_actor(mgr, assign(&dev, Some(&id)), Some(1)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        owner(&f, assign(&dev, Some(&id)), Some(1)).await.unwrap();
        owner(&f, assign(&dev, None), Some(1)).await.unwrap();
        assert_eq!(device_rev(&f, &dev), 3);
        let del = IntentPayload::GroupDelete(GroupDelete { id: id.clone() });
        owner(&f, del.clone(), None).await.unwrap();
        assert_eq!(
            field(&f, assign(&dev, Some(&id)), Some(3)).await,
            bad("group_id")
        );
    }
}

mod floor_zones {
    use super::*;
    use arena_proto::{
        ClientClass, DeviceKind, DevicePlace, DeviceRegister, FLOOR_COLS, FLOOR_ROWS, ZoneDelete,
        ZoneUpsert,
    };

    fn by(kind: ActorKind, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            kind: payload.kind(),
            payload,
            actor: Actor {
                kind,
                id: if kind == ActorKind::System {
                    arena_proto::PAIR_SYSTEM_ACTOR.into()
                } else {
                    "o".into()
                },
            },
            base_rev,
            ..bootstrap()
        }
    }

    async fn owner(
        f: &Fixture,
        payload: IntentPayload,
        base_rev: Option<i64>,
    ) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(by(ActorKind::Owner, payload, base_rev))
            .await
    }

    fn upsert(id: Option<&str>, name: &str, sort: i64) -> IntentPayload {
        IntentPayload::ZoneUpsert(ZoneUpsert {
            id: id.map(Into::into),
            name: name.into(),
            sort_order: sort,
        })
    }

    fn place(device: &str, zone: Option<&str>, x: Option<i64>, y: Option<i64>) -> IntentPayload {
        IntentPayload::DevicePlace(DevicePlace {
            device_id: device.into(),
            zone_id: zone.map(Into::into),
            floor_x: x,
            floor_y: y,
        })
    }

    async fn zone(f: &Fixture, name: &str, sort: i64) -> String {
        owner(f, upsert(None, name, sort), None)
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    async fn device(f: &Fixture, label: &str) -> String {
        let mut d = DeviceRegister::new(
            ClientClass::Paired,
            label,
            crate::devices::hash_token_secret("s").unwrap(),
        );
        d.device_kind = DeviceKind::Pc;
        f.writer
            .submit_intent(by(
                ActorKind::System,
                IntentPayload::DeviceRegister(d),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    async fn err(
        f: &Fixture,
        payload: IntentPayload,
        rev: Option<i64>,
    ) -> (ErrorCode, Option<String>) {
        let e = owner(f, payload, rev).await.unwrap_err();
        (e.code, e.field)
    }

    fn at(f: &Fixture, dev: &str) -> (Option<String>, Option<i64>, Option<i64>) {
        let m = reopen(f).device_meta(dev).unwrap().unwrap();
        (m.zone_id, m.floor_x, m.floor_y)
    }

    #[tokio::test]
    async fn zones_are_added_renamed_reordered_and_named_uniquely() {
        let f = bootstrapped().await;
        let a = zone(&f, "PC hall", 2).await;
        let b = zone(&f, "Console room", 1).await;
        let names = || {
            reopen(&f)
                .zones()
                .unwrap()
                .into_iter()
                .map(|z| z.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(), ["Console room", "PC hall"]);
        owner(&f, upsert(Some(&a), "PC hall", 0), Some(1))
            .await
            .unwrap();
        assert_eq!(names(), ["PC hall", "Console room"]);
        owner(&f, upsert(Some(&b), "Lounge", 1), Some(1))
            .await
            .unwrap();
        assert_eq!(names(), ["PC hall", "Lounge"]);
        assert_eq!(
            err(&f, upsert(None, "pc HALL", 3), None).await,
            (ErrorCode::ValidationFailed, Some("name".into()))
        );
        assert_eq!(
            err(&f, upsert(None, " ", 3), None).await.0,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            err(&f, upsert(Some("nope"), "X", 3), Some(1)).await,
            (ErrorCode::ValidationFailed, Some("id".into()))
        );
        let e = f
            .writer
            .submit_intent(by(
                ActorKind::Staff,
                upsert(Some(&a), "PC hall", 5),
                Some(1),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }

    #[tokio::test]
    async fn a_zone_with_devices_cannot_be_deleted_until_they_move() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let d = device(&f, "PC 1").await;
        owner(&f, place(&d, Some(&z), Some(0), Some(0)), Some(1))
            .await
            .unwrap();
        let del = || IntentPayload::ZoneDelete(ZoneDelete { id: z.clone() });
        let e = owner(&f, del(), Some(1)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ZoneNotEmpty);
        owner(&f, place(&d, None, None, None), Some(2))
            .await
            .unwrap();
        assert_eq!(at(&f, &d), (None, None, None));
        owner(&f, del(), Some(1)).await.unwrap();
        assert!(reopen(&f).zones().unwrap().is_empty());
        assert_eq!(err(&f, del(), Some(2)).await.0, ErrorCode::ValidationFailed);
        zone(&f, "pc hall", 1).await;
    }

    #[tokio::test]
    async fn placing_stores_the_cell_and_two_devices_cannot_share_one() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let z2 = zone(&f, "Console room", 2).await;
        let (a, b) = (device(&f, "A").await, device(&f, "B").await);
        owner(&f, place(&a, Some(&z), Some(3), Some(2)), Some(1))
            .await
            .unwrap();
        assert_eq!(at(&f, &a), (Some(z.clone()), Some(3), Some(2)));
        assert_eq!(
            err(&f, place(&b, Some(&z), Some(3), Some(2)), Some(1)).await,
            (ErrorCode::CellTaken, Some("floor_x".into()))
        );
        owner(&f, place(&b, Some(&z2), Some(3), Some(2)), Some(1))
            .await
            .unwrap();
        owner(&f, place(&a, Some(&z), Some(3), Some(2)), Some(2))
            .await
            .unwrap();
        owner(&f, place(&a, Some(&z), Some(4), Some(2)), Some(3))
            .await
            .unwrap();
        owner(&f, place(&b, Some(&z), Some(3), Some(2)), Some(2))
            .await
            .unwrap();
        assert_eq!(at(&f, &a), (Some(z), Some(4), Some(2)));
    }

    #[tokio::test]
    async fn place_validates_zone_cell_bounds_and_device() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let d = device(&f, "A").await;
        let bad = |field: &str| (ErrorCode::ValidationFailed, Some(field.to_owned()));
        assert_eq!(
            err(&f, place("nope", None, None, None), Some(1)).await,
            bad("device_id")
        );
        assert_eq!(
            err(&f, place(&d, None, Some(1), Some(1)), Some(1)).await,
            bad("zone_id")
        );
        assert_eq!(
            err(&f, place(&d, Some("nope"), Some(1), Some(1)), Some(1)).await,
            bad("zone_id")
        );
        assert_eq!(
            err(&f, place(&d, Some(&z), None, Some(1)), Some(1)).await,
            bad("floor_x")
        );
        assert_eq!(
            err(&f, place(&d, Some(&z), Some(1), None), Some(1)).await,
            bad("floor_y")
        );
        assert_eq!(
            err(&f, place(&d, Some(&z), Some(-1), Some(0)), Some(1)).await,
            bad("floor_x")
        );
        assert_eq!(
            err(&f, place(&d, Some(&z), Some(FLOOR_COLS), Some(0)), Some(1)).await,
            bad("floor_x")
        );
        assert_eq!(
            err(&f, place(&d, Some(&z), Some(0), Some(FLOOR_ROWS)), Some(1)).await,
            bad("floor_y")
        );
        owner(
            &f,
            place(&d, Some(&z), Some(FLOOR_COLS - 1), Some(FLOOR_ROWS - 1)),
            Some(1),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_unknown_staff_member_cannot_place() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let d = device(&f, "A").await;
        owner(&f, place(&d, Some(&z), Some(0), Some(0)), Some(1))
            .await
            .unwrap();
        let e = f
            .writer
            .submit_intent(by(ActorKind::Staff, place(&d, None, None, None), Some(1)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
    }

    async fn manager(f: &Fixture) -> String {
        use crate::rbac::ROLE_MANAGER;
        use crate::staff::hash_secret;
        let role = reopen(f)
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == RowValue::Text(ROLE_MANAGER.into()))
            .unwrap()["role_id"]
            .clone();
        let RowValue::Text(role_id) = role else {
            panic!("role id")
        };
        owner(
            f,
            IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                staff_id: None,
                login: "mgr".into(),
                display_name: "mgr".into(),
                role_id,
                password_hash: Some(hash_secret("pw").unwrap()),
                pin_hash: Some(hash_secret("2222").unwrap()),
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
            None,
        )
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone()
    }

    async fn staff(
        f: &Fixture,
        id: &str,
        payload: IntentPayload,
        rev: Option<i64>,
    ) -> Result<Applied, AppError> {
        let mut i = by(ActorKind::Staff, payload, rev);
        i.actor.id = id.into();
        f.writer.submit_intent(i).await
    }

    #[tokio::test]
    async fn staff_edits_need_a_current_base_rev_and_owner_edits_do_not() {
        let f = bootstrapped().await;
        let m = manager(&f).await;
        let id = staff(&f, &m, upsert(None, "PC hall", 1), None)
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let d = device(&f, "A").await;
        let want = |e: AppError, code, field: &str| {
            assert_eq!((e.code, e.field.as_deref()), (code, Some(field)));
        };
        let up = || upsert(Some(&id), "Hall", 1);
        want(
            staff(&f, &m, up(), None).await.unwrap_err(),
            ErrorCode::ValidationFailed,
            "base_rev",
        );
        want(
            staff(&f, &m, up(), Some(9)).await.unwrap_err(),
            ErrorCode::Stale,
            "base_rev",
        );
        staff(&f, &m, up(), Some(1)).await.unwrap();
        let pl = || place(&d, Some(&id), Some(0), Some(0));
        want(
            staff(&f, &m, pl(), None).await.unwrap_err(),
            ErrorCode::ValidationFailed,
            "base_rev",
        );
        want(
            staff(&f, &m, pl(), Some(9)).await.unwrap_err(),
            ErrorCode::Stale,
            "base_rev",
        );
        staff(&f, &m, pl(), Some(1)).await.unwrap();
        let del = || IntentPayload::ZoneDelete(ZoneDelete { id: id.clone() });
        want(
            staff(&f, &m, del(), Some(1)).await.unwrap_err(),
            ErrorCode::Stale,
            "base_rev",
        );
        owner(&f, place(&d, None, None, None), None).await.unwrap();
        owner(&f, del(), None).await.unwrap();
    }

    #[tokio::test]
    async fn deleted_zones_and_devices_free_their_names_and_cells() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let (a, b) = (device(&f, "A").await, device(&f, "B").await);
        owner(&f, place(&a, Some(&z), Some(1), Some(1)), Some(1))
            .await
            .unwrap();
        let mut revoke = by(
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: a.clone(),
            }),
            None,
        );
        revoke.reason = Some(arena_proto::ReasonCode::Other);
        f.writer.submit_intent(revoke).await.unwrap();
        let gone = zone(&f, "Gone", 5).await;
        owner(
            &f,
            IntentPayload::ZoneDelete(ZoneDelete { id: gone.clone() }),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            err(&f, place(&b, Some(&gone), Some(0), Some(0)), Some(1)).await,
            (ErrorCode::ValidationFailed, Some("zone_id".into()))
        );
        assert_eq!(
            err(&f, upsert(Some(&gone), "Gone", 5), Some(1)).await,
            (ErrorCode::ValidationFailed, Some("id".into()))
        );
        assert_eq!(
            err(&f, upsert(None, " PC hall ", 7), None).await,
            (ErrorCode::ValidationFailed, Some("name".into()))
        );
        owner(&f, place(&b, Some(&z), Some(1), Some(1)), Some(1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn zones_with_equal_sort_order_fall_back_to_name_and_placing_keeps_the_group() {
        let f = bootstrapped().await;
        zone(&f, "Beta", 1).await;
        zone(&f, "alpha", 1).await;
        let names: Vec<_> = reopen(&f)
            .zones()
            .unwrap()
            .into_iter()
            .map(|z| z.name)
            .collect();
        assert_eq!(names, ["alpha", "Beta"]);
        let z = zone(&f, "Third", 2).await;
        let g = owner(
            &f,
            IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                id: None,
                name: "PCs".into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
            None,
        )
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone();
        let d = device(&f, "A").await;
        owner(
            &f,
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: d.clone(),
                group_id: Some(g.clone()),
            }),
            Some(1),
        )
        .await
        .unwrap();
        owner(&f, place(&d, Some(&z), Some(0), Some(0)), Some(2))
            .await
            .unwrap();
        let m = reopen(&f).device_meta(&d).unwrap().unwrap();
        assert_eq!(m.group_id, Some(g));
    }

    #[tokio::test]
    async fn the_snapshot_carries_zones_and_places() {
        let f = bootstrapped().await;
        let z = zone(&f, "PC hall", 1).await;
        let d = device(&f, "A").await;
        owner(&f, place(&d, Some(&z), Some(5), Some(1)), Some(1))
            .await
            .unwrap();
        let snap = crate::snapshot::read_snapshot(&reopen(&f), 0).unwrap();
        assert_eq!(snap.zones.len(), 1);
        assert_eq!(snap.zones[0].name, "PC hall");
        let sd = &snap.devices[0];
        assert_eq!(
            (sd.zone.clone(), sd.floor_x, sd.floor_y),
            (Some(z), Some(5), Some(1))
        );
    }

    fn maint(device: &str, on: bool, note: Option<&str>) -> IntentPayload {
        IntentPayload::DeviceMaintenance(arena_proto::DeviceMaintenance {
            device_id: device.into(),
            on,
            note: note.map(Into::into),
        })
    }

    fn state(f: &Fixture, dev: &str) -> (String, Option<String>, Option<String>, i64) {
        let m = reopen(f).device_meta(dev).unwrap().unwrap();
        (m.state, m.prior_state, m.maintenance_note, m.rev)
    }

    #[tokio::test]
    async fn maintenance_sets_a_note_keeps_group_and_place_and_returns_to_service() {
        let f = bootstrapped().await;
        let z = zone(&f, "Hall", 1).await;
        let d = device(&f, "A").await;
        owner(&f, place(&d, Some(&z), Some(2), Some(3)), Some(1))
            .await
            .unwrap();
        assert_eq!(state(&f, &d).0, "offline");
        owner(&f, maint(&d, true, Some("  fan dead ")), Some(2))
            .await
            .unwrap();
        let (s, prior, note, rev) = state(&f, &d);
        assert_eq!(
            (s.as_str(), prior.as_deref(), note.as_deref(), rev),
            ("maintenance", Some("offline"), Some("fan dead"), 3)
        );
        assert_eq!(at(&f, &d), (Some(z.clone()), Some(2), Some(3)));
        owner(&f, maint(&d, true, None), Some(3)).await.unwrap();
        let (s, prior, note, _) = state(&f, &d);
        assert_eq!(
            (s.as_str(), prior.as_deref(), note),
            ("maintenance", Some("offline"), None)
        );
        let snap = crate::snapshot::read_snapshot(&reopen(&f), 0).unwrap();
        assert_eq!(
            snap.devices[0].status,
            arena_proto::DeviceStatus::Maintenance
        );
        owner(&f, maint(&d, false, None), Some(4)).await.unwrap();
        let (s, prior, note, _) = state(&f, &d);
        assert_eq!((s.as_str(), prior, note), ("offline", None, None));
        assert_eq!(at(&f, &d), (Some(z), Some(2), Some(3)));
        let snap = crate::snapshot::read_snapshot(&reopen(&f), 0).unwrap();
        assert_eq!(snap.devices[0].status, arena_proto::DeviceStatus::Offline);
    }

    #[tokio::test]
    async fn return_to_service_restores_a_held_prior_state() {
        let f = bootstrapped().await;
        let d = device(&f, "A").await;
        f.writer
            .submit({
                let d = d.clone();
                move |tx| arena_store::set_device_state(tx, &d, "reserved", None, None)
            })
            .await
            .unwrap();
        owner(&f, maint(&d, true, None), None).await.unwrap();
        assert_eq!(state(&f, &d).1.as_deref(), Some("reserved"));
        owner(&f, maint(&d, false, None), None).await.unwrap();
        assert_eq!(state(&f, &d).0, "reserved");
    }

    #[tokio::test]
    async fn maintenance_validates_device_note_and_direction() {
        let f = bootstrapped().await;
        let d = device(&f, "A").await;
        assert_eq!(
            err(&f, maint("nope", true, None), None).await,
            (ErrorCode::ValidationFailed, Some("device_id".into()))
        );
        assert_eq!(
            err(&f, maint(&d, false, None), None).await,
            (ErrorCode::ValidationFailed, Some("device_id".into()))
        );
        let long = "x".repeat(arena_proto::MAINTENANCE_NOTE_MAX + 1);
        assert_eq!(
            err(&f, maint(&d, true, Some(&long)), None).await,
            (ErrorCode::ValidationFailed, Some("note".into()))
        );
        let ok = "x".repeat(arena_proto::MAINTENANCE_NOTE_MAX);
        owner(&f, maint(&d, true, Some(&ok)), None).await.unwrap();
    }

    #[tokio::test]
    async fn a_manual_device_shows_online_until_maintenance() {
        let f = bootstrapped().await;
        let mut r = DeviceRegister::new(ClientClass::Paired, "TV 1", String::new());
        r.device_kind = DeviceKind::Console;
        r.tv_number = Some(1);
        let d = owner(&f, IntentPayload::DeviceRegister(r), None)
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let status = || {
            crate::snapshot::read_snapshot(&reopen(&f), 0)
                .unwrap()
                .devices[0]
                .status
        };
        assert_eq!(status(), arena_proto::DeviceStatus::Online);
        assert_eq!(state(&f, &d).0, "online");
        owner(&f, maint(&d, true, None), None).await.unwrap();
        assert_eq!(status(), arena_proto::DeviceStatus::Maintenance);
        owner(&f, maint(&d, false, None), None).await.unwrap();
        assert_eq!(status(), arena_proto::DeviceStatus::Online);
    }

    #[tokio::test]
    async fn maintenance_notes_are_trimmed_counted_in_characters_and_cleared_on_return() {
        let f = bootstrapped().await;
        let d = device(&f, "A").await;
        assert_eq!(
            err(&f, maint(&d, true, Some("a\nb")), None).await,
            (ErrorCode::ValidationFailed, Some("note".into()))
        );
        owner(&f, maint(&d, true, Some("   ")), None).await.unwrap();
        assert_eq!(state(&f, &d).2, None);
        let padded = format!("  {}  ", "é".repeat(arena_proto::MAINTENANCE_NOTE_MAX));
        owner(&f, maint(&d, true, Some(&padded)), None)
            .await
            .unwrap();
        assert_eq!(
            state(&f, &d).2.map(|n| n.chars().count()),
            Some(arena_proto::MAINTENANCE_NOTE_MAX)
        );
        owner(&f, maint(&d, true, Some("new")), Some(1))
            .await
            .unwrap();
        let (s, prior, note, _) = state(&f, &d);
        assert_eq!(
            (s.as_str(), prior.as_deref(), note.as_deref()),
            ("maintenance", Some("offline"), Some("new"))
        );
        owner(&f, maint(&d, false, Some("ignored")), None)
            .await
            .unwrap();
        assert_eq!(state(&f, &d).2, None);
    }

    #[tokio::test]
    async fn held_states_show_in_the_snapshot_and_a_removed_device_is_refused() {
        let f = bootstrapped().await;
        let d = device(&f, "A").await;
        for held in ["in_session", "reserved"] {
            f.writer
                .submit({
                    let d = d.clone();
                    move |tx| arena_store::set_device_state(tx, &d, held, None, None)
                })
                .await
                .unwrap();
            let snap = crate::snapshot::read_snapshot(&reopen(&f), 0).unwrap();
            assert!(snap.devices[0].status.held());
            assert_eq!(
                format!("{:?}", snap.devices[0].status).to_lowercase(),
                held.replace('_', "")
            );
        }
        f.writer
            .submit({
                let d = d.clone();
                move |tx| arena_store::set_device_meta_deleted(tx, &d, 5)
            })
            .await
            .unwrap();
        assert_eq!(
            err(&f, maint(&d, true, None), None).await,
            (ErrorCode::ValidationFailed, Some("device_id".into()))
        );
    }

    #[tokio::test]
    async fn maintenance_needs_a_current_base_rev_from_staff() {
        let f = bootstrapped().await;
        let m = manager(&f).await;
        let d = device(&f, "A").await;
        let e = staff(&f, &m, maint(&d, true, None), None)
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("base_rev"))
        );
        let e = staff(&f, &m, maint(&d, true, None), Some(9))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::Stale, Some("base_rev"))
        );
        staff(&f, &m, maint(&d, true, Some("ram")), Some(1))
            .await
            .unwrap();
        assert_eq!(state(&f, &d).0, "maintenance");
    }
}

mod kitchen {
    use super::*;
    use arena_proto::{TierGate, kitchen_gate};

    fn text(f: &Fixture, key: &str) -> String {
        match &setting(&reopen(f), key)["value"] {
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

    async fn flags(f: &Fixture, value: &str) {
        let sql = format!("UPDATE settings SET value = '{value}' WHERE key = 'licence.flags'");
        f.writer
            .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bootstrap_seeds_the_kitchen_defaults() {
        let f = bootstrapped().await;
        assert_eq!(text(&f, "licence.flags"), "");
        assert_eq!(text(&f, "kitchen.kot_mode"), "receipt");
        assert_eq!(text(&f, "kitchen.kot_printer"), "");
        assert_eq!(text(&f, "kitchen.kot_paper_mm"), "80");
    }

    #[tokio::test]
    async fn a_free_branch_refuses_the_kitchen_keys_and_writes_nothing() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        for (key, value) in [
            ("kitchen.kot_mode", "receipt"),
            ("kitchen.kot_paper_mm", "58"),
            (
                "kitchen.kot_printer",
                r#"{"kind":"lan","id_or_addr":"10.0.0.9","name":"P"}"#,
            ),
        ] {
            let e = f
                .writer
                .submit_intent(set(key, value, Some(rev(&f, key))))
                .await
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::TierLocked, "{key}");
        }
        assert_eq!(snapshot(&f), before);
        let r = rev(&f, "receipt.footer");
        f.writer
            .submit_intent(set("receipt.footer", "hi", Some(r)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_kitchen_flag_opens_the_keys_with_base_rev_and_validation() {
        let f = bootstrapped().await;
        flags(&f, "kitchen").await;
        let r = rev(&f, "kitchen.kot_mode");
        f.writer
            .submit_intent(set("kitchen.kot_mode", "another", Some(r)))
            .await
            .unwrap();
        assert_eq!(text(&f, "kitchen.kot_mode"), "another");
        let e = f
            .writer
            .submit_intent(set("kitchen.kot_mode", "receipt", Some(r)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
        let e = f
            .writer
            .submit_intent(set(
                "kitchen.kot_paper_mm",
                "57",
                Some(rev(&f, "kitchen.kot_paper_mm")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        f.writer
            .submit_intent(set(
                "kitchen.kot_printer",
                r#"{"kind":"lan","id_or_addr":"10.0.0.9","name":"P"}"#,
                Some(rev(&f, "kitchen.kot_printer")),
            ))
            .await
            .unwrap();
        assert_eq!(
            text(&f, "kitchen.kot_printer"),
            r#"{"kind":"lan","id_or_addr":"10.0.0.9:9100","name":"P"}"#
        );
    }

    #[tokio::test]
    async fn a_lapse_locks_the_keys_again_and_keeps_their_values() {
        let f = bootstrapped().await;
        flags(&f, "kitchen").await;
        f.writer
            .submit_intent(set(
                "kitchen.kot_mode",
                "none",
                Some(rev(&f, "kitchen.kot_mode")),
            ))
            .await
            .unwrap();
        flags(&f, "").await;
        let e = f
            .writer
            .submit_intent(set(
                "kitchen.kot_mode",
                "receipt",
                Some(rev(&f, "kitchen.kot_mode")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::TierLocked);
        assert_eq!(text(&f, "kitchen.kot_mode"), "none");
    }

    #[tokio::test]
    async fn a_retry_of_an_applied_write_replays_after_a_lapse() {
        let f = bootstrapped().await;
        flags(&f, "kitchen").await;
        let intent = set(
            "kitchen.kot_mode",
            "none",
            Some(rev(&f, "kitchen.kot_mode")),
        );
        let first = f.writer.submit_intent(intent.clone()).await.unwrap();
        flags(&f, "").await;
        assert_eq!(f.writer.submit_intent(intent).await.unwrap(), first);
    }

    #[tokio::test]
    async fn the_flag_list_is_not_settable() {
        let f = bootstrapped().await;
        let e = f
            .writer
            .submit_intent(set(
                "licence.flags",
                "kitchen",
                Some(rev(&f, "licence.flags")),
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(text(&f, "licence.flags"), "");
    }

    #[test]
    fn a_fixture_registration_gates_new_work_and_lets_existing_work_through() {
        let f = primary();
        let revoke = |id: &str| Intent {
            payload: IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: id.into(),
            }),
            ..set("x", "y", None)
        };
        let gate = |p: &IntentPayload| match p {
            IntentPayload::DeviceRevoke(d) if d.device_id == "new" => Some(TierGate::New),
            IntentPayload::DeviceRevoke(_) => Some(TierGate::ExistingWork),
            other => kitchen_gate(other),
        };
        let mut store = reopen(&f);
        store
            .write_tx(|tx| {
                assert_eq!(
                    guards::tier(tx, &revoke("new"), gate).unwrap_err().code,
                    ErrorCode::TierLocked
                );
                assert!(guards::tier(tx, &revoke("old"), gate).is_ok());
                assert!(guards::tier(tx, &set("receipt.footer", "z", Some(1)), gate).is_ok());
                Ok(())
            })
            .unwrap();
    }
}

mod tax {
    use super::*;
    use arena_proto::{DEFAULT_TAX, TaxMode, TaxSettings};

    const GSTIN: &str = "27AAPFU0939F1ZV";

    fn value(f: &Fixture) -> String {
        match setting(&reopen(f), "branch.tax")["value"].clone() {
            RowValue::Text(t) => t,
            other => panic!("{other:?}"),
        }
    }

    async fn save(f: &Fixture, edit: impl FnOnce(&mut TaxSettings)) -> Result<Applied, AppError> {
        let rev = match setting(&reopen(f), "branch.tax")["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        };
        let mut tax = TaxSettings::parse(DEFAULT_TAX).unwrap();
        edit(&mut tax);
        let mut intent = set("branch.tax", &tax.to_json(), Some(rev));
        intent.issued_at = f.clock.now_ms();
        f.writer.submit_intent(intent).await
    }

    #[tokio::test]
    async fn tax_defaults_are_seeded_and_gst_saves() {
        let f = bootstrapped().await;
        assert_eq!(value(&f), DEFAULT_TAX);
        save(&f, |t| {
            t.gst = true;
            t.gstin = GSTIN.into();
            t.inclusive = false;
        })
        .await
        .unwrap();
        let tax = TaxSettings::parse(&value(&f)).unwrap();
        assert_eq!((tax.tax_mode(), tax.inclusive), (TaxMode::GstIn, false));
    }

    #[tokio::test]
    async fn tax_is_stored_in_canonical_form() {
        let f = bootstrapped().await;
        let loose = DEFAULT_TAX.replace(',', " , ");
        let mut intent = set("branch.tax", &loose, Some(1));
        intent.issued_at = f.clock.now_ms();
        f.writer.submit_intent(intent).await.unwrap();
        assert_eq!(value(&f), DEFAULT_TAX);
    }

    #[tokio::test]
    async fn invalid_tax_names_the_field_and_writes_nothing() {
        let f = bootstrapped().await;
        let before = snapshot(&f);
        for (field, edit) in [
            (
                "branch.tax.gstin",
                (|t: &mut TaxSettings| t.gst = true) as fn(&mut TaxSettings),
            ),
            ("branch.tax.gstin", |t| {
                t.gst = true;
                t.gstin = "27AAPFU0939F1ZW".into();
            }),
            ("branch.tax.product_bp", |t| t.product_bp = 2801),
            ("branch.tax.rate_bp", |t| {
                t.country = "AE".into();
                t.rate_bp = 10001;
            }),
            ("branch.tax.gst", |t| {
                t.country = "AE".into();
                t.gst = true;
            }),
        ] {
            let e = save(&f, edit).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.field.as_deref(), Some(field));
            assert!(
                e.message.contains("GSTIN_INVALID")
                    || e.message.contains("RATE_RANGE")
                    || field == "branch.tax.gst",
                "{}",
                e.message
            );
        }
        for raw in [
            r#"{"country":"IN"}"#,
            "nope",
            &DEFAULT_TAX.replace('}', r#","x":1}"#),
        ] {
            let mut intent = set("branch.tax", raw, Some(1));
            intent.issued_at = f.clock.now_ms();
            let e = f.writer.submit_intent(intent).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{raw}");
        }
        assert_eq!(snapshot(&f), before);
    }

    #[tokio::test]
    async fn a_stale_tax_save_is_refused() {
        let f = bootstrapped().await;
        let mut intent = set("branch.tax", DEFAULT_TAX, Some(99));
        intent.issued_at = f.clock.now_ms();
        let e = f.writer.submit_intent(intent).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
    }
}

mod billing {
    use super::*;
    use arena_proto::{BillingRules, TimeUpMode};

    const KEYS: [&str; 4] = [
        "billing.rounding_min",
        "billing.minimum_min",
        "billing.grace_min",
        "billing.time_up_mode",
    ];

    fn rev(f: &Fixture, key: &str) -> i64 {
        match setting(&reopen(f), key)["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        }
    }

    async fn save(f: &Fixture, key: &str, value: &str) -> Result<Applied, AppError> {
        let mut intent = set(key, value, Some(rev(f, key)));
        intent.issued_at = f.clock.now_ms();
        f.writer.submit_intent(intent).await
    }

    async fn read(f: &Fixture) -> BillingRules {
        f.writer.submit(crate::billing::read).await.unwrap()
    }

    #[tokio::test]
    async fn defaults_are_seeded_for_a_new_branch() {
        let f = bootstrapped().await;
        assert_eq!(read(&f).await, BillingRules::defaults());
        for (key, value) in KEYS.into_iter().zip(["15", "30", "2", "clean_up"]) {
            assert_eq!(
                setting(&reopen(&f), key)["value"],
                RowValue::Text(value.into())
            );
        }
    }

    #[tokio::test]
    async fn an_upgraded_branch_gets_the_billing_rows_from_seed_defaults() {
        let mut f = bootstrapped().await;
        f.writer
            .submit(|tx| {
                tx.execute_batch("DELETE FROM settings WHERE key LIKE 'billing.%'")
                    .map_err(arena_store::db_error)
            })
            .await
            .unwrap();
        assert_eq!(read(&f).await, BillingRules::defaults());
        while f.events.try_recv().is_ok() {}
        let applied = f.writer.seed_defaults().await.unwrap().unwrap();
        assert_eq!(applied.changes.len(), 5);
        for (key, value) in KEYS.into_iter().zip(["15", "30", "2", "clean_up"]) {
            assert_eq!(
                setting(&reopen(&f), key)["value"],
                RowValue::Text(value.into())
            );
        }
        assert_eq!(
            setting(&reopen(&f), arena_proto::KEY_BILLING_CREDIT_AUTO_PAUSE)["value"],
            RowValue::Text("true".into())
        );
        assert_eq!(f.writer.seed_defaults().await.unwrap(), None);
    }

    #[tokio::test]
    async fn bounds_are_accepted_and_the_rest_name_their_key() {
        let f = bootstrapped().await;
        for (key, ok) in [
            ("billing.rounding_min", ["1", "60"]),
            ("billing.minimum_min", ["0", "240"]),
            ("billing.grace_min", ["0", "10"]),
            ("billing.time_up_mode", ["staff_handles", "clean_up"]),
        ] {
            for v in ok {
                save(&f, key, v).await.unwrap();
            }
        }
        let before = snapshot(&f);
        for (key, bad) in [
            ("billing.rounding_min", "0"),
            ("billing.rounding_min", "61"),
            ("billing.rounding_min", ""),
            ("billing.rounding_min", "1e1"),
            ("billing.rounding_min", "+5"),
            ("billing.rounding_min", "-1"),
            ("billing.rounding_min", "12.5"),
            ("billing.rounding_min", "abc"),
            ("billing.minimum_min", "241"),
            ("billing.minimum_min", ""),
            ("billing.grace_min", "11"),
            ("billing.grace_min", "+5"),
            ("billing.time_up_mode", "Clean up"),
            ("billing.time_up_mode", ""),
        ] {
            let e = save(&f, key, bad).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{key}={bad:?}");
            assert_eq!(e.field.as_deref(), Some(key), "{key}={bad:?}");
        }
        assert_eq!(snapshot(&f), before);
    }

    #[tokio::test]
    async fn a_stale_billing_save_is_refused() {
        let f = bootstrapped().await;
        save(&f, "billing.grace_min", "5").await.unwrap();
        let mut stale = set("billing.grace_min", "6", Some(1));
        stale.issued_at = f.clock.now_ms();
        let e = f.writer.submit_intent(stale).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
    }

    #[tokio::test]
    async fn a_value_read_earlier_never_changes() {
        let f = bootstrapped().await;
        let earlier = read(&f).await;
        for (key, value) in KEYS.into_iter().zip(["10", "0", "10", "staff_handles"]) {
            save(&f, key, value).await.unwrap();
        }
        assert_eq!(earlier, BillingRules::defaults());
        let later = read(&f).await;
        assert_eq!(
            (
                later.rounding_min,
                later.minimum_min,
                later.grace_min,
                later.time_up_mode
            ),
            (10, 0, 10, TimeUpMode::StaffHandles)
        );
        assert_ne!(earlier, later);
    }
}

mod manual_devices {
    use super::*;
    use arena_proto::{
        ClientClass, DeviceAssign, DeviceKind, DeviceRegister, DeviceRevoke, GroupUpsert,
    };

    fn reg(kind: DeviceKind, label: &str, tv: Option<i64>) -> DeviceRegister {
        DeviceRegister {
            device_id: None,
            class: ClientClass::Paired,
            device_kind: kind,
            label: label.into(),
            token_hash: String::new(),
            tv_number: tv,
            group_id: None,
        }
    }

    async fn send(
        f: &Fixture,
        kind: ActorKind,
        payload: IntentPayload,
        base_rev: Option<i64>,
    ) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(Intent {
                kind: payload.kind(),
                reason: payload.kind().reasons().map(|r| r[0]),
                payload,
                actor: Actor {
                    kind,
                    id: "o".into(),
                },
                base_rev,
                ..bootstrap()
            })
            .await
    }

    async fn add(f: &Fixture, d: DeviceRegister) -> Result<String, AppError> {
        let applied = send(f, ActorKind::Owner, IntentPayload::DeviceRegister(d), None).await?;
        Ok(applied.changes[0].row_id.clone())
    }

    async fn err(f: &Fixture, d: DeviceRegister) -> (ErrorCode, Option<String>) {
        let e = add(f, d).await.unwrap_err();
        (e.code, e.field)
    }

    fn invalid(field: &str) -> (ErrorCode, Option<String>) {
        (ErrorCode::ValidationFailed, Some(field.into()))
    }

    async fn group(f: &Fixture, kind: &str) -> String {
        let p = IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: format!("G-{kind}"),
            group_kind: kind.into(),
            colour: "teal".into(),
            sort_order: 1,
        });
        send(f, ActorKind::Owner, p, None).await.unwrap().changes[0]
            .row_id
            .clone()
    }

    async fn cap(f: &Fixture, value: &str) {
        let sql = format!(
            "INSERT INTO settings (branch_id, key, value, rev, updated_at)
             SELECT branch_id, 'licence.device_cap', '{value}', 0, 0 FROM branch"
        );
        f.writer
            .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_manual_console_registers_without_a_token() {
        let f = bootstrapped().await;
        let id = add(&f, reg(DeviceKind::Console, "PS5 One", Some(3)))
            .await
            .unwrap();
        let store = reopen(&f);
        let meta = store.device_meta(&id).unwrap().unwrap();
        assert_eq!(
            (meta.kind.as_str(), meta.tv_number, meta.group_id.clone()),
            ("console", Some(3), None)
        );
        let device = store.devices().unwrap().pop().unwrap();
        assert_eq!(
            (device.class.as_str(), device.token_hash.as_str()),
            ("paired", "")
        );
        let snap = crate::snapshot::read_snapshot(&store, 0).unwrap();
        assert!(snap.devices[0].manual && snap.devices[0].tv_number == Some(3));
        let c = &f
            .writer
            .submit_intent(Intent {
                kind: IntentKind::DeviceRegister,
                payload: IntentPayload::DeviceRegister(reg(DeviceKind::Vr, "VR 1", None)),
                actor: Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                ..bootstrap()
            })
            .await
            .unwrap()
            .changes[0];
        assert_eq!(
            store.change_actor_kind(&c.epoch, c.seq).unwrap().as_deref(),
            Some("owner")
        );
    }

    #[tokio::test]
    async fn a_manager_registers_by_hand_and_a_counter_cannot() {
        let f = bootstrapped().await;
        let role = |name: &str| -> String {
            match reopen(&f)
                .rows(Table::Roles)
                .unwrap()
                .into_iter()
                .find(|r| r["name"] == RowValue::Text(name.into()))
                .unwrap()["role_id"]
                .clone()
            {
                RowValue::Text(t) => t,
                other => panic!("{other:?}"),
            }
        };
        let mut staff = Vec::new();
        for (login, name) in [
            ("mgr", crate::rbac::ROLE_MANAGER),
            ("ctr", crate::rbac::ROLE_COUNTER),
        ] {
            let applied = send(
                &f,
                ActorKind::Owner,
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: role(name),
                    password_hash: Some(crate::staff::hash_secret("pw").unwrap()),
                    pin_hash: Some(crate::staff::hash_secret("2222").unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                None,
            )
            .await
            .unwrap();
            staff.push(applied.changes[0].row_id.clone());
        }
        let by = |id: &str| Intent {
            kind: IntentKind::DeviceRegister,
            payload: IntentPayload::DeviceRegister(reg(DeviceKind::Sim, "Sim 1", None)),
            actor: Actor {
                kind: ActorKind::Staff,
                id: id.into(),
            },
            ..bootstrap()
        };
        let e = f.writer.submit_intent(by(&staff[1])).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let applied = f.writer.submit_intent(by(&staff[0])).await.unwrap();
        let c = &applied.changes[0];
        assert_eq!(
            reopen(&f)
                .change_actor_kind(&c.epoch, c.seq)
                .unwrap()
                .as_deref(),
            Some("staff")
        );
        let id = applied
            .changes
            .iter()
            .find(|c| c.tbl == "devices")
            .unwrap()
            .row_id
            .clone();
        let settings = reopen(&f).rows(Table::Settings).unwrap();
        assert!(
            !settings
                .iter()
                .any(|r| r["key"] == RowValue::Text(arena_proto::hc_key(&id)))
        );
    }

    #[tokio::test]
    async fn the_pairing_actor_cannot_register_by_hand() {
        let f = bootstrapped().await;
        let e = send(
            &f,
            ActorKind::System,
            IntentPayload::DeviceRegister(reg(DeviceKind::Vr, "VR", None)),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let pairing = Intent {
            kind: IntentKind::DeviceRegister,
            payload: IntentPayload::DeviceRegister(reg(DeviceKind::Vr, "VR", None)),
            actor: Actor {
                kind: ActorKind::System,
                id: arena_proto::PAIR_SYSTEM_ACTOR.into(),
            },
            ..bootstrap()
        };
        let e = f.writer.submit_intent(pairing).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("kind"))
        );
        let mut d = reg(DeviceKind::Vr, "VR", None);
        d.class = ClientClass::Kds;
        assert_eq!(err(&f, d).await, invalid("kind"));
    }

    #[tokio::test]
    async fn edit_rules_for_a_manual_device() {
        let f = bootstrapped().await;
        let id = add(&f, reg(DeviceKind::Console, "A", Some(1)))
            .await
            .unwrap();
        let edit = |d: DeviceRegister| {
            let mut d = d;
            d.device_id = Some(id.clone());
            IntentPayload::DeviceRegister(d)
        };
        send(
            &f,
            ActorKind::Owner,
            edit(reg(DeviceKind::Console, "A2", Some(1))),
            Some(1),
        )
        .await
        .unwrap();
        let mut grouped = reg(DeviceKind::Console, "A2", Some(1));
        grouped.group_id = Some("G".into());
        let e = send(&f, ActorKind::Owner, edit(grouped), Some(2))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("group_id"))
        );
        let e = send(
            &f,
            ActorKind::Owner,
            edit(reg(DeviceKind::Pc, "A2", None)),
            Some(2),
        )
        .await
        .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("kind"))
        );
        let e = send(
            &f,
            ActorKind::Owner,
            edit(reg(DeviceKind::Console, "A2", None)),
            Some(2),
        )
        .await
        .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("tv_number"))
        );
    }

    #[tokio::test]
    async fn a_deleted_group_is_refused_and_tv_bounds_hold() {
        let f = bootstrapped().await;
        let g = group(&f, "console").await;
        send(
            &f,
            ActorKind::Owner,
            IntentPayload::GroupDelete(arena_proto::GroupDelete { id: g.clone() }),
            Some(1),
        )
        .await
        .unwrap();
        let mut d = reg(DeviceKind::Console, "A", Some(1));
        d.group_id = Some(g);
        assert_eq!(err(&f, d).await, invalid("group_id"));
        assert_eq!(
            err(&f, reg(DeviceKind::Console, "A", Some(1000))).await,
            invalid("tv_number")
        );
        add(&f, reg(DeviceKind::Console, "A", Some(999)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_cap_counts_paired_devices_and_a_revoke_frees_the_slot_with_a_change_row() {
        let f = bootstrapped().await;
        cap(&f, "1").await;
        let mut tokened = reg(DeviceKind::Pc, "PC", None);
        tokened.token_hash = crate::devices::hash_token_secret("s").unwrap();
        let id = add(&f, tokened.clone()).await.unwrap();
        assert_eq!(
            err(&f, tokened.clone()).await,
            (ErrorCode::DeviceLimit, None)
        );
        assert_eq!(
            err(&f, reg(DeviceKind::Vr, "VR", None)).await,
            (ErrorCode::DeviceLimit, None)
        );
        let applied = send(
            &f,
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(DeviceRevoke {
                device_id: id.clone(),
            }),
            None,
        )
        .await
        .unwrap();
        assert!(
            applied
                .changes
                .iter()
                .any(|c| c.tbl == "devices_meta" && c.row_id == id)
        );
        add(&f, tokened).await.unwrap();
    }

    #[tokio::test]
    async fn validation_of_a_manual_device() {
        let f = bootstrapped().await;
        assert_eq!(
            err(&f, reg(DeviceKind::Pc, "PC", None)).await,
            invalid("kind")
        );
        assert_eq!(
            err(&f, reg(DeviceKind::Console, "PS5", None)).await,
            invalid("tv_number")
        );
        assert_eq!(
            err(&f, reg(DeviceKind::Console, "PS5", Some(0))).await,
            invalid("tv_number")
        );
        assert_eq!(
            err(&f, reg(DeviceKind::Vr, "VR", Some(1))).await,
            invalid("tv_number")
        );
        assert_eq!(
            err(&f, reg(DeviceKind::Vr, " ", None)).await,
            invalid("label")
        );
        let mut g = reg(DeviceKind::Vr, "VR", None);
        g.group_id = Some("nope".into());
        assert_eq!(err(&f, g).await, invalid("group_id"));
    }

    #[tokio::test]
    async fn the_tv_number_is_unique_among_live_consoles() {
        let f = bootstrapped().await;
        let id = add(&f, reg(DeviceKind::Console, "A", Some(1)))
            .await
            .unwrap();
        assert_eq!(
            err(&f, reg(DeviceKind::Console, "B", Some(1))).await,
            invalid("tv_number")
        );
        add(&f, reg(DeviceKind::Console, "B", Some(2)))
            .await
            .unwrap();
        send(
            &f,
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(DeviceRevoke { device_id: id }),
            None,
        )
        .await
        .unwrap();
        add(&f, reg(DeviceKind::Console, "C", Some(1)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_group_must_fit_the_kind() {
        let f = bootstrapped().await;
        let consoles = group(&f, "console").await;
        let mut ok = reg(DeviceKind::Console, "A", Some(1));
        ok.group_id = Some(consoles.clone());
        let id = add(&f, ok).await.unwrap();
        assert_eq!(
            reopen(&f).device_meta(&id).unwrap().unwrap().group_id,
            Some(consoles.clone())
        );
        let mut bad = reg(DeviceKind::Vr, "V", None);
        bad.group_id = Some(consoles);
        assert_eq!(
            err(&f, bad).await,
            (ErrorCode::GroupKindMismatch, Some("group_id".into()))
        );
    }

    #[tokio::test]
    async fn the_device_cap_refuses_and_a_removal_frees_the_slot() {
        let f = bootstrapped().await;
        cap(&f, "2").await;
        let a = add(&f, reg(DeviceKind::Vr, "A", None)).await.unwrap();
        add(&f, reg(DeviceKind::Vr, "B", None)).await.unwrap();
        assert_eq!(
            err(&f, reg(DeviceKind::Vr, "C", None)).await,
            (ErrorCode::DeviceLimit, None)
        );
        send(
            &f,
            ActorKind::Owner,
            IntentPayload::DeviceRevoke(DeviceRevoke {
                device_id: a.clone(),
            }),
            None,
        )
        .await
        .unwrap();
        assert!(
            reopen(&f)
                .device_meta(&a)
                .unwrap()
                .unwrap()
                .deleted_at
                .is_some()
        );
        add(&f, reg(DeviceKind::Vr, "C", None)).await.unwrap();
    }

    #[tokio::test]
    async fn edits_need_base_rev_and_go_stale() {
        let f = bootstrapped().await;
        let id = add(&f, reg(DeviceKind::Console, "A", Some(1)))
            .await
            .unwrap();
        add(&f, reg(DeviceKind::Console, "B", Some(2)))
            .await
            .unwrap();
        let edit = |label: &str, tv: i64| {
            let mut d = reg(DeviceKind::Console, label, Some(tv));
            d.device_id = Some(id.clone());
            IntentPayload::DeviceRegister(d)
        };
        let e = send(&f, ActorKind::Owner, edit("A2", 5), None)
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("base_rev"))
        );
        send(&f, ActorKind::Owner, edit("A2", 5), Some(1))
            .await
            .unwrap();
        let meta = reopen(&f).device_meta(&id).unwrap().unwrap();
        assert_eq!(
            (meta.label.as_str(), meta.tv_number, meta.rev),
            ("A2", Some(5), 2)
        );
        let e = send(&f, ActorKind::Owner, edit("A3", 5), Some(1))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
        let e = send(&f, ActorKind::Owner, edit("A3", 2), Some(2))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("tv_number"))
        );
        send(&f, ActorKind::Owner, edit("A3", 1), Some(2))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_grouped_device_keeps_its_kind_on_edit() {
        let f = bootstrapped().await;
        let consoles = group(&f, "console").await;
        let id = add(&f, reg(DeviceKind::Console, "A", Some(1)))
            .await
            .unwrap();
        send(
            &f,
            ActorKind::Owner,
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: id.clone(),
                group_id: Some(consoles),
            }),
            Some(1),
        )
        .await
        .unwrap();
        let mut d = reg(DeviceKind::Vr, "A", None);
        d.device_id = Some(id);
        let e = send(
            &f,
            ActorKind::Owner,
            IntentPayload::DeviceRegister(d),
            Some(2),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::GroupKindMismatch);
    }

    #[test]
    fn the_wire_form_has_one_kind_key() {
        let p = IntentPayload::DeviceRegister(reg(DeviceKind::Console, "A", Some(1)));
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["kind"], "device.register");
        assert_eq!(json["device_kind"], "console");
        assert_eq!(serde_json::from_value::<IntentPayload>(json).unwrap(), p);
    }
}

mod licence {
    use super::*;
    use arena_proto::{
        ClientClass, DeviceKind, DeviceRegister, FLAG_KITCHEN, LicenceApply, LicenceStanding,
    };

    fn body(flags: &str, devices: u32) -> LicenceApply {
        LicenceApply {
            standing: LicenceStanding::Verified,
            clock_wrong: false,
            flags: flags.into(),
            device_cap: devices,
            user_cap: 10,
        }
    }

    fn text(f: &Fixture, key: &str) -> Option<String> {
        reopen(f)
            .rows(Table::Settings)
            .unwrap()
            .into_iter()
            .find(|r| r["key"] == RowValue::Text(key.into()))
            .map(|r| match &r["value"] {
                RowValue::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            })
    }

    async fn send(f: &Fixture, actor: ActorKind, p: LicenceApply) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(Intent {
                kind: IntentKind::LicenceApply,
                payload: IntentPayload::LicenceApply(p),
                actor: Actor {
                    kind: actor,
                    id: "hub-licence".into(),
                },
                base_rev: None,
                ..bootstrap()
            })
            .await
    }

    fn reg(label: &str) -> IntentPayload {
        IntentPayload::DeviceRegister(DeviceRegister {
            device_id: None,
            class: ClientClass::Paired,
            device_kind: DeviceKind::Vr,
            label: label.into(),
            token_hash: String::new(),
            tv_number: None,
            group_id: None,
        })
    }

    async fn add(f: &Fixture, label: &str) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(Intent {
                kind: IntentKind::DeviceRegister,
                payload: reg(label),
                actor: Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                base_rev: None,
                ..bootstrap()
            })
            .await
    }

    #[tokio::test]
    async fn the_system_writes_the_five_licence_rows_as_change_rows() {
        let f = bootstrapped().await;
        let applied = send(&f, ActorKind::System, body("kitchen,tunnel", 60))
            .await
            .unwrap();
        assert_eq!(applied.changes.len(), 5);
        assert_eq!(text(&f, "licence.flags").as_deref(), Some("kitchen,tunnel"));
        assert_eq!(text(&f, "licence.device_cap").as_deref(), Some("60"));
        assert_eq!(text(&f, "licence.user_cap").as_deref(), Some("10"));
        assert_eq!(text(&f, "licence.status").as_deref(), Some("verified"));
        assert_eq!(text(&f, "licence.clock").as_deref(), Some("ok"));
        let again = send(&f, ActorKind::System, body("kitchen", 20))
            .await
            .unwrap();
        assert_eq!(again.changes.len(), 2);
    }

    #[tokio::test]
    async fn only_the_system_may_apply_a_licence() {
        let f = bootstrapped().await;
        for kind in [ActorKind::Owner, ActorKind::Staff, ActorKind::Device] {
            let e = send(&f, kind, body("kitchen", 999)).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::PermissionDenied, "{kind:?}");
        }
        assert_eq!(text(&f, "licence.device_cap"), None);
    }

    #[tokio::test]
    async fn a_licence_needs_a_branch_and_clean_flags() {
        let f = primary_long_lease();
        let e = send(&f, ActorKind::System, body("", 20)).await.unwrap_err();
        assert_eq!(e.field.as_deref(), Some("branch"));
        let f = bootstrapped().await;
        let e = send(&f, ActorKind::System, body("Kitchen", 20))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("flags"))
        );
        let mut with_rev = body("", 20);
        with_rev.flags = "a,b".into();
        let mut i = Intent {
            kind: IntentKind::LicenceApply,
            payload: IntentPayload::LicenceApply(with_rev),
            actor: Actor {
                kind: ActorKind::System,
                id: "s".into(),
            },
            base_rev: Some(1),
            ..bootstrap()
        };
        i.idempotency_key = ulid::Ulid::generate().to_string();
        let e = f.writer.submit_intent(i).await.unwrap_err();
        assert_eq!(e.field.as_deref(), Some("base_rev"));
    }

    #[tokio::test]
    async fn the_entitlement_api_drives_the_device_cap_and_the_kitchen_gate() {
        let f = bootstrapped().await;
        send(&f, ActorKind::System, body("", 1)).await.unwrap();
        add(&f, "A").await.unwrap();
        assert_eq!(add(&f, "B").await.unwrap_err().code, ErrorCode::DeviceLimit);
        send(&f, ActorKind::System, body(FLAG_KITCHEN, 2))
            .await
            .unwrap();
        add(&f, "B").await.unwrap();
        let mut store = reopen(&f);
        store
            .write_tx(|tx| {
                assert_eq!(crate::entitlement::limit(tx, "devices")?, Some(2));
                assert_eq!(crate::entitlement::limit(tx, "users")?, Some(10));
                assert_eq!(crate::entitlement::limit(tx, "branches")?, None);
                assert!(crate::entitlement::has_flag(tx, FLAG_KITCHEN)?);
                assert!(!crate::entitlement::has_flag(tx, "tunnel")?);
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn the_writer_applies_only_what_changed() {
        let f = bootstrapped().await;
        let first = f.writer.apply_licence(body("kitchen", 60)).await.unwrap();
        assert_eq!(first.unwrap().changes.len(), 5);
        assert!(
            f.writer
                .apply_licence(body("kitchen", 60))
                .await
                .unwrap()
                .is_none()
        );
        let before = reopen(&f).changes().unwrap().len();
        let skewed = LicenceApply {
            clock_wrong: true,
            ..body("kitchen", 60)
        };
        assert_eq!(
            f.writer
                .apply_licence(skewed)
                .await
                .unwrap()
                .unwrap()
                .changes
                .len(),
            1
        );
        assert_eq!(reopen(&f).changes().unwrap().len(), before + 1);
    }

    fn kitchen_rev(f: &Fixture) -> i64 {
        match setting(&reopen(f), "kitchen.kot_mode")["rev"] {
            RowValue::Integer(n) => n,
            _ => unreachable!(),
        }
    }

    async fn kot(f: &Fixture, value: &str) -> Result<Applied, AppError> {
        let mut i = set("kitchen.kot_mode", value, Some(kitchen_rev(f)));
        i.issued_at = f.clock.now_ms();
        f.writer.submit_intent(i).await
    }

    #[tokio::test]
    async fn an_applied_licence_opens_then_closes_the_kitchen_gate() {
        let f = bootstrapped().await;
        assert_eq!(
            kot(&f, "none").await.unwrap_err().code,
            ErrorCode::TierLocked
        );
        let premium = arena_proto::Entitlement {
            standing: LicenceStanding::Verified,
            tier: arena_proto::Tier::Premium,
            flags: arena_proto::Tier::Premium
                .defaults()
                .flags
                .iter()
                .map(|f| (*f).to_owned())
                .collect(),
            limits: arena_proto::Tier::Premium.defaults().limits,
        };
        send(&f, ActorKind::System, premium.apply_payload(false))
            .await
            .unwrap();
        kot(&f, "none").await.unwrap();
        let lapsed = arena_proto::Entitlement::free(LicenceStanding::Expired);
        send(&f, ActorKind::System, lapsed.apply_payload(false))
            .await
            .unwrap();
        assert_eq!(
            kot(&f, "receipt").await.unwrap_err().code,
            ErrorCode::TierLocked
        );
    }

    #[tokio::test]
    async fn a_downgrade_blocks_new_devices_and_ends_nothing_running() {
        let f = bootstrapped().await;
        send(&f, ActorKind::System, body("", 5)).await.unwrap();
        let mut ids = Vec::new();
        for l in ["A", "B", "C"] {
            ids.push(add(&f, l).await.unwrap().changes[0].row_id.clone());
        }
        send(
            &f,
            ActorKind::System,
            LicenceApply {
                standing: LicenceStanding::Expired,
                ..body("", 1)
            },
        )
        .await
        .unwrap();
        assert_eq!(add(&f, "D").await.unwrap_err().code, ErrorCode::DeviceLimit);
        let store = reopen(&f);
        for id in &ids {
            assert!(store.device(id).unwrap().unwrap().revoked_at.is_none());
        }
        let revoke = f
            .writer
            .submit_intent(Intent {
                kind: IntentKind::DeviceRevoke,
                reason: IntentKind::DeviceRevoke.reasons().map(|r| r[0]),
                payload: IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                    device_id: ids[0].clone(),
                }),
                actor: Actor {
                    kind: ActorKind::Owner,
                    id: "o".into(),
                },
                base_rev: Some(1),
                ..bootstrap()
            })
            .await;
        assert!(revoke.is_ok(), "{revoke:?}");
    }

    #[tokio::test]
    async fn with_no_licence_rows_the_free_limits_apply() {
        let f = bootstrapped_free().await;
        let mut store = reopen(&f);
        store
            .write_tx(|tx| {
                assert_eq!(crate::entitlement::limit(tx, "devices")?, Some(20));
                assert_eq!(crate::entitlement::limit(tx, "users")?, Some(3));
                assert!(!crate::entitlement::has_flag(tx, FLAG_KITCHEN)?);
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn every_non_system_actor_kind_is_refused() {
        let f = bootstrapped().await;
        for kind in [
            ActorKind::Owner,
            ActorKind::Staff,
            ActorKind::Device,
            ActorKind::Admin,
        ] {
            let e = send(&f, kind, body("kitchen", 999)).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::PermissionDenied, "{kind:?}");
        }
    }
}

mod products {
    use super::*;
    use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
    use crate::staff::hash_secret;
    use arena_proto::{ProductDelete, ProductUpsert, StaffUpsert};
    use base64::Engine;

    const GSTIN: &str = "27AAPFU0939F1ZV";

    fn jpeg(w: u16, h: u16) -> String {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        b.extend([0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([0x01, 0x01, 0x11, 0x00]);
        format!(
            "{}{}",
            arena_proto::IMAGE_PREFIX,
            base64::engine::general_purpose::STANDARD.encode(b)
        )
    }

    fn owner() -> Actor {
        Actor {
            kind: ActorKind::Owner,
            id: "o".into(),
        }
    }

    fn as_actor(a: Actor, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            kind: payload.kind(),
            payload,
            actor: a,
            base_rev,
            ..bootstrap()
        }
    }

    fn up(name: &str) -> ProductUpsert {
        ProductUpsert {
            id: None,
            name: name.into(),
            category: "Drinks".into(),
            price_minor: 12000,
            tax_rate_bp: None,
            hsn: String::new(),
            track_stock: true,
            image: String::new(),
            barcode: String::new(),
            kitchen: false,
            favourite: false,
            active: true,
            bundle: false,
            components: Vec::new(),
        }
    }

    async fn run(
        f: &Fixture,
        u: ProductUpsert,
        base_rev: Option<i64>,
    ) -> Result<Applied, AppError> {
        f.writer
            .submit_intent(as_actor(owner(), IntentPayload::ProductUpsert(u), base_rev))
            .await
    }

    async fn make(f: &Fixture, u: ProductUpsert) -> String {
        run(f, u, None).await.unwrap().changes[0].row_id.clone()
    }

    async fn gst(f: &Fixture) {
        let tax = format!(
            r#"{{"country":"IN","gst":true,"gstin":"{GSTIN}","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}}"#
        );
        let rev = reopen(f)
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == "branch.tax")
            .unwrap()
            .rev;
        f.writer
            .submit_intent(set("branch.tax", &tax, Some(rev)))
            .await
            .unwrap();
    }

    fn row(f: &Fixture, id: &str) -> arena_store::ProductRow {
        reopen(f).product(id).unwrap().unwrap()
    }

    async fn refused(f: &Fixture, u: ProductUpsert) -> (ErrorCode, Option<String>) {
        let before = (snapshot(f), reopen(f).rows(Table::Products).unwrap());
        let e = run(f, u, None).await.unwrap_err();
        let after = (snapshot(f), reopen(f).rows(Table::Products).unwrap());
        assert_eq!(
            (&after.0.0, &after.0.1, &after.0.2, &after.1),
            (&before.0.0, &before.0.1, &before.0.2, &before.1)
        );
        (e.code, e.field)
    }

    #[tokio::test]
    async fn create_stores_every_field_with_the_tax_default() {
        let f = bootstrapped().await;
        let mut u = up("Cola");
        u.hsn = "2202".into();
        u.barcode = " 8901234567890 ".into();
        u.kitchen = true;
        u.favourite = true;
        u.image = jpeg(512, 400);
        let id = make(&f, u.clone()).await;
        let r = row(&f, &id);
        assert_eq!(
            (
                r.name.as_str(),
                r.category.as_str(),
                r.price_minor,
                r.tax_rate_bp,
                r.hsn.as_str()
            ),
            ("Cola", "Drinks", 12000, 0, "2202")
        );
        assert_eq!(r.barcode.as_deref(), Some("8901234567890"));
        assert!(r.track_stock && r.kitchen && r.favourite && r.active);
        assert_eq!(r.image, u.image);
        assert_eq!(r.rev, 1);
        gst(&f).await;
        u.barcode = String::new();
        let id = make(&f, u).await;
        assert_eq!(row(&f, &id).tax_rate_bp, 500);
        let mut u = up("Tea");
        u.hsn = "090240".into();
        u.tax_rate_bp = Some(1200);
        let id = make(&f, u).await;
        assert_eq!(row(&f, &id).tax_rate_bp, 1200);
    }

    #[tokio::test]
    async fn gst_branches_need_a_4_6_or_8_digit_hsn() {
        let f = bootstrapped().await;
        gst(&f).await;
        for bad in [
            "",
            "   ",
            "123",
            "12345",
            "1234567",
            "123456789",
            "12A4",
            "+123",
        ] {
            let mut u = up("Cola");
            u.hsn = bad.into();
            assert_eq!(
                refused(&f, u).await,
                (ErrorCode::HsnRequired, Some("hsn".into())),
                "{bad:?}"
            );
        }
        for ok in ["2202", "220210", "22021010", " 2202 "] {
            let mut u = up("Cola");
            u.hsn = ok.into();
            make(&f, u).await;
        }
        let mut u = up("Cola");
        u.hsn = "2202".into();
        u.tax_rate_bp = Some(2801);
        assert_eq!(
            refused(&f, u).await,
            (ErrorCode::ValidationFailed, Some("tax_rate_bp".into()))
        );
    }

    #[tokio::test]
    async fn outside_gst_the_hsn_is_optional_but_checked_when_given() {
        let f = bootstrapped().await;
        make(&f, up("Cola")).await;
        let mut u = up("Tea");
        u.hsn = "abc".into();
        assert_eq!(
            refused(&f, u).await,
            (ErrorCode::ValidationFailed, Some("hsn".into()))
        );
        let mut u = up("Tea");
        u.tax_rate_bp = Some(10001);
        assert_eq!(
            refused(&f, u).await,
            (ErrorCode::ValidationFailed, Some("tax_rate_bp".into()))
        );
        let mut u = up("Tea");
        u.tax_rate_bp = Some(10000);
        make(&f, u).await;
    }

    #[tokio::test]
    async fn a_barcode_belongs_to_one_active_product() {
        let f = bootstrapped().await;
        let mut a = up("Cola");
        a.barcode = "111".into();
        let a_id = make(&f, a.clone()).await;
        let mut b = up("Fanta");
        b.barcode = " 111 ".into();
        assert_eq!(
            refused(&f, b.clone()).await,
            (ErrorCode::BarcodeTaken, Some("barcode".into()))
        );

        let mut keep = a.clone();
        keep.id = Some(a_id.clone());
        keep.name = "Cola 2".into();
        run(&f, keep, Some(1)).await.unwrap();

        let mut off = a.clone();
        off.id = Some(a_id.clone());
        off.active = false;
        run(&f, off, Some(2)).await.unwrap();
        let b_id = make(&f, b.clone()).await;

        let mut on = a.clone();
        on.id = Some(a_id.clone());
        let before = snapshot(&f);
        let e = run(&f, on, Some(3)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BarcodeTaken);
        assert_eq!(snapshot(&f), before);

        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: b_id }),
                Some(1),
            ))
            .await
            .unwrap();
        let mut on = a;
        on.id = Some(a_id.clone());
        run(&f, on, Some(3)).await.unwrap();
        assert!(row(&f, &a_id).active);

        let mut c = up("Sprite");
        c.barcode = "bad barcode".into();
        assert_eq!(
            refused(&f, c).await,
            (ErrorCode::ValidationFailed, Some("barcode".into()))
        );
        let mut c = up("Sprite");
        c.barcode = "9".repeat(49);
        assert_eq!(
            refused(&f, c).await,
            (ErrorCode::ValidationFailed, Some("barcode".into()))
        );
        let mut d = up("Sprite");
        d.barcode = "   ".into();
        let id = make(&f, d.clone()).await;
        assert_eq!(row(&f, &id).barcode, None);
        make(&f, d).await;
    }

    #[tokio::test]
    async fn images_must_be_small_jpegs() {
        let f = bootstrapped().await;
        for (image, _) in [
            (jpeg(513, 10), "wide"),
            (jpeg(10, 513), "tall"),
            ("data:image/png;base64,iVBORw0KGgo=".to_owned(), "png"),
            ("http://x/y.jpg".to_owned(), "url"),
            (format!("{}!!", arena_proto::IMAGE_PREFIX), "base64"),
        ] {
            let mut u = up("Cola");
            u.image = image;
            assert_eq!(
                refused(&f, u).await,
                (ErrorCode::ValidationFailed, Some("image".into()))
            );
        }
        let mut u = up("Cola");
        u.image = jpeg(1, 1);
        make(&f, u).await;
    }

    #[tokio::test]
    async fn field_checks() {
        let f = bootstrapped().await;
        for (field, tweak) in [
            (
                "name",
                Box::new(|u: &mut ProductUpsert| u.name = "  ".into())
                    as Box<dyn Fn(&mut ProductUpsert)>,
            ),
            ("name", Box::new(|u| u.name = "x".repeat(81))),
            ("category", Box::new(|u| u.category = "c".repeat(41))),
            ("category", Box::new(|u| u.category = "a\nb".into())),
            ("price_minor", Box::new(|u| u.price_minor = -1)),
            (
                "price_minor",
                Box::new(|u| u.price_minor = arena_proto::MAX_PRICE_MINOR + 1),
            ),
            ("id", Box::new(|u| u.id = Some("nope".into()))),
        ] {
            let mut u = up("Cola");
            tweak(&mut u);
            assert_eq!(
                refused(&f, u).await,
                (ErrorCode::ValidationFailed, Some(field.into())),
                "{field}"
            );
        }
        let mut u = up("Free");
        u.price_minor = 0;
        u.category = String::new();
        make(&f, u).await;
    }

    #[tokio::test]
    async fn edits_carry_base_rev_and_delete_is_soft() {
        let f = bootstrapped().await;
        let mgr = staff(&f, "mgr", ROLE_MANAGER).await;
        let by = Actor {
            kind: ActorKind::Staff,
            id: mgr,
        };
        let id = make(&f, up("Cola")).await;
        let mut u = up("Cola 2");
        u.id = Some(id.clone());
        let edit = |u: &ProductUpsert, rev| {
            f.writer.submit_intent(as_actor(
                by.clone(),
                IntentPayload::ProductUpsert(u.clone()),
                rev,
            ))
        };
        let e = edit(&u, Some(7)).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::Stale, Some("base_rev"))
        );
        let e = edit(&u, None).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("base_rev"))
        );
        assert_eq!(row(&f, &id).name, "Cola");
        edit(&u, Some(1)).await.unwrap();
        assert_eq!(row(&f, &id).rev, 2);
        run(&f, u.clone(), None).await.unwrap();
        let del = |rev| {
            f.writer.submit_intent(as_actor(
                by.clone(),
                IntentPayload::ProductDelete(ProductDelete { id: id.clone() }),
                rev,
            ))
        };
        assert_eq!(del(Some(1)).await.unwrap_err().code, ErrorCode::Stale);
        del(Some(3)).await.unwrap();
        let r = row(&f, &id);
        assert!(r.deleted_at.is_some() && r.name == "Cola 2");
        assert!(reopen(&f).products().unwrap().is_empty());
        assert_eq!(
            edit(&u, Some(4)).await.unwrap_err().field.as_deref(),
            Some("id")
        );
        assert_eq!(del(Some(4)).await.unwrap_err().field.as_deref(), Some("id"));
    }

    async fn staff(f: &Fixture, login: &str, role: &str) -> String {
        let rid = match reopen(f)
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["name"] == RowValue::Text(role.into()))
            .unwrap()["role_id"]
            .clone()
        {
            RowValue::Text(t) => t,
            other => panic!("{other:?}"),
        };
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: rid,
                    password_hash: Some(hash_secret("pw").unwrap()),
                    pin_hash: None,
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone()
    }

    #[tokio::test]
    async fn only_catalog_editors_write_products() {
        let f = bootstrapped().await;
        let counter = staff(&f, "ctr", ROLE_COUNTER).await;
        let manager = staff(&f, "mgr", ROLE_MANAGER).await;
        let by = |id: &str| Actor {
            kind: ActorKind::Staff,
            id: id.into(),
        };
        let before = snapshot(&f);
        let e = f
            .writer
            .submit_intent(as_actor(
                by(&counter),
                IntentPayload::ProductUpsert(up("Cola")),
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert_eq!(snapshot(&f), before);
        let applied = f
            .writer
            .submit_intent(as_actor(
                by(&manager),
                IntentPayload::ProductUpsert(up("Cola")),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(applied.changes[0].tbl, "products");
    }

    #[tokio::test]
    async fn availability_setup_count_and_catalog_entries() {
        let f = bootstrapped().await;
        let r = || crate::SnapshotReader::open(&f.path).unwrap();
        assert_eq!(r().setup().unwrap().unwrap().products_count, 0);
        let tracked = make(&f, up("Cola")).await;
        let mut u = up("Chips");
        u.track_stock = false;
        let untracked = make(&f, u).await;
        let mut u = up("Old");
        u.active = false;
        let off = make(&f, u).await;
        assert_eq!(r().setup().unwrap().unwrap().products_count, 2);
        let by_id = |id: &str| {
            r().products()
                .unwrap()
                .into_iter()
                .find(|p| p.id == id)
                .unwrap()
        };
        let t = by_id(&tracked);
        assert_eq!(
            (t.available, t.reason.as_deref(), t.quantity),
            (false, Some("out_of_stock"), 0)
        );
        assert!(by_id(&untracked).available);
        assert_eq!(by_id(&off).reason.as_deref(), Some("inactive"));
        let ids = vec![tracked.clone(), untracked, "gone".into()];
        let entries = r().catalog(&ids).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.available, e.reason.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (false, Some("out_of_stock")),
                (true, None),
                (false, Some("inactive"))
            ]
        );
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete {
                    id: tracked.clone(),
                }),
                Some(1),
            ))
            .await
            .unwrap();
        assert_eq!(r().setup().unwrap().unwrap().products_count, 1);
        assert_eq!(
            r().catalog(&[tracked]).unwrap()[0].reason.as_deref(),
            Some("inactive")
        );
    }

    #[tokio::test]
    async fn limits_are_accepted_exactly_at_the_edge() {
        let f = bootstrapped().await;
        gst(&f).await;
        let mut u = up(&"n".repeat(80));
        u.category = "c".repeat(40);
        u.barcode = "9".repeat(48);
        u.price_minor = arena_proto::MAX_PRICE_MINOR;
        u.tax_rate_bp = Some(2800);
        u.hsn = "22021010".into();
        u.image = jpeg(512, 512);
        make(&f, u).await;
        let mut u = up("Zero");
        u.price_minor = 0;
        u.hsn = "2202".into();
        u.tax_rate_bp = Some(0);
        make(&f, u).await;
    }

    #[tokio::test]
    async fn an_edit_writes_every_field_and_a_cleared_barcode_is_freed() {
        let f = bootstrapped().await;
        let mut a = up("Cola");
        a.barcode = "111".into();
        a.tax_rate_bp = Some(1200);
        let id = make(&f, a).await;
        let edited = ProductUpsert {
            id: Some(id.clone()),
            name: " Fanta ".into(),
            category: " Soda ".into(),
            price_minor: 777,
            tax_rate_bp: None,
            hsn: " 2202 ".into(),
            track_stock: false,
            image: jpeg(8, 8),
            barcode: String::new(),
            kitchen: true,
            favourite: true,
            active: false,
            bundle: false,
            components: Vec::new(),
        };
        run(&f, edited.clone(), Some(1)).await.unwrap();
        let r = row(&f, &id);
        assert_eq!(
            (
                r.name.as_str(),
                r.category.as_str(),
                r.price_minor,
                r.tax_rate_bp,
                r.hsn.as_str(),
                r.track_stock,
                r.barcode.clone(),
                r.kitchen,
                r.favourite,
                r.active,
                r.rev
            ),
            (
                "Fanta", "Soda", 777, 1200, "2202", false, None, true, true, false, 2
            )
        );
        assert_eq!(r.image, edited.image);
        let mut b = up("Sprite");
        b.barcode = "111".into();
        make(&f, b).await;
        f.clock.advance(5);
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: id.clone() }),
                Some(2),
            ))
            .await
            .unwrap();
        assert_eq!(row(&f, &id).deleted_at, Some(T0 + 5));
    }

    #[tokio::test]
    async fn an_inactive_product_may_hold_a_barcode_it_cannot_reactivate_with() {
        let f = bootstrapped().await;
        let mut a = up("Cola");
        a.barcode = "111".into();
        make(&f, a.clone()).await;
        let mut b = a.clone();
        b.name = "Fanta".into();
        b.active = false;
        let b_id = make(&f, b.clone()).await;
        b.id = Some(b_id.clone());
        b.active = true;
        let before = reopen(&f).rows(Table::Products).unwrap();
        let e = run(&f, b, Some(1)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BarcodeTaken);
        assert_eq!(reopen(&f).rows(Table::Products).unwrap(), before);
    }

    #[tokio::test]
    async fn the_owner_may_omit_base_rev_but_not_send_a_stale_one() {
        let f = bootstrapped().await;
        let id = make(&f, up("Cola")).await;
        let mut u = up("Cola 2");
        u.id = Some(id.clone());
        let e = run(&f, u.clone(), Some(9)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
        assert_eq!(row(&f, &id).name, "Cola");
        run(&f, u, None).await.unwrap();
        assert_eq!(row(&f, &id).name, "Cola 2");
    }

    #[tokio::test]
    async fn deleting_a_missing_product_is_refused() {
        let f = bootstrapped().await;
        let e = f
            .writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: "nope".into() }),
                None,
            ))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("id"))
        );
    }
    fn parts(items: &[(&str, i64)]) -> Vec<arena_proto::BundleComponent> {
        items
            .iter()
            .map(|(id, qty)| arena_proto::BundleComponent {
                product_id: (*id).into(),
                qty: *qty,
            })
            .collect()
    }

    fn combo(name: &str, items: &[(&str, i64)]) -> ProductUpsert {
        let mut u = up(name);
        u.bundle = true;
        u.components = parts(items);
        u
    }

    fn component_rows(f: &Fixture) -> Vec<(String, String, i64)> {
        reopen(f)
            .rows(Table::BundleComponents)
            .unwrap()
            .into_iter()
            .map(|r| {
                let text = |c: &str| match &r[c] {
                    arena_store::RowValue::Text(t) => t.clone(),
                    other => panic!("{other:?}"),
                };
                let qty = match r["qty"] {
                    arena_store::RowValue::Integer(n) => n,
                    _ => panic!("qty"),
                };
                (text("bundle_id"), text("component_id"), qty)
            })
            .collect()
    }

    fn view(f: &Fixture, id: &str) -> arena_proto::Product {
        crate::SnapshotReader::open(&f.path)
            .unwrap()
            .products()
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
    }

    async fn untracked(f: &Fixture, name: &str) -> String {
        let mut u = up(name);
        u.track_stock = false;
        make(f, u).await
    }

    #[tokio::test]
    async fn a_bundle_has_its_own_price_tax_and_components() {
        let f = bootstrapped().await;
        let cola = make(&f, up("Cola")).await;
        let burger = untracked(&f, "Burger").await;
        let mut u = combo("Combo", &[(&cola, 2), (&burger, 1)]);
        u.price_minor = 5000;
        u.tax_rate_bp = Some(1200);
        let applied = run(&f, u, None).await.unwrap();
        let id = applied.changes[0].row_id.clone();
        let tables: Vec<_> = applied.changes.iter().map(|c| c.tbl.as_str()).collect();
        assert_eq!(
            tables,
            ["products", "bundle_components", "bundle_components"]
        );
        let r = row(&f, &id);
        assert_eq!(
            (r.bundle, r.track_stock, r.price_minor, r.tax_rate_bp),
            (true, false, 5000, 1200)
        );
        assert_eq!(row(&f, &cola).price_minor, 12000);
        let mut want = vec![(id.clone(), cola, 2), (id.clone(), burger, 1)];
        want.sort();
        let mut got = component_rows(&f);
        got.sort();
        assert_eq!(got, want);
        assert_eq!(view(&f, &id).components.len(), 2);
    }

    #[tokio::test]
    async fn bad_components_are_refused_and_nothing_is_written() {
        let f = bootstrapped().await;
        let cola = make(&f, up("Cola")).await;
        let inner = make(&f, combo("Inner", &[(&cola, 1)])).await;
        let gone = make(&f, up("Gone")).await;
        let other = untracked(&f, "Other").await;
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: gone.clone() }),
                Some(1),
            ))
            .await
            .unwrap();
        let many: Vec<(String, i64)> = (0..31).map(|i| (format!("p{i}"), 1)).collect();
        let many: Vec<(&str, i64)> = many.iter().map(|(a, b)| (a.as_str(), *b)).collect();
        let cases: Vec<(ProductUpsert, &str)> = vec![
            (combo("A", &[(&inner, 1)]), "components"),
            (combo("B", &[(&cola, 0)]), "components"),
            (combo("C", &[(&cola, -1)]), "components"),
            (combo("D", &[(&cola, 1001)]), "components"),
            (combo("E", &[(&cola, 1), (&cola, 2)]), "components"),
            (combo("F", &[("nope", 1)]), "components"),
            (combo("G", &[(&gone, 1)]), "components"),
            (combo("H", &[]), "components"),
            (combo("I", &many), "components"),
            (
                {
                    let mut u = up("J");
                    u.components = parts(&[(&cola, 1)]);
                    u
                },
                "components",
            ),
        ];
        for (u, field) in cases {
            let name = u.name.clone();
            let (code, f_) = refused(&f, u).await;
            assert_eq!(
                (code, f_.as_deref()),
                (ErrorCode::ValidationFailed, Some(field)),
                "{name}"
            );
        }
        let mut selfish = combo("Inner", &[(&inner, 1)]);
        selfish.id = Some(inner.clone());
        let e = run(&f, selfish, Some(1)).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("components"))
        );
        let mut nest = up("Cola");
        nest.id = Some(cola.clone());
        nest.bundle = true;
        nest.components = parts(&[(&other, 1)]);
        let e = run(&f, nest, Some(1)).await.unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("bundle"))
        );
        assert_eq!(component_rows(&f).len(), 1);
    }

    #[tokio::test]
    async fn editing_replaces_the_components_and_unbundling_clears_them() {
        let f = bootstrapped().await;
        let a = make(&f, up("A")).await;
        let b = make(&f, up("B")).await;
        let id = make(&f, combo("Combo", &[(&a, 1), (&b, 3)])).await;
        let mut u = combo("Combo", &[(&b, 2)]);
        u.id = Some(id.clone());
        let applied = run(&f, u, Some(1)).await.unwrap();
        let ops: Vec<_> = applied
            .changes
            .iter()
            .map(|c| (c.tbl.as_str(), c.op.as_str()))
            .collect();
        assert_eq!(
            ops,
            [
                ("products", "update"),
                ("bundle_components", "delete"),
                ("bundle_components", "delete"),
                ("bundle_components", "insert"),
            ]
        );
        assert_eq!(component_rows(&f), vec![(id.clone(), b.clone(), 2)]);
        let mut plain = up("Combo");
        plain.id = Some(id.clone());
        run(&f, plain, Some(2)).await.unwrap();
        assert!(component_rows(&f).is_empty());
        assert!(!row(&f, &id).bundle);
        assert!(row(&f, &id).track_stock);
    }

    #[tokio::test]
    async fn deleting_a_bundle_frees_its_components() {
        let f = bootstrapped().await;
        let a = make(&f, up("A")).await;
        let id = make(&f, combo("Combo", &[(&a, 1)])).await;
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: id.clone() }),
                Some(1),
            ))
            .await
            .unwrap();
        assert!(component_rows(&f).is_empty());
        let mut u = up("A");
        u.id = Some(a.clone());
        u.bundle = true;
        u.components = parts(&[]);
        let e = run(&f, u, Some(1)).await.unwrap_err();
        assert_eq!(e.field.as_deref(), Some("components"));
        let b = untracked(&f, "B").await;
        let mut u = combo("A", &[(&b, 1)]);
        u.id = Some(a);
        run(&f, u, Some(1)).await.unwrap();
    }

    #[tokio::test]
    async fn bundle_availability_follows_its_components() {
        let f = bootstrapped().await;
        let r = || crate::SnapshotReader::open(&f.path).unwrap();
        let tracked = make(&f, up("Cola")).await;
        let free = untracked(&f, "Burger").await;
        let all_free = make(&f, combo("Free", &[(&free, 2)])).await;
        let mixed = make(&f, combo("Mixed", &[(&tracked, 2), (&free, 1)])).await;
        let v = view(&f, &all_free);
        assert_eq!((v.available, v.reason.clone(), v.quantity), (true, None, 0));
        let v = view(&f, &mixed);
        assert_eq!(
            (v.available, v.reason.as_deref(), v.quantity),
            (false, Some("out_of_stock"), 0)
        );
        assert!(!v.track_stock);

        let mut off = up("Burger");
        off.id = Some(free.clone());
        off.track_stock = false;
        off.active = false;
        run(&f, off, Some(1)).await.unwrap();
        let v = view(&f, &all_free);
        assert_eq!(
            (v.available, v.reason.as_deref()),
            (false, Some("component_inactive"))
        );
        assert_eq!(
            v.components
                .iter()
                .map(|c| (c.name.as_str(), c.active))
                .collect::<Vec<_>>(),
            [("Burger", false)]
        );
        let ids = r().catalog(std::slice::from_ref(&free)).unwrap();
        let got: Vec<_> = ids
            .iter()
            .map(|e| (e.product_id.clone(), e.reason.clone()))
            .collect();
        assert!(
            got.contains(&(all_free.clone(), Some("component_inactive".into()))),
            "{got:?}"
        );
        assert!(
            got.contains(&(mixed.clone(), Some("component_inactive".into()))),
            "{got:?}"
        );

        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete {
                    id: tracked.clone(),
                }),
                Some(1),
            ))
            .await
            .unwrap();
        let v = view(&f, &mixed);
        assert_eq!(v.reason.as_deref(), Some("component_inactive"));
        assert!(v.components.iter().any(|c| c.name == "Cola" && !c.active));
        let gone = r().catalog(&[tracked]).unwrap();
        assert!(gone.iter().any(|e| e.product_id == mixed));

        let mut dead = combo("Mixed", &[(&free, 1)]);
        dead.id = Some(mixed.clone());
        dead.active = false;
        run(&f, dead, Some(1)).await.unwrap();
        assert_eq!(view(&f, &mixed).reason.as_deref(), Some("inactive"));
    }
    #[tokio::test]
    async fn limits_stale_edits_and_unchanged_components_are_exact() {
        let f = bootstrapped().await;
        let many: Vec<String> = {
            let mut ids = Vec::new();
            for i in 0..30 {
                ids.push(untracked(&f, &format!("P{i}")).await);
            }
            ids
        };
        let all: Vec<(&str, i64)> = many.iter().map(|i| (i.as_str(), 1000)).collect();
        let id = make(&f, combo("Big", &all)).await;
        assert_eq!(component_rows(&f).len(), 30);
        let mut same = combo("Big", &all);
        same.id = Some(id.clone());
        same.price_minor = 1;
        let applied = run(&f, same.clone(), Some(1)).await.unwrap();
        assert_eq!(applied.changes.len(), 1);
        let e = run(&f, same, Some(1)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Stale);
        assert_eq!(component_rows(&f).len(), 30);
    }

    #[tokio::test]
    async fn a_deleted_bundle_is_inactive_and_a_deleted_component_keeps_the_rows() {
        let f = bootstrapped().await;
        let a = untracked(&f, "A").await;
        let id = make(&f, combo("Combo", &[(&a, 1)])).await;
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: a.clone() }),
                Some(1),
            ))
            .await
            .unwrap();
        assert_eq!(component_rows(&f).len(), 1);
        assert_eq!(view(&f, &id).reason.as_deref(), Some("component_inactive"));
        f.writer
            .submit_intent(as_actor(
                owner(),
                IntentPayload::ProductDelete(ProductDelete { id: id.clone() }),
                Some(1),
            ))
            .await
            .unwrap();
        let r = crate::SnapshotReader::open(&f.path).unwrap();
        let e = &r.catalog(std::slice::from_ref(&id)).unwrap()[0];
        assert_eq!(
            (e.available, e.reason.as_deref()),
            (false, Some("inactive"))
        );
        assert!(component_rows(&f).is_empty());
    }
}

mod onboarding {
    use super::*;
    use arena_proto::{BootstrapOnboarding, BootstrapOwner, TaxMode};

    const HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNo";

    fn owner() -> BootstrapOwner {
        BootstrapOwner {
            name: "Asha Rao".into(),
            login: "+919876543210".into(),
            role: "Owner".into(),
            setup_code_hash: HASH.into(),
            setup_expires_at: T0 + 72 * 3_600_000,
        }
    }

    fn profile(country: &str, gst: bool) -> BranchBootstrap {
        let india_gst = country == "IN" && gst;
        BranchBootstrap {
            name: "Level Up".into(),
            branch_code: "LVL".into(),
            currency: "INR".into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 0,
            fy_start_month: 4,
            onboarding: Some(BootstrapOnboarding {
                country: country.into(),
                gst_registered: gst,
                gstin: if gst {
                    "27AAPFU0939F1ZV".into()
                } else {
                    String::new()
                },
                tax_mode: if india_gst {
                    TaxMode::GstIn
                } else {
                    TaxMode::Simple
                },
                owner_staff: owner(),
            }),
        }
    }

    fn by_owner(key: &str, b: BranchBootstrap) -> Intent {
        let mut i = intent_with(ActorKind::Owner, b);
        i.idempotency_key = key.into();
        i.actor.id = "tenant-1".into();
        i
    }

    fn key() -> String {
        ulid::Ulid::generate().to_string()
    }

    #[tokio::test]
    async fn the_owner_bootstrap_creates_branch_settings_and_the_owner_staff_in_one_intent() {
        let f = primary();
        let intent = by_owner(&key(), profile("IN", true));
        let applied = f.writer.submit_intent(intent.clone()).await.unwrap();
        let s = reopen(&f);
        let tax = arena_proto::TaxSettings::parse(match &setting(&s, "branch.tax")["value"] {
            RowValue::Text(t) => t,
            _ => panic!(),
        })
        .unwrap();
        assert_eq!(
            (
                tax.country.as_str(),
                tax.gst,
                tax.gstin.as_str(),
                tax.tax_mode()
            ),
            ("IN", true, "27AAPFU0939F1ZV", TaxMode::GstIn)
        );
        assert_eq!(
            setting(&s, "branch.code")["value"],
            RowValue::Text("LVL".into())
        );
        assert_eq!(
            setting(&s, "branch.code_locked_at")["value"],
            RowValue::Text("".into())
        );
        let staff = s.rows(Table::Staff).unwrap();
        assert_eq!(staff.len(), 1);
        let o = &staff[0];
        assert_eq!(o["login"], RowValue::Text("+919876543210".into()));
        assert_eq!(o["display_name"], RowValue::Text("Asha Rao".into()));
        assert_eq!(o["password_hash"], RowValue::Null);
        assert_eq!(o["pin_hash"], RowValue::Null);
        assert_eq!(o["setup_code_hash"], RowValue::Text(HASH.into()));
        assert_eq!(
            o["setup_expires_at"],
            RowValue::Integer(T0 + 72 * 3_600_000)
        );
        let RowValue::Text(role_id) = &o["role_id"] else {
            panic!()
        };
        let role = s
            .rows(Table::Roles)
            .unwrap()
            .into_iter()
            .find(|r| r["role_id"] == RowValue::Text(role_id.clone()))
            .unwrap();
        assert_eq!(role["name"], RowValue::Text("Owner".into()));
        let changes = s.changes().unwrap();
        assert_eq!(changes.len(), bootstrap_changes() + 1);
        assert_eq!(applied.changes.len(), changes.len());
        assert!(changes.iter().all(|c| c.intent_id == intent.intent_id
            && c.actor_kind == "owner"
            && c.actor_id == "tenant-1"));
        assert!(changes.iter().any(|c| c.tbl == "staff"));
    }

    #[tokio::test]
    async fn tax_mode_is_gst_in_only_for_a_registered_indian_branch() {
        for (country, gst, mode) in [
            ("IN", true, TaxMode::GstIn),
            ("IN", false, TaxMode::Simple),
            ("AE", false, TaxMode::Simple),
        ] {
            let f = primary();
            f.writer
                .submit_intent(by_owner(&key(), profile(country, gst)))
                .await
                .unwrap();
            let RowValue::Text(v) = setting(&reopen(&f), "branch.tax")["value"].clone() else {
                panic!()
            };
            let t = arena_proto::TaxSettings::parse(&v).unwrap();
            assert_eq!((t.country.as_str(), t.tax_mode()), (country, mode));
        }
        let f = primary();
        let mut bad = profile("AE", true);
        bad.onboarding.as_mut().unwrap().tax_mode = TaxMode::GstIn;
        assert_eq!(
            expect_err(&f, by_owner(&key(), bad)).await.field.as_deref(),
            Some("branch.tax.gst")
        );
        let mut wrong_mode = profile("IN", true);
        wrong_mode.onboarding.as_mut().unwrap().tax_mode = TaxMode::Simple;
        let e = expect_err(&f, by_owner(&key(), wrong_mode)).await;
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("tax_mode"))
        );
        assert_nothing_written(&reopen(&f));
    }

    #[tokio::test]
    async fn the_same_key_applies_once_and_a_different_key_is_already_bootstrapped() {
        let mut f = primary();
        let k = key();
        let first = f
            .writer
            .submit_intent(by_owner(&k, profile("IN", false)))
            .await
            .unwrap();
        f.events.try_recv().unwrap();
        let after_first = snapshot(&f);
        let again = f
            .writer
            .submit_intent(by_owner(&k, profile("IN", false)))
            .await
            .unwrap();
        assert_eq!(again.changes, first.changes);
        assert!(f.events.try_recv().is_err());
        assert_eq!(snapshot(&f), after_first);
        let e = expect_err(&f, by_owner(&key(), profile("IN", false))).await;
        assert_eq!(e.code, ErrorCode::AlreadyBootstrapped);
        let now = snapshot(&f);
        assert_eq!(
            (&now.0, &now.1, &now.2),
            (&after_first.0, &after_first.1, &after_first.2)
        );
        assert_eq!(reopen(&f).rows(Table::Staff).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bad_profiles_and_owners_are_refused_with_nothing_written() {
        let f = primary();
        type Edit = Box<dyn Fn(&mut BranchBootstrap)>;
        let cases: Vec<(&str, Edit)> = vec![
            ("branch_code", Box::new(|b| b.branch_code = "lvl".into())),
            ("branch_code", Box::new(|b| b.branch_code = "AB".into())),
            (
                "branch_code",
                Box::new(|b| b.branch_code = "ABCDEFG".into()),
            ),
            (
                "login",
                Box::new(|b| {
                    b.onboarding.as_mut().unwrap().owner_staff.login = "9876543210".into()
                }),
            ),
            (
                "login",
                Box::new(|b| b.onboarding.as_mut().unwrap().owner_staff.login = "+12".into()),
            ),
            (
                "display_name",
                Box::new(|b| b.onboarding.as_mut().unwrap().owner_staff.name = "  ".into()),
            ),
            (
                "role",
                Box::new(|b| b.onboarding.as_mut().unwrap().owner_staff.role = "Manager".into()),
            ),
            (
                "setup_code_hash",
                Box::new(|b| {
                    b.onboarding.as_mut().unwrap().owner_staff.setup_code_hash = "ABCD1234".into()
                }),
            ),
        ];
        for (field, edit) in cases {
            let mut b = profile("IN", false);
            edit(&mut b);
            let e = expect_err(&f, by_owner(&key(), b)).await;
            assert_eq!(
                (e.code, e.field.as_deref()),
                (ErrorCode::ValidationFailed, Some(field))
            );
        }
        assert_nothing_written(&reopen(&f));
    }

    #[tokio::test]
    async fn an_owner_may_bootstrap_only_with_the_onboarding_profile() {
        let f = primary();
        let e = expect_err(&f, by_owner(&key(), payload())).await;
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let mut staff = by_owner(&key(), profile("IN", false));
        staff.actor.kind = ActorKind::Staff;
        assert_eq!(
            expect_err(&f, staff).await.code,
            ErrorCode::PermissionDenied
        );
        assert_nothing_written(&reopen(&f));
    }

    async fn onboarded() -> Fixture {
        let mut f = primary_long_lease();
        f.writer
            .submit_intent(by_owner(&key(), profile("IN", false)))
            .await
            .unwrap();
        f.events.try_recv().unwrap();
        f
    }

    #[tokio::test]
    async fn branch_code_is_editable_until_locked_then_rejected() {
        let f = onboarded().await;
        let rev = |f: &Fixture| match setting(&reopen(f), "branch.code")["rev"] {
            RowValue::Integer(n) => n,
            _ => panic!(),
        };
        for bad in ["", "ab", "abc", "AB", "TOOLONG", "AB-1", "AB 1"] {
            let e = expect_err(&f, set("branch.code", bad, Some(rev(&f)))).await;
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{bad}");
        }
        f.writer
            .submit_intent(set("branch.code", "ARENA9", Some(rev(&f))))
            .await
            .unwrap();
        let summary = reopen(&f).branch_summary().unwrap().unwrap();
        assert_eq!(
            (summary.branch_code.as_str(), summary.code_locked),
            ("ARENA9", false)
        );
        let e = expect_err(&f, set("branch.code_locked_at", "1", Some(1))).await;
        assert_eq!(e.code, ErrorCode::ValidationFailed);

        let key_locked = reopen(&f)
            .write_tx(|tx| lock_branch_code(tx, T0 + 5))
            .unwrap();
        assert!(key_locked.is_some());
        let before = snapshot(&f);
        let e = expect_err(&f, set("branch.code", "NEW123", Some(rev(&f)))).await;
        assert_eq!(e.code, ErrorCode::BranchCodeLocked);
        assert_eq!(snapshot(&f), before);
        let summary = reopen(&f).branch_summary().unwrap().unwrap();
        assert_eq!(
            (summary.branch_code.as_str(), summary.code_locked),
            ("ARENA9", true)
        );
    }

    #[tokio::test]
    async fn lock_branch_code_sets_the_time_once_and_is_idempotent() {
        let f = onboarded().await;
        let mut store = reopen(&f);
        let first = store.write_tx(|tx| lock_branch_code(tx, 111)).unwrap();
        let again = store.write_tx(|tx| lock_branch_code(tx, 222)).unwrap();
        assert!(
            matches!(first, Some(arena_store::RowKey::Settings { ref key, .. }) if key == "branch.code_locked_at")
        );
        assert_eq!(again, None);
        assert_eq!(
            setting(&store, "branch.code_locked_at")["value"],
            RowValue::Text("111".into())
        );
    }

    #[tokio::test]
    async fn lock_branch_code_fails_on_a_database_with_no_branch() {
        let f = primary();
        let e = reopen(&f)
            .write_tx(|tx| lock_branch_code(tx, 1))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
    }
    fn identity() -> arena_store::HubIdentity {
        arena_store::HubIdentity {
            tenant_id: "tenant-1".into(),
            branch_id: "cloud-branch".into(),
            hub_id: "hub-1".into(),
            tier: arena_proto::Tier::Free,
            paired_at: T0,
        }
    }

    fn command(country: &str, hash: &str) -> arena_proto::BranchBootstrapCommand {
        let b = profile(country, false);
        let o = b.onboarding.unwrap();
        arena_proto::BranchBootstrapCommand {
            idempotency_key: key(),
            profile: arena_proto::BootstrapProfile {
                name: b.name,
                branch_code: b.branch_code,
                currency: b.currency,
                tz: b.tz,
                day_start_min: 0,
                country: o.country,
                gst_registered: false,
                gstin: String::new(),
                tax_mode: TaxMode::Simple,
            },
            owner_staff: BootstrapOwner {
                setup_code_hash: hash.into(),
                ..owner()
            },
        }
    }

    #[tokio::test]
    async fn pairing_with_a_bootstrap_is_all_or_nothing() {
        let mut f = primary_long_lease();
        let e = f
            .writer
            .record_pairing(identity(), "jwt".into(), Some(command("IN", "plain")))
            .await
            .unwrap_err();
        assert_eq!(
            (e.code, e.field.as_deref()),
            (ErrorCode::ValidationFailed, Some("setup_code_hash"))
        );
        let s = reopen(&f);
        assert!(s.hub_identity().unwrap().is_none() && s.cached_licence().unwrap().is_none());
        assert_nothing_written(&s);

        f.writer.submit_intent(bootstrap()).await.unwrap();
        f.events.try_recv().unwrap();
        let e = f
            .writer
            .record_pairing(identity(), "jwt".into(), Some(command("IN", HASH)))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::AlreadyBootstrapped);
        let s = reopen(&f);
        assert!(s.hub_identity().unwrap().is_none() && s.cached_licence().unwrap().is_none());
        assert!(s.rows(Table::Staff).unwrap().is_empty());
        assert!(f.events.try_recv().is_err());
    }

    #[tokio::test]
    async fn pairing_applies_the_command_once_with_the_locale_of_its_country() {
        for (country, fy) in [("IN", 4), ("AE", 1)] {
            let mut f = primary_long_lease();
            assert!(
                f.writer
                    .record_pairing(identity(), "jwt".into(), Some(command(country, HASH)))
                    .await
                    .unwrap()
            );
            let applied = f.events.try_recv().unwrap();
            assert!(f.events.try_recv().is_err());
            let s = reopen(&f);
            assert_eq!(s.hub_identity().unwrap().unwrap().tenant_id, "tenant-1");
            assert_eq!(s.cached_licence().unwrap().unwrap().jwt, "jwt");
            assert_eq!(
                s.rows(Table::Branch).unwrap()[0]["fy_start_month"],
                RowValue::Integer(fy)
            );
            assert_eq!(applied.changes.len(), bootstrap_changes() + 1);
            let again = f
                .writer
                .record_pairing(identity(), "jwt".into(), Some(command(country, HASH)))
                .await
                .unwrap();
            assert!(!again);
            assert!(f.events.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn a_locked_branch_code_answers_locked_before_any_other_check() {
        let f = onboarded().await;
        reopen(&f).write_tx(|tx| lock_branch_code(tx, T0)).unwrap();
        for (value, base_rev) in [("zz", Some(1)), ("NEW123", Some(99)), ("NEW123", None)] {
            let e = expect_err(&f, set("branch.code", value, base_rev)).await;
            assert_eq!(e.code, ErrorCode::BranchCodeLocked, "{value}");
        }
    }

    #[tokio::test]
    async fn seed_defaults_backfills_the_branch_code_rows_without_touching_existing_ones() {
        let f = onboarded().await;
        let mut store = reopen(&f);
        store
            .write_tx(|tx| {
                tx.execute_batch(
                    "UPDATE settings SET value = 'XYZ' WHERE key = 'branch.code';
                     DELETE FROM settings WHERE key = 'branch.code_locked_at';",
                )
                .map_err(arena_store::db_error)
            })
            .unwrap();
        f.writer.seed_defaults().await.unwrap().unwrap();
        let s = reopen(&f);
        assert_eq!(
            setting(&s, "branch.code")["value"],
            RowValue::Text("XYZ".into())
        );
        assert_eq!(
            setting(&s, "branch.code_locked_at")["value"],
            RowValue::Text("".into())
        );
        assert!(f.writer.seed_defaults().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_owner_actor_cannot_do_anything_but_the_onboarding_bootstrap() {
        let f = onboarded().await;
        let mut seed = intent_with(ActorKind::Owner, payload());
        seed.kind = IntentKind::BranchSeedDefaults;
        seed.payload = IntentPayload::BranchSeedDefaults(Default::default());
        assert_eq!(expect_err(&f, seed).await.code, ErrorCode::PermissionDenied);
        let mut device = by_owner(&key(), profile("IN", false));
        device.actor.kind = ActorKind::Device;
        assert_eq!(
            expect_err(&f, device).await.code,
            ErrorCode::PermissionDenied
        );
    }
}

mod set_credentials {
    use super::*;
    use arena_proto::{BootstrapOnboarding, BootstrapOwner, StaffSetCredentials, TaxMode};

    const CODE_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$Y29kZWhhc2hjb2RlaGFzaA";
    const PW_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$cGFzc3dvcmRoYXNocGFzcw";
    const PIN_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$cGluaGFzaHBpbmhhc2g";
    const LOGIN: &str = "+919876543210";

    async fn onboarded() -> (Fixture, String) {
        let f = primary_long_lease();
        let mut i = intent_with(
            ActorKind::Owner,
            BranchBootstrap {
                name: "Level Up".into(),
                branch_code: "LVL".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 0,
                fy_start_month: 4,
                onboarding: Some(BootstrapOnboarding {
                    country: "IN".into(),
                    gst_registered: false,
                    gstin: String::new(),
                    tax_mode: TaxMode::Simple,
                    owner_staff: BootstrapOwner {
                        name: "Asha Rao".into(),
                        login: LOGIN.into(),
                        role: "Owner".into(),
                        setup_code_hash: CODE_HASH.into(),
                        setup_expires_at: T0 + 72 * 3_600_000,
                    },
                }),
            },
        );
        i.actor.id = "tenant-1".into();
        f.writer.submit_intent(i).await.unwrap();
        let id = reopen(&f)
            .staff_with_role(LOGIN)
            .unwrap()
            .unwrap()
            .0
            .staff_id;
        (f, id)
    }

    fn set(id: &str, actor: &str, kind: ActorKind, hash: &str, now: i64) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::StaffSetCredentials,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev: None,
            actor: Actor {
                kind,
                id: actor.into(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::StaffSetCredentials(StaffSetCredentials {
                staff_id: id.into(),
                setup_code_hash: hash.into(),
                trusted_now_ms: now,
                password_hash: PW_HASH.into(),
                pin_hash: PIN_HASH.into(),
            }),
        }
    }

    fn own(id: &str, hash: &str, now: i64) -> Intent {
        set(id, id, ActorKind::Staff, hash, now)
    }

    #[tokio::test]
    async fn the_code_sets_the_hashes_once_and_is_spent() {
        let (f, id) = onboarded().await;
        let before = reopen(&f).staff_with_role(LOGIN).unwrap().unwrap().0;
        assert_eq!(
            (before.pin_hash.clone(), before.pin_verifier_version),
            (None, 1)
        );
        let applied = f
            .writer
            .submit_intent(own(&id, CODE_HASH, T0 + 1000))
            .await
            .unwrap();
        assert_eq!(applied.changes.len(), 1);
        let s = reopen(&f);
        let row = s.staff_with_role(LOGIN).unwrap().unwrap().0;
        assert_eq!(row.password_hash.as_deref(), Some(PW_HASH));
        assert_eq!(row.pin_hash.as_deref(), Some(PIN_HASH));
        assert_eq!(row.pin_verifier_version, 2);
        assert_eq!((row.setup_code_hash, row.setup_expires_at), (None, None));
        let change = s.changes().unwrap().pop().unwrap();
        assert_eq!(
            (
                change.intent_kind.as_str(),
                change.actor_kind.as_str(),
                change.actor_id.as_str(),
                change.tbl.as_str()
            ),
            ("staff.set_credentials", "staff", id.as_str(), "staff")
        );
        let e = expect_err(&f, own(&id, CODE_HASH, T0 + 2000)).await;
        assert_eq!(e.code, ErrorCode::SetupCodeInvalid);
    }

    #[tokio::test]
    async fn a_wrong_expired_or_unknown_code_is_one_refusal_and_writes_nothing() {
        let (f, id) = onboarded().await;
        let expiry = T0 + 72 * 3_600_000;
        let before = snapshot(&f);
        for intent in [
            own(
                &id,
                "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$b3RoZXJvdGhlcm90aGVy",
                T0,
            ),
            own(&id, CODE_HASH, expiry),
            own(&id, CODE_HASH, expiry + 1),
            own("01JNOSUCHSTAFF0000000000", CODE_HASH, T0),
        ] {
            let e = expect_err(&f, intent).await;
            assert_eq!(e.code, ErrorCode::SetupCodeInvalid);
            assert_eq!(e.message, "that setup code can't be used");
        }
        assert_eq!(snapshot(&f), before);
        f.writer
            .submit_intent(own(&id, CODE_HASH, expiry - 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn only_the_member_themself_may_submit_it() {
        let (f, id) = onboarded().await;
        for (kind, actor) in [
            (ActorKind::Owner, "tenant-1"),
            (ActorKind::Staff, "01JSOMEONEELSE00000000000"),
            (ActorKind::Device, id.as_str()),
            (ActorKind::Admin, id.as_str()),
        ] {
            let e = expect_err(&f, set(&id, actor, kind, CODE_HASH, T0)).await;
            assert_eq!(e.code, ErrorCode::PermissionDenied, "{kind:?}");
        }
        let mut elevated = own(&id, CODE_HASH, T0);
        elevated.elevated_by = Some(ElevatedBy {
            kind: ElevationKind::Pin,
            id: id.clone(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        });
        assert_eq!(
            expect_err(&f, elevated).await.code,
            ErrorCode::PermissionDenied
        );
    }

    #[tokio::test]
    async fn a_member_with_credentials_or_inactive_is_refused() {
        let (f, id) = onboarded().await;
        reopen(&f)
            .write_tx(|tx| {
                tx.execute("UPDATE staff SET active = 0", [])
                    .map_err(arena_store::db_error)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            expect_err(&f, own(&id, CODE_HASH, T0)).await.code,
            ErrorCode::SetupCodeInvalid
        );
        reopen(&f)
            .write_tx(|tx| {
                tx.execute("UPDATE staff SET active = 1, password_hash = ?1", [PW_HASH])
                    .map_err(arena_store::db_error)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            expect_err(&f, own(&id, CODE_HASH, T0)).await.code,
            ErrorCode::SetupCodeInvalid
        );
    }

    #[tokio::test]
    async fn plain_text_in_place_of_a_hash_is_a_validation_error() {
        let (f, id) = onboarded().await;
        for field in ["password_hash", "pin_hash"] {
            let mut i = own(&id, CODE_HASH, T0);
            if let IntentPayload::StaffSetCredentials(c) = &mut i.payload {
                match field {
                    "password_hash" => c.password_hash = "hunter2hunter2".into(),
                    _ => c.pin_hash = "1234".into(),
                }
            }
            let e = expect_err(&f, i).await;
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.field.as_deref(), Some(field));
        }
        assert!(
            reopen(&f)
                .staff_with_role(LOGIN)
                .unwrap()
                .unwrap()
                .0
                .setup_code_hash
                .is_some()
        );
    }

    #[tokio::test]
    async fn setting_the_pin_changes_the_manager_verifier() {
        let (f, id) = onboarded().await;
        let verifier = |f: &Fixture| {
            let v = crate::devices::DeviceDirectory::open(&f.path)
                .unwrap()
                .manager_verifier()
                .unwrap();
            (v.version, v.entries.len())
        };
        assert_eq!(verifier(&f).1, 0);
        f.writer
            .submit_intent(own(&id, CODE_HASH, T0))
            .await
            .unwrap();
        let (version, entries) = verifier(&f);
        assert_eq!(entries, 1);
        assert!(version > 1);
    }
}

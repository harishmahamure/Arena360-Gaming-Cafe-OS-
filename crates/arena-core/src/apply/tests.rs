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
            kind: ActorKind::Staff,
            id: "counter-1".into(),
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
        ("staff", "counter-1")
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
    assert_eq!(s.changes().unwrap().len(), 1 + DEFAULT_SETTINGS.len() + 1);
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
async fn settings_set_permission_stub() {
    let f = bootstrapped().await;
    for kind in [ActorKind::Device, ActorKind::Admin] {
        let mut intent = set("hub.locale", "en-GB", Some(1));
        intent.actor.kind = kind;
        assert_eq!(
            expect_err(&f, intent).await.code,
            ErrorCode::Forbidden,
            "{kind:?}"
        );
    }
    for (rev, kind) in (1..).zip([ActorKind::System, ActorKind::Owner, ActorKind::Staff]) {
        let mut intent = set("hub.locale", "en-GB", Some(rev));
        intent.actor.kind = kind;
        f.writer.submit_intent(intent).await.unwrap();
    }
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
        ErrorCode::Forbidden
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
        ErrorCode::Forbidden
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

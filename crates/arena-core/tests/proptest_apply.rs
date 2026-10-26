//! Property tests on `apply()` through the Writer: random sequences of bootstraps, edits with a
//! correct, stale or missing `base_rev`, valid and invalid values per key, retried idempotency
//! keys, reused keys with a perturbed body, expired intents and malformed keys, against a Hub on
//! a temp file with a `ManualClock`.
//!
//! After every step: `changes.seq` is 1..n with no gaps; a retried key gets its first outcome
//! back and the change count does not grow; a reused key with another body fails
//! `IDEMPOTENCY_CONFLICT` and writes nothing; a stale `base_rev` is rejected with the row
//! unchanged; a successful edit bumps `rev` by exactly 1; an edit by a staff member of each
//! built-in role (or an admin) is applied only with `settings.edit`, and a permission denial
//! writes nothing and is never recorded. After the sequence, replaying `changes` into an empty
//! database reproduces every change-log table.
//!
//! 64 cases by default; set `PROPTEST_CASES` for more.

use std::sync::Arc;

use arena_core::{
    Applied, DEFAULT_SETTINGS, HubStatus, INTENT_TTL_MS, LOCALE_KEYS, LeaseInfo, ManualClock, Role,
    Writer, WriterHandle, status_channel,
};
use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ErrorCode, Intent, IntentPayload, SettingsSet,
    StaffUpsert,
};

/// One argon2id hash shared by every seeded staff member (hashing is slow).
fn password_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| arena_core::staff::hash_secret("pw").unwrap())
}
use arena_store::{LeaseRow, Row, RowValue, Store, Table, write_lease};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::{Config, TestRunner};

const T0: i64 = 1_800_000_000_000;
const HUB: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";
const MAX_ADVANCE_MS: i64 = 3 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
enum Op {
    Bootstrap,
    /// `settings.set` of a valid value with `base_rev` equal to the row's `rev`.
    SetFresh {
        key: &'static str,
        value: String,
    },
    /// `settings.set` of a value the key's rule rejects, with a correct `base_rev`.
    SetInvalid {
        key: &'static str,
        value: String,
    },
    /// `settings.set` with `base_rev` one behind (or one ahead of) the row's `rev`.
    SetStale {
        key: &'static str,
        value: String,
        ahead: bool,
    },
    /// `settings.set` with no `base_rev`.
    SetMissingRev {
        key: &'static str,
        value: String,
    },
    /// Resubmits an earlier intent exactly, idempotency key included.
    Duplicate {
        pick: Index,
    },
    /// Reuses a recorded idempotency key with a different body.
    Perturb {
        pick: Index,
        how: Index,
    },
    /// `settings.set` issued just past the 7-day TTL.
    Expired {
        key: &'static str,
        value: String,
    },
    /// An intent whose idempotency key is not a ULID.
    MalformedKey {
        key: &'static str,
        which: Index,
    },
    /// A fresh `settings.set` by a staff member of a built-in role, or by an admin.
    AsStaff {
        key: &'static str,
        value: String,
        who: Index,
    },
    /// `product.upsert` of a new product by a staff member of a built-in role (or an admin).
    Product {
        name: String,
        barcode: String,
        bundle: bool,
        who: Index,
    },
    /// `stock.in` of a quantity and cost (valid or not) into the first tracked product, then
    /// maybe `stock.in.reverted` straight after.
    Stock {
        qty: i64,
        cost: i64,
        past: bool,
        undo: bool,
    },
    /// Moves the clock forward.
    Advance {
        ms: i64,
    },
}

/// The seeded staff roles (index 4 is an admin actor).
const STAFF_ROLES: [&str; 3] = ["Owner", "Manager", "Counter"];

const MALFORMED: &[&str] = &[
    "",
    "not-a-ulid",
    "01JAAAAAAAAAAAAAAAAAAAAAAU",
    "01JAAAAAAAAAAAAAAAAAAAAAA",
    "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
];

/// A known setting key with a value its rule accepts.
fn valid_setting() -> impl Strategy<Value = (&'static str, String)> {
    prop_oneof![
        (1..=30i64).prop_map(|n| ("idle_lock_minutes", n.to_string())),
        "[a-z]{2,3}(-[A-Za-z0-9]{2,8}){0,2}".prop_map(|v| ("hub.locale", v)),
        "[a-z0-9 !.]{0,16}".prop_map(|v| ("receipt.footer", v)),
        prop::sample::select(vec!["INR", "USD", "JPY", "KWD"])
            .prop_map(|v| ("branch.currency", v.to_owned())),
        prop::sample::select(vec!["Asia/Kolkata", "Asia/Dubai", "UTC"])
            .prop_map(|v| ("branch.tz", v.to_owned())),
        (0..=1439i64).prop_map(|n| ("branch.day_start_min", n.to_string())),
        (1..=60i64).prop_map(|n| ("billing.rounding_min", n.to_string())),
        (0..=240i64).prop_map(|n| ("billing.minimum_min", n.to_string())),
        (0..=10i64).prop_map(|n| ("billing.grace_min", n.to_string())),
        prop::sample::select(vec!["clean_up", "staff_handles"])
            .prop_map(|v| ("billing.time_up_mode", v.to_owned())),
    ]
}

/// A known setting key with a value its rule rejects.
fn invalid_setting() -> impl Strategy<Value = (&'static str, String)> {
    prop_oneof![
        prop_oneof![
            Just("0".to_owned()),
            Just("31".to_owned()),
            Just("+5".to_owned()),
            "[a-z]{1,4}",
            (31..=10_000i64).prop_map(|n| n.to_string()),
        ]
        .prop_map(|v| ("idle_lock_minutes", v)),
        prop_oneof![
            Just(String::new()),
            "[a-z]",
            "[a-z]{4,6}",
            "[a-z]{2}_[A-Z]{2}",
            "[a-z]{2}-",
        ]
        .prop_map(|v| ("hub.locale", v)),
        Just(("receipt.footer", "x".repeat(4097))),
        Just(("branch.currency", "XYZ".to_owned())),
        Just(("branch.tz", "Mars/Base".to_owned())),
        Just(("branch.day_start_min", "1440".to_owned())),
        prop_oneof![
            Just("0".to_owned()),
            Just("61".to_owned()),
            Just("+5".to_owned()),
            Just("1e1".to_owned()),
            "[a-z]{1,4}",
        ]
        .prop_map(|v| ("billing.rounding_min", v)),
        Just(("billing.minimum_min", "241".to_owned())),
        Just(("billing.grace_min", "11".to_owned())),
        Just(("billing.time_up_mode", "Clean up".to_owned())),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        1 => Just(Op::Bootstrap),
        4 => valid_setting().prop_map(|(key, value)| Op::SetFresh { key, value }),
        1 => invalid_setting().prop_map(|(key, value)| Op::SetInvalid { key, value }),
        2 => (valid_setting(), any::<bool>())
            .prop_map(|((key, value), ahead)| Op::SetStale { key, value, ahead }),
        1 => valid_setting().prop_map(|(key, value)| Op::SetMissingRev { key, value }),
        3 => any::<Index>().prop_map(|pick| Op::Duplicate { pick }),
        2 => (any::<Index>(), any::<Index>()).prop_map(|(pick, how)| Op::Perturb { pick, how }),
        1 => valid_setting().prop_map(|(key, value)| Op::Expired { key, value }),
        1 => (valid_setting(), any::<Index>())
            .prop_map(|((key, _), which)| Op::MalformedKey { key, which }),
        2 => (valid_setting(), any::<Index>())
            .prop_map(|((key, value), who)| Op::AsStaff { key, value, who }),
        2 => ("[A-Za-z]{1,12}", "[0-2]{0,2}", any::<bool>(), any::<Index>()).prop_map(
            |(name, barcode, bundle, who)| Op::Product {
                name,
                barcode,
                bundle,
                who,
            },
        ),
        3 => (
            prop_oneof![3 => 1..=1_000i64, 1 => -2..=2i64, 1 => 999_999..=1_000_002i64],
            prop_oneof![3 => 0..=100_000i64, 1 => -2..=2i64, 1 => Just(arena_proto::MAX_PRICE_MINOR), 1 => Just(arena_proto::MAX_PRICE_MINOR + 1)],
            prop::bool::weighted(0.1),
            any::<bool>(),
        )
            .prop_map(|(qty, cost, past, undo)| Op::Stock { qty, cost, past, undo }),
        1 => (0..=MAX_ADVANCE_MS).prop_map(|ms| Op::Advance { ms }),
    ]
}

/// One earlier submission and what the Hub did with it.
#[derive(Clone)]
struct Submitted {
    intent: Intent,
    first: Result<Applied, AppError>,
    /// The key now holds a dedupe record (a success or a deterministic rejection).
    recorded: bool,
    /// Resubmitting the exact intent must give `first` again: it is recorded, or its outcome
    /// is re-derived identically (`EXPIRED` stays expired, a malformed key stays malformed).
    stable: bool,
}

/// Which outcomes the Hub records, as the spec states them.
fn recorded(out: &Result<Applied, AppError>) -> bool {
    const FIELDS: &[&str] = &[
        "value",
        "base_rev",
        "currency",
        "tz",
        "name",
        "branch_code",
        "day_start_min",
        "fy_start_month",
        "intent_id",
    ];
    match out {
        Ok(_) => true,
        Err(e) => match e.code {
            ErrorCode::Stale | ErrorCode::AlreadyBootstrapped => true,
            ErrorCode::ValidationFailed => e.field.as_deref().is_some_and(|f| FIELDS.contains(&f)),
            _ => false,
        },
    }
}

/// A primary Hub on a temp file whose lease never expires (the clock moves by days).
struct Hub {
    _dir: tempfile::TempDir,
    /// Kept alive so the writer's status receiver stays connected.
    _status: tokio::sync::watch::Sender<HubStatus>,
    clock: Arc<ManualClock>,
    writer: WriterHandle,
    /// A second, read-only connection (the writer holds the process lock).
    reader: Store,
    rt: tokio::runtime::Runtime,
    /// Every submitted intent, except exact duplicates and perturbations.
    history: Vec<Submitted>,
    bootstrapped: bool,
    /// The staff ids seeded right after bootstrap, in [`STAFF_ROLES`] order.
    staff: Vec<String>,
}

impl Hub {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: HUB.into(),
            provisional: true,
            renewed_at: T0,
            expires_at: i64::MAX,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), HUB);
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease));
        let (status_tx, rx) = status_channel(status);
        let clock = Arc::new(ManualClock::new(T0));
        let writer = Writer::spawn(store, clock.clone(), rx).unwrap();
        let reader = Store::open(&path, T0).unwrap();
        assert!(!reader.holds_writer_lock());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        Self {
            _dir: dir,
            _status: status_tx,
            clock,
            writer,
            reader,
            rt,
            history: Vec::new(),
            bootstrapped: false,
            staff: Vec::new(),
        }
    }

    fn submit(&self, intent: Intent) -> Result<Applied, AppError> {
        self.rt.block_on(self.writer.submit_intent(intent))
    }

    /// Submits a new intent and remembers it for later duplicates and perturbations.
    fn submit_new(&mut self, intent: Intent) -> Result<Applied, AppError> {
        let first = self.submit(intent.clone());
        let is_recorded = recorded(&first);
        let rederived = matches!(
            first.as_ref().map_err(|e| e.code),
            Err(ErrorCode::Expired | ErrorCode::BadFrame)
        );
        self.history.push(Submitted {
            intent,
            first: first.clone(),
            recorded: is_recorded,
            stable: is_recorded || rederived,
        });
        first
    }

    fn intent(&self, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: self.clock_now(),
            base_rev,
            // Bootstrap needs the system actor; settings.set needs settings.edit, which the
            // owner holds.
            actor: Actor {
                kind: if matches!(payload, IntentPayload::BranchBootstrap(_)) {
                    ActorKind::System
                } else {
                    ActorKind::Owner
                },
                id: "proptest".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    fn clock_now(&self) -> i64 {
        use arena_core::Clock;
        self.clock.now_ms()
    }

    fn change_count(&self) -> usize {
        self.reader.changes().unwrap().len()
    }

    fn settings(&self) -> Vec<Row> {
        self.reader.rows(Table::Settings).unwrap()
    }

    fn setting(&self, key: &str) -> Option<Row> {
        self.settings()
            .into_iter()
            .find(|r| r["key"] == RowValue::Text(key.into()))
    }

    fn rev(&self, key: &str) -> Option<i64> {
        self.setting(key).map(|r| match r["rev"] {
            RowValue::Integer(n) => n,
            ref other => panic!("rev is {other:?}"),
        })
    }
}

fn set_payload(key: &str, value: &str) -> IntentPayload {
    IntentPayload::SettingsSet(SettingsSet {
        key: key.into(),
        value: value.into(),
    })
}

fn bootstrap_payload() -> IntentPayload {
    IntentPayload::BranchBootstrap(BranchBootstrap {
        name: "Dev Branch".into(),
        branch_code: "DEV01".into(),
        currency: "INR".into(),
        tz: "Asia/Kolkata".into(),
        day_start_min: 360,
        fy_start_month: 4,
        onboarding: None,
    })
}

fn code(r: &Result<Applied, AppError>) -> Option<(ErrorCode, Option<&str>)> {
    r.as_ref().err().map(|e| (e.code, e.field.as_deref()))
}

/// The log invariant: one epoch, `seq` 1..n with no gaps.
fn assert_gap_free(hub: &Hub) -> Result<(), TestCaseError> {
    let changes = hub.reader.changes().unwrap();
    for (i, c) in changes.iter().enumerate() {
        prop_assert_eq!(&c.epoch, &format!("1:{HUB}"));
        prop_assert_eq!(c.seq, i as i64 + 1, "seq gap at index {}", i);
    }
    Ok(())
}

/// The same idempotency key with another body, chosen by `how`.
fn perturb(intent: &Intent, how: &Index) -> Intent {
    let mut out = intent.clone();
    match how.index(3) {
        // A staff actor with another id would fail the permission step first.
        0 if out.actor.kind != ActorKind::Staff => out.actor.id = format!("{}-other", out.actor.id),
        0 => out.base_rev = Some(intent.base_rev.map_or(1_000, |r| r + 1_000)),
        1 => out.base_rev = Some(intent.base_rev.map_or(1_000, |r| r + 1_000)),
        _ => {
            out.payload = match &intent.payload {
                IntentPayload::SettingsSet(s) => set_payload(&s.key, &format!("{}~", s.value)),
                _ => set_payload("receipt.footer", "other"),
            };
            out.kind = out.payload.kind();
            // Keep an actor that passes the permission step, so the conflict is what fails.
            out.actor.kind = ActorKind::Owner;
        }
    }
    out
}

/// Runs one step and checks its own invariants.
fn step(hub: &mut Hub, op: &Op) -> Result<(), TestCaseError> {
    let changes_before = hub.change_count();
    let settings_before = hub.settings();
    match op {
        Op::Bootstrap => {
            let intent = hub.intent(bootstrap_payload(), None);
            let out = hub.submit_new(intent);
            if hub.bootstrapped {
                prop_assert_eq!(code(&out), Some((ErrorCode::AlreadyBootstrapped, None)));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                let roles: usize = arena_core::rbac::BUILTIN_ROLES
                    .iter()
                    .map(|r| 1 + arena_core::rbac::builtin_perms(r).len())
                    .sum();
                prop_assert_eq!(
                    hub.change_count(),
                    1 + DEFAULT_SETTINGS.len() + LOCALE_KEYS.len() + 3 + roles
                );
                hub.bootstrapped = true;
                // One staff member per built-in role (outside the history: not retried).
                let applied = out.unwrap();
                let role_ids: Vec<String> = applied
                    .changes
                    .iter()
                    .filter(|c| c.tbl == "roles")
                    .map(|c| c.row_id.clone())
                    .collect();
                for (i, name) in STAFF_ROLES.iter().enumerate() {
                    let at = arena_core::rbac::BUILTIN_ROLES
                        .iter()
                        .position(|r| r == name)
                        .unwrap();
                    let staff = hub.intent(
                        IntentPayload::StaffUpsert(StaffUpsert {
                            staff_id: None,
                            login: format!("staff{i}"),
                            display_name: (*name).into(),
                            role_id: role_ids[at].clone(),
                            password_hash: Some(password_hash().into()),
                            pin_hash: None,
                            active: true,
                            setup_code_hash: None,
                            setup_expires_at: None,
                        }),
                        None,
                    );
                    let applied = hub.submit(staff);
                    prop_assert!(applied.is_ok(), "{:?}", applied);
                    hub.staff.push(applied.unwrap().changes[0].row_id.clone());
                }
                prop_assert_eq!(
                    hub.change_count(),
                    1 + DEFAULT_SETTINGS.len() + LOCALE_KEYS.len() + 3 + roles + STAFF_ROLES.len()
                );
            }
        }
        Op::SetFresh { key, value } => {
            let rev = hub.rev(key);
            let intent = hub.intent(set_payload(key, value), Some(rev.unwrap_or(1)));
            let out = hub.submit_new(intent);
            match rev {
                Some(rev) => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(hub.rev(key), Some(rev + 1), "rev bumps by exactly 1");
                    prop_assert_eq!(
                        &hub.setting(key).unwrap()["value"],
                        &RowValue::Text(value.clone())
                    );
                    prop_assert_eq!(hub.change_count(), changes_before + 1);
                }
                None => {
                    prop_assert_eq!(code(&out), Some((ErrorCode::ValidationFailed, Some("key"))));
                    prop_assert_eq!(hub.change_count(), changes_before);
                }
            }
        }
        Op::SetInvalid { key, value } => {
            let rev = hub.rev(key);
            let intent = hub.intent(set_payload(key, value), Some(rev.unwrap_or(1)));
            let out = hub.submit_new(intent);
            let field = match rev {
                None => "key",
                Some(_) if key.starts_with("billing.") => key,
                Some(_) => "value",
            };
            prop_assert_eq!(code(&out), Some((ErrorCode::ValidationFailed, Some(field))));
            prop_assert_eq!(hub.settings(), settings_before);
            prop_assert_eq!(hub.change_count(), changes_before);
        }
        Op::SetStale { key, value, ahead } => {
            let rev = hub.rev(key);
            let base = rev.map_or(1, |r| if *ahead { r + 1 } else { r - 1 });
            let intent = hub.intent(set_payload(key, value), Some(base));
            let out = hub.submit_new(intent);
            if rev.is_some() {
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
            } else {
                prop_assert_eq!(code(&out), Some((ErrorCode::ValidationFailed, Some("key"))));
            }
            prop_assert_eq!(
                hub.settings(),
                settings_before,
                "a stale edit changes nothing"
            );
            prop_assert_eq!(hub.change_count(), changes_before);
        }
        Op::SetMissingRev { key, value } => {
            let intent = hub.intent(set_payload(key, value), None);
            let out = hub.submit_new(intent);
            prop_assert_eq!(
                code(&out),
                Some((ErrorCode::ValidationFailed, Some("base_rev")))
            );
            prop_assert_eq!(hub.settings(), settings_before);
            prop_assert_eq!(hub.change_count(), changes_before);
        }
        Op::Duplicate { pick } => {
            let stable: Vec<Submitted> = hub.history.iter().filter(|s| s.stable).cloned().collect();
            if stable.is_empty() {
                return Ok(());
            }
            let s = &stable[pick.index(stable.len())];
            let again = hub.submit(s.intent.clone());
            prop_assert_eq!(
                &again,
                &s.first,
                "a retried key gets its first outcome back"
            );
            prop_assert_eq!(
                hub.change_count(),
                changes_before,
                "and writes no change rows"
            );
            prop_assert_eq!(hub.settings(), settings_before);
        }
        Op::Perturb { pick, how } => {
            let recorded: Vec<Submitted> =
                hub.history.iter().filter(|s| s.recorded).cloned().collect();
            if recorded.is_empty() {
                return Ok(());
            }
            let s = &recorded[pick.index(recorded.len())];
            let out = hub.submit(perturb(&s.intent, how));
            prop_assert_eq!(
                code(&out),
                Some((ErrorCode::IdempotencyConflict, Some("idempotency_key")))
            );
            prop_assert_eq!(
                hub.change_count(),
                changes_before,
                "a conflict writes nothing"
            );
            prop_assert_eq!(hub.settings(), settings_before);
        }
        Op::Expired { key, value } => {
            let mut intent = hub.intent(set_payload(key, value), hub.rev(key).or(Some(1)));
            intent.issued_at = hub.clock_now() - INTENT_TTL_MS - 1;
            let out = hub.submit_new(intent);
            prop_assert_eq!(code(&out), Some((ErrorCode::Expired, None)));
            prop_assert_eq!(hub.settings(), settings_before);
            prop_assert_eq!(hub.change_count(), changes_before);
        }
        Op::MalformedKey { key, which } => {
            let mut intent = hub.intent(set_payload(key, "1"), hub.rev(key).or(Some(1)));
            intent.idempotency_key = MALFORMED[which.index(MALFORMED.len())].into();
            let out = hub.submit_new(intent);
            prop_assert_eq!(
                code(&out),
                Some((ErrorCode::BadFrame, Some("idempotency_key")))
            );
            prop_assert_eq!(hub.change_count(), changes_before);
            prop_assert_eq!(hub.settings(), settings_before);
        }
        Op::AsStaff { key, value, who } => {
            let rev = hub.rev(key);
            let pick = who.index(STAFF_ROLES.len() + 1);
            let mut intent = hub.intent(set_payload(key, value), Some(rev.unwrap_or(1)));
            intent.actor = match hub.staff.get(pick) {
                Some(id) if pick < STAFF_ROLES.len() => Actor {
                    kind: ActorKind::Staff,
                    id: id.clone(),
                },
                _ => Actor {
                    kind: ActorKind::Admin,
                    id: "admin".into(),
                },
            };
            let allowed = intent.actor.kind == ActorKind::Staff && pick <= 1;
            let records_before = hub.reader.applied_intent_count().unwrap();
            let out = hub.submit_new(intent);
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.settings(), settings_before);
                prop_assert_eq!(hub.change_count(), changes_before);
                prop_assert_eq!(
                    hub.reader.applied_intent_count().unwrap(),
                    records_before,
                    "a permission denial is never recorded"
                );
            } else if let Some(rev) = rev {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.rev(key), Some(rev + 1));
                let c = hub.reader.changes().unwrap().pop().unwrap();
                prop_assert_eq!(c.actor_kind.as_str(), "staff");
            }
        }
        Op::Product {
            name,
            barcode,
            bundle,
            who,
        } => {
            let component = hub
                .reader
                .products()
                .unwrap()
                .into_iter()
                .find(|p| !p.bundle)
                .map(|p| p.product_id);
            let pick = who.index(STAFF_ROLES.len() + 1);
            let mut intent = hub.intent(
                IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                    id: None,
                    name: name.clone(),
                    category: String::new(),
                    price_minor: 100,
                    tax_rate_bp: None,
                    hsn: String::new(),
                    track_stock: true,
                    image: String::new(),
                    barcode: barcode.clone(),
                    kitchen: false,
                    favourite: false,
                    active: true,
                    bundle: *bundle,
                    components: component
                        .iter()
                        .filter(|_| *bundle)
                        .map(|id| arena_proto::BundleComponent {
                            product_id: id.clone(),
                            qty: 1,
                        })
                        .collect(),
                }),
                None,
            );
            let staff = hub.staff.get(pick).filter(|_| pick < STAFF_ROLES.len());
            if pick > 0 || hub.staff.is_empty() {
                intent.actor = match staff {
                    Some(id) => Actor {
                        kind: ActorKind::Staff,
                        id: id.clone(),
                    },
                    None => Actor {
                        kind: ActorKind::Admin,
                        id: "admin".into(),
                    },
                };
            }
            let allowed = intent.actor.kind == ActorKind::Owner
                || (intent.actor.kind == ActorKind::Staff && pick == 1);
            let taken = !barcode.is_empty()
                && hub
                    .reader
                    .products()
                    .unwrap()
                    .iter()
                    .any(|p| p.barcode.as_deref() == Some(barcode.as_str()));
            let out = hub.submit_new(intent);
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else if taken {
                prop_assert_eq!(code(&out), Some((ErrorCode::BarcodeTaken, Some("barcode"))));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else if *bundle && component.is_none() {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::ValidationFailed, Some("components")))
                );
                prop_assert_eq!(hub.change_count(), changes_before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(
                    hub.change_count(),
                    changes_before + 1 + usize::from(*bundle)
                );
            }
        }
        Op::Stock {
            qty,
            cost,
            past,
            undo,
        } => {
            let Some(p) = hub
                .reader
                .products()
                .unwrap()
                .into_iter()
                .find(|p| !p.bundle && p.track_stock)
            else {
                return Ok(());
            };
            let id = p.product_id;
            let held = |hub: &Hub| {
                hub.reader
                    .stock(&id)
                    .unwrap()
                    .map_or((0, 0), |s| (s.qty, s.avg_cost_minor))
            };
            let (old_qty, old_avg) = held(hub);
            let intent = hub.intent(
                IntentPayload::StockIn(arena_proto::StockIn {
                    product_id: id.clone(),
                    qty: *qty,
                    unit_cost_minor: *cost,
                    supplier: "s".into(),
                    expiry: past.then(|| "2000-01-01".to_owned()),
                }),
                None,
            );
            let out = hub.submit_new(intent);
            let valid = (1..=arena_proto::MAX_STOCK_IN_QTY).contains(qty)
                && (0..=arena_proto::MAX_PRICE_MINOR).contains(cost)
                && !past
                && old_qty + qty <= arena_proto::MAX_STOCK_QTY;
            if !valid {
                prop_assert_eq!(
                    code(&out).map(|c| c.0),
                    Some(ErrorCode::ValidationFailed),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            let (new_qty, new_avg) = held(hub);
            prop_assert_eq!(new_qty, old_qty + qty);
            prop_assert_eq!(
                new_avg,
                arena_proto::weighted_avg_cost(old_qty, old_avg, *qty, *cost)
            );
            let (lo, hi) = if old_qty == 0 {
                (*cost, *cost)
            } else {
                (old_avg.min(*cost), old_avg.max(*cost))
            };
            prop_assert!((lo..=hi).contains(&new_avg));
            prop_assert_eq!(hub.change_count(), changes_before + 3);
            if *undo {
                let move_id = out
                    .unwrap()
                    .changes
                    .iter()
                    .find(|c| c.tbl == "stock_moves")
                    .unwrap()
                    .row_id
                    .clone();
                let revert = hub.intent(
                    IntentPayload::StockInReverted(arena_proto::StockInReverted { move_id }),
                    None,
                );
                let out = hub.submit_new(revert);
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(held(hub), (old_qty, old_avg));
                prop_assert_eq!(hub.change_count(), changes_before + 6);
            }
            let lots: i64 = hub
                .reader
                .lots(&id)
                .unwrap()
                .iter()
                .map(|l| l.qty_remaining)
                .sum();
            prop_assert_eq!(lots, held(hub).0);
        }
        Op::Advance { ms } => hub.clock.advance(*ms),
    }
    assert_gap_free(hub)
}

/// Replays the Hub's log into an empty database and compares the rows.
fn assert_replay_reproduces(hub: &Hub) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    let mut dest = Store::open(dir.path().join("replica.db"), T0).unwrap();
    let changes = hub.reader.changes().unwrap();
    let n = dest
        .write_tx_mut(|tx| arena_replay::replay(changes.clone(), tx))
        .unwrap();
    prop_assert_eq!(n, changes.len());
    for table in Table::ALL {
        prop_assert_eq!(
            dest.rows(table).unwrap(),
            hub.reader.rows(table).unwrap(),
            "{} after replay",
            table.name()
        );
    }
    Ok(())
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

#[test]
fn apply_invariants_hold_over_random_intent_sequences() {
    let mut runner = TestRunner::new(Config {
        cases: cases(),
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(&prop::collection::vec(op(), 1..40), |ops| {
            let mut hub = Hub::new();
            for op in &ops {
                step(&mut hub, op)?;
            }
            assert_replay_reproduces(&hub)
        })
        .unwrap();
}

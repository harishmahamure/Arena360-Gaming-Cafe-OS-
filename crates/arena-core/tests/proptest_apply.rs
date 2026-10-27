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
    Applied, Clock, DEFAULT_SETTINGS, HubStatus, INTENT_TTL_MS, LOCALE_KEYS, LeaseInfo,
    ManualClock, Role, Writer, WriterHandle, status_channel,
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
    /// `plan.upsert` of a pack, block or pass (valid or not) for a PC group, or for a PC and a
    /// console group together, by a staff member of a built-in role (or an admin); or an edit of
    /// the first plan with a fresh or stale `base_rev`.
    Plan {
        kind: u8,
        price: i64,
        minutes: i64,
        mix: bool,
        edit: bool,
        stale: bool,
        who: Index,
    },
    /// `rule.upsert` of a weekly or date rule (valid or not) for the PC group, by a staff member
    /// of a built-in role (or an admin); or an edit of the first rule with a fresh or stale
    /// `base_rev`. Overlapping rules save, and every conflict is reported on both sides.
    Rule {
        weekly: bool,
        mask: i64,
        percent: i64,
        edit: bool,
        stale: bool,
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
    Player {
        user: u8,
        phone: u8,
        consent: bool,
    },
    PlayerEdit {
        phone: u8,
        consent: bool,
        stale: bool,
    },
    PlayerReset {
        known: bool,
    },
    Session {
        slot: u8,
        guest: bool,
    },
    /// A pack or wallet start on an existing PC for the first player (or a guest), which must
    /// either write exactly the session rows with an end time or write nothing.
    PrepaidStart {
        slot: u8,
        wallet: bool,
        guest: bool,
    },
    /// A pass (for the first player) or block (for them or a guest) start on an existing PC: it
    /// either writes the session rows with a window end, plus the block's open bill, or nothing.
    WindowStart {
        slot: u8,
        pass: bool,
        guest: bool,
    },
    /// An extend of the live session on an existing PC from the wallet, a pack or credit: it
    /// either moves the end out by exactly the minutes and writes its own settled order, or
    /// writes nothing.
    SessionExtend {
        slot: u8,
        source: u8,
        minutes: u8,
    },
    SessionEnd {
        slot: u8,
    },
    SessionSettle {
        slot: u8,
        short: bool,
        split: bool,
    },
    /// A cash sale of a pack or pass to a player (or short, or a second live pass), then maybe its Undo.
    PlanSell {
        player: u8,
        plan: u8,
        short: bool,
        undo: bool,
        late: bool,
    },
    /// A cash top-up for a player, then maybe its Undo (late or in time).
    WalletTopup {
        player: u8,
        amount: u16,
        undo: bool,
        late: bool,
    },
    /// A Settle that pays an open bill from the wallet (or too much from it, or on a guest bill).
    WalletPay {
        slot: u8,
        over: bool,
    },
    /// Sets a player's credit limit (below what they owe is refused).
    CreditSet {
        player: u8,
        limit: u16,
    },
    /// Collects some of a player's tab in cash (or too much, or from a player without credit), then maybe its Undo.
    CreditCollect {
        player: u8,
        amount: u16,
        over: bool,
        undo: bool,
        late: bool,
    },
    /// The system check that pauses credit 15+ days overdue (a no-op is `STALE`).
    CreditAutoPause,
    SessionExpire {
        slot: u8,
    },
    /// The system flips the rate of a running session when a rule boundary has passed (nothing
    /// due is `STALE`).
    RateFlip {
        slot: u8,
    },
    /// The system raises an alert on a device (a repeat is `STALE`), then staff acknowledge it
    /// (a repeat is `STALE`, an unknown id `NOT_FOUND`).
    Alert {
        slot: u8,
        time_up: bool,
        ack: bool,
        bad_ack: bool,
    },
    /// A Settle of an open bill wholly on credit, maybe with the manager override.
    CreditPay {
        slot: u8,
        over_ride: bool,
    },
    /// A Pause of a running session or a Resume of a paused one, maybe stale, then the repeat.
    SessionPause {
        slot: u8,
        reason: u8,
        stale: bool,
    },
    /// Links a session (guest or not, running or ended) to an existing player, maybe stale.
    SessionAssign {
        slot: u8,
        player: u8,
        stale: bool,
    },
    /// An End (or, with `settle`, a Settle) and then its `.reverted` twin, maybe late or stale.
    SessionUndo {
        slot: u8,
        settle: bool,
        late: bool,
        stale: bool,
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
        2 => (
            0..3u8,
            prop_oneof![3 => 1..=100_000i64, 1 => -2..=2i64],
            prop_oneof![3 => 1..=1_000i64, 1 => -2..=2i64],
            prop::bool::weighted(0.2),
            any::<bool>(),
            prop::bool::weighted(0.3),
            any::<Index>(),
        )
            .prop_map(|(kind, price, minutes, mix, edit, stale, who)| Op::Plan {
                kind,
                price,
                minutes,
                mix,
                edit,
                stale,
                who,
            }),
        2 => (
            any::<bool>(),
            prop_oneof![5 => 1..=127i64, 1 => 0..=0i64, 1 => 128..=129i64],
            prop_oneof![5 => -10_000..=5_000i64, 1 => -10_003..=-10_001i64],
            any::<bool>(),
            prop::bool::weighted(0.3),
            any::<Index>(),
        )
            .prop_map(|(weekly, mask, percent, edit, stale, who)| Op::Rule {
                weekly,
                mask,
                percent,
                edit,
                stale,
                who,
            }),
        3 => (
            prop_oneof![3 => 1..=1_000i64, 1 => -2..=2i64, 1 => 999_999..=1_000_002i64],
            prop_oneof![3 => 0..=100_000i64, 1 => -2..=2i64, 1 => Just(arena_proto::MAX_PRICE_MINOR), 1 => Just(arena_proto::MAX_PRICE_MINOR + 1)],
            prop::bool::weighted(0.1),
            any::<bool>(),
        )
            .prop_map(|(qty, cost, past, undo)| Op::Stock { qty, cost, past, undo }),
        2 => (0..4u8, 0..4u8, any::<bool>()).prop_map(|(user, phone, consent)| Op::Player {
            user,
            phone,
            consent,
        }),
        2 => (0..4u8, any::<bool>(), prop::bool::weighted(0.2)).prop_map(
            |(phone, consent, stale)| Op::PlayerEdit {
                phone,
                consent,
                stale,
            }
        ),
        1 => prop::bool::weighted(0.8).prop_map(|known| Op::PlayerReset { known }),
        3 => (0..3u8, any::<bool>()).prop_map(|(slot, guest)| Op::Session { slot, guest }),
        1 => (0..3u8, any::<bool>(), any::<bool>()).prop_map(|(slot, pass, guest)| Op::WindowStart {
            slot,
            pass,
            guest
        }),
        2 => (0..3u8, any::<bool>(), prop::bool::weighted(0.2)).prop_map(|(slot, wallet, guest)| Op::PrepaidStart {
            slot,
            wallet,
            guest
        }),
        2 => (0..3u8, 0..3u8, 0..3u8).prop_map(|(slot, source, minutes)| Op::SessionExtend {
            slot,
            source,
            minutes
        }),
        2 => (0..3u8).prop_map(|slot| Op::SessionEnd { slot }),
        2 => (0..3u8, any::<bool>(), any::<bool>()).prop_map(|(slot, short, split)| Op::SessionSettle {
            slot,
            short,
            split
        }),
        2 => (any::<u8>(), 0..5000u16, any::<bool>(), prop::bool::weighted(0.3)).prop_map(
            |(player, amount, undo, late)| Op::WalletTopup {
                player,
                amount,
                undo,
                late
            }
        ),
        2 => (0..3u8, prop::bool::weighted(0.3)).prop_map(|(slot, over)| Op::WalletPay { slot, over }),
        2 => (any::<u8>(), any::<u8>(), prop::bool::weighted(0.2), any::<bool>(), prop::bool::weighted(0.3)).prop_map(
            |(player, plan, short, undo, late)| Op::PlanSell {
                player,
                plan,
                short,
                undo,
                late
            }
        ),
        2 => (any::<u8>(), 0..3000u16).prop_map(|(player, limit)| Op::CreditSet { player, limit }),
        2 => (
            any::<u8>(),
            0..3000u16,
            prop::bool::weighted(0.2),
            prop::bool::weighted(0.4),
            prop::bool::weighted(0.3)
        )
            .prop_map(|(player, amount, over, undo, late)| Op::CreditCollect {
                player,
                amount,
                over,
                undo,
                late
            }),
        1 => Just(Op::CreditAutoPause),
        2 => (0..3u8).prop_map(|slot| Op::SessionExpire { slot }),
        2 => (0..3u8).prop_map(|slot| Op::RateFlip { slot }),
        3 => (
            0..4u8,
            prop::bool::ANY,
            prop::bool::weighted(0.6),
            prop::bool::weighted(0.2)
        )
            .prop_map(|(slot, time_up, ack, bad_ack)| Op::Alert {
                slot,
                time_up,
                ack,
                bad_ack
            }),
        2 => (0..3u8, prop::bool::weighted(0.5)).prop_map(|(slot, over_ride)| Op::CreditPay {
            slot,
            over_ride
        }),
        3 => (0..3u8, 0..4u8, prop::bool::weighted(0.25)).prop_map(|(slot, reason, stale)| {
            Op::SessionPause { slot, reason, stale }
        }),
        2 => (0..6u8, any::<u8>(), prop::bool::weighted(0.25)).prop_map(|(slot, player, stale)| {
            Op::SessionAssign { slot, player, stale }
        }),
        3 => (0..3u8, any::<bool>(), prop::bool::weighted(0.25), prop::bool::weighted(0.25))
            .prop_map(|(slot, settle, late, stale)| Op::SessionUndo {
                slot,
                settle,
                late,
                stale
            }),
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
        Op::Plan {
            kind,
            price,
            minutes,
            mix,
            edit,
            stale,
            who,
        } => {
            if !hub.bootstrapped {
                return Ok(());
            }
            let pc = group_of(hub, "pc")?;
            let mut group_ids = vec![pc];
            if *mix {
                group_ids.push(group_of(hub, "console")?);
            }
            let existing = hub.reader.plans().unwrap().into_iter().next();
            let editing = existing.as_ref().filter(|_| *edit);
            let kind = match editing {
                Some((p, _)) => p.kind.clone(),
                None => ["pack", "block", "pass"][usize::from(*kind)].to_owned(),
            };
            let window = kind != "pack";
            let upsert = arena_proto::PlanUpsert {
                id: editing.map(|(p, _)| p.plan_id.clone()),
                plan_kind: kind.clone(),
                name: "Plan".into(),
                price_minor: *price,
                minutes: (kind == "pack").then_some(*minutes),
                validity_days: (kind != "block").then_some(7),
                window_start_min: window.then_some(1320),
                window_end_min: window.then_some(360),
                group_ids,
                active: true,
            };
            let base_rev = editing.map(|(p, _)| if *stale { p.rev + 1 } else { p.rev });
            let mut intent = hub.intent(IntentPayload::PlanUpsert(upsert), base_rev);
            let pick = who.index(STAFF_ROLES.len() + 1);
            if pick > 0 || hub.staff.is_empty() {
                intent.actor = match hub.staff.get(pick).filter(|_| pick < STAFF_ROLES.len()) {
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
            let before = hub.change_count();
            let out = hub.submit_new(intent);
            let bad_terms = *price < 1 || (kind == "pack" && *minutes < 1);
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), before);
            } else if bad_terms {
                prop_assert_eq!(
                    code(&out).map(|c| c.0),
                    Some(ErrorCode::ValidationFailed),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
            } else if *mix {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::GroupKindMismatch, Some("group_ids")))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else if editing.is_some() && *stale {
                prop_assert_eq!(code(&out).map(|c| c.0), Some(ErrorCode::Stale));
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
            }
            for (plan, groups) in hub.reader.plans().unwrap() {
                prop_assert!(!groups.is_empty(), "plan {} has no group", plan.plan_id);
                prop_assert!(plan.price_minor > 0);
            }
        }
        Op::Rule {
            weekly,
            mask,
            percent,
            edit,
            stale,
            who,
        } => {
            if !hub.bootstrapped {
                return Ok(());
            }
            let pc = group_of(hub, "pc")?;
            let existing = hub.reader.pricing_rules().unwrap().into_iter().next();
            let editing = existing.as_ref().filter(|_| *edit);
            let upsert = arena_proto::RuleUpsert {
                id: editing.map(|r| r.rule_id.clone()),
                name: "Rule".into(),
                group_id: pc,
                rule_kind: if *weekly { "weekly" } else { "date" }.into(),
                days_mask: weekly.then_some(*mask),
                start_min: weekly.then_some(1320),
                end_min: weekly.then_some(120),
                date_from: (!weekly).then(|| "2027-03-10".to_owned()),
                date_to: (!weekly).then(|| "2027-03-12".to_owned()),
                adjust_kind: "percent_bp".into(),
                adjust_value: *percent,
                priority: 0,
                active: true,
            };
            let base_rev = editing.map(|r| if *stale { r.rev + 1 } else { r.rev });
            let mut intent = hub.intent(IntentPayload::RuleUpsert(upsert), base_rev);
            let pick = who.index(STAFF_ROLES.len() + 1);
            if pick > 0 || hub.staff.is_empty() {
                intent.actor = match hub.staff.get(pick).filter(|_| pick < STAFF_ROLES.len()) {
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
            let before = hub.change_count();
            let out = hub.submit_new(intent);
            let bad_terms = *percent < -10_000 || (*weekly && !(1..=127).contains(mask));
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), before);
            } else if bad_terms {
                prop_assert_eq!(
                    code(&out).map(|c| c.0),
                    Some(ErrorCode::ValidationFailed),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
            } else if editing.is_some() && *stale {
                prop_assert_eq!(code(&out).map(|c| c.0), Some(ErrorCode::Stale));
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + 1);
            }
            let rules = hub.reader.pricing_rules().unwrap();
            let found = arena_core::rule_conflicts(&rules);
            for (i, list) in found.iter().enumerate() {
                for c in list {
                    let j = rules.iter().position(|r| r.rule_id == c.rule_id).unwrap();
                    let back = found[j].iter().find(|b| b.rule_id == rules[i].rule_id);
                    prop_assert!(back.is_some(), "conflict {} is not symmetric", c.rule_id);
                    prop_assert!(back.is_some_and(|b| b.other_wins != c.other_wins));
                }
                prop_assert!(rules[i].adjust_value >= -10_000);
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
        Op::Player {
            user,
            phone,
            consent,
        } => {
            const KEK: [u8; 32] = [3; 32];
            if !hub.bootstrapped {
                return Ok(());
            }
            let username = format!("user{user}");
            let phone = format!("+9190000000{phone}");
            let lookup = arena_core::players::phone_lookup(&KEK, &phone);
            let players = hub.reader.rows(Table::Players).unwrap();
            let held = |col: &str, v: &str| {
                players
                    .iter()
                    .any(|r| r[col] == RowValue::Text(v.to_owned()))
            };
            let (user_taken, phone_taken) =
                (held("username", &username), held("phone_lookup", &lookup));
            let form = arena_proto::PlayerForm {
                username,
                password: "secret1".into(),
                phone,
                name: "Asha".into(),
                email: String::new(),
                dob: String::new(),
                notes: String::new(),
                tags: Vec::new(),
                consent: *consent,
                policy_version: "1".into(),
            };
            let sealed = arena_core::players::seal(
                &KEK,
                &form,
                &ulid::Ulid::generate().to_string(),
                password_hash().to_owned(),
            )
            .unwrap();
            let out = hub.submit_new(hub.intent(IntentPayload::PlayerCreate(sealed), None));
            if user_taken || phone_taken {
                let want = if user_taken {
                    (ErrorCode::UsernameTaken, Some("username"))
                } else {
                    (ErrorCode::PhoneTaken, Some("phone"))
                };
                prop_assert_eq!(code(&out), Some(want));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), changes_before + 3);
                let consents = hub.reader.rows(Table::Consents).unwrap();
                let last = consents.last().unwrap();
                prop_assert_eq!(&last["granted"], &RowValue::Integer(i64::from(*consent)));
            }
        }
        Op::PlayerEdit {
            phone,
            consent,
            stale,
        } => {
            const KEK: [u8; 32] = [3; 32];
            let players = hub.reader.rows(Table::Players).unwrap();
            let Some(first) = players.first() else {
                return Ok(());
            };
            let (RowValue::Text(id), RowValue::Integer(rev)) = (&first["player_id"], &first["rev"])
            else {
                unreachable!()
            };
            let phone = format!("+9190000000{phone}");
            let lookup = arena_core::players::phone_lookup(&KEK, &phone);
            let other_holds = players.iter().any(|r| {
                r["phone_lookup"] == RowValue::Text(lookup.clone())
                    && r["player_id"] != first["player_id"]
            });
            let consents_before = hub.reader.rows(Table::Consents).unwrap().len();
            let was = hub
                .reader
                .latest_consent(id, "transactional")
                .unwrap()
                .map(|c| c.0);
            let wrapped = hub.reader.player_key(id).unwrap().unwrap();
            let form = arena_proto::PlayerEditForm {
                player_id: id.clone(),
                base_rev: *rev,
                phone,
                name: "Asha".into(),
                email: String::new(),
                dob: String::new(),
                notes: String::new(),
                tags: Vec::new(),
                consent: *consent,
                policy_version: "1".into(),
            };
            let update = arena_core::players::reseal(&KEK, &form, &wrapped).unwrap();
            let base = if *stale { rev + 1 } else { *rev };
            let out = hub.submit_new(hub.intent(IntentPayload::PlayerUpdate(update), Some(base)));
            if *stale {
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else if other_holds {
                prop_assert_eq!(code(&out), Some((ErrorCode::PhoneTaken, Some("phone"))));
                prop_assert_eq!(hub.change_count(), changes_before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                let appended = usize::from(was.unwrap_or(false) != *consent);
                prop_assert_eq!(hub.change_count(), changes_before + 1 + appended);
                prop_assert_eq!(
                    hub.reader.rows(Table::Consents).unwrap().len(),
                    consents_before + appended
                );
            }
        }
        Op::PlayerReset { known } => {
            let players = hub.reader.rows(Table::Players).unwrap();
            let Some(first) = players.first() else {
                return Ok(());
            };
            let RowValue::Text(id) = &first["player_id"] else {
                unreachable!()
            };
            let player_id = if *known {
                id.clone()
            } else {
                "missing".to_owned()
            };
            let out = hub.submit_new(hub.intent(
                IntentPayload::PlayerPasswordReset(arena_proto::PlayerPasswordReset {
                    player_id,
                    password_hash: password_hash().to_owned(),
                }),
                None,
            ));
            if *known {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), changes_before + 1);
            } else {
                prop_assert_eq!(code(&out), Some((ErrorCode::NotFound, None)));
                prop_assert_eq!(hub.change_count(), changes_before);
            }
        }
        Op::Session { slot, guest } => {
            if !hub.bootstrapped {
                return Ok(());
            }
            let slot = usize::from(*slot);
            let pcs = hub.reader.rows(Table::DevicesMeta).unwrap();
            let device = match pcs.get(slot) {
                Some(row) => text(row, "device_id"),
                None => {
                    let mut reg = arena_proto::DeviceRegister::new(
                        arena_proto::ClientClass::Paired,
                        "PC",
                        password_hash(),
                    );
                    reg.device_kind = arena_proto::DeviceKind::Pc;
                    let device = hub
                        .submit_new(hub.intent(IntentPayload::DeviceRegister(reg), None))
                        .unwrap()
                        .changes[0]
                        .row_id
                        .clone();
                    let group = hub
                        .submit_new(hub.intent(
                            IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                                id: None,
                                name: format!("PCs{}", pcs.len()),
                                group_kind: "pc".into(),
                                colour: "teal".into(),
                                sort_order: 1,
                            }),
                            None,
                        ))
                        .unwrap()
                        .changes[0]
                        .row_id
                        .clone();
                    for payload in [
                        IntentPayload::RateUpsert(arena_proto::RateUpsert {
                            group_id: group.clone(),
                            price_per_hour_minor: 12_000,
                        }),
                        IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                            device_id: device.clone(),
                            group_id: Some(group),
                        }),
                    ] {
                        let out = hub.submit_new(hub.intent(payload, None));
                        prop_assert!(out.is_ok(), "{:?}", out);
                    }
                    device
                }
            };
            let player = hub
                .reader
                .rows(Table::Players)
                .unwrap()
                .first()
                .map(|p| text(p, "player_id"))
                .filter(|_| !*guest);
            let live = |hub: &Hub| {
                hub.reader
                    .rows(Table::Sessions)
                    .unwrap()
                    .iter()
                    .filter(|s| {
                        text(s, "device_id") == device
                            && ["active", "paused", "grace"].contains(&text(s, "state").as_str())
                    })
                    .count()
            };
            let busy = live(hub) > 0;
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionStart(arena_proto::SessionStart {
                    device_id: device.clone(),
                    mode: arena_proto::SessionMode::Postpaid,
                    player_id: player,
                    pack_id: None,
                    plan_id: None,
                }),
                None,
            ));
            if busy {
                prop_assert_eq!(code(&out), Some((ErrorCode::PcBusy, Some("device_id"))));
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + 3);
            }
            prop_assert_eq!(live(hub), 1);
        }
        Op::WindowStart { slot, pass, guest } => {
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*slot))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let player = hub
                .reader
                .rows(Table::Players)
                .unwrap()
                .first()
                .map(|p| text(p, "player_id"))
                .filter(|_| !*guest || *pass);
            let owned = player.as_ref().and_then(|p| {
                hub.reader
                    .rows(Table::PlayerPacks)
                    .unwrap()
                    .iter()
                    .find(|r| text(r, "player_id") == *p && text(r, "kind") == "pass")
                    .map(|r| text(r, "id"))
            });
            let block = hub
                .reader
                .plans()
                .unwrap()
                .into_iter()
                .find(|(p, _)| p.kind == "block")
                .map(|(p, _)| p.plan_id);
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionStart(arena_proto::SessionStart {
                    device_id: device.clone(),
                    mode: if *pass {
                        arena_proto::SessionMode::Pass
                    } else {
                        arena_proto::SessionMode::Block
                    },
                    player_id: player,
                    pack_id: owned.filter(|_| *pass),
                    plan_id: block.filter(|_| !*pass),
                }),
                None,
            ));
            match &out {
                Ok(_) => {
                    prop_assert_eq!(hub.change_count(), before + if *pass { 3 } else { 5 });
                    let started = hub
                        .reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .filter(|s| text(s, "device_id") == device && text(s, "state") == "active")
                        .collect::<Vec<_>>();
                    prop_assert_eq!(started.len(), 1);
                    prop_assert!(matches!(started[0]["ends_at"], RowValue::Integer(_)));
                }
                Err(e) => {
                    prop_assert_eq!(hub.change_count(), before);
                    prop_assert!(
                        matches!(
                            e.code,
                            ErrorCode::PcBusy
                                | ErrorCode::ValidationFailed
                                | ErrorCode::NotFound
                                | ErrorCode::PackExpired
                                | ErrorCode::PlanNotForGroup
                                | ErrorCode::PassOutsideWindow
                        ),
                        "{:?}",
                        e
                    );
                }
            }
        }
        Op::PrepaidStart {
            slot,
            wallet,
            guest,
        } => {
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*slot))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let player = hub
                .reader
                .rows(Table::Players)
                .unwrap()
                .first()
                .map(|p| text(p, "player_id"))
                .filter(|_| !*guest);
            let pack = player.as_ref().and_then(|p| {
                hub.reader
                    .rows(Table::PlayerPacks)
                    .unwrap()
                    .iter()
                    .find(|r| text(r, "player_id") == *p)
                    .map(|r| text(r, "id"))
            });
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionStart(arena_proto::SessionStart {
                    device_id: device.clone(),
                    mode: if *wallet {
                        arena_proto::SessionMode::Wallet
                    } else {
                        arena_proto::SessionMode::Pack
                    },
                    player_id: player,
                    pack_id: pack.filter(|_| !*wallet),
                    plan_id: None,
                }),
                None,
            ));
            match &out {
                Ok(_) => {
                    prop_assert_eq!(hub.change_count(), before + 3);
                    let started = hub
                        .reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .filter(|s| text(s, "device_id") == device && text(s, "state") == "active")
                        .collect::<Vec<_>>();
                    prop_assert_eq!(started.len(), 1);
                    prop_assert!(matches!(started[0]["ends_at"], RowValue::Integer(_)));
                    if *wallet {
                        prop_assert!(int(&started[0], "reserved_minor") > 0);
                    }
                }
                Err(e) => {
                    prop_assert_eq!(hub.change_count(), before);
                    prop_assert!(
                        matches!(
                            e.code,
                            ErrorCode::PcBusy
                                | ErrorCode::ValidationFailed
                                | ErrorCode::InsufficientBalance
                                | ErrorCode::NotFound
                                | ErrorCode::PackExpired
                                | ErrorCode::PlanNotForGroup
                                | ErrorCode::PassOutsideWindow
                        ),
                        "{:?}",
                        e
                    );
                }
            }
        }
        Op::SessionExtend {
            slot,
            source,
            minutes,
        } => {
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*slot))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let Some(session) = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .find(|s| {
                    text(s, "device_id") == device
                        && ["active", "paused", "grace"].contains(&text(s, "state").as_str())
                })
            else {
                return Ok(());
            };
            let id = text(&session, "id");
            let source = [
                arena_proto::ExtendSource::Wallet,
                arena_proto::ExtendSource::Pack,
                arena_proto::ExtendSource::Credit,
            ][usize::from(*source)];
            let own = match &session["pack_id"] {
                RowValue::Text(t) => t.clone(),
                _ => String::new(),
            };
            let pack = hub
                .reader
                .rows(Table::PlayerPacks)
                .unwrap()
                .iter()
                .find(|r| text(r, "kind") == "pack" && text(r, "id") != own)
                .map(|r| text(r, "id"));
            let minutes = arena_proto::EXTEND_OPTIONS_MIN[usize::from(*minutes)];
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionExtend(arena_proto::SessionExtend {
                    session_id: id.clone(),
                    minutes,
                    source,
                    pack_id: pack.filter(|_| source == arena_proto::ExtendSource::Pack),
                    credit_override: None,
                }),
                None,
            ));
            match &out {
                Ok(_) => {
                    prop_assert!(hub.change_count() >= before + 3);
                    let after = hub
                        .reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .find(|s| text(s, "id") == id)
                        .unwrap();
                    prop_assert_eq!(
                        int(&after, "ends_at"),
                        int(&session, "ends_at") + minutes * 60_000
                    );
                    prop_assert_eq!(
                        int(&after, "extended_min"),
                        int(&session, "extended_min") + minutes
                    );
                    prop_assert_eq!(text(&after, "state") == "grace", false);
                }
                Err(e) => {
                    prop_assert_eq!(hub.change_count(), before);
                    prop_assert!(
                        matches!(
                            e.code,
                            ErrorCode::ValidationFailed
                                | ErrorCode::InsufficientBalance
                                | ErrorCode::NotFound
                                | ErrorCode::PackExpired
                                | ErrorCode::PlanNotForGroup
                                | ErrorCode::CreditLimit
                                | ErrorCode::CreditPaused
                                | ErrorCode::OverrideUsedToday
                                | ErrorCode::PastClosing
                                | ErrorCode::NoRate
                                | ErrorCode::Stale
                        ),
                        "{:?}",
                        e
                    );
                }
            }
        }
        Op::SessionEnd { slot } => {
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*slot))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let Some(session) = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .find(|s| {
                    text(s, "device_id") == device
                        && ["active", "paused", "grace"].contains(&text(s, "state").as_str())
                })
            else {
                return Ok(());
            };
            let (id, rev) = (text(&session, "id"), int(&session, "rev"));
            let before = hub.change_count();
            let payload = IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: id.clone(),
            });
            let out = hub.submit_new(hub.intent(payload.clone(), Some(rev)));
            prop_assert!(out.is_ok(), "{:?}", out);
            let prepaid = text(&session, "mode") != "postpaid";
            let wanted = if prepaid { "settled" } else { "open" };
            if prepaid {
                prop_assert!(hub.change_count() > before + 5);
            } else {
                prop_assert_eq!(hub.change_count(), before + 5);
            }
            let after = hub.change_count();
            let orders = hub.reader.rows(Table::Orders).unwrap();
            prop_assert_eq!(
                orders
                    .iter()
                    .filter(|o| {
                        o["session_id"] == RowValue::Text(id.clone()) && text(o, "status") == wanted
                    })
                    .count(),
                1
            );
            let out = hub.submit_new(hub.intent(payload, Some(rev)));
            prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
            prop_assert_eq!(hub.change_count(), after);
        }
        Op::SessionSettle { slot, short, split } => {
            let open: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "open")
                .collect();
            let Some(order) = open.get(usize::from(*slot) % open.len().max(1)) else {
                return Ok(());
            };
            let (id, rev, total) = (
                text(order, "id"),
                int(order, "rev"),
                int(order, "total_minor"),
            );
            let before = hub.change_count();
            let row = |method, amount_minor, reference: Option<&str>| arena_proto::PaymentRow {
                method,
                amount_minor,
                reference: reference.map(Into::into),
            };
            let first = (total / 2).max(1).min(total);
            let payments = match (*short, *split && total > 1) {
                (true, _) => vec![row(arena_proto::PaymentMethod::Cash, total - 1, None)],
                (false, true) => vec![
                    row(arena_proto::PaymentMethod::Cash, first, None),
                    row(arena_proto::PaymentMethod::Upi, total - first, Some("0042")),
                ],
                (false, false) => vec![row(arena_proto::PaymentMethod::Card, total, Some("T1"))],
            };
            let rows = payments.len();
            let payload = IntentPayload::SessionSettle(arena_proto::SessionSettle {
                order_id: id.clone(),
                credit_override: None,
                payments,
            });
            let out = hub.submit_new(hub.intent(payload.clone(), Some(rev)));
            if *short {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::PaymentShort, Some("payments")))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + rows + 1);
                let again = hub.submit_new(hub.intent(payload, Some(rev + 1)));
                prop_assert_eq!(code(&again), Some((ErrorCode::Stale, Some("order_id"))));
                prop_assert_eq!(hub.change_count(), before + rows + 1);
            }
            for o in hub.reader.rows(Table::Orders).unwrap() {
                if text(&o, "status") != "settled" {
                    continue;
                }
                let paid: i64 = hub
                    .reader
                    .rows(Table::Payments)
                    .unwrap()
                    .iter()
                    .filter(|p| text(p, "order_id") == text(&o, "id"))
                    .map(|p| int(p, "amount_minor"))
                    .sum();
                prop_assert_eq!(paid, int(&o, "total_minor"));
            }
        }
        Op::WalletTopup {
            player,
            amount,
            undo,
            late,
        } => {
            let players = hub.reader.rows(Table::Players).unwrap();
            let Some(p) = players.get(usize::from(*player) % players.len().max(1)) else {
                return Ok(());
            };
            let player_id = text(p, "player_id");
            let amount_minor = 100 + i64::from(*amount);
            let before = hub.change_count();
            let held = |hub: &Hub| {
                hub.reader
                    .rows(Table::Wallets)
                    .unwrap()
                    .iter()
                    .find(|w| text(w, "player_id") == player_id)
                    .map_or(0, |w| int(w, "balance_minor"))
            };
            let balance = held(hub);
            let out = hub.submit_new(hub.intent(
                IntentPayload::WalletTopup(arena_proto::WalletTopup {
                    player_id: player_id.clone(),
                    amount_minor,
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Cash,
                        amount_minor,
                        reference: None,
                    }],
                }),
                None,
            ));
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert_eq!(hub.change_count(), before + 4);
            prop_assert_eq!(held(hub), balance + amount_minor);
            if *undo {
                let order = hub
                    .reader
                    .rows(Table::Orders)
                    .unwrap()
                    .into_iter()
                    .rfind(|o| text(o, "kind") == "topup")
                    .unwrap();
                if *late {
                    hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
                }
                let after_forward = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::WalletTopupReverted(arena_proto::WalletTopupReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                if *late {
                    prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                    prop_assert_eq!(hub.change_count(), after_forward);
                    prop_assert_eq!(held(hub), balance + amount_minor);
                } else {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(hub.change_count(), after_forward + 4);
                    prop_assert_eq!(held(hub), balance);
                }
            }
        }
        Op::PlanSell {
            player,
            plan,
            short,
            undo,
            late,
        } => {
            let players = hub.reader.rows(Table::Players).unwrap();
            let plans: Vec<_> = hub
                .reader
                .plans()
                .unwrap()
                .into_iter()
                .map(|(p, _)| p)
                .filter(|p| p.active && p.kind != "block")
                .collect();
            let (Some(p), Some(chosen)) = (
                players.get(usize::from(*player) % players.len().max(1)),
                plans.get(usize::from(*plan) % plans.len().max(1)),
            ) else {
                return Ok(());
            };
            let player_id = text(p, "player_id");
            let amount_minor = if *short {
                chosen.price_minor - 1
            } else {
                chosen.price_minor
            };
            let now = hub.clock.now_ms();
            let live_pass = chosen.kind == "pass"
                && hub
                    .reader
                    .rows(Table::PlayerPacks)
                    .unwrap()
                    .iter()
                    .any(|k| {
                        text(k, "player_id") == player_id
                            && text(k, "kind") == "pass"
                            && int(k, "expires_at") > now
                    });
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::PlanSell(arena_proto::PlanSell {
                    player_id: player_id.clone(),
                    plan_id: chosen.plan_id.clone(),
                    credit_override: None,
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Cash,
                        amount_minor,
                        reference: None,
                    }],
                }),
                None,
            ));
            if live_pass {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::PassAlreadyActive, Some("plan_id")))
                );
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            if *short {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::PaymentShort, Some("payments")))
                );
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert_eq!(hub.change_count(), before + 4);
            if *undo {
                let order = hub
                    .reader
                    .rows(Table::Orders)
                    .unwrap()
                    .into_iter()
                    .rfind(|o| text(o, "kind") == "plan_sale")
                    .unwrap();
                if *late {
                    hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
                }
                let after_forward = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::PlanSellReverted(arena_proto::PlanSellReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                if *late {
                    prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                    prop_assert_eq!(hub.change_count(), after_forward);
                } else {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(hub.change_count(), after_forward + 4);
                }
            }
            let packs = hub.reader.rows(Table::PlayerPacks).unwrap();
            for k in &packs {
                prop_assert!(
                    hub.reader
                        .rows(Table::Orders)
                        .unwrap()
                        .iter()
                        .any(|o| text(o, "id") == text(k, "order_id"))
                );
            }
        }
        Op::WalletPay { slot, over } => {
            let open: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "open")
                .collect();
            let Some(order) = open.get(usize::from(*slot) % open.len().max(1)) else {
                return Ok(());
            };
            let (id, rev, total) = (
                text(order, "id"),
                int(order, "rev"),
                int(order, "total_minor"),
            );
            let balance = match &order["player_id"] {
                RowValue::Text(p) => hub
                    .reader
                    .rows(Table::Wallets)
                    .unwrap()
                    .iter()
                    .find(|w| text(w, "player_id") == *p)
                    .map_or(0, |w| int(w, "balance_minor")),
                _ => 0,
            };
            let guest = order["player_id"] == RowValue::Null;
            let wallet = if *over {
                balance + 1
            } else {
                balance.min(total)
            };
            if wallet == 0 {
                return Ok(());
            }
            let mut payments = vec![arena_proto::PaymentRow {
                method: arena_proto::PaymentMethod::Wallet,
                amount_minor: wallet,
                reference: None,
            }];
            if total > wallet {
                payments.push(arena_proto::PaymentRow {
                    method: arena_proto::PaymentMethod::Cash,
                    amount_minor: total - wallet,
                    reference: None,
                });
            }
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionSettle(arena_proto::SessionSettle {
                    order_id: id,
                    credit_override: None,
                    payments,
                }),
                Some(rev),
            ));
            if guest {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::ValidationFailed, Some("payments[0].method")))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else if *over {
                prop_assert_eq!(
                    code(&out),
                    Some((
                        ErrorCode::InsufficientBalance,
                        Some("payments[0].amount_minor")
                    ))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
            }
        }
        Op::CreditSet { player, limit } => {
            let players = hub.reader.rows(Table::Players).unwrap();
            let Some(p) = players.get(usize::from(*player) % players.len().max(1)) else {
                return Ok(());
            };
            let id = text(p, "player_id");
            let owed = hub
                .reader
                .rows(Table::CreditBalances)
                .unwrap()
                .iter()
                .find(|c| text(c, "player_id") == id)
                .map_or(0, |c| int(c, "outstanding_minor"));
            let limit = i64::from(*limit);
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::CreditSetLimit(arena_proto::CreditSetLimit {
                    player_id: id,
                    limit_minor: limit,
                    reminder_days: vec![3, 7, 14],
                }),
                None,
            ));
            if limit < owed {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::CreditBelowOutstanding, Some("limit_minor")))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
            }
        }
        Op::CreditCollect {
            player,
            amount,
            over,
            undo,
            late,
        } => {
            let players = hub.reader.rows(Table::Players).unwrap();
            let Some(p) = players.get(usize::from(*player) % players.len().max(1)) else {
                return Ok(());
            };
            let id = text(p, "player_id");
            let owed = hub
                .reader
                .rows(Table::CreditBalances)
                .unwrap()
                .iter()
                .find(|c| text(c, "player_id") == id)
                .map(|c| int(c, "outstanding_minor"));
            let wanted = if *over {
                owed.unwrap_or(0) + 1
            } else {
                i64::from(*amount).min(owed.unwrap_or(0))
            };
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::CreditCollect(arena_proto::CreditCollect {
                    player_id: id.clone(),
                    amount_minor: wanted,
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Cash,
                        amount_minor: wanted,
                        reference: None,
                    }],
                }),
                None,
            ));
            match owed {
                None => {
                    prop_assert_eq!(code(&out), Some((ErrorCode::NotFound, Some("player_id"))));
                    prop_assert_eq!(hub.change_count(), before);
                }
                Some(_) if wanted <= 0 || *over => {
                    prop_assert_eq!(
                        code(&out),
                        Some((ErrorCode::ValidationFailed, Some("amount_minor")))
                    );
                    prop_assert_eq!(hub.change_count(), before);
                }
                Some(owed) => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(hub.change_count(), before + 4);
                    let order = hub
                        .reader
                        .rows(Table::Orders)
                        .unwrap()
                        .into_iter()
                        .rfind(|o| text(o, "kind") == "credit_collect")
                        .unwrap();
                    let owed_now = |hub: &Hub| {
                        hub.reader
                            .rows(Table::CreditBalances)
                            .unwrap()
                            .iter()
                            .find(|c| text(c, "player_id") == id)
                            .map(|c| int(c, "outstanding_minor"))
                    };
                    prop_assert_eq!(owed_now(hub), Some(owed - wanted));
                    if *undo {
                        if *late {
                            hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
                        }
                        let after_forward = hub.change_count();
                        let out = hub.submit_new(hub.intent(
                            IntentPayload::CreditCollectReverted(
                                arena_proto::CreditCollectReverted {
                                    order_id: text(&order, "id"),
                                },
                            ),
                            Some(int(&order, "rev")),
                        ));
                        if *late {
                            prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                            prop_assert_eq!(hub.change_count(), after_forward);
                            prop_assert_eq!(owed_now(hub), Some(owed - wanted));
                        } else {
                            prop_assert!(out.is_ok(), "{:?}", out);
                            prop_assert_eq!(hub.change_count(), after_forward + 4);
                            prop_assert_eq!(owed_now(hub), Some(owed));
                        }
                    }
                }
            }
        }
        Op::Alert {
            slot,
            time_up,
            ack,
            bad_ack,
        } => {
            let devices: Vec<String> = hub
                .reader
                .rows(Table::Devices)
                .unwrap()
                .into_iter()
                .filter(|d| d["revoked_at"] == RowValue::Null)
                .map(|d| text(&d, "device_id"))
                .collect();
            if devices.is_empty() {
                return Ok(());
            }
            let device = devices[usize::from(*slot) % devices.len()].clone();
            let kind = if *time_up {
                arena_proto::AlertKind::TimeUp
            } else {
                arena_proto::AlertKind::Fixture
            };
            let open = |hub: &Hub| {
                hub.reader
                    .rows(Table::Alerts)
                    .unwrap()
                    .into_iter()
                    .filter(|a| {
                        a["acked_at"] == RowValue::Null
                            && text(a, "kind") == kind.as_str()
                            && a["device_id"] == RowValue::Text(device.clone())
                    })
                    .collect::<Vec<_>>()
            };
            let raise = |hub: &mut Hub| {
                let mut intent = hub.intent(
                    IntentPayload::AlertRaise(arena_proto::AlertRaise::new(
                        kind,
                        Some(device.clone()),
                    )),
                    None,
                );
                intent.actor.kind = ActorKind::System;
                hub.submit_new(intent)
            };
            let before = hub.change_count();
            let was_open = !open(hub).is_empty();
            let out = raise(hub);
            if was_open {
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, None)));
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + 1);
            }
            let now_open = open(hub);
            prop_assert_eq!(now_open.len(), 1);
            let again = raise(hub);
            prop_assert_eq!(code(&again), Some((ErrorCode::Stale, None)));
            let id = text(&now_open[0], "id");
            let after_raise = hub.change_count();
            if *bad_ack {
                let intent = hub.intent(
                    IntentPayload::AlertAck(arena_proto::AlertAck {
                        alert_id: "NOPE".into(),
                    }),
                    None,
                );
                let out = hub.submit_new(intent);
                prop_assert_eq!(code(&out), Some((ErrorCode::NotFound, Some("alert_id"))));
                prop_assert_eq!(hub.change_count(), after_raise);
            }
            if *ack {
                let intent = hub.intent(
                    IntentPayload::AlertAck(arena_proto::AlertAck {
                        alert_id: id.clone(),
                    }),
                    None,
                );
                let out = hub.submit_new(intent);
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), after_raise + 1);
                prop_assert!(open(hub).is_empty());
                let intent = hub.intent(
                    IntentPayload::AlertAck(arena_proto::AlertAck { alert_id: id }),
                    None,
                );
                let out = hub.submit_new(intent);
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, None)));
                prop_assert_eq!(hub.change_count(), after_raise + 1);
            }
        }
        Op::SessionExpire { slot } => {
            let Some(session) = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .filter(|s| text(s, "state") == "active")
                .nth(usize::from(*slot))
            else {
                return Ok(());
            };
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::SessionExpire(arena_proto::SessionExpire {
                    session_id: text(&session, "id"),
                }),
                None,
            );
            intent.actor.kind = ActorKind::System;
            let out = hub.submit_new(intent);
            match session["ends_at"] {
                RowValue::Null => prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::ValidationFailed, Some("session_id")))
                ),
                _ => prop_assert_eq!(code(&out), Some((ErrorCode::Stale, None))),
            }
            prop_assert_eq!(hub.change_count(), before);
        }
        Op::RateFlip { slot } => {
            let Some(session) = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .filter(|s| matches!(text(s, "state").as_str(), "active" | "paused"))
                .nth(usize::from(*slot))
            else {
                return Ok(());
            };
            let id = text(&session, "id");
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::SessionRateFlip(arena_proto::SessionRateFlip {
                    session_id: id.clone(),
                }),
                None,
            );
            intent.actor.kind = ActorKind::System;
            let out = hub.submit_new(intent);
            match code(&out) {
                Some((ErrorCode::Stale, _)) => prop_assert_eq!(hub.change_count(), before),
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
                None => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert!(hub.change_count() > before);
                    let open: Vec<_> = hub
                        .reader
                        .rows(Table::SessionSegments)
                        .unwrap()
                        .into_iter()
                        .filter(|g| text(g, "session_id") == id && g["to_at"] == RowValue::Null)
                        .collect();
                    prop_assert_eq!(open.len(), 1);
                    if text(&session, "mode") == "wallet" {
                        let now = hub.reader.rows(Table::Sessions).unwrap();
                        let s = now.iter().find(|s| text(s, "id") == id).unwrap();
                        prop_assert!(int(s, "ends_at") >= int(&open[0], "from_at"));
                        prop_assert!(int(s, "reserved_minor") <= int(&session, "reserved_minor"));
                    }
                }
            }
        }
        Op::CreditAutoPause => {
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::CreditAutoPause(arena_proto::CreditAutoPause { player_id: None }),
                None,
            );
            intent.actor.kind = ActorKind::System;
            let out = hub.submit_new(intent);
            match code(&out) {
                None => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert!(hub.change_count() > before);
                }
                Some(refused) => {
                    prop_assert_eq!(refused, (ErrorCode::Stale, None));
                    prop_assert_eq!(hub.change_count(), before);
                }
            }
            for c in hub.reader.rows(Table::CreditBalances).unwrap() {
                if int(&c, "credit_paused") == 1 {
                    prop_assert!(int(&c, "outstanding_minor") > 0);
                }
            }
        }
        Op::CreditPay { slot, over_ride } => {
            let open: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "open")
                .collect();
            let Some(order) = open.get(usize::from(*slot) % open.len().max(1)) else {
                return Ok(());
            };
            let (id, rev, total) = (
                text(order, "id"),
                int(order, "rev"),
                int(order, "total_minor"),
            );
            if total == 0 {
                return Ok(());
            }
            let credit = match &order["player_id"] {
                RowValue::Text(p) => hub
                    .reader
                    .rows(Table::CreditBalances)
                    .unwrap()
                    .into_iter()
                    .find(|c| text(c, "player_id") == *p)
                    .filter(|c| int(c, "limit_minor") > 0),
                _ => None,
            };
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionSettle(arena_proto::SessionSettle {
                    order_id: id,
                    credit_override: over_ride.then_some(true),
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Credit,
                        amount_minor: total,
                        reference: None,
                    }],
                }),
                Some(rev),
            ));
            if let Some((ErrorCode::CreditPaused, field)) = code(&out) {
                prop_assert_eq!(field, Some("payments[0].method"));
                prop_assert!(credit.is_some());
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            match credit {
                None => {
                    prop_assert_eq!(
                        code(&out),
                        Some((ErrorCode::ValidationFailed, Some("payments[0].method")))
                    );
                    prop_assert_eq!(hub.change_count(), before);
                }
                Some(c) if int(&c, "outstanding_minor") + total <= int(&c, "limit_minor") => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                }
                Some(_) if !over_ride => {
                    prop_assert_eq!(
                        code(&out),
                        Some((ErrorCode::CreditLimit, Some("payments[0].amount_minor")))
                    );
                    prop_assert_eq!(hub.change_count(), before);
                }
                Some(_) => match code(&out) {
                    None => prop_assert!(out.is_ok(), "{:?}", out),
                    Some(refused) => {
                        prop_assert_eq!(
                            refused,
                            (
                                ErrorCode::OverrideUsedToday,
                                Some("payments[0].amount_minor")
                            )
                        );
                        prop_assert_eq!(hub.change_count(), before);
                    }
                },
            }
        }
        Op::SessionAssign {
            slot,
            player,
            stale,
        } => {
            let sessions = hub.reader.rows(Table::Sessions).unwrap();
            let players = hub.reader.rows(Table::Players).unwrap();
            let (Some(session), Some(p)) = (
                sessions.get(usize::from(*slot)),
                players.get(usize::from(*player) % players.len().max(1)),
            ) else {
                return Ok(());
            };
            let (id, rev) = (text(session, "id"), int(session, "rev"));
            let order = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .find(|o| o["session_id"] == arena_store::RowValue::Text(id.clone()));
            let has_player = session["player_id"] != arena_store::RowValue::Null;
            let settled = order.as_ref().is_some_and(|o| text(o, "status") != "open");
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionAssignPlayer(arena_proto::SessionAssignPlayer {
                    session_id: id.clone(),
                    player_id: text(p, "player_id"),
                    create: None,
                }),
                Some(if *stale { rev - 1 } else { rev }),
            ));
            if *stale {
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
                prop_assert_eq!(hub.change_count(), before);
            } else if has_player || settled {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::ValidationFailed, Some("session_id")))
                );
                prop_assert_eq!(hub.change_count(), before);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(
                    hub.change_count(),
                    before + 1 + usize::from(order.is_some())
                );
            }
        }
        Op::SessionPause {
            slot,
            reason,
            stale,
        } => {
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*slot))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let find = |hub: &Hub| {
                hub.reader
                    .rows(Table::Sessions)
                    .unwrap()
                    .into_iter()
                    .find(|s| {
                        text(s, "device_id") == device
                            && ["active", "paused", "grace"].contains(&text(s, "state").as_str())
                    })
            };
            let Some(session) = find(hub) else {
                return Ok(());
            };
            let (id, rev, paused) = (
                text(&session, "id"),
                int(&session, "rev"),
                text(&session, "state") == "paused",
            );
            let before = hub.change_count();
            let pause_reason = arena_proto::PauseReason::ALL[usize::from(*reason)];
            let pause = |note: Option<&str>| {
                IntentPayload::SessionPause(arena_proto::SessionPause {
                    session_id: id.clone(),
                    reason: pause_reason,
                    note: note.map(Into::into),
                })
            };
            let resume = IntentPayload::SessionResume(arena_proto::SessionResume {
                session_id: id.clone(),
            });
            let based = if *stale { rev - 1 } else { rev };
            if paused {
                let out = hub.submit_new(hub.intent(resume.clone(), Some(based)));
                if *stale {
                    prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
                    prop_assert_eq!(hub.change_count(), before);
                    return Ok(());
                }
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + 2);
                let s = find(hub).unwrap();
                prop_assert_eq!(text(&s, "state"), "active".to_owned());
                prop_assert!(int(&s, "paused_total_ms") >= int(&session, "paused_total_ms"));
                let again = hub.submit_new(hub.intent(resume, Some(rev + 1)));
                prop_assert_eq!(
                    code(&again),
                    Some((ErrorCode::ValidationFailed, Some("session_id")))
                );
                prop_assert_eq!(hub.change_count(), before + 2);
            } else {
                let note = (pause_reason == arena_proto::PauseReason::Other).then_some("Fan noise");
                let wrong_note = if note.is_some() { None } else { Some("x") };
                let bad = hub.submit_new(hub.intent(pause(wrong_note), Some(rev)));
                prop_assert_eq!(
                    code(&bad),
                    Some((ErrorCode::ValidationFailed, Some("note")))
                );
                let out = hub.submit_new(hub.intent(pause(note), Some(based)));
                if *stale {
                    prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
                    prop_assert_eq!(hub.change_count(), before);
                    return Ok(());
                }
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(hub.change_count(), before + 1);
                let s = find(hub).unwrap();
                prop_assert_eq!(text(&s, "state"), "paused".to_owned());
                prop_assert_eq!(text(&s, "pause_reason"), pause_reason.as_str().to_owned());
                let again = hub.submit_new(hub.intent(pause(note), Some(rev + 1)));
                prop_assert_eq!(
                    code(&again),
                    Some((ErrorCode::ValidationFailed, Some("session_id")))
                );
                prop_assert_eq!(hub.change_count(), before + 1);
            }
        }
        Op::SessionUndo {
            slot,
            settle,
            late,
            stale,
        } => {
            let before = hub.change_count();
            let (forward, target, kind): (IntentPayload, String, &str) = if *settle {
                let open: Vec<_> = hub
                    .reader
                    .rows(Table::Orders)
                    .unwrap()
                    .into_iter()
                    .filter(|o| text(o, "status") == "open")
                    .collect();
                let Some(order) = open.get(usize::from(*slot) % open.len().max(1)) else {
                    return Ok(());
                };
                let (id, total) = (text(order, "id"), int(order, "total_minor"));
                let out = hub.submit_new(hub.intent(
                    IntentPayload::SessionSettle(arena_proto::SessionSettle {
                        order_id: id.clone(),
                        credit_override: None,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Card,
                            amount_minor: total,
                            reference: Some("T1".into()),
                        }],
                    }),
                    Some(int(order, "rev")),
                ));
                prop_assert!(out.is_ok(), "{:?}", out);
                (
                    IntentPayload::SessionSettleReverted(arena_proto::SessionSettleReverted {
                        order_id: id.clone(),
                    }),
                    id,
                    "settle",
                )
            } else {
                let Some(device) = hub
                    .reader
                    .rows(Table::DevicesMeta)
                    .unwrap()
                    .get(usize::from(*slot))
                    .map(|d| text(d, "device_id"))
                else {
                    return Ok(());
                };
                let Some(session) =
                    hub.reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .find(|s| {
                            text(s, "device_id") == device
                                && ["active", "paused", "grace"]
                                    .contains(&text(s, "state").as_str())
                        })
                else {
                    return Ok(());
                };
                let id = text(&session, "id");
                let out = hub.submit_new(hub.intent(
                    IntentPayload::SessionEnd(arena_proto::SessionEnd {
                        session_id: id.clone(),
                    }),
                    Some(int(&session, "rev")),
                ));
                prop_assert!(out.is_ok(), "{:?}", out);
                (
                    IntentPayload::SessionEndReverted(arena_proto::SessionEndReverted {
                        session_id: id.clone(),
                    }),
                    id,
                    "end",
                )
            };
            let after_forward = hub.change_count();
            let table = if *settle {
                Table::Orders
            } else {
                Table::Sessions
            };
            let rev = int(
                &hub.reader
                    .rows(table)
                    .unwrap()
                    .into_iter()
                    .find(|r| text(r, "id") == target)
                    .unwrap(),
                "rev",
            );
            if *late {
                hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
            }
            let based = if *stale { rev - 1 } else { rev };
            let out = hub.submit_new(hub.intent(forward, Some(based)));
            if *stale {
                prop_assert_eq!(code(&out), Some((ErrorCode::Stale, Some("base_rev"))));
                prop_assert_eq!(hub.change_count(), after_forward);
            } else if *late {
                prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                prop_assert_eq!(hub.change_count(), after_forward);
            } else {
                prop_assert!(out.is_ok(), "{:?}", out);
                let orders = hub.reader.rows(Table::Orders).unwrap();
                if kind == "settle" {
                    prop_assert_eq!(hub.change_count(), after_forward + 2);
                    let o = orders.iter().find(|o| text(o, "id") == target).unwrap();
                    prop_assert_eq!(text(o, "status"), "open".to_owned());
                    prop_assert!(
                        hub.reader
                            .rows(Table::Payments)
                            .unwrap()
                            .iter()
                            .all(|p| text(p, "order_id") != target)
                    );
                } else {
                    prop_assert_eq!(hub.change_count(), after_forward + 5);
                    prop_assert!(
                        orders
                            .iter()
                            .all(|o| o["session_id"] != RowValue::Text(target.clone()))
                    );
                    let s = hub
                        .reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .find(|s| text(s, "id") == target)
                        .unwrap();
                    prop_assert_eq!(text(&s, "state"), "active".to_owned());
                }
                prop_assert!(hub.change_count() > before);
            }
        }
        Op::Advance { ms } => hub.clock.advance(*ms),
    }
    assert_gap_free(hub)
}

fn int(row: &arena_store::Row, col: &str) -> i64 {
    match &row[col] {
        RowValue::Integer(n) => *n,
        other => panic!("{col}: {other:?}"),
    }
}

fn group_of(hub: &mut Hub, kind: &str) -> Result<String, TestCaseError> {
    let found = hub
        .reader
        .rows(Table::DeviceGroups)
        .unwrap()
        .iter()
        .find(|g| text(g, "kind") == kind && g["deleted_at"] == RowValue::Null)
        .map(|g| text(g, "group_id"));
    if let Some(id) = found {
        return Ok(id);
    }
    let out = hub.submit_new(hub.intent(
        IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
            id: None,
            name: format!("{kind} group"),
            group_kind: kind.into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
        None,
    ));
    prop_assert!(out.is_ok(), "{:?}", out);
    Ok(out.unwrap().changes[0].row_id.clone())
}

fn text(row: &arena_store::Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
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

fn assert_wallets_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let txns = hub.reader.rows(Table::WalletTxns).unwrap();
    for w in hub.reader.rows(Table::Wallets).unwrap() {
        let id = text(&w, "player_id");
        let sum: i64 = txns
            .iter()
            .filter(|t| text(t, "player_id") == id && text(t, "reason") != "credit")
            .map(|t| int(t, "delta_minor"))
            .sum();
        prop_assert!(int(&w, "balance_minor") >= 0);
        prop_assert_eq!(int(&w, "balance_minor"), sum);
    }
    Ok(())
}

fn assert_credit_holds(hub: &Hub) -> Result<(), TestCaseError> {
    let txns = hub.reader.rows(Table::WalletTxns).unwrap();
    for c in hub.reader.rows(Table::CreditBalances).unwrap() {
        let id = text(&c, "player_id");
        let sum: i64 = txns
            .iter()
            .filter(|t| text(t, "player_id") == id && text(t, "reason") == "credit")
            .map(|t| int(t, "delta_minor"))
            .sum();
        let owed = int(&c, "outstanding_minor");
        prop_assert!(owed >= 0);
        prop_assert_eq!(owed, sum);
        if owed > int(&c, "limit_minor") {
            prop_assert!(c["last_override_day"] != RowValue::Null);
        }
    }
    for o in hub.reader.rows(Table::Orders).unwrap() {
        if text(&o, "kind") != "credit_collect" {
            continue;
        }
        let summary = hub.reader.credit_collection(&text(&o, "id")).unwrap();
        prop_assert!(summary.is_some());
        let summary = summary.unwrap();
        let cleared: i64 = summary.cleared.iter().map(|c| c.cleared_minor).sum();
        prop_assert_eq!(cleared, int(&o, "total_minor"));
        prop_assert!(summary.outstanding_after_minor >= 0);
        prop_assert!(
            summary
                .cleared
                .iter()
                .all(|c| c.cleared_minor > 0 && c.cleared_minor <= c.spend_minor)
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
                assert_wallets_hold(&hub)?;
                assert_credit_holds(&hub)?;
            }
            assert_replay_reproduces(&hub)
        })
        .unwrap();
}

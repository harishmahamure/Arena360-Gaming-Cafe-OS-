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
    /// `coupon.upsert` of a percent or fixed coupon (valid or not) by a staff member of a built-in
    /// role; when it saves, a cash counter sale applies its code (or an unknown one), then maybe the
    /// sale's Undo. A coupon's `used` always equals the settled orders that took it.
    Coupon {
        percent: i64,
        fixed: bool,
        once: bool,
        unknown: bool,
        undo: bool,
        who: Index,
    },
    /// A cash counter sale with a manual discount (valid or not) by the owner or a staff member of
    /// a built-in role, then maybe the sale's Undo. Only the owner and managers may discount; the
    /// order takes exactly what the preview priced.
    Discount {
        percent: i64,
        fixed: bool,
        note: u8,
        undo: bool,
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
    /// `stock.waste` of a quantity (valid or not, sometimes without a reason) from the first
    /// tracked product, then maybe `stock.waste.reverted` straight after.
    Waste {
        qty: i64,
        reason: u8,
        undo: bool,
    },
    /// `stock.in` of a lot that expires `days` from the business day, into the first tracked
    /// product (relative to the Hub clock, so advancing it moves lots into and out of the window).
    ExpiringStock {
        days: i64,
        qty: i64,
    },
    /// `stock.expiry_check` (system), which must leave at most one open "Expiring soon" alert.
    ExpiryCheck,
    /// `stock.write_off` of one of the first tracked product's lots: applies only to an expired
    /// lot with stock left.
    WriteOff {
        pick: u8,
    },
    /// `shift.open` (a float in or out of range): before the branch exists it is refused, and
    /// afterwards the shift the bootstrap opened is still open, so it is refused too.
    ShiftOpen {
        float: i64,
    },
    /// `stock.threshold` (valid or not) on the first tracked product: the open "Low stock" alert
    /// exists exactly when the quantity is at or under the new threshold.
    Threshold {
        value: Option<i64>,
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
    /// A cash counter sale of one or two products (stock or tax may refuse it), then maybe its
    /// Undo (late or in time).
    PosSale {
        pick: u8,
        qty: u8,
        second: bool,
        undo: bool,
        late: bool,
        /// The receipt choice is Print: the sale is numbered in its own transaction, queues one
        /// print and can never be undone.
        print: bool,
        /// `receipt.drawer` is on for this sale: a cash sale queues one drawer kick and can never
        /// be undone. The setting goes back off straight after.
        drawer: bool,
        /// The receipt choice is WhatsApp for a player (the branch is made paid, with the message
        /// on): a player with current consent gets one queued `notify_outbox` row that an in-time
        /// Undo skips; any other player is refused `CONSENT_REQUIRED` and nothing is written.
        whatsapp: bool,
    },
    /// Products placed on a running session's bill (stock may refuse), then maybe the
    /// placement's Undo (late or in time).
    OrderPlace {
        slot: u8,
        pick: u8,
        qty: u8,
        undo: bool,
        late: bool,
    },
    /// An unserved placed line is marked served or cancelled (made or not), when one exists.
    OrderLine {
        pick: u8,
        cancel: bool,
        made: bool,
    },
    /// `order.void` of an open session order by the owner or a staff member of a built-in role,
    /// with a good, missing or wrong reason and note. Only the owner and managers may void; a
    /// voided order is detached from its session, keeps its lines and holds no stock.
    Void {
        pick: u8,
        reason: u8,
        note: u8,
        who: Index,
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
    /// The system numbers a settled order (maybe after the Undo window closed): it either issues
    /// the next number of the series or writes nothing.
    InvoiceIssue {
        pick: u8,
        window_closed: bool,
    },
    /// A staff transfer of a running or paused session to an existing device: it either moves
    /// the session, closing its segment and opening one on the target, or writes nothing.
    Transfer {
        slot: u8,
        target: u8,
    },
    /// A forced end of a running or paused session with a reason, which always applies, then maybe
    /// its undo, which either reopens the session or writes nothing.
    ForceEnd {
        slot: u8,
        undo: bool,
    },
    /// A deactivation that must be refused (self, by a non-Owner, an unknown member): nothing is
    /// written.
    DeactivateRefused {
        pick: u8,
    },
    /// A controller check of a session that may be running, on a PC, ended or already marked: it
    /// applies once for an ended console session and otherwise writes nothing.
    ControllerCheck {
        slot: u8,
        missing: bool,
    },
    /// Staff put a session and another into one group bill, or are refused and nothing is
    /// written; a payer is then named, or refused the same way.
    Group {
        slot: u8,
        other: u8,
        guest: u8,
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
        2 => (
            prop_oneof![5 => 1..=10_000i64, 1 => 0..=0i64, 1 => 10_001..=10_002i64],
            any::<bool>(),
            any::<bool>(),
            prop::bool::weighted(0.2),
            any::<bool>(),
            any::<Index>(),
        )
            .prop_map(|(percent, fixed, once, unknown, undo, who)| Op::Coupon {
                percent,
                fixed,
                once,
                unknown,
                undo,
                who,
            }),
        2 => (
            prop_oneof![5 => 1..=10_000i64, 1 => 0..=0i64, 1 => 10_001..=10_002i64],
            any::<bool>(),
            0..=2u8,
            any::<bool>(),
            any::<Index>(),
        )
            .prop_map(|(percent, fixed, note, undo, who)| Op::Discount {
                percent,
                fixed,
                note,
                undo,
                who,
            }),
        3 => (
            prop_oneof![3 => 1..=1_000i64, 1 => -2..=2i64, 1 => 999_999..=1_000_002i64],
            prop_oneof![3 => 0..=100_000i64, 1 => -2..=2i64, 1 => Just(arena_proto::MAX_PRICE_MINOR), 1 => Just(arena_proto::MAX_PRICE_MINOR + 1)],
            prop::bool::weighted(0.1),
            any::<bool>(),
        )
            .prop_map(|(qty, cost, past, undo)| Op::Stock { qty, cost, past, undo }),
        2 => (
            prop_oneof![4 => 1..=60i64, 1 => -2..=0i64, 1 => 1000..=2000i64],
            0..5u8,
            any::<bool>(),
        )
            .prop_map(|(qty, reason, undo)| Op::Waste { qty, reason, undo }),
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
        3 => (any::<u8>(), 1..=6u8, any::<bool>(), any::<bool>(), prop::bool::weighted(0.3), prop::bool::weighted(0.4), prop::bool::weighted(0.25), prop::bool::weighted(0.25)).prop_map(
            |(pick, qty, second, undo, late, print, drawer, whatsapp)| Op::PosSale { pick, qty, second, undo, late, print, drawer, whatsapp }
        ),
        3 => (0..3u8, any::<u8>(), 1..=6u8, any::<bool>(), prop::bool::weighted(0.3)).prop_map(
            |(slot, pick, qty, undo, late)| Op::OrderPlace { slot, pick, qty, undo, late }
        ),
        2 => (any::<u8>(), any::<bool>(), any::<bool>()).prop_map(
            |(pick, cancel, made)| Op::OrderLine { pick, cancel, made }
        ),
        2 => (any::<u8>(), 0..5u8, 0..3u8, any::<Index>()).prop_map(
            |(pick, reason, note, who)| Op::Void { pick, reason, note, who }
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
        2 => (0..8i64, 1..=20i64).prop_map(|(days, qty)| Op::ExpiringStock { days, qty }),
        1 => Just(Op::ExpiryCheck),
        2 => (0..6u8).prop_map(|pick| Op::WriteOff { pick }),
        1 => prop_oneof![4 => 0..=50_000i64, 1 => -2..=-1i64, 1 => Just(arena_proto::MAX_FLOAT_MINOR + 1)]
            .prop_map(|float| Op::ShiftOpen { float }),
        2 => prop_oneof![
            1 => Just(None),
            5 => (0..=60i64).prop_map(Some),
            1 => Just(Some(-1)),
            1 => Just(Some(arena_proto::MAX_STOCK_QTY + 1)),
        ]
        .prop_map(|value| Op::Threshold { value }),
        2 => (0..3u8).prop_map(|slot| Op::SessionExpire { slot }),
        2 => (0..3u8).prop_map(|slot| Op::RateFlip { slot }),
        3 => (any::<u8>(), prop::bool::weighted(0.8)).prop_map(|(pick, window_closed)| Op::InvoiceIssue {
            pick,
            window_closed
        }),
        2 => (0..3u8, 0..4u8).prop_map(|(slot, target)| Op::Transfer { slot, target }),
        2 => (0..3u8, 0..3u8, 0..4u8).prop_map(|(slot, other, guest)| Op::Group { slot, other, guest }),
        2 => (0..3u8, prop::bool::ANY).prop_map(|(slot, undo)| Op::ForceEnd { slot, undo }),
        2 => (0..4u8, prop::bool::ANY).prop_map(|(slot, missing)| Op::ControllerCheck { slot, missing }),
        1 => (0..4u8).prop_map(|pick| Op::DeactivateRefused { pick }),
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

/// Turns `receipt.drawer` on or off (the `PosSale` op wraps one sale in it).
fn set_drawer(hub: &mut Hub, on: bool) {
    let rev = hub
        .reader
        .rows(Table::Settings)
        .unwrap()
        .iter()
        .find(|s| text(s, "key") == arena_proto::KEY_RECEIPT_DRAWER)
        .map(|s| int(s, "rev"));
    if let Some(rev) = rev {
        let value = if on { "true" } else { "false" };
        let _ = hub.submit_new(hub.intent(
            set_payload(arena_proto::KEY_RECEIPT_DRAWER, value),
            Some(rev),
        ));
    }
}

/// Makes the branch paid (Basic) with the WhatsApp receipt switched on.
fn enable_whatsapp(hub: &mut Hub) {
    let limits = arena_proto::Tier::Basic.defaults().limits;
    let licence = arena_proto::LicenceApply {
        standing: arena_proto::LicenceStanding::Verified,
        clock_wrong: false,
        tier: arena_proto::Tier::Basic,
        flags: String::new(),
        device_cap: limits.devices,
        user_cap: limits.users,
    };
    let _ = hub.rt.block_on(hub.writer.apply_licence(licence));
    let rev = hub
        .reader
        .rows(Table::Settings)
        .unwrap()
        .iter()
        .find(|s| text(s, "key") == arena_proto::KEY_NOTIFY_WHATSAPP_RECEIPT)
        .filter(|s| text(s, "value") != "true")
        .map(|s| int(s, "rev"));
    if let Some(rev) = rev {
        let _ = hub.submit_new(hub.intent(
            set_payload(arena_proto::KEY_NOTIFY_WHATSAPP_RECEIPT, "true"),
            Some(rev),
        ));
    }
}

/// `(player_id, state)` of the notification rows of one order.
fn notify_rows(hub: &Hub, order_id: &str) -> Vec<(String, String)> {
    hub.reader
        .rows(Table::NotifyOutbox)
        .unwrap()
        .iter()
        .filter(|r| text(r, "order_id") == order_id)
        .map(|r| (text(r, "player_id"), text(r, "state")))
        .collect()
}

/// The notification outbox invariants: every row names an existing player, a receipt row is
/// transactional, and a row that is still queued belongs to a sale that stands.
fn assert_notifications_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let orders = hub.reader.rows(Table::Orders).unwrap();
    let players = hub.reader.rows(Table::Players).unwrap();
    for r in hub.reader.rows(Table::NotifyOutbox).unwrap() {
        prop_assert_eq!(text(&r, "purpose"), "transactional");
        prop_assert!(
            players
                .iter()
                .any(|p| text(p, "player_id") == text(&r, "player_id")),
            "a message names a missing player"
        );
        if text(&r, "state") == "queued" {
            let order = orders
                .iter()
                .find(|o| text(o, "id") == text(&r, "order_id"));
            prop_assert!(order.is_some(), "a queued message names a missing sale");
            prop_assert_eq!(text(order.unwrap(), "status"), "settled");
        }
    }
    Ok(())
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

/// How many "Expiring soon" alerts are open (the schema allows one).
fn open_expiring_alerts(hub: &Hub) -> usize {
    hub.reader
        .alerts()
        .unwrap()
        .iter()
        .filter(|a| a.kind == "stock_expiring")
        .count()
}

fn open_low_alerts(hub: &Hub, product_id: &str) -> Vec<arena_store::AlertRow> {
    hub.reader
        .alerts()
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "stock_low" && a.product_id.as_deref() == Some(product_id))
        .collect()
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
                // Cash flows need an open shift; the one drawer is opened with a float.
                let shift = hub.submit_new(hub.intent(
                    IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor: 5_000 }),
                    None,
                ));
                prop_assert!(shift.is_ok(), "{:?}", shift);
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
                    // The shift and its float event are the two rows after the bootstrap.
                    1 + DEFAULT_SETTINGS.len()
                        + LOCALE_KEYS.len()
                        + 3
                        + roles
                        + STAFF_ROLES.len()
                        + 2
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
                    kitchen: name.len() % 2 == 0,
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
        Op::Waste { qty, reason, undo } => {
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
            let lot_sum = |hub: &Hub| -> i64 {
                hub.reader
                    .lots(&id)
                    .unwrap()
                    .iter()
                    .map(|l| l.qty_remaining)
                    .sum()
            };
            let (old_qty, old_avg) = held(hub);
            let threshold = hub
                .reader
                .tracked_thresholds()
                .unwrap()
                .into_iter()
                .find(|r| r.product_id == id)
                .and_then(|r| r.low_stock_qty);
            let mut intent = hub.intent(
                IntentPayload::StockWaste(arena_proto::StockWaste {
                    product_id: id.clone(),
                    qty: *qty,
                }),
                None,
            );
            intent.reason = arena_proto::ReasonCode::WASTE
                .get(usize::from(*reason))
                .copied();
            let had_reason = intent.reason.is_some();
            let out = hub.submit_new(intent);
            if !had_reason || *qty < 1 || *qty > old_qty {
                let want = if !had_reason {
                    ErrorCode::ReasonRequired
                } else if *qty < 1 {
                    ErrorCode::ValidationFailed
                } else {
                    ErrorCode::OutOfStock
                };
                prop_assert_eq!(code(&out).map(|c| c.0), Some(want), "{:?}", out);
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert_eq!(held(hub), (old_qty - qty, old_avg));
            prop_assert_eq!(lot_sum(hub), old_qty - qty);
            prop_assert!(hub.change_count() >= changes_before + 3);
            let low = open_low_alerts(hub, &id);
            prop_assert!(low.len() <= 1);
            if threshold.is_some_and(|th| old_qty > th && old_qty - qty <= th) {
                prop_assert_eq!(low.len(), 1);
                prop_assert_eq!(low[0].qty, Some(old_qty - qty));
                prop_assert_eq!(low[0].threshold, threshold);
            }
            if *undo {
                let after_waste = hub.change_count();
                let move_id = out
                    .unwrap()
                    .changes
                    .iter()
                    .find(|c| c.tbl == "stock_moves")
                    .unwrap()
                    .row_id
                    .clone();
                let revert = hub.intent(
                    IntentPayload::StockWasteReverted(arena_proto::StockWasteReverted { move_id }),
                    None,
                );
                let out = hub.submit_new(revert);
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(held(hub), (old_qty, old_avg));
                prop_assert_eq!(lot_sum(hub), old_qty);
                prop_assert!(hub.change_count() >= after_waste + 3);
            }
        }
        Op::ExpiringStock { days, qty } => {
            let Some(today) =
                arena_core::locale::current_business_day(&hub.reader, hub.clock_now()).unwrap()
            else {
                return Ok(());
            };
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
            let old = hub.reader.stock(&id).unwrap().map_or(0, |s| s.qty);
            let expiry = arena_proto::add_days(&today, *days).unwrap();
            let intent = hub.intent(
                IntentPayload::StockIn(arena_proto::StockIn {
                    product_id: id.clone(),
                    qty: *qty,
                    unit_cost_minor: 100,
                    supplier: "s".into(),
                    expiry: Some(expiry),
                }),
                None,
            );
            let out = hub.submit_new(intent);
            if old + qty > arena_proto::MAX_STOCK_QTY {
                prop_assert_eq!(code(&out).map(|c| c.0), Some(ErrorCode::ValidationFailed));
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert_eq!(hub.reader.stock(&id).unwrap().unwrap().qty, old + qty);
            if *days <= 3 {
                prop_assert_eq!(open_expiring_alerts(hub), 1);
            }
        }
        Op::ExpiryCheck => {
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::StockExpiryCheck(arena_proto::StockExpiryCheck {}),
                None,
            );
            intent.actor.kind = ActorKind::System;
            let out = hub.submit_new(intent);
            match code(&out) {
                None => prop_assert!(hub.change_count() > before),
                Some(refused) => {
                    prop_assert_eq!(refused, (ErrorCode::Stale, None));
                    prop_assert_eq!(hub.change_count(), before);
                }
            }
            prop_assert!(open_expiring_alerts(hub) <= 1);
        }
        Op::WriteOff { pick } => {
            let Some(today) =
                arena_core::locale::current_business_day(&hub.reader, hub.clock_now()).unwrap()
            else {
                return Ok(());
            };
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
            let mut lots = hub.reader.lots(&id).unwrap();
            if lots.is_empty() {
                return Ok(());
            }
            lots.sort_by(|a, b| a.lot_id.cmp(&b.lot_id));
            let lot = lots[usize::from(*pick) % lots.len()].clone();
            let old = hub.reader.stock(&id).unwrap().unwrap().qty;
            let intent = hub.intent(
                IntentPayload::StockWriteOff(arena_proto::StockWriteOff {
                    lot_id: lot.lot_id.clone(),
                }),
                None,
            );
            let out = hub.submit_new(intent);
            let expired = lot
                .expiry
                .as_deref()
                .is_some_and(|e| arena_proto::lot_expired(e, &today));
            if lot.qty_remaining == 0 || !expired {
                let want = if lot.qty_remaining == 0 {
                    ErrorCode::Stale
                } else {
                    ErrorCode::ValidationFailed
                };
                prop_assert_eq!(code(&out).map(|c| c.0), Some(want), "{:?}", out);
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            let left = hub.reader.stock(&id).unwrap().unwrap().qty;
            prop_assert_eq!(left, old - lot.qty_remaining);
            let after = hub.reader.lots(&id).unwrap();
            let emptied = after.iter().find(|l| l.lot_id == lot.lot_id).unwrap();
            prop_assert_eq!(emptied.qty_remaining, 0);
            prop_assert_eq!(after.iter().map(|l| l.qty_remaining).sum::<i64>(), left);
        }
        Op::PosSale {
            pick,
            qty,
            second,
            undo,
            late,
            print,
            drawer,
            whatsapp,
        } => {
            let products: Vec<_> = hub
                .reader
                .products()
                .unwrap()
                .into_iter()
                .filter(|p| p.active)
                .collect();
            let tax = hub
                .reader
                .settings()
                .unwrap()
                .into_iter()
                .find(|s| s.key == "branch.tax")
                .and_then(|s| arena_proto::TaxSettings::parse(&s.value));
            let (false, Some(tax)) = (products.is_empty(), tax) else {
                return Ok(());
            };
            let mut lines = vec![(
                products[usize::from(*pick) % products.len()].clone(),
                i64::from(*qty),
            )];
            if *second && products.len() > 1 {
                lines.push((
                    products[(usize::from(*pick) + 1) % products.len()].clone(),
                    1,
                ));
            }
            let gst = tax.tax_mode() == arena_proto::TaxMode::GstIn;
            let taxed: Vec<_> = lines
                .iter()
                .map(|(p, q)| arena_core::invoice::tax::Line {
                    rate_bp: if gst {
                        u16::try_from(p.tax_rate_bp).unwrap_or(0)
                    } else {
                        tax.rate_bp
                    },
                    amount_minor: p.price_minor * q,
                })
                .collect();
            let Some(bill) = arena_core::invoice::tax::compute(&taxed, tax.inclusive, gst) else {
                return Ok(());
            };
            let stock_before = stock_image(hub);
            if *drawer {
                set_drawer(hub, true);
            }
            // WhatsApp needs a profiled player; a printed sale keeps its own choice.
            let players = hub.reader.rows(Table::Players).unwrap();
            let wa_player = (*whatsapp && !*print && !players.is_empty())
                .then(|| text(&players[usize::from(*pick) % players.len()], "player_id"));
            let consent = wa_player.as_ref().is_some_and(|id| {
                hub.reader
                    .latest_consent(id, arena_proto::CONSENT_TRANSACTIONAL)
                    .unwrap()
                    .is_some_and(|c| c.0)
            });
            if wa_player.is_some() {
                enable_whatsapp(hub);
            }
            let before = hub.change_count();
            let out = hub.submit_new(
                hub.intent(
                    IntentPayload::PosSale(arena_proto::PosSale {
                        business: None,
                        coupon: None,
                        discount: None,
                        lines: lines
                            .iter()
                            .map(|(p, q)| arena_proto::PosSaleLine {
                                product_id: p.product_id.clone(),
                                qty: *q,
                            })
                            .collect(),
                        payments: if bill.total_minor > 0 {
                            vec![arena_proto::PaymentRow {
                                method: arena_proto::PaymentMethod::Cash,
                                amount_minor: bill.total_minor,
                                reference: None,
                            }]
                        } else {
                            Vec::new()
                        },
                        player_id: wa_player.clone(),
                        credit_override: None,
                        stock_override: None,
                        receipt: if wa_player.is_some() {
                            Some(arena_proto::ReceiptChoice::Whatsapp)
                        } else {
                            print.then_some(arena_proto::ReceiptChoice::Print)
                        },
                    }),
                    None,
                ),
            );
            let after = hub.change_count();
            if *drawer {
                set_drawer(hub, false);
            }
            if let Err(e) = &out {
                // A refusal (short stock, a bundle part switched off, no shift) writes nothing.
                prop_assert!(
                    matches!(
                        e.code,
                        ErrorCode::OutOfStock
                            | ErrorCode::BundleShort
                            | ErrorCode::ValidationFailed
                            | ErrorCode::ShiftNotOpen
                            | ErrorCode::ConsentRequired
                    ),
                    "{:?}",
                    out
                );
                if e.code == ErrorCode::ConsentRequired {
                    prop_assert!(wa_player.is_some() && !consent);
                }
                prop_assert_eq!(stock_image(hub), stock_before);
                prop_assert_eq!(after, before);
                return Ok(());
            }
            let order = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .rfind(|o| text(o, "kind") == "pos_sale")
                .unwrap();
            prop_assert!(
                wa_player.is_none() || consent,
                "a WhatsApp receipt for a player without consent was accepted"
            );
            let messages = notify_rows(hub, &text(&order, "id"));
            if let Some(player_id) = &wa_player {
                prop_assert_eq!(
                    messages.len(),
                    usize::from(bill.total_minor > 0),
                    "a WhatsApp settle queues exactly one row, and none for a bill with no total"
                );
                for (player, state) in &messages {
                    prop_assert_eq!(player, player_id);
                    prop_assert_eq!(state, "queued");
                }
            } else {
                prop_assert!(messages.is_empty(), "no WhatsApp choice, no message");
            }
            let kicked = hub
                .reader
                .queued_drawers(10_000)
                .unwrap()
                .iter()
                .filter(|r| r.ref_id == text(&order, "id"))
                .count();
            prop_assert_eq!(
                kicked,
                usize::from(*drawer && bill.total_minor > 0),
                "a cash sale with the drawer on queues exactly one kick"
            );
            prop_assert_eq!(text(&order, "status"), "settled");
            prop_assert_eq!(int(&order, "total_minor"), bill.total_minor);
            prop_assert_eq!(int(&order, "tax_minor"), bill.tax_minor);
            prop_assert_eq!(
                matches!(order["invoice_no"], RowValue::Text(_)),
                *print && bill.total_minor > 0,
                "only a printed sale is numbered at settle"
            );
            if *undo && (*print || (*drawer && bill.total_minor > 0)) {
                let after_forward = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                prop_assert_eq!(hub.change_count(), after_forward);
            } else if *undo {
                if *late {
                    hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
                }
                let after_forward = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                if *late {
                    prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                    prop_assert_eq!(hub.change_count(), after_forward);
                } else {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(stock_image(hub), stock_before);
                    for (_, state) in notify_rows(hub, &text(&order, "id")) {
                        prop_assert_eq!(state, "skipped", "an undone sale is never messaged");
                    }
                }
            }
        }
        Op::Coupon {
            percent,
            fixed,
            once,
            unknown,
            undo,
            who,
        } => {
            if !hub.bootstrapped {
                return Ok(());
            }
            let coupon_code = format!("CPN{}", hub.change_count());
            let upsert = arena_proto::CouponUpsert {
                id: None,
                code: coupon_code.to_lowercase(),
                percent_bp: (!fixed).then_some(*percent),
                amount_minor: fixed.then_some(percent * 5),
                valid_from: None,
                valid_to: None,
                usage_limit: once.then_some(1),
                min_bill_minor: 0,
                targets: Vec::new(),
                active: true,
            };
            let mut intent = hub.intent(IntentPayload::CouponUpsert(upsert), None);
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
            let valid = if *fixed {
                *percent >= 1
            } else {
                (1..=arena_proto::COUPON_MAX_PERCENT_BP).contains(percent)
            };
            let before = hub.change_count();
            let out = hub.submit_new(intent);
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            if !valid {
                prop_assert_eq!(
                    code(&out).map(|c| c.0),
                    Some(ErrorCode::ValidationFailed),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert_eq!(hub.change_count(), before + 1);
            let upper = coupon_code.to_uppercase();
            let untracked = |hub: &Hub| {
                hub.reader
                    .products()
                    .unwrap()
                    .into_iter()
                    .find(|p| p.active && !p.track_stock && !p.bundle && p.price_minor > 0)
            };
            if untracked(hub).is_none() {
                let made = hub.submit_new(hub.intent(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: format!("Coupon item {}", hub.change_count()),
                        category: String::new(),
                        price_minor: 5_000,
                        tax_rate_bp: Some(500),
                        hsn: "210690".into(),
                        track_stock: false,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: false,
                        components: Vec::new(),
                    }),
                    None,
                ));
                if made.is_err() {
                    return Ok(());
                }
            }
            let Some(product) = untracked(hub) else {
                return Ok(());
            };
            let lines = vec![arena_proto::PosSaleLine {
                product_id: product.product_id.clone(),
                qty: 1,
            }];
            let applied = if *unknown { "NOSUCH" } else { upper.as_str() };
            let request = arena_proto::CouponPreviewRequest {
                code: applied.to_owned(),
                order_id: None,
                lines: Some(lines.clone()),
                plan_id: None,
            };
            let now = hub.clock.now_ms();
            let preview = hub
                .reader
                .read_with(|conn| arena_core::coupon_preview(conn, &request, now));
            let sale = |payments: Vec<arena_proto::PaymentRow>| {
                IntentPayload::PosSale(arena_proto::PosSale {
                    business: None,
                    coupon: Some(applied.to_owned()),
                    discount: None,
                    lines: lines.clone(),
                    payments,
                    player_id: None,
                    credit_override: None,
                    stock_override: None,
                    receipt: None,
                })
            };
            let used_of = |hub: &Hub| {
                hub.reader
                    .coupons()
                    .unwrap()
                    .into_iter()
                    .find(|(c, _)| c.code == upper)
                    .map_or(0, |(c, _)| c.used)
            };
            let used_before = used_of(hub);
            let preview = match preview {
                Ok(p) => p,
                Err(e) if e.code == ErrorCode::CouponInvalid => {
                    let before = hub.change_count();
                    let out = hub.submit_new(hub.intent(sale(Vec::new()), None));
                    prop_assert_eq!(code(&out), Some((ErrorCode::CouponInvalid, Some("coupon"))));
                    prop_assert_eq!(hub.change_count(), before);
                    return Ok(());
                }
                Err(_) => return Ok(()),
            };
            prop_assert!(!*unknown, "an unknown code priced a bill");
            let payments = if preview.total_minor > 0 {
                vec![arena_proto::PaymentRow {
                    method: arena_proto::PaymentMethod::Cash,
                    amount_minor: preview.total_minor,
                    reference: None,
                }]
            } else {
                Vec::new()
            };
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(sale(payments), None));
            if let Err(e) = &out {
                prop_assert!(
                    matches!(
                        e.code,
                        ErrorCode::OutOfStock
                            | ErrorCode::BundleShort
                            | ErrorCode::ValidationFailed
                            | ErrorCode::ShiftNotOpen
                    ),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
                prop_assert_eq!(used_of(hub), used_before);
                return Ok(());
            }
            let order = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .rfind(|o| text(o, "kind") == "pos_sale")
                .unwrap();
            prop_assert_eq!(text(&order, "coupon_code"), upper.clone());
            prop_assert_eq!(int(&order, "coupon_minor"), preview.discount_minor);
            prop_assert_eq!(int(&order, "total_minor"), preview.total_minor);
            prop_assert_eq!(used_of(hub), used_before + 1);
            if *undo {
                let out = hub.submit_new(hub.intent(
                    IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                prop_assert!(out.is_ok(), "{:?}", out);
                prop_assert_eq!(used_of(hub), used_before);
            }
        }
        Op::Discount {
            percent,
            fixed,
            note,
            undo,
            who,
        } => {
            if !hub.bootstrapped {
                return Ok(());
            }
            let find = |hub: &Hub| {
                hub.reader
                    .products()
                    .unwrap()
                    .into_iter()
                    .find(|p| p.active && !p.track_stock && !p.bundle && p.price_minor > 0)
            };
            if find(hub).is_none() {
                let made = hub.submit_new(hub.intent(
                    IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                        id: None,
                        name: format!("Discount item {}", hub.change_count()),
                        category: String::new(),
                        price_minor: 5_000,
                        tax_rate_bp: Some(500),
                        hsn: "210690".into(),
                        track_stock: false,
                        image: String::new(),
                        barcode: String::new(),
                        kitchen: false,
                        favourite: false,
                        active: true,
                        bundle: false,
                        components: Vec::new(),
                    }),
                    None,
                ));
                if made.is_err() {
                    return Ok(());
                }
            }
            let Some(product) = find(hub) else {
                return Ok(());
            };
            let lines = vec![arena_proto::PosSaleLine {
                product_id: product.product_id.clone(),
                qty: 1,
            }];
            let discount = arena_proto::ManualDiscount {
                percent_bp: (!fixed).then_some(*percent),
                amount_minor: fixed.then_some(percent * 5),
                reason: if *note == 0 {
                    arena_proto::DiscountReason::StaffMeal
                } else {
                    arena_proto::DiscountReason::Other
                },
                note: (*note == 1).then(|| "regular, bulk order".to_owned()),
            };
            let request = arena_proto::DiscountPreviewRequest {
                discount: discount.clone(),
                coupon: None,
                order_id: None,
                lines: Some(lines.clone()),
                plan_id: None,
            };
            let now = hub.clock.now_ms();
            let preview = hub
                .reader
                .read_with(|conn| arena_core::discount_preview(conn, &request, now));
            let payments = match &preview {
                Ok(p) if p.total_minor > 0 => vec![arena_proto::PaymentRow {
                    method: arena_proto::PaymentMethod::Cash,
                    amount_minor: p.total_minor,
                    reference: None,
                }],
                _ => Vec::new(),
            };
            let mut intent = hub.intent(
                IntentPayload::PosSale(arena_proto::PosSale {
                    business: None,
                    coupon: None,
                    discount: Some(discount.clone()),
                    lines: lines.clone(),
                    payments,
                    player_id: None,
                    credit_override: None,
                    stock_override: None,
                    receipt: None,
                }),
                None,
            );
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
            let actor_id = intent.actor.id.clone();
            let before = hub.change_count();
            let out = hub.submit_new(intent);
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            let Ok(preview) = preview else {
                prop_assert_eq!(
                    code(&out).map(|c| c.0),
                    Some(ErrorCode::ValidationFailed),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            };
            if let Err(e) = &out {
                prop_assert!(
                    matches!(
                        e.code,
                        ErrorCode::ShiftNotOpen | ErrorCode::ValidationFailed
                    ),
                    "{:?}",
                    out
                );
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            let order = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .rfind(|o| text(o, "kind") == "pos_sale")
                .unwrap();
            prop_assert_eq!(int(&order, "discount_minor"), preview.discount_minor);
            prop_assert_eq!(int(&order, "total_minor"), preview.total_minor);
            prop_assert_eq!(text(&order, "discount_by"), actor_id);
            if *undo {
                let out = hub.submit_new(hub.intent(
                    IntentPayload::PosSaleReverted(arena_proto::PosSaleReverted {
                        order_id: text(&order, "id"),
                    }),
                    Some(int(&order, "rev")),
                ));
                prop_assert!(out.is_ok(), "{:?}", out);
            }
        }
        Op::OrderPlace {
            slot,
            pick,
            qty,
            undo,
            late,
        } => {
            let live: Vec<_> = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .filter(|s| ["active", "paused", "grace"].contains(&text(s, "state").as_str()))
                .collect();
            let products: Vec<_> = hub
                .reader
                .products()
                .unwrap()
                .into_iter()
                .filter(|p| p.active)
                .collect();
            let (Some(session), false) = (
                live.get(usize::from(*slot) % live.len().max(1)),
                products.is_empty(),
            ) else {
                return Ok(());
            };
            let product = &products[usize::from(*pick) % products.len()];
            let supported = session["group_bill_id"] == RowValue::Null
                && ["postpaid", "block"].contains(&text(session, "mode").as_str());
            let stock_before = stock_image(hub);
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::OrderPlace(arena_proto::OrderPlace {
                    session_id: text(session, "id"),
                    lines: vec![arena_proto::PosSaleLine {
                        product_id: product.product_id.clone(),
                        qty: i64::from(*qty),
                    }],
                    stock_override: None,
                }),
                None,
            ));
            let applied = match out {
                Ok(applied) => applied,
                Err(e) => {
                    // A refusal (unsupported or settled bill, short stock, a bundle part switched
                    // off) writes nothing.
                    prop_assert!(
                        matches!(
                            e.code,
                            ErrorCode::OutOfStock
                                | ErrorCode::BundleShort
                                | ErrorCode::ValidationFailed
                        ),
                        "{:?}",
                        e
                    );
                    prop_assert_eq!(hub.change_count(), before);
                    prop_assert_eq!(stock_image(hub), stock_before);
                    return Ok(());
                }
            };
            prop_assert!(supported, "an unsupported session took an order");
            let order = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .find(|o| o["session_id"] == session["id"])
                .unwrap();
            prop_assert_eq!(text(&order, "status"), "open");
            if *undo {
                if *late {
                    hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
                }
                let after_forward = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::OrderPlaceReverted(arena_proto::OrderPlaceReverted {
                        order_id: text(&order, "id"),
                        placement_id: applied.intent_id.clone(),
                    }),
                    Some(int(&order, "rev")),
                ));
                if *late {
                    prop_assert_eq!(code(&out), Some((ErrorCode::UndoNotAllowed, None)));
                    prop_assert_eq!(hub.change_count(), after_forward);
                } else {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert_eq!(stock_image(hub), stock_before);
                }
            }
        }
        Op::OrderLine { pick, cancel, made } => {
            let lines: Vec<_> = hub
                .reader
                .rows(Table::OrderLines)
                .unwrap()
                .into_iter()
                .filter(|l| int(l, "served") == 0)
                .collect();
            let Some(l) = lines.get(usize::from(*pick) % lines.len().max(1)) else {
                return Ok(());
            };
            let stock_before = stock_image(hub);
            let open_before = open_hold_qty(hub);
            let (payload, reason) = if *cancel {
                (
                    IntentPayload::OrderLineCancel(arena_proto::OrderLineCancel {
                        line_id: text(l, "id"),
                        made: *made,
                    }),
                    Some(arena_proto::ReasonCode::Mistake),
                )
            } else {
                (
                    IntentPayload::OrderLineServed(arena_proto::OrderLineServed {
                        line_id: text(l, "id"),
                    }),
                    None,
                )
            };
            let mut intent = hub.intent(payload, None);
            intent.reason = reason;
            let out = hub.submit_new(intent);
            match out {
                Ok(_) if *cancel => {
                    prop_assert!(
                        hub.reader
                            .rows(Table::OrderLines)
                            .unwrap()
                            .iter()
                            .all(|x| x["id"] != l["id"])
                    );
                    prop_assert!(open_hold_qty(hub) <= open_before);
                }
                Ok(_) => {
                    let now = hub
                        .reader
                        .rows(Table::OrderLines)
                        .unwrap()
                        .into_iter()
                        .find(|x| x["id"] == l["id"])
                        .unwrap();
                    prop_assert_eq!(int(&now, "served"), 1);
                }
                Err(e) => {
                    // Only short stock refuses a serve; nothing is written.
                    prop_assert_eq!(e.code, ErrorCode::OutOfStock, "{:?}", e);
                    prop_assert!(!*cancel);
                    prop_assert_eq!(stock_image(hub), stock_before);
                }
            }
        }
        Op::Void {
            pick,
            reason,
            note,
            who,
        } => {
            let orders: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "open" && text(o, "kind") == "session")
                .filter(|o| o["session_id"] != RowValue::Null)
                .collect();
            let Some(o) = orders.get(usize::from(*pick) % orders.len().max(1)) else {
                return Ok(());
            };
            let id = text(o, "id");
            let (reason_code, note_text) = (
                match reason {
                    0 => Some(arena_proto::ReasonCode::Mistake),
                    1 => Some(arena_proto::ReasonCode::ChangedMind),
                    2 => Some(arena_proto::ReasonCode::Other),
                    3 => None,
                    _ => Some(arena_proto::ReasonCode::Lost),
                },
                match note {
                    0 => None,
                    1 => Some("Wrong table".to_owned()),
                    _ => Some("x".repeat(141)),
                },
            );
            let bad_reason = matches!(*reason, 3 | 4) || (*reason == 2 && *note != 1);
            let mut intent = hub.intent(
                IntentPayload::OrderVoid(arena_proto::OrderVoid {
                    order_id: id.clone(),
                    note: note_text,
                }),
                None,
            );
            intent.reason = reason_code;
            let pick = who.index(STAFF_ROLES.len() + 1);
            if pick > 0 || hub.staff.is_empty() {
                intent.actor = match hub.staff.get(pick).filter(|_| pick < STAFF_ROLES.len()) {
                    Some(staff) => Actor {
                        kind: ActorKind::Staff,
                        id: staff.clone(),
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
            if !allowed {
                prop_assert_eq!(code(&out), Some((ErrorCode::PermissionDenied, None)));
                prop_assert_eq!(hub.change_count(), before);
                return Ok(());
            }
            if bad_reason {
                prop_assert!(out.is_err(), "{:?}", out);
            }
            let now = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .find(|x| text(x, "id") == id)
                .unwrap();
            match out {
                Ok(_) => {
                    prop_assert_eq!(text(&now, "status"), "void");
                    prop_assert_eq!(&now["session_id"], &RowValue::Null);
                    prop_assert!(matches!(now["void_reason"], RowValue::Text(_)));
                    // A voided order holds no stock.
                    let lines: Vec<String> = hub
                        .reader
                        .rows(Table::OrderLines)
                        .unwrap()
                        .iter()
                        .filter(|l| text(l, "order_id") == id)
                        .map(|l| text(l, "id"))
                        .collect();
                    prop_assert!(
                        hub.reader
                            .rows(Table::StockHolds)
                            .unwrap()
                            .iter()
                            .all(|h| h["released_at"] != RowValue::Null
                                || !lines.contains(&text(h, "order_line_id")))
                    );
                }
                Err(e) => {
                    prop_assert!(
                        matches!(
                            e.code,
                            ErrorCode::ReasonRequired | ErrorCode::ValidationFailed
                        ),
                        "{:?}",
                        e
                    );
                    prop_assert_eq!(hub.change_count(), before);
                    prop_assert_eq!(text(&now, "status"), "open");
                }
            }
        }
        Op::ShiftOpen { float } => {
            let out = hub.submit_new(hub.intent(
                IntentPayload::ShiftOpen(arena_proto::ShiftOpen {
                    float_minor: *float,
                }),
                None,
            ));
            let (kind, field) = code(&out).unzip();
            prop_assert!(out.is_err(), "{:?}", out);
            if !(0..=arena_proto::MAX_FLOAT_MINOR).contains(float) {
                prop_assert_eq!(kind, Some(ErrorCode::ValidationFailed));
                prop_assert_eq!(field.flatten(), Some("float_minor"));
            } else if hub.bootstrapped {
                prop_assert_eq!(kind, Some(ErrorCode::ShiftAlreadyOpen));
            } else {
                prop_assert_eq!(kind, Some(ErrorCode::ValidationFailed));
                prop_assert_eq!(field.flatten(), Some("branch"));
            }
            prop_assert_eq!(hub.change_count(), changes_before);
            let shifts = hub.reader.rows(Table::Shifts).unwrap();
            prop_assert_eq!(shifts.len(), usize::from(hub.bootstrapped));
            // A cash event never falls outside a shift, and the float is the first of them.
            let events = hub.reader.rows(Table::CashEvents).unwrap();
            let inside = events
                .iter()
                .all(|e| shifts.iter().any(|s| s["id"] == e["shift_id"]));
            prop_assert!(inside);
        }
        Op::Threshold { value } => {
            let Some(row) = hub.reader.tracked_thresholds().unwrap().into_iter().next() else {
                return Ok(());
            };
            let id = row.product_id.clone();
            let intent = hub.intent(
                IntentPayload::StockThreshold(arena_proto::StockThreshold {
                    product_id: id.clone(),
                    low_stock_qty: *value,
                }),
                None,
            );
            let out = hub.submit_new(intent);
            if value.is_some_and(|v| !(0..=arena_proto::MAX_STOCK_QTY).contains(&v)) {
                prop_assert_eq!(
                    code(&out),
                    Some((ErrorCode::ValidationFailed, Some("low_stock_qty")))
                );
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            if *value == row.low_stock_qty {
                prop_assert_eq!(code(&out).map(|c| c.0), Some(ErrorCode::Stale));
                prop_assert_eq!(hub.change_count(), changes_before);
                return assert_gap_free(hub);
            }
            prop_assert!(out.is_ok(), "{:?}", out);
            let stored = hub
                .reader
                .tracked_thresholds()
                .unwrap()
                .into_iter()
                .find(|r| r.product_id == id)
                .unwrap();
            prop_assert_eq!(stored.low_stock_qty, *value);
            let low = open_low_alerts(hub, &id);
            let want = usize::from(value.is_some_and(|v| row.qty <= v));
            prop_assert_eq!(low.len(), want);
            if let (Some(alert), Some(v)) = (low.first(), value) {
                prop_assert_eq!(alert.qty, Some(row.qty));
                prop_assert_eq!(alert.threshold, Some(*v));
            }
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
                    group_with: None,
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
                    group_with: None,
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
                    group_with: None,
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
            // A grouped session also touches its group bill, so only an ungrouped postpaid
            // end has an exact change count.
            let grouped = session["group_bill_id"] != RowValue::Null;
            if prepaid || grouped {
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
            // A running postpaid session's bill settles once the session ends.
            let running: Vec<String> = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .filter(|s| {
                    ["active", "paused", "grace"].contains(&text(s, "state").as_str())
                        && text(s, "mode") != "block"
                })
                .map(|s| text(&s, "id"))
                .collect();
            let open: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "open")
                .filter(|o| !matches!(&o["session_id"], RowValue::Text(s) if running.contains(s)))
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
            // Each cash row also writes a `sale` cash event.
            let cash = payments
                .iter()
                .filter(|p| p.method == arena_proto::PaymentMethod::Cash)
                .count();
            let payload = IntentPayload::SessionSettle(arena_proto::SessionSettle {
                business: None,
                coupon: None,
                discount: None,
                order_id: id.clone(),
                credit_override: None,
                payments,
                receipt: None,
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
                prop_assert_eq!(hub.change_count(), before + rows + 1 + cash);
                let again = hub.submit_new(hub.intent(payload, Some(rev + 1)));
                prop_assert_eq!(code(&again), Some((ErrorCode::Stale, Some("order_id"))));
                prop_assert_eq!(hub.change_count(), before + rows + 1 + cash);
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
            prop_assert_eq!(hub.change_count(), before + 5);
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
                    prop_assert_eq!(hub.change_count(), after_forward + 5);
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
            // A price of one has nothing to fall short of.
            let short = &(*short && chosen.price_minor > 1);
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
                    business: None,
                    coupon: None,
                    discount: None,
                    player_id: player_id.clone(),
                    plan_id: chosen.plan_id.clone(),
                    credit_override: None,
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Cash,
                        amount_minor,
                        reference: None,
                    }],
                    receipt: None,
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
            prop_assert_eq!(hub.change_count(), before + 5);
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
                    prop_assert_eq!(hub.change_count(), after_forward + 5);
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
                    business: None,
                    coupon: None,
                    discount: None,
                    order_id: id,
                    credit_override: None,
                    payments,
                    receipt: None,
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
                    prop_assert_eq!(hub.change_count(), before + 5);
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
                            prop_assert_eq!(hub.change_count(), after_forward + 5);
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
        Op::InvoiceIssue {
            pick,
            window_closed,
        } => {
            let orders: Vec<_> = hub
                .reader
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .filter(|o| text(o, "status") == "settled")
                .collect();
            if orders.is_empty() {
                return Ok(());
            }
            let order = &orders[usize::from(*pick) % orders.len()];
            if *window_closed {
                hub.clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
            }
            let id = text(order, "id");
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::InvoiceIssue(arena_proto::InvoiceIssue {
                    order_id: id.clone(),
                }),
                None,
            );
            intent.actor.kind = ActorKind::System;
            let out = hub.submit_new(intent);
            match code(&out) {
                Some((ErrorCode::Stale | ErrorCode::ValidationFailed, _)) => {
                    prop_assert_eq!(hub.change_count(), before);
                }
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
                None => {
                    let after = hub
                        .reader
                        .rows(Table::Orders)
                        .unwrap()
                        .into_iter()
                        .find(|o| text(o, "id") == id)
                        .unwrap();
                    prop_assert!(matches!(after["invoice_no"], RowValue::Text(_)));
                    prop_assert!(int(&after, "total_minor") > 0);
                }
            }
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
        Op::Transfer { slot, target } => {
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
            let Some(device) = hub
                .reader
                .rows(Table::DevicesMeta)
                .unwrap()
                .get(usize::from(*target))
                .map(|d| text(d, "device_id"))
            else {
                return Ok(());
            };
            let (id, rev) = (text(&session, "id"), int(&session, "rev"));
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionTransfer(arena_proto::SessionTransfer {
                    session_id: id.clone(),
                    device_id: device.clone(),
                }),
                Some(rev),
            ));
            match code(&out) {
                None => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    prop_assert!(hub.change_count() > before);
                    let after = hub
                        .reader
                        .rows(Table::Sessions)
                        .unwrap()
                        .into_iter()
                        .find(|s| text(s, "id") == id)
                        .unwrap();
                    prop_assert_eq!(text(&after, "device_id"), device.clone());
                    prop_assert_eq!(text(&after, "state"), text(&session, "state"));
                    let open: Vec<_> = hub
                        .reader
                        .rows(Table::SessionSegments)
                        .unwrap()
                        .into_iter()
                        .filter(|g| text(g, "session_id") == id && g["to_at"] == RowValue::Null)
                        .collect();
                    prop_assert_eq!(open.len(), 1);
                    prop_assert_eq!(text(&open[0], "device_id"), device);
                }
                Some((
                    ErrorCode::ValidationFailed
                    | ErrorCode::NotFound
                    | ErrorCode::DeviceUnassigned
                    | ErrorCode::GroupKindMismatch
                    | ErrorCode::PcUnavailable
                    | ErrorCode::PcBusy
                    | ErrorCode::NoRate
                    | ErrorCode::PlanNotForGroup,
                    _,
                )) => prop_assert_eq!(hub.change_count(), before),
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
            }
        }
        Op::ForceEnd { slot, undo } => {
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
                IntentPayload::SessionForceEnd(arena_proto::SessionForceEnd {
                    session_id: id.clone(),
                    note: None,
                }),
                Some(int(&session, "rev")),
            );
            intent.reason = Some(arena_proto::ReasonCode::Closing);
            let out = hub.submit_new(intent);
            prop_assert!(out.is_ok(), "{:?}", out);
            prop_assert!(hub.change_count() > before);
            let find = |hub: &Hub| {
                hub.reader
                    .rows(Table::Sessions)
                    .unwrap()
                    .into_iter()
                    .find(|s| text(s, "id") == id)
                    .unwrap()
            };
            let ended = find(hub);
            prop_assert_eq!(text(&ended, "state"), "ended");
            prop_assert_eq!(text(&ended, "end_reason"), "forced");
            if *undo {
                let before = hub.change_count();
                let out = hub.submit_new(hub.intent(
                    IntentPayload::SessionForceEndReverted(arena_proto::SessionForceEndReverted {
                        session_id: id.clone(),
                    }),
                    Some(int(&ended, "rev")),
                ));
                match code(&out) {
                    None => {
                        prop_assert!(out.is_ok(), "{:?}", out);
                        let back = find(hub);
                        prop_assert_eq!(text(&back, "state"), "active");
                        prop_assert!(back["end_reason"] == RowValue::Null);
                    }
                    Some((ErrorCode::UndoNotAllowed | ErrorCode::Stale, _)) => {
                        prop_assert_eq!(hub.change_count(), before)
                    }
                    Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
                }
            }
        }
        Op::DeactivateRefused { pick } => {
            let [owner, manager, counter] = hub.staff.as_slice() else {
                return Ok(());
            };
            let (by, target, want) = match pick {
                0 => (owner, owner, ErrorCode::ValidationFailed),
                1 => (manager, counter, ErrorCode::PermissionDenied),
                2 => (counter, manager, ErrorCode::PermissionDenied),
                _ => (
                    owner,
                    &"no-such-member".to_owned(),
                    ErrorCode::ValidationFailed,
                ),
            };
            let before = hub.change_count();
            let mut intent = hub.intent(
                IntentPayload::StaffDeactivate(arena_proto::StaffDeactivate {
                    staff_id: target.clone(),
                }),
                None,
            );
            intent.actor = Actor {
                kind: ActorKind::Staff,
                id: by.clone(),
            };
            let out = hub.submit_new(intent);
            prop_assert_eq!(code(&out).map(|c| c.0), Some(want), "{:?}", out);
            prop_assert_eq!(hub.change_count(), before);
        }
        Op::ControllerCheck { slot, missing } => {
            let Some(session) = hub
                .reader
                .rows(Table::Sessions)
                .unwrap()
                .into_iter()
                .nth(usize::from(*slot))
            else {
                return Ok(());
            };
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::ControllerCheck(arena_proto::ControllerCheck {
                    session_id: text(&session, "id"),
                    result: if *missing {
                        arena_proto::ControllerResult::Missing
                    } else {
                        arena_proto::ControllerResult::Returned
                    },
                }),
                None,
            ));
            match code(&out) {
                None => prop_assert!(hub.change_count() > before),
                Some((ErrorCode::ValidationFailed | ErrorCode::Stale, _)) => {
                    prop_assert_eq!(hub.change_count(), before)
                }
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
            }
        }
        Op::Group { slot, other, guest } => {
            let sessions = hub.reader.rows(Table::Sessions).unwrap();
            let live: Vec<_> = sessions
                .iter()
                .filter(|s| matches!(text(s, "state").as_str(), "active" | "paused"))
                .collect();
            let (Some(host), Some(member)) =
                (live.get(usize::from(*slot)), live.get(usize::from(*other)))
            else {
                return Ok(());
            };
            let (host_id, member_id) = (text(host, "id"), text(member, "id"));
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::SessionGroup(arena_proto::SessionGroup {
                    session_id: host_id.clone(),
                    member_ids: vec![member_id.clone()],
                }),
                Some(int(host, "rev")),
            ));
            match code(&out) {
                None => {
                    prop_assert!(out.is_ok(), "{:?}", out);
                    let after = hub.reader.rows(Table::Sessions).unwrap();
                    let group_of = |id: &str| {
                        after
                            .iter()
                            .find(|s| text(s, "id") == id)
                            .map(|s| s["group_bill_id"].clone())
                    };
                    prop_assert!(group_of(&host_id) != Some(RowValue::Null));
                    prop_assert_eq!(group_of(&host_id), group_of(&member_id));
                }
                Some((ErrorCode::ValidationFailed | ErrorCode::NotFound, _)) => {
                    prop_assert_eq!(hub.change_count(), before)
                }
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
            }
            let Some(group) = hub.reader.rows(Table::GroupBills).unwrap().pop() else {
                return Ok(());
            };
            let before = hub.change_count();
            let out = hub.submit_new(hub.intent(
                IntentPayload::GroupPayer(arena_proto::GroupPayer {
                    group_bill_id: text(&group, "id"),
                    player_id: None,
                    guest_no: Some(i64::from(*guest)),
                }),
                Some(int(&group, "rev")),
            ));
            match code(&out) {
                None => prop_assert!(out.is_ok(), "{:?}", out),
                Some((ErrorCode::ValidationFailed | ErrorCode::Stale, _)) => {
                    prop_assert_eq!(hub.change_count(), before)
                }
                Some(other) => prop_assert!(false, "unexpected refusal {other:?}"),
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
                    business: None,
                    coupon: None,
                    discount: None,
                    order_id: id,
                    credit_override: over_ride.then_some(true),
                    payments: vec![arena_proto::PaymentRow {
                        method: arena_proto::PaymentMethod::Credit,
                        amount_minor: total,
                        reference: None,
                    }],
                    receipt: None,
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
                // A session's bill settles once the session ended; a running one holds an open
                // order from its first placement.
                let running: Vec<String> = hub
                    .reader
                    .rows(Table::Sessions)
                    .unwrap()
                    .iter()
                    .filter(|x| matches!(text(x, "state").as_str(), "active" | "paused" | "grace"))
                    .map(|x| text(x, "id"))
                    .collect();
                let open: Vec<_> = hub
                    .reader
                    .rows(Table::Orders)
                    .unwrap()
                    .into_iter()
                    .filter(|o| text(o, "status") == "open")
                    .filter(|o| match &o["session_id"] {
                        RowValue::Text(id) => !running.contains(id),
                        _ => true,
                    })
                    .collect();
                let Some(order) = open.get(usize::from(*slot) % open.len().max(1)) else {
                    return Ok(());
                };
                let (id, total) = (text(order, "id"), int(order, "total_minor"));
                let out = hub.submit_new(hub.intent(
                    IntentPayload::SessionSettle(arena_proto::SessionSettle {
                        business: None,
                        coupon: None,
                        discount: None,
                        order_id: id.clone(),
                        credit_override: None,
                        payments: vec![arena_proto::PaymentRow {
                            method: arena_proto::PaymentMethod::Card,
                            amount_minor: total,
                            reference: Some("T1".into()),
                        }],
                        receipt: None,
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

fn assert_sessions_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let mut live = std::collections::HashSet::new();
    for s in hub.reader.rows(Table::Sessions).unwrap() {
        let state = text(&s, "state");
        let held = state == "expired" && s["ended_at"] == RowValue::Null;
        if matches!(state.as_str(), "active" | "paused" | "grace") || held {
            prop_assert!(
                live.insert(text(&s, "device_id")),
                "two live sessions on a device"
            );
        }
    }
    Ok(())
}

fn assert_orders_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let payments = hub.reader.rows(Table::Payments).unwrap();
    // An order that holds placed products totals its lines, and a placed line has its stamp.
    let lines = hub.reader.rows(Table::OrderLines).unwrap();
    for o in hub.reader.rows(Table::Orders).unwrap() {
        let id = text(&o, "id");
        let own: Vec<_> = lines.iter().filter(|l| text(l, "order_id") == id).collect();
        if own.iter().any(|l| l["placement_id"] != RowValue::Null) {
            let sum: i64 = own.iter().map(|l| int(l, "amount_minor")).sum();
            prop_assert_eq!(int(&o, "total_minor"), sum, "order {} total", id);
            prop_assert_eq!(int(&o, "subtotal_minor"), sum, "order {} subtotal", id);
        }
    }
    for p in &payments {
        let reference = match &p["reference"] {
            RowValue::Text(t) => Some(t.clone()),
            _ => None,
        };
        match text(p, "method").as_str() {
            "upi" => {
                let r = reference.unwrap_or_default();
                prop_assert!(
                    r.len() == 4 && r.bytes().all(|b| b.is_ascii_digit()),
                    "UPI ref {r:?}"
                );
            }
            "card" => prop_assert!(reference.is_some_and(|r| !r.is_empty())),
            _ => {}
        }
    }
    for o in hub.reader.rows(Table::Orders).unwrap() {
        if text(&o, "status") != "settled" {
            continue;
        }
        let id = text(&o, "id");
        let paid: i64 = payments
            .iter()
            .filter(|p| text(p, "order_id") == id)
            .map(|p| int(p, "amount_minor"))
            .sum();
        prop_assert_eq!(
            paid,
            int(&o, "total_minor"),
            "settled order {} payments",
            id
        );
    }
    Ok(())
}

/// The cash ledger agrees with the payments: the net `sale` events (each `sale_reverted` row
/// cancels its sale) equal the cash payment rows that exist, and the float is counted in once.
fn assert_cash_holds(hub: &Hub) -> Result<(), TestCaseError> {
    let events = hub.reader.rows(Table::CashEvents).unwrap();
    let net = |kind: &str| -> i64 {
        events
            .iter()
            .filter(|e| text(e, "kind") == kind)
            .map(|e| int(e, "amount_minor"))
            .sum()
    };
    let cash: i64 = hub
        .reader
        .rows(Table::Payments)
        .unwrap()
        .iter()
        .filter(|p| text(p, "method") == "cash")
        .map(|p| int(p, "amount_minor"))
        .sum();
    prop_assert_eq!(net("sale") + net("sale_reverted"), cash);
    let shifts = hub.reader.rows(Table::Shifts).unwrap();
    prop_assert_eq!(
        net("float_in"),
        shifts.iter().map(|s| int(s, "float_minor")).sum::<i64>()
    );
    prop_assert!(shifts.iter().filter(|s| text(s, "state") == "open").count() <= 1);
    Ok(())
}

/// Every stock row with its lots, for comparing the stock before and after a sale or its Undo.
type StockImage = Vec<(String, i64, Vec<(String, i64)>)>;

fn stock_image(hub: &Hub) -> StockImage {
    let mut image: Vec<_> = hub
        .reader
        .rows(Table::Stock)
        .unwrap()
        .iter()
        .map(|s| {
            let id = text(s, "product_id");
            let mut lots: Vec<_> = hub
                .reader
                .lots(&id)
                .unwrap()
                .into_iter()
                .map(|l| (l.lot_id, l.qty_remaining))
                .collect();
            lots.sort();
            (id, int(s, "qty"), lots)
        })
        .collect();
    image.sort();
    image
}

/// Stock never goes below zero, and every product's quantity is what its lots hold.
fn assert_stock_holds(hub: &Hub) -> Result<(), TestCaseError> {
    for (id, qty, lots) in stock_image(hub) {
        prop_assert!(qty >= 0, "{id} went below zero");
        prop_assert_eq!(lots.iter().map(|(_, q)| q).sum::<i64>(), qty, "{} lots", id);
    }
    Ok(())
}

/// The units open stock holds claim, over every product.
fn open_hold_qty(hub: &Hub) -> i64 {
    hub.reader
        .rows(Table::StockHolds)
        .unwrap()
        .iter()
        .filter(|h| h["released_at"] == RowValue::Null)
        .map(|h| int(h, "qty"))
        .sum()
}

/// A hold is open only for a line that exists and is not served; a served line holds nothing.
fn assert_holds_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let lines = hub.reader.rows(Table::OrderLines).unwrap();
    for h in hub.reader.rows(Table::StockHolds).unwrap() {
        prop_assert!(int(&h, "qty") > 0);
        if h["released_at"] != RowValue::Null {
            continue;
        }
        let line = lines.iter().find(|l| l["id"] == h["order_line_id"]);
        prop_assert!(line.is_some(), "an open hold of a missing line");
        prop_assert_eq!(
            int(line.unwrap(), "served"),
            0,
            "an open hold of a served line"
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
                assert_sessions_hold(&hub)?;
                assert_orders_hold(&hub)?;
                assert_cash_holds(&hub)?;
                assert_stock_holds(&hub)?;
                assert_holds_hold(&hub)?;
                assert_invoices_hold(&hub)?;
                assert_coupons_hold(&hub)?;
                assert_discounts_hold(&hub)?;
                assert_voids_hold(&hub)?;
                assert_prints_hold(&hub)?;
                assert_drawers_hold(&hub)?;
                assert_notifications_hold(&hub)?;
            }
            assert_replay_reproduces(&hub)
        })
        .unwrap();
}

/// The outbox invariants: a queued print names a settled order, and a settled order with a total
/// that queued a print has its number.
fn assert_prints_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let orders = hub.reader.rows(Table::Orders).unwrap();
    for row in hub.reader.queued_prints(10_000).unwrap() {
        let order = orders.iter().find(|o| text(o, "id") == row.ref_id);
        prop_assert!(order.is_some(), "a queued print names a missing order");
        let order = order.unwrap();
        prop_assert_eq!(text(order, "status"), "settled");
        if int(order, "total_minor") > 0 {
            prop_assert!(matches!(order["invoice_no"], RowValue::Text(_)));
        }
    }
    Ok(())
}

/// The drawer invariants: a queued kick names a settled order that took cash against a shift, and
/// that order's Undo is closed by the recorded effect.
fn assert_drawers_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let orders = hub.reader.rows(Table::Orders).unwrap();
    let events = hub.reader.rows(Table::CashEvents).unwrap();
    for row in hub.reader.queued_drawers(10_000).unwrap() {
        let order = orders.iter().find(|o| text(o, "id") == row.ref_id);
        prop_assert!(order.is_some(), "a queued kick names a missing order");
        prop_assert_eq!(text(order.unwrap(), "status"), "settled");
        prop_assert!(
            events.iter().any(|e| text(e, "kind") == "sale"
                && e["order_id"] == RowValue::Text(row.ref_id.clone())),
            "a queued kick names an order that took no cash"
        );
    }
    Ok(())
}

/// The invoice invariants: a number is unique, only a settled sale has one, and every
/// `(fy, segment)` holds the sequence 1..next-1 with no gap and no repeat.
fn assert_invoices_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let orders = hub.reader.rows(Table::Orders).unwrap();
    let mut numbers = std::collections::BTreeSet::new();
    let mut per_segment = std::collections::BTreeMap::<String, Vec<i64>>::new();
    for o in &orders {
        let RowValue::Text(no) = &o["invoice_no"] else {
            continue;
        };
        prop_assert!(numbers.insert(no.clone()), "{no} issued twice");
        prop_assert_eq!(text(o, "status"), "settled");
        prop_assert!(int(o, "total_minor") > 0);
        let (prefix, seq) = no.rsplit_once('/').unwrap();
        per_segment
            .entry(prefix.to_owned())
            .or_default()
            .push(seq.parse().unwrap());
    }
    for seqs in per_segment.values_mut() {
        seqs.sort_unstable();
        let want: Vec<i64> = (1..=seqs.len() as i64).collect();
        prop_assert_eq!(&*seqs, &want, "a segment has a gap or a repeat");
    }
    let sequences: i64 = hub
        .reader
        .rows(Table::InvoiceSequences)
        .unwrap()
        .iter()
        .map(|s| int(s, "next") - 1)
        .sum();
    prop_assert_eq!(sequences, numbers.len() as i64);
    Ok(())
}

/// An order's discount amount, reason and approver are set together; the amount is above zero and
/// only a settled order has one.
fn assert_discounts_hold(hub: &Hub) -> Result<(), TestCaseError> {
    for o in &hub.reader.rows(Table::Orders).unwrap() {
        let set = [
            matches!(o["discount_minor"], RowValue::Integer(_)),
            matches!(o["discount_reason"], RowValue::Text(_)),
            matches!(o["discount_by"], RowValue::Text(_)),
        ];
        prop_assert!(
            set.iter().all(|s| *s) || set.iter().all(|s| !*s),
            "an order's discount amount, reason and approver are set together"
        );
        if set[0] {
            prop_assert!(int(o, "discount_minor") > 0);
            prop_assert_eq!(text(o, "status"), "settled");
        }
    }
    Ok(())
}

/// A voided order records when, by whom and why, and has left its session; an order that is not
/// void carries none of it.
fn assert_voids_hold(hub: &Hub) -> Result<(), TestCaseError> {
    for o in &hub.reader.rows(Table::Orders).unwrap() {
        let set = [
            matches!(o["voided_at"], RowValue::Integer(_)),
            matches!(o["voided_by"], RowValue::Text(_)),
            matches!(o["void_reason"], RowValue::Text(_)),
            matches!(o["void_session_id"], RowValue::Text(_)),
        ];
        let void = text(o, "status") == "void";
        prop_assert!(set.iter().all(|s| *s == void), "void columns of {:?}", o);
        if void {
            prop_assert_eq!(&o["session_id"], &RowValue::Null);
        }
    }
    let payments = hub.reader.rows(Table::Payments).unwrap();
    for o in hub.reader.rows(Table::Orders).unwrap() {
        if text(&o, "status") == "void" {
            prop_assert!(
                payments.iter().all(|p| p["order_id"] != o["id"]),
                "a voided order was never paid"
            );
        }
    }
    Ok(())
}

/// A coupon's `used` is the number of settled orders that took it, never above its limit, and an
/// order's coupon code and amount are set together.
fn assert_coupons_hold(hub: &Hub) -> Result<(), TestCaseError> {
    let orders = hub.reader.rows(Table::Orders).unwrap();
    for o in &orders {
        let (code, minor) = (&o["coupon_code"], &o["coupon_minor"]);
        prop_assert_eq!(
            matches!(code, RowValue::Text(_)),
            matches!(minor, RowValue::Integer(_)),
            "an order's coupon code and amount are set together"
        );
        if matches!(minor, RowValue::Integer(_)) {
            prop_assert!(int(o, "coupon_minor") > 0);
            prop_assert_eq!(text(o, "status"), "settled");
        }
    }
    for (coupon, _) in hub.reader.coupons().unwrap() {
        let took = orders
            .iter()
            .filter(|o| o["coupon_code"] == RowValue::Text(coupon.code.clone()))
            .count() as i64;
        prop_assert_eq!(coupon.used, took, "{} used", coupon.code);
        if let Some(limit) = coupon.usage_limit {
            prop_assert!(coupon.used <= limit, "{} is over its limit", coupon.code);
        }
    }
    Ok(())
}

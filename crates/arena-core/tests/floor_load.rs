//! Throughput proof for the floor (Story 3.37): 60 devices, a realistic mix of start, pause,
//! resume, extend, transfer, end and settle intents at 5/s with 50/s bursts, through the Writer on
//! a temp file.
//!
//! Fails when server-side apply is over 100 ms at p95, a committed change takes over 2 s to reach
//! a subscriber, an acknowledged intent is missing from the log, a resent idempotency key writes
//! again, or `changes.seq` has a gap. `FLOOR_LOAD_SECONDS` (default 6) and
//! `FLOOR_LOAD_BURST_SECONDS` (default 2) set the length; the release benchmark runs 600 and 10.
//! `FLOOR_LOAD_P95_MS` and `FLOOR_LOAD_DELIVERY_MS` override the budgets.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_core::{
    Applied, HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel,
};
use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ClientClass, DeviceAssign, DeviceKind,
    DeviceRegister, ErrorCode, ExtendSource, GroupUpsert, Intent, IntentPayload, PauseReason,
    PaymentMethod, PaymentRow, RateUpsert, SessionEnd, SessionExtend, SessionMode, SessionPause,
    SessionResume, SessionSettle, SessionStart, SessionTransfer, WalletTopup,
};
use arena_store::{LeaseRow, Row, RowValue, Store, Table, write_lease};

const T0: i64 = 1_800_000_000_000;
const HUB: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";
const DEVICES: usize = 60;
const SUSTAINED_PER_S: u64 = 5;
const BURST_PER_S: u64 = 50;

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn text(row: &Row, col: &str) -> String {
    match &row[col] {
        RowValue::Text(t) => t.clone(),
        other => panic!("{col}: {other:?}"),
    }
}

fn int(row: &Row, col: &str) -> i64 {
    match &row[col] {
        RowValue::Integer(n) => *n,
        other => panic!("{col}: {other:?}"),
    }
}

fn intent(payload: IntentPayload, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev,
        actor: Actor {
            kind: if matches!(payload, IntentPayload::BranchBootstrap(_)) {
                ActorKind::System
            } else {
                ActorKind::Owner
            },
            id: "load".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

struct Floor {
    _dir: Option<tempfile::TempDir>,
    _status: tokio::sync::watch::Sender<HubStatus>,
    writer: WriterHandle,
    reader: Store,
    devices: Vec<String>,
    player: String,
    clock: Arc<ManualClock>,
    /// Tracked products with their prices, for the counter-sale mix (Story 5.39).
    products: Vec<(String, i64)>,
}

async fn must(writer: &WriterHandle, payload: IntentPayload) -> Applied {
    writer
        .submit_intent(intent(payload, None))
        .await
        .expect("setup intent")
}

async fn floor() -> Floor {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    floor_at(path, Some(dir)).await
}

async fn floor_at(path: std::path::PathBuf, dir: Option<tempfile::TempDir>) -> Floor {
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

    must(
        &writer,
        IntentPayload::BranchBootstrap(BranchBootstrap {
            name: "Load Branch".into(),
            branch_code: "LOAD1".into(),
            currency: "INR".into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
            onboarding: None,
        }),
    )
    .await;
    // Cash settles need an open shift.
    must(
        &writer,
        IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor: 0 }),
    )
    .await;
    writer
        .apply_licence(arena_proto::LicenceApply {
            standing: arena_proto::LicenceStanding::Verified,
            clock_wrong: false,
            tier: arena_proto::Tier::Free,
            flags: String::new(),
            device_cap: 100,
            user_cap: 10,
        })
        .await
        .expect("licence");
    let hash = arena_core::staff::hash_secret("pw").unwrap();
    let group = must(
        &writer,
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
    )
    .await
    .changes[0]
        .row_id
        .clone();
    must(
        &writer,
        IntentPayload::RateUpsert(RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: 12_000,
        }),
    )
    .await;
    let mut devices = Vec::new();
    for n in 0..DEVICES {
        let mut reg = DeviceRegister::new(ClientClass::Paired, format!("PC {n}"), &hash);
        reg.device_kind = DeviceKind::Pc;
        let id = must(&writer, IntentPayload::DeviceRegister(reg))
            .await
            .changes[0]
            .row_id
            .clone();
        must(
            &writer,
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: id.clone(),
                group_id: Some(group.clone()),
            }),
        )
        .await;
        devices.push(id);
    }

    const KEK: [u8; 32] = [3; 32];
    let form = arena_proto::PlayerForm {
        username: "loader".into(),
        password: "secret1".into(),
        phone: "+919000000001".into(),
        name: "Asha".into(),
        email: String::new(),
        dob: String::new(),
        notes: String::new(),
        tags: Vec::new(),
        consent: true,
        policy_version: "1".into(),
    };
    let sealed = arena_core::players::seal(
        &KEK,
        &form,
        &ulid::Ulid::generate().to_string(),
        hash.clone(),
    )
    .unwrap();
    let player = must(&writer, IntentPayload::PlayerCreate(sealed))
        .await
        .changes[0]
        .row_id
        .clone();
    let amount_minor = 5_000_000;
    must(
        &writer,
        IntentPayload::WalletTopup(WalletTopup {
            player_id: player.clone(),
            amount_minor,
            payments: vec![PaymentRow {
                method: PaymentMethod::Cash,
                amount_minor,
                reference: None,
            }],
        }),
    )
    .await;
    let mut products = Vec::new();
    for (name, price_minor) in [("Cola", 4_000), ("Chips", 2_500), ("Water", 1_000)] {
        let id = must(
            &writer,
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: name.into(),
                category: String::new(),
                price_minor,
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
            }),
        )
        .await
        .changes[0]
            .row_id
            .clone();
        must(
            &writer,
            IntentPayload::StockIn(arena_proto::StockIn {
                product_id: id.clone(),
                qty: arena_proto::MAX_STOCK_IN_QTY,
                unit_cost_minor: price_minor / 2,
                supplier: "Wholesale".into(),
                expiry: None,
            }),
        )
        .await;
        products.push((id, price_minor));
    }
    Floor {
        _dir: dir,
        _status: status_tx,
        writer,
        reader,
        devices,
        player,
        clock,
        products,
    }
}

/// A deterministic xorshift so a failing run repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

struct Sent {
    intent: Intent,
    seqs: Vec<i64>,
}

fn live(s: &Row) -> bool {
    matches!(text(s, "state").as_str(), "active" | "paused" | "grace")
}

/// Picks the next realistic intent from the state the Hub holds now.
fn next_intent(f: &Floor, rng: &mut Rng) -> Option<Intent> {
    let sessions = f.reader.rows(Table::Sessions).unwrap();
    let running: Vec<&Row> = sessions.iter().filter(|s| live(s)).collect();
    let busy: HashSet<String> = running.iter().map(|s| text(s, "device_id")).collect();
    let free: Vec<&String> = f.devices.iter().filter(|d| !busy.contains(*d)).collect();
    let roll = rng.below(100);
    if roll < 30 || running.is_empty() {
        let device = (*free.get(rng.below(free.len()))?).clone();
        let wallet = rng.below(4) == 0;
        return Some(intent(
            IntentPayload::SessionStart(SessionStart {
                group_with: None,
                pay_from_wallet: false,
                device_id: device,
                mode: if wallet {
                    SessionMode::Wallet
                } else {
                    SessionMode::Postpaid
                },
                player_id: wallet.then(|| f.player.clone()),
                pack_id: None,
                plan_id: None,
            }),
            None,
        ));
    }
    let s = running[rng.below(running.len())];
    let (id, rev) = (text(s, "id"), int(s, "rev"));
    let paused = text(s, "state") == "paused";
    let wallet = text(s, "mode") == "wallet";
    Some(match roll {
        30..=44 if paused => intent(
            IntentPayload::SessionResume(SessionResume { session_id: id }),
            Some(rev),
        ),
        30..=44 => intent(
            IntentPayload::SessionPause(SessionPause {
                session_id: id,
                reason: PauseReason::ALL
                    .iter()
                    .copied()
                    .find(|r| *r != PauseReason::Other)
                    .unwrap(),
                note: None,
            }),
            Some(rev),
        ),
        45..=54 if wallet => intent(
            IntentPayload::SessionExtend(SessionExtend {
                session_id: id,
                minutes: arena_proto::EXTEND_OPTIONS_MIN[0],
                source: ExtendSource::Wallet,
                pack_id: None,
                credit_override: None,
            }),
            None,
        ),
        55..=64 if !free.is_empty() => intent(
            IntentPayload::SessionTransfer(SessionTransfer {
                session_id: id,
                device_id: free[rng.below(free.len())].clone(),
            }),
            Some(rev),
        ),
        _ => {
            let orders = f.reader.rows(Table::Orders).unwrap();
            let open = orders
                .iter()
                .find(|o| text(o, "status") == "open" && o["session_id"] != RowValue::Null);
            if let (Some(o), 70..=89) = (open, roll) {
                let total = int(o, "total_minor");
                if total > 0 {
                    return Some(intent(
                        IntentPayload::SessionSettle(SessionSettle {
                            business: None,
                            coupon: None,
                            discount: None,
                            order_id: text(o, "id"),
                            credit_override: None,
                            payments: vec![PaymentRow {
                                method: PaymentMethod::Cash,
                                amount_minor: total,
                                reference: None,
                            }],
                            receipt: None,
                        }),
                        Some(int(o, "rev")),
                    ));
                }
            }
            intent(
                IntentPayload::SessionEnd(SessionEnd { session_id: id }),
                Some(rev),
            )
        }
    })
}

fn p95(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples
        .get((samples.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .unwrap_or_default()
}

/// Refusals a busy floor produces on its own (a stale pick, a device taken meanwhile); anything
/// else is a defect.
fn expected_refusal(e: &AppError) -> bool {
    matches!(
        e.code,
        ErrorCode::Stale
            | ErrorCode::PcBusy
            | ErrorCode::ValidationFailed
            | ErrorCode::NotFound
            | ErrorCode::InsufficientBalance
            | ErrorCode::PaymentShort
            | ErrorCode::PastClosing
            | ErrorCode::RefundExceedsPaid
            | ErrorCode::InvoiceNotFound
            | ErrorCode::OutOfStock
    )
}

/// Top-ups and credit collections are payments, not sales: they never take an invoice number.
fn numbered_kind(order: &Row) -> bool {
    matches!(
        text(order, "kind").as_str(),
        "session" | "plan_sale" | "pos_sale"
    )
}

/// Which intents a run sends: the session floor of Story 3.37, or the counter of Story 5.39.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mix {
    Floor,
    Counter,
}

/// What the counter mix remembers between intents: numbered cash sales that can still be
/// refunded once.
struct Picker {
    mix: Mix,
    refundable: Vec<String>,
}

impl Picker {
    fn new(mix: Mix) -> Self {
        Self {
            mix,
            refundable: Vec::new(),
        }
    }

    fn next(&mut self, f: &Floor, rng: &mut Rng) -> Option<Intent> {
        match self.mix {
            Mix::Floor => next_intent(f, rng),
            Mix::Counter => Some(self.next_counter(f, rng)),
        }
    }

    /// Notes an acknowledged intent; a printed cash sale is numbered at once and refundable.
    fn acked(&mut self, sent: &Intent, applied: &Applied) {
        if let IntentPayload::PosSale(s) = &sent.payload
            && s.receipt.is_some()
            && s.payments.iter().all(|p| p.method == PaymentMethod::Cash)
        {
            self.refundable.push(applied.changes[0].row_id.clone());
        }
    }

    fn next_counter(&mut self, f: &Floor, rng: &mut Rng) -> Intent {
        f.clock.advance(250);
        let roll = rng.below(100);
        if (80..90).contains(&roll)
            && let Some(order) = self.refundable.pop()
            && let Some(line) = f
                .reader
                .rows(Table::OrderLines)
                .unwrap()
                .into_iter()
                .find(|l| text(l, "order_id") == order)
        {
            let mut refund = intent(
                IntentPayload::RefundCreate(arena_proto::RefundCreate {
                    invoice_no: f
                        .reader
                        .rows(Table::Orders)
                        .unwrap()
                        .iter()
                        .find(|o| text(o, "id") == order)
                        .map(|o| text(o, "invoice_no"))
                        .unwrap(),
                    lines: vec![arena_proto::RefundLineReq {
                        line_id: text(&line, "id"),
                        qty: 1,
                        return_to_stock: Some(true),
                    }],
                    method: Some(PaymentMethod::Cash),
                    reference: None,
                    note: None,
                    receipt: None,
                }),
                None,
            );
            refund.reason = Some(arena_proto::ReasonCode::ChangedMind);
            return refund;
        }
        if (70..80).contains(&roll) {
            let mut waste = intent(
                IntentPayload::StockWaste(arena_proto::StockWaste {
                    product_id: f.products[rng.below(f.products.len())].0.clone(),
                    qty: 1,
                }),
                None,
            );
            waste.reason = arena_proto::ReasonCode::WASTE.first().copied();
            return waste;
        }
        if roll >= 90
            && let Some(order) = f.reader.rows(Table::Orders).unwrap().iter().find(|o| {
                text(o, "status") == "settled"
                    && numbered_kind(o)
                    && o["invoice_no"] == RowValue::Null
                    && int(o, "total_minor") > 0
            })
        {
            let mut issue = intent(
                IntentPayload::InvoiceIssue(arena_proto::InvoiceIssue {
                    order_id: text(order, "id"),
                }),
                None,
            );
            issue.actor.kind = ActorKind::System;
            return issue;
        }
        let mut lines = Vec::new();
        let mut total = 0;
        for _ in 0..=rng.below(3) {
            let (id, price) = &f.products[rng.below(f.products.len())];
            if lines
                .iter()
                .any(|l: &arena_proto::PosSaleLine| &l.product_id == id)
            {
                continue;
            }
            let qty = 1 + rng.below(3) as i64;
            total += price * qty;
            lines.push(arena_proto::PosSaleLine {
                product_id: id.clone(),
                qty,
            });
        }
        let upi = (55..70).contains(&roll);
        let discount = (!upi && roll.is_multiple_of(8)).then_some(arena_proto::ManualDiscount {
            percent_bp: None,
            amount_minor: Some(100),
            reason: arena_proto::DiscountReason::RegularCustomer,
            note: None,
        });
        if discount.is_some() {
            total -= 100;
        }
        intent(
            IntentPayload::PosSale(arena_proto::PosSale {
                lines,
                payments: vec![PaymentRow {
                    method: if upi {
                        PaymentMethod::Upi
                    } else {
                        PaymentMethod::Cash
                    },
                    amount_minor: total,
                    reference: upi.then(|| "1234".to_owned()),
                }],
                player_id: None,
                credit_override: None,
                stock_override: None,
                receipt: (!upi && roll.is_multiple_of(3))
                    .then_some(arena_proto::ReceiptChoice::Print),
                business: None,
                coupon: None,
                discount,
            }),
            None,
        )
    }
}

/// The books after any run: stock equals its lots and its `stock_moves`, settled orders are
/// paid in full, invoice and credit-note numbers are unique and gap-free per series, the tax
/// of a counter sale adds up, and the cash ledger agrees with the cash payments and the shift.
fn assert_books(store: &Store) {
    let moves = store.rows(Table::StockMoves).unwrap();
    for s in store.rows(Table::Stock).unwrap() {
        let id = text(&s, "product_id");
        let qty = int(&s, "qty");
        assert!(qty >= 0, "{id} went below zero");
        let lots: i64 = store
            .lots(&id)
            .unwrap()
            .iter()
            .map(|l| l.qty_remaining)
            .sum();
        assert_eq!(lots, qty, "{id} lots");
        let moved: i64 = moves
            .iter()
            .filter(|m| text(m, "product_id") == id)
            .map(|m| int(m, "delta"))
            .sum();
        assert_eq!(moved, qty, "{id} stock_moves");
    }
    let payments = store.rows(Table::Payments).unwrap();
    let mut numbers = HashSet::new();
    let mut series = std::collections::BTreeMap::<String, Vec<i64>>::new();
    let mut note = |no: String| {
        assert!(numbers.insert(no.clone()), "{no} issued twice");
        let (prefix, seq) = no.rsplit_once('/').unwrap();
        series
            .entry(prefix.to_owned())
            .or_default()
            .push(seq.parse().unwrap());
    };
    for o in store.rows(Table::Orders).unwrap() {
        if text(&o, "kind") == "pos_sale" && text(&o, "status") == "settled" {
            assert_eq!(
                int(&o, "total_minor"),
                int(&o, "subtotal_minor") + int(&o, "tax_minor"),
                "counter sale tax"
            );
        }
        if text(&o, "status") == "settled" {
            let id = text(&o, "id");
            let paid: i64 = payments
                .iter()
                .filter(|p| text(p, "order_id") == id)
                .map(|p| int(p, "amount_minor"))
                .filter(|a| *a > 0) // refunds are negative rows, credited by their notes
                .sum();
            assert_eq!(paid, int(&o, "total_minor"), "settled order {id} payments");
        }
        if let RowValue::Text(no) = &o["invoice_no"] {
            assert_eq!(text(&o, "status"), "settled", "an invoice on an open order");
            note(no.clone());
        }
    }
    for r in store.rows(Table::Refunds).unwrap() {
        note(text(&r, "credit_note_no"));
    }
    for seqs in series.values_mut() {
        seqs.sort_unstable();
        let want: Vec<i64> = (1..=seqs.len() as i64).collect();
        assert_eq!(*seqs, want, "an invoice series has a gap or a repeat");
    }
    let events = store.rows(Table::CashEvents).unwrap();
    let net = |kinds: &[&str]| -> i64 {
        events
            .iter()
            .filter(|e| kinds.contains(&text(e, "kind").as_str()))
            .map(|e| int(e, "amount_minor"))
            .sum()
    };
    let cash: i64 = payments
        .iter()
        .filter(|p| text(p, "method") == "cash")
        .map(|p| int(p, "amount_minor"))
        .sum();
    assert_eq!(
        net(&["sale", "sale_reverted", "refund"]),
        cash,
        "cash events"
    );
    for shift in store.rows(Table::Shifts).unwrap() {
        let id = text(&shift, "id");
        let expected = store
            .read_with(|conn| arena_store::expected_cash(conn, &id))
            .unwrap();
        let own: i64 = events
            .iter()
            .filter(|e| text(e, "shift_id") == id)
            .map(|e| int(e, "amount_minor"))
            .sum();
        assert_eq!(expected, own, "shift {id} drawer");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_floor_load_holds_apply_p95_and_loses_nothing() {
    load(Mix::Floor).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn counter_sale_load_holds_apply_p95_and_the_books() {
    load(Mix::Counter).await;
}

async fn load(mix: Mix) {
    let seconds = env_u64("FLOOR_LOAD_SECONDS", 6);
    let burst_seconds = env_u64("FLOOR_LOAD_BURST_SECONDS", 2);
    let p95_budget = Duration::from_millis(env_u64("FLOOR_LOAD_P95_MS", 100));
    let delivery_budget = Duration::from_millis(env_u64("FLOOR_LOAD_DELIVERY_MS", 2_000));

    let f = floor().await;
    let base_changes = f.reader.changes().unwrap().len();
    let mut feed = f.writer.subscribe();
    let delivered = Arc::new(std::sync::Mutex::new(Vec::<(Instant, Applied)>::new()));
    let sink = Arc::clone(&delivered);
    let collector = tokio::spawn(async move {
        while let Ok(applied) = feed.recv().await {
            sink.lock().unwrap().push((Instant::now(), applied));
        }
    });

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut picker = Picker::new(mix);
    let mut apply = Vec::new();
    let mut sent: Vec<(Instant, Sent)> = Vec::new();
    let mut refused = 0usize;
    let mut resent = 0usize;
    let begin = Instant::now();
    let sustained = Duration::from_secs(seconds);
    let burst_at = sustained / 2;
    let mut sent_count = 0u64;
    let mut due = begin;
    while begin.elapsed() < sustained {
        let in_burst = begin.elapsed() >= burst_at
            && begin.elapsed() < burst_at + Duration::from_secs(burst_seconds);
        let rate = if in_burst {
            BURST_PER_S
        } else {
            SUSTAINED_PER_S
        };
        let Some(next) = picker.next(&f, &mut rng) else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        let t = Instant::now();
        let out = f.writer.submit_intent(next.clone()).await;
        let took = t.elapsed();
        apply.push(took);
        sent_count += 1;
        match out {
            Ok(applied) => {
                let seqs = applied.changes.iter().map(|c| c.seq).collect();
                picker.acked(&next, &applied);
                if sent_count.is_multiple_of(7) {
                    let again = f
                        .writer
                        .submit_intent(next.clone())
                        .await
                        .expect("a resent key returns its first outcome");
                    assert_eq!(again.changes, applied.changes, "resent key rewrote");
                    resent += 1;
                }
                sent.push((t, Sent { intent: next, seqs }));
            }
            Err(e) => {
                assert!(expected_refusal(&e), "unexpected refusal {e:?}");
                refused += 1;
            }
        }
        due += Duration::from_micros(1_000_000 / rate);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            tokio::time::sleep(wait).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    collector.abort();

    let log = f.reader.changes().unwrap();
    let seqs: Vec<i64> = log.iter().map(|c| c.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "changes.seq has a gap"
    );
    let logged: HashSet<i64> = seqs.iter().copied().collect();
    let expected: usize = sent.iter().map(|(_, s)| s.seqs.len()).sum();
    for (_, s) in &sent {
        for seq in &s.seqs {
            assert!(
                logged.contains(seq),
                "intent {} lost seq {seq}",
                s.intent.intent_id
            );
        }
    }
    assert_eq!(
        log.len() - base_changes,
        expected,
        "the log holds rows no acknowledged intent wrote ({resent} resends, {refused} refusals)"
    );

    let got = delivered.lock().unwrap();
    let mut slow = Duration::ZERO;
    for (at, s) in &sent {
        let hit = got
            .iter()
            .find(|(_, a)| a.intent_id == s.intent.intent_id)
            .unwrap_or_else(|| panic!("intent {} never reached a subscriber", s.intent.intent_id));
        slow = slow.max(hit.0.saturating_duration_since(*at));
    }
    assert!(
        slow <= delivery_budget,
        "slowest update took {slow:?}, budget {delivery_budget:?}"
    );

    assert_books(&f.reader);
    let got_p95 = p95(&mut apply);
    println!(
        "floor load: {} intents, {refused} refused, {resent} resent, apply p95 {got_p95:?}, slowest update {slow:?}",
        apply.len()
    );
    assert!(
        got_p95 <= p95_budget,
        "apply p95 {got_p95:?} over budget {p95_budget:?}"
    );
}

const CHILD_ENV: &str = "ARENA_FLOOR_CRASH_CHILD_DB";
const FLOOR_CRASH_TEST: &str = "a_hub_killed_during_a_burst_keeps_every_acknowledged_row";
const COUNTER_CRASH_TEST: &str =
    "a_hub_killed_during_a_sale_burst_keeps_every_sale_stock_move_and_invoice";
const ACKS_BEFORE_KILL: usize = 80;

/// The child: builds the floor, then submits intents flat out, printing the sequence numbers of
/// each acknowledged one (and the order of each acknowledged Print sale), until the parent kills
/// it.
async fn burst_until_killed(db: std::path::PathBuf, mix: Mix) -> ! {
    use std::io::Write;
    let f = floor_at(db, None).await;
    println!("READY");
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let mut picker = Picker::new(mix);
    loop {
        let Some(next) = picker.next(&f, &mut rng) else {
            continue;
        };
        if let Ok(applied) = f.writer.submit_intent(next.clone()).await {
            picker.acked(&next, &applied);
            if let IntentPayload::PosSale(s) = &next.payload
                && s.receipt.is_some()
            {
                println!("PRINT {}", applied.changes[0].row_id);
            }
            let seqs: Vec<String> = applied.changes.iter().map(|c| c.seq.to_string()).collect();
            println!("ACK {}", seqs.join(","));
            std::io::stdout().flush().unwrap();
        }
    }
}

/// What the parent read from the child before killing it.
struct Acked {
    seqs: Vec<i64>,
    printed: Vec<String>,
    count: usize,
}

async fn crash_drill(mix: Mix, test: &'static str) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    if let Some(db) = std::env::var_os(CHILD_ENV) {
        burst_until_killed(std::path::PathBuf::from(db), mix).await;
    }
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("branch.db");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, &db)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = tokio::task::spawn_blocking(move || {
        let mut acked = Acked {
            seqs: Vec::new(),
            printed: Vec::new(),
            count: 0,
        };
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(order) = line.trim_end().strip_prefix("PRINT ") {
                acked.printed.push(order.to_owned());
                continue;
            }
            let Some(rest) = line
                .trim_end()
                .rsplit("ACK ")
                .next()
                .filter(|_| line.contains("ACK "))
            else {
                continue;
            };
            acked
                .seqs
                .extend(rest.split(',').filter_map(|s| s.parse::<i64>().ok()));
            acked.count += 1;
            if acked.count >= ACKS_BEFORE_KILL {
                break;
            }
        }
        acked
    });
    let acked = tokio::time::timeout(Duration::from_secs(120), reader)
        .await
        .expect("the child acknowledged no burst in time")
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        acked.count >= ACKS_BEFORE_KILL,
        "the child died after {} acks",
        acked.count
    );

    let store = Store::open(&db, T0 + 1).unwrap();
    assert_eq!(store.integrity_check().unwrap(), "ok");
    let log: HashSet<i64> = store.changes().unwrap().iter().map(|c| c.seq).collect();
    let seqs = {
        let mut v: Vec<i64> = log.iter().copied().collect();
        v.sort_unstable();
        v
    };
    assert!(
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "changes.seq has a gap"
    );
    for seq in &acked.seqs {
        assert!(log.contains(seq), "acknowledged change {seq} is missing");
    }

    if mix == Mix::Counter {
        assert_books(&store);
        let queued = store.queued_prints(100_000).unwrap();
        assert!(!acked.printed.is_empty(), "no Print sale was acknowledged");
        for order in &acked.printed {
            let rows = queued.iter().filter(|r| &r.ref_id == order).count();
            assert_eq!(rows, 1, "the Print sale {order} has {rows} queued receipts");
            let row = store
                .rows(Table::Orders)
                .unwrap()
                .into_iter()
                .find(|o| text(o, "id") == *order)
                .unwrap();
            assert!(
                matches!(row["invoice_no"], RowValue::Text(_)),
                "a printed sale is numbered in its own settle"
            );
        }
        drop(store);
        issue_pending_invoices(&db).await;
        return;
    }

    let rows = store.rows(Table::Sessions).unwrap();
    let running = rows.iter().filter(|s| live(s)).count();
    let timed = arena_core::SnapshotReader::open(&db)
        .unwrap()
        .timed_sessions(T0 + 1)
        .unwrap();
    let mut engine = arena_core::TimerEngine::new();
    engine.sync(timed.clone(), T0 + 1);
    assert_eq!(engine.len(), timed.len(), "the timer heap lost a session");
    let timed_ids: HashSet<&str> = timed.iter().map(|t| t.session_id.as_str()).collect();
    for s in rows
        .iter()
        .filter(|s| live(s) && text(s, "state") != "paused")
    {
        if matches!(s["ends_at"], RowValue::Integer(_)) {
            assert!(
                timed_ids.contains(text(s, "id").as_str()),
                "timed session not rebuilt"
            );
        }
    }

    let snapshot = arena_core::read_snapshot(&store, T0 + 1).unwrap();
    assert!(
        snapshot.sessions.len() >= running,
        "the snapshot a kiosk re-syncs to lacks {} sessions",
        running - snapshot.sessions.len().min(running)
    );
}

/// The restarted Hub numbers every sale the crash left pending: each once, with no gap, and a
/// second ask is refused instead of taking another number.
async fn issue_pending_invoices(db: &std::path::Path) {
    let mut store = Store::open(db, T0 + 1).unwrap();
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
    let (_status, rx) = status_channel(status);
    let writer = Writer::spawn(store, Arc::new(ManualClock::new(T0 + 3_600_000)), rx).unwrap();
    let reader = Store::open(db, T0 + 1).unwrap();
    let pending: Vec<String> = reader
        .rows(Table::Orders)
        .unwrap()
        .iter()
        .filter(|o| {
            text(o, "status") == "settled"
                && numbered_kind(o)
                && o["invoice_no"] == RowValue::Null
                && int(o, "total_minor") > 0
        })
        .map(|o| text(o, "id"))
        .collect();
    let issue = |order: &String| {
        let mut i = intent(
            IntentPayload::InvoiceIssue(arena_proto::InvoiceIssue {
                order_id: order.clone(),
            }),
            None,
        );
        i.actor.kind = ActorKind::System;
        i
    };
    assert!(!pending.is_empty(), "no sale was left waiting for a number");
    for order in &pending {
        writer
            .submit_intent(issue(order))
            .await
            .expect("a pending invoice is issued after the restart");
        let again = writer.submit_intent(issue(order)).await;
        let refused = again.expect_err(&format!("{order} was numbered twice"));
        assert_eq!(refused.code, ErrorCode::Stale, "{order}: {refused:?}");
    }
    assert_books(&reader);
    assert!(
        reader
            .rows(Table::Orders)
            .unwrap()
            .iter()
            .filter(|o| numbered_kind(o)
                && text(o, "status") == "settled"
                && int(o, "total_minor") > 0)
            .all(|o| matches!(o["invoice_no"], RowValue::Text(_))),
        "a sale is still unnumbered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hub_killed_during_a_burst_keeps_every_acknowledged_row() {
    crash_drill(Mix::Floor, FLOOR_CRASH_TEST).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hub_killed_during_a_sale_burst_keeps_every_sale_stock_move_and_invoice() {
    crash_drill(Mix::Counter, COUNTER_CRASH_TEST).await;
}

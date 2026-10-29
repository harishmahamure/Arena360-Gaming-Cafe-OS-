//! Offline proof for the floor (Story 3.37): the Epic 3 money and session flows give the same
//! result with the WAN up and down, and nothing in `arena-core` can open a connection.
//!
//! The same scripted flow (player, top-up, postpaid session with pause, end and settle, wallet
//! session with extend, transfer and end) runs against a Hub whose connectivity probe says
//! `online` and against one that says `internet_offline`. The change log's shape and every money
//! total must match. The real-Hub, no-route-out run is `scripts/wan-down/run.sh`.

use std::sync::Arc;

use arena_core::{HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, Connectivity, ConnectivityState, DeviceAssign,
    DeviceKind, DeviceRegister, ExtendSource, GroupUpsert, Intent, IntentPayload, PauseReason,
    PaymentMethod, PaymentRow, RateUpsert, SessionEnd, SessionExtend, SessionMode, SessionPause,
    SessionResume, SessionSettle, SessionStart, SessionTransfer, WalletTopup,
};
use arena_store::{LeaseRow, Row, RowValue, Store, Table, write_lease};

const T0: i64 = 1_800_000_000_000;
const HUB: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

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
            id: "offline".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

async fn send(w: &WriterHandle, payload: IntentPayload, rev: Option<i64>) -> String {
    let out = w
        .submit_intent(intent(payload, rev))
        .await
        .expect("flow step");
    out.changes[0].row_id.clone()
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

fn row_of(reader: &Store, table: Table, id: &str) -> Row {
    reader
        .rows(table)
        .unwrap()
        .into_iter()
        .find(|r| text(r, "id") == id)
        .unwrap()
}

/// What the flow leaves behind: the change log's shape and the money.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    log: Vec<(String, String, String)>,
    wallet_minor: i64,
    paid_minor: i64,
    billed_minor: i64,
    extend: String,
}

/// A bootstrapped Hub with an open shift whose connectivity probe says `state`.
struct Booted {
    _dir: tempfile::TempDir,
    _status: tokio::sync::watch::Sender<HubStatus>,
    clock: Arc<ManualClock>,
    w: WriterHandle,
    reader: Store,
}

async fn boot(state: ConnectivityState, float_minor: i64) -> Booted {
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
    status.connectivity = Connectivity {
        state,
        checked_at: Some(T0),
    };
    let (status_tx, rx) = status_channel(status);
    let clock = Arc::new(ManualClock::new(T0));
    let w = Writer::spawn(store, clock.clone(), rx).unwrap();
    let reader = Store::open(&path, T0).unwrap();

    send(
        &w,
        IntentPayload::BranchBootstrap(BranchBootstrap {
            name: "Offline Branch".into(),
            branch_code: "OFF01".into(),
            currency: "INR".into(),
            tz: "Asia/Kolkata".into(),
            day_start_min: 360,
            fy_start_month: 4,
            onboarding: None,
        }),
        None,
    )
    .await;
    // Cash settles need an open shift.
    send(
        &w,
        IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor }),
        None,
    )
    .await;
    Booted {
        _dir: dir,
        _status: status_tx,
        clock,
        w,
        reader,
    }
}

async fn run_flow(state: ConnectivityState) -> Outcome {
    let Booted {
        _dir,
        _status: status_tx,
        w,
        reader,
        ..
    } = boot(state, 0).await;
    let hash = arena_core::staff::hash_secret("pw").unwrap();
    let group = send(
        &w,
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
        None,
    )
    .await;
    send(
        &w,
        IntentPayload::RateUpsert(RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: 12_000,
        }),
        None,
    )
    .await;
    let mut devices = Vec::new();
    for n in 0..3 {
        let mut reg = DeviceRegister::new(ClientClass::Paired, format!("PC {n}"), &hash);
        reg.device_kind = DeviceKind::Pc;
        let id = send(&w, IntentPayload::DeviceRegister(reg), None).await;
        send(
            &w,
            IntentPayload::DeviceAssign(DeviceAssign {
                device_id: id.clone(),
                group_id: Some(group.clone()),
            }),
            None,
        )
        .await;
        devices.push(id);
    }

    let form = arena_proto::PlayerForm {
        username: "asha".into(),
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
        &[3; 32],
        &form,
        &ulid::Ulid::generate().to_string(),
        hash.clone(),
    )
    .unwrap();
    let player = send(&w, IntentPayload::PlayerCreate(sealed), None).await;
    send(
        &w,
        IntentPayload::WalletTopup(WalletTopup {
            player_id: player.clone(),
            amount_minor: 5_000_000,
            payments: vec![PaymentRow {
                method: PaymentMethod::Upi,
                amount_minor: 5_000_000,
                reference: Some("1234".into()),
            }],
        }),
        None,
    )
    .await;

    let start = |device: &String, mode, player: Option<&String>| {
        IntentPayload::SessionStart(SessionStart {
            group_with: None,
            pay_from_wallet: false,
            device_id: device.clone(),
            mode,
            player_id: player.cloned(),
            pack_id: None,
            plan_id: None,
        })
    };
    let session = send(&w, start(&devices[0], SessionMode::Postpaid, None), None).await;
    let rev = |reader: &Store, id: &str| int(&row_of(reader, Table::Sessions, id), "rev");
    send(
        &w,
        IntentPayload::SessionPause(SessionPause {
            session_id: session.clone(),
            reason: PauseReason::ALL
                .iter()
                .copied()
                .find(|r| *r != PauseReason::Other)
                .unwrap(),
            note: None,
        }),
        Some(rev(&reader, &session)),
    )
    .await;
    send(
        &w,
        IntentPayload::SessionResume(SessionResume {
            session_id: session.clone(),
        }),
        Some(rev(&reader, &session)),
    )
    .await;
    send(
        &w,
        IntentPayload::SessionEnd(SessionEnd {
            session_id: session.clone(),
        }),
        Some(rev(&reader, &session)),
    )
    .await;
    let order = reader
        .rows(Table::Orders)
        .unwrap()
        .into_iter()
        .find(|o| o["session_id"] == RowValue::Text(session.clone()))
        .unwrap();
    let total = int(&order, "total_minor");
    send(
        &w,
        IntentPayload::SessionSettle(SessionSettle {
            business: None,
            coupon: None,
            discount: None,
            order_id: text(&order, "id"),
            credit_override: None,
            payments: vec![PaymentRow {
                method: PaymentMethod::Cash,
                amount_minor: total,
                reference: None,
            }],
            receipt: None,
        }),
        Some(int(&order, "rev")),
    )
    .await;

    let wallet_session = send(
        &w,
        start(&devices[1], SessionMode::Wallet, Some(&player)),
        None,
    )
    .await;
    let extend = w
        .submit_intent(intent(
            IntentPayload::SessionExtend(SessionExtend {
                session_id: wallet_session.clone(),
                minutes: arena_proto::EXTEND_OPTIONS_MIN[0],
                source: ExtendSource::Wallet,
                pack_id: None,
                credit_override: None,
            }),
            None,
        ))
        .await
        .map_or_else(|e| e.code.as_str().to_owned(), |_| "ok".to_owned());
    send(
        &w,
        IntentPayload::SessionTransfer(SessionTransfer {
            session_id: wallet_session.clone(),
            device_id: devices[2].clone(),
        }),
        Some(rev(&reader, &wallet_session)),
    )
    .await;
    send(
        &w,
        IntentPayload::SessionEnd(SessionEnd {
            session_id: wallet_session.clone(),
        }),
        Some(rev(&reader, &wallet_session)),
    )
    .await;
    drop(status_tx);

    let log = reader
        .changes()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c.tbl.clone(),
                c.op.as_str().to_owned(),
                c.intent_kind.clone(),
            )
        })
        .collect();
    let sum = |table: Table, col: &str| -> i64 {
        reader
            .rows(table)
            .unwrap()
            .iter()
            .map(|r| int(r, col))
            .sum()
    };
    Outcome {
        log,
        wallet_minor: sum(Table::Wallets, "balance_minor"),
        paid_minor: sum(Table::Payments, "amount_minor"),
        billed_minor: sum(Table::Orders, "total_minor"),
        extend,
    }
}

/// What the Epic 5 flow leaves behind: the change log's shape, the numbers issued and the money.
#[derive(Debug, PartialEq, Eq)]
struct Outcome5 {
    log: Vec<(String, String, String)>,
    invoices: Vec<String>,
    credit_notes: Vec<String>,
    stock_by_product: Vec<(String, i64)>,
    billed_minor: i64,
    paid_minor: i64,
    tax_minor: i64,
    drawer_minor: i64,
    z_report: String,
}

/// Products, stock, waste, a coupon, a discount, counter sales, a session order, invoices, a
/// refund with its credit note, a deposit and an expense, a product import and a shift close.
/// Printing is the fake: a `print` receipt only queues an outbox row inside `apply()`.
async fn run_epic_5_flow(state: ConnectivityState) -> Outcome5 {
    let Booted {
        _dir,
        _status,
        clock,
        w,
        reader,
    } = boot(state, 50_000).await;
    let hash = arena_core::staff::hash_secret("pw").unwrap();
    let group = send(
        &w,
        IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }),
        None,
    )
    .await;
    send(
        &w,
        IntentPayload::RateUpsert(RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: 12_000,
        }),
        None,
    )
    .await;
    let mut reg = DeviceRegister::new(ClientClass::Paired, "PC 0", &hash);
    reg.device_kind = DeviceKind::Pc;
    let device = send(&w, IntentPayload::DeviceRegister(reg), None).await;
    send(
        &w,
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: device.clone(),
            group_id: Some(group),
        }),
        None,
    )
    .await;

    let product = |name: &str, price_minor: i64| {
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
        })
    };
    let cola = send(&w, product("Cola", 4_000), None).await;
    let chips = send(&w, product("Chips", 2_500), None).await;
    for id in [&cola, &chips] {
        send(
            &w,
            IntentPayload::StockIn(arena_proto::StockIn {
                product_id: id.clone(),
                qty: 50,
                unit_cost_minor: 1_500,
                supplier: "Wholesale".into(),
                expiry: None,
            }),
            None,
        )
        .await;
    }
    let mut waste = intent(
        IntentPayload::StockWaste(arena_proto::StockWaste {
            product_id: chips.clone(),
            qty: 2,
        }),
        None,
    );
    waste.reason = arena_proto::ReasonCode::WASTE.first().copied();
    w.submit_intent(waste).await.expect("waste");
    send(
        &w,
        IntentPayload::CouponUpsert(arena_proto::CouponUpsert {
            id: None,
            code: "welcome10".into(),
            percent_bp: Some(1_000),
            amount_minor: None,
            valid_from: None,
            valid_to: None,
            usage_limit: None,
            min_bill_minor: 0,
            targets: Vec::new(),
            active: true,
        }),
        None,
    )
    .await;

    let lines = |items: &[(&String, i64)]| -> Vec<arena_proto::PosSaleLine> {
        items
            .iter()
            .map(|(id, qty)| arena_proto::PosSaleLine {
                product_id: (*id).clone(),
                qty: *qty,
            })
            .collect()
    };
    let pay = |method, amount_minor, reference: Option<&str>| {
        vec![PaymentRow {
            method,
            amount_minor,
            reference: reference.map(str::to_owned),
        }]
    };
    let sale = |lines, payments, receipt, coupon: Option<&str>, discount| {
        IntentPayload::PosSale(arena_proto::PosSale {
            kiosk: None,
            lines,
            payments,
            player_id: None,
            credit_override: None,
            stock_override: None,
            receipt,
            business: None,
            coupon: coupon.map(str::to_owned),
            discount,
        })
    };
    // Tax is inclusive at 0% by default, so a bill is its lines less coupon and discount.
    let first = send(
        &w,
        sale(
            lines(&[(&cola, 2), (&chips, 1)]),
            pay(PaymentMethod::Cash, 10_500, None),
            Some(arena_proto::ReceiptChoice::Print),
            None,
            None,
        ),
        None,
    )
    .await;
    let second = send(
        &w,
        sale(
            lines(&[(&cola, 1)]),
            pay(PaymentMethod::Upi, 3_600, Some("1234")),
            None,
            Some("welcome10"),
            None,
        ),
        None,
    )
    .await;
    let third = send(
        &w,
        sale(
            lines(&[(&chips, 3)]),
            pay(PaymentMethod::Cash, 7_000, None),
            None,
            None,
            Some(arena_proto::ManualDiscount {
                percent_bp: None,
                amount_minor: Some(500),
                reason: arena_proto::DiscountReason::StaffMeal,
                note: None,
            }),
        ),
        None,
    )
    .await;

    // A running session takes an order, then ends and settles with it on the bill.
    let session = send(
        &w,
        IntentPayload::SessionStart(SessionStart {
            group_with: None,
            pay_from_wallet: false,
            device_id: device.clone(),
            mode: SessionMode::Postpaid,
            player_id: None,
            pack_id: None,
            plan_id: None,
        }),
        None,
    )
    .await;
    send(
        &w,
        IntentPayload::OrderPlace(arena_proto::OrderPlace {
            counter: None,
            session_id: session.clone(),
            lines: lines(&[(&cola, 1)]),
            stock_override: None,
        }),
        None,
    )
    .await;
    let rev = |id: &str| int(&row_of(&reader, Table::Sessions, id), "rev");
    send(
        &w,
        IntentPayload::SessionEnd(SessionEnd {
            session_id: session.clone(),
        }),
        Some(rev(&session)),
    )
    .await;
    let order = reader
        .rows(Table::Orders)
        .unwrap()
        .into_iter()
        .find(|o| o["session_id"] == RowValue::Text(session.clone()))
        .unwrap();
    send(
        &w,
        IntentPayload::SessionSettle(SessionSettle {
            business: None,
            coupon: None,
            discount: None,
            order_id: text(&order, "id"),
            credit_override: None,
            payments: pay(PaymentMethod::Cash, int(&order, "total_minor"), None),
            receipt: None,
        }),
        Some(int(&order, "rev")),
    )
    .await;

    // The Undo window closes: every sale is numbered, as the invoice task would.
    clock.advance(arena_proto::UNDO_WINDOW_MS + 1);
    for id in [&first, &second, &third, &text(&order, "id")] {
        // The Print sale was numbered inside its settle.
        if matches!(
            row_of(&reader, Table::Orders, id)["invoice_no"],
            RowValue::Text(_)
        ) {
            continue;
        }
        let mut issue = intent(
            IntentPayload::InvoiceIssue(arena_proto::InvoiceIssue {
                order_id: id.clone(),
            }),
            None,
        );
        issue.actor.kind = ActorKind::System;
        w.submit_intent(issue).await.expect("invoice");
    }
    let invoice = text(&row_of(&reader, Table::Orders, &first), "invoice_no");
    let line = reader
        .rows(Table::OrderLines)
        .unwrap()
        .into_iter()
        .find(|l| text(l, "order_id") == first && text(l, "product_id") == cola)
        .unwrap();
    let mut refund = intent(
        IntentPayload::RefundCreate(arena_proto::RefundCreate {
            invoice_no: invoice,
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
    w.submit_intent(refund).await.expect("refund");
    send(
        &w,
        IntentPayload::CashDeposit(arena_proto::CashDeposit {
            amount_minor: 5_000,
            reference: "SLIP-1".into(),
        }),
        None,
    )
    .await;
    send(
        &w,
        IntentPayload::ExpenseCreate(arena_proto::ExpenseCreate {
            category: arena_proto::ExpenseCategory::Supplies,
            amount_minor: 1_000,
            paid_from: arena_proto::ExpensePaidFrom::Drawer,
            receipt_photo: None,
            note: None,
        }),
        None,
    )
    .await;
    send(
        &w,
        IntentPayload::ImportProducts(arena_proto::ImportProducts {
            rows: ["Water", "Juice"]
                .iter()
                .map(|name| arena_proto::ImportProductRow {
                    name: (*name).into(),
                    category: String::new(),
                    price_minor: 2_000,
                    tax_rate_bp: None,
                    hsn: String::new(),
                    barcode: String::new(),
                    track_stock: true,
                    kitchen: false,
                    opening_qty: 10,
                    unit_cost_minor: 800,
                })
                .collect(),
        }),
        None,
    )
    .await;
    let shift = reader
        .rows(Table::Shifts)
        .unwrap()
        .into_iter()
        .find(|s| text(s, "state") == "open")
        .unwrap();
    let expected = reader
        .read_with(|conn| arena_store::expected_cash(conn, &text(&shift, "id")))
        .unwrap();
    send(
        &w,
        IntentPayload::ShiftClose(arena_proto::ShiftClose {
            counted_minor: expected,
            deposit: None,
            reason: None,
            note: None,
        }),
        None,
    )
    .await;

    let log = reader
        .changes()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c.tbl.clone(),
                c.op.as_str().to_owned(),
                c.intent_kind.clone(),
            )
        })
        .collect();
    let mut invoices: Vec<_> = reader
        .rows(Table::Orders)
        .unwrap()
        .iter()
        .filter_map(|o| match &o["invoice_no"] {
            RowValue::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    invoices.sort();
    let mut credit_notes: Vec<_> = reader
        .rows(Table::Refunds)
        .unwrap()
        .iter()
        .map(|r| text(r, "credit_note_no"))
        .collect();
    credit_notes.sort();
    // Product ids are fresh ULIDs per run, so stock is compared by product name.
    let mut stock_by_product: Vec<_> = reader
        .rows(Table::Stock)
        .unwrap()
        .iter()
        .map(|s| {
            let product = reader
                .products()
                .unwrap()
                .into_iter()
                .find(|p| p.product_id == text(s, "product_id"))
                .unwrap();
            (product.name, int(s, "qty"))
        })
        .collect();
    stock_by_product.sort();
    let sum = |table: Table, col: &str| -> i64 {
        reader
            .rows(table)
            .unwrap()
            .iter()
            .map(|r| int(r, col))
            .sum()
    };
    let closed = reader
        .rows(Table::Shifts)
        .unwrap()
        .into_iter()
        .find(|s| text(s, "state") == "closed")
        .unwrap();
    Outcome5 {
        log,
        invoices,
        credit_notes,
        stock_by_product,
        billed_minor: sum(Table::Orders, "total_minor"),
        paid_minor: sum(Table::Payments, "amount_minor"),
        tax_minor: sum(Table::Orders, "tax_minor"),
        drawer_minor: expected,
        z_report: {
            // The shift id is a fresh ULID per run; every figure in the report must match.
            let mut report: serde_json::Value =
                serde_json::from_str(&text(&closed, "z_report")).unwrap();
            report.as_object_mut().unwrap().remove("shift_id");
            report.to_string()
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn epic_5_flows_give_the_same_result_with_the_wan_down() {
    let up = run_epic_5_flow(ConnectivityState::Online).await;
    let down = run_epic_5_flow(ConnectivityState::InternetOffline).await;
    let cloud = run_epic_5_flow(ConnectivityState::CloudUnreachable).await;
    assert_eq!(up.invoices.len(), 4);
    assert_eq!(up.credit_notes.len(), 1);
    assert!(up.billed_minor > 0 && up.paid_minor > 0 && up.drawer_minor > 0);
    assert_eq!(up, down);
    assert_eq!(up, cloud);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn epic_3_flows_give_the_same_result_with_the_wan_down() {
    let up = run_flow(ConnectivityState::Online).await;
    let down = run_flow(ConnectivityState::InternetOffline).await;
    let cloud = run_flow(ConnectivityState::CloudUnreachable).await;
    assert!(up.billed_minor > 0 && up.paid_minor > 0);
    assert_eq!(up, down);
    assert_eq!(up, cloud);
}

/// `arena-core` is the only code that applies a flow; it must stay unable to open a socket.
#[test]
fn arena_core_source_opens_no_connection() {
    const BANNED: &[&str] = &[
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "std::net::Tcp",
        "tokio::net",
        "reqwest",
        "hyper",
        "ureq",
        "curl",
        "http://",
        "https://",
    ];
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.components().any(|c| c.as_os_str() == "tests")
                && !path.ends_with("tests.rs")
            {
                let body = std::fs::read_to_string(&path).unwrap();
                for word in BANNED {
                    let hits = body
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("//"))
                        .filter(|l| l.contains(word))
                        .count();
                    assert_eq!(hits, 0, "{} uses `{word}`", path.display());
                }
            }
        }
    }
}

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

async fn run_flow(state: ConnectivityState) -> Outcome {
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
    let w = Writer::spawn(store, Arc::new(ManualClock::new(T0)), rx).unwrap();
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
        IntentPayload::ShiftOpen(arena_proto::ShiftOpen { float_minor: 0 }),
        None,
    )
    .await;
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
            order_id: text(&order, "id"),
            credit_override: None,
            payments: vec![PaymentRow {
                method: PaymentMethod::Cash,
                amount_minor: total,
                reference: None,
            }],
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

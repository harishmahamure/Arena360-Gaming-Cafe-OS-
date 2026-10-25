use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_api::pairing::Pairing;
use arena_core::rbac::{BUILTIN_ROLES, ROLE_COUNTER, ROLE_MANAGER};
use arena_core::staff::hash_secret;
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, StaffDirectory, Writer,
    status_channel,
};
use arena_kiosk_core::{HubClient, PairError, pair};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, ErrorCode, Hello, Intent, IntentPayload,
    PairRequest, PairResponse, StaffUpsert, Versions,
};
use arena_store::{LeaseRow, Store, write_lease};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};

const T0: i64 = 1_800_000_000_000;
const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

struct TestHub {
    _dir: tempfile::TempDir,
    path: PathBuf,
    lan: SocketAddr,
    web: SocketAddr,
    fingerprint: String,
    clock: Arc<ManualClock>,
    pairing: Arc<Pairing>,
    manager_id: String,
    _status: watch::Sender<HubStatus>,
    _stop: oneshot::Sender<()>,
}

fn intent(actor: Actor, payload: IntentPayload) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev: None,
        actor,
        elevated_by: None,
        reason: None,
        payload,
    }
}

async fn start() -> TestHub {
    start_with_lease(i64::MAX).await
}

async fn start_with_lease(expires_at: i64) -> TestHub {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    let lease = LeaseRow {
        counter: 1,
        hub_instance_id: HUB_ID.into(),
        provisional: true,
        renewed_at: T0,
        expires_at,
    };
    store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease));
    let (status_tx, status_rx) = status_channel(initial);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), status_rx.clone()).unwrap();
    let boot = writer
        .submit_intent(intent(
            Actor {
                kind: ActorKind::System,
                id: "test".into(),
            },
            IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Koramangala".into(),
                branch_code: "KOR01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
            }),
        ))
        .await
        .unwrap();
    let roles: Vec<String> = boot
        .changes
        .iter()
        .filter(|c| c.tbl == "roles")
        .map(|c| c.row_id.clone())
        .collect();
    let role = |name: &str| roles[BUILTIN_ROLES.iter().position(|r| *r == name).unwrap()].clone();
    let mut ids = Vec::new();
    for (login, role_name, pin) in [
        ("manager", ROLE_MANAGER, "2222"),
        ("counter", ROLE_COUNTER, "3333"),
    ] {
        let applied = writer
            .submit_intent(intent(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(StaffUpsert {
                    staff_id: None,
                    login: login.into(),
                    display_name: login.into(),
                    role_id: role(role_name),
                    password_hash: Some(hash_secret("pw").unwrap()),
                    pin_hash: Some(hash_secret(pin).unwrap()),
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
            ))
            .await
            .unwrap();
        ids.push(applied.changes[0].row_id.clone());
    }
    let cert = HubCert::generate(None).unwrap();
    status_tx.send_modify(|s| s.tls_fingerprint = Some(cert.fingerprint.clone()));
    let pairing = Arc::new(
        Pairing::new(clock.clone())
            .unwrap()
            .with_tick(Duration::from_millis(10)),
    );
    pairing.set_owner_check(|_| true);
    let hub = Arc::new(
        LanHub::new(
            Some(writer),
            status_rx,
            clock.clone(),
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            "0.1.0",
        )
        .with_staff(Arc::new(StaffDirectory::open(&path).unwrap()))
        .with_pairing(Arc::clone(&pairing)),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let (lan, web) = (listeners.lan_addr().unwrap(), listeners.web_addr().unwrap());
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));
    TestHub {
        _dir: dir,
        path,
        lan,
        web,
        fingerprint: cert.fingerprint,
        clock,
        pairing,
        manager_id: ids[0].clone(),
        _status: status_tx,
        _stop: stop_tx,
    }
}

fn by_code(code: &str, pc_name: &str) -> PairRequest {
    PairRequest {
        code: Some(code.into()),
        pin: None,
        pc_name: pc_name.into(),
        version: "0.1.0".into(),
    }
}

fn by_pin(pin: &str, pc_name: &str) -> PairRequest {
    PairRequest {
        code: None,
        pin: Some(pin.into()),
        pc_name: pc_name.into(),
        version: "0.1.0".into(),
    }
}

impl TestHub {
    async fn pair(&self, req: &PairRequest) -> Result<PairResponse, PairError> {
        pair(
            "127.0.0.1",
            self.web.port(),
            &self.fingerprint,
            req,
            Duration::from_secs(20),
        )
        .await
    }

    fn store(&self) -> Store {
        Store::open_reader(&self.path).unwrap()
    }
}

fn hub_code(r: Result<PairResponse, PairError>) -> (ErrorCode, Option<u32>) {
    match r {
        Err(PairError::Hub {
            error,
            retry_after_s,
        }) => (error.code, retry_after_s),
        other => panic!("expected a hub refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_valid_code_pairs_once_returns_the_token_once_and_registers_an_unassigned_pc() {
    let hub = start().await;
    let mut changes = hub.pairing.subscribe();
    let code = hub.pairing.issue("panel").unwrap().code;
    let ok = hub.pair(&by_code(&code, "PC-07")).await.unwrap();
    assert_eq!(ok.hub_fingerprint, hub.fingerprint);
    assert!(ok.token.starts_with(&format!("{}.", ok.device_id)));

    let store = hub.store();
    let device = store.device(&ok.device_id).unwrap().unwrap();
    assert_eq!(
        (device.class.as_str(), device.label.as_str()),
        ("paired", "PC-07")
    );
    assert!(device.token_hash.starts_with("$argon2id$"));
    assert!(!device.token_hash.contains(&ok.token));
    let meta = store.device_meta(&ok.device_id).unwrap().unwrap();
    assert_eq!(
        (meta.kind.as_str(), meta.label.as_str(), meta.group_id),
        ("pc", "PC-07", None)
    );
    let change = store
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.tbl == "devices_meta" && c.row_id == ok.device_id)
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("system", arena_proto::PAIR_SYSTEM_ACTOR)
    );
    let raw = String::from_utf8_lossy(&std::fs::read(&hub.path).unwrap()).into_owned();
    assert!(!raw.contains(ok.token.split_once('.').unwrap().1));

    assert_eq!(
        hub_code(hub.pair(&by_code(&code, "PC-08")).await),
        (ErrorCode::PairCodeInvalid, None)
    );
    let fresh = changes.try_recv().unwrap();
    assert_eq!(&*fresh.owner, "panel");
    assert_ne!(fresh.code.unwrap().code, code);

    let client = HubClient::connect(
        &format!("wss://127.0.0.1:{}/", hub.lan.port()),
        &ok.hub_fingerprint,
        Hello {
            credential: ok.token.clone(),
            class: ClientClass::Paired,
            versions: Versions {
                app: "0.1.0".into(),
            },
            capabilities: vec![],
        },
    )
    .await
    .unwrap();
    assert_eq!(
        client.hello_ok().device_id.as_deref(),
        Some(ok.device_id.as_str())
    );
}

#[tokio::test]
async fn an_expired_or_unknown_code_is_pair_code_invalid() {
    let hub = start().await;
    assert_eq!(
        hub_code(hub.pair(&by_code("123456", "PC-01")).await).0,
        ErrorCode::PairCodeInvalid
    );
    let code = hub.pairing.issue("panel").unwrap().code;
    hub.clock.advance(arena_proto::PAIR_CODE_TTL_MS);
    assert_eq!(
        hub_code(hub.pair(&by_code(&code, "PC-01")).await).0,
        ErrorCode::PairCodeInvalid
    );
    assert!(hub.store().devices().unwrap().is_empty());
}

#[tokio::test]
async fn two_kiosks_racing_with_one_code_pair_exactly_once() {
    let hub = Arc::new(start().await);
    let code = hub.pairing.issue("panel").unwrap().code;
    let tasks: Vec<_> = (0..4)
        .map(|i| {
            let (hub, code) = (Arc::clone(&hub), code.clone());
            tokio::spawn(async move { hub.pair(&by_code(&code, &format!("PC-{i}"))).await })
        })
        .collect();
    let mut ok = 0;
    for t in tasks {
        match t.await.unwrap() {
            Ok(_) => ok += 1,
            Err(e) => assert_eq!(hub_code(Err(e)).0, ErrorCode::PairCodeInvalid),
        }
    }
    assert_eq!(ok, 1);
    assert_eq!(hub.store().devices().unwrap().len(), 1);
}

#[tokio::test]
async fn a_manager_pin_pairs_with_elevated_by_and_five_wrong_pins_pause_only_the_pin() {
    let hub = start().await;
    let ok = hub.pair(&by_pin("2222", "PC-PIN")).await.unwrap();
    let change = hub
        .store()
        .changes()
        .unwrap()
        .into_iter()
        .find(|c| c.tbl == "devices_meta" && c.row_id == ok.device_id)
        .unwrap();
    assert_eq!(change.elevated_by_kind.as_deref(), Some("pin"));
    assert_eq!(
        change.elevated_by_id.as_deref(),
        Some(hub.manager_id.as_str())
    );
    assert_eq!(change.actor_kind, "system");

    assert_eq!(
        hub_code(hub.pair(&by_pin("3333", "PC-X")).await),
        (ErrorCode::PinRejected, None)
    );
    for _ in 0..3 {
        assert_eq!(
            hub_code(hub.pair(&by_pin("9999", "PC-X")).await).0,
            ErrorCode::PinRejected
        );
    }
    assert_eq!(
        hub_code(hub.pair(&by_pin("9999", "PC-X")).await),
        (ErrorCode::PairPinPaused, Some(60))
    );
    assert_eq!(
        hub_code(hub.pair(&by_pin("2222", "PC-X")).await),
        (ErrorCode::PairPinPaused, Some(60))
    );
    let code = hub.pairing.issue("panel").unwrap().code;
    hub.pair(&by_code(&code, "PC-CODE")).await.unwrap();
    hub.clock.advance(60_000);
    hub.pair(&by_pin("2222", "PC-AGAIN")).await.unwrap();
}

#[tokio::test]
async fn pairing_is_tls_only_and_a_bad_body_pairs_nothing() {
    let hub = start().await;
    let code = hub.pairing.issue("panel").unwrap().code;
    let body = serde_json::to_string(&by_code(&code, "PC-01")).unwrap();
    let mut tcp = TcpStream::connect(hub.web).await.unwrap();
    tcp.write_all(
        format!(
            "POST /pair HTTP/1.1\r\nHost: h\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut answer = String::new();
    tcp.read_to_string(&mut answer).await.unwrap();
    assert!(answer.starts_with("HTTP/1.1 404"), "{answer}");

    let both = PairRequest {
        pin: Some("2222".into()),
        ..by_code(&code, "PC-01")
    };
    assert_eq!(hub_code(hub.pair(&both).await).0, ErrorCode::BadFrame);
    let bad_name = by_code(&code, " ");
    assert_eq!(
        hub_code(hub.pair(&bad_name).await).0,
        ErrorCode::ValidationFailed
    );
    assert!(hub.store().devices().unwrap().is_empty());
    hub.pair(&by_code(&code, "PC-01")).await.unwrap();
}

#[tokio::test]
async fn a_kiosk_pinning_another_fingerprint_is_refused_before_it_sends_anything() {
    let hub = start().await;
    let code = hub.pairing.issue("panel").unwrap().code;
    let wrong = "ab".repeat(32);
    let err = pair(
        "127.0.0.1",
        hub.web.port(),
        &wrong,
        &by_code(&code, "PC-01"),
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    match err {
        PairError::PinMismatch {
            expected,
            presented,
        } => {
            assert_eq!(expected, wrong);
            assert_eq!(presented, hub.fingerprint);
        }
        other => panic!("{other:?}"),
    }
    assert!(hub.store().devices().unwrap().is_empty());
    hub.pair(&by_code(&code, "PC-01")).await.unwrap();
}

#[tokio::test]
async fn a_pairing_the_writer_refuses_keeps_the_code_valid() {
    let hub = start_with_lease(T0 + 60_000).await;
    let code = hub.pairing.issue("panel").unwrap().code;
    hub.clock.advance(60_000);
    assert_eq!(
        hub_code(hub.pair(&by_code(&code, "PC-01")).await).0,
        ErrorCode::NotPrimary
    );
    assert!(hub.store().devices().unwrap().is_empty());
    assert!(hub.pairing.take(&code).is_ok());
}

#[tokio::test]
async fn ten_wrong_codes_retire_the_live_code() {
    let hub = start().await;
    let code = hub.pairing.issue("panel").unwrap().code;
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for _ in 0..arena_api::pairing::MAX_WRONG_CODES {
        assert_eq!(
            hub_code(hub.pair(&by_code(wrong, "PC-01")).await).0,
            ErrorCode::PairCodeInvalid
        );
    }
    assert_eq!(
        hub_code(hub.pair(&by_code(&code, "PC-01")).await).0,
        ErrorCode::PairCodeInvalid
    );
    assert!(hub.store().devices().unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_lengths_and_transfer_encoding_are_refused() {
    let hub = start().await;
    let code = hub.pairing.issue("panel").unwrap().code;
    let body = serde_json::to_string(&by_code(&code, "PC-01")).unwrap();
    for extra in [
        format!("Content-Length: {}\r\n", body.len()),
        format!("Content-Length: {}\r\n", body.len() + 1),
        "Transfer-Encoding: chunked\r\n".to_owned(),
    ] {
        let head = format!(
            "POST /pair HTTP/1.1\r\nHost: h\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
            body.len()
        );
        let answer = raw_tls(&hub, head.as_bytes()).await;
        assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
        assert!(answer.contains("BAD_FRAME"), "{answer}");
    }
    assert!(hub.store().devices().unwrap().is_empty());
    hub.pair(&by_code(&code, "PC-01")).await.unwrap();
}

async fn raw_tls(hub: &TestHub, request: &[u8]) -> String {
    let config = arena_kiosk_core::pinned_config(&hub.fingerprint).unwrap();
    let tcp = TcpStream::connect(hub.web).await.unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(config)
        .connect(
            rustls::pki_types::ServerName::IpAddress(std::net::IpAddr::from([127, 0, 0, 1]).into()),
            tcp,
        )
        .await
        .unwrap();
    tls.write_all(request).await.unwrap();
    let mut out = Vec::new();
    let _ = tls.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

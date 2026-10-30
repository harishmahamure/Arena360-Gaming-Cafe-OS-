//! Remote kiosks (Story 8.41): the tunnel listener pairs a device with a code and admits it as the
//! paired class with its device token, and nothing else.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_api::pairing::Pairing;
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel,
};
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, dev_sign_any};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRevoke, ErrorCode, Frame, FrameBody,
    Hello, HelloOk, HubEvent, Intent, IntentPayload, LiveClaims, LiveFloor, LiveTileState,
    PairResponse, Versions, parse_frame,
};
use arena_store::{LeaseRow, Store, write_lease};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

const T0: i64 = 1_800_000_000_000;
const T0_S: i64 = T0 / 1000;
const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

struct TestHub {
    _dir: tempfile::TempDir,
    branch_id: String,
    tunnel: SocketAddr,
    pairing: Arc<Pairing>,
    _status: watch::Sender<HubStatus>,
    writer: Option<WriterHandle>,
    _stop: oneshot::Sender<()>,
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "test".into(),
    }
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
        reason: payload.kind().reasons().map(|r| r[0]),
        payload,
    }
}

async fn start(tunnel_flag: bool, primary: bool) -> TestHub {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let mut store = Store::open(&path, T0).unwrap();
    let lease = LeaseRow {
        counter: 1,
        hub_instance_id: HUB_ID.into(),
        provisional: true,
        renewed_at: T0,
        expires_at: i64::MAX,
    };
    store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
    initial.role = Role::Primary;
    initial.lease = Some(LeaseInfo::from(&lease));
    let (status_tx, status_rx) = status_channel(initial);
    let clock = Arc::new(ManualClock::new(T0));
    let writer = Writer::spawn(store, clock.clone(), status_rx.clone()).unwrap();
    writer
        .submit_intent(intent(
            system(),
            IntentPayload::BranchBootstrap(BranchBootstrap {
                name: "Dev Branch".into(),
                branch_code: "DEV01".into(),
                currency: "INR".into(),
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                onboarding: None,
            }),
        ))
        .await
        .unwrap();
    let flags = if tunnel_flag { "tunnel" } else { "" };
    let sql = format!("UPDATE settings SET value = '{flags}' WHERE key = 'licence.flags'");
    writer
        .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
        .await
        .unwrap();
    let branch_id = Store::open_reader(&path)
        .unwrap()
        .branch_summary()
        .unwrap()
        .unwrap()
        .branch_id;

    let live_writer = if primary {
        Some(writer)
    } else {
        writer.close().await;
        status_tx.send_modify(|s| s.role = Role::Follower);
        None
    };
    let cert = HubCert::generate(None).unwrap();
    status_tx.send_modify(|s| s.tls_fingerprint = Some(cert.fingerprint.clone()));
    let pairing = Arc::new(Pairing::new(clock.clone()).unwrap());
    pairing.set_owner_check(|_| true);
    let hub = LanHub::new(
        live_writer.clone(),
        status_rx,
        clock,
        Arc::new(DeviceDirectory::open(&path).unwrap()),
        cert.server_config().unwrap(),
        "0.1.0",
    )
    .with_live_view(DEV_LICENCE_VERIFY_KEYS.to_vec())
    .with_pairing(Arc::clone(&pairing));
    let mut listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    listeners.bind_tunnel(loopback).await.unwrap();
    let tunnel = listeners.tunnel_addr().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, Arc::new(hub), async {
        let _ = stop_rx.await;
    }));
    TestHub {
        _dir: dir,
        branch_id,
        tunnel,
        pairing,
        writer: live_writer,
        _status: status_tx,
        _stop: stop_tx,
    }
}

impl TestHub {
    /// `POST /pair` through the tunnel, as the connector forwards it.
    async fn pair_via_tunnel(&self, body: &str, extra: &str) -> String {
        let mut tcp = TcpStream::connect(self.tunnel).await.unwrap();
        tcp.write_all(
            format!(
                "POST /pair HTTP/1.1\r\nHost: h\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut answer = String::new();
        tcp.read_to_string(&mut answer).await.unwrap();
        answer
    }

    /// Pairs a device through the tunnel with a fresh code.
    async fn pair(&self, name: &str) -> PairResponse {
        let code = self.pairing.issue("panel").unwrap().code;
        let body = serde_json::json!({"code": code, "pc_name": name, "version": "0.1.0"});
        let answer = self.pair_via_tunnel(&body.to_string(), "").await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        serde_json::from_str(answer.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    }

    async fn connect(&self) -> WebSocketStream<impl AsyncRead + AsyncWrite + Unpin> {
        let tcp = TcpStream::connect(self.tunnel).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/", self.tunnel.port());
        tokio_tungstenite::client_async(url, tcp).await.unwrap().0
    }

    async fn set_flags(&self, flags: &str) {
        let sql = format!("UPDATE settings SET value = '{flags}' WHERE key = 'licence.flags'");
        self.writer
            .as_ref()
            .unwrap()
            .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
            .await
            .unwrap();
    }

    fn store(&self) -> Store {
        Store::open_reader(self._dir.path().join("branch.db")).unwrap()
    }
}

fn hello(class: ClientClass, token: &str) -> Frame {
    Frame::new(
        "h",
        FrameBody::Hello(Hello {
            credential: token.into(),
            class,
            versions: Versions {
                app: "0.1.0".into(),
            },
            capabilities: vec![],
        }),
    )
}

async fn send<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, frame: &Frame) {
    ws.send(Message::text(frame.to_text())).await.unwrap();
}

async fn recv<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>) -> Option<Frame> {
    loop {
        match tokio::time::timeout(Duration::from_secs(60), ws.next())
            .await
            .expect("a frame in time")
        {
            Some(Ok(Message::Text(t))) => return Some(parse_frame(t.as_str()).unwrap()),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return None,
            Some(Ok(_)) => {}
        }
    }
}

fn code_of(frame: &Frame) -> ErrorCode {
    let FrameBody::Error(e) = &frame.body else {
        panic!("expected an error, got {frame:?}")
    };
    e.error.code
}

async fn refused(hub: &TestHub, class: ClientClass, token: &str, code: ErrorCode) {
    let mut ws = hub.connect().await;
    send(&mut ws, &hello(class, token)).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(code_of(&frame), code);
    assert!(recv(&mut ws).await.is_none(), "the connection closes");
}

/// The next `live.floor` event.
async fn next_floor<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>) -> LiveFloor {
    loop {
        let frame = recv(ws).await.expect("the session stays open");
        if let FrameBody::Event(e) = frame.body
            && let HubEvent::LiveFloor(f) = e.event
        {
            return *f;
        }
    }
}

async fn admitted<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
    class: ClientClass,
    token: &str,
) -> HelloOk {
    send(ws, &hello(class, token)).await;
    let ok = recv(ws).await.unwrap();
    let FrameBody::HelloOk(ok) = ok.body else {
        panic!("expected hello.ok, got {ok:?}")
    };
    ok
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_device_pairs_through_the_tunnel_with_a_code_and_connects_as_paired() {
    let hub = start(true, true).await;
    let paired = hub.pair("PC-REMOTE").await;
    assert!(paired.token.starts_with(&paired.device_id));

    let mut ws = hub.connect().await;
    let ok = admitted(&mut ws, ClientClass::Paired, &paired.token).await;
    assert_eq!(ok.class, ClientClass::Paired);
    assert_eq!(ok.device_id.as_deref(), Some(paired.device_id.as_str()));

    // It is an ordinary registered device, so it counts toward the device cap.
    let devices = hub.store().devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].class, "paired");

    // A live viewer sees it on the floor, labelled Remote.
    let viewer_token = {
        let mut c = LiveClaims::new("tenant", &hub.branch_id, T0_S);
        c.exp = T0_S + 300;
        dev_sign_any(&c).unwrap()
    };
    let mut viewer = hub.connect().await;
    admitted(&mut viewer, ClientClass::LiveView, &viewer_token).await;
    let floor = next_floor(&mut viewer).await;
    let tile = floor
        .tiles
        .iter()
        .find(|t| t.device_id == paired.device_id)
        .expect("the remote device is on the floor");
    assert!(tile.remote);
    assert_eq!(tile.state, LiveTileState::Online);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_tunnel_refuses_a_bad_token_and_every_browser_class() {
    let hub = start(true, true).await;
    let paired = hub.pair("PC-REMOTE").await;
    refused(&hub, ClientClass::Paired, "nope", ErrorCode::TokenInvalid).await;
    refused(
        &hub,
        ClientClass::Paired,
        &format!("{}.wrong-secret", paired.device_id),
        ErrorCode::TokenInvalid,
    )
    .await;
    refused(
        &hub,
        ClientClass::Kds,
        &paired.token,
        ErrorCode::TokenInvalid,
    )
    .await;
    refused(&hub, ClientClass::Phone, "", ErrorCode::TokenInvalid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoked_remote_device_is_refused_token_revoked() {
    let hub = start(true, true).await;
    let paired = hub.pair("PC-REMOTE").await;
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::DeviceRevoke(DeviceRevoke {
                device_id: paired.device_id.clone(),
            }),
        ))
        .await
        .unwrap();
    refused(
        &hub,
        ClientClass::Paired,
        &paired.token,
        ErrorCode::TokenRevoked,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_pairing_needs_the_tunnel_flag_a_code_and_a_primary_hub() {
    // No `tunnel` flag: no pairing.
    let hub = start(false, true).await;
    let code = hub.pairing.issue("panel").unwrap().code;
    let body = serde_json::json!({"code": code, "pc_name": "PC-1", "version": "0.1.0"});
    let answer = hub.pair_via_tunnel(&body.to_string(), "").await;
    assert!(answer.contains("FEATURE_NOT_ENTITLED"), "{answer}");
    assert!(hub.store().devices().unwrap().is_empty());

    // A staff PIN is not a remote pairing method.
    let hub = start(true, true).await;
    let pin = serde_json::json!({"pin": "1111", "pc_name": "PC-1", "version": "0.1.0"});
    let answer = hub.pair_via_tunnel(&pin.to_string(), "").await;
    assert!(answer.contains("FORBIDDEN"), "{answer}");
    assert!(hub.store().devices().unwrap().is_empty());

    // A wrong code pairs nothing; a right one does.
    let wrong = serde_json::json!({"code": "000000", "pc_name": "PC-2", "version": "0.1.0"});
    let answer = hub.pair_via_tunnel(&wrong.to_string(), "").await;
    assert!(!answer.starts_with("HTTP/1.1 200"), "{answer}");
    hub.pair("PC-1").await;
    assert_eq!(hub.store().devices().unwrap().len(), 1);

    // A follower cannot pair anybody.
    let follower = start(true, false).await;
    let code = follower.pairing.issue("panel").unwrap().code;
    let body = serde_json::json!({"code": code, "pc_name": "PC-1", "version": "0.1.0"});
    let answer = follower.pair_via_tunnel(&body.to_string(), "").await;
    assert!(answer.contains("NOT_PRIMARY"), "{answer}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairing_attempts_are_limited_per_client_address_not_per_connector() {
    let hub = start(true, true).await;
    let wrong = serde_json::json!({"code": "000000", "pc_name": "PC-2", "version": "0.1.0"});
    let mut limited = false;
    for _ in 0..30 {
        let answer = hub
            .pair_via_tunnel(&wrong.to_string(), "CF-Connecting-IP: 203.0.113.9\r\n")
            .await;
        if answer.starts_with("HTTP/1.1 429") {
            limited = true;
            break;
        }
    }
    assert!(limited, "a noisy client address is rate limited");
    // Another client address is unaffected.
    let code = hub.pairing.issue("panel").unwrap().code;
    let body = serde_json::json!({"code": code, "pc_name": "PC-3", "version": "0.1.0"});
    let answer = hub
        .pair_via_tunnel(&body.to_string(), "CF-Connecting-IP: 203.0.113.10\r\n")
        .await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_the_tunnel_flag_drops_remote_devices_but_ends_no_session() {
    let hub = start(true, true).await;
    let paired = hub.pair("PC-REMOTE").await;
    let mut ws = hub.connect().await;
    admitted(&mut ws, ClientClass::Paired, &paired.token).await;
    hub.set_flags("").await;
    // The next keepalive check (every 15 s) notices and the Hub lets go of the connection.
    tokio::time::timeout(Duration::from_secs(40), async {
        while recv(&mut ws).await.is_some() {}
    })
    .await
    .expect("the remote connection is dropped");
    // It cannot come back in while the flag is off.
    refused(
        &hub,
        ClientClass::Paired,
        &paired.token,
        ErrorCode::FeatureNotEntitled,
    )
    .await;
    // The Hub did not end any session: it wrote none.
    assert!(hub.store().live_sessions().unwrap().is_empty());
}

//! The LAN protocol server, one test per row of the Story 1.9 matrix (the client-side pinning
//! and `HUB_SLOW` rows also live in `arena-kiosk-core/tests/client.rs`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel,
};
use arena_kiosk_core::{ClientError, HubClient, connect_ws};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRegister, DeviceRevoke, ErrorCode, Frame,
    FrameBody, Hello, HubEvent, Intent, IntentPayload, IntentSubmit, SettingsSet, Versions,
    parse_frame,
};
use arena_store::{LeaseRow, Store, write_lease};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

const T0: i64 = 1_800_000_000_000;
const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

// ---- log capture (for the FORBIDDEN warn line) ----

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn logs() -> &'static Captured {
    static LOGS: OnceLock<Captured> = OnceLock::new();
    LOGS.get_or_init(|| {
        let captured = Captured::default();
        let writer = captured.clone();
        tracing_subscriber::fmt()
            .json()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::INFO)
            .init();
        captured
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().0.lock().unwrap()).into_owned()
}

// ---- a hub on loopback ----

struct TestHub {
    _dir: tempfile::TempDir,
    path: PathBuf,
    lan: SocketAddr,
    web: SocketAddr,
    fingerprint: String,
    paired_id: String,
    paired_token: String,
    kds_token: String,
    writer: Option<WriterHandle>,
    _status: watch::Sender<HubStatus>,
    _stop: oneshot::Sender<()>,
}

fn intent(actor: Actor, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev,
        actor,
        elevated_by: None,
        payload,
    }
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "test".into(),
    }
}

async fn register(w: &WriterHandle, class: ClientClass, label: &str) -> (String, String) {
    let secret = new_token_secret().unwrap();
    let applied = w
        .submit_intent(intent(
            system(),
            IntentPayload::DeviceRegister(DeviceRegister {
                class,
                label: label.into(),
                token_hash: hash_token_secret(&secret).unwrap(),
            }),
            None,
        ))
        .await
        .unwrap();
    let id = applied.changes[0].row_id.clone();
    let token = format_token(&id, &secret);
    (id, token)
}

/// A seeded hub (one paired and one kds device) serving the LAN on loopback. `version` is the
/// hub's own version; a follower serves without a writer.
async fn start(version: &str, primary: bool) -> TestHub {
    logs();
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
            }),
            None,
        ))
        .await
        .unwrap();
    let (paired_id, paired_token) = register(&writer, ClientClass::Paired, "Dev Kiosk").await;
    let (_, kds_token) = register(&writer, ClientClass::Kds, "Pass").await;
    let writer = if primary {
        Some(writer)
    } else {
        writer.close().await;
        status_tx.send_modify(|s| s.role = Role::Follower);
        None
    };

    let cert = HubCert::generate(None).unwrap();
    let hub = Arc::new(LanHub::new(
        writer.clone(),
        status_rx,
        clock,
        Arc::new(DeviceDirectory::open(&path).unwrap()),
        cert.server_config().unwrap(),
        version,
    ));
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
        paired_id,
        paired_token,
        kds_token,
        writer,
        _status: status_tx,
        _stop: stop_tx,
    }
}

impl TestHub {
    fn url(&self) -> String {
        format!("wss://127.0.0.1:{}/", self.lan.port())
    }

    fn web_tls_url(&self) -> String {
        format!("wss://127.0.0.1:{}/", self.web.port())
    }

    fn hello(&self, class: ClientClass, credential: &str, app: &str) -> Hello {
        Hello {
            credential: credential.into(),
            class,
            versions: Versions { app: app.into() },
            capabilities: vec![],
        }
    }

    fn paired(&self) -> Hello {
        self.hello(ClientClass::Paired, &self.paired_token, "0.1.0")
    }

    async fn tls(&self) -> WebSocketStream<impl AsyncRead + AsyncWrite + Unpin> {
        connect_ws(&self.url(), &self.fingerprint).await.unwrap()
    }

    async fn plain(&self) -> WebSocketStream<impl AsyncRead + AsyncWrite + Unpin> {
        let tcp = TcpStream::connect(self.web).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/ws", self.web.port());
        tokio_tungstenite::client_async(url, tcp).await.unwrap().0
    }

    fn device_version(&self) -> Option<String> {
        Store::open_reader(&self.path)
            .unwrap()
            .device(&self.paired_id)
            .unwrap()
            .unwrap()
            .reported_version
    }

    fn change_count(&self) -> usize {
        Store::open_reader(&self.path)
            .unwrap()
            .changes()
            .unwrap()
            .len()
    }
}

// ---- raw frames ----

async fn send<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, text: &str) {
    ws.send(Message::text(text.to_owned())).await.unwrap();
}

async fn send_frame<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, f: &Frame) {
    send(ws, &f.to_text()).await;
}

/// The next frame, events included, or `None` when the connection closed.
async fn recv_any<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>) -> Option<Frame> {
    let deadline = Duration::from_secs(10);
    loop {
        match tokio::time::timeout(deadline, ws.next())
            .await
            .expect("a frame in time")
        {
            Some(Ok(Message::Text(t))) => return Some(parse_frame(t.as_str()).unwrap()),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return None,
            Some(Ok(_)) => {}
        }
    }
}

/// The next frame that is not a pushed event (other sessions' applies, such as the version a
/// paired hello records, are pushed to every session), or `None` when the connection closed.
async fn recv<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>) -> Option<Frame> {
    loop {
        let frame = recv_any(ws).await?;
        if !matches!(frame.body, FrameBody::Event(_)) {
            return Some(frame);
        }
    }
}

fn error_of(frame: &Frame) -> (&str, ErrorCode, &str) {
    let FrameBody::Error(e) = &frame.body else {
        panic!("expected an error frame, got {frame:?}")
    };
    (frame.rid.as_str(), e.error.code, e.error.message.as_str())
}

async fn expect_error_then_close<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
    code: ErrorCode,
) -> String {
    let frame = recv(ws).await.expect("an error frame before the close");
    let (_, got, message) = error_of(&frame);
    assert_eq!(got, code, "{message}");
    let message = message.to_owned();
    assert!(recv(ws).await.is_none(), "the connection closes");
    message
}

async fn hello_ok<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, hello: Hello) {
    send_frame(ws, &Frame::new("h", FrameBody::Hello(hello))).await;
    let frame = recv(ws).await.unwrap();
    assert!(
        matches!(frame.body, FrameBody::HelloOk(_)),
        "expected hello.ok, got {frame:?}"
    );
}

async fn ping<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, rid: &str) {
    send(ws, &format!(r#"{{"v":1,"kind":"ping","rid":"{rid}"}}"#)).await;
    let frame = recv(ws).await.expect("the session stays open");
    assert_eq!(frame, Frame::new(rid, FrameBody::Pong));
}

fn submit_text(rid: &str, payload: IntentPayload) -> String {
    Frame::new(
        rid,
        FrameBody::IntentSubmit(IntentSubmit {
            intent: intent(system(), payload, None),
        }),
    )
    .to_text()
}

fn revoke_payload(id: &str) -> IntentPayload {
    IntentPayload::DeviceRevoke(DeviceRevoke {
        device_id: id.into(),
    })
}

// ---- the matrix ----

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_hello_records_the_version_and_pushes_applies() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let ok = client.hello_ok();
    assert_eq!(ok.device_id.as_deref(), Some(hub.paired_id.as_str()));
    assert_eq!((ok.class, ok.primary), (ClientClass::Paired, true));
    assert_eq!(ok.versions.app, "0.1.0");
    let mut events = client.events().unwrap();

    // The version is recorded through `device.version_reported`, itself pushed as an event.
    let first = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .unwrap()
        .unwrap();
    let HubEvent::Applied(s) = first;
    assert_eq!(s.changes[0].tbl, "devices");
    assert_eq!(s.changes[0].row_id, hub.paired_id);
    assert_eq!(hub.device_version().as_deref(), Some("0.1.0"));
    let store = Store::open_reader(&hub.path).unwrap();
    let last = store.changes().unwrap().pop().unwrap();
    assert_eq!(last.intent_kind, "device.version_reported");
    assert_eq!(
        (last.actor_kind.as_str(), last.actor_id.as_str()),
        ("system", "hub-lan")
    );

    // A later apply by anyone is pushed; the client never polls.
    let applied = hub
        .writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            system(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "receipt.footer".into(),
                value: "thanks".into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    let pushed = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .unwrap()
        .unwrap();
    let HubEvent::Applied(s) = pushed;
    assert_eq!(s.intent_id, applied.intent_id);
    assert_eq!(s.changes[0].tbl, "settings");

    // Reconnecting with the same version records nothing new.
    let before = hub.change_count();
    let again = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    again
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(hub.change_count(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_or_unknown_tokens_are_token_invalid_and_closed() {
    let hub = start("0.1.0", true).await;
    let unknown = format_token(&ulid::Ulid::generate().to_string(), &"a".repeat(64));
    let wrong_secret = format_token(&hub.paired_id, &"0".repeat(64));
    for credential in ["garbage", "", unknown.as_str(), wrong_secret.as_str()] {
        let mut ws = hub.tls().await;
        let hello = hub.hello(ClientClass::Paired, credential, "0.1.0");
        send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hello))).await;
        expect_error_then_close(&mut ws, ErrorCode::TokenInvalid).await;
    }
    // A kds token is not a paired token.
    let mut ws = hub.tls().await;
    let hello = hub.hello(ClientClass::Paired, &hub.kds_token, "0.1.0");
    send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hello))).await;
    expect_error_then_close(&mut ws, ErrorCode::TokenInvalid).await;
    let err = HubClient::connect(
        &hub.url(),
        &hub.fingerprint,
        hub.hello(ClientClass::Paired, "garbage", "0.1.0"),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(err.code(), Some(ErrorCode::TokenInvalid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_failed_hellos_block_the_address_for_a_minute() {
    let hub = start("0.1.0", true).await;
    for _ in 0..5 {
        let mut ws = hub.tls().await;
        let hello = hub.hello(ClientClass::Paired, "garbage", "0.1.0");
        send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hello))).await;
        expect_error_then_close(&mut ws, ErrorCode::TokenInvalid).await;
    }
    // Even the right token is refused from this address now.
    let err = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), Some(ErrorCode::TokenInvalid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_first_frame_other_than_hello_is_refused_and_closed() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.tls().await;
    send(&mut ws, &submit_text("1", revoke_payload(&hub.paired_id))).await;
    let msg = expect_error_then_close(&mut ws, ErrorCode::BadFrame).await;
    assert!(msg.contains("hello"), "{msg}");
    // A malformed first frame is refused the same way.
    let mut ws = hub.tls().await;
    send(&mut ws, "{not json").await;
    expect_error_then_close(&mut ws, ErrorCode::BadFrame).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_kind_is_unsupported_with_its_rid_and_the_session_stays_open() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.tls().await;
    hello_ok(&mut ws, hub.paired()).await;
    send(&mut ws, r#"{"v":1,"kind":"foo","rid":"7"}"#).await;
    let frame = recv(&mut ws).await.unwrap();
    let (rid, code, _) = error_of(&frame);
    assert_eq!((rid, code), ("7", ErrorCode::Unsupported));
    send(&mut ws, r#"{"v":2,"kind":"ping","rid":"8"}"#).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Unsupported);
    ping(&mut ws, "9").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_malformed_frame_is_bad_frame_and_no_session_closes() {
    let hub = start("0.1.0", true).await;
    let mut a = hub.tls().await;
    hello_ok(&mut a, hub.paired()).await;
    let mut b = hub.plain().await;
    hello_ok(&mut b, hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0")).await;
    send(&mut a, "{not json").await;
    let frame = recv(&mut a).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::BadFrame);
    send(
        &mut a,
        r#"{"v":1,"kind":"intent.submit","rid":"x","intent":{}}"#,
    )
    .await;
    let frame = recv(&mut a).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::BadFrame);
    a.send(Message::binary(vec![1, 2, 3])).await.unwrap();
    let frame = recv(&mut a).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::BadFrame);
    ping(&mut a, "a").await;
    ping(&mut b, "b").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_fields_in_hello_are_ignored() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.tls().await;
    let text = format!(
        r#"{{"v":1,"kind":"hello","rid":"h","x":1,"credential":"{}","class":"paired",
            "versions":{{"app":"0.1.0","ui":"9"}},"capabilities":["events"],"extra":{{"a":1}}}}"#,
        hub.paired_token
    );
    send(&mut ws, &text).await;
    let frame = recv(&mut ws).await.unwrap();
    assert!(matches!(frame.body, FrameBody::HelloOk(_)), "{frame:?}");
    assert_eq!(frame.rid, "h");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clients_more_than_two_minors_behind_are_unsupported() {
    let hub = start("0.5.0", true).await;
    let mut ws = hub.tls().await;
    let hello = hub.hello(ClientClass::Paired, &hub.paired_token, "0.2.9");
    send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hello))).await;
    let msg = expect_error_then_close(&mut ws, ErrorCode::Unsupported).await;
    assert!(msg.contains("minimum 0.3"), "{msg}");
    for app in ["1.5.0"] {
        let err = HubClient::connect(
            &hub.url(),
            &hub.fingerprint,
            hub.hello(ClientClass::Paired, &hub.paired_token, app),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(err.code(), Some(ErrorCode::Unsupported), "{app}");
    }
    for app in ["0.3.0", "0.5.1", "0.7.0"] {
        HubClient::connect(
            &hub.url(),
            &hub.fingerprint,
            hub.hello(ClientClass::Paired, &hub.paired_token, app),
        )
        .await
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoking_a_connected_device_closes_it_within_a_second_and_refuses_it_after() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // A second, unrelated session stays open.
    let mut other = hub.plain().await;
    hello_ok(
        &mut other,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;

    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(system(), revoke_payload(&hub.paired_id), None))
        .await
        .unwrap();
    let revoked_at = Instant::now();
    let closed = tokio::time::timeout(Duration::from_secs(1), client.closed())
        .await
        .expect("closed within 1 s");
    assert!(revoked_at.elapsed() <= Duration::from_secs(1));
    assert_eq!(closed.error.map(|e| e.code), Some(ErrorCode::TokenRevoked));

    let err = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ClientError::Hub(_)), "{err:?}");
    assert_eq!(err.code(), Some(ErrorCode::TokenRevoked));
    // The kds session is still open and got no applied event: browser classes only see
    // allow-listed tables.
    send(&mut other, r#"{"v":1,"kind":"ping","rid":"still-here"}"#).await;
    let frame = recv_any(&mut other).await.unwrap();
    assert_eq!(frame, Frame::new("still-here", FrameBody::Pong));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browser_classes_are_refused_outside_their_scope() {
    let hub = start("0.1.0", true).await;
    let mut kds = hub.plain().await;
    hello_ok(
        &mut kds,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    send(&mut kds, &submit_text("r1", revoke_payload(&hub.paired_id))).await;
    let frame = recv(&mut kds).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
    assert_eq!(frame.rid, "r1");
    // The refusal is logged with the class, the kind and the device.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let logs = log_text();
        if logs.contains("intent outside the session's class scope")
            && logs.contains("\"class\":\"kds\"")
            && logs.contains("\"kind\":\"device.revoke\"")
        {
            break;
        }
        assert!(Instant::now() < deadline, "no warn line in {logs}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A mislabelled envelope cannot borrow an in-scope kind.
    let mut v: serde_json::Value =
        serde_json::from_str(&submit_text("r2", revoke_payload(&hub.paired_id))).unwrap();
    v["intent"]["kind"] = "console.plan_requested".into();
    send(&mut kds, &v.to_string()).await;
    assert_eq!(
        error_of(&recv(&mut kds).await.unwrap()).1,
        ErrorCode::Forbidden
    );
    // Nothing was revoked.
    assert!(
        !DeviceDirectory::open(&hub.path)
            .unwrap()
            .is_revoked(&hub.paired_id)
            .unwrap()
    );

    // A phone is anonymous (its credential is ignored) and may only ask for plans, which have
    // no handler yet.
    let mut phone = hub.plain().await;
    hello_ok(
        &mut phone,
        hub.hello(ClientClass::Phone, "whatever", "0.0.1"),
    )
    .await;
    send(
        &mut phone,
        &submit_text("p1", revoke_payload(&hub.paired_id)),
    )
    .await;
    assert_eq!(
        error_of(&recv(&mut phone).await.unwrap()).1,
        ErrorCode::Forbidden
    );
    let mut v: serde_json::Value =
        serde_json::from_str(&submit_text("p2", revoke_payload(&hub.paired_id))).unwrap();
    v["intent"]["kind"] = "console.plan_requested".into();
    v["intent"]["payload"] = serde_json::json!({"kind": "console.plan_requested"});
    send(&mut phone, &v.to_string()).await;
    assert_eq!(
        error_of(&recv(&mut phone).await.unwrap()).1,
        ErrorCode::Unsupported
    );
    // A paired session has no intents in scope yet either.
    let mut paired = hub.tls().await;
    hello_ok(&mut paired, hub.paired()).await;
    send(
        &mut paired,
        &submit_text("q", revoke_payload(&hub.paired_id)),
    )
    .await;
    assert_eq!(
        error_of(&recv(&mut paired).await.unwrap()).1,
        ErrorCode::Forbidden
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn classes_are_bound_to_their_transport() {
    let hub = start("0.1.0", true).await;
    // paired over plain :7401/ws.
    let mut ws = hub.plain().await;
    send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hub.paired()))).await;
    expect_error_then_close(&mut ws, ErrorCode::Forbidden).await;
    // kds and phone over TLS.
    for (class, cred) in [
        (ClientClass::Kds, hub.kds_token.clone()),
        (ClientClass::Phone, String::new()),
    ] {
        let mut ws = hub.tls().await;
        let hello = hub.hello(class, &cred, "0.1.0");
        send_frame(&mut ws, &Frame::new("h", FrameBody::Hello(hello))).await;
        expect_error_then_close(&mut ws, ErrorCode::Forbidden).await;
    }
}

async fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    tcp.write_all(format!("GET {path} HTTP/1.1\r\nHost: hub\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(5), tcp.read_to_string(&mut out))
        .await
        .unwrap()
        .unwrap();
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn port_7401_sniffs_tls_and_routes_plain_http_by_path() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.web_tls_url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert_eq!(client.hello_ok().class, ClientClass::Paired);
    client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();

    for path in ["/kitchen", "/console/tv-1"] {
        let resp = http_get(hub.web, path).await;
        assert!(resp.starts_with("HTTP/1.1 200 OK"), "{path}: {resp}");
        assert!(resp.contains("Arena 360"));
    }
    for path in ["/nope", "/", "/console/", "/ws/extra"] {
        let resp = http_get(hub.web, path).await;
        assert!(resp.starts_with("HTTP/1.1 404"), "{path}: {resp}");
    }
    // `:7400` serves the WebSocket at `/` only.
    let err = connect_ws(
        &format!("wss://127.0.0.1:{}/other", hub.lan.port()),
        &hub.fingerprint,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ClientError::Handshake(_)), "{err:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_pinned_fingerprint_fails_the_connect() {
    let hub = start("0.1.0", true).await;
    let other = HubCert::generate(None).unwrap();
    let err = HubClient::connect(&hub.url(), &other.fingerprint, hub.paired())
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ClientError::Tls(_)), "{err:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_follower_answers_hello_then_refuses_every_intent_not_primary() {
    let hub = start("0.1.0", false).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(!client.hello_ok().primary);
    let body = serde_json::to_value(IntentSubmit {
        intent: intent(system(), revoke_payload(&hub.paired_id), None),
    })
    .unwrap();
    let err = client.request("intent.submit", body).await.unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::NotPrimary));
    // A follower never records the reported version.
    assert_eq!(hub.device_version(), None);
}

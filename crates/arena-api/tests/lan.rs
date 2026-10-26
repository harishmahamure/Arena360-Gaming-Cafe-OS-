//! The LAN protocol server, one test per row of the Story 1.9 matrix (the client-side pinning
//! and `HUB_SLOW` rows also live in `arena-kiosk-core/tests/client.rs`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use arena_api::lan::{HubCert, HubInfoSource, LanHub, Presence, bind_lan, serve_lan};
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
    presence: Presence,
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
        reason: payload.kind().reasons().map(|r| r[0]),
        payload,
    }
}

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "test".into(),
    }
}

/// The owner: holds every permission (device.manage, device.remote, settings.edit).
fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "test-owner".into(),
    }
}

async fn register(w: &WriterHandle, class: ClientClass, label: &str) -> (String, String) {
    let secret = new_token_secret().unwrap();
    let applied = w
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceRegister(DeviceRegister {
                device_id: None,
                device_kind: Default::default(),
                class,
                label: label.into(),
                token_hash: hash_token_secret(&secret).unwrap(),
                tv_number: None,
                group_id: None,
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
                onboarding: None,
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
    status_tx.send_modify(|s| s.tls_fingerprint = Some(cert.fingerprint.clone()));
    let hub = Arc::new(
        LanHub::new(
            writer.clone(),
            status_rx,
            clock,
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            version,
        )
        .with_staff(Arc::new(arena_core::StaffDirectory::open(&path).unwrap()))
        .with_info(HubInfoSource {
            reader: Arc::new(arena_core::SnapshotReader::open(&path).unwrap()),
            hub_name: "test-hub".into(),
            lan_port: 7400,
            web_port: 7401,
        }),
    );
    let presence = hub.presence();
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
        presence,
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

    // The version is recorded through `device.version_reported`, itself pushed as an event
    // (after the manager verifier every paired session gets on hello).
    let first = loop {
        let ev = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .unwrap()
            .unwrap();
        if !matches!(ev, HubEvent::Verifier { .. }) {
            break ev;
        }
    };
    let HubEvent::Applied(s) = first else {
        panic!("{:?}", first)
    };
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
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "receipt.footer".into(),
                value: "thanks".into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    let pushed = loop {
        let ev = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .unwrap()
            .unwrap();
        if !matches!(ev, HubEvent::Verifier { .. }) {
            break ev;
        }
    };
    let HubEvent::Applied(s) = pushed else {
        panic!("{:?}", pushed)
    };
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
        .submit_intent(intent(owner(), revoke_payload(&hub.paired_id), None))
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
    assert!(matches!(err, ClientError::PinMismatch { .. }), "{err:?}");
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

// ---- Story 1.16: kiosk.log from a paired session, and presence ----

fn kiosk_log_body(actor: Actor) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            actor,
            IntentPayload::KioskLog(arena_proto::KioskLog {
                level: arena_proto::KioskLogLevel::Info,
                code: "app_started".into(),
                detail: String::new(),
            }),
            None,
        ),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_session_logs_kiosk_events_as_itself_and_shows_online() {
    let hub = start("0.1.0", true).await;
    assert!(!hub.presence.online(&hub.paired_id));
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(hub.presence.online(&hub.paired_id));
    // A forged actor is replaced by the session's device.
    let frame = client
        .request("intent.submit", kiosk_log_body(owner()))
        .await
        .unwrap();
    let FrameBody::IntentResult(r) = frame.body else {
        panic!("{frame:?}")
    };
    assert_eq!(r.applied.changes[0].tbl, "device_events");
    let change = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while hub.presence.online(&hub.paired_id) {
        assert!(Instant::now() < deadline, "still online after the close");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kds_session_may_not_log_kiosk_events() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.plain().await;
    let hello = Frame::new(
        "h",
        FrameBody::Hello(hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0")),
    );
    send_frame(&mut ws, &hello).await;
    assert!(matches!(
        recv(&mut ws).await.unwrap().body,
        FrameBody::HelloOk(_)
    ));
    let mut submit = kiosk_log_body(system());
    submit["v"] = 1.into();
    submit["kind"] = "intent.submit".into();
    submit["rid"] = "k1".into();
    send(&mut ws, &submit.to_string()).await;
    let reply = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&reply).1, ErrorCode::Forbidden);
    assert!(!hub.presence.online("anything"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kiosk_log_is_rate_limited_per_device_as_hub_slow() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for i in 0..60 {
        client
            .request("intent.submit", kiosk_log_body(owner()))
            .await
            .unwrap_or_else(|e| panic!("event {i}: {e}"));
    }
    let err = client
        .request("intent.submit", kiosk_log_body(owner()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::HubSlow));
}

fn process_blocked_body(n: usize) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            owner(),
            IntentPayload::KioskProcessBlocked(arena_proto::KioskProcessBlocked {
                exe_name: format!("g{n}.exe"),
                path: format!(r"C:\Games\g{n}.exe"),
                reason: arena_proto::ProcessBlockReason::NotAllowed,
                mode: arena_proto::ProcessMode::Report,
            }),
            None,
        ),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kiosk_events_share_one_rate_limit() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for i in 0..30 {
        client
            .request("intent.submit", kiosk_log_body(owner()))
            .await
            .unwrap_or_else(|e| panic!("log {i}: {e}"));
        client
            .request("intent.submit", process_blocked_body(i))
            .await
            .unwrap_or_else(|e| panic!("process_blocked {i}: {e}"));
    }
    for next in [kiosk_log_body(owner()), process_blocked_body(99)] {
        let err = client.request("intent.submit", next).await.unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::HubSlow));
    }
}

// ---- Story 1.18: the kiosk process lists over settings.get and settings.changed ----

fn settings_get_body(keys: &[&str]) -> serde_json::Value {
    serde_json::json!({ "keys": keys })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_session_reads_kiosk_settings_only() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let frame = client
        .request(
            "settings.get",
            settings_get_body(&["kiosk.process_allow", "kiosk.process_block", "kiosk.nope"]),
        )
        .await
        .unwrap();
    let FrameBody::SettingsValues(v) = frame.body else {
        panic!("{frame:?}")
    };
    let keys: Vec<&str> = v.values.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["kiosk.process_allow", "kiosk.process_block"]);
    assert_eq!(v.values[0].value, arena_proto::DEFAULT_PROCESS_ALLOW);
    assert_eq!(v.values[1].value, arena_proto::DEFAULT_PROCESS_BLOCK);

    // Any other key refuses the whole request.
    for keys in [
        &["branch.currency"][..],
        &["kiosk.process_allow", "receipt.footer"][..],
    ] {
        let err = client
            .request("settings.get", settings_get_body(keys))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Forbidden), "{keys:?}");
    }
    let err = client
        .request("settings.get", settings_get_body(&[]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::BadFrame));
    // The session stays usable.
    client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browser_classes_may_not_read_settings() {
    let hub = start("0.1.0", true).await;
    for (class, credential) in [
        (ClientClass::Kds, hub.kds_token.clone()),
        (ClientClass::Phone, String::new()),
    ] {
        let mut ws = hub.plain().await;
        hello_ok(&mut ws, hub.hello(class, &credential, "0.1.0")).await;
        send(
            &mut ws,
            r#"{"v":1,"kind":"settings.get","rid":"g1","keys":["kiosk.process_allow"]}"#,
        )
        .await;
        let reply = recv(&mut ws).await.unwrap();
        assert_eq!(error_of(&reply).0, "g1");
        assert_eq!(error_of(&reply).1, ErrorCode::Forbidden, "{class:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_list_change_is_pushed_to_paired_sessions_but_not_browsers() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    let mut kds = hub.plain().await;
    hello_ok(
        &mut kds,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let games = r#"[{"path_glob": "C:\\Games\\*.exe"}]"#;
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "kiosk.process_allow".into(),
                value: games.into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let values = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, events.next()).await.unwrap() {
            Some(HubEvent::SettingsChanged(v)) => break v.values,
            Some(_) => continue,
            None => panic!("closed"),
        }
    };
    assert_eq!(values.len(), 1);
    assert_eq!(
        (
            values[0].key.as_str(),
            values[0].value.as_str(),
            values[0].rev
        ),
        ("kiosk.process_allow", games, 2)
    );
    // The kds session hears nothing (no applied either: no table is on its list).
    ping(&mut kds, "p1").await;
    let next = tokio::time::timeout(Duration::from_millis(300), kds.next()).await;
    assert!(next.is_err(), "kds got {next:?}");
}

// ---- Story 1.21: a kiosk reads and hears only its own high-contrast setting ----

/// Waits up to `ms` for the next `settings.changed` on `events`.
async fn next_settings(
    events: &mut (impl futures_util::Stream<Item = HubEvent> + Unpin),
    ms: u64,
) -> Option<Vec<arena_proto::SettingEntry>> {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, events.next()).await {
            Ok(Some(HubEvent::SettingsChanged(v))) => return Some(v.values),
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return None,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_reads_and_hears_only_its_own_hc_key() {
    let hub = start("0.1.0", true).await;
    let writer = hub.writer.as_ref().unwrap();
    let (other_id, other_token) = register(writer, ClientClass::Paired, "Kiosk 2").await;
    let own_key = arena_proto::hc_key(&hub.paired_id);
    let other_key = arena_proto::hc_key(&other_id);
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let frame = client
        .request(
            "settings.get",
            settings_get_body(&["kiosk.process_mode", &own_key]),
        )
        .await
        .unwrap();
    let FrameBody::SettingsValues(v) = frame.body else {
        panic!("{frame:?}")
    };
    let got: Vec<(&str, &str)> = v
        .values
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("kiosk.process_mode", "enforce"),
            (own_key.as_str(), "false")
        ]
    );
    for keys in [
        &[other_key.as_str()][..],
        &[own_key.as_str(), "display.hc.hub"][..],
    ] {
        let err = client
            .request("settings.get", settings_get_body(keys))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Forbidden), "{keys:?}");
    }

    let other = HubClient::connect(
        &hub.url(),
        &hub.fingerprint,
        hub.hello(ClientClass::Paired, &other_token, "0.1.0"),
    )
    .await
    .unwrap();
    let mut mine = client.events().unwrap();
    let mut theirs = other.events().unwrap();
    writer
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: own_key.clone(),
                value: "true".into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    let values = next_settings(&mut mine, 5_000).await.expect("pushed");
    assert_eq!(values.len(), 1);
    assert_eq!(
        (
            values[0].key.as_str(),
            values[0].value.as_str(),
            values[0].rev
        ),
        (own_key.as_str(), "true", 2)
    );
    assert_eq!(
        next_settings(&mut theirs, 300).await,
        None,
        "not to another kiosk"
    );
    // The Hub's own key reaches no kiosk.
    writer
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "display.hc.hub".into(),
                value: "true".into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    assert_eq!(next_settings(&mut mine, 300).await, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fulfilment_and_kiosk_volume_reach_every_connected_kiosk() {
    let hub = start("0.1.0", true).await;
    let writer = hub.writer.as_ref().unwrap();
    let (_, other_token) = register(writer, ClientClass::Paired, "Kiosk 2").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let frame = client
        .request(
            "settings.get",
            settings_get_body(&["branch.fulfilment", "kiosk.volume_default"]),
        )
        .await
        .unwrap();
    let FrameBody::SettingsValues(v) = frame.body else {
        panic!("{frame:?}")
    };
    let got: Vec<(&str, &str)> = v
        .values
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("branch.fulfilment", "counter"),
            ("kiosk.volume_default", "50")
        ]
    );
    for key in [
        "branch.opening_hours",
        "system.backup_dir",
        "branch.ads_consent",
    ] {
        let err = client
            .request("settings.get", settings_get_body(&[key]))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Forbidden), "{key}");
    }
    let other = HubClient::connect(
        &hub.url(),
        &hub.fingerprint,
        hub.hello(ClientClass::Paired, &other_token, "0.1.0"),
    )
    .await
    .unwrap();
    let mut mine = client.events().unwrap();
    let mut theirs = other.events().unwrap();
    for (key, value) in [
        ("branch.fulfilment", "seat"),
        ("kiosk.volume_default", "30"),
    ] {
        writer
            .submit_intent(intent(
                owner(),
                IntentPayload::SettingsSet(SettingsSet {
                    key: key.into(),
                    value: value.into(),
                }),
                Some(1),
            ))
            .await
            .unwrap();
        for events in [&mut mine, &mut theirs] {
            let values = next_settings(events, 5_000).await.expect("pushed");
            assert_eq!(values.len(), 1);
            assert_eq!(
                (
                    values[0].key.as_str(),
                    values[0].value.as_str(),
                    values[0].rev
                ),
                (key, value, 2)
            );
        }
    }
    writer
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "system.backup_dir".into(),
                value: "/mnt/backup".into(),
            }),
            Some(1),
        ))
        .await
        .unwrap();
    assert_eq!(next_settings(&mut mine, 300).await, None);
}

// ---- Story 1.20: kiosk admin exit ----

/// Adds a staff member with `pin` in the built-in role `role`; returns the id.
async fn add_staff(hub: &TestHub, login: &str, role: &str, pin: &str) -> String {
    let role_id = Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::Roles)
        .unwrap()
        .into_iter()
        .find(|r| r["name"] == arena_store::RowValue::Text(role.into()))
        .map(|r| match &r["role_id"] {
            arena_store::RowValue::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        })
        .unwrap();
    let applied = hub
        .writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                staff_id: None,
                login: login.into(),
                display_name: login.into(),
                role_id,
                password_hash: Some(arena_core::staff::hash_secret("pw").unwrap()),
                pin_hash: Some(arena_core::staff::hash_secret(pin).unwrap()),
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
            None,
        ))
        .await
        .unwrap();
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "staff")
        .unwrap()
        .row_id
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_hello_gets_the_verifier_and_kds_never_does() {
    let hub = start("0.1.0", true).await;
    let manager = add_staff(&hub, "manager", "Manager", "2222").await;
    add_staff(&hub, "counter", "Counter", "3333").await;
    // A kds session first: it must never see the verifier, even when it changes.
    let mut kds = hub.plain().await;
    let hello = Frame::new(
        "h",
        FrameBody::Hello(hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0")),
    );
    send_frame(&mut kds, &hello).await;
    assert!(matches!(
        recv(&mut kds).await.unwrap().body,
        FrameBody::HelloOk(_)
    ));

    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    let first = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .unwrap()
            .unwrap()
        {
            HubEvent::Verifier { verifier } => break verifier,
            _ => continue,
        }
    };
    assert_eq!(
        first
            .entries
            .iter()
            .map(|e| e.staff_id.clone())
            .collect::<Vec<_>>(),
        vec![manager.clone()],
        "only device.remote holders"
    );
    assert!(first.entries[0].pin_hash.starts_with("$argon2id$"));
    // Another manager: a newer verifier is pushed.
    add_staff(&hub, "manager2", "Manager", "5555").await;
    let second = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .unwrap()
            .unwrap()
        {
            HubEvent::Verifier { verifier } => break verifier,
            _ => continue,
        }
    };
    assert!(second.version > first.version);
    assert_eq!(second.entries.len(), 2);
    // The kds session got nothing at all (no verifier, no applies for unlisted tables).
    ping(&mut kds, "k1").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_checks_admin_exit_pins_and_pauses_after_five_wrong() {
    let hub = start("0.1.0", true).await;
    let manager = add_staff(&hub, "manager", "Manager", "2222").await;
    add_staff(&hub, "counter", "Counter", "3333").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let verify = |pin: &str| {
        let body = serde_json::json!({ "pin_proof": pin });
        client.request("kiosk.verify_admin_pin", body)
    };
    let frame = verify("2222").await.unwrap();
    let FrameBody::AdminPinOk(ok) = frame.body else {
        panic!("{frame:?}")
    };
    assert_eq!(ok.staff_id, manager);
    assert!(ok.verifier_version >= 1);
    // Wrong, and a Counter's PIN: the same refusal.
    for pin in ["9999", "3333"] {
        match verify(pin).await {
            Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::PinRejected, "{pin}"),
            other => panic!("{other:?}"),
        }
    }
    for _ in 0..2 {
        verify("9999").await.unwrap_err();
    }
    match verify("9999").await {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::AuthLocked),
        other => panic!("{other:?}"),
    }
    // Paused even for the right PIN.
    match verify("2222").await {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::AuthLocked),
        other => panic!("{other:?}"),
    }
    let log = log_text();
    assert!(
        !log.contains("\"9999\"") && !log.contains("=9999"),
        "the PIN is never logged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_wrong_pins_cannot_get_past_the_limit() {
    let hub = start("0.1.0", true).await;
    add_staff(&hub, "manager", "Manager", "2222").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let attempts = (0..8).map(|_| {
        client.request(
            "kiosk.verify_admin_pin",
            serde_json::json!({ "pin_proof": "9999" }),
        )
    });
    let results = futures_util::future::join_all(attempts).await;
    let codes: Vec<ErrorCode> = results
        .into_iter()
        .map(|r| match r {
            Err(ClientError::Hub(e)) => e.code,
            other => panic!("{other:?}"),
        })
        .collect();
    let checked = codes
        .iter()
        .filter(|c| **c == ErrorCode::PinRejected)
        .count();
    assert!(checked <= 5, "{codes:?}");
    assert!(codes.contains(&ErrorCode::AuthLocked), "{codes:?}");
    // And the right PIN is paused now too.
    match client
        .request(
            "kiosk.verify_admin_pin",
            serde_json::json!({ "pin_proof": "2222" }),
        )
        .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::AuthLocked),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remote_unlock_reaches_the_kiosk_and_its_ack_reaches_the_hub_ui() {
    let hub = start("0.1.0", true).await;
    let mut acks = hub.presence.admin_unlock_acks().unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    let applied = hub
        .writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceAdminUnlock(arena_proto::DeviceAdminUnlock {
                device_id: hub.paired_id.clone(),
            }),
            None,
        ))
        .await
        .unwrap();
    let exit_id = applied.changes[0].row_id.clone();
    let pushed = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .unwrap()
            .unwrap()
        {
            HubEvent::AdminUnlock { exit_id } => break exit_id,
            _ => continue,
        }
    };
    assert_eq!(pushed, exit_id);
    // An ack naming someone else's (or no) exit is refused.
    let bad = client
        .request(
            "kiosk.admin_unlock_ack",
            serde_json::json!({ "exit_id": "01JNOPEAAAAAAAAAAAAAAAAAAA" }),
        )
        .await;
    assert!(matches!(bad, Err(ClientError::Hub(e)) if e.code == ErrorCode::ValidationFailed));
    let frame = client
        .request(
            "kiosk.admin_unlock_ack",
            serde_json::json!({ "exit_id": exit_id }),
        )
        .await
        .unwrap();
    assert_eq!(frame.body, FrameBody::Ack);
    let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (ack.exit_id, ack.device_id),
        (exit_id, hub.paired_id.clone())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pc_command_reaches_the_kiosk_and_only_a_pending_ack_reaches_the_hub_ui() {
    let hub = start("0.1.0", true).await;
    let mut acks = hub.presence.pc_acks().unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    let cmd_id = "01JCMDAAAAAAAAAAAAAAAAAAAA";
    let bad = client
        .request("kiosk.pc_ack", serde_json::json!({ "cmd_id": cmd_id }))
        .await;
    assert!(matches!(bad, Err(ClientError::Hub(e)) if e.code == ErrorCode::ValidationFailed));
    assert_eq!(
        hub.presence.push_pc_command(
            &hub.paired_id,
            cmd_id,
            arena_proto::PcAction::Message,
            Some("hello".into())
        ),
        1
    );
    let pushed = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .unwrap()
            .unwrap()
        {
            HubEvent::PcCommand {
                cmd_id,
                action,
                text,
            } => break (cmd_id, action, text),
            _ => continue,
        }
    };
    assert_eq!(
        pushed,
        (
            cmd_id.to_owned(),
            arena_proto::PcAction::Message,
            Some("hello".to_owned())
        )
    );
    let frame = client
        .request("kiosk.pc_ack", serde_json::json!({ "cmd_id": cmd_id }))
        .await
        .unwrap();
    assert_eq!(frame.body, FrameBody::Ack);
    let ack = tokio::time::timeout(Duration::from_secs(5), acks.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (ack.cmd_id.as_str(), ack.device_id),
        (cmd_id, hub.paired_id.clone())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_session_reaches_the_kiosk_and_only_it_completes_it() {
    let hub = start("0.1.0", true).await;
    let w = hub.writer.as_ref().unwrap();
    let group = w
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                id: None,
                name: "PCs".into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
            None,
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    w.submit_intent(intent(
        owner(),
        IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
            device_id: hub.paired_id.clone(),
            group_id: Some(group),
        }),
        None,
    ))
    .await
    .unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    w.submit_intent(intent(
        owner(),
        IntentPayload::DeviceTestSessionStart(arena_proto::DeviceTestSessionStart {
            device_id: hub.paired_id.clone(),
        }),
        None,
    ))
    .await
    .unwrap();
    let lock_after_ms = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .unwrap()
            .unwrap()
        {
            HubEvent::TestSession { lock_after_ms } => break lock_after_ms,
            _ => continue,
        }
    };
    assert_eq!(lock_after_ms, 120_000);
    let complete = |device: &str| {
        serde_json::to_value(IntentSubmit {
            intent: intent(
                owner(),
                IntentPayload::DeviceTestSessionCompleted(
                    arena_proto::DeviceTestSessionCompleted {
                        device_id: device.into(),
                        outcome: arena_proto::TestSessionOutcome::Locked,
                    },
                ),
                None,
            ),
        })
        .unwrap()
    };
    let other = client
        .request("intent.submit", complete("01JOTHERDEVICEAAAAAAAAAAAA"))
        .await;
    assert!(matches!(other, Err(ClientError::Hub(e)) if e.code == ErrorCode::Forbidden));
    let frame = client
        .request("intent.submit", complete(&hub.paired_id))
        .await
        .unwrap();
    let FrameBody::IntentResult(r) = frame.body else {
        panic!("{frame:?}")
    };
    assert_eq!(r.applied.changes[0].tbl, "settings");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kds_session_may_not_verify_admin_pins() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.plain().await;
    let hello = Frame::new(
        "h",
        FrameBody::Hello(hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0")),
    );
    send_frame(&mut ws, &hello).await;
    assert!(matches!(
        recv(&mut ws).await.unwrap().body,
        FrameBody::HelloOk(_)
    ));
    send(
        &mut ws,
        r#"{"v":1,"kind":"kiosk.verify_admin_pin","rid":"v","pin_proof":"2222"}"#,
    )
    .await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
}

fn http_body(resp: &str) -> serde_json::Value {
    let (_, body) = resp.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

async fn https_get(addr: SocketAddr, fingerprint: &str, path: &str) -> String {
    let config = arena_kiosk_core::pinned_config(fingerprint).unwrap();
    let tcp = TcpStream::connect(addr).await.unwrap();
    let name = rustls::pki_types::ServerName::IpAddress(addr.ip().into());
    let mut tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .unwrap();
    tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: hub\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut out)).await;
    String::from_utf8(out).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_info_answers_over_plain_http_and_tls_without_auth() {
    let hub = start("0.1.0", true).await;
    let want = serde_json::json!({
        "product": "arena360-hub",
        "branch_name": "Dev Branch",
        "hub_name": "test-hub",
        "protocol_version": arena_proto::PROTOCOL_VERSION,
        "fingerprint": hub.fingerprint,
        "lan_port": 7400,
        "web_port": 7401,
    });
    let plain = http_get(hub.web, "/hub-info").await;
    assert!(plain.starts_with("HTTP/1.1 200 OK"), "{plain}");
    assert!(
        plain.contains("Content-Type: application/json\r\n"),
        "{plain}"
    );
    assert!(plain.contains("Cache-Control: no-store\r\n"), "{plain}");
    assert_eq!(http_body(&plain), want);
    let tls = https_get(hub.web, &hub.fingerprint, "/hub-info").await;
    assert!(tls.starts_with("HTTP/1.1 200 OK"), "{tls}");
    assert_eq!(http_body(&tls), want);
    let captured =
        arena_kiosk_core::capture_fingerprint("127.0.0.1", hub.web.port(), Duration::from_secs(2))
            .await
            .unwrap();
    assert_eq!(captured, hub.fingerprint);

    let mut tcp = TcpStream::connect(hub.web).await.unwrap();
    tcp.write_all(b"POST /hub-info HTTP/1.1\r\nHost: hub\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(5), tcp.read_to_string(&mut out))
        .await
        .unwrap()
        .unwrap();
    assert!(out.starts_with("HTTP/1.1 404"), "{out}");
    let client = HubClient::connect(&hub.web_tls_url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_info_is_no_hub_before_the_branch_is_bootstrapped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branch.db");
    let store = Store::open(&path, T0).unwrap();
    let mut initial = HubStatus::opened(store.schema_version().unwrap(), HUB_ID);
    initial.role = Role::Primary;
    let cert = HubCert::generate(None).unwrap();
    initial.tls_fingerprint = Some(cert.fingerprint.clone());
    let (_status_tx, status_rx) = status_channel(initial);
    let hub = Arc::new(
        LanHub::new(
            None,
            status_rx,
            Arc::new(ManualClock::new(T0)),
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            cert.server_config().unwrap(),
            "0.1.0",
        )
        .with_info(HubInfoSource {
            reader: Arc::new(arena_core::SnapshotReader::open(&path).unwrap()),
            hub_name: "test-hub".into(),
            lan_port: 7400,
            web_port: 7401,
        }),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let web = listeners.web_addr().unwrap();
    let (_stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));
    let resp = http_get(web, "/hub-info").await;
    assert!(resp.starts_with("HTTP/1.1 503"), "{resp}");
    assert!(resp.contains("Cache-Control: no-store\r\n"), "{resp}");
    assert_eq!(http_body(&resp)["code"], "NO_HUB");
    drop(store);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_follower_answers_hub_info_no_hub() {
    let hub = start("0.1.0", false).await;
    let resp = http_get(hub.web, "/hub-info").await;
    assert!(resp.starts_with("HTTP/1.1 503"), "{resp}");
    assert_eq!(http_body(&resp)["code"], "NO_HUB");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setting_a_managers_pin_with_a_setup_code_pushes_the_new_verifier() {
    let hub = start("0.1.0", true).await;
    add_staff(&hub, "manager", "Manager", "2222").await;
    let newcomer = add_staff(&hub, "newcomer", "Manager", "1111").await;
    let code_hash = arena_core::staff::hash_secret("ABCD2345").unwrap();
    Store::open(&hub.path, 0)
        .unwrap()
        .write_tx(|tx| {
            tx.execute_batch(&format!(
                "UPDATE staff SET password_hash = NULL, pin_hash = NULL,
                                  setup_code_hash = '{code_hash}',
                                  setup_expires_at = {}
                 WHERE staff_id = '{newcomer}'",
                i64::MAX
            ))
            .map_err(arena_store::db_error)?;
            Ok(())
        })
        .unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    macro_rules! next_verifier {
        () => {
            loop {
                match tokio::time::timeout(Duration::from_secs(5), events.next_event())
                    .await
                    .unwrap()
                    .unwrap()
                {
                    HubEvent::Verifier { verifier } => break verifier,
                    _ => continue,
                }
            }
        };
    }
    let first = next_verifier!();
    assert!(first.entries.iter().all(|e| e.staff_id != newcomer));
    let pin_hash = arena_core::staff::hash_secret("4821").unwrap();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            Actor {
                kind: ActorKind::Staff,
                id: newcomer.clone(),
            },
            IntentPayload::StaffSetCredentials(arena_proto::StaffSetCredentials {
                staff_id: newcomer.clone(),
                setup_code_hash: code_hash,
                trusted_now_ms: 0,
                password_hash: arena_core::staff::hash_secret("a-long-new-password").unwrap(),
                pin_hash: pin_hash.clone(),
            }),
            None,
        ))
        .await
        .unwrap();
    let second = next_verifier!();
    assert!(second.version > first.version);
    let entry = second
        .entries
        .iter()
        .find(|e| e.staff_id == newcomer)
        .unwrap();
    assert_eq!(entry.pin_hash, pin_hash);
}

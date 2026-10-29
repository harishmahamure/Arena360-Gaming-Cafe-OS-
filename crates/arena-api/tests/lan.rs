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
    start_with(version, primary, None).await
}

/// [`start`] with the game media cache the Hub matches scans against (Story 6.5).
async fn start_with(
    version: &str,
    primary: bool,
    media: Option<Arc<arena_core::media_cache::MediaCache>>,
) -> TestHub {
    start_full(version, primary, media, true).await
}

/// The player key the test hub opens names with.
const PLAYER_KEK: [u8; 32] = [6; 32];

struct TestKek;

impl arena_core::players::Kek for TestKek {
    fn kek(&self) -> Result<[u8; 32], arena_proto::AppError> {
        Ok(PLAYER_KEK)
    }
}

/// [`start_with`], optionally without a player key (a Hub with no player directory).
async fn start_full(
    version: &str,
    primary: bool,
    media: Option<Arc<arena_core::media_cache::MediaCache>>,
    player_kek: bool,
) -> TestHub {
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
    let mut lan_hub = LanHub::new(
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
    });
    if let Some(cache) = media {
        lan_hub = lan_hub.with_media(cache);
    }
    if player_kek {
        lan_hub = lan_hub.with_player_kek(Arc::new(TestKek));
    }
    let hub = Arc::new(lan_hub);
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

// ---- Story 6.27: game.play_recorded and games.most_played ----

fn play_recorded_body(game: &str, minutes: i64) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            owner(),
            IntentPayload::KioskGamePlayRecorded(arena_proto::KioskGamePlayRecorded {
                game_id: game.into(),
                started_at: T0 - minutes * 60_000,
                ended_at: T0,
                minutes,
            }),
            None,
        ),
    })
    .unwrap()
}

async fn most_played_frame(client: &HubClient) -> Result<arena_proto::MostPlayed, ClientError> {
    match client
        .request("games.most_played", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::GamesMostPlayedResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plays_are_rate_limited_with_the_other_kiosk_events() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for i in 0..60 {
        let frame = client
            .request("intent.submit", play_recorded_body("G1", 5))
            .await
            .unwrap_or_else(|e| panic!("play {i}: {e}"));
        let FrameBody::IntentResult(r) = frame.body else {
            panic!("{frame:?}")
        };
        assert_eq!(r.applied.changes[0].tbl, "game_plays");
    }
    let err = client
        .request("intent.submit", play_recorded_body("G1", 5))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::HubSlow));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_gets_the_top_games_and_only_its_own_players_last_played_times() {
    let hub = start("0.1.0", true).await;
    let (client, player) = playing(&hub).await;
    for (game, minutes) in [("G1", 40), ("G2", 70), ("G1", 10)] {
        client
            .request("intent.submit", play_recorded_body(game, minutes))
            .await
            .unwrap();
    }
    // The Hub named the player on the PC; the kiosk never did.
    let rows = Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::GamePlays)
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r["player_id"] == arena_store::RowValue::Text(player.clone()))
    );
    let got = most_played_frame(&client).await.unwrap();
    let top: Vec<_> = got
        .top
        .iter()
        .map(|g| (g.game_id.as_str(), g.minutes))
        .collect();
    assert_eq!(top, [("G2", 70), ("G1", 50)]);
    // No library game here is linked to stored media (Story 6.29): the list says so.
    assert!(got.media.is_empty());
    let last: Vec<_> = got
        .last_played
        .iter()
        .map(|l| (l.game_id.as_str(), l.at))
        .collect();
    assert_eq!(last, [("G1", T0), ("G2", T0)]);
    assert!(!serde_json::to_string(&got).unwrap().contains(&player));
    // The top list is kept for 15 minutes; the player's own times are always current.
    client
        .request("intent.submit", play_recorded_body("G3", 200))
        .await
        .unwrap();
    let again = most_played_frame(&client).await.unwrap();
    assert_eq!(again.top, got.top);
    assert_eq!(again.last_played.len(), 3);
}

// ---- Story 6.11: kiosk.cleanup_failed ----

fn cleanup_failed_body(step: &str) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            owner(),
            IntentPayload::KioskCleanupFailed(arena_proto::KioskCleanupFailed {
                step: step.into(),
                detail: "a program would not close".into(),
            }),
            None,
        ),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_session_reports_a_cleanup_failure_and_the_tile_gets_one_check_pc_alert() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // A forged actor is replaced by the session's device.
    let frame = client
        .request("intent.submit", cleanup_failed_body("programs"))
        .await
        .unwrap();
    let FrameBody::IntentResult(r) = frame.body else {
        panic!("{frame:?}")
    };
    let tables: Vec<&str> = r.applied.changes.iter().map(|c| c.tbl.as_str()).collect();
    assert_eq!(tables, ["device_events", "alerts"]);
    client
        .request("intent.submit", cleanup_failed_body("volume"))
        .await
        .unwrap();
    let store = Store::open_reader(&hub.path).unwrap();
    let events = store.rows(arena_store::Table::DeviceEvents).unwrap();
    let cleanup: Vec<_> = events
        .iter()
        .filter(|e| e["code"] == arena_store::RowValue::Text("cleanup_failed".into()))
        .collect();
    assert_eq!(cleanup.len(), 2);
    assert_eq!(
        cleanup[0]["device_id"],
        arena_store::RowValue::Text(hub.paired_id.clone())
    );
    let alerts = store.rows(arena_store::Table::Alerts).unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(
        alerts[0]["kind"],
        arena_store::RowValue::Text("check_pc".into())
    );
    assert_eq!(
        alerts[0]["device_id"],
        arena_store::RowValue::Text(hub.paired_id.clone())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cleanup_failures_share_the_kiosk_event_rate_limit() {
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
        .request("intent.submit", cleanup_failed_body("programs"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::HubSlow));
}

// ---- Story 6.4: the kiosk scan report ----

fn scan_body(n: i64) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            owner(),
            IntentPayload::KioskScanReported(arena_proto::KioskScanReported {
                scanned_at: T0 - 10_000 + n,
                entries: vec![arena_proto::ScanEntry {
                    path: format!(r"C:\Games\g{n}.exe"),
                    file_name: format!("g{n}.exe"),
                    product_name: String::new(),
                    publisher: String::new(),
                    launcher: "none".into(),
                }],
                launchers: Vec::new(),
                skipped: 0,
            }),
            None,
        ),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_session_reports_scans_as_itself_and_the_device_rate_limit_does_not_apply() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // More than the 60 a minute a chatty kiosk may log: every report applies.
    for n in 0..70 {
        let frame = client
            .request("intent.submit", scan_body(n))
            .await
            .unwrap_or_else(|e| panic!("scan {n}: {e}"));
        let FrameBody::IntentResult(r) = frame.body else {
            panic!("{frame:?}")
        };
        assert_eq!(r.applied.changes[0].tbl, "kiosk_scans");
        assert_eq!(r.applied.changes[0].row_id, hub.paired_id);
    }
    // (The Hub matches each scan in the background, so the newest change may be its match.)
    let change = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .into_iter()
        .rfind(|c| c.intent_kind == "kiosk.scan_reported")
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    // An older report is refused as a validation error naming `scanned_at`.
    let err = client
        .request("intent.submit", scan_body(3))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::ValidationFailed));
    // The log limit is not shared with it: the log bucket is still full.
    client
        .request("intent.submit", kiosk_log_body(owner()))
        .await
        .unwrap();
}

// ---- Story 6.5: the Hub matches a stored scan to the catalogue ----

/// Waits up to five seconds for `game_matches` to hold a row for `device` made from `scanned_at`.
async fn matches_of(hub: &TestHub, device: &str, scanned_at: i64) -> Vec<arena_proto::GameMatch> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = Store::open_reader(&hub.path)
            .unwrap()
            .rows(arena_store::Table::GameMatches)
            .unwrap();
        let found = rows.into_iter().find(|r| {
            r.get("device_id") == Some(&arena_store::RowValue::Text(device.into()))
                && r.get("scanned_at") == Some(&arena_store::RowValue::Integer(scanned_at))
        });
        if let Some(row) = found {
            let Some(arena_store::RowValue::Text(json)) = row.get("matches") else {
                panic!("{row:?}")
            };
            return serde_json::from_str(json).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "no matches recorded for {device}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn scan_with(at: i64, entries: Vec<arena_proto::ScanEntry>) -> serde_json::Value {
    serde_json::to_value(IntentSubmit {
        intent: intent(
            owner(),
            IntentPayload::KioskScanReported(arena_proto::KioskScanReported {
                scanned_at: at,
                entries,
                launchers: Vec::new(),
                skipped: 0,
            }),
            None,
        ),
    })
    .unwrap()
}

fn exe(path: &str, product: &str) -> arena_proto::ScanEntry {
    arena_proto::ScanEntry {
        path: path.into(),
        file_name: path.rsplit('\\').next().unwrap().into(),
        product_name: product.into(),
        publisher: "Maker".into(),
        launcher: "none".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stored_scan_is_matched_against_the_catalogue_and_recorded_as_the_system() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(
        arena_core::media_cache::MediaCache::open(
            dir.path().join("reference"),
            Arc::new(|| 1 << 30),
        )
        .unwrap(),
    );
    cache
        .apply_manifest(&arena_proto::CatalogManifest {
            catalog_version: 1,
            games: vec![arena_proto::CatalogGame {
                id: "01JCATALOG".into(),
                rev: 1,
                deleted: false,
                name: Some("Great Game".into()),
                launcher: None,
                category: None,
                age_rating: None,
                signatures: vec![arena_proto::CatalogSignature {
                    exe_name: "g.exe".into(),
                    product_name: "Great".into(),
                    publisher: String::new(),
                    sha256: None,
                }],
                media: vec![],
            }],
        })
        .unwrap();
    let hub = start_with("0.1.0", true, Some(cache)).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let at = T0 - 10_000;
    client
        .request(
            "intent.submit",
            scan_with(
                at,
                vec![
                    exe(r"C:\Games\G.EXE", "great"),
                    exe(r"C:\Tools\other.exe", "Other"),
                ],
            ),
        )
        .await
        .unwrap();
    let matches = matches_of(&hub, &hub.paired_id, at).await;
    assert_eq!(
        matches,
        [arena_proto::GameMatch {
            catalog_id: "01JCATALOG".into(),
            path: r"C:\Games\G.EXE".into()
        }]
    );
    let change = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .into_iter()
        .rfind(|c| c.intent_kind == "game.matched")
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("system", "hub-lan")
    );

    // A newer scan replaces the matches; with no catalogue hit it records none.
    client
        .request(
            "intent.submit",
            scan_with(at + 1, vec![exe(r"C:\x\y.exe", "Y")]),
        )
        .await
        .unwrap();
    assert!(matches_of(&hub, &hub.paired_id, at + 1).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_catalogue_cache_a_scan_still_records_empty_matches() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let at = T0 - 10_000;
    client
        .request(
            "intent.submit",
            scan_with(at, vec![exe(r"C:\Games\g.exe", "G")]),
        )
        .await
        .unwrap();
    assert!(matches_of(&hub, &hub.paired_id, at).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kds_session_may_not_report_a_scan_and_a_paired_one_reads_the_branch_zone() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let mut submit = scan_body(1);
    submit["v"] = 1.into();
    submit["kind"] = "intent.submit".into();
    submit["rid"] = "s1".into();
    send(&mut ws, &submit.to_string()).await;
    assert_eq!(
        error_of(&recv(&mut ws).await.unwrap()).1,
        ErrorCode::Forbidden
    );

    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let frame = client
        .request("settings.get", settings_get_body(&["branch.tz"]))
        .await
        .unwrap();
    let FrameBody::SettingsValues(v) = frame.body else {
        panic!("{frame:?}")
    };
    assert_eq!(
        (v.values[0].key.as_str(), v.values[0].value.as_str()),
        ("branch.tz", "Asia/Kolkata")
    );
    // The zone is the only locale key a kiosk reads.
    let err = client
        .request("settings.get", settings_get_body(&["branch.currency"]))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::Forbidden));
    // And a change to it is pushed to the paired session.
    let mut events = client.events().unwrap();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "branch.tz".into(),
                value: "Asia/Dubai".into(),
            }),
            Some(v.values[0].rev),
        ))
        .await
        .unwrap();
    let pushed = next_settings(&mut events, 2_000).await.expect("pushed");
    assert_eq!(
        (pushed[0].key.as_str(), pushed[0].value.as_str()),
        ("branch.tz", "Asia/Dubai")
    );
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

// ---- Story 6.3: the branch library reaches every paired kiosk as `kiosk.library` ----

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_library_change_reaches_paired_kiosks_within_two_seconds() {
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
    // The kiosk reads the seeded, empty library.
    let frame = client
        .request("settings.get", settings_get_body(&["kiosk.library"]))
        .await
        .unwrap();
    let FrameBody::SettingsValues(v) = frame.body else {
        panic!("{frame:?}")
    };
    assert_eq!(
        (v.values[0].key.as_str(), v.values[0].value.as_str()),
        ("kiosk.library", "[]")
    );

    let started = Instant::now();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::LibraryUpsert(arena_proto::LibraryUpsert {
                id: None,
                entry_kind: "game".into(),
                name: "Counter-Strike 2".into(),
                launcher: "steam".into(),
                executable_path: r"C:\Games\cs2.exe".into(),
                launch_uri: String::new(),
                launch_args: String::new(),
                category: String::new(),
                tags: Vec::new(),
                age_rating: String::new(),
                enabled: true,
                catalog_id: None,
                installs: Vec::new(),
            }),
            None,
        ))
        .await
        .unwrap();
    let mut seen = Vec::new();
    while !(seen.iter().any(|k| k == "kiosk.library")
        && seen.iter().any(|k| k == "kiosk.process_allow"))
    {
        let values = next_settings(&mut events, 2_000)
            .await
            .expect("pushed within 2 s");
        for e in values {
            if e.key == "kiosk.library" {
                let list = arena_proto::parse_kiosk_library(&e.value).unwrap();
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].name, "Counter-Strike 2");
                assert_eq!(e.rev, 2);
            }
            if e.key == "kiosk.process_allow" {
                assert!(e.value.contains(r"C:\\Games\\cs2.exe"), "{}", e.value);
                assert!(e.value.contains("Steam"), "{}", e.value);
            }
            seen.push(e.key);
        }
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    // The kds session hears nothing.
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
            Some("hello".into()),
            None
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
                ..
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

// ---- Story 6.8: player login ----

/// A player with a real argon2id password; returns the player id.
async fn add_player(hub: &TestHub, username: &str, password: &str, name: &str) -> String {
    let id = ulid::Ulid::generate().to_string();
    let form = arena_proto::PlayerForm {
        username: username.into(),
        password: password.into(),
        phone: format!(
            "+9198{:08}",
            id.bytes().map(u64::from).sum::<u64>() % 100_000_000
        ),
        name: name.into(),
        email: String::new(),
        dob: String::new(),
        notes: String::new(),
        tags: vec![],
        consent: true,
        policy_version: "1".into(),
    };
    let hash = arena_core::staff::hash_secret(password).unwrap();
    let sealed = arena_core::players::seal(&PLAYER_KEK, &form, &id, hash).unwrap();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(owner(), IntentPayload::PlayerCreate(sealed), None))
        .await
        .unwrap();
    id
}

async fn login(
    client: &HubClient,
    username: &str,
    password: &str,
) -> arena_proto::PlayerAuthResult {
    let frame = client
        .request(
            "player.authenticate",
            serde_json::json!({ "username": username, "password": password }),
        )
        .await
        .unwrap();
    let FrameBody::PlayerAuthResult(result) = frame.body else {
        panic!("{frame:?}")
    };
    result
}

async fn login_error(client: &HubClient, username: &str, password: &str) -> ErrorCode {
    match client
        .request(
            "player.authenticate",
            serde_json::json!({ "username": username, "password": password }),
        )
        .await
    {
        Err(ClientError::Hub(e)) => e.code,
        other => panic!("{other:?}"),
    }
}

fn device_events(hub: &TestHub, code: &str) -> Vec<arena_store::Row> {
    Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::DeviceEvents)
        .unwrap()
        .into_iter()
        .filter(|r| r["code"] == arena_store::RowValue::Text(code.into()))
        .collect()
}

fn lockout_alerts(hub: &TestHub) -> Vec<arena_store::AlertRow> {
    Store::open_reader(&hub.path)
        .unwrap()
        .alerts()
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "login_lockout")
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_player_logs_in_and_the_login_is_audited_without_the_password() {
    use arena_proto::PlayerAuthResult;
    let hub = start("0.1.0", true).await;
    let id = add_player(&hub, "asha_9", "hunter2-secret", "Asha Rao").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let started = Instant::now();
    // The username is normalised.
    let result = login(&client, " Asha_9 ", "hunter2-secret").await;
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        result,
        PlayerAuthResult::Ok {
            player: arena_proto::PlayerContext {
                player_id: id.clone(),
                name: "Asha Rao".into(),
            }
        }
    );
    let rows = device_events(&hub, "kiosk.player_login");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["detail"],
        arena_store::RowValue::Text(format!("player {id}"))
    );
    assert_eq!(rows[0]["level"], arena_store::RowValue::Text("info".into()));
    assert_eq!(
        rows[0]["device_id"],
        arena_store::RowValue::Text(hub.paired_id.clone())
    );
    let log = log_text();
    assert!(
        !log.contains("hunter2") && !log.contains("Asha Rao"),
        "the credentials and the name are never logged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_password_an_unknown_user_and_an_imported_player_are_all_rejected_alike() {
    use arena_proto::PlayerAuthResult;
    let hub = start("0.1.0", true).await;
    add_player(&hub, "asha", "right-password", "Asha").await;
    let imported = ulid::Ulid::generate().to_string();
    let row = arena_core::players::seal_import(
        &PLAYER_KEK,
        &imported,
        "ravi",
        "+919000000001",
        "Ravi",
        0,
        0,
    )
    .unwrap();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::ImportPlayers(arena_proto::ImportPlayers { rows: vec![row] }),
            None,
        ))
        .await
        .unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for (u, p) in [
        ("asha", "wrong-password"),
        ("nobody", "right-password"),
        ("ravi", "!"),
        ("ravi", "anything"),
    ] {
        assert_eq!(
            login(&client, u, p).await,
            PlayerAuthResult::Rejected,
            "{u}"
        );
    }
    assert!(device_events(&hub, "kiosk.player_login").is_empty());
    assert!(lockout_alerts(&hub).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_wrong_tries_pause_the_kiosk_and_raise_one_lockout_alert() {
    use arena_proto::PlayerAuthResult;
    let hub = start("0.1.0", true).await;
    add_player(&hub, "asha", "right-password", "Asha").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            login(&client, "asha", "nope-nope").await,
            PlayerAuthResult::Rejected
        );
    }
    assert!(lockout_alerts(&hub).is_empty());
    assert_eq!(
        login(&client, "asha", "nope-nope").await,
        PlayerAuthResult::Locked { retry_s: 60 },
        "the fifth wrong try answers locked"
    );
    let alerts = lockout_alerts(&hub);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].device_id.as_deref(), Some(hub.paired_id.as_str()));
    assert!(alerts[0].acked_at.is_none());
    // Paused even for the right password, and nothing is recorded.
    match login(&client, "asha", "right-password").await {
        PlayerAuthResult::Locked { retry_s } => assert!((1..=60).contains(&retry_s)),
        other => panic!("{other:?}"),
    }
    assert!(device_events(&hub, "kiosk.player_login").is_empty());
    assert_eq!(lockout_alerts(&hub).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_correct_login_resets_the_count() {
    use arena_proto::PlayerAuthResult;
    let hub = start("0.1.0", true).await;
    add_player(&hub, "asha", "right-password", "Asha").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            login(&client, "asha", "nope-nope").await,
            PlayerAuthResult::Rejected
        );
    }
    assert!(matches!(
        login(&client, "asha", "right-password").await,
        PlayerAuthResult::Ok { .. }
    ));
    for _ in 0..4 {
        assert_eq!(
            login(&client, "asha", "nope-nope").await,
            PlayerAuthResult::Rejected
        );
    }
    assert!(lockout_alerts(&hub).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_wrong_logins_cannot_get_past_the_limit() {
    use arena_proto::PlayerAuthResult;
    let hub = start("0.1.0", true).await;
    add_player(&hub, "asha", "right-password", "Asha").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let attempts = (0..8).map(|_| login(&client, "asha", "nope-nope"));
    let results = futures_util::future::join_all(attempts).await;
    let rejected = results
        .iter()
        .filter(|r| **r == PlayerAuthResult::Rejected)
        .count();
    assert!(rejected <= 4, "{results:?}");
    assert!(
        results
            .iter()
            .any(|r| matches!(r, PlayerAuthResult::Locked { .. })),
        "{results:?}"
    );
    assert!(matches!(
        login(&client, "asha", "right-password").await,
        PlayerAuthResult::Locked { .. }
    ));
    assert_eq!(lockout_alerts(&hub).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_player_with_a_live_session_on_another_pc_is_busy_and_nothing_is_recorded() {
    use arena_proto::{
        DeviceAssign, GroupUpsert, PlayerAuthResult, RateUpsert, SessionMode, SessionStart,
    };
    let hub = start("0.1.0", true).await;
    let id = add_player(&hub, "asha", "right-password", "Asha").await;
    let w = hub.writer.as_ref().unwrap();
    let (other, _) = register(w, ClientClass::Paired, "PC 3").await;
    let group = w
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(GroupUpsert {
                id: None,
                name: "PC".into(),
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
        IntentPayload::RateUpsert(RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: 12_000,
        }),
        None,
    ))
    .await
    .unwrap();
    w.submit_intent(intent(
        owner(),
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: other.clone(),
            group_id: Some(group),
        }),
        None,
    ))
    .await
    .unwrap();
    w.submit_intent(intent(
        owner(),
        IntentPayload::SessionStart(SessionStart {
            group_with: None,
            pay_from_wallet: false,
            device_id: other,
            mode: SessionMode::Postpaid,
            player_id: Some(id),
            pack_id: None,
            plan_id: None,
        }),
        None,
    ))
    .await
    .unwrap();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // A wrong password is still just rejected: the busy answer needs the right one.
    assert_eq!(
        login(&client, "asha", "nope-nope").await,
        PlayerAuthResult::Rejected
    );
    assert_eq!(
        login(&client, "asha", "right-password").await,
        PlayerAuthResult::Busy {
            device_name: "PC 3".into()
        }
    );
    assert!(device_events(&hub, "kiosk.player_login").is_empty());
    // Busy is not a failure: six of them never lock the kiosk.
    for _ in 0..6 {
        assert!(matches!(
            login(&client, "asha", "right-password").await,
            PlayerAuthResult::Busy { .. }
        ));
    }
    assert!(lockout_alerts(&hub).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_login_frame_is_bad_frame_and_a_hub_that_cannot_check_answers_an_error() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert_eq!(login_error(&client, "asha", "").await, ErrorCode::BadFrame);
    assert_eq!(login_error(&client, "", "pw").await, ErrorCode::BadFrame);
    assert_eq!(
        login_error(&client, "asha", &"p".repeat(2000)).await,
        ErrorCode::BadFrame
    );
    let bare = start_full("0.1.0", true, None, false).await;
    let client = HubClient::connect(&bare.url(), &bare.fingerprint, bare.paired())
        .await
        .unwrap();
    assert_eq!(
        login_error(&client, "asha", "pw").await,
        ErrorCode::Unsupported
    );
    let follower = start("0.1.0", false).await;
    let client = HubClient::connect(&follower.url(), &follower.fingerprint, follower.paired())
        .await
        .unwrap();
    assert_eq!(
        login_error(&client, "asha", "pw").await,
        ErrorCode::NotPrimary
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_paired_session_may_authenticate_a_player() {
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
        r#"{"v":1,"kind":"player.authenticate","rid":"v","username":"asha","password":"pw"}"#,
    )
    .await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
}

// ---- Story 6.9: the plan picker ----

/// Puts the hub's paired PC in a rated group; returns the group id.
async fn rated_pc(hub: &TestHub, rate: i64) -> String {
    use arena_proto::{DeviceAssign, GroupUpsert, RateUpsert};
    let w = hub.writer.as_ref().unwrap();
    let group = w
        .submit_intent(intent(
            owner(),
            IntentPayload::GroupUpsert(GroupUpsert {
                id: None,
                name: "PC".into(),
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
        IntentPayload::RateUpsert(RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: rate,
        }),
        None,
    ))
    .await
    .unwrap();
    w.submit_intent(intent(
        owner(),
        IntentPayload::DeviceAssign(DeviceAssign {
            device_id: hub.paired_id.clone(),
            group_id: Some(group.clone()),
        }),
        None,
    ))
    .await
    .unwrap();
    group
}

fn upi(amount: i64) -> Vec<arena_proto::PaymentRow> {
    vec![arena_proto::PaymentRow {
        method: arena_proto::PaymentMethod::Upi,
        amount_minor: amount,
        reference: Some("1234".into()),
    }]
}

async fn top_up(hub: &TestHub, player: &str, amount: i64) {
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::WalletTopup(arena_proto::WalletTopup {
                player_id: player.into(),
                amount_minor: amount,
                payments: upi(amount),
            }),
            None,
        ))
        .await
        .unwrap();
}

/// Sells the player a 5 h pack for `group`; returns the owned pack id.
async fn own_pack(hub: &TestHub, player: &str, group: &str) -> String {
    use arena_proto::PlanUpsert;
    let w = hub.writer.as_ref().unwrap();
    let plan = w
        .submit_intent(intent(
            owner(),
            IntentPayload::PlanUpsert(PlanUpsert {
                id: None,
                plan_kind: "pack".into(),
                name: "5 h".into(),
                price_minor: 30_000,
                minutes: Some(300),
                validity_days: Some(7),
                window_start_min: None,
                window_end_min: None,
                group_ids: vec![group.to_owned()],
                active: true,
            }),
            None,
        ))
        .await
        .unwrap()
        .changes
        .iter()
        .find(|c| c.tbl == "plans")
        .unwrap()
        .row_id
        .clone();
    w.submit_intent(intent(
        owner(),
        IntentPayload::PlanSell(arena_proto::PlanSell {
            player_id: player.into(),
            plan_id: plan,
            payments: upi(30_000),
            credit_override: None,
            receipt: None,
            business: None,
            coupon: None,
            discount: None,
        }),
        None,
    ))
    .await
    .unwrap();
    Store::open_reader(&hub.path)
        .unwrap()
        .player_packs(player)
        .unwrap()[0]
        .id
        .clone()
}

async fn plans(client: &HubClient) -> Result<arena_proto::PlayerPlansResult, ClientError> {
    match client
        .request("player.plans", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::PlayerPlansResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn start_choice(
    client: &HubClient,
    choice: serde_json::Value,
) -> Result<arena_proto::PlayerStartResult, ClientError> {
    match client
        .request(
            "player.start_session",
            serde_json::json!({ "choice": choice }),
        )
        .await?
        .body
    {
        FrameBody::PlayerStartResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

fn login_required(e: ClientError) -> bool {
    matches!(e, ClientError::Hub(e) if e.message == arena_proto::PLAYER_LOGIN_REQUIRED)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_login_the_picker_frames_answer_login_required() {
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(login_required(plans(&client).await.unwrap_err()));
    assert!(login_required(
        start_choice(&client, serde_json::json!({"mode": "wallet"}))
            .await
            .unwrap_err()
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_logged_in_player_sees_the_options_and_starts_a_pack_and_the_pc_is_told() {
    use arena_proto::{PlayerAuthResult, PlayerChoice, PlayerStartResult};
    let hub = start("0.1.0", true).await;
    let group = rated_pc(&hub, 6_000).await;
    let player = add_player(&hub, "asha", "right-password", "Asha").await;
    top_up(&hub, &player, 4_000).await;
    let pack = own_pack(&hub, &player, &group).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    assert!(matches!(
        login(&client, "asha", "right-password").await,
        PlayerAuthResult::Ok { .. }
    ));
    let r = plans(&client).await.unwrap();
    assert_eq!(
        r.options
            .iter()
            .map(|o| o.choice.clone())
            .collect::<Vec<_>>(),
        [
            PlayerChoice::Pack {
                pack_id: pack.clone()
            },
            PlayerChoice::Wallet
        ]
    );
    let wallet = r.wallet.unwrap();
    assert_eq!((wallet.balance_minor, wallet.rate_minor), (4_000, 6_000));
    assert_eq!(r.options[0].minutes_left, Some(300));
    assert_eq!(
        start_choice(
            &client,
            serde_json::json!({"mode": "pack", "pack_id": pack})
        )
        .await
        .unwrap(),
        PlayerStartResult::Ok
    );
    // The PC is unlocked: session.started reaches it.
    let started = loop {
        let ev = tokio::time::timeout(Duration::from_secs(3), events.next_event())
            .await
            .expect("session.started within 3 s")
            .unwrap();
        if let HubEvent::SessionStarted { session, .. } = ev {
            break session;
        }
    };
    assert!(!started.session_id.is_empty());
    let reader = Store::open_reader(&hub.path).unwrap();
    let sessions = reader.live_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].player_id.as_deref(), Some(player.as_str()));
    assert_eq!(sessions[0].mode, "pack");
    // The login is spent: another start needs a new login.
    assert!(login_required(plans(&client).await.unwrap_err()));
    let changes = reader.changes().unwrap();
    let change = changes.iter().rev().find(|c| c.tbl == "sessions").unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_start_keeps_the_player_on_the_picker_with_the_typed_reason() {
    use arena_proto::PlayerStartResult;
    let hub = start("0.1.0", true).await;
    let group = rated_pc(&hub, 6_000).await;
    let player = add_player(&hub, "asha", "right-password", "Asha").await;
    top_up(&hub, &player, 100).await;
    let _ = own_pack(&hub, &player, &group).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    login(&client, "asha", "right-password").await;
    match start_choice(&client, serde_json::json!({"mode": "wallet"}))
        .await
        .unwrap()
    {
        PlayerStartResult::Refused { code, message } => {
            assert_eq!(code, ErrorCode::InsufficientBalance);
            assert!(message.contains("Balance"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    match start_choice(
        &client,
        serde_json::json!({"mode": "pack", "pack_id": "NOPE"}),
    )
    .await
    .unwrap()
    {
        PlayerStartResult::Refused { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("{other:?}"),
    }
    // Still logged in, nothing started.
    assert_eq!(plans(&client).await.unwrap().options.len(), 2);
    assert!(
        Store::open_reader(&hub.path)
            .unwrap()
            .live_sessions()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_is_paid_from_the_wallet_when_the_player_starts_it() {
    use arena_proto::{PlanUpsert, PlayerStartResult};
    let hub = start("0.1.0", true).await;
    let group = rated_pc(&hub, 6_000).await;
    let player = add_player(&hub, "asha", "right-password", "Asha").await;
    top_up(&hub, &player, 25_000).await;
    let plan = hub
        .writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::PlanUpsert(PlanUpsert {
                id: None,
                plan_kind: "block".into(),
                name: "All day".into(),
                price_minor: 20_000,
                minutes: None,
                validity_days: None,
                window_start_min: Some(0),
                window_end_min: Some(1_439),
                group_ids: vec![group],
                active: true,
            }),
            None,
        ))
        .await
        .unwrap()
        .changes
        .iter()
        .find(|c| c.tbl == "plans")
        .unwrap()
        .row_id
        .clone();
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    login(&client, "asha", "right-password").await;
    let r = plans(&client).await.unwrap();
    let block = r
        .options
        .iter()
        .find(|o| o.price_minor == Some(20_000))
        .unwrap();
    assert!(block.selectable);
    assert_eq!(
        start_choice(
            &client,
            serde_json::json!({"mode": "block", "plan_id": plan})
        )
        .await
        .unwrap(),
        PlayerStartResult::Ok
    );
    let reader = Store::open_reader(&hub.path).unwrap();
    assert_eq!(reader.wallet_balance(&player).unwrap(), 5_000);
    assert_eq!(reader.live_sessions().unwrap()[0].mode, "block");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logout_forgets_the_player_and_a_login_is_good_for_its_connection_only() {
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    add_player(&hub, "asha", "right-password", "Asha").await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    login(&client, "asha", "right-password").await;
    assert!(plans(&client).await.is_ok());
    // Another connection of the same device cannot use the record.
    let other = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(login_required(plans(&other).await.unwrap_err()));
    // Logging out on the other connection leaves the first one's record alone.
    let ack = other
        .request("player.logout", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(ack.body, FrameBody::Ack);
    assert!(plans(&client).await.is_ok());
    let ack = client
        .request("player.logout", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(ack.body, FrameBody::Ack);
    assert!(login_required(plans(&client).await.unwrap_err()));
    // A new login brings it back; closing the connection drops it for good.
    login(&client, "asha", "right-password").await;
    assert!(plans(&client).await.is_ok());
    drop(client);
    let again = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(login_required(plans(&again).await.unwrap_err()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_picker_frames_are_for_paired_sessions_only_and_session_start_is_out_of_scope() {
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
    for body in [
        r#"{"v":1,"kind":"player.plans","rid":"a"}"#,
        r#"{"v":1,"kind":"player.start_session","rid":"b","choice":{"mode":"wallet"}}"#,
        r#"{"v":1,"kind":"player.logout","rid":"c"}"#,
    ] {
        send(&mut ws, body).await;
        let frame = recv(&mut ws).await.unwrap();
        assert_eq!(error_of(&frame).1, ErrorCode::Forbidden, "{body}");
    }
    // A paired kiosk cannot send session.start itself.
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let start = intent(
        Actor {
            kind: ActorKind::Device,
            id: hub.paired_id.clone(),
        },
        IntentPayload::SessionStart(arena_proto::SessionStart {
            device_id: hub.paired_id.clone(),
            mode: arena_proto::SessionMode::Wallet,
            player_id: Some("P".into()),
            pack_id: None,
            plan_id: None,
            group_with: None,
            pay_from_wallet: false,
        }),
        None,
    );
    match client.submit(start).await {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_follower_answers_the_picker_frames_with_an_error() {
    let follower = start("0.1.0", false).await;
    let client = HubClient::connect(&follower.url(), &follower.fingerprint, follower.paired())
        .await
        .unwrap();
    match plans(&client).await {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::NotPrimary),
        other => panic!("{other:?}"),
    }
}

// ---- Story 6.15: Call staff ----

fn call_alerts(hub: &TestHub) -> Vec<arena_store::AlertRow> {
    Store::open_reader(&hub.path)
        .unwrap()
        .alerts()
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "call_staff")
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn call_staff_raises_one_alert_for_the_device_and_answers_ack_each_time() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let ack = client
        .request("device.call_staff", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(ack.body, FrameBody::Ack);
    let alerts = call_alerts(&hub);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].device_id.as_deref(), Some(hub.paired_id.as_str()));
    assert!(alerts[0].acked_at.is_none());
    // A call already open for this PC: no second alert, same answer.
    let again = client
        .request("device.call_staff", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(again.body, FrameBody::Ack);
    assert_eq!(call_alerts(&hub).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn call_staff_is_for_paired_sessions_and_a_follower_refuses_it() {
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
    send(&mut ws, r#"{"v":1,"kind":"device.call_staff","rid":"a"}"#).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
    assert!(call_alerts(&hub).is_empty());

    let follower = start("0.1.0", false).await;
    let client = HubClient::connect(&follower.url(), &follower.fingerprint, follower.paired())
        .await
        .unwrap();
    match client
        .request("device.call_staff", serde_json::Value::Null)
        .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::NotPrimary),
        other => panic!("{other:?}"),
    }
}

// ---- Story 6.10: sign-up at the kiosk ----

fn signup_body(
    username: &str,
    password: &str,
    phone: &str,
    name: &str,
    consent: bool,
) -> serde_json::Value {
    serde_json::json!({
        "username": username,
        "password": password,
        "phone": phone,
        "name": name,
        "consent": consent,
    })
}

async fn signup(
    client: &HubClient,
    body: serde_json::Value,
) -> Result<arena_proto::PlayerSignupResult, ClientError> {
    match client.request("player.signup", body).await?.body {
        FrameBody::PlayerSignupResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

fn refused_with(r: arena_proto::PlayerSignupResult) -> (ErrorCode, Option<String>) {
    match r {
        arena_proto::PlayerSignupResult::Refused { code, field } => (code, field),
        other => panic!("{other:?}"),
    }
}

fn players_written(hub: &TestHub) -> usize {
    Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::Players)
        .unwrap()
        .len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_player_signs_up_is_logged_in_and_the_log_shows_the_device_and_no_personal_data() {
    use arena_proto::{PLAYER_POLICY_VERSION, PlayerAuthResult, PlayerSignupResult};
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let started = Instant::now();
    let result = signup(
        &client,
        signup_body(
            " Asha_9 ",
            "hunter2-secret",
            "+91 98765-43210",
            "Asha Rao",
            true,
        ),
    )
    .await
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    let PlayerSignupResult::Ok { player } = result else {
        panic!("{result:?}")
    };
    assert_eq!(player.name, "Asha Rao");
    let reader = Store::open_reader(&hub.path).unwrap();
    // Player, wallet and a granted transactional consent row with the Hub's policy version.
    let players = reader.rows(arena_store::Table::Players).unwrap();
    assert_eq!(players.len(), 1);
    assert_eq!(
        players[0]["player_id"],
        arena_store::RowValue::Text(player.player_id.clone())
    );
    assert_eq!(
        players[0]["username"],
        arena_store::RowValue::Text("asha_9".into())
    );
    assert_eq!(reader.wallet_balance(&player.player_id).unwrap(), 0);
    let consents = reader.rows(arena_store::Table::Consents).unwrap();
    assert_eq!(consents.len(), 1);
    assert_eq!(
        consents[0]["purpose"],
        arena_store::RowValue::Text("transactional".into())
    );
    assert_eq!(consents[0]["granted"], arena_store::RowValue::Integer(1));
    assert_eq!(
        consents[0]["policy_version"],
        arena_store::RowValue::Text(PLAYER_POLICY_VERSION.into())
    );
    // The change log shows the kiosk device and the player id, and carries no name or phone.
    let changes = reader.changes().unwrap();
    let change = changes.iter().rev().find(|c| c.tbl == "players").unwrap();
    assert_eq!(
        (
            change.actor_kind.as_str(),
            change.actor_id.as_str(),
            change.row_id.as_str()
        ),
        ("device", hub.paired_id.as_str(), player.player_id.as_str())
    );
    let log: Vec<u8> = changes.into_iter().flat_map(|c| c.row).collect();
    let log = String::from_utf8_lossy(&log).into_owned();
    for plain in ["Asha Rao", "9876543210", "hunter2"] {
        assert!(!log.contains(plain), "{plain}");
    }
    assert!(device_events(&hub, "kiosk.player_login").is_empty());
    let text = log_text();
    for plain in ["hunter2", "Asha Rao", "9876543210", "asha_9"] {
        assert!(
            !text.contains(plain),
            "{plain} must never be logged:\n{text}"
        );
    }
    // Logged in at once: the picker works on this connection with no further login.
    let options = plans(&client).await.unwrap();
    assert!(options.wallet.is_some());
    // The password works for a later login.
    assert!(matches!(
        login(&client, "asha_9", "hunter2-secret").await,
        PlayerAuthResult::Ok { .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_up_without_the_consent_box_is_refused_and_writes_nothing() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let before = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .len();
    let r = signup(
        &client,
        signup_body("asha", "secret1", "+919876543210", "Asha", false),
    )
    .await
    .unwrap();
    assert_eq!(
        refused_with(r),
        (ErrorCode::ValidationFailed, Some("consent".into()))
    );
    assert_eq!(players_written(&hub), 0);
    assert_eq!(
        Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len(),
        before
    );
    // Not logged in either.
    assert!(login_required(plans(&client).await.unwrap_err()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_taken_username_or_phone_and_bad_fields_come_back_typed_and_write_nothing() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    assert!(matches!(
        signup(
            &client,
            signup_body("asha", "secret1", "+91 98765 43210", "Asha", true)
        )
        .await
        .unwrap(),
        arena_proto::PlayerSignupResult::Ok { .. }
    ));
    let before = Store::open_reader(&hub.path)
        .unwrap()
        .changes()
        .unwrap()
        .len();
    let cases = [
        (
            signup_body("ASHA", "secret1", "+919000000002", "Other", true),
            ErrorCode::UsernameTaken,
            "username",
        ),
        (
            signup_body("other", "secret1", "+91-98765-43210", "Other", true),
            ErrorCode::PhoneTaken,
            "phone",
        ),
        (
            signup_body("ab", "secret1", "+919000000002", "Other", true),
            ErrorCode::ValidationFailed,
            "username",
        ),
        (
            signup_body("other", "12345", "+919000000002", "Other", true),
            ErrorCode::ValidationFailed,
            "password",
        ),
        (
            signup_body("other", "secret1", "+12", "Other", true),
            ErrorCode::ValidationFailed,
            "phone",
        ),
        (
            signup_body("other", "secret1", "+919000000002", "   ", true),
            ErrorCode::ValidationFailed,
            "name",
        ),
    ];
    for (body, code, field) in cases {
        let r = signup(&client, body).await.unwrap();
        assert_eq!(refused_with(r), (code, Some(field.to_owned())), "{field}");
    }
    assert_eq!(players_written(&hub), 1);
    assert_eq!(
        Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len(),
        before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_oversized_sign_up_frame_is_bad_frame_and_a_hub_that_cannot_create_answers_an_error() {
    let hub = start("0.1.0", true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    match signup(
        &client,
        signup_body("asha", &"p".repeat(2000), "+919876543210", "Asha", true),
    )
    .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::BadFrame),
        other => panic!("{other:?}"),
    }
    let bare = start_full("0.1.0", true, None, false).await;
    let client = HubClient::connect(&bare.url(), &bare.fingerprint, bare.paired())
        .await
        .unwrap();
    match signup(
        &client,
        signup_body("asha", "secret1", "+919876543210", "Asha", true),
    )
    .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::Unsupported),
        other => panic!("{other:?}"),
    }
    let follower = start("0.1.0", false).await;
    let client = HubClient::connect(&follower.url(), &follower.fingerprint, follower.paired())
        .await
        .unwrap();
    match signup(
        &client,
        signup_body("asha", "secret1", "+919876543210", "Asha", true),
    )
    .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::NotPrimary),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_paired_session_may_sign_a_player_up_and_a_kiosk_cannot_send_player_create() {
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
        r#"{"v":1,"kind":"player.signup","rid":"v","username":"asha","password":"secret1","phone":"+919876543210","name":"Asha","consent":true}"#,
    )
    .await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
    assert_eq!(players_written(&hub), 0);

    // A paired kiosk cannot send player.create itself (the paired scope is unchanged).
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let form = arena_proto::PlayerForm {
        username: "asha".into(),
        password: "secret1".into(),
        phone: "+919876543210".into(),
        name: "Asha".into(),
        email: String::new(),
        dob: String::new(),
        notes: String::new(),
        tags: vec![],
        consent: true,
        policy_version: "1".into(),
    };
    let sealed = arena_core::players::seal(
        &PLAYER_KEK,
        &form,
        &ulid::Ulid::generate().to_string(),
        arena_core::staff::hash_secret("secret1").unwrap(),
    )
    .unwrap();
    let create = intent(
        Actor {
            kind: ActorKind::Device,
            id: hub.paired_id.clone(),
        },
        IntentPayload::PlayerCreate(sealed),
        None,
    );
    match client.submit(create).await {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    assert_eq!(players_written(&hub), 0);
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

async fn account_frame(
    client: &HubClient,
) -> Result<arena_proto::PlayerAccountResult, ClientError> {
    match client
        .request("player.account", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::PlayerAccountResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn unlock_frame(client: &HubClient, password: &str) -> arena_proto::PlayerUnlockResult {
    match client
        .request("player.unlock", serde_json::json!({ "password": password }))
        .await
        .unwrap()
        .body
    {
        FrameBody::PlayerUnlockResult(r) => r,
        other => panic!("{other:?}"),
    }
}

async fn end_frame(client: &HubClient) -> arena_proto::PlayerEndResult {
    match client
        .request("player.end_session", serde_json::Value::Null)
        .await
        .unwrap()
        .body
    {
        FrameBody::PlayerEndResult(r) => r,
        other => panic!("{other:?}"),
    }
}

/// The reason of the next lock command pushed to the PC.
async fn next_lock(events: &mut arena_kiosk_core::EventStream) -> Option<arena_proto::LockReason> {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(3), events.next_event())
            .await
            .expect("a lock command within 3 s")
            .unwrap();
        if let HubEvent::PcCommand {
            action: arena_proto::PcAction::Lock,
            reason,
            ..
        } = ev
        {
            return reason;
        }
    }
}

/// A logged-in player with a pack and some wallet, running a pack session on the paired PC.
async fn playing(hub: &TestHub) -> (HubClient, String) {
    use arena_proto::PlayerStartResult;
    let group = rated_pc(hub, 6_000).await;
    let player = add_player(hub, "asha", "right-password", "Asha").await;
    top_up(hub, &player, 4_000).await;
    let pack = own_pack(hub, &player, &group).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    login(&client, "asha", "right-password").await;
    assert_eq!(
        start_choice(
            &client,
            serde_json::json!({"mode": "pack", "pack_id": pack})
        )
        .await
        .unwrap(),
        PlayerStartResult::Ok
    );
    (client, player)
}

async fn extend_options_frame(
    client: &HubClient,
) -> Result<arena_proto::PlayerExtendOptions, ClientError> {
    match client
        .request("player.extend_options", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::PlayerExtendOptionsResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn extend_frame(
    client: &HubClient,
    body: serde_json::Value,
) -> Result<arena_proto::PlayerExtendResult, ClientError> {
    match client.request("player.extend", body).await?.body {
        FrameBody::PlayerExtendResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn start_postpaid(hub: &TestHub, player: Option<String>) {
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::SessionStart(arena_proto::SessionStart {
                group_with: None,
                pay_from_wallet: false,
                device_id: hub.paired_id.clone(),
                mode: arena_proto::SessionMode::Postpaid,
                player_id: player,
                pack_id: None,
                plan_id: None,
            }),
            None,
        ))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_account_shows_the_balance_and_owned_pack_hours_of_the_session_player() {
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    let account = account_frame(&client).await.unwrap();
    assert_eq!(account.balance_minor, 4_000);
    assert_eq!(account.packs.len(), 1);
    assert_eq!(account.packs[0].name, "5 h");
    assert_eq!(account.packs[0].minutes_left, Some(300));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_account_of_a_guest_or_an_empty_pc_is_not_found() {
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let ClientError::Hub(e) = account_frame(&client).await.unwrap_err() else {
        panic!("a hub refusal")
    };
    assert_eq!(e.code, ErrorCode::NotFound);
    start_postpaid(&hub, None).await;
    let ClientError::Hub(e) = account_frame(&client).await.unwrap_err() else {
        panic!("a hub refusal")
    };
    assert_eq!(e.code, ErrorCode::NotFound);
    // A guest has no password to unlock with, and nothing counts toward a pause.
    for _ in 0..6 {
        assert_eq!(
            unlock_frame(&client, "anything").await,
            arena_proto::PlayerUnlockResult::Rejected
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_lock_screen_takes_the_own_password_and_five_wrong_tries_pause_it_for_a_minute() {
    use arena_proto::PlayerUnlockResult as R;
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    assert_eq!(unlock_frame(&client, "wrong-one").await, R::Rejected);
    assert_eq!(unlock_frame(&client, "right-password").await, R::Ok);
    // The streak was cleared by the right password: four wrong, then the fifth pauses.
    for _ in 0..4 {
        assert_eq!(unlock_frame(&client, "wrong-one").await, R::Rejected);
    }
    assert_eq!(
        unlock_frame(&client, "wrong-one").await,
        R::Locked { retry_s: 60 }
    );
    // While paused even the right password is not checked.
    assert!(matches!(
        unlock_frame(&client, "right-password").await,
        R::Locked { retry_s } if retry_s > 0
    ));
    let alerts = Store::open_reader(&hub.path).unwrap().alerts().unwrap();
    assert!(
        alerts
            .iter()
            .any(|a| a.device_id.as_deref() == Some(hub.paired_id.as_str()))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logging_out_of_a_prepaid_session_ends_it_and_sends_the_pc_back_to_attract() {
    use arena_proto::{LockReason, PlayerEndResult};
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    let mut events = client.events().unwrap();
    assert_eq!(end_frame(&client).await, PlayerEndResult::Ok);
    assert_eq!(next_lock(&mut events).await, Some(LockReason::LoggedOut));
    let reader = Store::open_reader(&hub.path).unwrap();
    assert!(reader.live_sessions().unwrap().is_empty());
    let changes = reader.changes().unwrap();
    let ended = changes.iter().rev().find(|c| c.tbl == "sessions").unwrap();
    assert_eq!(
        (ended.actor_kind.as_str(), ended.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    // Nothing is left to end.
    assert!(matches!(
        end_frame(&client).await,
        PlayerEndResult::Refused {
            code: ErrorCode::NotFound,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logging_out_of_a_postpaid_session_bills_it_and_locks_to_pay_at_counter() {
    use arena_proto::{LockReason, PlayerEndResult};
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let player = add_player(&hub, "asha", "right-password", "Asha").await;
    start_postpaid(&hub, Some(player)).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    assert_eq!(end_frame(&client).await, PlayerEndResult::Ok);
    assert_eq!(next_lock(&mut events).await, Some(LockReason::PayAtCounter));
    let reader = Store::open_reader(&hub.path).unwrap();
    assert!(reader.live_sessions().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_account_frames_are_for_paired_sessions_and_a_follower_refuses_them() {
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
    for line in [
        r#"{"v":1,"kind":"player.account","rid":"a"}"#,
        r#"{"v":1,"kind":"player.end_session","rid":"b"}"#,
        r#"{"v":1,"kind":"player.unlock","rid":"c","password":"x"}"#,
        r#"{"v":1,"kind":"player.extend_options","rid":"d"}"#,
        r#"{"v":1,"kind":"games.most_played","rid":"g"}"#,
        r#"{"v":1,"kind":"player.extend","rid":"e","minutes":30,"source":"wallet"}"#,
    ] {
        send(&mut ws, line).await;
        let frame = recv(&mut ws).await.unwrap();
        assert_eq!(error_of(&frame).1, ErrorCode::Forbidden, "{line}");
    }

    let follower = start("0.1.0", false).await;
    let client = HubClient::connect(&follower.url(), &follower.fingerprint, follower.paired())
        .await
        .unwrap();
    for (kind, body) in [
        ("player.account", serde_json::Value::Null),
        ("player.end_session", serde_json::Value::Null),
        ("player.unlock", serde_json::json!({ "password": "x" })),
        ("player.extend_options", serde_json::Value::Null),
        (
            "player.extend",
            serde_json::json!({ "minutes": 30, "source": "wallet" }),
        ),
    ] {
        match client.request(kind, body).await {
            Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::NotPrimary, "{kind}"),
            other => panic!("{kind}: {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extending_from_the_kiosk_quotes_live_charges_the_wallet_and_pushes_the_new_end() {
    use arena_proto::PlayerExtendResult as R;
    let hub = start("0.1.0", true).await;
    let (client, player) = playing(&hub).await;
    let options = extend_options_frame(&client).await.unwrap();
    let quoted: Vec<_> = options
        .quotes
        .iter()
        .map(|q| (q.minutes, q.price_minor))
        .collect();
    assert_eq!(quoted, [(30, 3_000), (60, 6_000), (120, 12_000)]);
    assert_eq!(options.wallet_minor, 4_000);
    assert!(
        options.packs.is_empty(),
        "the session's own pack is not a source"
    );
    assert_eq!(options.credit_headroom_minor, None);
    let reader = Store::open_reader(&hub.path).unwrap();
    let before = reader.live_sessions().unwrap()[0].ends_at.unwrap();
    let mut events = client.events().unwrap();
    assert_eq!(
        extend_frame(
            &client,
            serde_json::json!({"minutes": 30, "source": "wallet"})
        )
        .await
        .unwrap(),
        R::Ok
    );
    let after = reader.live_sessions().unwrap()[0].ends_at.unwrap();
    assert_eq!(after - before, 30 * 60_000);
    assert_eq!(reader.wallet_balance(&player).unwrap(), 1_000);
    // The PC is told, like a staff extend: the session screen again, with the new end.
    let screen = loop {
        let ev = tokio::time::timeout(Duration::from_secs(3), events.next_event())
            .await
            .expect("a session screen within 3 s")
            .unwrap();
        if let HubEvent::SessionStarted { session, .. } = ev {
            break session;
        }
    };
    assert!(screen.remaining_ms.is_some());
    let changes = reader.changes().unwrap();
    let order = changes.iter().rev().find(|c| c.tbl == "orders").unwrap();
    assert_eq!(
        (order.actor_kind.as_str(), order.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    // Short of balance: a typed refusal that names the balance, and nothing is written.
    let R::Refused { code, message } = extend_frame(
        &client,
        serde_json::json!({"minutes": 60, "source": "wallet"}),
    )
    .await
    .unwrap() else {
        panic!("a refusal")
    };
    assert_eq!(code, ErrorCode::InsufficientBalance);
    assert_eq!(message, "Balance ₹10.00. Top up at counter.");
    assert_eq!(reader.wallet_balance(&player).unwrap(), 1_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guest_or_postpaid_session_or_an_empty_pc_cannot_extend() {
    use arena_proto::PlayerExtendResult as R;
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let wallet = serde_json::json!({"minutes": 30, "source": "wallet"});
    let ClientError::Hub(e) = extend_options_frame(&client).await.unwrap_err() else {
        panic!("a hub refusal")
    };
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(matches!(
        extend_frame(&client, wallet.clone()).await.unwrap(),
        R::Refused {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    start_postpaid(&hub, None).await;
    let ClientError::Hub(e) = extend_options_frame(&client).await.unwrap_err() else {
        panic!("a hub refusal")
    };
    assert_eq!(e.code, ErrorCode::ValidationFailed);
    assert!(matches!(
        extend_frame(&client, wallet).await.unwrap(),
        R::Refused {
            code: ErrorCode::ValidationFailed,
            ..
        }
    ));
    // A step that is not offered never reaches the Hub's rules.
    let ClientError::Hub(e) = extend_frame(
        &client,
        serde_json::json!({"minutes": 45, "source": "wallet"}),
    )
    .await
    .unwrap_err() else {
        panic!("a hub refusal")
    };
    assert_eq!(e.code, ErrorCode::BadFrame);
}

// ---- Story 6.30: shop.menu and shop.image ----

/// A JPEG header the Hub accepts as a product photo (64 x 64, not a decodable picture).
const SHOP_PHOTO: &str = "data:image/jpeg;base64,/9j/4AAEAAD/wAALCABAAEABAREA";

async fn shop_menu_frame(client: &HubClient) -> Result<arena_proto::ShopMenu, ClientError> {
    match client
        .request("shop.menu", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::ShopMenuResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn shop_image_frame(
    client: &HubClient,
    id: &str,
) -> Result<arena_proto::ShopImageResult, ClientError> {
    match client
        .request("shop.image", serde_json::json!({ "id": id }))
        .await?
        .body
    {
        FrameBody::ShopImageResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn shop_product(hub: &TestHub, name: &str, photo: &str, active: bool) -> String {
    let applied = hub
        .writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: name.into(),
                category: "Drinks".into(),
                price_minor: 4_000,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: false,
                image: photo.into(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        ))
        .await
        .unwrap();
    applied.changes[0].row_id.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_free_branch_answers_the_shop_with_no_menu_and_no_photos() {
    let hub = start("0.1.0", true).await;
    let cola = shop_product(&hub, "Cola", SHOP_PHOTO, true).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let menu = shop_menu_frame(&client).await.unwrap();
    assert!(!menu.enabled);
    assert!(menu.items.is_empty());
    assert_eq!(shop_image_frame(&client, &cola).await.unwrap().image, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_basic_branch_serves_the_shop_menu_and_one_photo_at_a_time_and_a_change_shows_on_the_next_ask()
 {
    let hub = start("0.1.0", true).await;
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            system(),
            IntentPayload::LicenceApply(arena_proto::LicenceApply {
                standing: arena_proto::LicenceStanding::Verified,
                clock_wrong: false,
                tier: arena_proto::Tier::Basic,
                flags: "kitchen,kiosk_orders".into(),
                device_cap: 10,
                user_cap: 10,
            }),
            None,
        ))
        .await
        .unwrap();
    let cola = shop_product(&hub, "Cola", SHOP_PHOTO, true).await;
    let plain = shop_product(&hub, "Water", "", true).await;
    let retired = shop_product(&hub, "Retired", SHOP_PHOTO, false).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();

    let menu = shop_menu_frame(&client).await.unwrap();
    assert!(menu.enabled);
    let seen: Vec<_> = menu
        .items
        .iter()
        .map(|i| (i.name.as_str(), i.available, i.has_image))
        .collect();
    assert_eq!(seen, [("Cola", true, true), ("Water", true, false)]);
    // The menu frame carries no photo.
    assert!(!serde_json::to_string(&menu).unwrap().contains("base64"));
    assert_eq!(
        shop_image_frame(&client, &cola).await.unwrap().image,
        Some(SHOP_PHOTO.to_owned())
    );
    assert_eq!(shop_image_frame(&client, &plain).await.unwrap().image, None);
    assert_eq!(
        shop_image_frame(&client, &retired).await.unwrap().image,
        None
    );

    // The Hub deactivates the cola: the next ask no longer lists it.
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: Some(cola.clone()),
                name: "Cola".into(),
                category: "Drinks".into(),
                price_minor: 4_000,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: false,
                image: SHOP_PHOTO.into(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: false,
                bundle: false,
                components: Vec::new(),
            }),
            None,
        ))
        .await
        .unwrap();
    let menu = shop_menu_frame(&client).await.unwrap();
    assert_eq!(menu.items.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_shop_frames_are_for_paired_sessions_and_a_bad_image_ask_is_refused() {
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
    for line in [
        r#"{"v":1,"kind":"shop.menu","rid":"a"}"#,
        r#"{"v":1,"kind":"shop.image","rid":"b","id":"P1"}"#,
    ] {
        send(&mut ws, line).await;
        let frame = recv(&mut ws).await.unwrap();
        assert_eq!(error_of(&frame).1, ErrorCode::Forbidden, "{line}");
    }

    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    match client
        .request("shop.image", serde_json::json!({ "id": "" }))
        .await
    {
        Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::BadFrame),
        other => panic!("{other:?}"),
    }
}

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
    kds_id: String,
    kds_token: String,
    kds_pairing: Arc<arena_api::pairing::Pairing>,
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

/// Pairs a kitchen display the way the Hub does (Story 7.6): `kds.pair` as the system actor on a
/// Basic branch, created by a Manager. The branch goes back to Free afterwards. Returns the
/// display id and its `kds` token.
async fn register_display(
    w: &WriterHandle,
    path: &std::path::Path,
    name: &str,
) -> (String, String) {
    let flags = |value: &'static str| {
        let sql = format!("UPDATE settings SET value = '{value}' WHERE key = 'licence.flags'");
        w.submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
    };
    flags("kitchen").await.unwrap();
    let manager = match Store::open_reader(path)
        .unwrap()
        .rows(arena_store::Table::Staff)
        .unwrap()
        .iter()
        .find_map(|s| match &s["staff_id"] {
            arena_store::RowValue::Text(t) => Some(t.clone()),
            _ => None,
        }) {
        Some(id) => id,
        None => {
            let role_id = Store::open_reader(path)
                .unwrap()
                .rows(arena_store::Table::Roles)
                .unwrap()
                .into_iter()
                .find(|r| r["name"] == arena_store::RowValue::Text("Manager".into()))
                .and_then(|r| match &r["role_id"] {
                    arena_store::RowValue::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .unwrap();
            w.submit_intent(intent(
                owner(),
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: None,
                    login: "mgr".into(),
                    display_name: "Manager".into(),
                    role_id,
                    password_hash: Some(arena_core::staff::hash_secret("pw").unwrap()),
                    pin_hash: None,
                    active: true,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
                .row_id
                .clone()
        }
    };
    let secret = arena_core::devices::new_paired_token_secret().unwrap();
    let applied = w
        .submit_intent(intent(
            system(),
            IntentPayload::KdsPair(arena_proto::KdsPair {
                name: name.into(),
                token_hash: hash_token_secret(&secret).unwrap(),
                created_by: manager.clone(),
            }),
            None,
        ))
        .await
        .unwrap();
    flags("").await.unwrap();
    // The staff seat the display needed is given back, so tests that count staff see none of it.
    let sql = format!("DELETE FROM staff WHERE staff_id = '{manager}'");
    w.submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
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
    start_inner(version, primary, media, player_kek, None).await
}

/// [`start`] serving the Kitchen display app from `dir` (Story 7.6).
async fn start_kitchen(dir: PathBuf) -> TestHub {
    start_inner("0.1.0", true, None, true, Some(dir)).await
}

async fn start_inner(
    version: &str,
    primary: bool,
    media: Option<Arc<arena_core::media_cache::MediaCache>>,
    player_kek: bool,
    kitchen_dir: Option<PathBuf>,
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
    let (kds_id, kds_token) = register_display(&writer, &path, "Pass").await;
    let writer = if primary {
        Some(writer)
    } else {
        writer.close().await;
        status_tx.send_modify(|s| s.role = Role::Follower);
        None
    };

    let cert = HubCert::generate(None).unwrap();
    status_tx.send_modify(|s| s.tls_fingerprint = Some(cert.fingerprint.clone()));
    let kds_pairing = Arc::new(arena_api::pairing::Pairing::new(clock.clone()).unwrap());
    let mut lan_hub = LanHub::new(
        writer.clone(),
        status_rx,
        clock,
        Arc::new(DeviceDirectory::open(&path).unwrap()),
        cert.server_config().unwrap(),
        version,
    )
    .with_staff(Arc::new(arena_core::StaffDirectory::open(&path).unwrap()))
    .with_kds_pairing(Arc::clone(&kds_pairing))
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
    if let Some(dir) = kitchen_dir {
        lan_hub = lan_hub.with_kitchen_dir(dir);
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
        kds_id,
        kds_token,
        kds_pairing,
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
    // The Hub states the epoch of its lease, which a device keeps as its floor (Story 9.21).
    assert_eq!(ok.epoch.as_deref(), Some(format!("1:{HUB_ID}").as_str()));
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
    // Plain HTTP answers only the kitchen, the socket, `/hub-info` and `/console/<tv>`: any other
    // path is FORBIDDEN (Story 7.6).
    for path in [
        "/nope",
        "/",
        "/console/",
        "/ws/extra",
        "/media/abc",
        "/pair",
    ] {
        let resp = http_get(hub.web, path).await;
        assert!(resp.starts_with("HTTP/1.1 403"), "{path}: {resp}");
        assert!(resp.contains(r#""code":"FORBIDDEN""#), "{path}: {resp}");
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
    assert!(out.starts_with("HTTP/1.1 403"), "{out}");
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

async fn account_page_frame(
    client: &HubClient,
) -> Result<arena_proto::PlayerAccountPage, ClientError> {
    match client
        .request("player.account_page", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::PlayerAccountPageResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

async fn change_password_frame(
    client: &HubClient,
    current: &str,
    new: &str,
) -> arena_proto::PlayerPasswordChangeResult {
    match client
        .request(
            "player.password_change",
            serde_json::json!({ "current": current, "new": new }),
        )
        .await
        .unwrap()
        .body
    {
        FrameBody::PlayerPasswordChangeResult(r) => r,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_account_page_shows_details_with_a_masked_phone_wallet_and_packs() {
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    let page = account_page_frame(&client).await.unwrap();
    assert_eq!(
        (page.name.as_str(), page.username.as_str()),
        ("Asha", "asha")
    );
    assert_eq!(
        page.phone_masked
            .chars()
            .filter(char::is_ascii_digit)
            .count(),
        4
    );
    assert!(page.phone_masked.contains('•'), "{}", page.phone_masked);
    assert_eq!(page.balance_minor, 4_000);
    assert_eq!(page.packs.len(), 1);
    assert_eq!(
        (
            page.packs[0].name.as_str(),
            page.packs[0].minutes_left,
            page.packs[0].expired
        ),
        ("5 h", Some(300), false)
    );
    assert!(!page.packs[0].groups.is_empty());
    assert!(page.orders.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_account_page_of_a_guest_or_an_empty_pc_is_not_found() {
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    for guest in [false, true] {
        if guest {
            start_postpaid(&hub, None).await;
        }
        let ClientError::Hub(e) = account_page_frame(&client).await.unwrap_err() else {
            panic!("a hub refusal")
        };
        assert_eq!(e.code, ErrorCode::NotFound);
    }
    // A guest has no password: the change is a wrong current password and nothing counts.
    for _ in 0..6 {
        assert_eq!(
            change_password_frame(&client, "anything", "a-new-password").await,
            arena_proto::PlayerPasswordChangeResult::WrongCurrent
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_player_changes_their_password_and_five_wrong_currents_pause_the_form() {
    use arena_proto::PlayerPasswordChangeResult as R;
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    // A wrong current password, and a new one outside 6 to 128 characters, change nothing.
    assert_eq!(
        change_password_frame(&client, "wrong-one", "a-new-password").await,
        R::WrongCurrent
    );
    assert_eq!(
        change_password_frame(&client, "right-password", "short").await,
        R::InvalidNew
    );
    assert_eq!(
        change_password_frame(&client, "right-password", &"x".repeat(129)).await,
        R::InvalidNew
    );
    assert_eq!(
        unlock_frame(&client, "right-password").await,
        arena_proto::PlayerUnlockResult::Ok
    );
    // The right current password stores the new one, as the device, and the old one stops working.
    assert_eq!(
        change_password_frame(&client, "right-password", "a-new-password").await,
        R::Ok
    );
    assert_eq!(
        unlock_frame(&client, "right-password").await,
        arena_proto::PlayerUnlockResult::Rejected
    );
    assert_eq!(
        unlock_frame(&client, "a-new-password").await,
        arena_proto::PlayerUnlockResult::Ok
    );
    let reader = Store::open_reader(&hub.path).unwrap();
    let changed = reader
        .changes()
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.tbl == "players")
        .unwrap();
    assert_eq!(
        (changed.actor_kind.as_str(), changed.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    // Four wrong in a row, then the fifth pauses the form for a minute, checking nothing more.
    for _ in 0..4 {
        assert_eq!(
            change_password_frame(&client, "wrong-one", "another-one").await,
            R::WrongCurrent
        );
    }
    assert_eq!(
        change_password_frame(&client, "wrong-one", "another-one").await,
        R::Locked { retry_s: 60 }
    );
    assert!(matches!(
        change_password_frame(&client, "a-new-password", "another-one").await,
        R::Locked { retry_s } if retry_s > 0
    ));
}

async fn pc_settings_frame(client: &HubClient) -> arena_proto::PcSettings {
    match client
        .request("player.pc_settings", serde_json::Value::Null)
        .await
        .unwrap()
        .body
    {
        FrameBody::PlayerPcSettingsResult(r) => r.settings,
        other => panic!("{other:?}"),
    }
}

async fn pc_settings_set_frame(
    client: &HubClient,
    body: serde_json::Value,
) -> Result<bool, ClientError> {
    match client.request("player.pc_settings_set", body).await?.body {
        FrameBody::PlayerPcSettingsSetResult(r) => Ok(r.saved),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_players_pc_settings_are_saved_to_their_profile_and_read_back() {
    let hub = start("0.1.0", true).await;
    let (client, _) = playing(&hub).await;
    // Nothing saved yet.
    assert!(pc_settings_frame(&client).await.is_empty());
    assert!(
        pc_settings_set_frame(&client, serde_json::json!({ "mouse_speed": 14 }))
            .await
            .unwrap()
    );
    assert!(
        pc_settings_set_frame(
            &client,
            serde_json::json!({ "display_mode": "1920x1080@144", "keyboard_layout": "00000409" })
        )
        .await
        .unwrap()
    );
    let saved = pc_settings_frame(&client).await;
    assert_eq!(saved.mouse_speed, Some(14));
    assert_eq!(saved.display_mode.as_deref(), Some("1920x1080@144"));
    assert_eq!(saved.keyboard_layout.as_deref(), Some("00000409"));
    assert_eq!(saved.audio_output, None);
    // Written by the device actor through apply().
    let reader = Store::open_reader(&hub.path).unwrap();
    let logged = reader
        .changes()
        .unwrap()
        .into_iter()
        .rev()
        .find(|c| c.tbl == "player_pc_settings")
        .unwrap();
    assert_eq!(
        (logged.actor_kind.as_str(), logged.actor_id.as_str()),
        ("device", hub.paired_id.as_str())
    );
    // A bad value is refused and nothing more is stored.
    for bad in [
        serde_json::json!({ "mouse_speed": 0 }),
        serde_json::json!({ "mouse_speed": 21 }),
        serde_json::json!({ "display_mode": "big" }),
        serde_json::json!({ "keyboard_layout": "us" }),
        serde_json::json!({}),
    ] {
        let ClientError::Hub(e) = pc_settings_set_frame(&client, bad).await.unwrap_err() else {
            panic!("a hub refusal")
        };
        assert_eq!(e.code, ErrorCode::ValidationFailed);
    }
    assert_eq!(pc_settings_frame(&client).await.mouse_speed, Some(14));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guests_pc_settings_are_never_saved() {
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // No session at all, then a guest's: both answer saved:false and store nothing.
    for guest in [false, true] {
        if guest {
            start_postpaid(&hub, None).await;
        }
        assert!(
            !pc_settings_set_frame(&client, serde_json::json!({ "mouse_speed": 9 }))
                .await
                .unwrap()
        );
        assert!(pc_settings_frame(&client).await.is_empty());
    }
    let reader = Store::open_reader(&hub.path).unwrap();
    assert!(
        reader
            .changes()
            .unwrap()
            .iter()
            .all(|c| c.tbl != "player_pc_settings")
    );
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
                kitchen_notes: None,
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
                kitchen_notes: None,
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

// ---- Story 6.31: shop.place ----

async fn basic_branch(hub: &TestHub) {
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
}

/// A tracked 40.00 Cola with `qty` on the shelf.
async fn tracked_cola(hub: &TestHub, qty: i64) -> String {
    let w = hub.writer.as_ref().unwrap();
    let id = w
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: "Cola".into(),
                category: "Drinks".into(),
                price_minor: 4_000,
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
                kitchen_notes: None,
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
        IntentPayload::StockIn(arena_proto::StockIn {
            product_id: id.clone(),
            qty,
            unit_cost_minor: 2_000,
            supplier: String::new(),
            expiry: None,
        }),
        None,
    ))
    .await
    .unwrap();
    id
}

async fn place_frame(
    client: &HubClient,
    lines: &[(&str, i64)],
) -> Result<arena_proto::ShopPlaceResult, ClientError> {
    let lines: Vec<_> = lines
        .iter()
        .map(|(id, qty)| serde_json::json!({"product_id": id, "qty": qty}))
        .collect();
    match client
        .request("shop.place", serde_json::json!({ "lines": lines }))
        .await?
        .body
    {
        FrameBody::ShopPlaceResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

fn kiosk_order_alerts(hub: &TestHub) -> Vec<arena_store::AlertRow> {
    Store::open_reader(&hub.path)
        .unwrap()
        .alerts()
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "kiosk_order" && a.acked_at.is_none())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_order_joins_the_session_bill_raises_one_alert_and_a_short_cart_is_refused() {
    use arena_proto::ShopPlaceResult as R;
    let hub = start("0.1.0", true).await;
    basic_branch(&hub).await;
    rated_pc(&hub, 6_000).await;
    let cola = tracked_cola(&hub, 3).await;
    start_postpaid(&hub, None).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();

    assert_eq!(place_frame(&client, &[(&cola, 2)]).await.unwrap(), R::Ok);
    let alerts = kiosk_order_alerts(&hub);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].device_id.as_deref(), Some(hub.paired_id.as_str()));

    // One unit is left; asking for two is refused, names the product, and writes nothing.
    match place_frame(&client, &[(&cola, 2)]).await.unwrap() {
        R::Refused {
            code, product_id, ..
        } => {
            assert_eq!(code, ErrorCode::OutOfStock);
            assert_eq!(product_id.as_deref(), Some(cola.as_str()));
        }
        other => panic!("{other:?}"),
    }
    // The next good order finds the alert already open.
    assert_eq!(place_frame(&client, &[(&cola, 1)]).await.unwrap(), R::Ok);
    assert_eq!(kiosk_order_alerts(&hub).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_order_without_the_flag_or_a_session_is_refused_and_a_bad_cart_never_reaches_the_hub()
 {
    use arena_proto::ShopPlaceResult as R;
    let hub = start("0.1.0", true).await;
    rated_pc(&hub, 6_000).await;
    let cola = tracked_cola(&hub, 3).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    // No session on the PC yet.
    assert!(matches!(
        place_frame(&client, &[(&cola, 1)]).await.unwrap(),
        R::Refused {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    // A session, but a Free branch.
    start_postpaid(&hub, None).await;
    assert!(matches!(
        place_frame(&client, &[(&cola, 1)]).await.unwrap(),
        R::Refused {
            code: ErrorCode::FeatureNotEntitled,
            ..
        }
    ));
    assert!(kiosk_order_alerts(&hub).is_empty());
    for bad in [
        serde_json::json!({"lines": []}),
        serde_json::json!({"lines": [{"product_id": cola, "qty": 0}]}),
        serde_json::json!({"lines": [{"product_id": cola, "qty": 1}, {"product_id": cola, "qty": 1}]}),
    ] {
        match client.request("shop.place", bad).await {
            Err(ClientError::Hub(e)) => assert_eq!(e.code, ErrorCode::BadFrame),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shop_place_is_for_paired_sessions() {
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
        r#"{"v":1,"kind":"shop.place","rid":"a","lines":[{"product_id":"P1","qty":1}]}"#,
    )
    .await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
}

// ---- Story 6.32: pay now from the wallet, pay at counter ----

async fn place_pay_frame(
    client: &HubClient,
    lines: &[(&str, i64)],
    pay: &str,
) -> Result<arena_proto::ShopPlaceResult, ClientError> {
    let lines: Vec<_> = lines
        .iter()
        .map(|(id, qty)| serde_json::json!({"product_id": id, "qty": qty}))
        .collect();
    match client
        .request(
            "shop.place",
            serde_json::json!({ "lines": lines, "pay": pay }),
        )
        .await?
        .body
    {
        FrameBody::ShopPlaceResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pay_now_settles_a_prepaid_players_cart_from_the_wallet_and_a_short_wallet_is_refused() {
    use arena_proto::ShopPlaceResult as R;
    let hub = start("0.1.0", true).await;
    basic_branch(&hub).await;
    let cola = tracked_cola(&hub, 5).await;
    let (client, _player) = playing(&hub).await;

    // The pack session takes no bill order, but offers the wallet and the counter.
    let pay = shop_menu_frame(&client).await.unwrap().pay;
    assert!(!pay.bill && pay.counter);
    assert_eq!(pay.wallet_minor, Some(4_000));
    assert!(matches!(
        place_pay_frame(&client, &[(&cola, 1)], "bill")
            .await
            .unwrap(),
        R::Refused { .. }
    ));

    assert_eq!(
        place_pay_frame(&client, &[(&cola, 1)], "wallet")
            .await
            .unwrap(),
        R::Ok
    );
    assert_eq!(account_frame(&client).await.unwrap().balance_minor, 0);
    assert_eq!(kiosk_order_alerts(&hub).len(), 1);
    // The wallet is empty now: the next one is refused with the real balance, nothing written.
    match place_pay_frame(&client, &[(&cola, 1)], "wallet")
        .await
        .unwrap()
    {
        R::Refused {
            code,
            balance_minor,
            ..
        } => {
            assert_eq!(code, ErrorCode::InsufficientBalance);
            assert_eq!(balance_minor, Some(0));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        Store::open_reader(&hub.path)
            .unwrap()
            .unpaid_counter_minor(&hub.paired_id)
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pay_at_counter_leaves_an_unpaid_order_on_the_pc_and_pay_now_needs_a_profile() {
    use arena_proto::ShopPlaceResult as R;
    let hub = start("0.1.0", true).await;
    basic_branch(&hub).await;
    rated_pc(&hub, 6_000).await;
    let cola = tracked_cola(&hub, 5).await;
    start_postpaid(&hub, None).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();

    // A guest has no wallet: Pay now is refused, the bill and the counter are offered.
    let pay = shop_menu_frame(&client).await.unwrap().pay;
    assert!(pay.bill && pay.counter && pay.wallet_minor.is_none());
    assert!(matches!(
        place_pay_frame(&client, &[(&cola, 1)], "wallet")
            .await
            .unwrap(),
        R::Refused {
            code: ErrorCode::ValidationFailed,
            ..
        }
    ));

    assert_eq!(
        place_pay_frame(&client, &[(&cola, 2)], "counter")
            .await
            .unwrap(),
        R::Ok
    );
    let store = Store::open_reader(&hub.path).unwrap();
    assert_eq!(store.unpaid_counter_minor(&hub.paired_id).unwrap(), 8_000);
    assert_eq!(kiosk_order_alerts(&hub).len(), 1);
}

// ---- Story 6.36: session.sync ----

async fn session_sync_frame(client: &HubClient) -> Result<arena_proto::SessionSync, ClientError> {
    match client
        .request("session.sync", serde_json::Value::Null)
        .await?
        .body
    {
        FrameBody::SessionSyncResult(r) => Ok(r),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kiosk_that_got_the_hub_back_reads_its_live_session_and_then_why_it_ended() {
    use arena_proto::{LockReason, PlayerEndResult};
    let hub = start("0.1.0", true).await;
    let (client, _player) = playing(&hub).await;
    let live = session_sync_frame(&client).await.unwrap();
    let screen = live.session.expect("the session is live");
    assert!(screen.lock_after_ms > 0);
    assert_eq!(live.lock, None);

    assert_eq!(end_frame(&client).await, PlayerEndResult::Ok);
    let ended = session_sync_frame(&client).await.unwrap();
    assert!(ended.session.is_none());
    assert_eq!(ended.lock, Some(LockReason::PayAtCounter));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_sync_is_for_paired_sessions_only() {
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
    send(&mut ws, r#"{"v":1,"kind":"session.sync","rid":"s"}"#).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::Forbidden);
}

// ---- Story 6.38: Journey 3 over the LAN, timed ----

/// A budget in milliseconds: the story's number unless `var` overrides it.
fn budget_ms(var: &str, default: u64) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Waits for the next `session.started` pushed to the PC and returns how long it took.
async fn session_screen_within(
    events: &mut arena_kiosk_core::EventStream,
    since: Instant,
    limit: Duration,
) -> Duration {
    loop {
        let ev = tokio::time::timeout(limit, events.next_event())
            .await
            .expect("a session screen reached the PC in time")
            .unwrap();
        if matches!(ev, HubEvent::SessionStarted { .. }) {
            return since.elapsed();
        }
    }
}

/// The Hub's half of the scripted Journey 3 (login, approval, Extend, a seat order on the tile),
/// timed over the real LAN protocol and checked against the story's budgets. With
/// `ARENA_JOURNEY_OUT` set, the timings are written there for `scripts/journey-report.mjs`, which
/// adds the kiosk UI's own legs and fails the release on a miss. Budgets come from
/// `JOURNEY_LOGIN_MS`, `JOURNEY_APPROVE_MS`, `JOURNEY_EXTEND_MS` and `JOURNEY_ORDER_MS`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_side_of_journey_3_meets_its_timings() {
    use arena_proto::{PlayerAuthResult, PlayerChoice, PlayerExtendResult, PlayerStartResult};
    let hub = start("0.1.0", true).await;
    basic_branch(&hub).await;
    let group = rated_pc(&hub, 6_000).await;
    let player = add_player(&hub, "asha", "right-password", "Asha").await;
    top_up(&hub, &player, 4_000).await;
    let pack = own_pack(&hub, &player, &group).await;
    let cola = tracked_cola(&hub, 5).await;
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut events = client.events().unwrap();
    let limit = Duration::from_secs(10);

    // Login.
    let at = Instant::now();
    let result = login(&client, "asha", "right-password").await;
    let login_ms = at.elapsed();
    assert!(matches!(result, PlayerAuthResult::Ok { .. }));

    // The plan picker, then approval: the Hub accepts the start and the PC is told.
    let plans = plans(&client).await.unwrap();
    assert!(plans.options.iter().any(|o| o.choice
        == PlayerChoice::Pack {
            pack_id: pack.clone()
        }));
    let at = Instant::now();
    assert_eq!(
        start_choice(
            &client,
            serde_json::json!({"mode": "pack", "pack_id": pack})
        )
        .await
        .unwrap(),
        PlayerStartResult::Ok
    );
    let approve_ms = session_screen_within(&mut events, at, limit).await;

    // Extend: the answer is back and the PC has the new end.
    let before = Store::open_reader(&hub.path)
        .unwrap()
        .live_sessions()
        .unwrap()[0]
        .ends_at
        .unwrap();
    let at = Instant::now();
    assert_eq!(
        extend_frame(
            &client,
            serde_json::json!({"minutes": 30, "source": "wallet"})
        )
        .await
        .unwrap(),
        PlayerExtendResult::Ok
    );
    let extend_ms = session_screen_within(&mut events, at, limit).await;
    let after = Store::open_reader(&hub.path)
        .unwrap()
        .live_sessions()
        .unwrap()[0]
        .ends_at
        .unwrap();
    assert_eq!(after - before, 30 * 60_000);

    // An order from the seat is on the Hub tile (its open alert) once the Hub has answered.
    let at = Instant::now();
    assert_eq!(
        place_pay_frame(&client, &[(&cola, 1)], "counter")
            .await
            .unwrap(),
        arena_proto::ShopPlaceResult::Ok
    );
    while kiosk_order_alerts(&hub).is_empty() {
        assert!(at.elapsed() < limit, "the order never reached the tile");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let order_ms = at.elapsed();

    let ms = |d: Duration| d.as_millis() as u64;
    let timings = serde_json::json!({
        "source": "hub",
        "login_ms": ms(login_ms),
        "approve_ms": ms(approve_ms),
        "extend_ms": ms(extend_ms),
        "order_ms": ms(order_ms),
    });
    println!("journey hub timings: {timings}");
    if let Ok(out) = std::env::var("ARENA_JOURNEY_OUT") {
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, serde_json::to_string_pretty(&timings).unwrap()).unwrap();
    }
    for (what, took, var, default) in [
        ("login", login_ms, "JOURNEY_LOGIN_MS", 3_000),
        (
            "approval to the PC",
            approve_ms,
            "JOURNEY_APPROVE_MS",
            2_000,
        ),
        ("Extend to the floor", extend_ms, "JOURNEY_EXTEND_MS", 2_000),
        ("order on the tile", order_ms, "JOURNEY_ORDER_MS", 3_000),
    ] {
        let allowed = budget_ms(var, default);
        assert!(
            ms(took) <= allowed,
            "{what} took {} ms, the budget is {allowed} ms",
            ms(took)
        );
    }
}

// ---- Story 7.6: kitchen display pairing ----

/// A plain HTTP request with `extra` header lines and an optional JSON body; the whole response.
async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    extra: &str,
    body: &str,
) -> String {
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: hub\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    tcp.write_all(request.as_bytes()).await.unwrap();
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(5), tcp.read_to_string(&mut out))
        .await
        .unwrap()
        .unwrap();
    out
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or("")
}

fn json_body(response: &str) -> serde_json::Value {
    let body = response.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

async fn pair_display(hub: &TestHub, code: &str, name: Option<&str>) -> String {
    let body = match name {
        Some(n) => serde_json::json!({ "code": code, "name": n }),
        None => serde_json::json!({ "code": code }),
    };
    http_request(hub.web, "POST", "/kitchen/pair", "", &body.to_string()).await
}

async fn set_flags(hub: &TestHub, value: &'static str) {
    let sql = format!("UPDATE settings SET value = '{value}' WHERE key = 'licence.flags'");
    hub.writer
        .as_ref()
        .unwrap()
        .submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
        .await
        .unwrap();
}

fn displays(hub: &TestHub) -> Vec<arena_store::Row> {
    Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::KitchenDisplays)
        .unwrap()
}

fn column(row: &arena_store::Row, col: &str) -> String {
    match &row[col] {
        arena_store::RowValue::Text(t) => t.clone(),
        arena_store::RowValue::Integer(n) => n.to_string(),
        other => format!("{other:?}"),
    }
}

/// A Basic branch with a Manager and a live kitchen code; returns the code.
async fn kitchen_code(hub: &TestHub) -> String {
    set_flags(hub, "kitchen").await;
    let manager = add_staff(hub, "manager", "Manager", "2222").await;
    hub.kds_pairing.issue(&manager).unwrap().code
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_display_pairs_with_the_code_and_connects_as_kds() {
    let hub = start("0.1.0", true).await;
    let code = kitchen_code(&hub).await;

    let status = http_request(hub.web, "GET", "/kitchen/status", "", "").await;
    assert!(status_line(&status).contains("200"), "{status}");
    assert_eq!(
        json_body(&status),
        serde_json::json!({ "available": true, "branch_name": "Dev Branch" })
    );

    let paired = pair_display(&hub, &code, Some("Pass 2")).await;
    assert!(status_line(&paired).contains("200"), "{paired}");
    let body = json_body(&paired);
    let (id, token) = (
        body["display_id"].as_str().unwrap().to_owned(),
        body["token"].as_str().unwrap().to_owned(),
    );
    assert_eq!(body["name"], "Pass 2");
    // The row holds only the Argon2id hash, never the token or the secret.
    let row = displays(&hub)
        .into_iter()
        .find(|r| column(r, "id") == id)
        .unwrap();
    let secret = token.split_once('.').unwrap().1;
    assert!(column(&row, "token_hash").starts_with("$argon2id$"));
    assert!(!column(&row, "token_hash").contains(secret));
    assert_eq!(column(&row, "name"), "Pass 2");
    assert!(!log_text().contains(secret), "the token reached a log line");
    assert!(!log_text().contains(&code), "the code reached a log line");

    // The code was single-use.
    let again = pair_display(&hub, &code, None).await;
    assert!(status_line(&again).contains("401"), "{again}");
    assert_eq!(json_body(&again)["code"], "PAIR_CODE_INVALID");

    // The page connects with `hello{class: kds}`; the Hub marks the display seen.
    let mut ws = hub.plain().await;
    hello_ok(&mut ws, hub.hello(ClientClass::Kds, &token, "0.1.0")).await;
    assert!(hub.presence.display_online(&id));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let seen = displays(&hub)
            .into_iter()
            .find(|r| column(r, "id") == id)
            .map(|r| column(&r, "last_seen"))
            .unwrap_or_default();
        if seen != "Null" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the display was never marked seen"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_name_defaults_to_kitchen_and_a_free_branch_pairs_nothing() {
    let hub = start("0.1.0", true).await;
    let before = displays(&hub).len();
    // Free: the page says so, and `kds.pair` is refused with nothing written.
    let status = http_request(hub.web, "GET", "/kitchen/status", "", "").await;
    assert_eq!(json_body(&status)["available"], false);
    let manager = add_staff(&hub, "manager", "Manager", "2222").await;
    let code = hub.kds_pairing.issue(&manager).unwrap().code;
    let locked = pair_display(&hub, &code, None).await;
    assert!(status_line(&locked).contains("403"), "{locked}");
    assert_eq!(json_body(&locked)["code"], "TIER_LOCKED");
    assert_eq!(displays(&hub).len(), before);

    // Basic: the same code now pairs, as "Kitchen".
    set_flags(&hub, "kitchen").await;
    let ok = pair_display(&hub, &code, Some("   ")).await;
    assert!(status_line(&ok).contains("200"), "{ok}");
    assert_eq!(json_body(&ok)["name"], "Kitchen");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_wrong_codes_kill_the_code_and_the_21st_attempt_is_rate_limited() {
    let hub = start("0.1.0", true).await;
    let code = kitchen_code(&hub).await;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for n in 0..5 {
        let r = pair_display(&hub, wrong, None).await;
        assert!(status_line(&r).contains("401"), "try {n}: {r}");
        assert_eq!(json_body(&r)["code"], "PAIR_CODE_INVALID");
    }
    // The fifth wrong code killed the live code; the right one no longer works.
    let dead = pair_display(&hub, &code, None).await;
    assert_eq!(json_body(&dead)["code"], "PAIR_CODE_INVALID", "{dead}");
    for _ in 7..=20 {
        let r = pair_display(&hub, wrong, None).await;
        assert!(status_line(&r).contains("401"), "{r}");
    }
    let limited = pair_display(&hub, wrong, None).await;
    assert!(status_line(&limited).contains("429"), "{limited}");
    assert_eq!(json_body(&limited)["code"], "RATE_LIMITED");
    assert!(
        limited.to_ascii_lowercase().contains("retry-after:"),
        "{limited}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fifth_display_is_refused_and_the_code_stays_valid() {
    let hub = start("0.1.0", true).await;
    set_flags(&hub, "kitchen").await;
    let manager = add_staff(&hub, "manager", "Manager", "2222").await;
    // The harness holds one display; three more fill the branch.
    for n in 0..3 {
        let code = hub.kds_pairing.issue(&manager).unwrap().code;
        let r = pair_display(&hub, &code, Some(&format!("Screen {n}"))).await;
        assert!(status_line(&r).contains("200"), "{r}");
    }
    let code = hub.kds_pairing.issue(&manager).unwrap().code;
    let full = pair_display(&hub, &code, Some("Fifth")).await;
    assert!(status_line(&full).contains("403"), "{full}");
    let body = json_body(&full);
    assert_eq!(body["code"], "DEVICE_LIMIT");
    assert_eq!(body["message"], arena_proto::KDS_LIMIT_MESSAGE);
    assert_eq!(displays(&hub).len(), 4);
    // None of them is a device: the kiosk list and the device cap are untouched.
    let devices = Store::open_reader(&hub.path).unwrap().devices().unwrap();
    assert!(devices.iter().all(|d| d.class == "paired"));

    // Revoking one frees the slot, and the same code still works.
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::KdsRevoke(arena_proto::KdsRevoke {
                display_id: hub.kds_id.clone(),
            }),
            None,
        ))
        .await
        .unwrap();
    let ok = pair_display(&hub, &code, Some("Fifth")).await;
    assert!(status_line(&ok).contains("200"), "{ok}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_toggle_reaches_a_display_within_two_seconds_and_a_revoke_closes_its_socket() {
    let hub = start("0.1.0", true).await;
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let key = arena_proto::hc_key(&hub.kds_id);

    // A display reads only its own high-contrast key.
    send(
        &mut ws,
        &format!(r#"{{"v":1,"kind":"settings.get","rid":"g1","keys":["{key}"]}}"#),
    )
    .await;
    let reply = recv(&mut ws).await.unwrap();
    let FrameBody::SettingsValues(values) = reply.body else {
        panic!("{reply:?}")
    };
    assert_eq!(values.values[0].value, "false");
    send(
        &mut ws,
        r#"{"v":1,"kind":"settings.get","rid":"g2","keys":["display.hc.hub"]}"#,
    )
    .await;
    let reply = recv(&mut ws).await.unwrap();
    assert_eq!(error_of(&reply).1, ErrorCode::Forbidden);

    // The manager's toggle (settings.set with the display's rev) is pushed at once.
    let rev: i64 = column(
        displays(&hub)
            .iter()
            .find(|r| column(r, "id") == hub.kds_id)
            .unwrap(),
        "rev",
    )
    .parse()
    .unwrap();
    let toggled_at = Instant::now();
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::SettingsSet(SettingsSet {
                key: key.clone(),
                value: "true".into(),
            }),
            Some(rev),
        ))
        .await
        .unwrap();
    let pushed = loop {
        let frame = recv_any(&mut ws).await.expect("a pushed settings.changed");
        if let FrameBody::Event(e) = frame.body
            && let HubEvent::SettingsChanged(v) = e.event
        {
            break v;
        }
    };
    assert!(toggled_at.elapsed() <= Duration::from_secs(2));
    assert_eq!(pushed.values[0].key, key);
    assert_eq!(pushed.values[0].value, "true");
    assert_eq!(
        column(
            displays(&hub)
                .iter()
                .find(|r| column(r, "id") == hub.kds_id)
                .unwrap(),
            "high_contrast"
        ),
        "1"
    );

    // Revoke: TOKEN_REVOKED, then the socket closes, and the token no longer connects.
    hub.writer
        .as_ref()
        .unwrap()
        .submit_intent(intent(
            owner(),
            IntentPayload::KdsRevoke(arena_proto::KdsRevoke {
                display_id: hub.kds_id.clone(),
            }),
            None,
        ))
        .await
        .unwrap();
    let revoked_at = Instant::now();
    loop {
        let frame = recv_any(&mut ws)
            .await
            .expect("a TOKEN_REVOKED error before the close");
        if let FrameBody::Error(e) = &frame.body {
            assert_eq!(e.error.code, ErrorCode::TokenRevoked);
            break;
        }
    }
    assert!(revoked_at.elapsed() <= Duration::from_secs(2));
    assert!(recv_any(&mut ws).await.is_none(), "the socket closes");
    let mut again = hub.plain().await;
    send_frame(
        &mut again,
        &Frame::new(
            "h",
            FrameBody::Hello(hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0")),
        ),
    )
    .await;
    let frame = recv(&mut again).await.unwrap();
    assert_eq!(error_of(&frame).1, ErrorCode::TokenRevoked);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plain_http_strays_and_foreign_bearers_on_the_kitchen_are_forbidden() {
    let hub = start("0.1.0", true).await;
    for (method, path) in [
        ("GET", "/media/abc"),
        ("GET", "/nope"),
        ("POST", "/pair"),
        ("POST", "/kitchen"),
        ("GET", "/kitchen/pair"),
    ] {
        let r = http_request(hub.web, method, path, "", "").await;
        assert!(status_line(&r).contains("403"), "{method} {path}: {r}");
        assert_eq!(json_body(&r)["code"], "FORBIDDEN", "{method} {path}");
    }
    // A live display token passes.
    let ok = http_request(
        hub.web,
        "GET",
        "/kitchen/status",
        &format!("Authorization: Bearer {}\r\n", hub.kds_token),
        "",
    )
    .await;
    assert!(status_line(&ok).contains("200"), "{ok}");
    // TLS clients on the same port are unaffected.
    let client = HubClient::connect(&hub.web_tls_url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();
    // (Wrong tokens count against the hello throttle of the address, so they come last.)
    // A paired device's token, a made-up one and a Basic header are refused on every kitchen path.
    let paired = format!("Authorization: Bearer {}\r\n", hub.paired_token);
    for extra in [
        paired.as_str(),
        "Authorization: Bearer nope\r\n",
        "Authorization: Basic abc\r\n",
    ] {
        for (method, path) in [
            ("GET", "/kitchen"),
            ("GET", "/kitchen/status"),
            ("POST", "/kitchen/pair"),
        ] {
            let r = http_request(hub.web, method, path, extra, "{}").await;
            assert!(status_line(&r).contains("403"), "{method} {path}: {r}");
            assert_eq!(json_body(&r)["code"], "FORBIDDEN", "{method} {path}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_serves_the_kitchen_build_from_its_directory_and_nothing_outside_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("kitchen");
    std::fs::create_dir_all(root.join("_app")).unwrap();
    std::fs::write(root.join("index.html"), "<!doctype html>APP-INDEX").unwrap();
    std::fs::write(root.join("_app").join("a.js"), "console.log(1)").unwrap();
    std::fs::write(dir.path().join("outside-secret.txt"), "secret").unwrap();
    let hub = start_kitchen(root).await;

    let page = http_request(hub.web, "GET", "/kitchen", "", "").await;
    assert!(page.contains("APP-INDEX"), "{page}");
    assert!(page.contains("text/html"));
    let page = http_request(hub.web, "GET", "/kitchen/", "", "").await;
    assert!(page.contains("APP-INDEX"), "{page}");
    let js = http_request(hub.web, "GET", "/kitchen/_app/a.js", "", "").await;
    assert!(
        js.contains("console.log(1)") && js.contains("text/javascript"),
        "{js}"
    );
    // A client-side route is the app's page; a missing file is a 404.
    let spa = http_request(hub.web, "GET", "/kitchen/board", "", "").await;
    assert!(spa.contains("APP-INDEX"), "{spa}");
    let missing = http_request(hub.web, "GET", "/kitchen/_app/missing.js", "", "").await;
    assert!(status_line(&missing).contains("404"), "{missing}");
    for path in [
        "/kitchen/../outside-secret.txt",
        "/kitchen/_app/../../outside-secret.txt",
        "/kitchen/%2e%2e/outside-secret.txt",
        "/kitchen/..%5coutside-secret.txt",
    ] {
        let r = http_request(hub.web, "GET", path, "", "").await;
        assert!(!r.contains("secret"), "{path}: {r}");
        assert!(!status_line(&r).contains("200"), "{path}: {r}");
    }

    // Without a build directory the built-in page pairs the same way.
    let bare = start("0.1.0", true).await;
    let page = http_request(bare.web, "GET", "/kitchen", "", "").await;
    assert!(
        status_line(&page).contains("200") && page.contains("/kitchen/pair"),
        "{page}"
    );
}

/// Seeds two kitchen tickets straight into the branch database (Story 7.7): `K-1` New with a
/// noted line and `K-2` already Served. Only the kitchen board reads them.
fn seed_tickets(hub: &TestHub, now: i64) {
    let mut store = Store::open(&hub.path, T0).unwrap();
    store
        .write_tx(|tx| {
            let notes = vec!["No onion".to_owned()];
            for (id, number, seat) in [("T1", 1, "PC 07"), ("T2", 2, "Counter")] {
                arena_store::insert_ticket(
                    tx,
                    &arena_store::NewTicket {
                        id,
                        order_id: "O1",
                        device_id: None,
                        seat,
                        number,
                        placement_id: None,
                        placed_at: now - 60_000,
                    },
                )?;
                arena_store::insert_ticket_line(
                    tx,
                    &arena_store::NewTicketLine {
                        id: &format!("L{number}"),
                        ticket_id: id,
                        order_line_id: &format!("OL{number}"),
                        label: "Burger",
                        qty: 2,
                        notes: if number == 1 { &notes } else { &[] },
                    },
                )?;
            }
            arena_store::set_ticket_stage(tx, "T2", "served", now - 10_000, "test")?;
            Ok(())
        })
        .unwrap();
}

async fn kitchen_board(
    ws: &mut (
             impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
             + SinkExt<Message>
             + Unpin
         ),
    rid: &str,
) -> Frame {
    ws.send(Message::text(format!(
        r#"{{"v":1,"kind":"kitchen.board","rid":"{rid}"}}"#
    )))
    .await
    .ok()
    .unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = ws.next().await else {
            panic!("the board reply")
        };
        let frame = parse_frame(t.as_str()).unwrap();
        if !matches!(frame.body, FrameBody::Event(_)) {
            return frame;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_display_reads_the_board_and_is_pushed_it_after_a_ticket_changes() {
    let hub = start("0.1.0", true).await;
    seed_tickets(&hub, T0);
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;

    let mut phone = hub.plain().await;
    hello_ok(&mut phone, hub.hello(ClientClass::Phone, "", "0.1.0")).await;

    let reply = kitchen_board(&mut ws, "b1").await;
    let FrameBody::KitchenBoardResult(board) = reply.body else {
        panic!("{reply:?}")
    };
    assert_eq!(board.now_ms, T0);
    let seen: Vec<(i64, &str, &str)> = board
        .tickets
        .iter()
        .map(|t| (t.number, t.stage.as_str(), t.seat.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![(2, "served", "Counter"), (1, "new", "PC 07")],
        "newest first, with the seat and no player"
    );
    assert_eq!(board.tickets[1].lines[0].notes, vec!["No onion".to_owned()]);
    assert_eq!(board.tickets[1].lines[0].qty, 2);

    // The display's own advance (its only intent) reaches it back as a whole board.
    let advance = Intent {
        base_rev: Some(0),
        ..intent(
            system(),
            IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
                ticket_id: "T1".into(),
                to: "preparing".into(),
            }),
            None,
        )
    };
    send(
        &mut ws,
        &Frame::new(
            "a1",
            FrameBody::IntentSubmit(IntentSubmit { intent: advance }),
        )
        .to_text(),
    )
    .await;
    let pushed = loop {
        let frame = recv_any(&mut ws).await.expect("a pushed board");
        if let FrameBody::Event(e) = frame.body
            && let HubEvent::KitchenBoard(b) = e.event
        {
            break b;
        }
    };
    let t1 = pushed.tickets.iter().find(|t| t.id == "T1").unwrap();
    assert_eq!(t1.stage, "preparing");
    assert!(t1.stage_at >= t1.placed_at);

    // Only a display is pushed the board: a phone on the same fan-out never gets it.
    while let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(300), phone.next()).await
    {
        let frame = parse_frame(text.as_str()).unwrap();
        assert!(
            !matches!(&frame.body, FrameBody::Event(e) if matches!(e.event, HubEvent::KitchenBoard(_))),
            "a phone was pushed the kitchen board"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_kitchen_display_may_ask_for_the_board() {
    let hub = start("0.1.0", true).await;
    seed_tickets(&hub, T0);
    let mut phone = hub.plain().await;
    hello_ok(&mut phone, hub.hello(ClientClass::Phone, "", "0.1.0")).await;
    let reply = kitchen_board(&mut phone, "b1").await;
    assert_eq!(error_of(&reply).1, ErrorCode::Forbidden);

    let mut kiosk = hub.tls().await;
    hello_ok(
        &mut kiosk,
        hub.hello(ClientClass::Paired, &hub.paired_token, "0.1.0"),
    )
    .await;
    let reply = kitchen_board(&mut kiosk, "b2").await;
    assert_eq!(error_of(&reply).1, ErrorCode::Forbidden);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_board_still_serves_its_tickets_after_a_lapse_to_free() {
    let hub = start("0.1.0", true).await;
    seed_tickets(&hub, T0);
    // Basic, then lapsed: the branch holds no `kitchen` flag now, and the tickets still show.
    set_flags(&hub, "kitchen").await;
    set_flags(&hub, "").await;
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let reply = kitchen_board(&mut ws, "b1").await;
    let FrameBody::KitchenBoardResult(board) = reply.body else {
        panic!("{reply:?}")
    };
    assert_eq!(
        board.tickets.len(),
        2,
        "the licence flags do not gate the read"
    );
}

/// Submits a kitchen intent as the display and returns the reply to it, skipping pushed events.
async fn kitchen_move<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
    rid: &str,
    payload: IntentPayload,
    base_rev: i64,
) -> Frame {
    let intent = Intent {
        base_rev: Some(base_rev),
        ..intent(system(), payload, None)
    };
    send(
        ws,
        &Frame::new(rid, FrameBody::IntentSubmit(IntentSubmit { intent })).to_text(),
    )
    .await;
    loop {
        let frame = recv_any(ws).await.expect("a reply");
        if frame.rid == rid && !matches!(frame.body, FrameBody::Event(_)) {
            return frame;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_display_advances_a_ticket_one_stage_and_takes_the_latest_advance_back() {
    let hub = start("0.1.0", true).await;
    seed_tickets(&hub, T0);
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let advance = |to: &str| {
        IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
            ticket_id: "T1".into(),
            to: to.into(),
        })
    };
    let revert = || {
        IntentPayload::KitchenAdvanceReverted(arena_proto::KitchenAdvanceReverted {
            ticket_id: "T1".into(),
        })
    };
    let rev_of =
        |b: &arena_proto::KitchenBoard| b.tickets.iter().find(|t| t.id == "T1").unwrap().rev;

    let board = match kitchen_board(&mut ws, "b0").await.body {
        FrameBody::KitchenBoardResult(b) => b,
        other => panic!("{other:?}"),
    };
    let rev = rev_of(&board);

    // A skipped stage and a stale rev change nothing.
    let skip = kitchen_move(&mut ws, "a0", advance("ready"), rev).await;
    assert_eq!(error_of(&skip).1, ErrorCode::BadStage);
    let stale = kitchen_move(&mut ws, "a1", advance("preparing"), rev + 5).await;
    assert_eq!(error_of(&stale).1, ErrorCode::Stale);

    let done = kitchen_move(&mut ws, "a2", advance("preparing"), rev).await;
    assert!(matches!(done.body, FrameBody::IntentResult(_)), "{done:?}");
    let board = match kitchen_board(&mut ws, "b1").await.body {
        FrameBody::KitchenBoardResult(b) => b,
        other => panic!("{other:?}"),
    };
    let moved = board.tickets.iter().find(|t| t.id == "T1").unwrap();
    assert_eq!(moved.stage, "preparing");

    // The Undo carries the rev the advance left; any other rev is stale.
    let stale = kitchen_move(&mut ws, "u0", revert(), rev).await;
    assert_eq!(error_of(&stale).1, ErrorCode::Stale);
    let undone = kitchen_move(&mut ws, "u1", revert(), moved.rev).await;
    assert!(
        matches!(undone.body, FrameBody::IntentResult(_)),
        "{undone:?}"
    );
    let board = match kitchen_board(&mut ws, "b2").await.body {
        FrameBody::KitchenBoardResult(b) => b,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        board.tickets.iter().find(|t| t.id == "T1").unwrap().stage,
        "new"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_a_lapse_a_display_advances_only_tickets_that_already_exist() {
    let hub = start("0.1.0", true).await;
    seed_tickets(&hub, T0);
    set_flags(&hub, "kitchen").await;
    set_flags(&hub, "").await;
    let mut ws = hub.plain().await;
    hello_ok(
        &mut ws,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let other = kitchen_move(
        &mut ws,
        "x1",
        IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
            ticket_id: "NOPE".into(),
            to: "preparing".into(),
        }),
        0,
    )
    .await;
    assert_eq!(error_of(&other).1, ErrorCode::TierLocked);
    let old = kitchen_move(
        &mut ws,
        "x2",
        IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
            ticket_id: "T1".into(),
            to: "preparing".into(),
        }),
        0,
    )
    .await;
    assert!(matches!(old.body, FrameBody::IntentResult(_)), "{old:?}");
}

// ---- Story 7.11: Journey 4 and the kitchen under load, timed ----

const KOT_PRINTER: &str = r#"{"kind":"lan","id_or_addr":"192.168.1.50:9100","name":"Kitchen"}"#;

/// A Basic branch with a kitchen item on the menu and the KOT task printing to a fake printer.
struct KitchenRig {
    fake: Arc<arena_tasks::printer::FakePrinter>,
    burger: String,
}

async fn kitchen_rig(hub: &TestHub, device_cap: u32) -> KitchenRig {
    let w = hub.writer.as_ref().unwrap();
    w.submit_intent(intent(
        system(),
        IntentPayload::LicenceApply(arena_proto::LicenceApply {
            standing: arena_proto::LicenceStanding::Verified,
            clock_wrong: false,
            tier: arena_proto::Tier::Basic,
            flags: "kitchen,kiosk_orders".into(),
            device_cap,
            user_cap: 10,
        }),
        None,
    ))
    .await
    .unwrap();
    let sql = format!("UPDATE settings SET value = '{KOT_PRINTER}' WHERE key = 'printer.selected'");
    w.submit(move |tx| tx.execute_batch(&sql).map_err(arena_store::db_error))
        .await
        .unwrap();
    let burger = w
        .submit_intent(intent(
            owner(),
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: "Burger".into(),
                category: "Food".into(),
                price_minor: 12_000,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: false,
                image: String::new(),
                barcode: String::new(),
                kitchen: true,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
                kitchen_notes: None,
            }),
            None,
        ))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let fake = Arc::new(arena_tasks::printer::FakePrinter::new(Vec::new()));
    let task = arena_tasks::ReceiptPrintTask::new(
        w.clone(),
        Arc::new(arena_core::SnapshotReader::open(&hub.path).unwrap()),
        Arc::new(ManualClock::new(T0)),
        fake.clone(),
        Arc::default(),
    );
    tokio::spawn(task.run());
    KitchenRig { fake, burger }
}

/// The ticket numbers of the KOT slips the fake printer has been sent, in the order sent.
fn kot_numbers(fake: &arena_tasks::printer::FakePrinter) -> Vec<i64> {
    fake.sent()
        .iter()
        .filter_map(|job| {
            let text = String::from_utf8_lossy(&job.bytes).into_owned();
            if !text.contains("KOT") {
                return None;
            }
            let at = text.find("K-")? + 2;
            let digits: String = text[at..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok()
        })
        .collect()
}

fn ticket_rows(hub: &TestHub) -> Vec<arena_store::Row> {
    Store::open_reader(&hub.path)
        .unwrap()
        .rows(arena_store::Table::KitchenTickets)
        .unwrap()
}

fn board_ticket(
    board: &arena_proto::KitchenBoard,
    number: i64,
) -> &arena_proto::KitchenBoardTicket {
    board
        .tickets
        .iter()
        .find(|t| t.number == number)
        .expect("the ticket is on the board")
}

/// The next kitchen board the Hub pushes to `ws` that holds at least `tickets` tickets.
async fn pushed_board<S: AsyncRead + AsyncWrite + Unpin>(
    ws: &mut WebSocketStream<S>,
    tickets: usize,
) -> arena_proto::KitchenBoard {
    loop {
        let frame = recv_any(ws).await.expect("a pushed board");
        if let FrameBody::Event(e) = frame.body
            && let HubEvent::KitchenBoard(b) = e.event
            && b.tickets.len() >= tickets
        {
            return *b;
        }
    }
}

/// Waits for the KOT slip of `number` to reach the fake printer; how long after `since`.
async fn kot_within(
    fake: &arena_tasks::printer::FakePrinter,
    number: i64,
    since: Instant,
    limit: Duration,
) -> Duration {
    while !kot_numbers(fake).contains(&number) {
        assert!(since.elapsed() < limit, "the KOT never reached the printer");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    since.elapsed()
}

/// Waits for a pushed `session.tick` that carries the order's ready copy; how long after `since`.
async fn hud_ready_within(
    events: &mut arena_kiosk_core::EventStream,
    since: Instant,
    limit: Duration,
) -> Duration {
    loop {
        assert!(
            since.elapsed() < limit,
            "the ready copy never reached the PC"
        );
        let ev = tokio::time::timeout(limit, events.next_event())
            .await
            .expect("the ready copy reached the PC in time")
            .unwrap();
        if let HubEvent::SessionTick(s) = &ev
            && s.orders.iter().any(|o| o.ready.is_some())
        {
            return since.elapsed();
        }
    }
}

/// Waits for the Hub to tell a staff client that a ticket changed and the snapshot to show the PC's
/// ticket Ready (the tile's READY tag); how long after `since`.
async fn tile_ready_within(
    hub: &TestHub,
    staff: &mut arena_kiosk_core::EventStream,
    since: Instant,
    limit: Duration,
) -> Duration {
    let reader = Arc::new(arena_core::SnapshotReader::open(&hub.path).unwrap());
    let device = hub.paired_id.clone();
    loop {
        assert!(since.elapsed() < limit, "the tile never showed READY");
        let ev = tokio::time::timeout(limit, staff.next_event())
            .await
            .expect("the tile heard of the change in time")
            .unwrap();
        let HubEvent::Applied(a) = ev else {
            continue;
        };
        if !a.changes.iter().any(|c| c.tbl == "kitchen_tickets") {
            continue;
        }
        let (reader, device) = (Arc::clone(&reader), device.clone());
        let ready = tokio::task::spawn_blocking(move || {
            reader
                .snapshot(T0)
                .unwrap()
                .bills
                .iter()
                .any(|b| b.device_id == device && b.tickets.iter().any(|t| t.stage == "ready"))
        })
        .await
        .unwrap();
        if ready {
            return since.elapsed();
        }
    }
}

fn write_out(var: &str, value: &serde_json::Value) {
    println!("{var}: {value}");
    if let Ok(out) = std::env::var(var) {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }
}

/// The Hub's half of the scripted Journey 4 (kiosk order, KOT, board New, Preparing, Ready, HUD
/// copy, tile READY, Served), timed over the real LAN protocol. With `ARENA_KITCHEN_OUT` set the
/// timings are written there for `scripts/kitchen-report.mjs`. Budgets come from `KITCHEN_KOT_MS`
/// (3000), `KITCHEN_BOARD_MS` (2000), `KITCHEN_HUD_MS` (5000) and `KITCHEN_TILE_MS` (2000).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_side_of_journey_4_meets_its_timings() {
    let hub = start("0.1.0", true).await;
    let rig = kitchen_rig(&hub, 10).await;
    rated_pc(&hub, 6_000).await;
    start_postpaid(&hub, None).await;
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
    let (staff_id, staff_token) =
        register(hub.writer.as_ref().unwrap(), ClientClass::Paired, "Hub UI").await;
    let _ = staff_id;
    let staff = HubClient::connect(
        &hub.url(),
        &hub.fingerprint,
        hub.hello(ClientClass::Paired, &staff_token, "0.1.0"),
    )
    .await
    .unwrap();
    let mut staff_events = staff.events().unwrap();
    let limit = Duration::from_secs(10);

    // The order from the seat: the KOT prints and the ticket is on the board as New.
    let at = Instant::now();
    assert_eq!(
        place_frame(&client, &[(&rig.burger, 1)]).await.unwrap(),
        arena_proto::ShopPlaceResult::Ok
    );
    let (kot, (board_ms, board)) = tokio::join!(kot_within(&rig.fake, 1, at, limit), async {
        let b = pushed_board(&mut kds, 1).await;
        (at.elapsed(), b)
    });
    assert_eq!(board_ticket(&board, 1).stage, "new");
    assert_eq!(kot_numbers(&rig.fake), vec![1], "one KOT for the ticket");

    // Preparing, then Ready: the PC's HUD copy and the tile's READY follow the tap.
    let id = board_ticket(&board, 1).id.clone();
    let advance = |to: &str| {
        IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
            ticket_id: id.clone(),
            to: to.into(),
        })
    };
    let rev = board_ticket(&board, 1).rev;
    let moved = kitchen_move(&mut kds, "p", advance("preparing"), rev).await;
    assert!(
        matches!(moved.body, FrameBody::IntentResult(_)),
        "{moved:?}"
    );
    let board = match kitchen_board(&mut kds, "b1").await.body {
        FrameBody::KitchenBoardResult(b) => b,
        other => panic!("{other:?}"),
    };
    assert_eq!(board_ticket(&board, 1).stage, "preparing");
    // Earlier events are harmless: the tile and the HUD only count once the ticket is Ready.
    let at = Instant::now();
    let (moved, hud, tile) = tokio::join!(
        kitchen_move(&mut kds, "r", advance("ready"), board_ticket(&board, 1).rev),
        hud_ready_within(&mut events, at, limit),
        tile_ready_within(&hub, &mut staff_events, at, limit),
    );
    assert!(
        matches!(moved.body, FrameBody::IntentResult(_)),
        "{moved:?}"
    );

    let board = match kitchen_board(&mut kds, "b2").await.body {
        FrameBody::KitchenBoardResult(b) => b,
        other => panic!("{other:?}"),
    };
    assert_eq!(board_ticket(&board, 1).stage, "ready");
    let served = kitchen_move(
        &mut kds,
        "s",
        advance("served"),
        board_ticket(&board, 1).rev,
    )
    .await;
    assert!(
        matches!(served.body, FrameBody::IntentResult(_)),
        "{served:?}"
    );

    let ms = |d: Duration| d.as_millis() as u64;
    let timings = serde_json::json!({
        "source": "hub",
        "kot_ms": ms(kot),
        "board_ms": ms(board_ms),
        "hud_ms": ms(hud),
        "tile_ms": ms(tile),
    });
    write_out("ARENA_KITCHEN_OUT", &timings);
    for (what, took, var, default) in [
        ("KOT printed", kot, "KITCHEN_KOT_MS", 3_000),
        ("ticket on the board", board_ms, "KITCHEN_BOARD_MS", 2_000),
        ("HUD ready copy", hud, "KITCHEN_HUD_MS", 5_000),
        ("tile READY", tile, "KITCHEN_TILE_MS", 2_000),
    ] {
        let allowed = budget_ms(var, default);
        assert!(
            ms(took) <= allowed,
            "{what} took {} ms, the budget is {allowed} ms",
            ms(took)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unplugged_printer_raises_the_alert_and_the_board_still_gets_the_ticket() {
    let hub = start("0.1.0", true).await;
    let rig = kitchen_rig(&hub, 10).await;
    rated_pc(&hub, 6_000).await;
    start_postpaid(&hub, None).await;
    rig.fake
        .fail_with(Some(arena_tasks::printer::PrinterError::Refused(
            "192.168.1.50:9100".parse().unwrap(),
        )));
    let client = HubClient::connect(&hub.url(), &hub.fingerprint, hub.paired())
        .await
        .unwrap();
    let mut kds = hub.plain().await;
    hello_ok(
        &mut kds,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;

    let at = Instant::now();
    assert_eq!(
        place_frame(&client, &[(&rig.burger, 1)]).await.unwrap(),
        arena_proto::ShopPlaceResult::Ok
    );
    let board = pushed_board(&mut kds, 1).await;
    assert_eq!(board_ticket(&board, 1).stage, "new");
    let offline = |h: &TestHub| -> usize {
        Store::open_reader(&h.path)
            .unwrap()
            .alerts()
            .unwrap()
            .into_iter()
            .filter(|a| a.kind == "printer_offline" && a.acked_at.is_none())
            .count()
    };
    while offline(&hub) == 0 {
        assert!(
            at.elapsed() < Duration::from_secs(10),
            "no printer-offline alert"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(offline(&hub), 1);
    assert!(rig.fake.sent().is_empty(), "nothing printed");
    write_out(
        "ARENA_KITCHEN_UNPLUGGED_OUT",
        &serde_json::json!({ "source": "unplugged", "alert": true, "board": true }),
    );
}

/// A plain-HTTP socket that does not borrow the hub, so a task can own it.
async fn owned_plain(web: SocketAddr) -> WebSocketStream<TcpStream> {
    let tcp = TcpStream::connect(web).await.unwrap();
    let url = format!("ws://127.0.0.1:{}/ws", web.port());
    tokio_tungstenite::client_async(url, tcp).await.unwrap().0
}

/// Keeps the newest kitchen board a display was pushed.
struct BoardFollower {
    last: Arc<Mutex<Option<arena_proto::KitchenBoard>>>,
    task: tokio::task::JoinHandle<()>,
}

impl BoardFollower {
    fn follow<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        mut ws: WebSocketStream<S>,
    ) -> Self {
        let last = Arc::new(Mutex::new(None));
        let keep = Arc::clone(&last);
        let task = tokio::spawn(async move {
            // A display pings every 15 s, or the Hub closes a silent session after 45 s.
            let mut ping = tokio::time::interval(Duration::from_secs(15));
            let mut n = 0u32;
            loop {
                let message = tokio::select! {
                    m = ws.next() => match m {
                        Some(Ok(m)) => m,
                        _ => return,
                    },
                    _ = ping.tick() => {
                        n += 1;
                        let text = format!(r#"{{"v":1,"kind":"ping","rid":"k{n}"}}"#);
                        if ws.send(Message::text(text)).await.is_err() {
                            return;
                        }
                        continue;
                    }
                };
                let Message::Text(text) = message else {
                    continue;
                };
                if let Ok(frame) = parse_frame(text.as_str())
                    && let FrameBody::Event(e) = frame.body
                    && let HubEvent::KitchenBoard(b) = e.event
                {
                    *keep.lock().unwrap() = Some(*b);
                }
            }
        });
        Self { last, task }
    }

    fn board(&self) -> Option<arena_proto::KitchenBoard> {
        self.last.lock().unwrap().clone()
    }
}

impl Drop for BoardFollower {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// `kiosks` paired PCs, each with a running postpaid session, and a client on each.
async fn kiosk_floor(hub: &TestHub, kiosks: usize) -> Vec<Arc<HubClient>> {
    let group = rated_pc(hub, 6_000).await;
    let w = hub.writer.as_ref().unwrap();
    let mut floor = vec![(hub.paired_id.clone(), hub.paired())];
    for n in 1..kiosks {
        let (id, token) = register(w, ClientClass::Paired, &format!("PC {:02}", n + 1)).await;
        w.submit_intent(intent(
            owner(),
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: id.clone(),
                group_id: Some(group.clone()),
            }),
            None,
        ))
        .await
        .unwrap();
        floor.push((id, hub.hello(ClientClass::Paired, &token, "0.1.0")));
    }
    let mut clients = Vec::new();
    for (id, hello) in floor {
        w.submit_intent(intent(
            owner(),
            IntentPayload::SessionStart(arena_proto::SessionStart {
                group_with: None,
                pay_from_wallet: false,
                device_id: id,
                mode: arena_proto::SessionMode::Postpaid,
                player_id: None,
                pack_id: None,
                plan_id: None,
            }),
            None,
        ))
        .await
        .unwrap();
        clients.push(Arc::new(
            HubClient::connect(&hub.url(), &hub.fingerprint, hello)
                .await
                .unwrap(),
        ));
    }
    clients
}

/// 95th percentile of `ms` (nearest rank).
fn p95(ms: &mut [u64]) -> u64 {
    ms.sort_unstable();
    ms.get((ms.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .unwrap_or(0)
}

/// Places orders at `rate` per second for `secs` across `kiosks` PCs while two displays follow the
/// board; every KOT must print once, within `KITCHEN_P95_MS` (3000) at the 95th percentile, and
/// both boards must end equal to the Hub's tickets. Written to `ARENA_KITCHEN_BURST_OUT`.
async fn burst(rate: u64, secs: u64, kiosks: usize) {
    let hub = start("0.1.0", true).await;
    let rig = kitchen_rig(&hub, 40).await;
    let clients = kiosk_floor(&hub, kiosks).await;
    let mut boards = Vec::new();
    for _ in 0..2 {
        let mut ws = owned_plain(hub.web).await;
        hello_ok(
            &mut ws,
            hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
        )
        .await;
        boards.push(BoardFollower::follow(ws));
    }
    let total = (rate * secs) as usize;

    // Watches the tickets and the printer: when each ticket first showed in the database and when
    // its KOT first reached the printer.
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let watcher = {
        let (watching, fake, path) = (Arc::clone(&watching), rig.fake.clone(), hub.path.clone());
        tokio::spawn(async move {
            let mut created: std::collections::HashMap<i64, Instant> = Default::default();
            let mut printed: std::collections::HashMap<i64, Instant> = Default::default();
            while watching.load(std::sync::atomic::Ordering::Relaxed) {
                let now = Instant::now();
                let rows = tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || {
                        Store::open_reader(&path)
                            .unwrap()
                            .rows(arena_store::Table::KitchenTickets)
                            .unwrap()
                    }
                })
                .await
                .unwrap();
                for row in rows {
                    if let Ok(n) = column(&row, "number").parse() {
                        created.entry(n).or_insert(now);
                    }
                }
                for n in kot_numbers(&fake) {
                    printed.entry(n).or_insert_with(Instant::now);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            (created, printed)
        })
    };

    let mut tick = tokio::time::interval(Duration::from_millis(1000 / rate));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut placing = Vec::new();
    for n in 0..total {
        tick.tick().await;
        let client = Arc::clone(&clients[n % clients.len()]);
        let burger = rig.burger.clone();
        placing.push(tokio::spawn(async move {
            place_frame(&client, &[(&burger, 1)]).await.unwrap()
        }));
    }
    for placed in placing {
        assert_eq!(placed.await.unwrap(), arena_proto::ShopPlaceResult::Ok);
    }
    // Everything placed: wait for the last KOT and the last board.
    let settle = Instant::now();
    while kot_numbers(&rig.fake).len() < total
        || boards
            .iter()
            .any(|b| b.board().is_none_or(|b| b.tickets.len() < total))
    {
        assert!(
            settle.elapsed() < Duration::from_secs(30),
            "the KOTs or boards never caught up: {} of {total} KOTs, boards {:?}, warnings {:?}",
            kot_numbers(&rig.fake).len(),
            boards
                .iter()
                .map(|b| b.board().map(|b| b.tickets.len()))
                .collect::<Vec<_>>(),
            log_text()
                .lines()
                .filter(|l| l.contains("WARN") && !l.contains("reported version"))
                .take(3)
                .collect::<Vec<_>>(),
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    watching.store(false, std::sync::atomic::Ordering::Relaxed);
    let (created, printed) = watcher.await.unwrap();

    let mut numbers = kot_numbers(&rig.fake);
    numbers.sort_unstable();
    let once = numbers == (1..=total as i64).collect::<Vec<_>>();
    let mut latencies: Vec<u64> = created
        .iter()
        .filter_map(|(n, made)| {
            printed
                .get(n)
                .map(|p| p.saturating_duration_since(*made).as_millis() as u64)
        })
        .collect();
    let p95_ms = p95(&mut latencies);

    let hub_tickets: std::collections::BTreeSet<(i64, String)> = ticket_rows(&hub)
        .iter()
        .map(|r| (column(r, "number").parse().unwrap(), column(r, "stage")))
        .collect();
    let boards_match = boards.iter().all(|b| {
        let seen: std::collections::BTreeSet<(i64, String)> = b
            .board()
            .unwrap()
            .tickets
            .iter()
            .map(|t| (t.number, t.stage.clone()))
            .collect();
        seen == hub_tickets
    });
    let report = serde_json::json!({
        "source": "burst",
        "rate": rate,
        "secs": secs,
        "kiosks": kiosks,
        "displays": 2,
        "orders": total,
        "kots": numbers.len(),
        "kot_once_each": once,
        "kot_p95_ms": p95_ms,
        "boards_match": boards_match,
    });
    write_out("ARENA_KITCHEN_BURST_OUT", &report);
    assert!(once, "every KOT prints once: {numbers:?}");
    assert!(boards_match, "both boards match the Hub");
    let allowed = budget_ms("KITCHEN_P95_MS", 3_000);
    assert!(
        p95_ms <= allowed,
        "KOT p95 was {p95_ms} ms, the budget is {allowed} ms"
    );
}

/// A few seconds of the burst, so the ordinary suite exercises the path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_short_burst_across_twenty_kiosks_prints_every_kot_once_and_the_boards_match() {
    burst(5, 3, 20).await;
}

/// The release burst: 5 orders per second for `KITCHEN_BURST_SECS` (60) across 20 kiosks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "the release burst; run by release-bench.yml"]
async fn five_orders_a_second_for_a_minute_across_twenty_kiosks() {
    burst(5, budget_ms("KITCHEN_BURST_SECS", 60), 20).await;
}

/// A short soak so the ordinary suite exercises the path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forty_orders_flow_through_the_kitchen_with_nothing_lost() {
    soak(40).await;
}

/// `KITCHEN_SOAK_ORDERS` (500) orders go through the kitchen, each ticket taken New to Served by a
/// display, and nothing is lost: every ticket Served, every KOT printed once. The outcome is
/// written to `ARENA_KITCHEN_SOAK_OUT` with `KITCHEN_SOAK_LABEL` (`wan-up` or `wan-down`) so the
/// report can check that a run with no route out differs from one with a route in nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "the release soak; run by release-bench.yml"]
async fn five_hundred_orders_flow_through_the_kitchen_with_nothing_lost() {
    soak(budget_ms("KITCHEN_SOAK_ORDERS", 500) as usize).await;
}

async fn soak(orders: usize) {
    let pause = Duration::from_millis(
        budget_ms("KITCHEN_SOAK_SPREAD_SECS", 0) * 1000 / orders.max(1) as u64,
    );
    let hub = start("0.1.0", true).await;
    let rig = kitchen_rig(&hub, 40).await;
    let clients = kiosk_floor(&hub, 5).await;
    let mut observer = owned_plain(hub.web).await;
    hello_ok(
        &mut observer,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;
    let follower = BoardFollower::follow(observer);
    let mut display = hub.plain().await;
    hello_ok(
        &mut display,
        hub.hello(ClientClass::Kds, &hub.kds_token, "0.1.0"),
    )
    .await;

    let mut advances = 0usize;
    for n in 0..orders {
        let number = n as i64 + 1;
        assert_eq!(
            place_frame(&clients[n % clients.len()], &[(&rig.burger, 1)])
                .await
                .unwrap(),
            arena_proto::ShopPlaceResult::Ok
        );
        for (from, to) in [
            ("new", "preparing"),
            ("preparing", "ready"),
            ("ready", "served"),
        ] {
            // The display moves a ticket on the rev it last saw, as the tablet does.
            let waited = Instant::now();
            let seen = loop {
                let found = follower
                    .board()
                    .and_then(|b| b.tickets.iter().find(|t| t.number == number).cloned());
                if let Some(t) = found
                    && t.stage == from
                {
                    break t;
                }
                assert!(
                    waited.elapsed() < Duration::from_secs(10),
                    "ticket {number} never reached the board as {from}"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            };
            let moved = kitchen_move(
                &mut display,
                &format!("m{number}{to}"),
                IntentPayload::KitchenAdvance(arena_proto::KitchenAdvance {
                    ticket_id: seen.id,
                    to: to.into(),
                }),
                seen.rev,
            )
            .await;
            assert!(
                matches!(moved.body, FrameBody::IntentResult(_)),
                "{moved:?}"
            );
            advances += 1;
        }
        // The release soak spreads its orders over `KITCHEN_SOAK_SPREAD_SECS` (8 hours), the display
        // pinging as a tablet does so the Hub keeps its session.
        let mut left = pause;
        while !left.is_zero() {
            let slice = left.min(Duration::from_secs(15));
            tokio::time::sleep(slice).await;
            left -= slice;
            send(&mut display, r#"{"v":1,"kind":"ping","rid":"soak-ping"}"#).await;
        }
    }
    let settle = Instant::now();
    while kot_numbers(&rig.fake).len() < orders {
        assert!(
            settle.elapsed() < Duration::from_secs(30),
            "KOTs never caught up"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let rows = ticket_rows(&hub);
    let served = rows
        .iter()
        .filter(|r| column(r, "stage") == "served")
        .count();
    let mut numbers = kot_numbers(&rig.fake);
    numbers.sort_unstable();
    let duplicate = numbers.windows(2).filter(|w| w[0] == w[1]).count();
    let outcome = serde_json::json!({
        "source": "soak",
        "label": std::env::var("KITCHEN_SOAK_LABEL").unwrap_or_else(|_| "local".into()),
        "orders": orders,
        "tickets": rows.len(),
        "served": served,
        "advances": advances,
        "kots": numbers.len(),
        "duplicate_kots": duplicate,
        "lost": orders.saturating_sub(served),
    });
    write_out("ARENA_KITCHEN_SOAK_OUT", &outcome);
    assert_eq!(rows.len(), orders);
    assert_eq!(served, orders, "every ticket is Served");
    assert_eq!(advances, orders * 3);
    assert_eq!(duplicate, 0, "no KOT printed twice");
    assert_eq!(numbers, (1..=orders as i64).collect::<Vec<_>>());
}

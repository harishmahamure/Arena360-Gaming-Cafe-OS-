//! Live view (Story 8.40): the tunnel listener admits a `live_view` session with a cloud-signed
//! token, pushes the floor and refuses everything else.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel,
};
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, dev_sign_any};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRegister, ErrorCode, Frame, FrameBody,
    Hello, HubEvent, Intent, IntentPayload, IntentSubmit, LiveClaims, LiveFloor, LiveTileState,
    LiveToken, Versions, parse_frame,
};
use arena_store::{LeaseRow, Store, write_lease};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
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
    web: SocketAddr,
    writer: Option<WriterHandle>,
    _status: watch::Sender<HubStatus>,
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
    let hub = LanHub::new(
        live_writer.clone(),
        status_rx,
        clock,
        Arc::new(DeviceDirectory::open(&path).unwrap()),
        cert.server_config().unwrap(),
        "0.1.0",
    )
    .with_live_view(DEV_LICENCE_VERIFY_KEYS.to_vec());
    let mut listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    listeners.bind_tunnel(loopback).await.unwrap();
    let (tunnel, web) = (
        listeners.tunnel_addr().unwrap(),
        listeners.web_addr().unwrap(),
    );
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, Arc::new(hub), async {
        let _ = stop_rx.await;
    }));
    TestHub {
        _dir: dir,
        branch_id,
        tunnel,
        web,
        writer: live_writer,
        _status: status_tx,
        _stop: stop_tx,
    }
}

impl TestHub {
    fn token_for(&self, branch: &str, iat: i64, exp: i64) -> String {
        let mut claims = LiveClaims::new("tenant", branch, iat);
        claims.exp = exp;
        dev_sign_any(&claims).unwrap()
    }

    fn token(&self) -> String {
        self.token_for(&self.branch_id, T0_S, T0_S + 300)
    }

    async fn connect(&self) -> WebSocketStream<impl AsyncRead + AsyncWrite + Unpin> {
        let tcp = TcpStream::connect(self.tunnel).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/live", self.tunnel.port());
        tokio_tungstenite::client_async(url, tcp).await.unwrap().0
    }

    async fn register_pc(&self, label: &str) -> String {
        let secret = new_token_secret().unwrap();
        let applied = self
            .writer
            .as_ref()
            .unwrap()
            .submit_intent(intent(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::DeviceRegister(DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: ClientClass::Paired,
                    label: label.into(),
                    token_hash: hash_token_secret(&secret).unwrap(),
                    tv_number: None,
                    group_id: None,
                }),
            ))
            .await
            .unwrap();
        let id = applied.changes[0].row_id.clone();
        let _ = format_token(&id, &secret);
        id
    }
}

fn hello(token: &str) -> Frame {
    Frame::new(
        "h",
        FrameBody::Hello(Hello {
            credential: token.into(),
            class: ClientClass::LiveView,
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
        match tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("a frame in time")
        {
            Some(Ok(Message::Text(t))) => return Some(parse_frame(t.as_str()).unwrap()),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return None,
            Some(Ok(_)) => {}
        }
    }
}

/// The next `live.floor` event, skipping keepalive pings.
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

fn code_of(frame: &Frame) -> ErrorCode {
    let FrameBody::Error(e) = &frame.body else {
        panic!("expected an error, got {frame:?}")
    };
    e.error.code
}

async fn admitted<S: AsyncRead + AsyncWrite + Unpin>(ws: &mut WebSocketStream<S>, token: &str) {
    send(ws, &hello(token)).await;
    let ok = recv(ws).await.unwrap();
    let FrameBody::HelloOk(ok) = ok.body else {
        panic!("expected hello.ok, got {ok:?}")
    };
    assert_eq!(ok.class, ClientClass::LiveView);
    assert_eq!(ok.device_id, None);
}

async fn refused(hub: &TestHub, token: &str, code: ErrorCode) {
    let mut ws = hub.connect().await;
    send(&mut ws, &hello(token)).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(code_of(&frame), code);
    assert!(recv(&mut ws).await.is_none(), "the connection closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_valid_token_is_admitted_and_gets_the_floor_at_once_then_on_change() {
    let hub = start(true, true).await;
    let mut ws = hub.connect().await;
    admitted(&mut ws, &hub.token()).await;
    let first = next_floor(&mut ws).await;
    assert!(first.tiles.is_empty());
    assert!(first.generated_at > 0);

    let pc = hub.register_pc("PC 7").await;
    let started = std::time::Instant::now();
    let floor = loop {
        let f = next_floor(&mut ws).await;
        if f.tiles.iter().any(|t| t.device_id == pc) {
            break f;
        }
        assert!(started.elapsed() < Duration::from_secs(2), "within 2 s");
    };
    let tile = floor.tiles.iter().find(|t| t.device_id == pc).unwrap();
    assert_eq!(tile.label, "PC 7");
    assert_eq!(
        tile.state,
        LiveTileState::Offline,
        "nobody paired to it is connected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_viewer_that_sends_anything_but_a_ping_is_forbidden_and_kept() {
    let hub = start(true, true).await;
    let mut ws = hub.connect().await;
    admitted(&mut ws, &hub.token()).await;
    let _ = next_floor(&mut ws).await;
    let submit = Frame::new(
        "i1",
        FrameBody::IntentSubmit(IntentSubmit {
            intent: intent(
                system(),
                IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "x".into(),
                    branch_code: "X".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 0,
                    fy_start_month: 4,
                    onboarding: None,
                }),
            ),
        }),
    );
    send(&mut ws, &submit).await;
    let reply = loop {
        let f = recv(&mut ws).await.expect("the session stays open");
        if !matches!(f.body, FrameBody::Event(_)) {
            break f;
        }
    };
    assert_eq!(
        (reply.rid.as_str(), code_of(&reply)),
        ("i1", ErrorCode::Forbidden)
    );
    for (rid, raw) in [
        (
            "g1",
            r#"{"v":1,"kind":"settings.get","rid":"g1","keys":["kiosk.x"]}"#,
        ),
        (
            "k1",
            r#"{"v":1,"kind":"kiosk.verify_admin_pin","rid":"k1","pin_proof":"x"}"#,
        ),
    ] {
        ws.send(Message::text(raw.to_owned())).await.unwrap();
        let reply = loop {
            let f = recv(&mut ws).await.expect("the session stays open");
            if !matches!(f.body, FrameBody::Event(_)) && f.rid == rid {
                break f;
            }
        };
        assert_eq!(code_of(&reply), ErrorCode::Forbidden, "{rid}");
    }
    ws.send(Message::text(
        r#"{"v":1,"kind":"ping","rid":"p"}"#.to_owned(),
    ))
    .await
    .unwrap();
    loop {
        let f = recv(&mut ws).await.expect("the session stays open");
        if f.rid == "p" {
            assert_eq!(f.body, FrameBody::Pong);
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_tokens_and_unentitled_branches_are_refused() {
    let hub = start(true, true).await;
    refused(&hub, "garbage", ErrorCode::TokenInvalid).await;
    refused(
        &hub,
        &hub.token_for("another-branch", T0_S, T0_S + 300),
        ErrorCode::TokenInvalid,
    )
    .await;
    refused(
        &hub,
        &hub.token_for(&hub.branch_id, T0_S - 400, T0_S - 100),
        ErrorCode::TokenInvalid,
    )
    .await;
    // A licence token cannot stand in for a live token.
    let licence =
        arena_proto::LicenceClaims::for_tier("t", &hub.branch_id, arena_proto::Tier::Premium, T0_S);
    refused(
        &hub,
        &dev_sign_any(&licence).unwrap(),
        ErrorCode::TokenInvalid,
    )
    .await;

    let no_flag = start(false, true).await;
    refused(&no_flag, &no_flag.token(), ErrorCode::FeatureNotEntitled).await;

    let follower = start(true, false).await;
    refused(&follower, &follower.token(), ErrorCode::NotPrimary).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_tunnel_is_the_only_way_in_for_a_live_viewer() {
    let hub = start(true, true).await;
    let tcp = TcpStream::connect(hub.web).await.unwrap();
    let url = format!("ws://127.0.0.1:{}/ws", hub.web.port());
    let mut ws = tokio_tungstenite::client_async(url, tcp).await.unwrap().0;
    send(&mut ws, &hello(&hub.token())).await;
    let frame = recv(&mut ws).await.unwrap();
    assert_eq!(code_of(&frame), ErrorCode::Forbidden);

    // The tunnel takes a live viewer or a paired device with its token: a paired hello with a
    // bad token, and any browser class, is refused `TOKEN_INVALID` (Story 8.41).
    let mut ws = hub.connect().await;
    let paired = Frame::new(
        "h",
        FrameBody::Hello(Hello {
            credential: "x.y".into(),
            class: ClientClass::Paired,
            versions: Versions {
                app: "0.1.0".into(),
            },
            capabilities: vec![],
        }),
    );
    send(&mut ws, &paired).await;
    assert_eq!(
        code_of(&recv(&mut ws).await.unwrap()),
        ErrorCode::TokenInvalid
    );

    // The tunnel listener answers its WebSocket at `/live` or `/` and nothing else.
    let tcp = TcpStream::connect(hub.tunnel).await.unwrap();
    let url = format!("ws://127.0.0.1:{}/other", hub.tunnel.port());
    assert!(tokio_tungstenite::client_async(url, tcp).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_that_runs_out_closes_the_session_unless_it_is_refreshed() {
    let hub = start(true, true).await;
    let short = hub.token_for(&hub.branch_id, T0_S - 298, T0_S + 2);

    let mut ws = hub.connect().await;
    admitted(&mut ws, &short).await;
    let started = std::time::Instant::now();
    loop {
        let frame = recv(&mut ws).await.expect("an error before the close");
        if matches!(frame.body, FrameBody::Error(_)) {
            assert_eq!(code_of(&frame), ErrorCode::TokenInvalid);
            break;
        }
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(recv(&mut ws).await.is_none(), "then it closes");

    // A fresh token on the open socket extends the view; a bad one is refused and changes nothing.
    let mut ws = hub.connect().await;
    admitted(&mut ws, &short).await;
    send(
        &mut ws,
        &Frame::new(
            "r0",
            FrameBody::LiveToken(LiveToken {
                token: "junk".into(),
            }),
        ),
    )
    .await;
    send(
        &mut ws,
        &Frame::new("r1", FrameBody::LiveToken(LiveToken { token: hub.token() })),
    )
    .await;
    let mut saw = (false, false);
    while saw != (true, true) {
        let f = recv(&mut ws).await.expect("still open");
        match (f.rid.as_str(), &f.body) {
            ("r0", FrameBody::Error(_)) => {
                assert_eq!(code_of(&f), ErrorCode::TokenInvalid);
                saw.0 = true;
            }
            ("r1", FrameBody::Ack) => saw.1 = true,
            _ => {}
        }
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    ws.send(Message::text(
        r#"{"v":1,"kind":"ping","rid":"p"}"#.to_owned(),
    ))
    .await
    .unwrap();
    loop {
        let f = recv(&mut ws)
            .await
            .expect("past the old expiry the view is still open");
        if f.rid == "p" {
            break;
        }
    }
}

#[tokio::test]
async fn the_tunnel_listener_is_loopback_only() {
    let mut listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let any = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
    assert!(listeners.bind_tunnel(any).await.is_err());
    assert!(listeners.tunnel_addr().is_none());
}

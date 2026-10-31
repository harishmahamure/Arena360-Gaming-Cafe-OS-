//! The main Hub serves its linked standby, and a standby follows it (Story 9.19): a real TLS
//! server, a pinned-TLS link and the `arena-tasks` follower engine over loopback.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, Role, Writer, WriterHandle, status_channel,
};
use arena_kiosk_core::{HubStream, connect_ws, pinned_config};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, ClientClass, DeviceRegister, ErrorCode, Intent,
    IntentPayload, REPLICATE_PATH, REPLICATE_SNAPSHOT_PATH, ReplicateFrame, SettingsSet,
};
use arena_store::{LeaseRow, Store, link_standby, write_lease};
use arena_tasks::{FollowStatus, Follower, FollowerConfig, LinkError, ReplicationLink};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::tungstenite::Message;

const T0: i64 = 1_800_000_000_000;
const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

fn intent(actor: Actor, payload: IntentPayload, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: T0,
        base_rev,
        actor,
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "test-owner".into(),
    }
}

struct Main {
    _dir: tempfile::TempDir,
    path: PathBuf,
    lan: SocketAddr,
    web: SocketAddr,
    fingerprint: String,
    standby_token: String,
    kiosk_token: String,
    writer: Option<WriterHandle>,
    _status: watch::Sender<HubStatus>,
    _stop: oneshot::Sender<()>,
}

async fn register(w: &WriterHandle, device_id: Option<String>, label: &str) -> (String, String) {
    let secret = new_token_secret().unwrap();
    let applied = w
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceRegister(DeviceRegister {
                device_id,
                device_kind: Default::default(),
                class: ClientClass::Paired,
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
    (id.clone(), format_token(&id, &secret))
}

async fn start(primary: bool) -> Main {
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
            Actor {
                kind: ActorKind::System,
                id: "test".into(),
            },
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
    let (standby_id, standby_token) = register(&writer, None, "Standby PC").await;
    // The owner linked this device as the standby (a second handle: the writer is idle).
    Store::open(&path, T0)
        .unwrap()
        .write_tx(|tx| link_standby(tx, &standby_id, T0))
        .unwrap();
    let (_, kiosk_token) = register(&writer, None, "Kiosk").await;
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
            "0.1.0",
        )
        .with_replication(path.clone()),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let (lan, web) = (listeners.lan_addr().unwrap(), listeners.web_addr().unwrap());
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));
    Main {
        _dir: dir,
        path,
        lan,
        web,
        fingerprint: cert.fingerprint,
        standby_token,
        kiosk_token,
        writer,
        _status: status_tx,
        _stop: stop_tx,
    }
}

impl Main {
    async fn set(&self, value: &str, rev: i64) {
        self.writer
            .as_ref()
            .unwrap()
            .submit_intent(intent(
                owner(),
                IntentPayload::SettingsSet(SettingsSet {
                    key: "receipt.footer".into(),
                    value: value.into(),
                }),
                Some(rev),
            ))
            .await
            .unwrap();
    }

    fn changes(&self) -> Vec<arena_store::ChangeRecord> {
        Store::open_reader(&self.path).unwrap().changes().unwrap()
    }

    fn link(&self, token: &str) -> WsLink {
        WsLink {
            lan: self.lan,
            web: self.web,
            fingerprint: self.fingerprint.clone(),
            token: token.to_owned(),
            schema_override: None,
            stream: None,
        }
    }
}

/// The standby's link over pinned TLS: HTTPS for the snapshot, a WebSocket for the stream.
struct WsLink {
    lan: SocketAddr,
    web: SocketAddr,
    fingerprint: String,
    token: String,
    schema_override: Option<u32>,
    stream: Option<HubStream>,
}

fn io(e: impl std::fmt::Display) -> LinkError {
    LinkError::Io(e.to_string())
}

impl ReplicationLink for WsLink {
    async fn snapshot(&mut self, into: PathBuf) -> Result<Option<(String, i64)>, LinkError> {
        let config = pinned_config(&self.fingerprint).map_err(io)?;
        let tcp = TcpStream::connect(self.web).await.map_err(io)?;
        let name = rustls::pki_types::ServerName::try_from("hub.local").unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(io)?;
        let request = format!(
            "GET {REPLICATE_SNAPSHOT_PATH} HTTP/1.1\r\nHost: hub\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            self.token
        );
        tls.write_all(request.as_bytes()).await.map_err(io)?;
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw).await.map_err(io)?;
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| io("no response head"))?;
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head.split(' ').nth(1).unwrap_or("");
        match status {
            "200" => {}
            "401" => return Err(LinkError::Refused(ErrorCode::TokenInvalid, head)),
            "403" => return Err(LinkError::Refused(ErrorCode::Forbidden, head)),
            "503" => return Err(LinkError::Refused(ErrorCode::NotPrimary, head)),
            _ => return Err(io(head)),
        }
        let header = |name: &str| {
            head.split("\r\n").skip(1).find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_owned())
            })
        };
        let epoch = header("x-arena-epoch").unwrap_or_default();
        let seq: i64 = header("x-arena-seq")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        std::fs::write(&into, &raw[split + 4..]).map_err(io)?;
        Ok((!epoch.is_empty()).then_some((epoch, seq)))
    }

    async fn open(
        &mut self,
        from: Option<(String, i64)>,
        schema_version: u32,
    ) -> Result<Option<(String, i64)>, LinkError> {
        let url = format!("wss://127.0.0.1:{}{REPLICATE_PATH}", self.lan.port());
        let mut ws = connect_ws(&url, &self.fingerprint).await.map_err(io)?;
        let open = ReplicateFrame::Open {
            token: self.token.clone(),
            schema_version: self.schema_override.unwrap_or(schema_version),
            from: from.map(|(epoch, seq)| arena_proto::ReplicatePosition { epoch, seq }),
        };
        ws.send(Message::text(serde_json::to_string(&open).unwrap()))
            .await
            .map_err(io)?;
        let first = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .map_err(io)?;
        let frame = match first {
            Some(Ok(Message::Text(t))) => serde_json::from_str::<ReplicateFrame>(&t).map_err(io)?,
            _ => return Err(io("closed before ready")),
        };
        match frame {
            ReplicateFrame::Ready { tip } => {
                self.stream = Some(ws);
                Ok(tip.map(|p| (p.epoch, p.seq)))
            }
            ReplicateFrame::Refused { code, message } => Err(LinkError::Refused(code, message)),
            other => Err(io(format!("unexpected {other:?}"))),
        }
    }

    async fn next(&mut self) -> Result<Option<ReplicateFrame>, LinkError> {
        let Some(ws) = self.stream.as_mut() else {
            return Ok(None);
        };
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    return Ok(Some(serde_json::from_str(&t).map_err(io)?));
                }
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(io(e)),
            }
        }
    }
}

fn standby_store(dir: &Path) -> Store {
    Store::open(dir.join("standby.db"), T0).unwrap()
}

fn fast() -> FollowerConfig {
    FollowerConfig {
        backoff: Arc::new(|_| Duration::from_millis(50)),
    }
}

/// Waits until the follower reports it has applied through `want` and the stream is live.
async fn caught_up(rx: &mut watch::Receiver<FollowStatus>, want: (String, i64)) {
    let wait = async {
        loop {
            if let FollowStatus::Following { applied, live } = &*rx.borrow()
                && *live
                && applied.as_ref() == Some(&want)
            {
                return;
            }
            rx.changed().await.unwrap();
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .expect("the standby caught up");
}

fn tip(changes: &[arena_store::ChangeRecord]) -> (String, i64) {
    let last = changes.last().unwrap();
    (last.epoch.clone(), last.seq)
}

#[tokio::test]
async fn a_standby_bootstraps_then_follows_live_and_resumes_after_a_restart() {
    let main = start(true).await;
    main.set("thanks", 1).await;
    let dir = tempfile::tempdir().unwrap();

    // Bootstrap from the snapshot, then follow commits as they land.
    let (mut follower, mut status) = Follower::new(
        standby_store(dir.path()),
        main.link(&main.standby_token),
        dir.path().join("snapshot.part"),
        fast(),
    );
    let (stop_tx, stop_rx) = watch::channel(false);
    let job = tokio::spawn(async move {
        follower.run(stop_rx).await;
        follower
    });
    caught_up(&mut status, tip(&main.changes())).await;
    main.set("welcome", 2).await;
    main.set("see you", 3).await;
    caught_up(&mut status, tip(&main.changes())).await;

    let standby = Store::open_reader(dir.path().join("standby.db")).unwrap();
    assert_eq!(standby.changes().unwrap(), main.changes());
    assert!(!dir.path().join("snapshot.part").exists());

    // Stop it, let the main Hub move on, and start a new follower on the same database: it
    // resumes from its own tip (no snapshot, no gap, no duplicate).
    stop_tx.send(true).unwrap();
    drop(job.await.unwrap());
    main.set("again", 4).await;
    let (mut again, mut status) = Follower::new(
        standby_store(dir.path()),
        main.link(&main.standby_token),
        dir.path().join("snapshot.part"),
        fast(),
    );
    let (_keep, stop_rx) = watch::channel(false);
    let job = tokio::spawn(async move { again.run(stop_rx).await });
    caught_up(&mut status, tip(&main.changes())).await;
    assert!(!matches!(*status.borrow(), FollowStatus::Bootstrapping));
    let standby = Store::open_reader(dir.path().join("standby.db")).unwrap();
    assert_eq!(standby.changes().unwrap(), main.changes());
    job.abort();
}

#[tokio::test]
async fn only_the_linked_standby_may_replicate() {
    let main = start(true).await;
    let dir = tempfile::tempdir().unwrap();

    // A paired kiosk: the snapshot and the stream are both forbidden.
    let mut kiosk = main.link(&main.kiosk_token);
    let err = kiosk.snapshot(dir.path().join("s.db")).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::Forbidden, _)),
        "{err:?}"
    );
    let err = kiosk.open(None, 90).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::Forbidden, _)),
        "{err:?}"
    );

    // A token no one holds.
    let mut stranger =
        main.link("nobody.0000000000000000000000000000000000000000000000000000000000000000");
    let err = stranger.open(None, 90).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::TokenInvalid, _)),
        "{err:?}"
    );
    assert!(!dir.path().join("s.db").exists());
}

#[tokio::test]
async fn a_schema_behind_standby_is_refused_and_stops() {
    let main = start(true).await;
    let mut behind = main.link(&main.standby_token);
    behind.schema_override = Some(1);
    let err = behind.open(None, 90).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::SchemaBehind, _)),
        "{err:?}"
    );

    let dir = tempfile::tempdir().unwrap();
    let mut link = main.link(&main.standby_token);
    link.schema_override = Some(1);
    let (mut follower, mut status) = Follower::new(
        standby_store(dir.path()),
        link,
        dir.path().join("snapshot.part"),
        fast(),
    );
    let (_keep, stop_rx) = watch::channel(false);
    tokio::time::timeout(Duration::from_secs(20), follower.run(stop_rx))
        .await
        .expect("the follower stops by itself");
    assert_eq!(*status.borrow_and_update(), FollowStatus::Behind);
}

#[tokio::test]
async fn a_position_the_main_hub_does_not_hold_is_a_gap() {
    let main = start(true).await;
    let mut link = main.link(&main.standby_token);
    let err = link
        .open(Some(("9:elsewhere".into(), 5)), 90)
        .await
        .unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::ReplicateGap, _)),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_hub_that_is_not_primary_serves_no_standby() {
    let main = start(false).await;
    let mut link = main.link(&main.standby_token);
    let err = link.open(None, 90).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::NotPrimary, _)),
        "{err:?}"
    );
    let dir = tempfile::tempdir().unwrap();
    let err = link.snapshot(dir.path().join("s.db")).await.unwrap_err();
    assert!(
        matches!(err, LinkError::Refused(ErrorCode::NotPrimary, _)),
        "{err:?}"
    );
}

/// A link that fails the way a network does, to test the engine's reconnect and resync.
struct Flaky {
    inner: WsLink,
    /// Frames to pass on the first connection before it "closes".
    drop_after: Option<usize>,
    /// Refuse the first open as a gap.
    gap_once: bool,
    opens: Arc<std::sync::atomic::AtomicUsize>,
    snapshots: Arc<std::sync::atomic::AtomicUsize>,
}

impl ReplicationLink for Flaky {
    async fn snapshot(&mut self, into: PathBuf) -> Result<Option<(String, i64)>, LinkError> {
        self.snapshots
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.snapshot(into).await
    }

    async fn open(
        &mut self,
        from: Option<(String, i64)>,
        schema_version: u32,
    ) -> Result<Option<(String, i64)>, LinkError> {
        self.opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if std::mem::take(&mut self.gap_once) {
            return Err(LinkError::Refused(ErrorCode::ReplicateGap, "gap".into()));
        }
        self.inner.open(from, schema_version).await
    }

    async fn next(&mut self) -> Result<Option<ReplicateFrame>, LinkError> {
        if let Some(left) = self.drop_after.as_mut() {
            if *left == 0 {
                self.drop_after = None;
                self.inner.stream = None;
                return Ok(None);
            }
            *left -= 1;
        }
        self.inner.next().await
    }
}

fn flaky(
    main: &Main,
    drop_after: Option<usize>,
    gap_once: bool,
) -> (Flaky, [Arc<std::sync::atomic::AtomicUsize>; 2]) {
    let opens: Arc<std::sync::atomic::AtomicUsize> = Arc::default();
    let snapshots: Arc<std::sync::atomic::AtomicUsize> = Arc::default();
    let link = Flaky {
        inner: main.link(&main.standby_token),
        drop_after,
        gap_once,
        opens: Arc::clone(&opens),
        snapshots: Arc::clone(&snapshots),
    };
    (link, [opens, snapshots])
}

#[tokio::test]
async fn a_dropped_stream_reconnects_and_resumes_without_a_new_snapshot() {
    use std::sync::atomic::Ordering::SeqCst;
    let main = start(true).await;
    let dir = tempfile::tempdir().unwrap();
    let (link, [opens, snapshots]) = flaky(&main, Some(1), false);
    let (mut follower, mut status) = Follower::new(
        standby_store(dir.path()),
        link,
        dir.path().join("snapshot.part"),
        fast(),
    );
    let (_keep, stop_rx) = watch::channel(false);
    let job = tokio::spawn(async move { follower.run(stop_rx).await });
    caught_up(&mut status, tip(&main.changes())).await;
    // The stream has dropped; rows committed meanwhile arrive once it is back.
    main.set("after the drop", 1).await;
    main.set("and again", 2).await;
    caught_up(&mut status, tip(&main.changes())).await;
    let standby = Store::open_reader(dir.path().join("standby.db")).unwrap();
    assert_eq!(standby.changes().unwrap(), main.changes());
    assert!(opens.load(SeqCst) >= 2, "it reconnected");
    assert_eq!(
        snapshots.load(SeqCst),
        1,
        "it resumed, it did not start over"
    );
    job.abort();
}

#[tokio::test]
async fn a_position_gap_takes_a_fresh_snapshot() {
    use std::sync::atomic::Ordering::SeqCst;
    let main = start(true).await;
    let dir = tempfile::tempdir().unwrap();
    let (link, [opens, snapshots]) = flaky(&main, None, true);
    let (mut follower, mut status) = Follower::new(
        standby_store(dir.path()),
        link,
        dir.path().join("snapshot.part"),
        fast(),
    );
    let (_keep, stop_rx) = watch::channel(false);
    let job = tokio::spawn(async move { follower.run(stop_rx).await });
    caught_up(&mut status, tip(&main.changes())).await;
    let standby = Store::open_reader(dir.path().join("standby.db")).unwrap();
    assert_eq!(standby.changes().unwrap(), main.changes());
    assert_eq!(snapshots.load(SeqCst), 2, "the gap forced a new snapshot");
    assert_eq!(opens.load(SeqCst), 2);
    job.abort();
}

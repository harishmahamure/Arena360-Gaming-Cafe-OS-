//! The Hub's game media cache end to end (Story 6.2): the pull task fills the reference cache
//! from a stand-in cloud, `GET /media/<sha256>` serves it to a paired kiosk over TLS on `:7401`,
//! and none of it reaches the branch store.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_api::lan::{HubCert, LanHub, bind_lan, serve_lan};
use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_core::media_cache::{CapFn, MediaCache};
use arena_core::{
    DeviceDirectory, HubStatus, LeaseInfo, ManualClock, PairGate, Role, SnapshotReader, Writer,
    WriterHandle, status_channel,
};
use arena_proto::{
    Actor, ActorKind, BranchBootstrap, CatalogGame, CatalogManifest, CatalogMedia, ClientClass,
    DeviceRegister, DeviceRevoke, Intent, IntentPayload,
};
use arena_store::{LeaseRow, Store, write_lease};
use arena_tasks::{CATALOG_INTERVAL, CatalogPullTask, CloudClient, CloudEndpoint, Keystore};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};

const T0: i64 = 1_800_000_000_000;
const HUB_ID: &str = "01JHUBAAAAAAAAAAAAAAAAAAAA";

const COVER: &[u8] = b"0123456789 the cover of the first game, long enough to slice";
const COVER_2: &[u8] = b"a second cover";
const VIDEO: &[u8] = b"the video of the first game";
const GAME_NAME: &str = "Quarantined Quest";

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

fn owner() -> Actor {
    Actor {
        kind: ActorKind::Owner,
        id: "test-owner".into(),
    }
}

struct Mem(Mutex<Option<Vec<u8>>>);

impl Keystore for Mem {
    fn put(&self, secret: &[u8]) -> std::io::Result<()> {
        *self.0.lock().unwrap() = Some(secret.to_vec());
        Ok(())
    }
    fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

/// A stand-in cloud and CDN: answers from `routes` by path, one request per connection.
async fn cloud(routes: Routes) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            let head = String::from_utf8_lossy(&raw).into_owned();
            let path = head
                .split(' ')
                .nth(1)
                .unwrap_or("")
                .split('?')
                .next()
                .unwrap_or("")
                .to_owned();
            let (status, body) = routes
                .lock()
                .unwrap()
                .get(&path)
                .cloned()
                .unwrap_or((404, b"{}".to_vec()));
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(reply.as_bytes()).await;
            let _ = sock.write_all(&body).await;
        }
    });
    port
}

fn put(routes: &Routes, path: &str, status: u16, body: &[u8]) {
    routes
        .lock()
        .unwrap()
        .insert(path.to_owned(), (status, body.to_vec()));
}

fn media(port: u16, kind: &str, body: &[u8]) -> CatalogMedia {
    CatalogMedia {
        kind: kind.into(),
        sha256: hex(body),
        bytes: body.len() as u64,
        url: format!("http://127.0.0.1:{port}/catalog/{}.png", hex(body)),
    }
}

fn game(id: &str, name: &str, media: Vec<CatalogMedia>) -> CatalogGame {
    CatalogGame {
        id: id.into(),
        rev: 1,
        deleted: false,
        name: Some(name.into()),
        launcher: Some("steam".into()),
        category: None,
        age_rating: None,
        signatures: vec![],
        media,
    }
}

struct Rig {
    dir: tempfile::TempDir,
    branch_db: PathBuf,
    web: SocketAddr,
    fingerprint: String,
    token: String,
    device_id: String,
    writer: WriterHandle,
    cache: Arc<MediaCache>,
    routes: Routes,
    cloud_port: u16,
    task: CatalogPullTask,
    status: watch::Sender<HubStatus>,
    _stop: oneshot::Sender<()>,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let branch_db = dir.path().join("branch.db");
    let mut store = Store::open(&branch_db, T0).unwrap();
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
        ))
        .await
        .unwrap();
    let secret = new_token_secret().unwrap();
    let applied = writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceRegister(DeviceRegister {
                device_id: None,
                device_kind: Default::default(),
                class: ClientClass::Paired,
                label: "Dev Kiosk".into(),
                token_hash: hash_token_secret(&secret).unwrap(),
                tv_number: None,
                group_id: None,
            }),
        ))
        .await
        .unwrap();
    let device_id = applied.changes[0].row_id.clone();
    let token = format_token(&device_id, &secret);

    let routes: Routes = Arc::default();
    let cloud_port = cloud(Arc::clone(&routes)).await;

    // The cache, exactly as the Hub builds it: under `reference/`, the cap from the settings.
    let snapshots = Arc::new(SnapshotReader::open(&branch_db).unwrap());
    let cap_source = Arc::clone(&snapshots);
    let cap: CapFn = Arc::new(move || {
        arena_core::media_cache::media_cap_bytes(&cap_source.settings().unwrap_or_default())
    });
    let cache = Arc::new(MediaCache::open(dir.path().join("reference"), cap).unwrap());

    let cert = HubCert::generate(None).unwrap();
    status_tx.send_modify(|s| s.tls_fingerprint = Some(cert.fingerprint.clone()));
    let hub = Arc::new(
        LanHub::new(
            Some(writer.clone()),
            status_rx,
            clock.clone(),
            Arc::new(DeviceDirectory::open(&branch_db).unwrap()),
            cert.server_config().unwrap(),
            "0.1.0",
        )
        .with_media(Arc::clone(&cache)),
    );
    let listeners = bind_lan(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0)
        .await
        .unwrap();
    let web = listeners.web_addr().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(serve_lan(listeners, hub, async {
        let _ = stop_rx.await;
    }));

    let task = CatalogPullTask::new(
        CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{cloud_port}"), true).unwrap(),
        )
        .unwrap(),
        Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec())))),
        PairGate::new(true),
        status_tx.clone(),
        Arc::clone(&cache),
        clock,
    );
    Rig {
        dir,
        branch_db,
        web,
        fingerprint: cert.fingerprint,
        token,
        device_id,
        writer,
        cache,
        routes,
        cloud_port,
        task,
        status: status_tx,
        _stop: stop_tx,
    }
}

impl Rig {
    /// Publishes game one (two covers and a video) and, unless `video_missing`, the files.
    fn publish(&self, video_available: bool) {
        let m = CatalogManifest {
            catalog_version: 3,
            games: vec![
                game(
                    "G1",
                    GAME_NAME,
                    vec![
                        media(self.cloud_port, "cover", COVER),
                        media(self.cloud_port, "video", VIDEO),
                    ],
                ),
                game(
                    "G2",
                    "Second Game",
                    vec![media(self.cloud_port, "cover", COVER_2)],
                ),
            ],
        };
        put(
            &self.routes,
            "/v1/catalog/manifest",
            200,
            &serde_json::to_vec(&m).unwrap(),
        );
        for (body, available) in [(COVER, true), (COVER_2, true), (VIDEO, video_available)] {
            if available {
                put(
                    &self.routes,
                    &format!("/catalog/{}.png", hex(body)),
                    200,
                    body,
                );
            }
        }
    }

    /// `GET path` over TLS on `:7401`, with `Authorization: Bearer <token>` when given and any
    /// `extra` header lines. The status, the header text and the body.
    async fn get(&self, path: &str, token: Option<&str>, extra: &str) -> (u16, String, Vec<u8>) {
        let config = arena_kiosk_core::pinned_config(&self.fingerprint).unwrap();
        let tcp = TcpStream::connect(self.web).await.unwrap();
        let name = rustls::pki_types::ServerName::IpAddress(self.web.ip().into());
        let mut tls = tokio_rustls::TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: hub\r\n{auth}{extra}\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut out)).await;
        split(&out)
    }

    async fn media(&self, sha: &str, extra: &str) -> (u16, String, Vec<u8>) {
        self.get(&format!("/media/{sha}"), Some(&self.token.clone()), extra)
            .await
    }
}

fn split(raw: &[u8]) -> (u16, String, Vec<u8>) {
    let at = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&raw[..at]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status");
    (status, head, raw[at + 4..].to_vec())
}

fn has(head: &str, line: &str) -> bool {
    head.lines().any(|l| l.eq_ignore_ascii_case(line))
}

/// Every byte of a database file and its WAL.
fn raw(db: &Path) -> Vec<u8> {
    let mut all = std::fs::read(db).unwrap_or_default();
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    all.extend(std::fs::read(PathBuf::from(wal)).unwrap_or_default());
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_kiosk_gets_cached_media_with_immutable_headers() {
    let mut r = rig().await;
    r.publish(true);
    assert_eq!(r.task.pull().await, CATALOG_INTERVAL);

    let (status, head, body) = r.media(&hex(COVER), "").await;
    assert_eq!(status, 200, "{head}");
    assert_eq!(body, COVER);
    assert!(has(
        &head,
        "Cache-Control: public, max-age=31536000, immutable"
    ));
    assert!(has(&head, &format!("ETag: \"{}\"", hex(COVER))));
    assert!(has(&head, "X-Content-Type-Options: nosniff"));
    assert!(has(&head, "Content-Type: image/png"));
    assert!(has(&head, "Accept-Ranges: bytes"));
    assert!(has(&head, &format!("Content-Length: {}", COVER.len())));
    // The video was stored too, and served as the same kind of file.
    let (status, _, body) = r.media(&hex(VIDEO), "").await;
    assert_eq!((status, body.as_slice()), (200, VIDEO));

    // A served file is noted as served.
    let served = r
        .cache
        .files()
        .unwrap()
        .into_iter()
        .find(|f| f.sha256 == hex(VIDEO))
        .unwrap();
    assert_eq!(served.last_served_ms, Some(T0));

    // One range is honoured; an unsatisfiable one is 416; the matching ETag is 304.
    let (status, head, body) = r.media(&hex(COVER), "Range: bytes=2-5\r\n").await;
    assert_eq!((status, body.as_slice()), (206, &COVER[2..6]), "{head}");
    assert!(has(
        &head,
        &format!("Content-Range: bytes 2-5/{}", COVER.len())
    ));
    let (status, head, body) = r
        .media(&hex(COVER), &format!("Range: bytes={}-\r\n", COVER.len()))
        .await;
    assert_eq!((status, body.len()), (416, 0));
    assert!(has(
        &head,
        &format!("Content-Range: bytes */{}", COVER.len())
    ));
    let (status, _, body) = r
        .media(
            &hex(COVER),
            &format!("If-None-Match: \"{}\"\r\n", hex(COVER)),
        )
        .await;
    assert_eq!((status, body.len()), (304, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_malformed_and_not_yet_downloaded_hashes_are_404() {
    let mut r = rig().await;
    // The video is listed but its file is not available yet.
    r.publish(false);
    r.task.pull().await;
    let started = std::time::Instant::now();
    for sha in [
        hex(VIDEO),
        hex(b"nobody has this"),
        "not-a-hash".to_owned(),
        "..%2F..%2Fbranch.db".to_owned(),
        "z".repeat(64),
        String::new(),
    ] {
        let (status, _, body) = r.media(&sha, "").await;
        assert_eq!((status, body.len()), (404, 0), "{sha}");
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "answered at once"
    );
    let (status, ..) = r.media(&hex(COVER), "").await;
    assert_eq!(status, 200, "the covers that did arrive are served");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unauthenticated_or_revoked_request_is_refused_and_failures_are_throttled() {
    let mut r = rig().await;
    r.publish(true);
    r.task.pull().await;
    let path = format!("/media/{}", hex(COVER));

    // No header, a malformed token and an unknown device are 401 with no body detail.
    let unknown = format!("{}.{}", "0".repeat(26), "a".repeat(43));
    for token in [None, Some("garbage"), Some(unknown.as_str())] {
        let (status, head, body) = r.get(&path, token, "").await;
        assert_eq!(status, 401, "{token:?}");
        assert!(body.is_empty());
        assert!(!head.contains(&hex(COVER)));
    }
    // Plain HTTP is refused even with the right token.
    let mut tcp = TcpStream::connect(r.web).await.unwrap();
    tcp.write_all(
        format!(
            "GET {path} HTTP/1.1\r\nHost: hub\r\nAuthorization: Bearer {}\r\n\r\n",
            r.token
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut out)).await;
    let (status, _, body) = split(&out);
    // Story 7.6: plain HTTP answers only the kitchen, the socket and the info pages; any other
    // path is FORBIDDEN, with no file bytes.
    assert_eq!(status, 403);
    assert!(!String::from_utf8_lossy(&body).contains(&hex(COVER)));

    // The right token works, a revoked one does not.
    assert_eq!(r.media(&hex(COVER), "").await.0, 200);
    r.writer
        .submit_intent(intent(
            owner(),
            IntentPayload::DeviceRevoke(DeviceRevoke {
                device_id: r.device_id.clone(),
            }),
        ))
        .await
        .unwrap();
    assert_eq!(r.media(&hex(COVER), "").await.0, 401);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeated_bad_tokens_block_the_address_even_for_a_good_one() {
    let mut r = rig().await;
    r.publish(true);
    r.task.pull().await;
    let path = format!("/media/{}", hex(COVER));
    let bad = format!("{}.{}", r.device_id, "b".repeat(43));
    for _ in 0..5 {
        assert_eq!(r.get(&path, Some(&bad), "").await.0, 401);
    }
    // Shared with `hello`: the sixth, with the right token, is refused without a check.
    assert_eq!(r.media(&hex(COVER), "").await.0, 401);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_wan_keeps_serving_what_is_cached_and_the_next_pull_resumes() {
    let mut r = rig().await;
    r.publish(false);
    r.task.pull().await;
    assert_eq!(r.media(&hex(COVER), "").await.0, 200);
    assert_eq!(r.media(&hex(VIDEO), "").await.0, 404);

    // The cloud goes away entirely.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let live = CloudClient::new(
        &CloudEndpoint::parse(&format!("http://127.0.0.1:{}", r.cloud_port), true).unwrap(),
    )
    .unwrap();
    let down =
        CloudClient::new(&CloudEndpoint::parse(&format!("http://127.0.0.1:{dead}"), true).unwrap())
            .unwrap();
    let mut offline = CatalogPullTask::new(
        down,
        Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec())))),
        PairGate::new(true),
        r.status.clone(),
        Arc::clone(&r.cache),
        Arc::new(ManualClock::new(T0)),
    );
    let wait = offline.pull().await;
    assert!(
        wait < CATALOG_INTERVAL,
        "a failing pull retries soon: {wait:?}"
    );
    let (status, _, body) = r.media(&hex(COVER), "").await;
    assert_eq!(
        (status, body.as_slice()),
        (200, COVER),
        "no player-visible error"
    );
    assert_eq!(r.media(&hex(VIDEO), "").await.0, 404);

    // Back on the air, the video that was pending arrives.
    put(
        &r.routes,
        &format!("/catalog/{}.png", hex(VIDEO)),
        200,
        VIDEO,
    );
    let mut online = CatalogPullTask::new(
        live,
        Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec())))),
        PairGate::new(true),
        r.status.clone(),
        Arc::clone(&r.cache),
        Arc::new(ManualClock::new(T0)),
    );
    assert_eq!(online.pull().await, CATALOG_INTERVAL);
    assert_eq!(r.media(&hex(VIDEO), "").await.0, 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_of_the_cache_reaches_the_change_log_batches_or_snapshots() {
    let mut r = rig().await;
    let reader = SnapshotReader::open(&r.branch_db).unwrap();
    let before_changes = Store::open_reader(&r.branch_db)
        .unwrap()
        .changes()
        .unwrap()
        .len();
    let before_snapshot = serde_json::to_string(&reader.snapshot(T0).unwrap()).unwrap();

    r.publish(true);
    assert_eq!(r.task.pull().await, CATALOG_INTERVAL);
    assert_eq!(r.media(&hex(COVER), "").await.0, 200);

    // The change log did not move.
    let changes = Store::open_reader(&r.branch_db).unwrap().changes().unwrap();
    assert_eq!(changes.len(), before_changes);
    // The snapshot a client would see is unchanged and says nothing of the catalogue.
    let after_snapshot = serde_json::to_string(&reader.snapshot(T0).unwrap()).unwrap();
    assert_eq!(after_snapshot, before_snapshot);
    for needle in [
        GAME_NAME,
        "Second Game",
        hex(COVER).as_str(),
        hex(VIDEO).as_str(),
    ] {
        assert!(!after_snapshot.contains(needle), "{needle}");
        assert!(
            !changes.iter().any(|c| format!("{c:?}").contains(needle)),
            "{needle}"
        );
    }
    // The branch database file itself, including its WAL, never held a byte of it; the
    // reference database, a separate file, does.
    let branch = raw(&r.branch_db);
    let reference = raw(&r.dir.path().join("reference").join("reference.db"));
    for needle in [GAME_NAME, "ref_games", "ref_files", hex(COVER).as_str()] {
        assert!(!contains(&branch, needle), "branch.db holds {needle}");
    }
    assert!(contains(&reference, GAME_NAME) && contains(&reference, "ref_games"));
    // Only the stored files are on disk, under the cache's own directory.
    let media_dir = r.dir.path().join("reference").join("media");
    assert_eq!(std::fs::read(media_dir.join(hex(COVER))).unwrap(), COVER);
}

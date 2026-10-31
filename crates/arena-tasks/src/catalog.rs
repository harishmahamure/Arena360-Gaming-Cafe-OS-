//! Keeps the Hub's copy of the global game catalogue and its media (Story 6.2).
//!
//! The primary Hub asks the control plane for the manifest from the catalogue version it holds
//! (the Hub device token, like the heartbeat) when it starts and every 6 hours, stores games,
//! signatures and media references in the reference database, and then downloads the files the
//! manifest names: every cover first, then videos, each streamed to a `.part` file, checked for
//! size and SHA-256 and renamed. Nothing goes through the writer, `apply()` or the change log.
//!
//! A standby Hub (Story 9.20) runs the same pull for its own reference cache: it asks the cloud for
//! the manifest itself and fetches each file from the main Hub over the LAN first ([`LanMedia`]),
//! from the CDN when the main Hub cannot supply it. Nothing in the cache comes through
//! replication.
//!
//! A failed pull (the WAN is down, the cloud errs, a file does not verify) is retried with the
//! heartbeat's back-off up to 30 minutes; what is already cached keeps being served meanwhile and
//! the next good pull resumes the pending downloads.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use arena_core::media_cache::{Committed, MediaCache};
use arena_core::{Clock, HubStatus, PairGate, PairingState, Role};
use arena_proto::ErrorCode;
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure, MediaFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;

/// How long after a good pull the next one starts.
pub const CATALOG_INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// The longest wait between two tries of a failing pull.
pub const RETRY_CEILING: Duration = Duration::from_secs(30 * 60);
/// How often a Hub that is not the primary writer (yet) looks again.
pub const ROLE_RECHECK: Duration = Duration::from_secs(10);

/// The main Hub's media route as a standby sees it: `GET /media/<sha256>` on the LAN, with the
/// standby's device token and the pinned certificate (wired in with the standby runtime).
pub trait LanMedia: Send + Sync {
    /// Streams the file with this hash into `part`, checked for `bytes` and the hash.
    fn fetch<'a>(
        &'a self,
        sha256: &'a str,
        part: &'a Path,
        bytes: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), MediaFailure>> + Send + 'a>>;
}

pub struct CatalogPullTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    cache: Arc<MediaCache>,
    clock: Arc<dyn Clock>,
    lan: Option<Arc<dyn LanMedia>>,
    fails: u32,
}

impl CatalogPullTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        cache: Arc<MediaCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            gate,
            status,
            cache,
            clock,
            lan: None,
            fails: 0,
        }
    }

    /// Fetches media from the main Hub first (a standby); the CDN stays the fallback.
    #[must_use]
    pub fn with_lan_media(mut self, lan: Arc<dyn LanMedia>) -> Self {
        self.lan = Some(lan);
        self
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        loop {
            let wait = self.pull().await;
            tokio::time::sleep(wait).await;
        }
    }

    /// One pull: the manifest, then the pending downloads. Answers how long to wait before the
    /// next.
    pub async fn pull(&mut self) -> Duration {
        let (role, pairing, standby) = {
            let s = self.status.borrow();
            (s.role, s.pairing, s.following.is_some())
        };
        if role != Role::Primary && !standby {
            return ROLE_RECHECK;
        }
        if pairing == PairingState::Revoked {
            return REVOKED_BACKOFF;
        }
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "catalogue: no hub token");
                return ROLE_RECHECK;
            }
        };
        let cache = Arc::clone(&self.cache);
        let since = match blocking(move || {
            cache.note_cap()?;
            cache.catalog_version()
        })
        .await
        {
            Some(v) => v,
            None => return self.failed(),
        };
        match self.cloud.catalog_manifest(&token, since).await {
            Ok(Some(manifest)) => {
                if manifest.catalog_version != since || !manifest.games.is_empty() {
                    let cache = Arc::clone(&self.cache);
                    let version = manifest.catalog_version;
                    let Some(applied) = blocking(move || cache.apply_manifest(&manifest)).await
                    else {
                        return self.failed();
                    };
                    tracing::info!(
                        catalog_version = version,
                        games = applied.games,
                        files_removed = applied.removed,
                        "catalogue: manifest stored"
                    );
                }
            }
            Ok(None) => {}
            Err(CloudFailure::Refused { error, .. })
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!("catalogue: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                return REVOKED_BACKOFF;
            }
            Err(f) => {
                tracing::warn!(failure = ?f, "catalogue: the manifest call failed");
                return self.failed();
            }
        }
        if self.download_pending().await {
            self.fails = 0;
            CATALOG_INTERVAL
        } else {
            self.failed()
        }
    }

    /// Downloads every pending file, covers first. `false` when any file did not arrive.
    async fn download_pending(&self) -> bool {
        let cache = Arc::clone(&self.cache);
        let Some(pending) = blocking(move || cache.pending()).await else {
            return false;
        };
        let mut all = true;
        for file in pending {
            let cache = Arc::clone(&self.cache);
            let probe = file.clone();
            match blocking(move || cache.admit(&probe)).await {
                Some(true) => {}
                Some(false) => continue,
                None => {
                    all = false;
                    continue;
                }
            }
            let part = self.cache.part_path(&file.sha256);
            match self.fetch_file(&file, &part).await {
                Ok(()) => {
                    let cache = Arc::clone(&self.cache);
                    let stored = file.clone();
                    let now = self.clock.now_ms();
                    match blocking(move || cache.commit(&stored, now)).await {
                        Some(Committed::Stored { evicted }) => {
                            tracing::info!(
                                kind = file.kind.as_str(),
                                bytes = file.bytes,
                                evicted,
                                "catalogue: media stored"
                            );
                        }
                        Some(Committed::NoRoom) => {}
                        None => all = false,
                    }
                }
                Err(MediaFailure::Unreachable(detail)) => {
                    // The WAN is down or dropped: the rest would fail the same way.
                    tracing::warn!(detail = %detail, "catalogue: a media download was cut off");
                    return false;
                }
                Err(failure) => {
                    tracing::warn!(
                        kind = file.kind.as_str(),
                        failure = ?failure,
                        "catalogue: a media file did not verify; it stays pending"
                    );
                    all = false;
                }
            }
        }
        all
    }

    /// One file: from the main Hub over the LAN when this is a standby, else (or when that fails)
    /// from the CDN.
    async fn fetch_file(
        &self,
        file: &arena_store::RefFile,
        part: &Path,
    ) -> Result<(), MediaFailure> {
        if let Some(lan) = &self.lan {
            match lan.fetch(&file.sha256, part, file.bytes).await {
                Ok(()) => return Ok(()),
                Err(failure) => {
                    tracing::info!(failure = ?failure, "catalogue: the main hub could not supply a file; using the cloud");
                    let _ = tokio::fs::remove_file(part).await;
                }
            }
        }
        self.cloud
            .fetch_media(&file.url, part, file.bytes, &file.sha256)
            .await
    }

    fn failed(&mut self) -> Duration {
        self.fails += 1;
        backoff(self.fails, RETRY_CEILING, unit())
    }
}

/// Runs a blocking cache call off the async threads; `None` (logged) when it failed.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, arena_proto::AppError> + Send + 'static,
) -> Option<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), "catalogue: the media cache failed");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "catalogue: the cache task failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use arena_core::ManualClock;
    use arena_core::status_channel;
    use arena_proto::{CatalogGame, CatalogManifest, CatalogMedia};
    use arena_store::FileState;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

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

    type Seen = Arc<Mutex<Vec<String>>>;
    type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

    fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A one-request-per-connection HTTP server answering from `routes` by path (query
    /// stripped); an unrouted path is 404. `Seen` collects every request head.
    async fn serve(routes: Routes) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
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
                log.lock().unwrap().push(head.clone());
                let target = head.split(' ').nth(1).unwrap_or("");
                let path = target.split('?').next().unwrap_or("").to_owned();
                let (status, body) = routes
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"{}".to_vec()));
                let extra = if status == 302 {
                    "location: http://127.0.0.1:1/elsewhere\r\n"
                } else {
                    ""
                };
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/octet-stream\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            }
        });
        (port, seen)
    }

    fn route(routes: &Routes, path: &str, status: u16, body: Vec<u8>) {
        routes
            .lock()
            .unwrap()
            .insert(path.to_owned(), (status, body));
    }

    fn manifest_body(m: &CatalogManifest) -> Vec<u8> {
        serde_json::to_vec(m).unwrap()
    }

    fn refused(code: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"code": code, "message": "no"})).unwrap()
    }

    struct Fx {
        dir: tempfile::TempDir,
        cache: Arc<MediaCache>,
        status: watch::Sender<HubStatus>,
        task: CatalogPullTask,
        routes: Routes,
        seen: Seen,
        port: u16,
        cap: Arc<AtomicU64>,
    }

    async fn fx(cap: u64) -> Fx {
        let routes: Routes = Arc::default();
        let (port, seen) = serve(Arc::clone(&routes)).await;
        build(routes, seen, port, cap)
    }

    fn build(routes: Routes, seen: Seen, port: u16, cap: u64) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let cap = Arc::new(AtomicU64::new(cap));
        let seen_cap = Arc::clone(&cap);
        let cache = Arc::new(
            MediaCache::open(
                dir.path().join("reference"),
                Arc::new(move || seen_cap.load(Ordering::SeqCst)),
            )
            .unwrap(),
        );
        let (status, _rx) = status_channel(HubStatus::opened(12, OWN));
        status.send_modify(|s| s.role = Role::Primary);
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = CatalogPullTask::new(
            cloud,
            keystore,
            PairGate::new(true),
            status.clone(),
            Arc::clone(&cache),
            Arc::new(ManualClock::new(T0)),
        );
        Fx {
            dir,
            cache,
            status,
            task,
            routes,
            seen,
            port,
            cap,
        }
    }

    fn media(port: u16, kind: &str, body: &[u8]) -> CatalogMedia {
        let sha = hex(body);
        CatalogMedia {
            kind: kind.into(),
            url: format!("http://127.0.0.1:{port}/catalog/{sha}.png"),
            sha256: sha,
            bytes: body.len() as u64,
        }
    }

    fn game(id: &str, media: Vec<CatalogMedia>) -> CatalogGame {
        CatalogGame {
            id: id.into(),
            rev: 1,
            deleted: false,
            name: Some(format!("Game {id}")),
            launcher: Some("steam".into()),
            category: None,
            age_rating: None,
            signatures: vec![],
            media,
        }
    }

    /// Serves `m` as the manifest and every media body of `files` at its path.
    fn publish(f: &Fx, m: &CatalogManifest, files: &[&[u8]]) {
        route(&f.routes, "/v1/catalog/manifest", 200, manifest_body(m));
        for body in files {
            route(
                &f.routes,
                &format!("/catalog/{}.png", hex(body)),
                200,
                body.to_vec(),
            );
        }
    }

    fn requests(seen: &Seen, needle: &str) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|h| h.starts_with(&format!("GET {needle}")))
            .cloned()
            .collect()
    }

    fn state(f: &Fx, body: &[u8]) -> Option<FileState> {
        f.cache
            .files()
            .unwrap()
            .into_iter()
            .find(|x| x.sha256 == hex(body))
            .map(|x| x.state)
    }

    fn parts(f: &Fx) -> usize {
        std::fs::read_dir(f.dir.path().join("reference").join("media"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .count()
    }

    const COVER_A: &[u8] = b"cover bytes of game A";
    const COVER_B: &[u8] = b"cover bytes of game B, a little longer";
    const VIDEO_A: &[u8] = b"video bytes of game A that are the largest of all of them";

    #[tokio::test]
    async fn the_first_pull_stores_the_manifest_then_covers_before_the_video() {
        let f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 4,
            games: vec![
                game(
                    "A",
                    vec![
                        media(f.port, "video", VIDEO_A),
                        media(f.port, "cover", COVER_A),
                    ],
                ),
                game("B", vec![media(f.port, "cover", COVER_B)]),
            ],
        };
        publish(&f, &m, &[COVER_A, COVER_B, VIDEO_A]);
        let mut f = f;
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);

        let status = f.cache.status().unwrap();
        assert_eq!(status.catalog_version, 4);
        assert_eq!(status.games, 2);
        assert_eq!(
            status.used_bytes as usize,
            COVER_A.len() + COVER_B.len() + VIDEO_A.len()
        );
        assert_eq!(status.cover_bytes as usize, COVER_A.len() + COVER_B.len());
        for body in [COVER_A, COVER_B, VIDEO_A] {
            assert_eq!(state(&f, body), Some(FileState::Stored));
            let on_disk = std::fs::read(f.cache.file_path(&hex(body))).unwrap();
            assert_eq!(on_disk, body);
        }
        assert_eq!(parts(&f), 0);

        // The manifest call carried the token and the stored version (0); the media calls did
        // not carry any token; covers came before the video.
        let manifest = requests(&f.seen, "/v1/catalog/manifest?since=0");
        assert_eq!(manifest.len(), 1);
        assert!(
            manifest[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer hub_x.y")
        );
        let all = f.seen.lock().unwrap().clone();
        let media_calls: Vec<_> = all
            .iter()
            .filter(|h| h.starts_with("GET /catalog/"))
            .collect();
        assert_eq!(media_calls.len(), 3);
        for call in &media_calls {
            assert!(
                !call.to_ascii_lowercase().contains("authorization"),
                "no token goes to the media URL: {call}"
            );
        }
        assert!(
            media_calls[2].contains(&hex(VIDEO_A)),
            "the video is last: {media_calls:?}"
        );
    }

    #[tokio::test]
    async fn an_unchanged_catalogue_changes_nothing_but_still_resumes_pending_downloads() {
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 2,
            games: vec![game("A", vec![media(f.port, "cover", COVER_A)])],
        };
        publish(&f, &m, &[]);
        // The cover is not there yet: the pull fails and backs off, the row stays pending.
        let wait = f.task.pull().await;
        assert!(wait <= Duration::from_secs(4), "{wait:?}");
        assert_eq!(state(&f, COVER_A), Some(FileState::Pending));
        assert_eq!(f.cache.catalog_version().unwrap(), 2);

        // The cloud now answers 304 for the stored version, and the cover is available.
        route(&f.routes, "/v1/catalog/manifest", 304, vec![]);
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(COVER_A)),
            200,
            COVER_A.to_vec(),
        );
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        let asked = requests(&f.seen, "/v1/catalog/manifest?since=2");
        assert_eq!(asked.len(), 1, "the second pull asked from version 2");
        assert_eq!(state(&f, COVER_A), Some(FileState::Stored));
        assert_eq!(f.cache.catalog_version().unwrap(), 2);
    }

    #[tokio::test]
    async fn a_hash_mismatch_is_discarded_stays_pending_and_is_retried() {
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![game("A", vec![media(f.port, "cover", COVER_A)])],
        };
        publish(&f, &m, &[]);
        // Same length, different bytes.
        let mut wrong = COVER_A.to_vec();
        wrong[0] ^= 0xff;
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(COVER_A)),
            200,
            wrong,
        );
        let wait = f.task.pull().await;
        assert!(wait <= Duration::from_secs(4), "backs off: {wait:?}");
        assert_eq!(state(&f, COVER_A), Some(FileState::Pending));
        assert!(!f.cache.file_path(&hex(COVER_A)).exists());
        assert_eq!(parts(&f), 0);
        assert!(f.cache.serve_lookup(&hex(COVER_A), T0).unwrap().is_none());

        // A short body (size differs) is discarded too.
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(COVER_A)),
            200,
            COVER_A[..5].to_vec(),
        );
        f.task.pull().await;
        assert_eq!(state(&f, COVER_A), Some(FileState::Pending));
        assert_eq!(parts(&f), 0);

        // The next pull gets the right bytes.
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(COVER_A)),
            200,
            COVER_A.to_vec(),
        );
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        assert_eq!(state(&f, COVER_A), Some(FileState::Stored));
    }

    #[tokio::test]
    async fn a_redirect_is_refused_not_followed() {
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![game("A", vec![media(f.port, "cover", COVER_A)])],
        };
        publish(&f, &m, &[]);
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(COVER_A)),
            302,
            vec![],
        );
        f.task.pull().await;
        assert_eq!(state(&f, COVER_A), Some(FileState::Pending));
        assert_eq!(requests(&f.seen, "/elsewhere").len(), 0);
    }

    #[tokio::test]
    async fn a_tombstone_removes_the_game_and_its_files_unless_another_game_uses_them() {
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![
                game(
                    "A",
                    vec![
                        media(f.port, "cover", COVER_A),
                        media(f.port, "cover", COVER_B),
                    ],
                ),
                game("B", vec![media(f.port, "cover", COVER_B)]),
            ],
        };
        publish(&f, &m, &[COVER_A, COVER_B]);
        f.task.pull().await;
        assert_eq!(f.cache.status().unwrap().games, 2);

        let gone = CatalogGame {
            deleted: true,
            name: None,
            ..game("A", vec![])
        };
        publish(
            &f,
            &CatalogManifest {
                catalog_version: 2,
                games: vec![gone],
            },
            &[],
        );
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        assert_eq!(f.cache.status().unwrap().games, 1);
        assert_eq!(state(&f, COVER_A), None);
        assert!(!f.cache.file_path(&hex(COVER_A)).exists());
        assert_eq!(state(&f, COVER_B), Some(FileState::Stored));
        assert!(f.cache.file_path(&hex(COVER_B)).exists());
    }

    #[tokio::test]
    async fn over_the_cap_the_oldest_served_video_goes_and_an_oversized_one_is_flagged() {
        // Cap fits both covers (21 + 38 bytes) and 100 bytes of video.
        let v1: &[u8] = &[1u8; 60];
        let v2: &[u8] = &[2u8; 60];
        let huge: &[u8] = &[3u8; 500];
        let mut f = fx(160).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![game(
                "A",
                vec![
                    media(f.port, "cover", COVER_A),
                    media(f.port, "cover", COVER_B),
                    media(f.port, "video", v1),
                ],
            )],
        };
        publish(&f, &m, &[COVER_A, COVER_B, v1]);
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        assert_eq!(state(&f, v1), Some(FileState::Stored));
        f.cache.serve_lookup(&hex(v1), T0 + 5).unwrap();

        let m = CatalogManifest {
            catalog_version: 2,
            games: vec![game(
                "C",
                vec![media(f.port, "video", v2), media(f.port, "video", huge)],
            )],
        };
        publish(&f, &m, &[v2, huge]);
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        assert_eq!(
            state(&f, v1),
            Some(FileState::Evicted),
            "the older video made room"
        );
        assert_eq!(state(&f, v2), Some(FileState::Stored));
        assert_eq!(
            state(&f, huge),
            Some(FileState::Evicted),
            "too big for the cap"
        );
        assert_eq!(state(&f, COVER_A), Some(FileState::Stored));
        assert_eq!(state(&f, COVER_B), Some(FileState::Stored));
        assert!(
            requests(&f.seen, &format!("/catalog/{}", hex(huge))).is_empty(),
            "an oversized video is never downloaded"
        );

        // Not retried while the cap stays; retried once it grows.
        let before = requests(&f.seen, "/catalog/").len();
        route(&f.routes, "/v1/catalog/manifest", 304, vec![]);
        f.task.pull().await;
        assert_eq!(requests(&f.seen, "/catalog/").len(), before);
        f.cap.store(1 << 20, Ordering::SeqCst);
        f.task.pull().await;
        assert_eq!(state(&f, huge), Some(FileState::Stored));
        assert_eq!(state(&f, v1), Some(FileState::Stored));
    }

    #[tokio::test]
    async fn a_dead_wan_keeps_the_cache_serving_and_the_next_pull_resumes() {
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![game(
                "A",
                vec![
                    media(f.port, "cover", COVER_A),
                    media(f.port, "video", VIDEO_A),
                ],
            )],
        };
        // Only the cover is available; the connection for the video is refused.
        publish(&f, &m, &[COVER_A]);
        f.task.pull().await;
        assert_eq!(state(&f, COVER_A), Some(FileState::Stored));
        assert_eq!(state(&f, VIDEO_A), Some(FileState::Pending));

        // Now the whole cloud is unreachable.
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let live = f.task.cloud.clone();
        f.task.cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{dead}"), true).unwrap(),
        )
        .unwrap();
        let wait = f.task.pull().await;
        assert!(
            wait >= Duration::from_secs(1) && wait <= RETRY_CEILING,
            "backs off: {wait:?}"
        );
        assert!(
            f.cache.serve_lookup(&hex(COVER_A), T0).unwrap().is_some(),
            "what is cached is still served"
        );
        assert_eq!(f.cache.catalog_version().unwrap(), 1);
        assert_eq!(state(&f, VIDEO_A), Some(FileState::Pending));

        // Back on the air: the video, which was pending, arrives.
        f.task.cloud = live;
        route(&f.routes, "/v1/catalog/manifest", 304, vec![]);
        route(
            &f.routes,
            &format!("/catalog/{}.png", hex(VIDEO_A)),
            200,
            VIDEO_A.to_vec(),
        );
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);
        assert_eq!(state(&f, VIDEO_A), Some(FileState::Stored));
    }

    #[tokio::test]
    async fn a_revoked_token_marks_the_pairing_revoked_and_stops_asking() {
        let mut f = fx(1 << 30).await;
        for code in ["TOKEN_REVOKED", "TOKEN_INVALID"] {
            f.status.send_modify(|s| s.pairing = PairingState::Active);
            route(&f.routes, "/v1/catalog/manifest", 401, refused(code));
            assert_eq!(f.task.pull().await, REVOKED_BACKOFF, "{code}");
            assert_eq!(f.status.borrow().pairing, PairingState::Revoked);
            let asked = requests(&f.seen, "/v1/catalog/manifest").len();
            assert_eq!(f.task.pull().await, REVOKED_BACKOFF);
            assert_eq!(
                requests(&f.seen, "/v1/catalog/manifest").len(),
                asked,
                "a revoked hub stops asking"
            );
        }
    }

    #[tokio::test]
    async fn only_the_primary_pulls() {
        let mut f = fx(1 << 30).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        assert_eq!(f.task.pull().await, ROLE_RECHECK);
        assert!(f.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn other_refusals_back_off_without_revoking() {
        let mut f = fx(1 << 30).await;
        for (status, code) in [(429, "RATE_LIMITED"), (500, "INTERNAL"), (403, "FORBIDDEN")] {
            route(&f.routes, "/v1/catalog/manifest", status, refused(code));
            let wait = f.task.pull().await;
            assert!(wait <= Duration::from_secs(1800), "{code}: {wait:?}");
            assert_eq!(f.status.borrow().pairing, PairingState::Active, "{code}");
        }
    }

    #[tokio::test]
    async fn nothing_of_the_cache_is_written_to_the_branch_store() {
        // The cache lives in its own files under `reference/`: the pull needs no branch
        // database at all.
        let mut f = fx(1 << 30).await;
        let m = CatalogManifest {
            catalog_version: 1,
            games: vec![game("A", vec![media(f.port, "cover", COVER_A)])],
        };
        publish(&f, &m, &[COVER_A]);
        f.task.pull().await;
        let names: Vec<_> = std::fs::read_dir(f.dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["reference".to_owned()]);
        assert!(f.dir.path().join("reference").join("reference.db").exists());
    }

    /// A main Hub as the standby sees it: serves what it has by hash, counts the asks.
    struct FakeMain {
        files: HashMap<String, Vec<u8>>,
        asked: Mutex<Vec<String>>,
    }

    impl LanMedia for FakeMain {
        fn fetch<'a>(
            &'a self,
            sha256: &'a str,
            part: &'a Path,
            bytes: u64,
        ) -> Pin<Box<dyn Future<Output = Result<(), MediaFailure>> + Send + 'a>> {
            Box::pin(async move {
                self.asked.lock().unwrap().push(sha256.to_owned());
                match self.files.get(sha256) {
                    Some(body) if body.len() as u64 == bytes => {
                        tokio::fs::write(part, body).await.unwrap();
                        Ok(())
                    }
                    Some(_) => Err(MediaFailure::Mismatch("the size differs")),
                    None => Err(MediaFailure::Refused(404)),
                }
            })
        }
    }

    fn become_standby(f: &Fx) {
        f.status.send_modify(|s| {
            s.role = Role::Follower;
            s.following = Some(arena_proto::StandbyReport {
                applied_epoch: None,
                applied_seq: None,
                lag_rows: 0,
                lag_seconds: 0,
                last_row_at: None,
                schema_version: 12,
                behind: None,
            });
        });
    }

    #[tokio::test]
    async fn a_standby_pulls_its_own_manifest_and_takes_media_from_the_main_hub_first() {
        let mut f = fx(1 << 30).await;
        become_standby(&f);
        let m = CatalogManifest {
            catalog_version: 3,
            games: vec![game(
                "A",
                vec![
                    media(f.port, "cover", COVER_A),
                    media(f.port, "video", VIDEO_A),
                ],
            )],
        };
        // The CDN has the video only; the main Hub has the cover only.
        publish(&f, &m, &[VIDEO_A]);
        let main = Arc::new(FakeMain {
            files: HashMap::from([(hex(COVER_A), COVER_A.to_vec())]),
            asked: Mutex::default(),
        });
        f.task = f.task.with_lan_media(main.clone());
        assert_eq!(f.task.pull().await, CATALOG_INTERVAL);

        assert_eq!(f.cache.status().unwrap().catalog_version, 3);
        assert_eq!(state(&f, COVER_A), Some(FileState::Stored));
        assert_eq!(state(&f, VIDEO_A), Some(FileState::Stored));
        assert!(
            requests(&f.seen, &format!("/catalog/{}.png", hex(COVER_A))).is_empty(),
            "the cover came from the main hub"
        );
        assert_eq!(
            requests(&f.seen, &format!("/catalog/{}.png", hex(VIDEO_A))).len(),
            1,
            "the cloud filled in what the main hub did not hold"
        );
        assert_eq!(main.asked.lock().unwrap().len(), 2);
        assert_eq!(parts(&f), 0);
    }

    #[tokio::test]
    async fn a_follower_that_is_not_a_standby_does_not_pull() {
        let mut f = fx(1 << 30).await;
        f.status.send_modify(|s| {
            s.role = Role::Follower;
            s.following = None;
        });
        assert_eq!(f.task.pull().await, ROLE_RECHECK);
        assert!(f.seen.lock().unwrap().is_empty());
    }
}

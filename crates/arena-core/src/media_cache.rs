//! The Hub's game media cache (Story 6.2): the reference database plus the `media/<sha256>` files
//! it describes, with the cap, the eviction and the lookup the LAN server serves from.
//!
//! Nothing here touches the branch store, the writer or `apply()`: the cache is its own database
//! file and directory, so it never reaches the change log, batches or snapshots.
//!
//! Covers are always kept and may exceed the cap. Videos are best-effort: stored videos plus a
//! new one may not exceed `cap - stored cover bytes`, and the least recently served videos are
//! dropped (and flagged `evicted`, so they are not fetched again every pull) to make room. The
//! flags are cleared when the cap grows.
//!
//! Every method blocks on SQLite or the disk: run them on a blocking thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use arena_proto::{
    AppError, CatalogManifest, DEFAULT_MEDIA_CAP_GB, ErrorCode, KEY_MEDIA_CAP_GB,
    MEDIA_CAP_GB_RANGE, MediaCacheStatus, SettingEntry,
};
use arena_store::{FileKind, FileState, RefFile, ReferenceDb, is_sha256_hex};

/// One GiB, the unit of `system.media_cap_gb`.
pub const GIB: u64 = 1 << 30;

/// The cap in bytes, read when needed so a changed setting applies at once.
pub type CapFn = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The cap in bytes `settings` give: `system.media_cap_gb` when it is a valid value, else the
/// default.
pub fn media_cap_bytes(settings: &[SettingEntry]) -> u64 {
    let (min, max) = MEDIA_CAP_GB_RANGE;
    let gb = settings
        .iter()
        .find(|s| s.key == KEY_MEDIA_CAP_GB)
        .and_then(|s| s.value.parse::<i64>().ok())
        .filter(|n| (min..=max).contains(n))
        .or_else(|| DEFAULT_MEDIA_CAP_GB.parse().ok())
        .unwrap_or(10);
    gb as u64 * GIB
}

fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// The `Content-Type` for a file whose manifest URL ends in `.ext`.
pub fn content_type_of(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// A stored file ready to be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedFile {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub content_type: &'static str,
}

/// What [`MediaCache::apply_manifest`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// Games in the manifest (live and tombstones).
    pub games: usize,
    /// Files no game references any more, deleted from the disk and the table.
    pub removed: usize,
}

/// What [`MediaCache::commit`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Committed {
    /// Stored, after dropping this many older videos.
    Stored { evicted: usize },
    /// A video that no longer fits even with no other video: dropped and flagged `evicted`.
    NoRoom,
}

pub struct MediaCache {
    media_dir: PathBuf,
    db: Mutex<ReferenceDb>,
    cap: CapFn,
}

impl std::fmt::Debug for MediaCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaCache")
            .field("media_dir", &self.media_dir)
            .finish_non_exhaustive()
    }
}

impl MediaCache {
    /// Opens the cache under `root` (`<data_dir>/reference`): `reference.db` and `media/`.
    /// Leftover `.part` files of an interrupted download are removed, and a file recorded as
    /// stored but gone from the disk goes back to pending.
    pub fn open(root: impl AsRef<Path>, cap: CapFn) -> Result<Self, AppError> {
        let root = root.as_ref();
        let media_dir = root.join("media");
        std::fs::create_dir_all(&media_dir)
            .map_err(|e| internal(format!("cannot create the media cache: {e}")))?;
        let db = ReferenceDb::open(root.join("reference.db"))?;
        let cache = Self {
            media_dir,
            db: Mutex::new(db),
            cap,
        };
        cache.clean_parts();
        cache.reconcile()?;
        Ok(cache)
    }

    fn db(&self) -> Result<MutexGuard<'_, ReferenceDb>, AppError> {
        self.db
            .lock()
            .map_err(|_| internal("the media cache lock is poisoned"))
    }

    fn clean_parts(&self) {
        let Ok(entries) = std::fs::read_dir(&self.media_dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "part") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn reconcile(&self) -> Result<(), AppError> {
        let db = self.db()?;
        for f in db.stored_files()? {
            if !self.is_on_disk(&f) {
                db.mark_unstored(&f.sha256, FileState::Pending)?;
            }
        }
        Ok(())
    }

    fn is_on_disk(&self, f: &RefFile) -> bool {
        std::fs::metadata(self.file_path(&f.sha256))
            .is_ok_and(|m| m.is_file() && m.len() == f.bytes)
    }

    /// Where a stored file lives (`sha256` must be a validated hash).
    pub fn file_path(&self, sha256: &str) -> PathBuf {
        self.media_dir.join(sha256)
    }

    /// Where a download is written before it is verified and renamed.
    pub fn part_path(&self, sha256: &str) -> PathBuf {
        self.media_dir.join(format!("{sha256}.part"))
    }

    pub fn cap_bytes(&self) -> u64 {
        (self.cap)()
    }

    /// Called at the start of a pull: when the cap has grown since the last pull, every evicted
    /// video may be fetched again.
    pub fn note_cap(&self) -> Result<(), AppError> {
        let now = self.cap_bytes();
        let db = self.db()?;
        if db.cap_seen()?.is_some_and(|seen| now > seen) {
            let cleared = db.clear_evicted()?;
            if cleared > 0 {
                tracing::info!(
                    cleared,
                    "media cache: the cap grew; evicted videos may return"
                );
            }
        }
        db.set_cap_seen(now)?;
        drop(db);
        self.enforce_cap()
    }

    /// Drops the least recently served videos while the stored ones exceed what the cap leaves
    /// after the covers (a lowered cap takes effect at the next pull).
    fn enforce_cap(&self) -> Result<(), AppError> {
        let db = self.db()?;
        loop {
            let (covers, videos) = db.stored_bytes()?;
            if videos <= self.cap_bytes().saturating_sub(covers) {
                return Ok(());
            }
            let Some(victim) = db.stored_videos_lru()?.into_iter().next() else {
                return Ok(());
            };
            self.evict(&db, &victim.sha256)?;
        }
    }

    /// Deletes a stored video from the disk and flags it `evicted`. A file that cannot be removed
    /// stays stored, so the table never disagrees with the disk.
    fn evict(&self, db: &ReferenceDb, sha256: &str) -> Result<(), AppError> {
        let path = self.file_path(sha256);
        if let Err(e) = std::fs::remove_file(&path)
            && path.exists()
        {
            return Err(internal(format!("cannot evict a media file: {e}")));
        }
        db.mark_unstored(sha256, FileState::Evicted)
    }

    pub fn catalog_version(&self) -> Result<u64, AppError> {
        self.db()?.catalog_version()
    }

    /// The cached catalogue game with this id (Story 6.3: the library links to it).
    pub fn game(&self, id: &str) -> Result<Option<arena_store::RefGame>, AppError> {
        self.db()?.game(id)
    }

    /// Every game's executable signatures (Story 6.5: the Hub matches scans against them).
    pub fn signatures(&self) -> Result<Vec<arena_store::RefSignature>, AppError> {
        self.db()?.signatures()
    }

    /// The SHA-256 of the game's stored cover, if the cache holds one (Story 6.5).
    pub fn cover_sha256(&self, game_id: &str) -> Result<Option<String>, AppError> {
        self.db()?.cover_sha256(game_id)
    }

    /// The SHA-256 of the game's stored video, if the cache holds one (Story 6.7).
    pub fn video_sha256(&self, game_id: &str) -> Result<Option<String>, AppError> {
        self.db()?.video_sha256(game_id)
    }

    /// Stores a manifest and deletes the files nothing references any more.
    pub fn apply_manifest(&self, manifest: &CatalogManifest) -> Result<Applied, AppError> {
        let orphans = self.db()?.apply_manifest(manifest)?;
        for sha in &orphans {
            if is_sha256_hex(sha) {
                let _ = std::fs::remove_file(self.file_path(sha));
                let _ = std::fs::remove_file(self.part_path(sha));
            }
        }
        Ok(Applied {
            games: manifest.games.len(),
            removed: orphans.len(),
        })
    }

    /// The files still to download, every cover before any video.
    pub fn pending(&self) -> Result<Vec<RefFile>, AppError> {
        self.db()?.pending_files()
    }

    /// Whether `file` may be downloaded: a cover always; a video only when it can fit with no
    /// other video stored. One that cannot is flagged `evicted` so it is not tried every pull.
    pub fn admit(&self, file: &RefFile) -> Result<bool, AppError> {
        if file.kind == FileKind::Cover {
            return Ok(true);
        }
        let db = self.db()?;
        let (covers, _) = db.stored_bytes()?;
        if file.bytes <= self.cap_bytes().saturating_sub(covers) {
            return Ok(true);
        }
        db.mark_unstored(&file.sha256, FileState::Evicted)?;
        tracing::info!(
            bytes = file.bytes,
            "media cache: a video larger than the cap allows is skipped"
        );
        Ok(false)
    }

    /// Takes the verified download at [`part_path`](Self::part_path) into the cache. For a video,
    /// the least recently served stored videos are dropped first until it fits.
    pub fn commit(&self, file: &RefFile, now: i64) -> Result<Committed, AppError> {
        let part = self.part_path(&file.sha256);
        let db = self.db()?;
        let mut evicted = 0;
        if file.kind == FileKind::Video {
            loop {
                let (covers, videos) = db.stored_bytes()?;
                let allowed = self.cap_bytes().saturating_sub(covers);
                if videos.saturating_add(file.bytes) <= allowed {
                    break;
                }
                let victim = db
                    .stored_videos_lru()?
                    .into_iter()
                    .find(|v| v.sha256 != file.sha256);
                let Some(victim) = victim else {
                    let _ = std::fs::remove_file(&part);
                    db.mark_unstored(&file.sha256, FileState::Evicted)?;
                    return Ok(Committed::NoRoom);
                };
                self.evict(&db, &victim.sha256)?;
                evicted += 1;
            }
        }
        if let Err(e) = std::fs::rename(&part, self.file_path(&file.sha256)) {
            let _ = std::fs::remove_file(&part);
            return Err(internal(format!("cannot store a media file: {e}")));
        }
        db.mark_stored(&file.sha256, now)?;
        Ok(Committed::Stored { evicted })
    }

    /// The stored file for a request path's hash, or `None` for a malformed hash, an unknown one,
    /// one not downloaded yet, or a file gone from the disk (which goes back to pending). A hit
    /// counts as served now.
    pub fn serve_lookup(&self, sha256: &str, now: i64) -> Result<Option<ServedFile>, AppError> {
        let sha = sha256.to_ascii_lowercase();
        if !is_sha256_hex(&sha) {
            return Ok(None);
        }
        let db = self.db()?;
        let Some(file) = db.file(&sha)? else {
            return Ok(None);
        };
        if file.state != FileState::Stored {
            return Ok(None);
        }
        if !self.is_on_disk(&file) {
            db.mark_unstored(&sha, FileState::Pending)?;
            return Ok(None);
        }
        db.touch_served(&sha, now)?;
        Ok(Some(ServedFile {
            path: self.file_path(&sha),
            sha256: sha,
            bytes: file.bytes,
            content_type: content_type_of(&file.url),
        }))
    }

    pub fn status(&self) -> Result<MediaCacheStatus, AppError> {
        let db = self.db()?;
        let (cover_bytes, video_bytes) = db.stored_bytes()?;
        Ok(MediaCacheStatus {
            used_bytes: cover_bytes + video_bytes,
            cover_bytes,
            video_bytes,
            cap_bytes: self.cap_bytes(),
            catalog_version: db.catalog_version()?,
            games: db.games_count()?,
        })
    }

    /// The file rows (tests and diagnostics).
    pub fn files(&self) -> Result<Vec<RefFile>, AppError> {
        self.db()?.files()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{CatalogGame, CatalogMedia};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn sha(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn media(kind: &str, c: char, bytes: u64, ext: &str) -> CatalogMedia {
        CatalogMedia {
            kind: kind.into(),
            sha256: sha(c),
            bytes,
            url: format!("cdn.example/catalog/{}.{ext}", sha(c)),
        }
    }

    fn game(id: &str, media: Vec<CatalogMedia>) -> CatalogGame {
        CatalogGame {
            id: id.into(),
            rev: 1,
            deleted: false,
            name: Some(id.into()),
            launcher: None,
            category: None,
            age_rating: None,
            signatures: vec![],
            media,
        }
    }

    struct Fx {
        _dir: tempfile::TempDir,
        cache: MediaCache,
        cap: Arc<AtomicU64>,
    }

    fn fx(cap: u64) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let cap = Arc::new(AtomicU64::new(cap));
        let seen = Arc::clone(&cap);
        let cache = MediaCache::open(
            dir.path().join("reference"),
            Arc::new(move || seen.load(Ordering::SeqCst)),
        )
        .unwrap();
        Fx {
            _dir: dir,
            cache,
            cap,
        }
    }

    fn manifest(version: u64, games: Vec<CatalogGame>) -> CatalogManifest {
        CatalogManifest {
            catalog_version: version,
            games,
        }
    }

    /// Writes the part file the downloader would have and commits it.
    fn store(cache: &MediaCache, c: char, now: i64) -> Committed {
        let file = cache.db().unwrap().file(&sha(c)).unwrap().unwrap();
        std::fs::write(
            cache.part_path(&file.sha256),
            vec![7u8; file.bytes as usize],
        )
        .unwrap();
        cache.commit(&file, now).unwrap()
    }

    fn state(cache: &MediaCache, c: char) -> FileState {
        cache.db().unwrap().file(&sha(c)).unwrap().unwrap().state
    }

    #[test]
    fn the_cap_comes_from_the_setting_with_a_ten_gib_default() {
        let setting = |v: &str| SettingEntry {
            key: KEY_MEDIA_CAP_GB.into(),
            value: v.into(),
            rev: 1,
        };
        assert_eq!(media_cap_bytes(&[]), 10 * GIB);
        assert_eq!(media_cap_bytes(&[setting("1")]), GIB);
        assert_eq!(media_cap_bytes(&[setting("10000")]), 10_000 * GIB);
        for bad in ["0", "10001", "x", ""] {
            assert_eq!(media_cap_bytes(&[setting(bad)]), 10 * GIB, "{bad}");
        }
    }

    #[test]
    fn content_types_follow_the_manifest_url() {
        for (url, want) in [
            ("c/x.png", "image/png"),
            ("c/x.JPG?v=1", "image/jpeg"),
            ("c/x.webp", "image/webp"),
            ("c/x.mp4#t", "video/mp4"),
            ("c/x.webm", "video/webm"),
            ("c/x", "application/octet-stream"),
            ("c.d/x/y", "application/octet-stream"),
        ] {
            assert_eq!(content_type_of(url), want, "{url}");
        }
    }

    #[test]
    fn a_stored_file_is_served_and_anything_else_is_not() {
        let f = fx(GIB);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 10, "png"),
                        media("cover", 'b', 5, "png"),
                    ],
                )],
            ))
            .unwrap();
        // Listed but not downloaded.
        assert!(f.cache.serve_lookup(&sha('a'), 1).unwrap().is_none());
        assert_eq!(store(&f.cache, 'a', 100), Committed::Stored { evicted: 0 });
        let served = f.cache.serve_lookup(&sha('a'), 200).unwrap().unwrap();
        assert_eq!((served.bytes, served.content_type), (10, "image/png"));
        assert_eq!(std::fs::read(&served.path).unwrap().len(), 10);
        // Upper-case hex names the same file.
        assert!(
            f.cache
                .serve_lookup(&sha('a').to_ascii_uppercase(), 201)
                .unwrap()
                .is_some()
        );
        for bad in ["", "zz", "../a", &sha('c'), &"a".repeat(63)] {
            assert!(f.cache.serve_lookup(bad, 1).unwrap().is_none(), "{bad}");
        }
        // The file vanished: not served, and back to pending for the next pull.
        std::fs::remove_file(f.cache.file_path(&sha('a'))).unwrap();
        assert!(f.cache.serve_lookup(&sha('a'), 300).unwrap().is_none());
        assert_eq!(state(&f.cache, 'a'), FileState::Pending);
    }

    #[test]
    fn serving_records_when_and_status_adds_up_what_is_stored() {
        let f = fx(GIB);
        f.cache
            .apply_manifest(&manifest(
                3,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 10, "png"),
                        media("video", 'b', 90, "mp4"),
                    ],
                )],
            ))
            .unwrap();
        store(&f.cache, 'a', 1);
        store(&f.cache, 'b', 2);
        f.cache.serve_lookup(&sha('b'), 99).unwrap();
        let file = f.cache.db().unwrap().file(&sha('b')).unwrap().unwrap();
        assert_eq!(file.last_served_ms, Some(99));
        assert_eq!(
            f.cache.status().unwrap(),
            MediaCacheStatus {
                used_bytes: 100,
                cover_bytes: 10,
                video_bytes: 90,
                cap_bytes: GIB,
                catalog_version: 3,
                games: 1,
            }
        );
    }

    #[test]
    fn a_tombstone_deletes_the_files_nothing_uses() {
        let f = fx(GIB);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![
                    game(
                        "A",
                        vec![media("cover", 'a', 4, "png"), media("cover", 'c', 4, "png")],
                    ),
                    game("B", vec![media("cover", 'a', 4, "png")]),
                ],
            ))
            .unwrap();
        store(&f.cache, 'a', 1);
        store(&f.cache, 'c', 1);
        let gone = CatalogGame {
            deleted: true,
            name: None,
            ..game("A", vec![])
        };
        let applied = f.cache.apply_manifest(&manifest(2, vec![gone])).unwrap();
        assert_eq!(
            applied,
            Applied {
                games: 1,
                removed: 1
            }
        );
        assert!(!f.cache.file_path(&sha('c')).exists());
        assert!(f.cache.file_path(&sha('a')).exists(), "B still uses it");
    }

    #[test]
    fn the_least_recently_served_videos_make_room_and_covers_stay() {
        // Cap 1000, covers 300: videos may hold 700.
        let f = fx(1000);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 300, "png"),
                        media("video", 'b', 300, "mp4"),
                        media("video", 'c', 300, "mp4"),
                        media("video", 'd', 300, "mp4"),
                    ],
                )],
            ))
            .unwrap();
        store(&f.cache, 'a', 1);
        store(&f.cache, 'b', 10);
        store(&f.cache, 'c', 20);
        // b was served more recently than c.
        f.cache.serve_lookup(&sha('b'), 500).unwrap();
        let d = f.cache.db().unwrap().file(&sha('d')).unwrap().unwrap();
        assert!(f.cache.admit(&d).unwrap());
        assert_eq!(store(&f.cache, 'd', 600), Committed::Stored { evicted: 1 });
        assert_eq!(
            state(&f.cache, 'c'),
            FileState::Evicted,
            "c was the stalest"
        );
        assert_eq!(state(&f.cache, 'b'), FileState::Stored);
        assert_eq!(
            state(&f.cache, 'a'),
            FileState::Stored,
            "covers are never evicted"
        );
        assert!(!f.cache.file_path(&sha('c')).exists());
        let status = f.cache.status().unwrap();
        assert_eq!((status.cover_bytes, status.video_bytes), (300, 600));
        // An evicted video is not pending again.
        assert!(f.cache.pending().unwrap().is_empty());
    }

    #[test]
    fn a_lowered_cap_drops_the_stalest_videos_at_the_next_pull() {
        let f = fx(1000);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 100, "png"),
                        media("video", 'b', 300, "mp4"),
                        media("video", 'c', 300, "mp4"),
                    ],
                )],
            ))
            .unwrap();
        f.cache.note_cap().unwrap();
        store(&f.cache, 'a', 1);
        store(&f.cache, 'b', 10);
        store(&f.cache, 'c', 20);
        f.cache.serve_lookup(&sha('b'), 500).unwrap();
        // Cap 450 leaves 350 for videos: only the most recently served one stays.
        f.cap.store(450, Ordering::SeqCst);
        f.cache.note_cap().unwrap();
        assert_eq!(state(&f.cache, 'c'), FileState::Evicted);
        assert_eq!(state(&f.cache, 'b'), FileState::Stored);
        assert_eq!(state(&f.cache, 'a'), FileState::Stored);
        assert!(!f.cache.file_path(&sha('c')).exists());
        assert_eq!(f.cache.status().unwrap().used_bytes, 400);
    }

    #[test]
    fn a_video_that_cannot_fit_at_all_is_skipped_and_flagged() {
        let f = fx(500);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 100, "png"),
                        media("video", 'b', 450, "mp4"),
                    ],
                )],
            ))
            .unwrap();
        f.cache.note_cap().unwrap();
        store(&f.cache, 'a', 1);
        let b = f.cache.db().unwrap().file(&sha('b')).unwrap().unwrap();
        assert!(!f.cache.admit(&b).unwrap());
        assert_eq!(state(&f.cache, 'b'), FileState::Evicted);
        assert!(
            f.cache.pending().unwrap().is_empty(),
            "not retried every pull"
        );
        // Growing the cap lets it come back at the next pull.
        f.cap.store(2000, Ordering::SeqCst);
        f.cache.note_cap().unwrap();
        assert_eq!(state(&f.cache, 'b'), FileState::Pending);
        assert!(f.cache.admit(&f.cache.pending().unwrap()[0]).unwrap());
    }

    #[test]
    fn covers_may_exceed_the_cap_and_an_unchanged_cap_clears_nothing() {
        let f = fx(100);
        f.cache
            .apply_manifest(&manifest(
                1,
                vec![game(
                    "A",
                    vec![
                        media("cover", 'a', 500, "png"),
                        media("video", 'b', 10, "mp4"),
                    ],
                )],
            ))
            .unwrap();
        f.cache.note_cap().unwrap();
        let a = f.cache.db().unwrap().file(&sha('a')).unwrap().unwrap();
        assert!(f.cache.admit(&a).unwrap());
        store(&f.cache, 'a', 1);
        let b = f.cache.db().unwrap().file(&sha('b')).unwrap().unwrap();
        assert!(!f.cache.admit(&b).unwrap(), "no room left for videos");
        f.cache.note_cap().unwrap();
        assert_eq!(state(&f.cache, 'b'), FileState::Evicted);
        assert_eq!(f.cache.status().unwrap().used_bytes, 500);
    }

    #[test]
    fn reopening_drops_part_files_and_re_queues_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("reference");
        let cap: CapFn = Arc::new(|| GIB);
        {
            let cache = MediaCache::open(&root, Arc::clone(&cap)).unwrap();
            cache
                .apply_manifest(&manifest(
                    1,
                    vec![game(
                        "A",
                        vec![media("cover", 'a', 3, "png"), media("cover", 'b', 3, "png")],
                    )],
                ))
                .unwrap();
            store(&cache, 'a', 1);
            store(&cache, 'b', 1);
            std::fs::write(cache.part_path(&sha('c')), b"half").unwrap();
            std::fs::remove_file(cache.file_path(&sha('b'))).unwrap();
        }
        let cache = MediaCache::open(&root, cap).unwrap();
        assert!(!cache.part_path(&sha('c')).exists());
        assert_eq!(state(&cache, 'a'), FileState::Stored);
        assert_eq!(state(&cache, 'b'), FileState::Pending);
        assert_eq!(cache.catalog_version().unwrap(), 1);
    }
}

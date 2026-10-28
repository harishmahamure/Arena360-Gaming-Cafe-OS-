//! The Hub's reference cache database (Story 6.2): the global game catalogue and the media files
//! it points at, kept in its own SQLite file (`<data_dir>/reference/reference.db`).
//!
//! It is not the branch store: nothing here goes through the writer, `apply()`, the change log,
//! batches or snapshots, and a restored or promoted Hub fills its own copy from the manifest.
//! Tables: `ref_games`, `ref_signatures`, `ref_media` (a game's references to files), `ref_files`
//! (one row per content hash, with its download state) and `ref_meta`.

use std::path::Path;

use arena_proto::{AppError, CatalogManifest, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

const META_CATALOG_VERSION: &str = "catalog_version";
const META_CAP_SEEN: &str = "cap_seen_bytes";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ref_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS ref_games (
    id TEXT PRIMARY KEY,
    rev INTEGER NOT NULL,
    name TEXT NOT NULL,
    launcher TEXT,
    category TEXT,
    age_rating TEXT
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS ref_signatures (
    game_id TEXT NOT NULL,
    exe_name TEXT NOT NULL,
    product_name TEXT NOT NULL,
    publisher TEXT NOT NULL,
    sha256 TEXT
);
CREATE INDEX IF NOT EXISTS ref_signatures_game ON ref_signatures (game_id);
CREATE TABLE IF NOT EXISTS ref_media (
    game_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    PRIMARY KEY (game_id, sha256)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS ref_media_sha ON ref_media (sha256);
CREATE TABLE IF NOT EXISTS ref_files (
    sha256 TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    url TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    stored_at INTEGER,
    last_served_ms INTEGER
) WITHOUT ROWID;
";

/// Whether `s` is a SHA-256 as 64 lower-case hex digits (the only shape a file name here takes).
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A cover (always kept) or a video (best-effort, evictable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Cover,
    Video,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Video => "video",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cover" => Some(Self::Cover),
            "video" => Some(Self::Video),
            _ => None,
        }
    }
}

/// Where a file is on its way to the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// Listed by the manifest, not on the disk (yet, or again after a failed download).
    Pending,
    /// On the disk, verified.
    Stored,
    /// A video dropped for the cap; not fetched again until the cap grows.
    Evicted,
}

impl FileState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Stored => "stored",
            Self::Evicted => "evicted",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "stored" => Self::Stored,
            "evicted" => Self::Evicted,
            _ => Self::Pending,
        }
    }
}

/// One content-addressed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFile {
    pub sha256: String,
    pub kind: FileKind,
    pub bytes: u64,
    pub url: String,
    pub state: FileState,
    pub stored_at: Option<i64>,
    pub last_served_ms: Option<i64>,
}

/// A game as the cache keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefGame {
    pub id: String,
    pub rev: u64,
    pub name: String,
    pub launcher: Option<String>,
    pub category: Option<String>,
    pub age_rating: Option<String>,
}

/// One executable signature of a catalogue game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSignature {
    pub game_id: String,
    pub exe_name: String,
    pub product_name: String,
    pub publisher: String,
    pub sha256: Option<String>,
}

/// An open reference database.
pub struct ReferenceDb {
    conn: Connection,
}

impl std::fmt::Debug for ReferenceDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceDb").finish_non_exhaustive()
    }
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn to_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

fn row_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<RefFile> {
    let kind: String = r.get(1)?;
    let state: String = r.get(4)?;
    Ok(RefFile {
        sha256: r.get(0)?,
        kind: FileKind::parse(&kind).unwrap_or(FileKind::Video),
        bytes: to_u64(r.get(2)?),
        url: r.get(3)?,
        state: FileState::parse(&state),
        stored_at: r.get(5)?,
        last_served_ms: r.get(6)?,
    })
}

const FILE_COLUMNS: &str = "sha256, kind, bytes, url, state, stored_at, last_served_ms";

impl ReferenceDb {
    /// Opens (creating if needed) the reference database at `path`. A cache can be refilled, so
    /// it does not pay for `synchronous=FULL`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let conn = Connection::open(path.as_ref()).map_err(db_error)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        let _mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(db_error)?;
        conn.execute_batch("PRAGMA synchronous=NORMAL")
            .map_err(db_error)?;
        Self::with_schema(conn)
    }

    /// An empty database in memory (tests).
    pub fn open_in_memory() -> Result<Self, AppError> {
        Self::with_schema(Connection::open_in_memory().map_err(db_error)?)
    }

    fn with_schema(conn: Connection) -> Result<Self, AppError> {
        conn.execute_batch(SCHEMA).map_err(db_error)?;
        Ok(Self { conn })
    }

    fn meta(&self, key: &str) -> Result<Option<String>, AppError> {
        self.conn
            .query_row("SELECT value FROM ref_meta WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_error)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO ref_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// The catalogue version this cache follows (0 before the first pull).
    pub fn catalog_version(&self) -> Result<u64, AppError> {
        Ok(self
            .meta(META_CATALOG_VERSION)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// The cap in bytes as seen at the last pull.
    pub fn cap_seen(&self) -> Result<Option<u64>, AppError> {
        Ok(self.meta(META_CAP_SEEN)?.and_then(|v| v.parse().ok()))
    }

    pub fn set_cap_seen(&self, bytes: u64) -> Result<(), AppError> {
        self.set_meta(META_CAP_SEEN, &bytes.to_string())
    }

    /// Takes a manifest in one transaction: a live game replaces its row, signatures and media
    /// references, a tombstone removes them; files no game references any more leave the table.
    /// Answers the hashes of the files that left, for the caller to delete from the disk.
    /// Media entries that are not a cover or video with a SHA-256 are ignored.
    pub fn apply_manifest(&mut self, manifest: &CatalogManifest) -> Result<Vec<String>, AppError> {
        let tx = self.conn.transaction().map_err(db_error)?;
        for game in &manifest.games {
            tx.execute("DELETE FROM ref_signatures WHERE game_id = ?1", [&game.id])
                .map_err(db_error)?;
            tx.execute("DELETE FROM ref_media WHERE game_id = ?1", [&game.id])
                .map_err(db_error)?;
            tx.execute("DELETE FROM ref_games WHERE id = ?1", [&game.id])
                .map_err(db_error)?;
            if game.deleted {
                continue;
            }
            let Some(name) = game.name.as_deref() else {
                continue;
            };
            tx.execute(
                "INSERT INTO ref_games (id, rev, name, launcher, category, age_rating)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    game.id,
                    to_i64(game.rev),
                    name,
                    game.launcher,
                    game.category,
                    game.age_rating
                ],
            )
            .map_err(db_error)?;
            for sig in &game.signatures {
                tx.execute(
                    "INSERT INTO ref_signatures (game_id, exe_name, product_name, publisher, sha256)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![game.id, sig.exe_name, sig.product_name, sig.publisher, sig.sha256],
                )
                .map_err(db_error)?;
            }
            for media in &game.media {
                let (Some(kind), true) =
                    (FileKind::parse(&media.kind), is_sha256_hex(&media.sha256))
                else {
                    continue;
                };
                tx.execute(
                    "INSERT OR REPLACE INTO ref_media (game_id, kind, sha256) VALUES (?1, ?2, ?3)",
                    params![game.id, kind.as_str(), media.sha256],
                )
                .map_err(db_error)?;
                // A hash two games share is a cover if either says so: covers are never evicted.
                tx.execute(
                    "INSERT INTO ref_files (sha256, kind, bytes, url) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(sha256) DO UPDATE SET
                        url = excluded.url,
                        bytes = excluded.bytes,
                        kind = CASE WHEN ref_files.kind = 'cover' OR excluded.kind = 'cover'
                                    THEN 'cover' ELSE 'video' END",
                    params![media.sha256, kind.as_str(), to_i64(media.bytes), media.url],
                )
                .map_err(db_error)?;
            }
        }
        let orphans: Vec<String> = {
            let mut stmt = tx
                .prepare(
                    "SELECT sha256 FROM ref_files
                     WHERE sha256 NOT IN (SELECT sha256 FROM ref_media)",
                )
                .map_err(db_error)?;
            stmt.query_map([], |r| r.get(0))
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?
        };
        tx.execute(
            "DELETE FROM ref_files WHERE sha256 NOT IN (SELECT sha256 FROM ref_media)",
            [],
        )
        .map_err(db_error)?;
        tx.execute(
            "INSERT INTO ref_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_CATALOG_VERSION, manifest.catalog_version.to_string()],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(orphans)
    }

    pub fn game(&self, id: &str) -> Result<Option<RefGame>, AppError> {
        self.conn
            .query_row(
                "SELECT id, rev, name, launcher, category, age_rating FROM ref_games WHERE id = ?1",
                [id],
                |r| {
                    Ok(RefGame {
                        id: r.get(0)?,
                        rev: to_u64(r.get(1)?),
                        name: r.get(2)?,
                        launcher: r.get(3)?,
                        category: r.get(4)?,
                        age_rating: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    /// Every game's executable signatures, ordered by game id then file name (Story 6.5).
    pub fn signatures(&self) -> Result<Vec<RefSignature>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT game_id, exe_name, product_name, publisher, sha256
                 FROM ref_signatures ORDER BY game_id, exe_name, product_name, publisher",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RefSignature {
                    game_id: r.get(0)?,
                    exe_name: r.get(1)?,
                    product_name: r.get(2)?,
                    publisher: r.get(3)?,
                    sha256: r.get(4)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// The SHA-256 of the game's cover when the cache holds that file (stored and verified).
    pub fn cover_sha256(&self, game_id: &str) -> Result<Option<String>, AppError> {
        self.stored_media_sha256(game_id, "cover")
    }

    /// The SHA-256 of the game's video when the cache holds that file (stored and verified; an
    /// evicted or still pending video is not one).
    pub fn video_sha256(&self, game_id: &str) -> Result<Option<String>, AppError> {
        self.stored_media_sha256(game_id, "video")
    }

    fn stored_media_sha256(&self, game_id: &str, kind: &str) -> Result<Option<String>, AppError> {
        self.conn
            .query_row(
                "SELECT m.sha256 FROM ref_media m
                 JOIN ref_files f ON f.sha256 = m.sha256
                 WHERE m.game_id = ?1 AND m.kind = ?2 AND f.state = 'stored'
                 ORDER BY m.sha256 LIMIT 1",
                params![game_id, kind],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)
    }

    pub fn games_count(&self) -> Result<u64, AppError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM ref_games", [], |r| r.get::<_, i64>(0))
            .map(to_u64)
            .map_err(db_error)
    }

    pub fn signatures_count(&self, game_id: &str) -> Result<u64, AppError> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM ref_signatures WHERE game_id = ?1",
                [game_id],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
            .map_err(db_error)
    }

    pub fn file(&self, sha256: &str) -> Result<Option<RefFile>, AppError> {
        self.conn
            .query_row(
                &format!("SELECT {FILE_COLUMNS} FROM ref_files WHERE sha256 = ?1"),
                [sha256],
                row_file,
            )
            .optional()
            .map_err(db_error)
    }

    /// Every file in the table, covers first.
    pub fn files(&self) -> Result<Vec<RefFile>, AppError> {
        self.query_files("SELECT {} FROM ref_files ORDER BY kind, sha256")
    }

    /// Files still to download: every cover before any video, smaller first within each.
    pub fn pending_files(&self) -> Result<Vec<RefFile>, AppError> {
        self.query_files(
            "SELECT {} FROM ref_files WHERE state = 'pending'
             ORDER BY (kind = 'cover') DESC, bytes, sha256",
        )
    }

    /// Stored videos, the least recently served first (never served first of all, then by when
    /// they were stored).
    pub fn stored_videos_lru(&self) -> Result<Vec<RefFile>, AppError> {
        self.query_files(
            "SELECT {} FROM ref_files WHERE state = 'stored' AND kind = 'video'
             ORDER BY COALESCE(last_served_ms, 0), COALESCE(stored_at, 0), sha256",
        )
    }

    /// Files recorded as stored (to check them against the disk).
    pub fn stored_files(&self) -> Result<Vec<RefFile>, AppError> {
        self.query_files("SELECT {} FROM ref_files WHERE state = 'stored' ORDER BY sha256")
    }

    fn query_files(&self, sql: &str) -> Result<Vec<RefFile>, AppError> {
        let sql = sql.replace("{}", FILE_COLUMNS);
        let mut stmt = self.conn.prepare(&sql).map_err(db_error)?;
        stmt.query_map([], row_file)
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)
    }

    /// Bytes of stored covers and stored videos.
    pub fn stored_bytes(&self) -> Result<(u64, u64), AppError> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(CASE WHEN kind = 'cover' THEN bytes ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN kind = 'video' THEN bytes ELSE 0 END), 0)
                 FROM ref_files WHERE state = 'stored'",
                [],
                |r| Ok((to_u64(r.get(0)?), to_u64(r.get(1)?))),
            )
            .map_err(db_error)
    }

    /// Records `sha256` as downloaded and verified at `now`.
    pub fn mark_stored(&self, sha256: &str, now: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE ref_files SET state = 'stored', stored_at = ?2, last_served_ms = NULL
                 WHERE sha256 = ?1",
                params![sha256, now],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Sets the state of a file that is not on the disk (any state but stored).
    pub fn mark_unstored(&self, sha256: &str, state: FileState) -> Result<(), AppError> {
        if state == FileState::Stored {
            return Err(AppError::new(
                ErrorCode::Internal,
                "a stored file is marked with mark_stored",
            ));
        }
        self.conn
            .execute(
                "UPDATE ref_files SET state = ?2, stored_at = NULL, last_served_ms = NULL
                 WHERE sha256 = ?1",
                params![sha256, state.as_str()],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Notes that a stored file was just served.
    pub fn touch_served(&self, sha256: &str, now: i64) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE ref_files SET last_served_ms = ?2 WHERE sha256 = ?1 AND state = 'stored'",
                params![sha256, now],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Lets every evicted video be fetched again; answers how many.
    pub fn clear_evicted(&self) -> Result<usize, AppError> {
        self.conn
            .execute(
                "UPDATE ref_files SET state = 'pending' WHERE state = 'evicted'",
                [],
            )
            .map_err(db_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{CatalogGame, CatalogMedia, CatalogSignature};

    fn sha(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn media(kind: &str, c: char, bytes: u64) -> CatalogMedia {
        CatalogMedia {
            kind: kind.into(),
            sha256: sha(c),
            bytes,
            url: format!("https://cdn.example/catalog/{}.png", sha(c)),
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
            signatures: vec![CatalogSignature {
                exe_name: format!("{id}.exe"),
                product_name: "P".into(),
                publisher: "Pub".into(),
                sha256: None,
            }],
            media,
        }
    }

    fn tombstone(id: &str) -> CatalogGame {
        CatalogGame {
            id: id.into(),
            rev: 2,
            deleted: true,
            name: None,
            launcher: None,
            category: None,
            age_rating: None,
            signatures: vec![],
            media: vec![],
        }
    }

    fn manifest(version: u64, games: Vec<CatalogGame>) -> CatalogManifest {
        CatalogManifest {
            catalog_version: version,
            games,
        }
    }

    #[test]
    fn a_manifest_stores_games_signatures_media_and_the_version() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        assert_eq!(db.catalog_version().unwrap(), 0);
        let orphans = db
            .apply_manifest(&manifest(
                5,
                vec![
                    game("A", vec![media("cover", 'a', 10), media("video", 'b', 100)]),
                    game("B", vec![media("cover", 'c', 20)]),
                ],
            ))
            .unwrap();
        assert!(orphans.is_empty());
        assert_eq!(db.catalog_version().unwrap(), 5);
        assert_eq!(db.games_count().unwrap(), 2);
        assert_eq!(db.signatures_count("A").unwrap(), 1);
        assert_eq!(db.game("A").unwrap().unwrap().name, "Game A");
        let pending = db.pending_files().unwrap();
        assert_eq!(
            pending.iter().map(|f| f.kind).collect::<Vec<_>>(),
            [FileKind::Cover, FileKind::Cover, FileKind::Video],
            "covers first"
        );
    }

    #[test]
    fn a_game_reports_its_stored_cover_and_video_only() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        db.apply_manifest(&manifest(
            1,
            vec![game(
                "A",
                vec![media("cover", 'a', 10), media("video", 'b', 100)],
            )],
        ))
        .unwrap();
        // Nothing is stored yet: nothing is reported.
        assert_eq!(db.cover_sha256("A").unwrap(), None);
        assert_eq!(db.video_sha256("A").unwrap(), None);
        db.mark_stored(&sha('b'), 5).unwrap();
        assert_eq!(db.video_sha256("A").unwrap(), Some(sha('b')));
        assert_eq!(
            db.cover_sha256("A").unwrap(),
            None,
            "the cover is not stored"
        );
        db.mark_stored(&sha('a'), 6).unwrap();
        assert_eq!(db.cover_sha256("A").unwrap(), Some(sha('a')));
        // An evicted video is not served.
        db.mark_unstored(&sha('b'), FileState::Evicted).unwrap();
        assert_eq!(db.video_sha256("A").unwrap(), None);
        assert_eq!(db.video_sha256("nope").unwrap(), None);
    }
    #[test]
    fn a_live_game_replaces_its_rows_and_a_tombstone_removes_them() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        db.apply_manifest(&manifest(
            1,
            vec![
                game("A", vec![media("cover", 'a', 10), media("video", 'b', 100)]),
                game("B", vec![media("cover", 'a', 10)]),
            ],
        ))
        .unwrap();
        // A drops its video and gains a new cover: the video file is orphaned, the shared
        // cover is kept for B.
        let orphans = db
            .apply_manifest(&manifest(2, vec![game("A", vec![media("cover", 'd', 5)])]))
            .unwrap();
        assert_eq!(orphans, vec![sha('b')]);
        assert!(db.file(&sha('b')).unwrap().is_none());
        assert!(db.file(&sha('a')).unwrap().is_some());
        assert_eq!(db.signatures_count("A").unwrap(), 1);
        // A is deleted upstream: its own cover goes, the one B still uses stays.
        let orphans = db
            .apply_manifest(&manifest(3, vec![tombstone("A")]))
            .unwrap();
        assert_eq!(orphans, vec![sha('d')]);
        assert!(db.game("A").unwrap().is_none());
        assert_eq!(db.signatures_count("A").unwrap(), 0);
        assert!(db.file(&sha('a')).unwrap().is_some());
        let orphans = db
            .apply_manifest(&manifest(4, vec![tombstone("B")]))
            .unwrap();
        assert_eq!(orphans, vec![sha('a')]);
        assert_eq!(db.games_count().unwrap(), 0);
        assert!(db.files().unwrap().is_empty());
    }

    #[test]
    fn a_shared_hash_is_a_cover_if_any_game_says_so_and_state_survives_a_reapply() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        db.apply_manifest(&manifest(
            1,
            vec![
                game("A", vec![media("video", 'a', 10)]),
                game("B", vec![media("cover", 'a', 10)]),
            ],
        ))
        .unwrap();
        assert_eq!(db.file(&sha('a')).unwrap().unwrap().kind, FileKind::Cover);
        db.mark_stored(&sha('a'), 7).unwrap();
        db.apply_manifest(&manifest(2, vec![game("A", vec![media("video", 'a', 10)])]))
            .unwrap();
        let f = db.file(&sha('a')).unwrap().unwrap();
        assert_eq!((f.state, f.stored_at), (FileState::Stored, Some(7)));
    }

    #[test]
    fn unusable_media_entries_are_ignored() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        let mut bad = media("cover", 'a', 1);
        bad.sha256 = "../../etc/passwd".into();
        let odd = CatalogMedia {
            kind: "audio".into(),
            ..media("cover", 'b', 1)
        };
        db.apply_manifest(&manifest(1, vec![game("A", vec![bad, odd])]))
            .unwrap();
        assert!(db.files().unwrap().is_empty());
        assert_eq!(db.games_count().unwrap(), 1);
    }

    #[test]
    fn states_sizes_lru_order_and_evicted_flags() {
        let mut db = ReferenceDb::open_in_memory().unwrap();
        db.apply_manifest(&manifest(
            1,
            vec![game(
                "A",
                vec![
                    media("cover", 'a', 10),
                    media("video", 'b', 100),
                    media("video", 'c', 200),
                    media("video", 'd', 300),
                ],
            )],
        ))
        .unwrap();
        for (c, at) in [('a', 1), ('b', 2), ('c', 3), ('d', 4)] {
            db.mark_stored(&sha(c), at).unwrap();
        }
        assert_eq!(db.stored_bytes().unwrap(), (10, 600));
        db.touch_served(&sha('b'), 50).unwrap();
        db.touch_served(&sha('c'), 40).unwrap();
        let order: Vec<_> = db
            .stored_videos_lru()
            .unwrap()
            .into_iter()
            .map(|f| f.sha256)
            .collect();
        assert_eq!(order, vec![sha('d'), sha('c'), sha('b')]);
        db.mark_unstored(&sha('d'), FileState::Evicted).unwrap();
        assert_eq!(db.stored_bytes().unwrap(), (10, 300));
        assert!(db.pending_files().unwrap().is_empty());
        assert_eq!(db.clear_evicted().unwrap(), 1);
        assert_eq!(db.pending_files().unwrap().len(), 1);
        assert!(db.mark_unstored(&sha('d'), FileState::Stored).is_err());
    }

    #[test]
    fn it_reopens_from_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reference.db");
        {
            let mut db = ReferenceDb::open(&path).unwrap();
            db.apply_manifest(&manifest(9, vec![game("A", vec![])]))
                .unwrap();
            db.set_cap_seen(123).unwrap();
        }
        let db = ReferenceDb::open(&path).unwrap();
        assert_eq!(db.catalog_version().unwrap(), 9);
        assert_eq!(db.cap_seen().unwrap(), Some(123));
        assert!(is_sha256_hex(&sha('f')));
        assert!(!is_sha256_hex(&sha('F')) && !is_sha256_hex("abc"));
    }
}

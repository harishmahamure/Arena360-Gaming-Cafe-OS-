//! Searches over a built audit month (Story 8.37, AD-37).
//!
//! A `ready` month is the object `audit-cache/<yyyy-mm>.db.zst` ([`crate::audit_build`]). The
//! first query unpacks it into the local cache ([`AuditCache`]); later ones open that copy. The
//! cache keeps at most [`arena_proto::AUDIT_LOCAL_CACHE_BYTES`], evicting the least recently used
//! month first, and an evicted month is fetched again while its object exists. The file name
//! carries the build's `built_at`, so a rebuilt month never reads an older copy.
//!
//! Only the questions of the `arena-proto` catalogue ([`AuditSearch`]) run, each as a fixed
//! statement over the `changes` log, newest first, [`AUDIT_QUERY_PAGE`] rows a page. A result is
//! change metadata: who, what intent, which table and row, when. Row images never leave the
//! cache, so player data is at most an id (a `players` row id or a `player` reference in a
//! row id), and no ciphertext column is read. The changes a fork point kept out of state are
//! returned in their own list.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use arena_proto::{
    AUDIT_QUERY_PAGE, AuditActor, AuditActorKind, AuditElevatedBy, AuditElevationKind,
    AuditElevationMode, AuditItem, AuditSearch, AuditSearchPage, ChangeOpKind,
};
use arena_proto::{ActorKind, ElevationKind, ElevationMode};
use arena_store::{AuditCursor, AuditFilter, AuditRow, ChangeOp, list_audit, list_orphaned};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::presign::Presigner;

const DB_SUFFIX: &str = ".db";
/// Catalogue keys for the actors that are not a staff member or a device (as the Hub's audit).
const ACTOR_SYSTEM_KEY: &str = "hub.audit.actor.system";
const ACTOR_OWNER_KEY: &str = "hub.audit.actor.owner";
const ACTOR_ADMIN_KEY: &str = "hub.audit.actor.admin";

/// Why a search could not run.
#[derive(Debug)]
pub enum QueryError {
    /// The month's object is gone from the bucket (it expired, or the sweep took it).
    Gone,
    /// The cursor is not one this service issued.
    BadCursor,
    /// Anything else: the bucket, the disk or the database.
    Failed(String),
}

impl From<String> for QueryError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

/// The builder's local copies of built months.
pub struct AuditCache {
    dir: PathBuf,
    max_bytes: u64,
    /// Held while a month is fetched or the cache is trimmed, so two queries for the same month
    /// fetch it once.
    lock: tokio::sync::Mutex<()>,
}

impl AuditCache {
    /// A cache under `dir` holding up to `max_bytes`.
    pub fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            dir,
            max_bytes,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    fn path(&self, branch_id: &str, month: &str, built_at: i64) -> PathBuf {
        self.dir
            .join(branch_id)
            .join(format!("{month}-{built_at}{DB_SUFFIX}"))
    }

    /// The local copy of the month built at `built_at`, fetched from `key` on a miss. `None` when
    /// the object is gone. A hit counts as a use for the least-recently-used order.
    pub async fn open(
        &self,
        data: &Presigner,
        key: &str,
        branch_id: &str,
        month: &str,
        built_at: i64,
    ) -> Result<Option<PathBuf>, String> {
        let _guard = self.lock.lock().await;
        let path = self.path(branch_id, month, built_at);
        if path.is_file() {
            touch(&path);
            return Ok(Some(path));
        }
        let Some((bytes, _)) = data.read(key).await? else {
            return Ok(None);
        };
        let (dir, max, target) = (self.dir.clone(), self.max_bytes, path.clone());
        let stem = format!("{month}-");
        tokio::task::spawn_blocking(move || {
            let parent = target.parent().ok_or("no cache directory")?;
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot make the cache: {e}"))?;
            // An older build of this month is out of date.
            for entry in std::fs::read_dir(parent)
                .map_err(|e| e.to_string())?
                .flatten()
            {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(&stem) && name.ends_with(DB_SUFFIX) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            let part = target.with_extension("part");
            let mut out =
                std::fs::File::create(&part).map_err(|e| format!("cannot write the cache: {e}"))?;
            zstd::stream::copy_decode(&bytes[..], &mut out)
                .map_err(|e| format!("cannot unpack the cached month: {e}"))?;
            drop(out);
            std::fs::rename(&part, &target).map_err(|e| format!("cannot store the cache: {e}"))?;
            evict(&dir, max, &target);
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| format!("the cache thread failed: {e}"))??;
        Ok(Some(path))
    }
}

/// Marks `path` as just used.
fn touch(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Every cached month under `dir`: `(path, size, last use)`.
fn cached_months(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut out = Vec::new();
    let Ok(branches) = std::fs::read_dir(dir) else {
        return out;
    };
    for branch in branches.flatten() {
        let Ok(files) = std::fs::read_dir(branch.path()) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "db")
                && let Ok(meta) = f.metadata()
            {
                out.push((
                    path,
                    meta.len(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ));
            }
        }
    }
    out
}

/// Removes the least recently used months until the cache fits `max_bytes`; `keep` stays.
fn evict(dir: &Path, max_bytes: u64, keep: &Path) {
    let mut months = cached_months(dir);
    let mut total: u64 = months.iter().map(|m| m.1).sum();
    months.sort_by_key(|m| m.2);
    for (path, size, _) in months {
        if total <= max_bytes {
            break;
        }
        if path == keep {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            tracing::info!(path = %path.display(), "audit cache evicted a month");
        }
    }
}

/// The opaque cursor of the last row of a page: `at.seq.epoch`, base64url.
fn encode_cursor(row: &AuditRow) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}.{}.{}", row.at, row.seq, row.epoch))
}

fn decode_cursor(text: &str) -> Result<AuditCursor, QueryError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| QueryError::BadCursor)?;
    let s = String::from_utf8(bytes).map_err(|_| QueryError::BadCursor)?;
    let mut parts = s.splitn(3, '.');
    let at = parts.next().and_then(|p| p.parse().ok());
    let seq = parts.next().and_then(|p| p.parse().ok());
    let epoch = parts.next().filter(|e| !e.is_empty());
    match (at, seq, epoch) {
        (Some(at), Some(seq), Some(epoch)) => Ok(AuditCursor {
            at,
            epoch: epoch.to_owned(),
            seq,
        }),
        _ => Err(QueryError::BadCursor),
    }
}

fn item(row: &AuditRow) -> AuditItem {
    let (kind, name) = match ActorKind::parse(&row.actor_kind) {
        Some(k) => (
            AuditActorKind::from(k),
            match k {
                ActorKind::System => ACTOR_SYSTEM_KEY.to_owned(),
                ActorKind::Owner => ACTOR_OWNER_KEY.to_owned(),
                ActorKind::Admin => ACTOR_ADMIN_KEY.to_owned(),
                ActorKind::Staff | ActorKind::Device => row.actor_id.clone(),
            },
        ),
        None => (AuditActorKind::Unknown, row.actor_id.clone()),
    };
    let elevated_by = match (
        &row.elevated_by_kind,
        &row.elevated_by_id,
        &row.elevated_by_mode,
    ) {
        (Some(k), Some(id), Some(m)) => Some(AuditElevatedBy {
            kind: ElevationKind::parse(k).map_or(AuditElevationKind::Unknown, Into::into),
            id: id.clone(),
            name: id.clone(),
            mode: ElevationMode::parse(m).map_or(AuditElevationMode::Unknown, Into::into),
        }),
        _ => None,
    };
    AuditItem {
        epoch: row.epoch.clone(),
        seq: row.seq,
        at: row.at,
        business_day: row.business_day.clone(),
        actor: AuditActor {
            kind,
            id: row.actor_id.clone(),
            name,
        },
        elevated_by,
        module: row
            .intent_kind
            .split_once('.')
            .map_or(row.intent_kind.as_str(), |(d, _)| d)
            .to_owned(),
        intent_kind: row.intent_kind.clone(),
        intent_id: row.intent_id.clone(),
        tbl: row.tbl.clone(),
        row_id: row.row_id.clone(),
        op: match row.op {
            ChangeOp::Insert => ChangeOpKind::Insert,
            ChangeOp::Update => ChangeOpKind::Update,
            ChangeOp::Delete => ChangeOpKind::Delete,
        },
        flagged: row.flagged,
        reverts: row.reverts.clone(),
    }
}

/// Runs `search` over the cached month at `path`.
pub fn run_search(
    path: &Path,
    branch_id: &str,
    month: &str,
    built_at: i64,
    search: &AuditSearch,
) -> Result<AuditSearchPage, QueryError> {
    let first_page = search.cursor.is_none();
    let filter = AuditFilter {
        actor_kind: search.actor_kind.clone(),
        actor_id: search.actor_id.clone(),
        module: search.module.clone(),
        intent_kind: search.intent_kind.clone(),
        tbl: search.tbl.clone(),
        row_id: search.row_id.clone(),
        from_ms: search.from_ms,
        to_ms: search.to_ms,
        after: search.cursor.as_deref().map(decode_cursor).transpose()?,
    };
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| QueryError::Failed(format!("cannot open the cached month: {e}")))?;
    let failed = |e: arena_proto::AppError| QueryError::Failed(e.message);
    let mut rows = list_audit(&conn, &filter, AUDIT_QUERY_PAGE + 1).map_err(failed)?;
    let more = rows.len() > AUDIT_QUERY_PAGE as usize;
    rows.truncate(AUDIT_QUERY_PAGE as usize);
    let next_cursor = if more {
        rows.last().map(encode_cursor)
    } else {
        None
    };
    let orphaned = if first_page {
        list_orphaned(&conn, &filter, AUDIT_QUERY_PAGE)
            .map_err(failed)?
            .iter()
            .map(item)
            .collect()
    } else {
        Vec::new()
    };
    Ok(AuditSearchPage {
        branch_id: branch_id.to_owned(),
        month: month.to_owned(),
        built_at,
        items: rows.iter().map(item).collect(),
        orphaned,
        next_cursor,
    })
}

/// Fetches the month into `cache` if needed and runs `search` on the blocking pool. `Gone` when
/// the month's object no longer exists.
pub async fn search_month(
    cache: &AuditCache,
    data: &Presigner,
    key: &str,
    branch_id: &str,
    month: &str,
    built_at: i64,
    search: AuditSearch,
) -> Result<AuditSearchPage, QueryError> {
    let path = cache
        .open(data, key, branch_id, month, built_at)
        .await?
        .ok_or(QueryError::Gone)?;
    let (branch_id, month) = (branch_id.to_owned(), month.to_owned());
    tokio::task::spawn_blocking(move || run_search(&path, &branch_id, &month, built_at, &search))
        .await
        .map_err(|e| QueryError::Failed(format!("the search thread failed: {e}")))?
}

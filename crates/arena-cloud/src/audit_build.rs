//! The on-demand audit builder (Story 8.36, AD-37).
//!
//! The control role queues a `builder_jobs` row when an owner asks for a month
//! ([`request_build`]); the `builder` role ([`AuditBuilder`]) claims queued rows one at a time,
//! downloads the nearest snapshot on or before the month start and every change-log batch after
//! it, replays them with `arena-replay` into a temporary SQLite file, packs it as
//! `audit-cache/<yyyy-mm>.db.zst` (the builder's own sweep deletes it after 24 hours) and marks
//! the job `ready`. A missing batch or a batch that does not match its checksum fails the job; asking
//! again starts a new attempt. The cloud keeps no standing copy: the file is the only trace and
//! it expires.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arena_proto::{
    AUDIT_BUILD_STALE_MS, AUDIT_BUILDS_PER_HOUR, AUDIT_CACHE_TTL_MS, AuditBuildState,
    AuditBuildStatus, audit_month_start, is_rollup_date,
};
use arena_replay::{EpochFork, MonthBuilder};
use arena_store::DecodedBatch;
use arena_store::control::{BuilderJob, ControlDb, ControlError, RateRule};

use crate::Clock;
use crate::gaps::parse_batch_key;
use crate::presign::{Presigner, branch_prefix, has_path_chars};

const RATE_PREFIX: &str = "auditbuild:";
const RATE_BUCKET_MS: i64 = 60 * 1000;
const RATE_WINDOW_MS: i64 = 60 * 60 * 1000;
/// How long an idle builder waits before it looks for a job again.
pub const IDLE_POLL: Duration = Duration::from_secs(5);
/// The share of the progress bar the replay fills; the rest is the upload.
const REPLAY_SHARE: usize = 95;
/// The longest failure reason kept on a job.
const ERROR_MAX_CHARS: usize = 300;

/// `audit-cache/<month>.db.zst` under `prefix`.
pub fn cache_key(prefix: &str, month: &str) -> String {
    format!("{prefix}audit-cache/{month}.db.zst")
}

/// Whether a `ready` job's cache is still good at `now`.
pub fn is_fresh(job: &BuilderJob, now: i64) -> bool {
    job.state == AuditBuildState::Ready
        && job
            .built_at
            .is_some_and(|at| at > now.saturating_sub(AUDIT_CACHE_TTL_MS))
}

/// A job as the API shows it. The caller drops a `ready` job past its cache lifetime ([`is_fresh`])
/// before showing it.
pub fn status_of(job: &BuilderJob) -> AuditBuildStatus {
    AuditBuildStatus {
        branch_id: job.branch_id.clone(),
        month: job.month.clone(),
        state: job.state,
        progress: if job.state == AuditBuildState::Ready {
            100
        } else {
            job.progress
        },
        built_at: job.built_at,
        code: AuditBuildStatus::code_of(job.state),
    }
}

/// What [`request_build`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requested {
    /// A usable job already exists (queued, building, or ready and fresh).
    Existing(BuilderJob),
    /// A new attempt was queued.
    Queued(BuilderJob),
    /// The tenant asked for too many builds this hour; retry after this many ms.
    RateLimited { wait_ms: i64 },
}

/// Asks for `month` of `branch_id` (of `tenant_id`) to be rebuilt: reuses a queued, building or
/// fresh `ready` job, otherwise queues a new attempt, subject to the tenant's builds per hour.
/// `requested_by` names who asked (`owner:<id>`); `reason` is recorded for the support path.
pub async fn request_build(
    db: &ControlDb,
    tenant_id: &str,
    branch_id: &str,
    month: &str,
    requested_by: &str,
    reason: Option<&str>,
    now: i64,
) -> Result<Requested, ControlError> {
    let usable = |j: &BuilderJob| match j.state {
        AuditBuildState::Queued => true,
        AuditBuildState::Building => j.updated_at > now.saturating_sub(AUDIT_BUILD_STALE_MS),
        AuditBuildState::Ready => is_fresh(j, now),
        AuditBuildState::Failed => false,
    };
    if let Some(job) = db.builder_job(branch_id, month).await?
        && usable(&job)
    {
        return Ok(Requested::Existing(job));
    }
    let key = format!("{RATE_PREFIX}{tenant_id}");
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: RATE_BUCKET_MS,
        window_ms: RATE_WINDOW_MS,
        limit: AUDIT_BUILDS_PER_HOUR,
    };
    if let Err(wait_ms) = db.rate_reserve(&[rule], now).await? {
        tracing::warn!(
            tenant_id,
            branch_id,
            month,
            "audit build refused: rate limited"
        );
        return Ok(Requested::RateLimited { wait_ms });
    }
    let queued = db
        .queue_builder_job(
            branch_id,
            month,
            requested_by,
            reason,
            now,
            now.saturating_sub(AUDIT_CACHE_TTL_MS),
            now.saturating_sub(AUDIT_BUILD_STALE_MS),
        )
        .await?;
    let job = db
        .builder_job(branch_id, month)
        .await?
        .ok_or_else(|| ControlError::Db("a queued builder job vanished".to_owned()))?;
    let depth = db.builder_queue_depth().await?;
    tracing::info!(
        tenant_id,
        branch_id,
        month,
        builder_queue_depth = depth,
        "audit build queued"
    );
    Ok(if queued {
        Requested::Queued(job)
    } else {
        Requested::Existing(job)
    })
}

/// `(counter, instance)` of an epoch `counter:instance`.
fn epoch_order(epoch: &str) -> Option<(i64, &str)> {
    let (c, id) = epoch.split_once(':')?;
    Some((c.parse().ok()?, id))
}

/// One change-log batch in the bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BatchKey {
    epoch: String,
    first: i64,
    last: i64,
    key: String,
}

/// Every batch under `<prefix>changes/`, in log order. Keys of any other shape are ignored.
fn batch_keys(prefix: &str, keys: &[String]) -> Vec<BatchKey> {
    let root = format!("{prefix}changes/");
    let mut out: Vec<BatchKey> = keys
        .iter()
        .filter_map(|k| {
            let rest = k.strip_prefix(&root)?;
            let (epoch, name) = rest.rsplit_once('/')?;
            epoch_order(epoch)?;
            let (first, last) = parse_batch_key(name)?;
            Some(BatchKey {
                epoch: epoch.to_owned(),
                first,
                last,
                key: k.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        epoch_order(&a.epoch)
            .cmp(&epoch_order(&b.epoch))
            .then(a.first.cmp(&b.first))
            .then(a.last.cmp(&b.last))
    });
    out
}

/// The key of the newest snapshot dated on or before `start` (`yyyy-mm-dd`).
fn snapshot_before(prefix: &str, start: &str, keys: &[String]) -> Option<String> {
    let root = format!("{prefix}snapshots/");
    keys.iter()
        .filter_map(|k| {
            let date = k.strip_prefix(&root)?.strip_suffix(".db.zst")?;
            (is_rollup_date(date) && date <= start).then(|| (date.to_owned(), k.clone()))
        })
        .max()
        .map(|(_, k)| k)
}

fn clip(mut s: String) -> String {
    if s.chars().count() > ERROR_MAX_CHARS {
        s = s.chars().take(ERROR_MAX_CHARS).collect();
    }
    s
}

/// Runs `f` on the blocking pool with the builder, then hands the builder back.
async fn blocking<T: Send + 'static>(
    mut mb: MonthBuilder,
    f: impl FnOnce(&mut MonthBuilder) -> T + Send + 'static,
) -> Result<(MonthBuilder, T), String> {
    tokio::task::spawn_blocking(move || {
        let out = f(&mut mb);
        (mb, out)
    })
    .await
    .map_err(|e| format!("the replay thread failed: {e}"))
}

/// Builds queued months.
pub struct AuditBuilder {
    db: Arc<ControlDb>,
    data: Presigner,
    clock: Clock,
    scratch: PathBuf,
}

impl AuditBuilder {
    /// A builder over `db` and the data bucket; temporary databases go under `scratch`.
    pub fn new(db: Arc<ControlDb>, data: Presigner, clock: Clock, scratch: PathBuf) -> Self {
        Self {
            db,
            data,
            clock,
            scratch,
        }
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Claims and builds one job. `None` when no job was waiting.
    pub async fn run_one(&self) -> Result<Option<BuilderJob>, ControlError> {
        let now = self.now();
        let Some(job) = self
            .db
            .claim_builder_job(now, now.saturating_sub(AUDIT_BUILD_STALE_MS))
            .await?
        else {
            return Ok(None);
        };
        let depth = self.db.builder_queue_depth().await?;
        tracing::info!(
            branch_id = %job.branch_id,
            month = %job.month,
            attempt = job.attempt,
            builder_queue_depth = depth,
            "audit build started"
        );
        match self.build(&job).await {
            Ok(key) => {
                self.db
                    .finish_builder_job(&job.branch_id, &job.month, job.attempt, &key, self.now())
                    .await?;
                tracing::info!(branch_id = %job.branch_id, month = %job.month, "audit build ready");
            }
            Err(reason) => {
                let reason = clip(reason);
                tracing::warn!(
                    branch_id = %job.branch_id,
                    month = %job.month,
                    reason = %reason,
                    "audit build failed"
                );
                // A re-queued, expired cache is still in the bucket; a failed rebuild must not
                // leave it past its 24 hours.
                if let Ok(Some(b)) = self.db.branch(&job.branch_id).await
                    && !has_path_chars(&b.tenant_id)
                    && !has_path_chars(&b.id)
                {
                    let key = cache_key(&branch_prefix(&b.tenant_id, &b.id), &job.month);
                    if let Err(e) = self.data.delete(&key).await {
                        tracing::warn!(%key, error = %e, "stale audit cache not deleted");
                    }
                }
                self.db
                    .fail_builder_job(&job.branch_id, &job.month, job.attempt, &reason, self.now())
                    .await?;
            }
        }
        self.db.builder_job(&job.branch_id, &job.month).await
    }

    /// Deletes the cache objects of months built more than [`AUDIT_CACHE_TTL_MS`] ago: the
    /// 24-hour expiry of `audit-cache/`. Returns how many it removed.
    pub async fn sweep_expired(&self) -> Result<usize, ControlError> {
        let before = self.now().saturating_sub(AUDIT_CACHE_TTL_MS);
        let mut removed = 0;
        for (branch_id, month, key) in self.db.expired_builder_caches(before).await? {
            if !self
                .db
                .clear_builder_cache(&branch_id, &month, before)
                .await?
            {
                continue;
            }
            match self.data.delete(&key).await {
                Ok(()) => removed += 1,
                Err(e) => tracing::warn!(%key, error = %e, "expired audit cache not deleted"),
            }
        }
        Ok(removed)
    }

    async fn progress(&self, job: &BuilderJob, percent: usize) {
        let p = u8::try_from(percent.min(100)).unwrap_or(100);
        if let Err(e) = self
            .db
            .set_builder_progress(&job.branch_id, &job.month, job.attempt, p, self.now())
            .await
        {
            tracing::warn!(error = %e, "audit build progress not saved");
        }
    }

    /// Replays the month and stores it; the bucket key on success, the reason on failure.
    async fn build(&self, job: &BuilderJob) -> Result<String, String> {
        let branch = self
            .db
            .branch(&job.branch_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("the branch no longer exists")?;
        if has_path_chars(&branch.tenant_id) || has_path_chars(&branch.id) {
            return Err("tenant or branch id with path characters".to_owned());
        }
        let start = audit_month_start(&job.month).ok_or("not a month")?;
        let prefix = branch_prefix(&branch.tenant_id, &branch.id);

        let snapshots = self.data.list(&format!("{prefix}snapshots/")).await?;
        let snapshot = match snapshot_before(&prefix, &start, &snapshots) {
            Some(key) => {
                let (bytes, _) = self
                    .data
                    .read(&key)
                    .await?
                    .ok_or("the snapshot is gone from the bucket")?;
                Some(bytes)
            }
            None => None,
        };

        std::fs::create_dir_all(&self.scratch)
            .map_err(|e| format!("cannot use the scratch directory: {e}"))?;
        let dir = tempfile::Builder::new()
            .prefix("arena-audit-")
            .tempdir_in(&self.scratch)
            .map_err(|e| format!("cannot make a scratch directory: {e}"))?;
        let (path, month, now) = (dir.path().to_path_buf(), job.month.clone(), self.now());
        let mut mb = tokio::task::spawn_blocking(move || {
            MonthBuilder::open(&path, &month, snapshot.as_deref(), Vec::new(), now)
        })
        .await
        .map_err(|e| format!("the replay thread failed: {e}"))?
        .map_err(|e| e.message)?;

        // The batches after the snapshot's position, in log order.
        let keys = self.data.list(&format!("{prefix}changes/")).await?;
        let position = mb.position().cloned();
        let batches: Vec<BatchKey> = batch_keys(&prefix, &keys)
            .into_iter()
            .filter(|b| match &position {
                None => true,
                Some(p) => match epoch_order(&b.epoch).cmp(&epoch_order(&p.epoch)) {
                    std::cmp::Ordering::Less => false,
                    std::cmp::Ordering::Equal => b.last > p.seq,
                    std::cmp::Ordering::Greater => true,
                },
            })
            .collect();

        // Each replayed epoch's first batch says where it forked from.
        let mut forks = Vec::new();
        let mut firsts: HashMap<String, DecodedBatch> = HashMap::new();
        let mut seen: Option<&str> = None;
        for b in &batches {
            if seen == Some(b.epoch.as_str()) {
                continue;
            }
            seen = Some(&b.epoch);
            let decoded = self.fetch(b).await?;
            if let Some(f) = &decoded.header.fork {
                forks.push(EpochFork {
                    epoch: b.epoch.clone(),
                    parent_epoch: f.parent_epoch.clone(),
                    parent_seq: f.parent_seq,
                });
            }
            firsts.insert(b.key.clone(), decoded);
        }
        mb.set_forks(forks);

        let total = batches.len().max(1);
        let mut last_percent = 0;
        for (i, b) in batches.iter().enumerate() {
            let decoded = match firsts.remove(&b.key) {
                Some(d) => d,
                None => self.fetch(b).await?,
            };
            let (next, more) = blocking(mb, move |mb| mb.apply(&decoded)).await?;
            mb = next;
            let more = more.map_err(|e| e.message)?;
            let percent = (i + 1) * REPLAY_SHARE / total;
            if percent != last_percent {
                last_percent = percent;
                self.progress(job, percent).await;
            }
            if !more {
                break;
            }
        }
        self.progress(job, REPLAY_SHARE).await;

        let built = tokio::task::spawn_blocking(move || mb.finish())
            .await
            .map_err(|e| format!("the replay thread failed: {e}"))?
            .map_err(|e| e.message)?;
        tracing::info!(
            branch_id = %job.branch_id,
            month = %job.month,
            applied = built.applied,
            orphaned = built.orphaned,
            bytes = built.bytes.len(),
            "audit month replayed"
        );
        let key = cache_key(&prefix, &job.month);
        self.data.put(&key, built.bytes).await?;
        drop(dir);
        Ok(key)
    }

    /// Reads and checks one batch.
    async fn fetch(&self, b: &BatchKey) -> Result<DecodedBatch, String> {
        let (bytes, _) = self.data.read(&b.key).await?.ok_or_else(|| {
            format!(
                "batch {}..{} of epoch {} is missing from the bucket",
                b.first, b.last, b.epoch
            )
        })?;
        let decoded = arena_store::decode_batch(&bytes).map_err(|e| {
            format!(
                "batch {}..{} of epoch {} failed its check: {}",
                b.first, b.last, b.epoch, e.message
            )
        })?;
        if decoded.header.epoch != b.epoch
            || decoded.header.first != b.first
            || decoded.header.last != b.last
        {
            return Err(format!(
                "batch {} does not match its name",
                b.key.rsplit('/').next().unwrap_or(&b.key)
            ));
        }
        Ok(decoded)
    }
}

/// Runs the builder forever: claim, build, and wait [`IDLE_POLL`] when nothing is queued.
pub async fn run_worker(builder: Arc<AuditBuilder>) {
    loop {
        match builder.run_one().await {
            Ok(Some(_)) => continue,
            Ok(None) => {
                if let Err(e) = builder.sweep_expired().await {
                    tracing::warn!(error = %e, "audit cache sweep failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "audit builder could not reach gz_control"),
        }
        tokio::time::sleep(IDLE_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    const P: &str = "tenants/t/branches/b/";

    #[test]
    fn batches_sort_by_epoch_number_then_first_and_skip_other_keys() {
        let list = keys(&[
            "tenants/t/branches/b/changes/10:b/000000000001-000000000002.zst",
            "tenants/t/branches/b/changes/2:a/000000000004-000000000006.zst",
            "tenants/t/branches/b/changes/2:a/000000000001-000000000003.zst",
            "tenants/t/branches/b/changes/2:a/notes.txt",
            "tenants/t/branches/b/changes/x/000000000001-000000000003.zst",
            "tenants/t/branches/b/rollups/2026-10-01.json",
        ]);
        let got: Vec<(String, i64)> = batch_keys(P, &list)
            .into_iter()
            .map(|b| (b.epoch, b.first))
            .collect();
        assert_eq!(
            got,
            vec![("2:a".into(), 1), ("2:a".into(), 4), ("10:b".into(), 1)]
        );
    }

    #[test]
    fn the_snapshot_is_the_newest_dated_on_or_before_the_month_start() {
        let list = keys(&[
            "tenants/t/branches/b/snapshots/2026-08-31.db.zst",
            "tenants/t/branches/b/snapshots/2026-09-30.db.zst",
            "tenants/t/branches/b/snapshots/2026-10-01.db.zst",
            "tenants/t/branches/b/snapshots/2026-10-02.db.zst",
            "tenants/t/branches/b/snapshots/junk.db.zst",
        ]);
        assert_eq!(
            snapshot_before(P, "2026-10-01", &list).as_deref(),
            Some("tenants/t/branches/b/snapshots/2026-10-01.db.zst")
        );
        assert_eq!(
            snapshot_before(P, "2026-09-01", &list).as_deref(),
            Some("tenants/t/branches/b/snapshots/2026-08-31.db.zst")
        );
        assert_eq!(snapshot_before(P, "2026-08-01", &list), None);
    }

    #[test]
    fn a_cache_key_sits_in_the_audit_cache_prefix() {
        assert_eq!(
            cache_key(P, "2026-10"),
            "tenants/t/branches/b/audit-cache/2026-10.db.zst"
        );
    }
}

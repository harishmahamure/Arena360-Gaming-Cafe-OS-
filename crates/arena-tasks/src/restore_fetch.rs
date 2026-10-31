//! "Restore from cloud backup" on a replacement Hub (Story 9.12, AD-36, AD-4, AD-40, NFR11).
//!
//! A Hub in "Restore this branch" that was released the branch key (Story 9.10) downloads the
//! branch's cloud backup into `<data dir>/restore-staging/`:
//!
//! ```text
//! snapshot.db.zst   snapshot.json   the newest snapshot and its date and last change
//! player-keys.bin                   the sealed player key backup (Story 9.13 unwraps it)
//! changes/<epoch>/<first:012>-<last:012>.zst   (`:` of the epoch written `@`)
//! manifest.json                     written when the download is complete
//! ```
//!
//! The plan comes from `POST /v1/restore/plan` in two calls (the cloud cannot read a snapshot's
//! position; see [`arena_proto::RestorePlan`]). Every object is written to a `.part` file and
//! renamed once it is checked, so a finished file is never fetched again after a dropped
//! connection or a restart. Batches are checked with [`arena_store::decode_batch`] (sha256, row
//! count, sequence numbers) and against their name; the snapshot is unpacked and its last change
//! read. Links last 10 minutes: they are re-signed by asking the plan again, before they expire
//! and when the bucket refuses one.
//!
//! A batch that is missing from the bucket, fails its check, or leaves a hole in the sequence
//! stops the download with a gap ([`RestoreGapView`]): nothing more is fetched until the Owner
//! chooses "Restore up to <last good>" ([`RestoreFetchControl::restore_partial`]). Network
//! failures retry with backoff and keep the finished files. Nothing is replayed here (Story 9.13)
//! and `restore_pending` is never touched.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{HubStatus, PairingState, RestoreFetchControl};
use arena_proto::{
    ErrorCode, RestoreFetchPhase, RestoreFetchView, RestoreGapView, RestorePlan, RestorePlanBatch,
    RestorePlanRequest, RestorePosition, RestoreReadyView, Tier, UiError,
};
use arena_store::{decode_batch, packed_snapshot_position};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::cloud::{CloudClient, CloudFailure, ObjectFailure};
use crate::keystore::Keystore;

/// The staging folder under the Hub's data directory.
pub const STAGING_DIR: &str = "restore-staging";
const SNAPSHOT_FILE: &str = "snapshot.db.zst";
const SNAPSHOT_INFO: &str = "snapshot.json";
const SNAPSHOT_SCRATCH: &str = "snapshot.unpacked.tmp";
const KEYS_FILE: &str = "player-keys.bin";
const MANIFEST_FILE: &str = "manifest.json";
/// Links are re-signed after this long (they last 10 minutes).
const LINK_REFRESH: Duration = Duration::from_secs(8 * 60);
/// The longest wait between two failed attempts.
const RETRY_CEILING: Duration = Duration::from_secs(5 * 60);
/// How long to wait when the cloud limits the calls and does not say how long.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);
/// `after` for a snapshot that holds no change row: every epoch counter is above 0.
const EMPTY_EPOCH: &str = "0:snapshot";

/// What a finished download wrote beside the files, for Story 9.13.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedManifest {
    pub snapshot_date: Option<String>,
    /// The snapshot's last change; `None` when it holds none or there is no snapshot.
    pub snapshot_epoch: Option<String>,
    pub snapshot_seq: i64,
    pub keys: bool,
    /// Staged batches in log order.
    pub batches: Vec<StagedBatch>,
    /// The Owner stopped before a gap.
    pub partial: bool,
}

impl StagedManifest {
    /// The manifest of a finished download in `staging`, if it is there and readable.
    pub fn read(staging: &Path) -> Option<Self> {
        let bytes = std::fs::read(staging.join(MANIFEST_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedBatch {
    pub epoch: String,
    pub first: i64,
    pub last: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SnapshotInfo {
    date: String,
    epoch: Option<String>,
    seq: i64,
}

/// A gap found, with what the Owner's choice would keep and drop.
struct GapState {
    manifest: StagedManifest,
    drop: Vec<PathBuf>,
}

/// How a download ended.
enum Finished {
    Ready(StagedManifest),
    Gap(Box<RestoreGapView>, GapState),
}

/// A refusal that retrying will not fix; the code goes to the screen.
struct Stop(String);

impl Stop {
    fn code(code: ErrorCode) -> Self {
        Self(code.as_str().to_owned())
    }
}

struct Cfg {
    cloud: CloudClient,
    token: Arc<dyn Keystore>,
    hub_instance_id: String,
    staging: PathBuf,
    status: watch::Receiver<HubStatus>,
    /// The first wait after a failure; doubles up to [`RETRY_CEILING`].
    retry: Duration,
}

#[derive(Default)]
struct Shared {
    view: Mutex<Option<RestoreFetchView>>,
    gap: Mutex<Option<GapState>>,
    job: Mutex<Option<JoinHandle<()>>>,
}

/// The control the Hub screen uses and the download behind it.
#[derive(Clone)]
pub struct RestoreFetcher {
    cfg: Arc<Cfg>,
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn ui_error(code: ErrorCode, message: &str) -> UiError {
    UiError {
        code,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

/// The folder of an epoch: `:` is not allowed in a Windows file name, `@` is in no epoch.
fn epoch_dir(epoch: &str) -> String {
    epoch.replace(':', "@")
}

fn batch_path(staging: &Path, b: &RestorePlanBatch) -> PathBuf {
    staging
        .join("changes")
        .join(epoch_dir(&b.epoch))
        .join(format!("{:012}-{:012}.zst", b.first, b.last))
}

fn part_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// The first hole in the sequence: `(index of the batch after it, epoch, from, to)`.
fn first_hole(
    batches: &[RestorePlanBatch],
    position: Option<&(String, i64)>,
) -> Option<(usize, String, i64, i64)> {
    let mut prev: Option<(&str, i64)> = position.map(|(e, s)| (e.as_str(), *s));
    for (i, b) in batches.iter().enumerate() {
        match prev {
            Some((epoch, last)) if epoch == b.epoch => {
                if b.first > last + 1 {
                    return Some((i, b.epoch.clone(), last + 1, b.first - 1));
                }
                prev = Some((epoch, last.max(b.last)));
            }
            _ => {
                // Without a snapshot the log starts at 1; a later epoch starts where it forked.
                if i == 0 && position.is_none() && b.first > 1 {
                    return Some((0, b.epoch.clone(), 1, b.first - 1));
                }
                prev = Some((&b.epoch, b.last));
            }
        }
    }
    None
}

fn changes_in(b: &RestorePlanBatch) -> i64 {
    b.last - b.first + 1
}

impl RestoreFetcher {
    pub fn new(
        cloud: CloudClient,
        token: Arc<dyn Keystore>,
        hub_instance_id: String,
        data_dir: &Path,
        status: watch::Receiver<HubStatus>,
    ) -> Self {
        Self {
            cfg: Arc::new(Cfg {
                cloud,
                token,
                hub_instance_id,
                staging: data_dir.join(STAGING_DIR),
                status,
                retry: Duration::from_secs(5),
            }),
            shared: Arc::new(Shared::default()),
        }
    }

    /// Waits `retry` after the first failure instead of 5 s (tests).
    pub fn with_retry(mut self, retry: Duration) -> Self {
        if let Some(cfg) = Arc::get_mut(&mut self.cfg) {
            cfg.retry = retry;
        }
        self
    }

    fn check_allowed(&self) -> Result<(), UiError> {
        let s = self.cfg.status.borrow();
        if s.restore_pending.is_none() {
            return Err(ui_error(
                ErrorCode::NotFound,
                "This PC is not waiting to be restored.",
            ));
        }
        if s.pairing == PairingState::Revoked {
            return Err(ui_error(
                ErrorCode::TokenRevoked,
                "This Hub was replaced by another.",
            ));
        }
        if s.tier == Some(Tier::Free) {
            return Err(ui_error(
                ErrorCode::FeatureNotEntitled,
                "Restoring from the cloud needs a paid plan.",
            ));
        }
        if !s.restore_key_released {
            return Err(ui_error(
                ErrorCode::KeyNotReleased,
                "Waiting for the owner to release the branch key.",
            ));
        }
        Ok(())
    }

    /// The sealed key backup (`keys/player-keys.bin`), fetched once for a restore from an export
    /// file (Story 9.27): one plan call and one download, no retries and no staging folder. `None`
    /// when the Hub is not paid or was replaced, the cloud cannot be reached or refuses, there is
    /// no backup, or it is empty. Nothing is left on disk.
    pub async fn key_backup_bytes(&self) -> Option<Vec<u8>> {
        {
            let s = self.cfg.status.borrow();
            if s.pairing == PairingState::Revoked || !s.tier.is_some_and(|t| t != Tier::Free) {
                return None;
            }
        }
        let keystore = Arc::clone(&self.cfg.token);
        let token = tokio::task::spawn_blocking(move || keystore.get())
            .await
            .ok()?
            .ok()??;
        let token = String::from_utf8_lossy(&token).into_owned();
        let req = RestorePlanRequest {
            hub_instance_id: self.cfg.hub_instance_id.clone(),
            after: None,
        };
        let plan = match self.cfg.cloud.restore_plan(&token, &req).await {
            Ok(plan) => plan,
            Err(e) => {
                tracing::info!(
                    reason = match e {
                        CloudFailure::Refused { ref error, .. } => error.code.as_str(),
                        CloudFailure::Unreachable(_) => ErrorCode::CloudUnreachable.as_str(),
                    },
                    "key backup lookup: the cloud gave no plan"
                );
                return None;
            }
        };
        let url = plan.keys_url?;
        // One file per call (lookups can overlap), removed even when the caller stops waiting.
        static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scratch = Scratch(
            self.cfg
                .staging
                .with_file_name(format!("key-backup-lookup-{}-{n}.tmp", std::process::id())),
        );
        let got = self.cfg.cloud.download_object(&url, &scratch.0).await;
        let bytes = match got {
            Ok(_) => tokio::fs::read(&scratch.0).await.ok(),
            Err(_) => None,
        };
        bytes.filter(|b| !b.is_empty())
    }

    fn publish(&self, f: impl FnOnce(&mut RestoreFetchView)) {
        if let Some(v) = lock(&self.shared.view).as_mut() {
            f(v);
        }
    }

    fn progress(&self, done: u32, total: u32, bytes: u64) {
        self.publish(|v| {
            v.done = done;
            v.total = total;
            v.bytes = bytes;
            v.percent = (u64::from(done) * 100)
                .checked_div(u64::from(total))
                .map_or(0, |p| p.min(100) as u8);
            v.retrying = None;
        });
    }

    fn retrying(&self, code: &str) {
        let code = code.to_owned();
        self.publish(|v| v.retrying = Some(code));
    }

    fn wait_for(&self, fails: u32) -> Duration {
        let doublings = fails.saturating_sub(1).min(16);
        self.cfg
            .retry
            .saturating_mul(1 << doublings)
            .min(RETRY_CEILING)
    }

    async fn token(&self) -> String {
        loop {
            let token = Arc::clone(&self.cfg.token);
            if let Ok(Ok(Some(bytes))) = tokio::task::spawn_blocking(move || token.get()).await {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
            tracing::warn!("restore fetch: no hub token yet");
            self.retrying("NO_TOKEN");
            tokio::time::sleep(self.cfg.retry).await;
        }
    }

    /// One plan, retried through network trouble and rate limits.
    async fn plan(&self, after: Option<&RestorePosition>) -> Result<RestorePlan, Stop> {
        let req = RestorePlanRequest {
            hub_instance_id: self.cfg.hub_instance_id.clone(),
            after: after.cloned(),
        };
        let mut fails = 0u32;
        loop {
            if self.cfg.status.borrow().pairing == PairingState::Revoked {
                return Err(Stop::code(ErrorCode::TokenRevoked));
            }
            let token = self.token().await;
            let wait = match self.cfg.cloud.restore_plan(&token, &req).await {
                Ok(plan) => return Ok(plan),
                Err(CloudFailure::Refused {
                    error,
                    retry_after_s,
                    ..
                }) => match error.code {
                    ErrorCode::TokenRevoked
                    | ErrorCode::TokenInvalid
                    | ErrorCode::FeatureNotEntitled
                    | ErrorCode::KeyNotReleased
                    | ErrorCode::Forbidden
                    | ErrorCode::ValidationFailed
                    | ErrorCode::NotFound => return Err(Stop::code(error.code)),
                    ErrorCode::RateLimited => {
                        self.retrying(error.code.as_str());
                        retry_after_s.map_or(RATE_LIMIT_WAIT, |s| Duration::from_secs(s.into()))
                    }
                    code => {
                        tracing::warn!(code = code.as_str(), "restore fetch: the plan was refused");
                        self.retrying(code.as_str());
                        fails = fails.saturating_add(1);
                        self.wait_for(fails)
                    }
                },
                Err(CloudFailure::Unreachable(_)) => {
                    self.retrying(ErrorCode::CloudUnreachable.as_str());
                    fails = fails.saturating_add(1);
                    self.wait_for(fails)
                }
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Downloads the object `find` names in `links` into `dest`'s `.part` file. `Ok(None)` is an
    /// object the bucket no longer has (or a link it refuses even when fresh); `Ok(Some(n))` is
    /// `n` bytes ready to be checked and renamed.
    async fn download(
        &self,
        links: &mut Links,
        find: &(dyn Fn(&RestorePlan) -> Option<String> + Sync),
        dest: &Path,
    ) -> Result<Option<u64>, Stop> {
        let part = part_of(dest);
        if let Some(dir) = part.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        let mut refreshed_for_refusal = false;
        let mut fails = 0u32;
        loop {
            if links.at.elapsed() > LINK_REFRESH {
                self.refresh(links).await?;
            }
            let Some(url) = find(&links.plan) else {
                return Ok(None);
            };
            match self.cfg.cloud.download_object(&url, &part).await {
                Ok(n) => return Ok(Some(n)),
                Err(ObjectFailure::Status(404 | 410)) => return Ok(None),
                Err(ObjectFailure::Status(401 | 403)) => {
                    if refreshed_for_refusal {
                        return Ok(None);
                    }
                    refreshed_for_refusal = true;
                    self.refresh(links).await?;
                }
                Err(ObjectFailure::Status(_) | ObjectFailure::Unreachable(_)) => {
                    self.retrying(ErrorCode::CloudUnreachable.as_str());
                    fails = fails.saturating_add(1);
                    tokio::time::sleep(self.wait_for(fails)).await;
                }
            }
        }
    }

    async fn refresh(&self, links: &mut Links) -> Result<(), Stop> {
        links.plan = self.plan(links.after.as_ref()).await?;
        links.at = Instant::now();
        Ok(())
    }

    /// The snapshot: staged already for this date, or downloaded and checked now. Returns its
    /// last change, or `Stop` when the newest snapshot cannot be read.
    async fn stage_snapshot(
        &self,
        links: &mut Links,
        date: &str,
        bytes: &mut u64,
    ) -> Result<Option<(String, i64)>, Stop> {
        let dest = self.cfg.staging.join(SNAPSHOT_FILE);
        let info_path = self.cfg.staging.join(SNAPSHOT_INFO);
        if let Ok(text) = tokio::fs::read_to_string(&info_path).await
            && let Ok(info) = serde_json::from_str::<SnapshotInfo>(&text)
            && info.date == date
            && let Ok(meta) = tokio::fs::metadata(&dest).await
        {
            *bytes += meta.len();
            return Ok(info.epoch.map(|e| (e, info.seq)));
        }
        let _ = tokio::fs::remove_file(&info_path).await;
        let _ = tokio::fs::remove_file(&dest).await;
        let size = self
            .download(
                links,
                &|p: &RestorePlan| p.snapshot.as_ref().map(|s| s.url.clone()),
                &dest,
            )
            .await?
            .ok_or_else(|| Stop::code(ErrorCode::RestoreGap))?;
        let (part, scratch) = (part_of(&dest), self.cfg.staging.join(SNAPSHOT_SCRATCH));
        let position = tokio::task::spawn_blocking(move || {
            packed_snapshot_position(&part, &scratch)
        })
        .await
        .map_err(|_| Stop::code(ErrorCode::Internal))?
        .map_err(|e| {
            tracing::error!(detail = %e.message, "restore fetch: the snapshot cannot be read");
            Stop::code(ErrorCode::RestoreCorrupt)
        })?;
        let info = SnapshotInfo {
            date: date.to_owned(),
            epoch: position.as_ref().map(|(e, _)| e.clone()),
            seq: position.as_ref().map_or(0, |(_, s)| *s),
        };
        let text = serde_json::to_string(&info).map_err(|_| Stop::code(ErrorCode::Internal))?;
        tokio::fs::write(&info_path, text)
            .await
            .map_err(|_| Stop::code(ErrorCode::Internal))?;
        tokio::fs::rename(part_of(&dest), &dest)
            .await
            .map_err(|_| Stop::code(ErrorCode::Internal))?;
        *bytes += size;
        Ok(position)
    }

    /// The key backup: staged already, or downloaded and checked now.
    async fn stage_keys(&self, links: &mut Links, bytes: &mut u64) -> Result<(), Stop> {
        let dest = self.cfg.staging.join(KEYS_FILE);
        if let Ok(meta) = tokio::fs::metadata(&dest).await
            && meta.len() > 0
        {
            *bytes += meta.len();
            return Ok(());
        }
        let size = self
            .download(links, &|p: &RestorePlan| p.keys_url.clone(), &dest)
            .await?
            .ok_or_else(|| Stop::code(ErrorCode::RestoreGap))?;
        if size == 0 {
            let _ = tokio::fs::remove_file(part_of(&dest)).await;
            return Err(Stop::code(ErrorCode::RestoreCorrupt));
        }
        tokio::fs::rename(part_of(&dest), &dest)
            .await
            .map_err(|_| Stop::code(ErrorCode::Internal))?;
        *bytes += size;
        Ok(())
    }

    /// Whether the `.part` file of `b` is the batch the plan names.
    async fn batch_ok(&self, b: &RestorePlanBatch, part: &Path) -> bool {
        let (path, want) = (part.to_path_buf(), b.clone());
        tokio::task::spawn_blocking(move || {
            let Ok(bytes) = std::fs::read(&path) else {
                return false;
            };
            match decode_batch(&bytes) {
                Ok(d) => {
                    d.header.epoch == want.epoch
                        && d.header.first == want.first
                        && d.header.last == want.last
                }
                Err(e) => {
                    tracing::warn!(
                        epoch = %want.epoch,
                        first = want.first,
                        last = want.last,
                        detail = %e.message,
                        "restore fetch: a batch failed its check"
                    );
                    false
                }
            }
        })
        .await
        .unwrap_or(false)
    }

    async fn fetch_all(&self) -> Result<Finished, Stop> {
        let staging = &self.cfg.staging;
        tokio::fs::create_dir_all(staging)
            .await
            .map_err(|_| Stop::code(ErrorCode::Internal))?;
        let mut links = Links::new(self.plan(None).await?);
        let mut bytes = 0u64;
        let mut done = 0u32;

        let mut snapshot_date = None;
        let mut position: Option<(String, i64)> = None;
        let mut total = 0u32;
        if let Some(snap) = links.plan.snapshot.clone() {
            total += 1 + u32::from(links.plan.keys_url.is_some());
            self.progress(done, total, bytes);
            position = self
                .stage_snapshot(&mut links, &snap.date, &mut bytes)
                .await?;
            snapshot_date = Some(snap.date);
            done += 1;
            self.progress(done, total, bytes);
        } else {
            total += u32::from(links.plan.keys_url.is_some());
        }
        let has_keys = links.plan.keys_url.is_some();
        if has_keys {
            self.stage_keys(&mut links, &mut bytes).await?;
            done += 1;
            self.progress(done, total, bytes);
        }
        if snapshot_date.is_some() {
            let after = position.as_ref().map_or_else(
                || RestorePosition {
                    epoch: EMPTY_EPOCH.to_owned(),
                    seq: 0,
                },
                |(epoch, seq)| RestorePosition {
                    epoch: epoch.clone(),
                    seq: *seq,
                },
            );
            links.after = Some(after);
            self.refresh(&mut links).await?;
        }
        let batches = links.plan.batches.clone();
        total += batches.len() as u32;
        self.progress(done, total, bytes);

        let hole = first_hole(&batches, position.as_ref());
        let limit = hole.as_ref().map_or(batches.len(), |h| h.0);
        let mut bad: Option<usize> = None;
        for (i, b) in batches[..limit].iter().enumerate() {
            let path = batch_path(staging, b);
            if let Ok(meta) = tokio::fs::metadata(&path).await {
                bytes += meta.len();
                done += 1;
                self.progress(done, total, bytes);
                continue;
            }
            let key = (b.epoch.clone(), b.first, b.last);
            let find = move |p: &RestorePlan| {
                p.batches
                    .iter()
                    .find(|x| (x.epoch.as_str(), x.first, x.last) == (key.0.as_str(), key.1, key.2))
                    .map(|x| x.url.clone())
            };
            let Some(size) = self.download(&mut links, &find, &path).await? else {
                bad = Some(i);
                break;
            };
            if !self.batch_ok(b, &part_of(&path)).await {
                let _ = tokio::fs::remove_file(part_of(&path)).await;
                bad = Some(i);
                break;
            }
            tokio::fs::rename(part_of(&path), &path)
                .await
                .map_err(|_| Stop::code(ErrorCode::Internal))?;
            bytes += size;
            done += 1;
            self.progress(done, total, bytes);
        }

        let (snapshot_epoch, snapshot_seq) =
            position.clone().map_or((None, 0), |(e, s)| (Some(e), s));
        let manifest = |kept: &[RestorePlanBatch], partial: bool| StagedManifest {
            snapshot_date: snapshot_date.clone(),
            snapshot_epoch: snapshot_epoch.clone(),
            snapshot_seq,
            keys: has_keys,
            batches: kept
                .iter()
                .map(|b| StagedBatch {
                    epoch: b.epoch.clone(),
                    first: b.first,
                    last: b.last,
                })
                .collect(),
            partial,
        };
        // The earliest trouble in log order: a batch that failed, or the hole before the one
        // after it.
        let trouble = match (bad, hole) {
            (Some(i), _) => {
                let b = &batches[i];
                Some((i, b.epoch.clone(), b.first, b.last, 0))
            }
            (None, Some((i, epoch, from, to))) => Some((i, epoch, from, to, to - from + 1)),
            (None, None) => None,
        };
        let Some((at, epoch, from, to, hole_size)) = trouble else {
            let manifest = manifest(&batches, false);
            return Ok(Finished::Ready(manifest));
        };
        let last_good_seq = if at > 0 && batches[at - 1].epoch == epoch {
            batches[at - 1].last
        } else if let Some((e, s)) = position.as_ref().filter(|(e, _)| *e == epoch) {
            let _ = e;
            *s
        } else {
            from - 1
        };
        let lost = hole_size + batches[at..].iter().map(changes_in).sum::<i64>();
        let gap = RestoreGapView {
            epoch,
            from_seq: from,
            to_seq: to,
            last_good_seq,
            lost,
        };
        let state = GapState {
            manifest: manifest(&batches[..at], true),
            drop: batches[at..]
                .iter()
                .map(|b| batch_path(staging, b))
                .collect(),
        };
        Ok(Finished::Gap(Box::new(gap), state))
    }

    async fn run(self) {
        let result = self.fetch_all().await;
        match result {
            Ok(Finished::Ready(manifest)) => {
                if let Err(e) = self.write_manifest(&manifest) {
                    tracing::error!(error = %e, "restore fetch: the manifest was not written");
                    self.failed(ErrorCode::Internal.as_str());
                    return;
                }
                tracing::info!(batches = manifest.batches.len(), "restore fetch: complete");
                self.ready(&manifest);
            }
            Ok(Finished::Gap(gap, state)) => {
                tracing::warn!(
                    epoch = %gap.epoch,
                    from = gap.from_seq,
                    to = gap.to_seq,
                    lost = gap.lost,
                    "restore fetch: a gap in the backup"
                );
                *lock(&self.shared.gap) = Some(state);
                self.publish(|v| {
                    v.phase = RestoreFetchPhase::Gap;
                    v.retrying = None;
                    v.gap = Some(*gap);
                });
            }
            Err(Stop(code)) => {
                tracing::warn!(code = %code, "restore fetch: stopped");
                self.failed(&code);
            }
        }
    }

    fn failed(&self, code: &str) {
        let code = code.to_owned();
        self.publish(|v| {
            v.phase = RestoreFetchPhase::Failed;
            v.retrying = None;
            v.error = Some(code);
        });
    }

    fn ready(&self, m: &StagedManifest) {
        let view = RestoreReadyView {
            snapshot_date: m.snapshot_date.clone(),
            batches: m.batches.len() as u32,
            last_seq: m.batches.last().map_or(m.snapshot_seq, |b| b.last),
            partial: m.partial,
        };
        self.publish(|v| {
            v.phase = RestoreFetchPhase::Ready;
            v.retrying = None;
            v.gap = None;
            v.ready = Some(view);
        });
    }

    fn write_manifest(&self, m: &StagedManifest) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(m).map_err(std::io::Error::other)?;
        std::fs::write(self.cfg.staging.join(MANIFEST_FILE), text)
    }
}

/// The links in hand: the plan, when it was signed, and the position it was asked for.
struct Links {
    plan: RestorePlan,
    at: Instant,
    after: Option<RestorePosition>,
}

impl Links {
    fn new(plan: RestorePlan) -> Self {
        Self {
            plan,
            at: Instant::now(),
            after: None,
        }
    }
}

impl RestoreFetchControl for RestoreFetcher {
    fn view(&self) -> Option<RestoreFetchView> {
        lock(&self.shared.view).clone()
    }

    fn start(&self) -> Result<(), UiError> {
        self.check_allowed()?;
        let mut job = lock(&self.shared.job);
        if job.as_ref().is_some_and(|j| !j.is_finished()) {
            return Ok(());
        }
        *lock(&self.shared.gap) = None;
        *lock(&self.shared.view) = Some(RestoreFetchView {
            phase: RestoreFetchPhase::Downloading,
            done: 0,
            total: 0,
            bytes: 0,
            percent: 0,
            retrying: None,
            error: None,
            gap: None,
            ready: None,
        });
        let me = self.clone();
        *job = Some(tokio::spawn(me.run()));
        Ok(())
    }

    fn cancel(&self) {
        if let Some(job) = lock(&self.shared.job).take() {
            job.abort();
        }
        *lock(&self.shared.gap) = None;
        *lock(&self.shared.view) = None;
        if let Err(e) = std::fs::remove_dir_all(&self.cfg.staging)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %e, "restore fetch: the staging folder was not removed");
        }
    }

    fn restore_partial(&self) -> Result<(), UiError> {
        let Some(state) = lock(&self.shared.gap).take() else {
            return Err(ui_error(ErrorCode::NotFound, "There is no gap to stop at."));
        };
        for path in &state.drop {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(part_of(path));
        }
        self.write_manifest(&state.manifest).map_err(|e| {
            tracing::error!(error = %e, "restore fetch: the manifest was not written");
            ui_error(ErrorCode::Internal, "The backup could not be saved.")
        })?;
        self.ready(&state.manifest);
        Ok(())
    }
}

#[cfg(test)]
#[path = "restore_fetch_tests.rs"]
mod tests;

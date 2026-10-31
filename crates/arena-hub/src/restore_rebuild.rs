//! "Rebuild this branch" from the backup the Hub downloaded (Story 9.13, AD-4, AD-11, AD-24).
//!
//! The Owner has released the branch key (Story 9.10) and `restore-staging/` holds a complete
//! backup (Story 9.12). The rebuild runs on a blocking task, away from the live database:
//! `arena_replay::rebuild` unpacks the snapshot, replays the batches honouring fork points, puts
//! the player keys back from the key backup and checks the result into
//! `restore-rebuild.db`. That file is then staged exactly like a restore from an export file
//! (`arena_store::stage_rebuilt`) and the Hub restarts onto it: at start-up, before the writer
//! opens, the swap of Story 9.7 opens the new epoch forked at the last change replayed and the
//! Hub records `ops.restore`. Cancelling before the restart removes the rebuilt file and leaves
//! the Hub in "Restore this branch" with the downloaded backup still staged.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arena_core::players::KEK_LEN;
use arena_core::{
    Clock, HubStatus, KeyBackupLookup, KeyLookupFuture, PairingState, RestoreFetchControl,
    RestoreRebuildControl, key_backup::open_key_table,
};
use arena_proto::{
    CloudRestoreFacts, ErrorCode, HubPlatform, RestoreFetchPhase, RestorePosition,
    RestoreRebuildPhase, RestoreRebuildView, Tier, UiError,
};
use arena_replay::{RebuildSource, RebuildStage, RebuildStop, Rebuilt, rebuild};
use arena_store::PlayerKeyRow;
use arena_tasks::{Keystore, RESTORE_STAGING_DIR, RestoreFetcher, StagedManifest};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const REBUILD_FILE: &str = "restore-rebuild.db";
const SNAPSHOT_FILE: &str = "snapshot.db.zst";
const KEYS_FILE: &str = "player-keys.bin";

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn refuse(code: ErrorCode, message: &str) -> UiError {
    UiError {
        code,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

/// The path of a staged batch (Story 9.12 layout; the `:` of an epoch is written `@`).
fn batch_file(staging: &Path, epoch: &str, first: i64, last: i64) -> PathBuf {
    staging
        .join("changes")
        .join(epoch.replace(':', "@"))
        .join(format!("{first:012}-{last:012}.zst"))
}

#[derive(Default)]
struct State {
    view: Option<RestoreRebuildView>,
    cancel: Option<Arc<AtomicBool>>,
    job: Option<JoinHandle<()>>,
}

struct Inner {
    fetch: Arc<dyn RestoreFetchControl>,
    kek: Arc<dyn Keystore>,
    db_path: PathBuf,
    data_dir: PathBuf,
    clock: Arc<dyn Clock>,
    status: watch::Receiver<HubStatus>,
    /// Stops the Hub so it starts again onto the staged rebuild.
    restart: fn(),
    state: Mutex<State>,
}

/// The control the Hub screen uses and the rebuild behind it.
#[derive(Clone)]
pub struct Rebuilder {
    inner: Arc<Inner>,
}

impl Rebuilder {
    pub fn new(
        fetch: Arc<dyn RestoreFetchControl>,
        kek: Arc<dyn Keystore>,
        db_path: PathBuf,
        data_dir: PathBuf,
        clock: Arc<dyn Clock>,
        status: watch::Receiver<HubStatus>,
        restart: fn(),
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                fetch,
                kek,
                db_path,
                data_dir,
                clock,
                status,
                restart,
                state: Mutex::new(State::default()),
            }),
        }
    }

    fn check_allowed(&self) -> Result<(), UiError> {
        let i = &self.inner;
        {
            let s = i.status.borrow();
            if s.restore_pending.is_none() {
                return Err(refuse(
                    ErrorCode::NotFound,
                    "This PC is not waiting to be restored.",
                ));
            }
            if s.pairing == PairingState::Revoked {
                return Err(refuse(
                    ErrorCode::TokenRevoked,
                    "This Hub was replaced by another.",
                ));
            }
            if s.platform != HubPlatform::Windows {
                return Err(refuse(
                    ErrorCode::Unsupported,
                    "Rebuilding from the cloud backup needs the Windows Hub.",
                ));
            }
            if s.tier == Some(Tier::Free) {
                return Err(refuse(
                    ErrorCode::FeatureNotEntitled,
                    "Restoring from the cloud needs a paid plan.",
                ));
            }
            if !s.restore_key_released {
                return Err(refuse(
                    ErrorCode::KeyNotReleased,
                    "Waiting for the owner to release the branch key.",
                ));
            }
        }
        match i.fetch.view() {
            Some(v) if v.phase == RestoreFetchPhase::Ready => Ok(()),
            _ => Err(refuse(
                ErrorCode::NotFound,
                "The backup has not finished downloading.",
            )),
        }
    }

    fn publish(&self, f: impl FnOnce(&mut RestoreRebuildView)) {
        if let Some(v) = lock(&self.inner.state).view.as_mut() {
            f(v);
        }
    }

    fn progress(&self, phase: RestoreRebuildPhase, done: u32, total: u32) {
        self.publish(|v| {
            v.phase = phase;
            v.done = done;
            v.total = total;
            v.percent = (u64::from(done) * 100)
                .checked_div(u64::from(total))
                .map_or(100, |p| p.min(100) as u8);
        });
    }

    fn failed(&self, code: &str) {
        let code = code.to_owned();
        self.publish(|v| {
            v.phase = RestoreRebuildPhase::Failed;
            v.error = Some(code);
        });
    }

    /// The whole rebuild, blocking: read the staging folder, rebuild, stage, ask for a restart.
    fn run(&self, cancel: &AtomicBool) {
        let i = &self.inner;
        let staging = i.data_dir.join(RESTORE_STAGING_DIR);
        let work = i.data_dir.join(REBUILD_FILE);
        match self.rebuild_staged(&staging, &work, cancel) {
            Ok(()) => {
                // The cancel flag and the phase change are decided together: a cancel that got in
                // before this point unstages the rebuilt file, one after it is too late.
                let cancelled = {
                    let mut state = lock(&i.state);
                    if cancel.load(Ordering::SeqCst) {
                        state.view = None;
                        true
                    } else {
                        drop(state);
                        self.progress(RestoreRebuildPhase::Restarting, 1, 1);
                        false
                    }
                };
                if cancelled {
                    tracing::info!("restore rebuild: cancelled before the restart");
                    let _ = std::fs::remove_dir_all(i.data_dir.join(arena_store::PENDING_DIR));
                } else {
                    (i.restart)();
                }
            }
            Err(RebuildStop::Cancelled) => {
                tracing::info!("restore rebuild: cancelled before the restart");
                lock(&i.state).view = None;
            }
            Err(RebuildStop::Failed(e)) => {
                tracing::error!(
                    code = e.code.as_str(),
                    detail = %e.message,
                    "restore rebuild: failed; the downloaded backup is kept"
                );
                self.failed(e.code.as_str());
            }
        }
    }

    fn rebuild_staged(
        &self,
        staging: &Path,
        work: &Path,
        cancel: &AtomicBool,
    ) -> Result<(), RebuildStop> {
        let i = &self.inner;
        let fail = |what: &str| {
            RebuildStop::Failed(arena_proto::AppError::new(
                ErrorCode::RestoreCorrupt,
                what.to_owned(),
            ))
        };
        let manifest = StagedManifest::read(staging)
            .ok_or_else(|| fail("the staged backup has no manifest"))?;

        // The branch key opens the key backup; without the key nothing here can be trusted.
        let kek: [u8; KEK_LEN] = match i.kek.get() {
            Ok(Some(bytes)) => bytes.try_into().map_err(|_| {
                RebuildStop::Failed(arena_proto::AppError::new(
                    ErrorCode::KeyNotReleased,
                    "the branch key is not usable",
                ))
            })?,
            _ => {
                return Err(RebuildStop::Failed(arena_proto::AppError::new(
                    ErrorCode::KeyNotReleased,
                    "the branch key is not in this Hub's keystore",
                )));
            }
        };
        let keys: Vec<PlayerKeyRow> = if manifest.keys {
            let bytes = std::fs::read(staging.join(KEYS_FILE))
                .map_err(|_| fail("the staged key backup is missing"))?;
            open_key_table(&kek, &bytes).map_err(RebuildStop::Failed)?
        } else {
            Vec::new()
        };

        let snapshot = staging.join(SNAPSHOT_FILE);
        let snapshot = manifest.snapshot_date.is_some().then_some(snapshot);
        let batches: Vec<PathBuf> = manifest
            .batches
            .iter()
            .map(|b| batch_file(staging, &b.epoch, b.first, b.last))
            .collect();
        let now = i.clock.now_ms();
        let done: Rebuilt = rebuild(
            work,
            &RebuildSource {
                snapshot: snapshot.as_deref(),
                batches: &batches,
                keys: &keys,
            },
            now,
            cancel,
            &mut |stage, done, total| {
                let phase = match stage {
                    RebuildStage::Replaying => RestoreRebuildPhase::Replaying,
                    RebuildStage::Checking => RestoreRebuildPhase::Checking,
                };
                self.progress(phase, done, total);
            },
        )?;
        tracing::info!(
            batches = done.batches,
            applied = done.applied,
            orphaned = done.orphaned,
            keys_restored = done.keys_restored,
            keys_dropped = done.keys_dropped,
            pair_again = done.pair_again,
            "restore rebuild: the branch is rebuilt"
        );

        // From here the Hub restarts; a cancel no longer applies.
        let position = |r: &arena_replay::Resume| RestorePosition {
            epoch: r.epoch.clone(),
            seq: r.seq,
        };
        let facts = CloudRestoreFacts {
            snapshot_date: manifest.snapshot_date.clone(),
            first: done.first_applied.as_ref().map(position),
            last: if done.applied > 0 {
                done.last.as_ref().map(position)
            } else {
                None
            },
            batches: done.batches,
            applied: done.applied,
            orphaned: done.orphaned,
            keys_restored: done.keys_restored,
        };
        check_cancel(cancel, work)?;
        let staged = arena_store::stage_rebuilt(&i.db_path, work, facts, now);
        let _ = std::fs::remove_file(work);
        staged.map(|_| ()).map_err(RebuildStop::Failed)
    }
}

fn check_cancel(cancel: &AtomicBool, work: &Path) -> Result<(), RebuildStop> {
    if cancel.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(work);
        Err(RebuildStop::Cancelled)
    } else {
        Ok(())
    }
}

impl RestoreRebuildControl for Rebuilder {
    fn view(&self) -> Option<RestoreRebuildView> {
        lock(&self.inner.state).view.clone()
    }

    fn start(&self) -> Result<(), UiError> {
        self.check_allowed()?;
        let mut state = lock(&self.inner.state);
        // Once staged the Hub is restarting: a second start must not clear what was staged.
        if state
            .view
            .as_ref()
            .is_some_and(|v| v.phase == RestoreRebuildPhase::Restarting)
        {
            return Ok(());
        }
        if state.job.as_ref().is_some_and(|j| !j.is_finished()) {
            // A start that follows a cancel would be lost when the job stops: say so.
            if state
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::SeqCst))
            {
                return Err(refuse(
                    ErrorCode::StepNotReady,
                    "The rebuild is still stopping. Try again in a moment.",
                ));
            }
            return Ok(());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancel = Some(Arc::clone(&cancel));
        state.view = Some(RestoreRebuildView {
            phase: RestoreRebuildPhase::Replaying,
            done: 0,
            total: 0,
            percent: 0,
            error: None,
        });
        let me = self.clone();
        state.job = Some(tokio::task::spawn_blocking(move || me.run(&cancel)));
        Ok(())
    }

    fn cancel(&self) {
        let mut state = lock(&self.inner.state);
        // After the file is staged the Hub is restarting: that cannot be taken back here.
        if state
            .view
            .as_ref()
            .is_some_and(|v| v.phase == RestoreRebuildPhase::Restarting)
        {
            return;
        }
        if let Some(cancel) = &state.cancel {
            cancel.store(true, Ordering::SeqCst);
        }
        let running = state.job.as_ref().is_some_and(|j| !j.is_finished());
        if !running {
            state.view = None;
            let _ = std::fs::remove_file(self.inner.data_dir.join(REBUILD_FILE));
        }
    }
}

/// Whom the cloud key backup holds a key for (Story 9.27): the sealed backup is fetched by the
/// [`RestoreFetcher`] and opened with the branch key, on this Hub's own keystore. Anything that
/// fails (Free, offline, no backup, no key, a backup the key does not open) answers `None`, and a
/// restore then erases nobody extra.
pub struct BackupKeys {
    fetcher: RestoreFetcher,
    kek: Arc<dyn Keystore>,
}

impl BackupKeys {
    pub fn new(fetcher: RestoreFetcher, kek: Arc<dyn Keystore>) -> Self {
        Self { fetcher, kek }
    }

    fn open(&self, sealed: &[u8]) -> Option<Vec<String>> {
        let bytes = self.kek.get().ok()??;
        let kek: [u8; KEK_LEN] = bytes.try_into().ok()?;
        match open_key_table(&kek, sealed) {
            // A backup that opens but holds no key at all is a backup not yet filled, not proof that
            // every player was erased: erasing them all cannot be undone, so it answers nothing.
            Ok(rows) if rows.is_empty() => {
                tracing::warn!("key backup lookup: the backup holds no key, so it is not used");
                None
            }
            Ok(rows) => Some(rows.into_iter().map(|r| r.player_id).collect()),
            Err(e) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "key backup lookup: the backup does not open with the branch key"
                );
                None
            }
        }
    }
}

impl KeyBackupLookup for BackupKeys {
    fn live_keys(&self) -> KeyLookupFuture<'_> {
        Box::pin(async move {
            let sealed = self.fetcher.key_backup_bytes().await?;
            let ids = self.open(&sealed);
            if let Some(ids) = &ids {
                tracing::info!(keys = ids.len(), "key backup lookup: the backup was read");
            }
            ids
        })
    }
}

#[cfg(test)]
#[path = "restore_rebuild_tests.rs"]
mod tests;

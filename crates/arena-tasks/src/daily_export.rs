//! Saves the daily export to the backup location (Story 9.5).
//!
//! A primary Hub asks the snapshot reader once a minute whether the business day has closed
//! ([`arena_core::auto_export::plan_auto_export`]: closing time + 30 minutes, or the last shift
//! close). When it has, the task makes the Story 9.4 export under the auto file name, records it
//! with `backup.export` as `actor{system}` and deletes the auto files beyond the newest seven;
//! files a person exported are never touched. If the location cannot take the file, the Hub
//! header shows "Backup folder not available" until the next export succeeds, and the task tries
//! again every 15 minutes.
//!
//! A location that is a platform folder URI (Android) cannot be written by the Hub yet (the same
//! limit as Story 9.4): the task skips it quietly rather than alert every day for something the
//! owner cannot fix.

use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, HubStatus, Role, SnapshotReader, WriterHandle};
use arena_proto::{
    AUTO_EXPORT_ACTOR, Actor, ActorKind, AppError, BackupExport, ErrorCode, Intent, IntentPayload,
    KEY_BACKUP_DIR,
};
use tokio::sync::watch;

/// How often the plan is looked at: a closing shift is noticed within this.
pub const DAILY_EXPORT_TICK: Duration = Duration::from_secs(60);
/// How long after a failed export the next attempt waits.
pub const DAILY_EXPORT_RETRY: Duration = Duration::from_secs(15 * 60);

pub struct DailyExportTask {
    writer: WriterHandle,
    snapshots: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    status: watch::Sender<HubStatus>,
    tick: Duration,
    retry: Duration,
}

/// What one pass found, for the loop and the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Not the primary, or the branch is not set up.
    Idle,
    /// Nothing due.
    NotDue,
    /// Due, but no location is set (or it is one the Hub cannot write).
    NoLocation,
    /// The export was saved.
    Saved,
    /// The export could not be saved; the header alerts.
    Failed,
}

fn record(facts: BackupExport, now: i64) -> Intent {
    let payload = IntentPayload::BackupExport(facts);
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: AUTO_EXPORT_ACTOR.into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

impl DailyExportTask {
    pub fn new(
        writer: WriterHandle,
        snapshots: Arc<SnapshotReader>,
        clock: Arc<dyn Clock>,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            writer,
            snapshots,
            clock,
            status,
            tick: DAILY_EXPORT_TICK,
            retry: DAILY_EXPORT_RETRY,
        }
    }

    /// Tests: how often the plan is looked at and how long a failure waits.
    pub fn with_timing(mut self, tick: Duration, retry: Duration) -> Self {
        self.tick = tick;
        self.retry = retry;
        self
    }

    async fn blocking<T: Send + 'static>(
        &self,
        what: &'static str,
        job: impl FnOnce(&SnapshotReader) -> Result<T, AppError> + Send + 'static,
    ) -> Option<T> {
        let reader = Arc::clone(&self.snapshots);
        match tokio::task::spawn_blocking(move || job(&reader)).await {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), what, "daily export: read failed");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, what, "daily export: task failed");
                None
            }
        }
    }

    /// Records whether the last attempt failed and, when the attempt showed it, whether the folder
    /// is unavailable; refreshes the setup status when either changed.
    async fn flag(&self, failed: bool, folder_down: Option<bool>) {
        let mut changed = self.snapshots.set_export_failed(failed);
        if let Some(down) = folder_down {
            changed |= self.snapshots.set_backup_unavailable(down);
        }
        if changed {
            arena_core::setup::refresh(&self.snapshots, &self.status).await;
        }
    }

    /// One look: exports if the day has closed and no auto export covers it.
    pub async fn pass(&self) -> Pass {
        if self.status.borrow().role != Role::Primary {
            return Pass::Idle;
        }
        let now = self.clock.now_ms();
        let Some(Some(plan)) = self
            .blocking("plan", move |r| r.auto_export_plan(now))
            .await
        else {
            return Pass::Idle;
        };
        if !plan.due {
            return Pass::NotDue;
        }
        let folder = self
            .blocking("folder", |r| r.setting_value(KEY_BACKUP_DIR))
            .await
            .flatten()
            .unwrap_or_default();
        if folder.trim().is_empty() || arena_proto::is_backup_tree_uri(&folder) {
            // Nothing to write to, and nothing the owner is told to fix here.
            self.flag(false, None).await;
            return Pass::NoLocation;
        }
        let (target, at) = (folder.clone(), now);
        let made = self
            .blocking("export", move |r| {
                // A failure to write is an answer, not a read error.
                Ok(r.auto_export_to_folder(&target, at))
            })
            .await;
        let (facts, path) = match made {
            Some(Ok(done)) => done,
            Some(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "daily export: could not save");
                let folder_problem = e.code == ErrorCode::ValidationFailed;
                self.flag(true, folder_problem.then_some(true)).await;
                return Pass::Failed;
            }
            None => {
                self.flag(true, None).await;
                return Pass::Failed;
            }
        };
        let file = facts.file_name.clone();
        match self.writer.submit_intent(record(facts, now)).await {
            Ok(_) => {
                tracing::info!(file = %file, "daily export saved");
                let target = folder;
                if let Some(gone) = self
                    .blocking("prune", move |r| r.prune_auto_exports(&target))
                    .await
                    && !gone.is_empty()
                {
                    tracing::info!(deleted = gone.len(), "old daily exports deleted");
                }
                self.flag(false, Some(false)).await;
                Pass::Saved
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                // Demoted while exporting: the file is a copy of data this Hub no longer owns.
                let _ = std::fs::remove_file(&path);
                Pass::Idle
            }
            Err(e) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "daily export: could not be recorded"
                );
                let _ = std::fs::remove_file(&path);
                self.flag(true, None).await;
                Pass::Failed
            }
        }
    }

    pub async fn run(self) {
        loop {
            let wait = match self.pass().await {
                Pass::Failed => self.retry,
                _ => self.tick,
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LeaseTask;
    use arena_core::{ManualClock, status_channel};
    use arena_store::Store;

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

    struct Fx {
        _dir: tempfile::TempDir,
        out: tempfile::TempDir,
        status: watch::Sender<HubStatus>,
        task: DailyExportTask,
        snapshots: Arc<SnapshotReader>,
        writer: WriterHandle,
    }

    fn intent(payload: IntentPayload, kind: ActorKind, base_rev: Option<i64>) -> Intent {
        Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev,
            actor: Actor {
                kind,
                id: "test".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    /// A bootstrapped Kolkata branch `ARN`, the backup location set to `out` when `set` is true.
    async fn fx(set: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(73, OWN));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
        let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        writer
            .submit_intent(intent(
                IntentPayload::BranchBootstrap(arena_proto::BranchBootstrap {
                    name: "Arena".into(),
                    branch_code: "ARN".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 360,
                    fy_start_month: 4,
                    onboarding: None,
                }),
                ActorKind::System,
                None,
            ))
            .await
            .unwrap();
        let snapshots = Arc::new(SnapshotReader::open(&path).unwrap());
        if set {
            set_folder(&writer, &snapshots, out.path().to_str().unwrap()).await;
        }
        let task = DailyExportTask::new(
            writer.clone(),
            Arc::clone(&snapshots),
            clock,
            status.clone(),
        );
        Fx {
            _dir: dir,
            out,
            status,
            task,
            snapshots,
            writer,
        }
    }

    async fn set_folder(writer: &WriterHandle, snapshots: &SnapshotReader, folder: &str) {
        let (rev, _) = snapshots
            .setting_rev_and_replay(KEY_BACKUP_DIR, "x")
            .unwrap();
        writer
            .submit_intent(intent(
                IntentPayload::SettingsSet(arena_proto::SettingsSet {
                    key: KEY_BACKUP_DIR.into(),
                    value: folder.into(),
                }),
                ActorKind::Owner,
                rev,
            ))
            .await
            .unwrap();
    }

    fn files(fx: &Fx) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(fx.out.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn last_export(fx: &Fx) -> Option<BackupExport> {
        BackupExport::parse(
            &fx.snapshots
                .setting_value(arena_proto::KEY_BACKUP_LAST_EXPORT)
                .unwrap()
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn a_closed_day_is_exported_once_as_the_system_and_recorded_with_no_person() {
        let f = fx(true).await;
        assert_eq!(f.task.pass().await, Pass::Saved);
        assert_eq!(files(&f), vec!["ARN-auto-2027-01-15-1330.a360"]);
        let kept = last_export(&f).unwrap();
        assert_eq!(kept.file_name, "ARN-auto-2027-01-15-1330.a360");
        assert_eq!(kept.by, None);
        assert_eq!(f.task.pass().await, Pass::NotDue, "the day is done");
        assert_eq!(files(&f).len(), 1);
        assert!(!f.snapshots.setup().unwrap().unwrap().backup_export_failed);
    }

    #[tokio::test]
    async fn only_the_newest_seven_auto_files_stay_and_a_persons_file_is_never_deleted() {
        let f = fx(true).await;
        for day in 1..=8 {
            std::fs::write(
                f.out
                    .path()
                    .join(format!("ARN-auto-2027-01-0{day}-0130.a360")),
                b"old",
            )
            .unwrap();
        }
        std::fs::write(f.out.path().join("ARN-2027-01-01-0900.a360"), b"mine").unwrap();
        assert_eq!(f.task.pass().await, Pass::Saved);
        let names = files(&f);
        assert_eq!(names.iter().filter(|n| n.contains("-auto-")).count(), 7);
        assert!(!names.contains(&"ARN-auto-2027-01-01-0130.a360".to_owned()));
        assert!(!names.contains(&"ARN-auto-2027-01-02-0130.a360".to_owned()));
        assert!(names.contains(&"ARN-auto-2027-01-15-1330.a360".to_owned()));
        assert!(names.contains(&"ARN-2027-01-01-0900.a360".to_owned()));
    }

    #[tokio::test]
    async fn a_missing_folder_raises_the_alert_until_the_next_export_succeeds() {
        let f = fx(false).await;
        let gone = f.out.path().join("usb");
        set_folder(&f.writer, &f.snapshots, gone.to_str().unwrap()).await;
        assert_eq!(f.task.pass().await, Pass::Failed);
        let setup = f.snapshots.setup().unwrap().unwrap();
        assert!(setup.backup_export_failed);
        assert!(
            !setup.backup_location_set,
            "setup reports no usable location"
        );
        assert!(
            f.status
                .borrow()
                .setup
                .is_some_and(|s| s.backup_export_failed),
            "the header's status carries it"
        );

        std::fs::create_dir(&gone).unwrap();
        assert_eq!(f.task.pass().await, Pass::Saved);
        let setup = f.snapshots.setup().unwrap().unwrap();
        assert!(!setup.backup_export_failed);
        assert!(setup.backup_location_set);
        assert!(
            f.status
                .borrow()
                .setup
                .is_some_and(|s| !s.backup_export_failed)
        );
        assert_eq!(std::fs::read_dir(&gone).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn no_location_writes_nothing_and_raises_no_alert() {
        let f = fx(false).await;
        assert_eq!(f.task.pass().await, Pass::NoLocation);
        assert!(files(&f).is_empty());
        assert!(!f.snapshots.setup().unwrap().unwrap().backup_export_failed);
    }

    #[tokio::test]
    async fn a_follower_hub_exports_nothing() {
        let f = fx(true).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        assert_eq!(f.task.pass().await, Pass::Idle);
        assert!(files(&f).is_empty());
    }
}

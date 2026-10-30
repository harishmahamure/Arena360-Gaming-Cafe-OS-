//! Restore from a file (Story 9.7), the Hub's part: apply a staged restore before the database
//! is opened, record `ops.restore` once the writer runs, sweep `orphaned/`, and start the Hub
//! again after the Owner confirms.
//!
//! The swap itself is `arena_store::apply_pending`. A failure there leaves the old database in
//! place and the Hub starts normally, so a bad restore never costs a branch its trading.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_core::{Clock, WriterHandle};
use arena_proto::{
    Actor, ActorKind, ErrorCode, Intent, IntentKind, IntentPayload, RESTORE_SYSTEM_ACTOR,
};
use arena_store::RestoreOutcome;

static RESTART: AtomicBool = AtomicBool::new(false);

/// Set to `1` by a supervisor (a service) that starts the Hub again by itself when it exits; the
/// Hub then leaves the restart after a restore to it.
const SUPERVISED_ENV: &str = "ARENA_HUB_SUPERVISED";

/// Stops the Hub so it starts again onto the restore the Owner just staged.
pub fn request_restart() {
    RESTART.store(true, Ordering::SeqCst);
    crate::stop();
}

/// Whether [`request_restart`] was called.
pub fn restart_requested() -> bool {
    RESTART.load(Ordering::SeqCst)
}

/// Starts this executable again with the same arguments, unless a supervisor does that. Called
/// once the Hub has closed its database and listeners.
pub fn relaunch(args: &[OsString]) {
    if std::env::var_os(SUPERVISED_ENV).is_some_and(|v| v == "1") {
        tracing::info!("restore staged; the supervisor starts the hub again");
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            tracing::error!(error = %e, "cannot find the hub executable to start it again");
            return;
        }
    };
    let mut command = std::process::Command::new(exe);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: outlive this process and its console.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    match command.spawn() {
        Ok(child) => tracing::info!(pid = child.id(), "the hub started again onto the restore"),
        Err(e) => tracing::error!(error = %e, "could not start the hub again after the restore"),
    }
}

/// Applies a staged restore (if any) and sweeps `orphaned/`. Answers the outcome to record: the
/// one just made, or one an earlier start made and did not get to record.
pub async fn apply_at_start(
    db_path: PathBuf,
    windows: bool,
    now_ms: i64,
) -> Option<RestoreOutcome> {
    let job = tokio::task::spawn_blocking(move || {
        let applied = if windows {
            match arena_store::apply_pending(&db_path, now_ms) {
                Ok(Some(done)) => {
                    tracing::warn!(
                        old_epoch = %done.restore.old_epoch,
                        new_epoch = %done.restore.new_epoch,
                        rows_set_aside = done.restore.rows_set_aside,
                        erasures_reapplied = done.restore.erasures_reapplied,
                        "restored the branch from an export file as a new epoch"
                    );
                    if done.restore.cloud.is_some() {
                        // The downloaded backup has done its work; the rebuilt branch is the copy.
                        clear_cloud_staging(&db_path);
                    }
                    Some(done)
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::error!(
                        code = e.code.as_str(),
                        detail = %e.message,
                        "the staged restore failed; the current database is kept"
                    );
                    None
                }
            }
        } else {
            None
        };
        let swept = arena_store::sweep_orphaned(&db_path, now_ms);
        if swept > 0 {
            tracing::info!(
                files = swept,
                "deleted orphaned databases older than 90 days"
            );
        }
        applied.or_else(|| arena_store::read_outcome(&db_path))
    })
    .await;
    match job {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::error!(error = %e, "the restore start-up task failed");
            None
        }
    }
}

/// Removes the folder of the downloaded cloud backup (Story 9.12) once it was rebuilt (Story 9.13).
fn clear_cloud_staging(db_path: &Path) {
    let Some(dir) = db_path.parent() else {
        return;
    };
    let staging = dir.join(arena_tasks::RESTORE_STAGING_DIR);
    if let Err(e) = std::fs::remove_dir_all(&staging)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, "the downloaded backup could not be removed after the rebuild");
    }
}

fn record_intent(outcome: &RestoreOutcome, actor: Actor, now_ms: i64) -> Intent {
    Intent {
        intent_id: outcome.intent_id.clone(),
        kind: IntentKind::OpsRestore,
        idempotency_key: outcome.idempotency_key.clone(),
        issued_at: now_ms,
        base_rev: None,
        actor,
        elevated_by: None,
        reason: None,
        payload: IntentPayload::OpsRestore(outcome.restore.clone()),
    }
}

/// Records `ops.restore` as the Owner who confirmed it (or as the Hub when that Owner is not in
/// the restored database), then forgets the outcome. A Hub that is not primary, or a failure,
/// leaves the outcome for the next start.
pub fn spawn_record(
    writer: Option<WriterHandle>,
    clock: Arc<dyn Clock>,
    db_path: PathBuf,
    outcome: RestoreOutcome,
) -> Option<tokio::task::JoinHandle<()>> {
    let writer = writer?;
    Some(tokio::spawn(async move {
        // The setting `ops.restore` writes may not exist in a file made by an older build.
        if let Err(e) = writer.seed_defaults().await {
            tracing::warn!(
                code = e.code.as_str(),
                "seeding defaults before the restore record failed"
            );
        }
        let owner = Actor {
            kind: ActorKind::Staff,
            id: outcome.confirmed_by.clone(),
        };
        let mut sent = writer
            .submit_intent(record_intent(&outcome, owner, clock.now_ms()))
            .await;
        // The Owner who confirmed may have been added after the file was made.
        if sent.as_ref().is_err_and(|e| {
            matches!(
                e.code,
                ErrorCode::PermissionDenied | ErrorCode::ValidationFailed
            )
        }) {
            let hub = Actor {
                kind: ActorKind::System,
                id: RESTORE_SYSTEM_ACTOR.into(),
            };
            sent = writer
                .submit_intent(record_intent(&outcome, hub, clock.now_ms()))
                .await;
        }
        match sent {
            Ok(_) => {
                forget(&db_path);
                tracing::info!(new_epoch = %outcome.restore.new_epoch, "ops.restore recorded");
            }
            Err(e) => tracing::error!(
                code = e.code.as_str(),
                detail = %e.message,
                "ops.restore could not be recorded; it is tried again at the next start"
            ),
        }
    }))
}

fn forget(db_path: &Path) {
    arena_store::clear_outcome(db_path);
}

//! The Windows Hub imports the Android Hub's final snapshot as the next epoch (Story 10.18).
//!
//! [`MigrationImporter::import`] runs once the Hub holds a checked snapshot (Story 10.17) and the
//! owner released the branch key (the same request and release as for a replacement Hub, Story
//! 9.10; [`crate::KeyRequestTask`] asks for it as soon as the snapshot is here):
//!
//! 1. `POST /v1/migration/claim` moves the branch fence from the snapshot's epoch to the next one,
//!    minted for this Hub, with `fork = {snapshot epoch, last seq}`. Asking again for the epoch this
//!    Hub already holds answers it again, so a failed attempt can simply be repeated.
//! 2. The snapshot is staged exactly as a restore from a file is ([`arena_store::stage_migration`]),
//!    with that epoch, and the Hub restarts. At start the staged file becomes the database:
//!    migrated forward, serving under the claimed epoch, nothing set aside.
//! 3. [`ImportReporter`] tells the cloud (`POST /v1/migration/imported`) that the imported branch
//!    is serving; the cloud then makes this Hub the branch's main Hub and revokes the Android Hub.
//!    The Hub does not beat before that, because its token is still the move's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arena_core::handover::{Handover, ImportFuture, JoinFuture, MigrationImportControl};
use arena_core::{HubStatus, PairingState, WriterHandle};
use arena_proto::{
    ErrorCode, FenceFork, HANDOVER_PURPOSE, HandoverFileRequest, HandoverSnapshotFacts,
    HandoverView, MIGRATION_REPORT_KEY, MigrationClaimRequest, MigrationImportedRequest,
    MigrationImporting, UiError,
};
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;

/// The longest wait between two failed reports.
const RETRY_CEILING: Duration = Duration::from_secs(5 * 60);
/// How long the answer takes to reach the browser before the Hub restarts.
const RESTART_DELAY: Duration = Duration::from_millis(500);

/// What stops the Hub for the import: the same restart the restore from a file uses.
pub type RestartHook = Arc<dyn Fn() + Send + Sync>;

fn err(code: ErrorCode, message: impl Into<String>) -> UiError {
    UiError {
        code,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

fn app_err(e: arena_proto::AppError) -> UiError {
    err(e.code, e.message)
}

fn path_err(reason: &str, message: &str) -> UiError {
    UiError {
        field: Some("path".into()),
        ..err(ErrorCode::ValidationFailed, format!("{message} ({reason})"))
    }
}

/// How a refused call reads to the person at the PC.
fn cloud_err(failure: CloudFailure) -> UiError {
    match failure {
        CloudFailure::Refused { error, .. } => match error.code {
            ErrorCode::Fenced => err(
                ErrorCode::Fenced,
                "The branch moved on since the snapshot was made. Cancel the move and start again.",
            ),
            ErrorCode::StepNotReady => err(
                ErrorCode::StepNotReady,
                "The cloud has not heard from the tablet that it handed over yet. Try again in a minute.",
            ),
            ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => err(
                ErrorCode::TokenRevoked,
                "This PC is no longer part of the move. Start the move again from the owner app.",
            ),
            code => err(code, error.message),
        },
        CloudFailure::Unreachable(_) => crate::cloud::unreachable(),
    }
}

/// The Windows Hub's import of a received snapshot.
pub struct MigrationImporter {
    handover: Arc<Handover>,
    status: watch::Sender<HubStatus>,
    cloud: CloudClient,
    token: Arc<dyn Keystore>,
    hub_instance_id: String,
    db_path: PathBuf,
    /// Where a snapshot picked from a file is kept.
    dir: PathBuf,
    restart: RestartHook,
    busy: tokio::sync::Mutex<()>,
}

impl MigrationImporter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handover: Arc<Handover>,
        status: watch::Sender<HubStatus>,
        cloud: CloudClient,
        token: Arc<dyn Keystore>,
        hub_instance_id: String,
        db_path: PathBuf,
        dir: PathBuf,
        restart: RestartHook,
    ) -> Self {
        Self {
            handover,
            status,
            cloud,
            token,
            hub_instance_id,
            db_path,
            dir,
            restart,
            busy: tokio::sync::Mutex::new(()),
        }
    }

    async fn token(&self) -> Result<String, UiError> {
        let token = Arc::clone(&self.token);
        match tokio::task::spawn_blocking(move || token.get()).await {
            Ok(Ok(Some(t))) if !t.is_empty() => Ok(String::from_utf8_lossy(&t).into_owned()),
            _ => Err(err(ErrorCode::StepNotReady, "This PC is not paired yet.")),
        }
    }

    async fn use_file_inner(&self, req: HandoverFileRequest) -> Result<HandoverView, UiError> {
        let _one = self.busy.lock().await;
        let Some(branch_name) = self.status.borrow().migration_wait.clone() else {
            return Err(err(
                ErrorCode::StepNotReady,
                "This PC is not waiting for an Android Hub.",
            ));
        };
        if let Some(view) = self.handover.view().await {
            return Ok(view);
        }
        let path = PathBuf::from(req.path.trim());
        if path.as_os_str().is_empty() {
            return Err(path_err("empty", "Type where the snapshot file is."));
        }
        let (db, dir) = (self.db_path.clone(), self.dir.clone());
        let kept = tokio::task::spawn_blocking(move || keep_file(&db, &path, &dir))
            .await
            .map_err(|_| err(ErrorCode::Internal, "the check failed"))??;
        let (target, name, facts) = kept;
        self.handover
            .record_received(target, name, facts, &branch_name)
            .await
            .map_err(app_err)
    }

    async fn import_inner(&self) -> Result<MigrationImporting, UiError> {
        let _one = self.busy.lock().await;
        {
            let s = self.status.borrow();
            if s.migration_wait.is_none() {
                return Err(err(
                    ErrorCode::StepNotReady,
                    "This PC is not waiting for an Android Hub.",
                ));
            }
            if !s.restore_key_released {
                return Err(err(
                    ErrorCode::StepNotReady,
                    "Waiting for the owner to release the branch key to this PC.",
                ));
            }
        }
        let Some((file, facts)) = self.handover.received().await else {
            return Err(err(
                ErrorCode::StepNotReady,
                "This PC does not hold the tablet's snapshot yet.",
            ));
        };
        let token = self.token().await?;
        let claimed = self
            .cloud
            .migration_claim(
                &token,
                &MigrationClaimRequest {
                    instance_id: self.hub_instance_id.clone(),
                    fork: FenceFork {
                        parent_epoch: facts.epoch.clone(),
                        parent_seq: facts.last_seq,
                    },
                },
            )
            .await
            .map_err(cloud_err)?;
        let (db, epoch) = (self.db_path.clone(), claimed.epoch.clone());
        tokio::task::spawn_blocking(move || arena_store::stage_migration(&db, &file, &epoch))
            .await
            .map_err(|_| err(ErrorCode::Internal, "the import failed"))?
            .map_err(app_err)?;
        tracing::warn!(
            epoch = %claimed.epoch,
            last_seq = facts.last_seq,
            "migration: the snapshot is staged; the hub is restarting onto it"
        );
        let restart = Arc::clone(&self.restart);
        tokio::spawn(async move {
            tokio::time::sleep(RESTART_DELAY).await;
            restart();
        });
        Ok(MigrationImporting { restarting: true })
    }
}

impl MigrationImportControl for MigrationImporter {
    fn use_file(&self, req: HandoverFileRequest) -> JoinFuture<'_> {
        Box::pin(self.use_file_inner(req))
    }

    fn import(&self) -> ImportFuture<'_> {
        Box::pin(self.import_inner())
    }
}

/// Checks a snapshot file on this PC and keeps a copy beside the pulled ones. Blocking.
fn keep_file(
    db_path: &Path,
    path: &Path,
    dir: &Path,
) -> Result<(PathBuf, String, HandoverSnapshotFacts), UiError> {
    let found = arena_store::inspect_export(db_path, path).map_err(app_err)?;
    let header = found.header;
    if header.purpose.as_deref() != Some(HANDOVER_PURPOSE) {
        return Err(err(
            ErrorCode::RestoreCorrupt,
            "That file was not made for a move. Use \"Use a file instead\" on the tablet.",
        ));
    }
    let sha = header.sha256.to_ascii_lowercase();
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err(ErrorCode::RestoreCorrupt, "That file is damaged."));
    }
    let facts = HandoverSnapshotFacts {
        branch_id: header.branch_id,
        epoch: header.epoch,
        last_seq: header.last_seq,
        sha256: sha.clone(),
        bytes: found.file_bytes,
    };
    std::fs::create_dir_all(dir).map_err(|e| {
        err(
            ErrorCode::Internal,
            format!("could not make the folder: {e}"),
        )
    })?;
    let name = format!("handover-{}.a360", &sha[..16]);
    let target = dir.join(&name);
    std::fs::copy(path, &target).map_err(|e| {
        err(
            ErrorCode::Internal,
            format!("could not keep the snapshot: {e}"),
        )
    })?;
    Ok((target, name, facts))
}

/// Tells the cloud that the imported branch is serving, until it has been told (Story 10.18).
pub struct ImportReporter {
    cloud: CloudClient,
    token: Arc<dyn Keystore>,
    writer: WriterHandle,
    status: watch::Sender<HubStatus>,
    fails: u32,
}

impl ImportReporter {
    pub fn new(
        cloud: CloudClient,
        token: Arc<dyn Keystore>,
        writer: WriterHandle,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            cloud,
            token,
            writer,
            status,
            fails: 0,
        }
    }

    pub async fn run(mut self) {
        let mut status = self.status.subscribe();
        loop {
            let epoch = {
                let Ok(s) = status.wait_for(|s| s.migration_report.is_some()).await else {
                    return;
                };
                s.migration_report.clone()
            };
            let Some(epoch) = epoch else { continue };
            let wait = self.report(&epoch).await;
            if let Some(wait) = wait {
                tokio::time::sleep(wait).await;
            }
        }
    }

    /// One report. `None` when the cloud has it; else how long to wait before the next attempt.
    async fn report(&mut self, epoch: &str) -> Option<Duration> {
        let keystore = Arc::clone(&self.token);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) if !t.is_empty() => String::from_utf8_lossy(&t).into_owned(),
            _ => {
                tracing::warn!("migration: no hub token to report with");
                return Some(Duration::from_secs(30));
            }
        };
        let req = MigrationImportedRequest {
            epoch: epoch.to_owned(),
        };
        match self.cloud.migration_imported(&token, &req).await {
            Ok(_) => {
                if let Err(e) = self
                    .writer
                    .record_meta(MIGRATION_REPORT_KEY, String::new())
                    .await
                {
                    tracing::error!(code = e.code.as_str(), "migration: the report was not kept");
                    return Some(Duration::from_secs(30));
                }
                self.fails = 0;
                self.status.send_modify(|s| s.migration_report = None);
                tracing::info!(
                    epoch,
                    "migration: the cloud knows this Hub serves the branch"
                );
                None
            }
            Err(CloudFailure::Refused { error, .. }) => {
                match error.code {
                    ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => {
                        self.status
                            .send_modify(|s| s.pairing = PairingState::Revoked);
                        return Some(REVOKED_BACKOFF);
                    }
                    code => {
                        tracing::warn!(code = code.as_str(), "migration: the report was refused");
                    }
                }
                Some(self.failed())
            }
            Err(CloudFailure::Unreachable(_)) => Some(self.failed()),
        }
    }

    fn failed(&mut self) -> Duration {
        self.fails = self.fails.saturating_add(1);
        backoff(self.fails, RETRY_CEILING, unit()).max(Duration::from_secs(5))
    }
}

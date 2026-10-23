//! `arena-hub dev seed`: a dev-only fixture that bootstraps a branch (feature `dev-fixtures`,
//! absent from default and release builds). Epic 2 replaces it with real onboarding.
//!
//! It opens the store, takes the lease once as the Hub does at start (no renewal loop),
//! submits `branch.bootstrap` as `actor{system, "dev-seed"}` through the Writer, then releases
//! the lease (it expires now) so a real Hub can start on the same data dir at once.

use std::sync::Arc;

use arena_core::{Clock, HubStatus, load_or_mint_instance_id, status_channel};
use arena_proto::{
    Actor, ActorKind, AppError, BranchBootstrap, ErrorCode, Intent, IntentKind, IntentPayload,
};
use arena_store::Store;
use arena_tasks::LeaseTask;

use crate::config::Config;

/// The fixture branch.
pub fn seed_payload() -> BranchBootstrap {
    BranchBootstrap {
        name: "Dev Branch".into(),
        branch_code: "DEV01".into(),
        currency: "INR".into(),
        tz: "Asia/Kolkata".into(),
        day_start_min: 360,
        fy_start_month: 4,
    }
}

/// Seeds the branch in `cfg.data_dir`. Returns the new branch's change count on success.
pub async fn seed(cfg: &Config, clock: Arc<dyn Clock>) -> Result<usize, AppError> {
    std::fs::create_dir_all(&cfg.data_dir)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("create data dir: {e}")))?;
    let mut store = Store::open(cfg.db_path(), clock.now_ms())?;
    let schema_version = store.schema_version()?;
    let instance_id = load_or_mint_instance_id(&mut store)?;
    let (status_tx, _status_rx) =
        status_channel(HubStatus::opened(schema_version, instance_id.clone()));
    let lease = LeaseTask::start(store, instance_id, Arc::clone(&clock), status_tx).await?;
    let Some(writer) = lease.writer() else {
        lease.release().await?;
        return Err(AppError::new(
            ErrorCode::NotPrimary,
            "another hub holds this branch database",
        ));
    };
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::BranchBootstrap,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: clock.now_ms(),
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: "dev-seed".into(),
        },
        elevated_by: None,
        payload: IntentPayload::BranchBootstrap(seed_payload()),
    };
    let result = writer.submit_intent(intent).await;
    let released = lease.release().await;
    let applied = result?;
    released?;
    Ok(applied.changes.len())
}

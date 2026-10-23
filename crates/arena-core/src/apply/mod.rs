//! `apply()`: the single mutation path. Every write to a branch table is an [`Intent`] run
//! through this fixed pipeline, by the Writer actor, in one `BEGIN IMMEDIATE` transaction:
//!
//! permission → epoch → ttl_dedupe → base_rev → validate → mutate → stamp → append_changes →
//! commit → emit
//!
//! [`apply`] runs the steps up to `append_changes` on the caller's transaction. The Writer then
//! commits that transaction and, only once it has committed, calls [`emit`]. No I/O other than
//! SQLite happens inside the transaction. Any failing step returns exactly one [`AppError`];
//! the Writer drops the transaction, which rolls every earlier step back.

mod changes;
mod guards;
mod mutate;
mod validate;

#[cfg(test)]
mod tests;

use arena_proto::{AppError, Intent};
use arena_store::{ChangeOp, StoreTx};
use tokio::sync::broadcast;

use crate::clock::Clock;
use crate::status::{Epoch, Role};

pub use validate::KNOWN_CURRENCIES;

/// What `apply()` needs besides the transaction and the intent.
pub struct ApplyCtx<'a> {
    pub clock: &'a dyn Clock,
    /// The epoch this writer believes it holds (`None` when it holds no lease).
    pub epoch: Option<Epoch>,
    pub role: Role,
    /// Test hook: fail right after `mutate`, to prove the rollback.
    #[cfg(test)]
    pub(crate) fail_after_mutate: bool,
}

impl<'a> ApplyCtx<'a> {
    pub fn new(clock: &'a dyn Clock, epoch: Option<Epoch>, role: Role) -> Self {
        Self {
            clock,
            epoch,
            role,
            #[cfg(test)]
            fail_after_mutate: false,
        }
    }
}

/// One change row written by an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMeta {
    pub epoch: String,
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
    pub op: ChangeOp,
}

/// The outcome of a committed intent, broadcast by `emit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub intent_id: String,
    pub changes: Vec<ChangeMeta>,
}

/// Runs the pipeline up to `append_changes` inside `tx`. The caller commits, then [`emit`]s.
pub fn apply(
    tx: &mut StoreTx<'_>,
    ctx: &ApplyCtx<'_>,
    intent: &Intent,
) -> Result<Applied, AppError> {
    let now = ctx.clock.now_ms();
    guards::permission(intent)?;
    let epoch = guards::epoch(tx, ctx)?;
    guards::ttl_dedupe(tx, intent, now)?;
    guards::base_rev(tx, intent)?;
    validate::validate(intent)?;
    let written = mutate::mutate(tx, intent)?;
    #[cfg(test)]
    if ctx.fail_after_mutate {
        return Err(AppError::new(
            arena_proto::ErrorCode::Internal,
            "injected failure after mutate",
        ));
    }
    mutate::stamp(tx, &written, now)?;
    let changes = changes::append_changes(tx, &epoch, intent, &written, now)?;
    Ok(Applied {
        intent_id: intent.intent_id.clone(),
        changes,
    })
}

/// The last step: publishes a committed intent. Having no subscribers is not an error.
pub fn emit(events: &broadcast::Sender<Applied>, applied: &Applied) {
    let _ = events.send(applied.clone());
}

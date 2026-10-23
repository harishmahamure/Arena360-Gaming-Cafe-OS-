//! `apply()`: the single mutation path. Every write to a branch table is an [`Intent`] run
//! through this fixed pipeline, by the Writer actor, in one `BEGIN IMMEDIATE` transaction:
//!
//! permission → epoch → ttl_dedupe → (load → authorize) → base_rev → validate → mutate → stamp
//! → append_changes → commit → emit
//!
//! **permission** is the intent kind's [`Requirement`](arena_proto::Requirement), checked
//! against the actor before anything else (`rbac::check`). **authorize** adds the rules that
//! depend on the target row (built-in roles are fixed; a Manager assigns only Counter and
//! Kitchen). Both fail `PERMISSION_DENIED`.
//!
//! **stamp** also gives every change row its `business_day` and `fy`, under the branch locale in
//! effect at that moment (`locale::resolve`); a tz or day-start change applies from the next
//! business day, and existing rows are never restamped.
//!
//! [`apply`] runs the steps up to `append_changes` on the caller's transaction. The Writer then
//! commits that transaction and, only once it has committed, calls [`emit`]. No I/O other than
//! SQLite happens inside the transaction. Any failing step returns exactly one [`AppError`];
//! the Writer drops the transaction, which rolls every earlier step back.
//!
//! ttl_dedupe may end the pipeline early with a stored outcome (a retried idempotency key);
//! nothing is written then, and the Writer does not emit. A success is recorded in
//! `applied_intents` right after append_changes, in the same transaction.

mod changes;
mod devices;
mod frame;
mod guards;
mod mutate;
mod read_grants;
pub(crate) mod seed_defaults;
mod settings_set;
mod staff_roles;
mod ttl_dedupe;
mod validate;

#[cfg(test)]
mod tests;

use arena_proto::{AppError, Intent};
use arena_store::{ChangeOp, StoreTx};
use tokio::sync::broadcast;

use crate::clock::Clock;
use crate::status::{Epoch, Role};

pub use devices::MAX_VERSION_LEN;
pub use frame::{is_ulid, validate_frame};
pub use settings_set::MAX_SETTING_VALUE_CHARS;
pub(crate) use ttl_dedupe::record_rejection;
pub use ttl_dedupe::{INTENT_TTL_MS, MAX_CLOCK_SKEW_MS, fingerprint};

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

/// The existing row an edit intent works on, read once before base_rev and passed to
/// base_rev, validate and mutate.
pub(crate) enum Target {
    /// The intent edits no existing row (`branch.bootstrap`).
    None,
    /// `settings.set`: the setting row, if it exists.
    Setting(Option<arena_store::SettingRef>),
    /// `device.register`: the branch the device joins, if it has been bootstrapped.
    NewDevice(Option<String>),
    /// `device.revoke`, `device.version_reported`: the device row, if it exists.
    Device(Option<arena_store::DeviceRow>),
    /// `staff.upsert`: the branch, the edited row, the roles involved and the login's holder.
    Staff(Box<staff_roles::StaffTarget>),
    /// `role.upsert`: the branch, the edited role and its rows, and the name's holder.
    Role(Box<staff_roles::RoleTarget>),
    /// `audit.read_granted`: the branch, if it has been bootstrapped.
    Branch(Option<String>),
}

fn load(tx: &StoreTx<'_>, intent: &Intent) -> Result<Target, AppError> {
    use arena_proto::IntentPayload as P;
    Ok(match &intent.payload {
        P::BranchBootstrap(_) | P::BranchSeedDefaults(_) => Target::None,
        P::SettingsSet(s) => Target::Setting(settings_set::load(tx, s)?),
        P::DeviceRegister(_) => devices::load_register(tx)?,
        P::DeviceRevoke(d) => devices::load_device(tx, &d.device_id)?,
        P::DeviceVersionReported(d) => devices::load_device(tx, &d.device_id)?,
        P::StaffUpsert(s) => Target::Staff(Box::new(staff_roles::load_staff(tx, intent, s)?)),
        P::RoleUpsert(r) => Target::Role(Box::new(staff_roles::load_role(tx, intent, r)?)),
        P::AuditReadGranted(_) => read_grants::load(tx)?,
    })
}

/// What [`run`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The intent ran now: commit, then emit.
    Fresh(Applied),
    /// The idempotency key was already recorded: its stored outcome, returned unchanged.
    /// Nothing was written and nothing is emitted.
    Replayed(Result<Applied, AppError>),
}

/// Runs the pipeline up to `append_changes` inside `tx`. The caller commits, then [`emit`]s.
/// A retried idempotency key returns its stored outcome.
pub fn apply(
    tx: &mut StoreTx<'_>,
    ctx: &ApplyCtx<'_>,
    intent: &Intent,
) -> Result<Applied, AppError> {
    match run(tx, ctx, intent)? {
        Outcome::Fresh(applied) => Ok(applied),
        Outcome::Replayed(stored) => stored,
    }
}

/// Like [`apply`], but tells a fresh run apart from a dedupe hit (the Writer emits only the
/// former).
pub(crate) fn run(
    tx: &mut StoreTx<'_>,
    ctx: &ApplyCtx<'_>,
    intent: &Intent,
) -> Result<Outcome, AppError> {
    let now = ctx.clock.now_ms();
    guards::permission(tx, intent)?;
    let epoch = guards::epoch(tx, ctx)?;
    if let Some(stored) = ttl_dedupe::ttl_dedupe(tx, intent, now)? {
        return Ok(Outcome::Replayed(stored));
    }
    let target = load(tx, intent)?;
    guards::authorize(intent, &target)?;
    guards::base_rev(intent, &target)?;
    validate::validate(intent, &target)?;
    let written = mutate::mutate(tx, intent, &target, now)?;
    #[cfg(test)]
    if ctx.fail_after_mutate {
        return Err(AppError::new(
            arena_proto::ErrorCode::Internal,
            "injected failure after mutate",
        ));
    }
    mutate::stamp(tx, &written, now)?;
    let day = crate::locale::stamp(tx, now)?;
    let changes = changes::append_changes(tx, &epoch, intent, &written, now, day.as_ref())?;
    let applied = Applied {
        intent_id: intent.intent_id.clone(),
        changes,
    };
    ttl_dedupe::record_success(tx, intent, &applied, now)?;
    Ok(Outcome::Fresh(applied))
}

/// The epoch step on its own, for maintenance writes that must run only while primary.
pub(crate) fn check_epoch(tx: &StoreTx<'_>, ctx: &ApplyCtx<'_>) -> Result<(), AppError> {
    guards::epoch(tx, ctx).map(|_| ())
}

/// The last step: publishes a committed intent. Having no subscribers is not an error.
pub fn emit(events: &broadcast::Sender<Applied>, applied: &Applied) {
    let _ = events.send(applied.clone());
}

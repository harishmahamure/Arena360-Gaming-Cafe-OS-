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

mod ad;
mod admin_exit;
mod ads_consent;
mod alert;
mod branch_code;
mod business_invoice;
mod changes;
mod controller_check;
mod credit;
mod credit_collect;
pub(crate) mod credit_pause;
mod devices;
mod extend;
mod force_end;
mod frame;
mod group_bill;
mod groups;
mod guards;
mod invoice_issue;
mod kiosk_log;
mod licence;
mod maintenance;
mod mutate;
mod order_line;
mod order_place;
mod pc_command;
mod plan_sales;
mod plans;
mod players;
mod pos_sale;
mod prepaid;
mod printer_status;
mod products;
pub(crate) mod rate_flip;
mod rates;
mod read_grants;
mod receipt_notify;
mod receipt_print;
mod rules;
pub(crate) mod seed_defaults;
mod session;
pub(crate) mod session_expire;
mod settings_set;
mod shift;
mod staff_credentials;
mod staff_deactivate;
mod staff_roles;
pub(crate) mod stock;
mod test_session;
mod transfer;
mod ttl_dedupe;
mod validate;
pub(crate) mod verifier;
mod wallet;
mod zones;

#[cfg(test)]
mod tests;

use arena_proto::{AppError, Intent};
use arena_store::{ChangeOp, StoreTx};
use tokio::sync::broadcast;

use crate::clock::Clock;
use crate::status::{Epoch, Role};

pub use branch_code::lock_branch_code;
pub use devices::MAX_VERSION_LEN;
pub use frame::{is_ulid, validate_frame};
pub use kiosk_log::{MAX_KIOSK_LOG_CODE_LEN, MAX_KIOSK_LOG_DETAIL_CHARS, PROCESS_BLOCKED_CODE};
pub(crate) use licence::pending as licence_pending;
pub use mutate::Written;
pub use settings_set::MAX_SETTING_VALUE_CHARS;
pub use stock::decrement;
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
    /// `settings.set`, `ads.consent`: the setting row, if it exists.
    Setting(Option<arena_store::SettingRef>),
    /// `device.register`: the branch the device joins, if it has been bootstrapped.
    NewDevice(Box<devices::NewDev>),
    RegisterEdit(Box<devices::RegisterEdit>),
    /// `device.revoke`, `device.version_reported`: the device row, if it exists.
    Device(Option<arena_store::DeviceRow>),
    /// `staff.upsert`: the branch, the edited row, the roles involved and the login's holder.
    Staff(Box<staff_roles::StaffTarget>),
    /// `staff.deactivate`: the member, whether the actor is an Owner, the Owner count.
    Deactivate(Box<staff_deactivate::DeactivateTarget>),
    /// `role.upsert`: the branch, the edited role and its rows, and the name's holder.
    Role(Box<staff_roles::RoleTarget>),
    /// `audit.read_granted`, `kiosk.log`, `kiosk.process_blocked`: the branch, if it has been
    /// bootstrapped.
    Branch(Option<String>),
    /// `device.admin_exit`: the branch and whether the exit id is taken.
    AdminExit(Box<admin_exit::ExitTarget>),
    /// `device.admin_relock`: the exit row, if it exists.
    AdminRelock(Option<arena_store::AdminExitRow>),
    /// `device.admin_unlock`: the branch and the device.
    AdminUnlock(Box<admin_exit::UnlockTarget>),
    Group(Box<groups::GroupTarget>),
    Zone(Box<zones::ZoneTarget>),
    Product(Box<products::ProductTarget>),
    Credentials(Option<arena_store::StaffRow>),
    Stock(Box<stock::StockTarget>),
    Maintenance(Box<maintenance::MaintenanceTarget>),
    Rate(Box<rates::RateTarget>),
    Plan(Box<plans::PlanTarget>),
    Rule(Box<rules::RuleTarget>),
    TestSession(Box<test_session::SessionTarget>),
    Pc(Box<pc_command::PcTarget>),
    Player(Box<players::PlayerTarget>),
    Session(Box<session::SessionTarget>),
    SessionEnd(Box<session::EndTarget>),
    SessionSettle(Box<session::SettleTarget>),
    SessionEndReverted(Box<session::EndRevertTarget>),
    SessionForceEnd(Box<force_end::ForceEndTarget>),
    SessionForceEndReverted(Box<force_end::ForceEndRevertTarget>),
    SessionSettleReverted(Box<session::SettleRevertTarget>),
    SessionPause(Box<session::PauseTarget>),
    SessionResume(Box<session::PauseTarget>),
    SessionExtend(Box<extend::ExtendTarget>),
    SessionTransfer(Box<transfer::TransferTarget>),
    SessionGroup(Box<group_bill::GroupTarget>),
    GroupPayer(Box<group_bill::PayerTarget>),
    SessionAssign(Box<session::AssignTarget>),
    WalletTopup(Box<wallet::TopupTarget>),
    WalletTopupReverted(Box<wallet::TopupRevertTarget>),
    PlanSell(Box<plan_sales::SellTarget>),
    PlanSellReverted(Box<plan_sales::SellRevertTarget>),
    PosSale(Box<pos_sale::SaleTarget>),
    PosSaleReverted(Box<pos_sale::RevertTarget>),
    OrderPlace(Box<order_place::PlaceTarget>),
    OrderPlaceReverted(Box<order_place::RevertTarget>),
    OrderLineServed(Box<order_line::LineTarget>),
    OrderLineCancel(Box<order_line::LineTarget>),
    CreditSetLimit(Box<credit::LimitTarget>),
    CreditCollect(Box<credit_collect::CollectTarget>),
    CreditCollectReverted(Box<credit_collect::CollectRevertTarget>),
    CreditAutoPause(Box<credit_pause::PauseTarget>),
    SessionExpire(Box<session_expire::ExpireTarget>),
    SessionRateFlip(Box<rate_flip::FlipTarget>),
    InvoiceIssue(Box<invoice_issue::IssueTarget>),
    Receipt(Box<receipt_print::ReceiptTarget>),
    Alert(Box<alert::AlertTarget>),
    ControllerCheck(Box<controller_check::CheckTarget>),
    Shift(Box<shift::ShiftTarget>),
}

fn load(tx: &StoreTx<'_>, intent: &Intent, now: i64) -> Result<Target, AppError> {
    use arena_proto::IntentPayload as P;
    Ok(match &intent.payload {
        P::BranchBootstrap(_) | P::BranchSeedDefaults(_) => Target::None,
        P::SettingsSet(s) => Target::Setting(settings_set::load(tx, s)?),
        P::AdsConsent(_) => Target::Setting(ads_consent::load(tx)?),
        P::PrinterStatus(_) => Target::Setting(printer_status::load(tx)?),
        P::DeviceRegister(d) => devices::load_register(tx, d)?,
        P::DeviceRevoke(d) => devices::load_device(tx, &d.device_id)?,
        P::DeviceVersionReported(d) => devices::load_device(tx, &d.device_id)?,
        P::StaffUpsert(s) => Target::Staff(Box::new(staff_roles::load_staff(tx, intent, s)?)),
        P::RoleUpsert(r) => Target::Role(Box::new(staff_roles::load_role(tx, intent, r)?)),
        P::AuditReadGranted(_) => read_grants::load(tx)?,
        P::LicenceApply(_) => kiosk_log::load(tx)?,
        P::AdImpressionRecorded(_) | P::AdRequestRecorded(_) => ad::load(tx)?,
        P::KioskLog(_) | P::KioskProcessBlocked(_) => kiosk_log::load(tx)?,
        P::DeviceAdminExit(p) => admin_exit::load_exit(tx, p)?,
        P::DeviceAdminRelock(p) => admin_exit::load_relock(tx, p)?,
        P::DeviceAdminUnlock(p) => admin_exit::load_unlock(tx, p)?,
        P::GroupUpsert(_) | P::GroupDelete(_) | P::DeviceAssign(_) => {
            Target::Group(Box::new(groups::load(tx, &intent.payload)?))
        }
        P::DeviceMaintenance(_) => {
            Target::Maintenance(Box::new(maintenance::load(tx, &intent.payload)?))
        }
        P::RateUpsert(_) => Target::Rate(Box::new(rates::load(tx, &intent.payload)?)),
        P::PlanUpsert(_) => Target::Plan(Box::new(plans::load(tx, &intent.payload)?)),
        P::RuleUpsert(_) => Target::Rule(Box::new(rules::load(tx, &intent.payload)?)),
        P::DeviceTestSessionStart(_) | P::DeviceTestSessionCompleted(_) => {
            test_session::load(tx, &intent.payload)?
        }
        P::ZoneUpsert(_) | P::ZoneDelete(_) | P::DevicePlace(_) => {
            Target::Zone(Box::new(zones::load(tx, &intent.payload)?))
        }
        P::DeviceScreen(_) | P::DevicePower(_) => {
            Target::Pc(Box::new(pc_command::load(tx, &intent.payload)?))
        }
        P::StaffSetCredentials(c) => Target::Credentials(staff_credentials::load(tx, c)?),
        P::StaffDeactivate(d) => {
            Target::Deactivate(Box::new(staff_deactivate::load(tx, intent, d)?))
        }
        P::ProductUpsert(_) | P::ProductDelete(_) => {
            Target::Product(Box::new(products::load(tx, &intent.payload)?))
        }
        P::PlayerCreate(_) | P::PlayerUpdate(_) | P::PlayerPasswordReset(_) => {
            Target::Player(Box::new(players::load(tx, &intent.payload)?))
        }
        P::SessionStart(s) => Target::Session(Box::new(session::load(tx, s, now)?)),
        P::SessionEnd(e) => Target::SessionEnd(Box::new(session::load_end(tx, e)?)),
        P::SessionSettle(s) => Target::SessionSettle(Box::new(session::load_settle(tx, s, now)?)),
        P::SessionForceEnd(f) => Target::SessionForceEnd(Box::new(force_end::load(tx, f)?)),
        P::SessionForceEndReverted(r) => {
            Target::SessionForceEndReverted(Box::new(force_end::load_reverted(tx, r, now)?))
        }
        P::SessionEndReverted(r) => {
            Target::SessionEndReverted(Box::new(session::load_end_reverted(tx, r, now)?))
        }
        P::SessionSettleReverted(r) => {
            Target::SessionSettleReverted(Box::new(session::load_settle_reverted(tx, r, now)?))
        }
        P::SessionPause(p) => Target::SessionPause(Box::new(session::load_pause(tx, p)?)),
        P::SessionResume(r) => Target::SessionResume(Box::new(session::load_resume(tx, r)?)),
        P::SessionExtend(e) => Target::SessionExtend(Box::new(extend::load(tx, e, now)?)),
        P::SessionTransfer(t) => Target::SessionTransfer(Box::new(transfer::load(tx, t, now)?)),
        P::SessionGroup(g) => Target::SessionGroup(Box::new(group_bill::load(tx, g)?)),
        P::GroupPayer(p) => Target::GroupPayer(Box::new(group_bill::load_payer(tx, p)?)),
        P::SessionAssignPlayer(a) => Target::SessionAssign(Box::new(session::load_assign(tx, a)?)),
        P::WalletTopup(t) => Target::WalletTopup(Box::new(wallet::load_topup(tx, t)?)),
        P::WalletTopupReverted(r) => {
            Target::WalletTopupReverted(Box::new(wallet::load_topup_reverted(tx, r, now)?))
        }
        P::PlanSell(s) => Target::PlanSell(Box::new(plan_sales::load_sell(tx, s, now)?)),
        P::PlanSellReverted(r) => {
            Target::PlanSellReverted(Box::new(plan_sales::load_sell_reverted(tx, r, now)?))
        }
        P::PosSale(s) => Target::PosSale(Box::new(pos_sale::load_sale(tx, s, now)?)),
        P::PosSaleReverted(r) => {
            Target::PosSaleReverted(Box::new(pos_sale::load_sale_reverted(tx, r, now)?))
        }
        P::OrderPlace(p) => Target::OrderPlace(Box::new(order_place::load_place(tx, p, now)?)),
        P::OrderPlaceReverted(r) => {
            Target::OrderPlaceReverted(Box::new(order_place::load_place_reverted(tx, r, now)?))
        }
        P::OrderLineServed(s) => {
            Target::OrderLineServed(Box::new(order_line::load_line(tx, &s.line_id, now)?))
        }
        P::OrderLineCancel(c) => {
            Target::OrderLineCancel(Box::new(order_line::load_line(tx, &c.line_id, now)?))
        }
        P::CreditSetLimit(c) => Target::CreditSetLimit(Box::new(credit::load_set_limit(tx, c)?)),
        P::CreditCollect(c) => {
            Target::CreditCollect(Box::new(credit_collect::load_collect(tx, c)?))
        }
        P::CreditCollectReverted(r) => Target::CreditCollectReverted(Box::new(
            credit_collect::load_collect_reverted(tx, r, now)?,
        )),
        P::CreditAutoPause(p) => Target::CreditAutoPause(Box::new(credit_pause::load(tx, p, now)?)),
        P::SessionExpire(e) => Target::SessionExpire(Box::new(session_expire::load(tx, e)?)),
        P::SessionRateFlip(f) => Target::SessionRateFlip(Box::new(rate_flip::load(tx, f, now)?)),
        P::InvoiceIssue(p) => Target::InvoiceIssue(Box::new(invoice_issue::load(tx, p, now)?)),
        P::ReceiptReprint(_)
        | P::ReceiptPrinted(_)
        | P::ReceiptPrintFailed(_)
        | P::DrawerKicked(_)
        | P::DrawerKickFailed(_) => {
            Target::Receipt(Box::new(receipt_print::load(tx, &intent.payload)?))
        }
        P::AlertRaise(_) | P::AlertAck(_) => {
            Target::Alert(Box::new(alert::load(tx, &intent.payload)?))
        }
        P::ControllerCheck(c) => Target::ControllerCheck(Box::new(controller_check::load(tx, c)?)),
        P::ShiftOpen(_) => Target::Shift(Box::new(shift::load(tx)?)),
        P::StockIn(_)
        | P::StockInReverted(_)
        | P::StockWaste(_)
        | P::StockWasteReverted(_)
        | P::StockWriteOff(_)
        | P::StockExpiryCheck(_)
        | P::StockThreshold(_) => Target::Stock(Box::new(stock::load(tx, &intent.payload, now)?)),
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
    guards::tier(tx, intent, arena_proto::kitchen_gate)?;
    guards::paid_tier(tx, intent)?;
    let target = load(tx, intent, now)?;
    guards::authorize(intent, &target)?;
    guards::base_rev(intent, &target)?;
    validate::validate(intent, &target)?;
    let written = mutate::mutate(tx, intent, &target, &epoch, now)?;
    // A change to staff or roles may change who can unlock a kiosk offline.
    if matches!(
        intent.kind,
        arena_proto::IntentKind::BranchBootstrap
            | arena_proto::IntentKind::BranchSeedDefaults
            | arena_proto::IntentKind::StaffUpsert
            | arena_proto::IntentKind::StaffSetCredentials
            | arena_proto::IntentKind::StaffDeactivate
            | arena_proto::IntentKind::RoleUpsert
    ) {
        verifier::refresh(tx, now)?;
    }
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

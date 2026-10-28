//! The steps before any write: permission, epoch and base_rev (ttl_dedupe is in
//! `ttl_dedupe.rs`).

use arena_proto::{
    ActorKind, AppError, ElevatedBy, ElevationKind, ElevationMode, ErrorCode, FLAG_KITCHEN, Intent,
    IntentPayload, Requirement, TierGate,
};
use arena_store::{StoreTx, find_device, find_group_by_name, find_staff, read_lease};

use super::{
    ApplyCtx, Target, admin_exit, coupon, devices, groups, maintenance, plans, players, products,
    rules, settings_set, staff_roles, zones,
};
use crate::rbac;
use crate::staff::pin_required;
use crate::status::{Epoch, Role};

fn refused(message: &str) -> AppError {
    AppError::new(ErrorCode::PermissionDenied, message)
}

/// **permission.** Runs first, before any write: the actor must have an id, a device actor's
/// row must exist and not be revoked, and then the kind's requirement must hold — against the
/// actor, or, when the intent is elevated, against the elevating staff member
/// ([`elevation`]). Every failure is `PERMISSION_DENIED`, except a PIN that changed since it was
/// checked (`PIN_REQUIRED`).
pub(super) fn permission(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    if intent.actor.id.trim().is_empty() {
        return Err(refused("the actor has no id"));
    }
    // A device acts only while its row exists and is not revoked: this closes the window
    // between a revoke committing and the LAN fan-out closing the device's sessions.
    if intent.actor.kind == ActorKind::Device
        && find_device(tx, &intent.actor.id)?.is_none_or(|d| d.revoked_at.is_some())
    {
        return Err(refused("the device is revoked or unknown"));
    }
    // The kiosk's admin exit: the device acts on a manager's PIN, checked at the Hub or, only
    // here, against the cached verifier.
    if intent.payload.kind().elevator_permission().is_some() {
        return admin_exit::permission(tx, intent);
    }
    match &intent.elevated_by {
        Some(e) => elevation(tx, intent, e)?,
        None => rbac::check(tx, intent)?,
    }
    creating_player(tx, intent)?;
    overriding_credit(tx, intent)?;
    manual_discount(tx, intent)?;
    overriding_stock(tx, intent)?;
    importing_stock(tx, intent)?;
    importing_setup(tx, intent)?;
    importing_players(tx, intent)
}

/// A setup file that creates device groups needs `device.manage` on top of `catalog.edit`: the
/// actor's own, or the elevating manager's.
fn importing_setup(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let IntentPayload::ImportSetup(i) = &intent.payload else {
        return Ok(());
    };
    if intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let mut creates = false;
    for g in &i.file.groups {
        if find_group_by_name(tx, g.name.trim())?.is_none() {
            creates = true;
            break;
        }
    }
    if !creates {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::DeviceManage)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

/// A product import that books opening stock needs `stock.in` on top of `catalog.edit`: the
/// actor's own, or the elevating manager's.
fn importing_stock(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let IntentPayload::ImportProducts(i) = &intent.payload else {
        return Ok(());
    };
    if !i.rows.iter().any(|r| r.opening_qty > 0) || intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::StockIn)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

/// A stock override at the counter needs `stock.override` on top of the sale: the actor's own,
/// or the elevating manager's.
fn overriding_stock(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let over = match &intent.payload {
        IntentPayload::PosSale(s) => s.stock_override,
        IntentPayload::OrderPlace(p) => p.stock_override,
        _ => return Ok(()),
    };
    if over != Some(true) || intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::StockOverride)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

/// A manual discount (Story 5.22) needs `discount.manual` on top of the settle: the actor's own,
/// or the elevating manager's.
fn manual_discount(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let given = match &intent.payload {
        IntentPayload::SessionSettle(s) => s.discount.is_some(),
        IntentPayload::PosSale(s) => s.discount.is_some(),
        IntentPayload::PlanSell(s) => s.discount.is_some(),
        _ => return Ok(()),
    };
    if !given || intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::DiscountManual)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

/// A credit override needs `credit.manage` on top of the settle: the actor's own, or the
/// elevating manager's.
fn overriding_credit(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let credit_override = match &intent.payload {
        IntentPayload::SessionSettle(s) => s.credit_override,
        IntentPayload::PosSale(s) => s.credit_override,
        IntentPayload::SessionExtend(e) if e.source == arena_proto::ExtendSource::Credit => {
            e.credit_override
        }
        _ => return Ok(()),
    };
    if credit_override != Some(true) || intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::CreditManage)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

/// A conversion that also creates the profile needs `player.manage` on top of the kind's
/// requirement: the actor's own, or the elevating manager's.
fn creating_player(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let IntentPayload::SessionAssignPlayer(a) = &intent.payload else {
        return Ok(());
    };
    if a.create.is_none() || intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    if rbac::staff_allows(tx, holder, arena_proto::Permission::PlayerManage)? {
        Ok(())
    } else {
        Err(rbac::denied())
    }
}

pub(super) fn tier(
    tx: &StoreTx<'_>,
    intent: &Intent,
    gate: impl Fn(&IntentPayload) -> Option<TierGate>,
) -> Result<(), AppError> {
    if gate(&intent.payload) != Some(TierGate::New) {
        return Ok(());
    }
    if crate::entitlement::has_flag(tx, FLAG_KITCHEN)? {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::TierLocked,
        "available on Basic and above",
    ))
}

/// The paid-tier gate (Story 5.15): a `New` intent in [`arena_proto::notify_gate`] is refused
/// with `TIER_LOCKED` while the effective tier is Free. Values stay stored across a lapse.
pub(super) fn paid_tier(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    if arena_proto::notify_gate(&intent.payload) != Some(TierGate::New)
        || crate::entitlement::is_paid(tx)?
    {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::TierLocked,
        "available on Basic and above",
    ))
}

/// The permission step of an elevated intent. Only the Hub's ticket path sets `elevated_by`
/// (every edge strips a client's), and this re-checks everything the ticket was issued on:
///
/// - the elevation is a PIN verified at the Hub (`mfa` is a later story; `cached_verifier` is
///   accepted only for `device.admin_exit`, which never comes here);
/// - the actor is an active staff member, and the elevating member is someone else;
/// - the kind's requirement is a permission, and the elevating member is active and holds it;
/// - the elevating member's `pin_verifier_version` is still the one the PIN was checked
///   against, else `PIN_REQUIRED` (the PIN rotated: enter it again).
fn elevation(tx: &StoreTx<'_>, intent: &Intent, e: &ElevatedBy) -> Result<(), AppError> {
    let Requirement::Perm(p) = intent.payload.kind().requirement() else {
        return Err(rbac::denied());
    };
    if e.kind != ElevationKind::Pin || e.mode != ElevationMode::Hub {
        return Err(rbac::denied());
    }
    let system = rbac::is_pairing(intent);
    if !system {
        if intent.actor.kind != ActorKind::Staff || intent.actor.id == e.id {
            return Err(rbac::denied());
        }
        if find_staff(tx, &intent.actor.id)?.is_none_or(|s| !s.active) {
            return Err(rbac::denied());
        }
    }
    if !rbac::staff_allows(tx, &e.id, p)? {
        return Err(rbac::denied());
    }
    match find_staff(tx, &e.id)? {
        Some(s) if s.pin_hash.is_some() && s.pin_verifier_version == e.verifier_version => Ok(()),
        _ => Err(pin_required()),
    }
}

/// **authorize.** The permission rules that depend on the loaded target.
pub(super) fn authorize(intent: &Intent, target: &Target) -> Result<(), AppError> {
    match (target, &intent.payload) {
        (Target::Staff(t), _) => staff_roles::authorize_staff(t),
        (Target::Deactivate(t), _) => super::staff_deactivate::authorize(t),
        (Target::Role(t), arena_proto::IntentPayload::RoleUpsert(r)) => {
            staff_roles::authorize_role(r, t)
        }
        (Target::Role(_), _) => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
        (Target::AdminRelock(row), _) => admin_exit::authorize_relock(intent, row.as_ref()),
        (Target::TestSession(_), _) => super::test_session::authorize(intent),
        (
            Target::None
            | Target::Setting(_)
            | Target::NewDevice(_)
            | Target::RegisterEdit(_)
            | Target::Device(_)
            | Target::Branch(_)
            | Target::KioskScan(_)
            | Target::AdminExit(_)
            | Target::AdminUnlock(_)
            | Target::Group(_)
            | Target::Zone(_)
            | Target::Library(_)
            | Target::Product(_)
            | Target::Import(_)
            | Target::ImportPlayers(_)
            | Target::ImportPlans(_)
            | Target::ImportSetup(_)
            | Target::Credentials(_)
            | Target::Stock(_)
            | Target::Maintenance(_)
            | Target::Rate(_)
            | Target::Plan(_)
            | Target::Rule(_)
            | Target::Coupon(_)
            | Target::Pc(_)
            | Target::Player(_)
            | Target::Session(_)
            | Target::SessionEnd(_)
            | Target::SessionSettle(_)
            | Target::SessionEndReverted(_)
            | Target::SessionForceEnd(_)
            | Target::SessionForceEndReverted(_)
            | Target::SessionSettleReverted(_)
            | Target::SessionPause(_)
            | Target::SessionResume(_)
            | Target::SessionExtend(_)
            | Target::SessionTransfer(_)
            | Target::SessionGroup(_)
            | Target::GroupPayer(_)
            | Target::SessionAssign(_)
            | Target::WalletTopup(_)
            | Target::WalletTopupReverted(_)
            | Target::PlanSell(_)
            | Target::PlanSellReverted(_)
            | Target::PosSale(_)
            | Target::PosSaleReverted(_)
            | Target::OrderPlace(_)
            | Target::OrderPlaceReverted(_)
            | Target::OrderLineServed(_)
            | Target::OrderLineCancel(_)
            | Target::OrderVoid(_)
            | Target::Refund(_)
            | Target::CreditSetLimit(_)
            | Target::CreditCollect(_)
            | Target::CreditCollectReverted(_)
            | Target::CreditAutoPause(_)
            | Target::SessionExpire(_)
            | Target::SessionRateFlip(_)
            | Target::InvoiceIssue(_)
            | Target::Receipt(_)
            | Target::Alert(_)
            | Target::ControllerCheck(_)
            | Target::Shift(_),
            _,
        ) => Ok(()),
    }
}

fn not_primary(message: &str) -> AppError {
    AppError::new(ErrorCode::NotPrimary, message)
}

/// **epoch.** Fails `NOT_PRIMARY` unless this writer is primary and the lease row, read inside
/// the transaction, still holds its epoch and has not expired. Returns that epoch.
pub(crate) fn epoch(tx: &StoreTx<'_>, ctx: &ApplyCtx<'_>) -> Result<Epoch, AppError> {
    if ctx.role != Role::Primary {
        return Err(not_primary("this hub is not the primary writer"));
    }
    let Some(own) = ctx.epoch.clone() else {
        return Err(not_primary("this hub holds no lease"));
    };
    match read_lease(tx)? {
        Some(row) if row.counter == own.counter && row.hub_instance_id == own.hub_instance_id => {
            if row.expires_at > ctx.clock.now_ms() {
                Ok(own)
            } else {
                Err(not_primary("this hub's lease has expired"))
            }
        }
        _ => Err(not_primary("the lease is held by another epoch")),
    }
}

/// **base_rev.** Edit intents carry the `rev` they were based on, and a mismatch fails
/// `STALE`. `branch.bootstrap` creates rows, so it has none to check (validate rejects one).
pub(super) fn base_rev(intent: &Intent, target: &Target) -> Result<(), AppError> {
    match target {
        Target::None => Ok(()),
        Target::Setting(_)
            if matches!(intent.payload, arena_proto::IntentPayload::PrinterStatus(_)) =>
        {
            Ok(())
        }
        Target::Setting(row) => settings_set::base_rev(intent, row.as_ref()),
        Target::NewDevice(_)
        | Target::Branch(_)
        | Target::KioskScan(_)
        | Target::AdminExit(_)
        | Target::AdminRelock(_)
        | Target::AdminUnlock(_)
        | Target::Credentials(_)
        | Target::Deactivate(_)
        | Target::Stock(_)
        | Target::Shift(_)
        | Target::TestSession(_)
        | Target::Pc(_)
        | Target::Session(_)
        | Target::Rate(_) => Ok(()),
        Target::SessionEnd(t) => super::session::base_rev_end(intent, t),
        Target::SessionSettle(t) => super::session::base_rev_settle(intent, t),
        Target::SessionEndReverted(t) => super::session::base_rev_end_reverted(intent, t),
        Target::SessionForceEnd(t) => super::force_end::base_rev(intent, t),
        Target::SessionForceEndReverted(t) => super::force_end::base_rev_reverted(intent, t),
        Target::SessionSettleReverted(t) => super::session::base_rev_settle_reverted(intent, t),
        Target::SessionPause(t) => super::session::base_rev_pause(intent, t, "session.pause"),
        Target::SessionResume(t) => super::session::base_rev_pause(intent, t, "session.resume"),
        Target::SessionExtend(_) => Ok(()),
        Target::SessionTransfer(t) => super::transfer::base_rev(intent, t),
        Target::SessionGroup(t) => super::group_bill::base_rev(intent, t),
        Target::GroupPayer(t) => super::group_bill::base_rev_payer(intent, t),
        Target::SessionAssign(t) => super::session::base_rev_assign(intent, t),
        Target::WalletTopup(_) => Ok(()),
        Target::WalletTopupReverted(t) => super::wallet::base_rev_topup_reverted(intent, t),
        Target::PlanSell(_)
        | Target::PosSale(_)
        | Target::OrderPlace(_)
        | Target::OrderLineServed(_)
        | Target::OrderLineCancel(_)
        | Target::OrderVoid(_)
        | Target::Refund(_) => Ok(()),
        Target::OrderPlaceReverted(t) => super::order_place::base_rev_place_reverted(intent, t),
        Target::PosSaleReverted(t) => super::pos_sale::base_rev_sale_reverted(intent, t),
        Target::PlanSellReverted(t) => super::plan_sales::base_rev_sell_reverted(intent, t),
        Target::CreditSetLimit(_) => Ok(()),
        Target::CreditCollect(_) => Ok(()),
        Target::CreditAutoPause(_) => Ok(()),
        Target::SessionExpire(_) => Ok(()),
        Target::SessionRateFlip(_) => Ok(()),
        Target::InvoiceIssue(_) | Target::Receipt(_) => Ok(()),
        Target::Alert(_) | Target::ControllerCheck(_) => Ok(()),
        Target::CreditCollectReverted(t) => {
            super::credit_collect::base_rev_collect_reverted(intent, t)
        }
        Target::Player(t) => players::base_rev(intent, t),
        Target::Device(row) => devices::base_rev(intent, row.as_ref()),
        Target::RegisterEdit(t) => devices::base_rev_edit(intent, t),
        Target::Staff(t) => staff_roles::base_rev_staff(intent, t),
        Target::Role(t) => staff_roles::base_rev_role(intent, t),
        Target::Group(t) => groups::base_rev(intent, t),
        Target::Zone(t) => zones::base_rev(intent, t),
        Target::Library(t) => super::library::base_rev(intent, t),
        Target::Product(t) => products::base_rev(intent, t),
        Target::Import(_)
        | Target::ImportPlayers(_)
        | Target::ImportPlans(_)
        | Target::ImportSetup(_) => Ok(()),
        Target::Plan(t) => plans::base_rev(intent, t),
        Target::Rule(t) => rules::base_rev(intent, t),
        Target::Coupon(t) => coupon::base_rev(intent, t),
        Target::Maintenance(t) => maintenance::base_rev(intent, t),
    }
}

/// A player import that brings balances needs `wallet.topup`, and one that sets credit limits
/// needs `credit.manage`, on top of `player.manage`: the actor's own, or the elevating manager's.
fn importing_players(tx: &StoreTx<'_>, intent: &Intent) -> Result<(), AppError> {
    let IntentPayload::ImportPlayers(i) = &intent.payload else {
        return Ok(());
    };
    if intent.actor.kind == ActorKind::Owner {
        return Ok(());
    }
    let holder = intent
        .elevated_by
        .as_ref()
        .map_or(intent.actor.id.as_str(), |e| e.id.as_str());
    let needs = [
        (
            i.rows.iter().any(|r| r.balance_minor > 0),
            arena_proto::Permission::WalletTopup,
        ),
        (
            i.rows.iter().any(|r| r.credit_limit_minor > 0),
            arena_proto::Permission::CreditManage,
        ),
    ];
    for (needed, permission) in needs {
        if needed && !rbac::staff_allows(tx, holder, permission)? {
            return Err(rbac::denied());
        }
    }
    Ok(())
}

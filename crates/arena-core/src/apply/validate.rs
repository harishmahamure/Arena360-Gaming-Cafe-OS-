//! **validate.** Payload checks; a failure is `VALIDATION_FAILED` naming the field.

use super::{
    Target, devices, groups, maintenance, plans, players, products, rates, rules, settings_set,
    staff_roles, stock, zones,
};
use arena_proto::{AppError, BranchBootstrap, ErrorCode, Intent, IntentPayload};

/// Longest branch name, in characters, after trimming.
pub const MAX_NAME_CHARS: usize = 80;
/// Longest branch code.
pub const MAX_BRANCH_CODE_LEN: usize = 16;

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn validate(intent: &Intent, target: &Target) -> Result<(), AppError> {
    if intent.kind != intent.payload.kind() {
        return Err(invalid(
            "kind",
            "the intent kind does not match its payload",
        ));
    }
    if intent.intent_id.parse::<ulid::Ulid>().is_err() {
        return Err(invalid("intent_id", "intent_id must be a ULID"));
    }
    validate_reason(intent)?;
    match &intent.payload {
        IntentPayload::BranchBootstrap(b) => {
            if intent.base_rev.is_some() {
                return Err(invalid("base_rev", "branch.bootstrap takes no base_rev"));
            }
            validate_bootstrap(b)
        }
        IntentPayload::BranchSeedDefaults(_) => {
            if intent.base_rev.is_some() {
                return Err(invalid(
                    "base_rev",
                    "branch.seed_defaults takes no base_rev",
                ));
            }
            Ok(())
        }
        IntentPayload::SettingsSet(s) => {
            let row = match target {
                Target::Setting(row) => row.as_ref(),
                _ => None,
            };
            settings_set::validate(s, row)
        }
        IntentPayload::AdsConsent(p) => match target {
            Target::Setting(row) => super::ads_consent::validate(intent, p, row.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::PrinterStatus(p) => match target {
            Target::Setting(row) => super::printer_status::validate(intent, p, row.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceRegister(d) => match target {
            Target::NewDevice(t) => devices::validate_register(intent, d, t),
            Target::RegisterEdit(t) => devices::validate_edit(intent, d, t),
            _ => devices::validate_register(intent, d, &devices::NewDev::default()),
        },
        IntentPayload::DeviceRevoke(d) => devices::validate_revoke(d, device_row(target)),
        IntentPayload::DeviceVersionReported(d) => devices::validate_version(d, device_row(target)),
        IntentPayload::StaffUpsert(s) => match target {
            Target::Staff(t) => staff_roles::validate_staff(intent, s, t),
            _ => Err(internal_target()),
        },
        IntentPayload::RoleUpsert(r) => match target {
            Target::Role(t) => staff_roles::validate_role(intent, r, t),
            _ => Err(internal_target()),
        },
        IntentPayload::LicenceApply(p) => match target {
            Target::Branch(b) => super::licence::validate(intent, p, b.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::AuditReadGranted(g) => match target {
            Target::Branch(b) => super::read_grants::validate(intent, g, b.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::KioskLog(l) => match target {
            Target::Branch(b) => {
                super::kiosk_log::validate(intent, super::kiosk_log::Event::Log(l), b.as_ref())
            }
            _ => Err(internal_target()),
        },
        IntentPayload::KioskProcessBlocked(p) => match target {
            Target::Branch(b) => super::kiosk_log::validate(
                intent,
                super::kiosk_log::Event::ProcessBlocked(p),
                b.as_ref(),
            ),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceAdminExit(p) => match target {
            Target::AdminExit(t) => super::admin_exit::validate_exit(intent, p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceAdminRelock(p) => match target {
            Target::AdminRelock(row) => super::admin_exit::validate_relock(intent, p, row.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceAdminUnlock(p) => match target {
            Target::AdminUnlock(t) => super::admin_exit::validate_unlock(intent, p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::GroupUpsert(g) => match target {
            Target::Group(t) => groups::validate_upsert(g, t),
            _ => Err(internal_target()),
        },
        IntentPayload::GroupDelete(_) => match target {
            Target::Group(t) => groups::validate_delete(t),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceAssign(a) => match target {
            Target::Group(t) => groups::validate_assign(a, t),
            _ => Err(internal_target()),
        },
        IntentPayload::ZoneUpsert(z) => match target {
            Target::Zone(t) => zones::validate_upsert(z, t),
            _ => Err(internal_target()),
        },
        IntentPayload::ZoneDelete(_) => match target {
            Target::Zone(t) => zones::validate_delete(t),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceScreen(_) | IntentPayload::DevicePower(_) => match target {
            Target::Pc(t) => super::pc_command::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::RateUpsert(r) => match target {
            Target::Rate(t) => rates::validate(r, t),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceMaintenance(m) => match target {
            Target::Maintenance(t) => maintenance::validate(m, t),
            _ => Err(internal_target()),
        },
        IntentPayload::DeviceTestSessionStart(_) | IntentPayload::DeviceTestSessionCompleted(_) => {
            match target {
                Target::TestSession(t) => super::test_session::validate(intent, t),
                _ => Err(internal_target()),
            }
        }
        IntentPayload::DevicePlace(d) => match target {
            Target::Zone(t) => zones::validate_place(d, t),
            _ => Err(internal_target()),
        },
        IntentPayload::StaffDeactivate(d) => match target {
            Target::Deactivate(t) => super::staff_deactivate::validate(intent, d, t),
            _ => Err(internal_target()),
        },
        IntentPayload::StaffSetCredentials(c) => match target {
            Target::Credentials(row) => super::staff_credentials::validate(c, row.as_ref()),
            _ => Err(internal_target()),
        },
        IntentPayload::ProductUpsert(u) => match target {
            Target::Product(t) => products::validate_upsert(u, t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlanUpsert(u) => match target {
            Target::Plan(t) => plans::validate(u, t),
            _ => Err(internal_target()),
        },
        IntentPayload::RuleUpsert(u) => match target {
            Target::Rule(t) => rules::validate(u, t),
            _ => Err(internal_target()),
        },
        IntentPayload::ProductDelete(_) => match target {
            Target::Product(t) => products::validate_delete(t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlayerCreate(p) => match target {
            Target::Player(t) => players::validate(p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlayerUpdate(p) => match target {
            Target::Player(t) => players::validate_update(p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlayerPasswordReset(p) => match target {
            Target::Player(t) => players::validate_reset(p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionStart(s) => match target {
            Target::Session(t) => super::session::validate(intent, s, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionEnd(_) => match target {
            Target::SessionEnd(t) => super::session::validate_end(t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionForceEnd(f) => match target {
            Target::SessionForceEnd(t) => super::force_end::validate(intent, f, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionForceEndReverted(_) => match target {
            Target::SessionForceEndReverted(t) => super::force_end::validate_reverted(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionSettle(s) => match target {
            Target::SessionSettle(t) => super::session::validate_settle(s, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionEndReverted(_) => match target {
            Target::SessionEndReverted(t) => super::session::validate_end_reverted(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionSettleReverted(_) => match target {
            Target::SessionSettleReverted(t) => super::session::validate_settle_reverted(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionPause(p) => match target {
            Target::SessionPause(t) => super::session::validate_pause(p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionResume(_) => match target {
            Target::SessionResume(t) => super::session::validate_resume(t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionExtend(e) => match target {
            Target::SessionExtend(t) => super::extend::validate(intent, e, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionGroup(g) => match target {
            Target::SessionGroup(t) => super::group_bill::validate(g, t),
            _ => Err(internal_target()),
        },
        IntentPayload::GroupPayer(p) => match target {
            Target::GroupPayer(t) => super::group_bill::validate_payer(p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionTransfer(_) => match target {
            Target::SessionTransfer(t) => super::transfer::validate(t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionAssignPlayer(a) => match target {
            Target::SessionAssign(t) => super::session::validate_assign(a, t),
            _ => Err(internal_target()),
        },
        IntentPayload::WalletTopup(p) => match target {
            Target::WalletTopup(t) => super::wallet::validate_topup(intent, p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::WalletTopupReverted(_) => match target {
            Target::WalletTopupReverted(t) => super::wallet::validate_topup_reverted(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlanSell(p) => match target {
            Target::PlanSell(t) => super::plan_sales::validate_sell(intent, p, t),
            _ => Err(internal_target()),
        },
        IntentPayload::PlanSellReverted(_) => match target {
            Target::PlanSellReverted(t) => super::plan_sales::validate_sell_reverted(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::CreditSetLimit(c) => match target {
            Target::CreditSetLimit(t) => super::credit::validate_set_limit(intent, c, t),
            _ => Err(internal_target()),
        },
        IntentPayload::CreditCollect(c) => match target {
            Target::CreditCollect(t) => super::credit_collect::validate_collect(intent, c, t),
            _ => Err(internal_target()),
        },
        IntentPayload::CreditAutoPause(_) => match target {
            Target::CreditAutoPause(t) => super::credit_pause::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::AlertRaise(_) | IntentPayload::AlertAck(_) => match target {
            Target::Alert(t) => super::alert::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::ControllerCheck(_) => match target {
            Target::ControllerCheck(t) => super::controller_check::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionExpire(_) => match target {
            Target::SessionExpire(t) => super::session_expire::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::SessionRateFlip(_) => match target {
            Target::SessionRateFlip(t) => super::rate_flip::validate(intent, t),
            _ => Err(internal_target()),
        },
        IntentPayload::CreditCollectReverted(_) => match target {
            Target::CreditCollectReverted(t) => {
                super::credit_collect::validate_collect_reverted(intent, t)
            }
            _ => Err(internal_target()),
        },
        IntentPayload::StockIn(u) => match target {
            Target::Stock(t) => stock::validate_in(u, t),
            _ => Err(internal_target()),
        },
        IntentPayload::StockInReverted(r) => match target {
            Target::Stock(t) => stock::validate_revert(intent, r, t),
            _ => Err(internal_target()),
        },
    }
}

/// A kind with reasons needs one of them (`REASON_REQUIRED` when absent); a reason the kind
/// does not list — including any reason on a kind that takes none — fails on `reason`.
fn validate_reason(intent: &Intent) -> Result<(), AppError> {
    match (intent.payload.kind().reasons(), intent.reason) {
        (Some(_), None) => Err(AppError::with_field(
            ErrorCode::ReasonRequired,
            "reason",
            "choose a reason",
        )),
        (Some(list), Some(r)) if !list.contains(&r) => {
            Err(invalid("reason", "this reason does not apply here"))
        }
        (None, Some(_)) => Err(invalid("reason", "this intent takes no reason")),
        _ => Ok(()),
    }
}

fn internal_target() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "the loaded target does not match the payload",
    )
}

fn device_row(target: &Target) -> Option<&arena_store::DeviceRow> {
    match target {
        Target::Device(row) => row.as_ref(),
        _ => None,
    }
}

/// A display label: 1–80 characters after trimming, with no control characters.
pub(super) fn validate_label(label: &str) -> Result<(), String> {
    let label = label.trim();
    let len = label.chars().count();
    if len == 0 || len > MAX_NAME_CHARS || label.chars().any(char::is_control) {
        Err(format!(
            "label must be 1–{MAX_NAME_CHARS} characters with no control characters"
        ))
    } else {
        Ok(())
    }
}

fn validate_bootstrap(b: &BranchBootstrap) -> Result<(), AppError> {
    let name = b.name.trim();
    let name_len = name.chars().count();
    if name_len == 0 || name_len > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(invalid(
            "name",
            "name must be 1–80 characters with no control characters",
        ));
    }
    let code_ok = (1..=MAX_BRANCH_CODE_LEN).contains(&b.branch_code.len())
        && b.branch_code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-');
    if !code_ok {
        return Err(invalid(
            "branch_code",
            "branch_code must be 1–16 ASCII letters, digits or '-'",
        ));
    }
    if !arena_proto::currency::is_known_currency(&b.currency) {
        return Err(invalid("currency", "currency is not a known ISO 4217 code"));
    }
    // An exact IANA name from the bundled tz database (e.g. `Asia/Kolkata`, `UTC`).
    if b.tz.parse::<chrono_tz::Tz>().is_err() {
        return Err(invalid(
            "tz",
            "tz must be an IANA zone such as Asia/Kolkata",
        ));
    }
    if b.day_start_min > 1439 {
        return Err(invalid("day_start_min", "day_start_min must be 0–1439"));
    }
    if !(1..=12).contains(&b.fy_start_month) {
        return Err(invalid("fy_start_month", "fy_start_month must be 1–12"));
    }
    b.onboarding
        .as_ref()
        .map_or(Ok(()), |o| validate_onboarding(b, o))
}

fn validate_onboarding(
    b: &BranchBootstrap,
    o: &arena_proto::BootstrapOnboarding,
) -> Result<(), AppError> {
    arena_proto::check_bootstrap(
        &b.branch_code,
        &o.country,
        o.gst_registered,
        &o.gstin,
        o.tax_mode,
        &o.owner_staff,
    )
    .map_err(|(field, message)| invalid(field, message))
}

pub(crate) fn default_tax() -> arena_proto::TaxSettings {
    arena_proto::TaxSettings::parse(arena_proto::DEFAULT_TAX).expect("DEFAULT_TAX parses")
}

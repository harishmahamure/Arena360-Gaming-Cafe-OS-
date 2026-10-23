//! **validate.** Payload checks; a failure is `VALIDATION_FAILED` naming the field.

use super::{Target, devices, settings_set, staff_roles};
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
        IntentPayload::DeviceRegister(d) => match target {
            Target::NewDevice(branch) => devices::validate_register(intent, d, branch.as_ref()),
            _ => devices::validate_register(intent, d, None),
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
    Ok(())
}

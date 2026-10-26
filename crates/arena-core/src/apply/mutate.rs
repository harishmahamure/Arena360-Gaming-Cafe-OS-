//! **mutate** and **stamp.**

use arena_proto::{AppError, ErrorCode, Intent, IntentPayload};
use arena_store::{
    ChangeOp, NewBranch, Row, RowKey, StaffWrite, StoreTx, branch_exists, insert_branch,
    insert_role, insert_role_perm, insert_setting, insert_staff, set_staff_setup_code, stamp_row,
};

use super::{
    Target, devices, groups, maintenance, players, products, rates, settings_set, staff_roles,
    stock, zones,
};
use crate::rbac::{BUILTIN_ROLES, builtin_perms};
use crate::settings_defaults::{DEFAULT_SETTINGS, bootstrap_locale_settings};

/// One row the intent wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub key: RowKey,
    pub op: ChangeOp,
    /// For a delete: the row as it was before the delete (read by mutate before deleting).
    pub before: Option<Row>,
}

/// **mutate.** Writes the intent's rows (with `rev = 0` and zero timestamps on insert; `stamp`
/// sets them) and returns them in write order.
pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    target: &Target,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    match &intent.payload {
        IntentPayload::BranchBootstrap(b) => {
            if branch_exists(tx)? {
                return Err(AppError::new(
                    ErrorCode::AlreadyBootstrapped,
                    "this database already has a branch",
                ));
            }
            let branch_id = ulid::Ulid::generate().to_string();
            insert_branch(
                tx,
                &NewBranch {
                    branch_id: &branch_id,
                    name: b.name.trim(),
                    branch_code: &b.branch_code,
                    currency: &b.currency,
                    tz: &b.tz,
                    day_start_min: b.day_start_min,
                    fy_start_month: b.fy_start_month,
                },
            )?;
            let mut written = vec![Written {
                key: RowKey::Branch {
                    branch_id: branch_id.clone(),
                },
                op: ChangeOp::Insert,
                before: None,
            }];
            let locale = bootstrap_locale_settings(b);
            let tax = b.onboarding.as_ref().map(|o| {
                arena_proto::TaxSettings {
                    country: o.country.clone(),
                    gst: o.gst_registered,
                    gstin: o.gstin.clone(),
                    ..super::validate::default_tax()
                }
                .to_json()
            });
            let seeded = DEFAULT_SETTINGS
                .iter()
                .map(|(k, v)| match (&tax, *k == arena_proto::KEY_TAX) {
                    (Some(t), true) => (*k, t.as_str()),
                    _ => (*k, *v),
                })
                .chain(locale.iter().map(|(k, v)| (*k, v.as_str())))
                .chain([
                    (arena_proto::KEY_BRANCH_NAME, b.name.as_str()),
                    (arena_proto::KEY_BRANCH_CODE, b.branch_code.as_str()),
                    (arena_proto::KEY_BRANCH_CODE_LOCKED_AT, ""),
                ]);
            for (key, value) in seeded {
                insert_setting(tx, &branch_id, key, value)?;
                written.push(Written {
                    key: RowKey::Settings {
                        branch_id: branch_id.clone(),
                        key: (*key).to_owned(),
                    },
                    op: ChangeOp::Insert,
                    before: None,
                });
            }
            // The built-in roles, each permission row an allow.
            for name in BUILTIN_ROLES {
                let role_id = ulid::Ulid::generate().to_string();
                insert_role(tx, &role_id, &branch_id, name, true)?;
                written.push(Written {
                    key: RowKey::Roles {
                        role_id: role_id.clone(),
                    },
                    op: ChangeOp::Insert,
                    before: None,
                });
                for p in builtin_perms(name) {
                    insert_role_perm(tx, &role_id, p.as_str(), "allow")?;
                    written.push(Written {
                        key: RowKey::RolePerms {
                            role_id: role_id.clone(),
                            permission: p.as_str().to_owned(),
                        },
                        op: ChangeOp::Insert,
                        before: None,
                    });
                }
                if name == crate::rbac::ROLE_OWNER
                    && let Some(o) = &b.onboarding
                {
                    let staff_id = ulid::Ulid::generate().to_string();
                    insert_staff(
                        tx,
                        &StaffWrite {
                            staff_id: &staff_id,
                            branch_id: &branch_id,
                            login: &o.owner_staff.login,
                            display_name: o.owner_staff.name.trim(),
                            role_id: &role_id,
                            password_hash: None,
                            pin_hash: None,
                            pin_verifier_version: 1,
                            active: true,
                        },
                    )?;
                    set_staff_setup_code(
                        tx,
                        &staff_id,
                        &o.owner_staff.setup_code_hash,
                        o.owner_staff.setup_expires_at,
                    )?;
                    written.push(Written {
                        key: RowKey::Staff { staff_id },
                        op: ChangeOp::Insert,
                        before: None,
                    });
                }
            }
            Ok(written)
        }
        IntentPayload::BranchSeedDefaults(_) => super::seed_defaults::mutate(tx),
        IntentPayload::SettingsSet(s) => {
            let row = match target {
                Target::Setting(row) => row.as_ref(),
                _ => None,
            };
            settings_set::mutate(tx, s, row, now_ms)
        }
        IntentPayload::AdsConsent(p) => match target {
            Target::Setting(row) => super::ads_consent::mutate(tx, intent, p, row.as_ref(), now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::PrinterStatus(p) => match target {
            Target::Setting(row) => super::printer_status::mutate(tx, intent, p, row.as_ref()),
            _ => Err(mismatch()),
        },
        IntentPayload::DeviceRegister(d) => match target {
            Target::RegisterEdit(t) => devices::mutate_edit(tx, d, t),
            Target::NewDevice(t) => devices::mutate_register(tx, d, t),
            _ => devices::mutate_register(tx, d, &devices::NewDev::default()),
        },
        IntentPayload::DeviceRevoke(_) => devices::mutate_revoke(tx, device_row(target), now_ms),
        IntentPayload::DeviceVersionReported(d) => {
            devices::mutate_version(tx, d, device_row(target))
        }
        IntentPayload::StaffUpsert(s) => match target {
            Target::Staff(t) => staff_roles::mutate_staff(tx, s, t),
            _ => Err(mismatch()),
        },
        IntentPayload::RoleUpsert(r) => match target {
            Target::Role(t) => staff_roles::mutate_role(tx, r, t),
            _ => Err(mismatch()),
        },
        IntentPayload::LicenceApply(p) => match target {
            Target::Branch(b) => super::licence::mutate(tx, p, b.as_ref()),
            _ => Err(mismatch()),
        },
        IntentPayload::AuditReadGranted(g) => match target {
            Target::Branch(b) => super::read_grants::mutate(tx, g, b.as_ref()),
            _ => Err(mismatch()),
        },
        IntentPayload::KioskLog(l) => match target {
            Target::Branch(b) => {
                super::kiosk_log::mutate(tx, intent, super::kiosk_log::Event::Log(l), b.as_ref())
            }
            _ => Err(mismatch()),
        },
        IntentPayload::KioskProcessBlocked(p) => match target {
            Target::Branch(b) => super::kiosk_log::mutate(
                tx,
                intent,
                super::kiosk_log::Event::ProcessBlocked(p),
                b.as_ref(),
            ),
            _ => Err(mismatch()),
        },
        IntentPayload::DeviceAdminExit(p) => match target {
            Target::AdminExit(t) => super::admin_exit::mutate_exit(tx, intent, p, t),
            _ => Err(mismatch()),
        },
        IntentPayload::DeviceAdminRelock(p) => super::admin_exit::mutate_relock(tx, intent, p),
        IntentPayload::DeviceAdminUnlock(p) => match target {
            Target::AdminUnlock(t) => super::admin_exit::mutate_unlock(tx, intent, p, t, now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::GroupUpsert(g) => match target {
            Target::Group(t) => groups::mutate_upsert(tx, g, t),
            _ => Err(mismatch()),
        },
        IntentPayload::GroupDelete(g) => groups::mutate_delete(tx, g, now_ms),
        IntentPayload::DeviceAssign(a) => groups::mutate_assign(tx, a),
        IntentPayload::ZoneUpsert(z) => match target {
            Target::Zone(t) => zones::mutate_upsert(tx, z, t),
            _ => Err(mismatch()),
        },
        IntentPayload::ZoneDelete(z) => zones::mutate_delete(tx, z, now_ms),
        IntentPayload::DevicePlace(d) => zones::mutate_place(tx, d),
        IntentPayload::StaffSetCredentials(c) => super::staff_credentials::mutate(tx, c),
        IntentPayload::ProductUpsert(u) => match target {
            Target::Product(t) => products::mutate_upsert(tx, u, t),
            _ => Err(mismatch()),
        },
        IntentPayload::ProductDelete(p) => match target {
            Target::Product(t) => products::mutate_delete(tx, p, t, now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::StockIn(u) => match target {
            Target::Stock(t) => stock::mutate_in(tx, intent, u, t, now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::StockInReverted(_) => match target {
            Target::Stock(t) => stock::mutate_revert(tx, intent, t, now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::DeviceScreen(_) | IntentPayload::DevicePower(_) => match target {
            Target::Pc(t) => super::pc_command::mutate(tx, intent, t, now_ms),
            _ => Err(mismatch()),
        },
        IntentPayload::RateUpsert(r) => match target {
            Target::Rate(t) => rates::mutate(tx, r, t),
            _ => Err(mismatch()),
        },
        IntentPayload::PlayerCreate(p) => players::mutate(tx, p, now_ms),
        IntentPayload::DeviceTestSessionStart(_) | IntentPayload::DeviceTestSessionCompleted(_) => {
            match target {
                Target::TestSession(t) => super::test_session::mutate(tx, intent, t),
                _ => Err(mismatch()),
            }
        }
        IntentPayload::DeviceMaintenance(m) => match target {
            Target::Maintenance(t) => maintenance::mutate(tx, m, t),
            _ => Err(mismatch()),
        },
    }
}

fn mismatch() -> AppError {
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

/// **stamp.** `rev` goes 0 → 1 on insert and +1 on update; `created_at` (insert) and
/// `updated_at` come from the clock. (The change rows' `business_day` and `fy` are stamped by
/// `locale::stamp`, from the locale in effect after this write.)
pub(super) fn stamp(tx: &StoreTx<'_>, written: &[Written], now_ms: i64) -> Result<(), AppError> {
    for w in written {
        match w.op {
            ChangeOp::Insert => stamp_row(tx, &w.key, now_ms, true)?,
            ChangeOp::Update => stamp_row(tx, &w.key, now_ms, false)?,
            ChangeOp::Delete => {}
        }
    }
    Ok(())
}

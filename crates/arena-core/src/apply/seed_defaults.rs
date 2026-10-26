//! `branch.seed_defaults`: brings an upgraded branch up to the current defaults. It inserts every
//! default setting the branch lacks (the `branch.*` locale keys take the branch row's bootstrap
//! columns; `kiosk.process_mode` is backfilled as `report`, not the bootstrap `enforce`), the
//! `display.hc.<device_id>` row (`false`) of every non-revoked paired device, every built-in role it lacks, and every permission row a built-in role lacks, as
//! ordinary change rows. Nothing that exists is changed. It is idempotent: with nothing missing
//! it fails `ALREADY_BOOTSTRAPPED` and writes nothing, and the Hub only submits it when
//! [`missing`] finds something.

use std::collections::BTreeSet;

use arena_proto::{
    AppError, ErrorCode, HC_DEFAULT, KEY_BRANCH_CODE, KEY_BRANCH_CODE_LOCKED_AT, KEY_BRANCH_NAME,
    hc_key, parse_hc_key,
};
use arena_store::{
    ChangeOp, RowKey, StoreTx, current_branch_id, find_branch_locale, find_branch_summary,
    insert_role, insert_role_perm, insert_setting, list_devices, list_role_perms, list_roles,
    list_settings,
};

use super::mutate::Written;
use crate::locale::branch_default;
use crate::rbac::{BUILTIN_ROLES, builtin_perms};
use crate::settings_defaults::{DEFAULT_SETTINGS, LOCALE_KEYS, backfill_value};

/// What an upgraded branch lacks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Missing {
    pub branch_id: String,
    /// `(key, value)` settings to insert.
    pub settings: Vec<(String, String)>,
    /// Built-in role names with no row.
    pub roles: Vec<&'static str>,
    /// `(role_id, permission)` allow rows a built-in role lacks.
    pub role_perms: Vec<(String, &'static str)>,
}

impl Missing {
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.roles.is_empty() && self.role_perms.is_empty()
    }
}

/// What the branch lacks, or `None` before bootstrap.
pub fn missing(tx: &StoreTx<'_>) -> Result<Option<Missing>, AppError> {
    let (Some(branch_id), Some(branch)) = (current_branch_id(tx)?, find_branch_locale(tx)?) else {
        return Ok(None);
    };
    let have: BTreeSet<String> = list_settings(tx)?.into_iter().map(|s| s.key).collect();
    let mut out = Missing {
        branch_id,
        ..Missing::default()
    };
    for (key, value) in DEFAULT_SETTINGS {
        if !have.contains(*key) {
            out.settings
                .push(((*key).to_owned(), backfill_value(key, value).to_owned()));
        }
    }
    if let Some(summary) = find_branch_summary(tx)? {
        for (key, value) in [
            (KEY_BRANCH_NAME, summary.name.as_str()),
            (KEY_BRANCH_CODE, summary.branch_code.as_str()),
            (KEY_BRANCH_CODE_LOCKED_AT, ""),
        ] {
            if !have.contains(key) {
                out.settings.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    for key in LOCALE_KEYS {
        if !have.contains(key) {
            out.settings
                .push((key.to_owned(), branch_default(&branch, key)));
        }
    }
    // Story 1.21: every non-revoked paired device that predates its high-contrast row.
    for device in list_devices(tx)? {
        let key = hc_key(&device.device_id);
        if device.class == "paired"
            && device.revoked_at.is_none()
            && parse_hc_key(&key).is_some()
            && !have.contains(&key)
        {
            out.settings.push((key, HC_DEFAULT.to_owned()));
        }
    }
    let roles = list_roles(tx)?;
    for name in BUILTIN_ROLES {
        match roles.iter().find(|r| r.name.eq_ignore_ascii_case(name)) {
            Some(role) if role.builtin => {
                let rows = list_role_perms(tx, &role.role_id)?;
                for p in builtin_perms(name) {
                    if !rows.iter().any(|(perm, _)| perm == p.as_str()) {
                        out.role_perms.push((role.role_id.clone(), p.as_str()));
                    }
                }
            }
            Some(_) => tracing::warn!(
                role = name,
                "seed defaults: a custom role holds a built-in role's name; not seeding it"
            ),
            None => out.roles.push(name),
        }
    }
    Ok(Some(out))
}

/// **mutate.** Inserts what [`missing`] found.
pub(super) fn mutate(tx: &StoreTx<'_>) -> Result<Vec<Written>, AppError> {
    let Some(m) = missing(tx)? else {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "branch",
            "there is no branch to seed",
        ));
    };
    if m.is_empty() {
        return Err(AppError::new(
            ErrorCode::AlreadyBootstrapped,
            "the branch already has every default row",
        ));
    }
    let insert = |key: RowKey| Written {
        key,
        op: ChangeOp::Insert,
        before: None,
    };
    let mut written = Vec::new();
    for (key, value) in &m.settings {
        insert_setting(tx, &m.branch_id, key, value)?;
        written.push(insert(RowKey::Settings {
            branch_id: m.branch_id.clone(),
            key: key.clone(),
        }));
    }
    for name in &m.roles {
        let role_id = ulid::Ulid::generate().to_string();
        insert_role(tx, &role_id, &m.branch_id, name, true)?;
        written.push(insert(RowKey::Roles {
            role_id: role_id.clone(),
        }));
        for p in builtin_perms(name) {
            insert_role_perm(tx, &role_id, p.as_str(), "allow")?;
            written.push(insert(RowKey::RolePerms {
                role_id: role_id.clone(),
                permission: p.as_str().to_owned(),
            }));
        }
    }
    for (role_id, p) in &m.role_perms {
        insert_role_perm(tx, role_id, p, "allow")?;
        written.push(insert(RowKey::RolePerms {
            role_id: role_id.clone(),
            permission: (*p).to_owned(),
        }));
    }
    Ok(written)
}

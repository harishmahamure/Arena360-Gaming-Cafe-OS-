//! `roles`, `role_perms` and `staff`. Only `apply()` writes them (through these helpers); the
//! Hub's loopback sign-in reads staff on its own read-only connection.

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `roles` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRow {
    pub role_id: String,
    pub branch_id: String,
    pub name: String,
    pub builtin: bool,
    pub rev: i64,
}

/// One `staff` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffRow {
    pub staff_id: String,
    pub branch_id: String,
    pub login: String,
    pub display_name: String,
    pub role_id: String,
    pub password_hash: Option<String>,
    pub pin_hash: Option<String>,
    pub pin_verifier_version: i64,
    pub active: bool,
    pub rev: i64,
}

/// A new or edited staff row (`rev` and timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffWrite<'a> {
    pub staff_id: &'a str,
    pub branch_id: &'a str,
    pub login: &'a str,
    pub display_name: &'a str,
    pub role_id: &'a str,
    pub password_hash: Option<&'a str>,
    pub pin_hash: Option<&'a str>,
    pub pin_verifier_version: i64,
    pub active: bool,
}

fn role_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<RoleRow> {
    Ok(RoleRow {
        role_id: r.get(0)?,
        branch_id: r.get(1)?,
        name: r.get(2)?,
        builtin: r.get::<_, i64>(3)? != 0,
        rev: r.get(4)?,
    })
}

const ROLE_COLUMNS: &str = "role_id, branch_id, name, builtin, rev";

/// The role `role_id`, if it exists.
pub fn find_role(conn: &Connection, role_id: &str) -> Result<Option<RoleRow>, AppError> {
    conn.query_row(
        &format!("SELECT {ROLE_COLUMNS} FROM roles WHERE role_id = ?1"),
        [role_id],
        role_from,
    )
    .optional()
    .map_err(db_error)
}

/// The role named `name` (compared without case: the column is `NOCASE`), if it exists. A database holds one branch.
pub fn find_role_by_name(conn: &Connection, name: &str) -> Result<Option<RoleRow>, AppError> {
    conn.query_row(
        &format!("SELECT {ROLE_COLUMNS} FROM roles WHERE name = ?1"),
        [name],
        role_from,
    )
    .optional()
    .map_err(db_error)
}

/// Every role, ordered by id.
pub fn list_roles(conn: &Connection) -> Result<Vec<RoleRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ROLE_COLUMNS} FROM roles ORDER BY role_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], role_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Inserts a role with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_role(
    conn: &Connection,
    role_id: &str,
    branch_id: &str,
    name: &str,
    builtin: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO roles (role_id, branch_id, name, builtin, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 0)",
        params![role_id, branch_id, name, i64::from(builtin)],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Renames a role (the stamp step then bumps `rev` and `updated_at`).
pub fn update_role_name(conn: &Connection, role_id: &str, name: &str) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE roles SET name = ?2 WHERE role_id = ?1",
            params![role_id, name],
        )
        .map_err(db_error)?;
    expect_one(n, "roles", role_id)
}

/// The `(permission, effect)` rows of `role_id`, ordered by permission.
pub fn list_role_perms(
    conn: &Connection,
    role_id: &str,
) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT permission, effect FROM role_perms WHERE role_id = ?1 ORDER BY permission")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([role_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The effect (`allow` or `deny`) `role_id` has for `permission`, if it has a row for it.
pub fn role_perm_effect(
    conn: &Connection,
    role_id: &str,
    permission: &str,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT effect FROM role_perms WHERE role_id = ?1 AND permission = ?2",
        params![role_id, permission],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// Inserts one permission row of a role.
pub fn insert_role_perm(
    conn: &Connection,
    role_id: &str,
    permission: &str,
    effect: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO role_perms (role_id, permission, effect) VALUES (?1, ?2, ?3)",
        params![role_id, permission, effect],
    )
    .map_err(db_error)?;
    Ok(())
}

const STAFF_COLUMNS: &str = "staff_id, branch_id, login, display_name, role_id, password_hash, \
                             pin_hash, pin_verifier_version, active, rev";

fn staff_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<StaffRow> {
    Ok(StaffRow {
        staff_id: r.get(0)?,
        branch_id: r.get(1)?,
        login: r.get(2)?,
        display_name: r.get(3)?,
        role_id: r.get(4)?,
        password_hash: r.get(5)?,
        pin_hash: r.get(6)?,
        pin_verifier_version: r.get(7)?,
        active: r.get::<_, i64>(8)? != 0,
        rev: r.get(9)?,
    })
}

pub fn set_staff_setup_code(
    conn: &Connection,
    staff_id: &str,
    hash: &str,
    expires_at: i64,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE staff SET setup_code_hash = ?2, setup_expires_at = ?3 WHERE staff_id = ?1",
            params![staff_id, hash, expires_at],
        )
        .map_err(db_error)?;
    expect_one(n, "staff", staff_id)
}

/// The staff member `staff_id`, if it exists.
pub fn find_staff(conn: &Connection, staff_id: &str) -> Result<Option<StaffRow>, AppError> {
    conn.query_row(
        &format!("SELECT {STAFF_COLUMNS} FROM staff WHERE staff_id = ?1"),
        [staff_id],
        staff_from,
    )
    .optional()
    .map_err(db_error)
}

/// The staff member with `login`, compared case-insensitively (the column is `NOCASE`).
pub fn find_staff_by_login(conn: &Connection, login: &str) -> Result<Option<StaffRow>, AppError> {
    conn.query_row(
        &format!("SELECT {STAFF_COLUMNS} FROM staff WHERE login = ?1"),
        [login],
        staff_from,
    )
    .optional()
    .map_err(db_error)
}

/// A staff member whose PIN may approve an intent: their id, PIN hash and the PIN's
/// `pin_verifier_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinCandidate {
    pub staff_id: String,
    pub pin_hash: String,
    pub pin_verifier_version: i64,
}

/// Up to `limit` active staff members with a PIN whose role allows every one of
/// `permissions`, other than `exclude`, ordered by id. No permissions means no candidates.
pub fn list_pin_candidates(
    conn: &Connection,
    permissions: &[&str],
    exclude: &str,
    limit: usize,
) -> Result<Vec<PinCandidate>, AppError> {
    let mut wanted: Vec<&str> = permissions.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (0..wanted.len())
        .map(|i| format!("?{}", i + 4))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT s.staff_id, s.pin_hash, s.pin_verifier_version
         FROM staff s
         JOIN role_perms p ON p.role_id = s.role_id
         WHERE s.active = 1 AND s.pin_hash IS NOT NULL AND s.staff_id <> ?1
           AND p.effect = 'allow' AND p.permission IN ({placeholders})
         GROUP BY s.staff_id
         HAVING COUNT(DISTINCT p.permission) = ?2
         ORDER BY s.staff_id
         LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql).map_err(db_error)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let count = i64::try_from(wanted.len()).unwrap_or(i64::MAX);
    let mut values: Vec<rusqlite::types::Value> =
        vec![exclude.to_owned().into(), count.into(), limit.into()];
    values.extend(
        wanted
            .iter()
            .map(|p| rusqlite::types::Value::from((*p).to_owned())),
    );
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), |r| {
            Ok(PinCandidate {
                staff_id: r.get(0)?,
                pin_hash: r.get(1)?,
                pin_verifier_version: r.get(2)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Up to `limit` active staff members with a PIN, `first` (when it qualifies) ahead of the
/// others, the rest ordered by id: whose PIN may unlock an idle-locked Hub UI.
pub fn list_unlock_candidates(
    conn: &Connection,
    first: &str,
    limit: usize,
) -> Result<Vec<PinCandidate>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT staff_id, pin_hash, pin_verifier_version
             FROM staff
             WHERE active = 1 AND pin_hash IS NOT NULL
             ORDER BY staff_id <> ?1, staff_id
             LIMIT ?2",
        )
        .map_err(db_error)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt
        .query_map(rusqlite::params![first, limit], |r| {
            Ok(PinCandidate {
                staff_id: r.get(0)?,
                pin_hash: r.get(1)?,
                pin_verifier_version: r.get(2)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// How many active staff members hold `role_id`.
pub fn count_active_staff_with_role(conn: &Connection, role_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM staff WHERE role_id = ?1 AND active = 1",
        [role_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Inserts a staff member with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_staff(conn: &Connection, s: &StaffWrite<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO staff (staff_id, branch_id, login, display_name, role_id, password_hash,
                            pin_hash, pin_verifier_version, active, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0)",
        params![
            s.staff_id,
            s.branch_id,
            s.login,
            s.display_name,
            s.role_id,
            s.password_hash,
            s.pin_hash,
            s.pin_verifier_version,
            i64::from(s.active)
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Rewrites a staff member's editable columns (the stamp step then bumps `rev`).
pub fn update_staff(conn: &Connection, s: &StaffWrite<'_>) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE staff SET login = ?2, display_name = ?3, role_id = ?4, password_hash = ?5,
                              pin_hash = ?6, pin_verifier_version = ?7, active = ?8
             WHERE staff_id = ?1",
            params![
                s.staff_id,
                s.login,
                s.display_name,
                s.role_id,
                s.password_hash,
                s.pin_hash,
                s.pin_verifier_version,
                i64::from(s.active)
            ],
        )
        .map_err(db_error)?;
    expect_one(n, "staff", s.staff_id)
}

fn expect_one(n: usize, table: &str, id: &str) -> Result<(), AppError> {
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no {table} row {id}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewBranch, Store, insert_branch};

    #[test]
    fn roles_perms_and_staff_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "N",
                        branch_code: "C",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                insert_role(tx, "R", "B", "Counter", true)?;
                insert_role_perm(tx, "R", "pos.sell", "allow")?;
                insert_role_perm(tx, "R", "pos.void", "deny")?;
                assert!(insert_role_perm(tx, "R", "x.y", "maybe").is_err());
                let write = StaffWrite {
                    staff_id: "S",
                    branch_id: "B",
                    login: "Counter",
                    display_name: "Counter",
                    role_id: "R",
                    password_hash: Some("$argon2id$x"),
                    pin_hash: None,
                    pin_verifier_version: 1,
                    active: true,
                };
                insert_staff(tx, &write)?;
                // Logins are unique without regard to case.
                assert!(
                    insert_staff(
                        tx,
                        &StaffWrite {
                            staff_id: "S2",
                            login: "COUNTER",
                            ..write.clone()
                        }
                    )
                    .is_err()
                );
                assert_eq!(
                    find_staff_by_login(tx, "cOuNtEr")?.map(|s| s.staff_id),
                    Some("S".into())
                );
                assert_eq!(
                    role_perm_effect(tx, "R", "pos.void")?.as_deref(),
                    Some("deny")
                );
                assert_eq!(role_perm_effect(tx, "R", "kitchen.use")?, None);
                assert_eq!(list_role_perms(tx, "R")?.len(), 2);
                assert_eq!(find_role_by_name(tx, "Counter")?.unwrap().role_id, "R");
                assert!(find_role(tx, "R")?.unwrap().builtin);
                update_staff(
                    tx,
                    &StaffWrite {
                        active: false,
                        ..write
                    },
                )?;
                assert!(!find_staff(tx, "S")?.unwrap().active);
                // PIN candidates: active, with a PIN, an allow row, and not the excluded one.
                insert_role(tx, "M", "B", "Manager", true)?;
                insert_role_perm(tx, "M", "settings.edit", "allow")?;
                insert_role_perm(tx, "R", "settings.edit", "deny")?;
                let manager = StaffWrite {
                    staff_id: "M1",
                    login: "m1",
                    role_id: "M",
                    pin_hash: Some("$argon2id$p"),
                    pin_verifier_version: 3,
                    active: true,
                    ..write.clone()
                };
                insert_staff(tx, &manager)?;
                insert_staff(
                    tx,
                    &StaffWrite {
                        staff_id: "M2",
                        login: "m2",
                        pin_hash: None,
                        ..manager.clone()
                    },
                )?;
                insert_staff(
                    tx,
                    &StaffWrite {
                        staff_id: "M3",
                        login: "m3",
                        active: false,
                        ..manager.clone()
                    },
                )?;
                insert_staff(
                    tx,
                    &StaffWrite {
                        staff_id: "M4",
                        login: "m4",
                        ..manager.clone()
                    },
                )?;
                insert_staff(
                    tx,
                    &StaffWrite {
                        staff_id: "C2",
                        login: "c2",
                        role_id: "R",
                        pin_hash: Some("$argon2id$c"),
                        active: true,
                        ..write.clone()
                    },
                )?;
                let ids =
                    |c: Vec<PinCandidate>| c.into_iter().map(|c| c.staff_id).collect::<Vec<_>>();
                assert_eq!(
                    ids(list_pin_candidates(tx, &["settings.edit"], "", 50)?),
                    ["M1", "M4"]
                );
                assert_eq!(
                    ids(list_pin_candidates(tx, &["settings.edit"], "M1", 50)?),
                    ["M4"]
                );
                assert_eq!(
                    ids(list_pin_candidates(tx, &["settings.edit"], "", 1)?),
                    ["M1"]
                );
                assert_eq!(
                    list_pin_candidates(tx, &["settings.edit"], "", 1)?[0].pin_verifier_version,
                    3
                );
                // Every permission must be allowed; duplicates count once.
                insert_role_perm(tx, "M", "device.remote", "allow")?;
                let both = ["settings.edit", "device.remote", "settings.edit"];
                assert_eq!(ids(list_pin_candidates(tx, &both, "", 50)?), ["M1", "M4"]);
                assert!(
                    list_pin_candidates(tx, &["settings.edit", "pos.void"], "", 50)?.is_empty()
                );
                assert!(list_pin_candidates(tx, &[], "", 50)?.is_empty());
                Ok(())
            })
            .unwrap();
    }
}

//! Kiosk admin exits (`device_admin_exits`, change-logged) and the manager-verifier history
//! (`verifier_history`, derived metadata written in the same transaction as the staff change
//! that caused it). Story 1.20.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// The `hub_meta` key holding the monotonic part of the manager-verifier version.
pub const META_VERIFIER_COUNTER: &str = "verifier_counter";

/// A new admin-exit row (`rev` and timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAdminExit<'a> {
    pub exit_id: &'a str,
    pub branch_id: &'a str,
    pub device_id: &'a str,
    pub staff_id: &'a str,
    pub mode: &'a str,
    pub started_at: i64,
    pub relock_at: i64,
    pub flagged: bool,
    pub flag_reason: Option<&'a str>,
}

/// One `device_admin_exits` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminExitRow {
    pub exit_id: String,
    pub branch_id: String,
    pub device_id: String,
    pub staff_id: String,
    pub mode: String,
    pub started_at: i64,
    pub relock_at: i64,
    pub relocked_at: Option<i64>,
    pub flagged: bool,
    pub flag_reason: Option<String>,
    pub rev: i64,
}

/// Inserts an admin-exit row with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_admin_exit(conn: &Connection, e: &NewAdminExit<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO device_admin_exits (exit_id, branch_id, device_id, staff_id, mode,
                                         started_at, relock_at, relocked_at, flagged,
                                         flag_reason, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, 0, 0, 0)",
        params![
            e.exit_id,
            e.branch_id,
            e.device_id,
            e.staff_id,
            e.mode,
            e.started_at,
            e.relock_at,
            i64::from(e.flagged),
            e.flag_reason
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The admin exit `exit_id`, if any.
pub fn find_admin_exit(conn: &Connection, exit_id: &str) -> Result<Option<AdminExitRow>, AppError> {
    conn.query_row(
        "SELECT exit_id, branch_id, device_id, staff_id, mode, started_at, relock_at,
                relocked_at, flagged, flag_reason, rev
         FROM device_admin_exits WHERE exit_id = ?1",
        [exit_id],
        |r| {
            Ok(AdminExitRow {
                exit_id: r.get(0)?,
                branch_id: r.get(1)?,
                device_id: r.get(2)?,
                staff_id: r.get(3)?,
                mode: r.get(4)?,
                started_at: r.get(5)?,
                relock_at: r.get(6)?,
                relocked_at: r.get(7)?,
                flagged: r.get::<_, i64>(8)? != 0,
                flag_reason: r.get(9)?,
                rev: r.get(10)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Records when the kiosk re-locked after the exit `exit_id` (the stamp step bumps `rev`).
pub fn set_admin_exit_relocked(conn: &Connection, exit_id: &str, at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE device_admin_exits SET relocked_at = ?2 WHERE exit_id = ?1",
        params![exit_id, at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The newest manager-verifier version and its digest, if any was recorded.
pub fn latest_verifier(conn: &Connection) -> Result<Option<(i64, String)>, AppError> {
    conn.query_row(
        "SELECT version, digest FROM verifier_history ORDER BY version DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// The manager-verifier version in effect at `at_ms` (0 when none was recorded by then).
pub fn verifier_version_at(conn: &Connection, at_ms: i64) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM verifier_history WHERE valid_from <= ?1",
        [at_ms],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Records that manager-verifier `version` (whose entries hash to `digest` and list
/// `staff_ids`) is in effect from `valid_from`.
pub fn insert_verifier_version(
    conn: &Connection,
    version: i64,
    valid_from: i64,
    digest: &str,
    staff_ids: &[String],
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO verifier_history (version, valid_from, digest, staff_ids)
         VALUES (?1, ?2, ?3, ?4)",
        params![version, valid_from, digest, staff_ids.join("\n")],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The staff ids manager-verifier `version` listed (`None` when that version was never
/// recorded).
pub fn verifier_staff(conn: &Connection, version: i64) -> Result<Option<Vec<String>>, AppError> {
    conn.query_row(
        "SELECT staff_ids FROM verifier_history WHERE version = ?1",
        [version],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map(|o| {
        o.map(|ids| {
            ids.split('\n')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
    })
    .map_err(db_error)
}

/// The highest `pin_verifier_version` of any staff member (0 with none). It never goes down:
/// staff rows are never deleted and a PIN version only rises.
pub fn max_pin_verifier_version(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(pin_verifier_version), 0) FROM staff",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewBranch, NewDevice, Store, insert_branch, insert_device};

    #[test]
    fn exits_and_verifier_history_round_trip() {
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
                insert_device(
                    tx,
                    &NewDevice {
                        device_id: "D",
                        branch_id: "B",
                        class: "paired",
                        label: "K",
                        token_hash: "h",
                    },
                )?;
                insert_admin_exit(
                    tx,
                    &NewAdminExit {
                        exit_id: "E",
                        branch_id: "B",
                        device_id: "D",
                        staff_id: "S",
                        mode: "cached_verifier",
                        started_at: 10,
                        relock_at: 20,
                        flagged: true,
                        flag_reason: Some("stale_verifier"),
                    },
                )?;
                set_admin_exit_relocked(tx, "E", 15)?;
                let row = find_admin_exit(tx, "E")?.unwrap();
                assert!(row.flagged);
                assert_eq!(row.relocked_at, Some(15));
                assert!(find_admin_exit(tx, "X")?.is_none());
                assert_eq!(latest_verifier(tx)?, None);
                assert_eq!(verifier_version_at(tx, 100)?, 0);
                insert_verifier_version(tx, 2, 50, "a", &["S1".into(), "S2".into()])?;
                insert_verifier_version(tx, 5, 80, "b", &[])?;
                assert_eq!(verifier_staff(tx, 2)?, Some(vec!["S1".into(), "S2".into()]));
                assert_eq!(verifier_staff(tx, 5)?, Some(vec![]));
                assert_eq!(verifier_staff(tx, 3)?, None);
                assert_eq!(latest_verifier(tx)?, Some((5, "b".into())));
                assert_eq!(verifier_version_at(tx, 49)?, 0);
                assert_eq!(verifier_version_at(tx, 50)?, 2);
                assert_eq!(verifier_version_at(tx, 79)?, 2);
                assert_eq!(verifier_version_at(tx, 80)?, 5);
                assert_eq!(max_pin_verifier_version(tx)?, 0);
                assert_eq!(
                    crate::get_meta(tx, META_VERIFIER_COUNTER)?.as_deref(),
                    Some("0")
                );
                Ok(())
            })
            .unwrap();
        // A mode outside the list is refused by the CHECK.
        let bad = store.write_tx(|tx| {
            insert_admin_exit(
                tx,
                &NewAdminExit {
                    exit_id: "F",
                    branch_id: "B",
                    device_id: "D",
                    staff_id: "S",
                    mode: "teleport",
                    started_at: 1,
                    relock_at: 2,
                    flagged: false,
                    flag_reason: None,
                },
            )
        });
        assert!(bad.is_err());
    }
}

//! The `devices` table: LAN devices that authenticate with a token. Only `apply()` writes it
//! (through these helpers); the Hub's LAN sessions read it to verify tokens.

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `devices` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub device_id: String,
    pub branch_id: String,
    /// `paired` or `kds`.
    pub class: String,
    pub label: String,
    /// The argon2id PHC string of the token's secret.
    pub token_hash: String,
    pub reported_version: Option<String>,
    pub revoked_at: Option<i64>,
    pub rev: i64,
}

/// A new device (`rev` and timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDevice<'a> {
    pub device_id: &'a str,
    pub branch_id: &'a str,
    pub class: &'a str,
    pub label: &'a str,
    pub token_hash: &'a str,
}

const COLUMNS: &str =
    "device_id, branch_id, class, label, token_hash, reported_version, revoked_at, rev";

fn device_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceRow> {
    Ok(DeviceRow {
        device_id: r.get(0)?,
        branch_id: r.get(1)?,
        class: r.get(2)?,
        label: r.get(3)?,
        token_hash: r.get(4)?,
        reported_version: r.get(5)?,
        revoked_at: r.get(6)?,
        rev: r.get(7)?,
    })
}

/// The device `device_id`, if it exists.
pub fn find_device(conn: &Connection, device_id: &str) -> Result<Option<DeviceRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM devices WHERE device_id = ?1"),
        [device_id],
        device_from,
    )
    .optional()
    .map_err(db_error)
}

/// Every device, ordered by id.
pub fn list_devices(conn: &Connection) -> Result<Vec<DeviceRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLUMNS} FROM devices ORDER BY device_id"))
        .map_err(db_error)?;
    let rows = stmt.query_map([], device_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Inserts a device with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_device(conn: &Connection, d: &NewDevice<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO devices (device_id, branch_id, class, label, token_hash, reported_version,
                              revoked_at, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, 0, 0, 0)",
        params![d.device_id, d.branch_id, d.class, d.label, d.token_hash],
    )
    .map_err(db_error)?;
    Ok(())
}

fn expect_one(n: usize, device_id: &str) -> Result<(), AppError> {
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no devices row {device_id}"),
        ))
    }
}

/// Sets `revoked_at` (the stamp step then bumps `rev` and `updated_at`).
pub fn set_device_revoked(conn: &Connection, device_id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices SET revoked_at = ?2 WHERE device_id = ?1",
            params![device_id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

/// Sets `reported_version` (the stamp step then bumps `rev` and `updated_at`).
pub fn set_device_reported_version(
    conn: &Connection,
    device_id: &str,
    version: &str,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices SET reported_version = ?2 WHERE device_id = ?1",
            params![device_id, version],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewBranch, Store, insert_branch};

    #[test]
    fn devices_round_trip_and_class_is_checked() {
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
                        label: "Dev Kiosk",
                        token_hash: "$argon2id$x",
                    },
                )?;
                set_device_reported_version(tx, "D", "0.1.0")?;
                set_device_revoked(tx, "D", 42)
            })
            .unwrap();
        let d = store.device("D").unwrap().unwrap();
        assert_eq!(d.class, "paired");
        assert_eq!(d.reported_version.as_deref(), Some("0.1.0"));
        assert_eq!(d.revoked_at, Some(42));
        assert_eq!(store.device("nope").unwrap(), None);
        let err = store
            .write_tx(|tx| {
                insert_device(
                    tx,
                    &NewDevice {
                        device_id: "P",
                        branch_id: "B",
                        class: "phone",
                        label: "x",
                        token_hash: "h",
                    },
                )
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(
            store
                .write_tx(|tx| set_device_revoked(tx, "nope", 1))
                .is_err()
        );
    }
}

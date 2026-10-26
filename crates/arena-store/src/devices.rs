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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMetaRow {
    pub device_id: String,
    pub kind: String,
    pub label: String,
    pub group_id: Option<String>,
    pub rev: i64,
    pub deleted_at: Option<i64>,
    pub tv_number: Option<i64>,
    pub zone_id: Option<String>,
    pub floor_x: Option<i64>,
    pub floor_y: Option<i64>,
    pub state: String,
    pub prior_state: Option<String>,
    pub maintenance_note: Option<String>,
}

const META_COLUMNS: &str = "device_id, kind, label, group_id, rev, deleted_at, tv_number, zone_id, floor_x, floor_y, state, prior_state, maintenance_note";

fn meta_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceMetaRow> {
    Ok(DeviceMetaRow {
        device_id: r.get(0)?,
        kind: r.get(1)?,
        label: r.get(2)?,
        group_id: r.get(3)?,
        rev: r.get(4)?,
        deleted_at: r.get(5)?,
        tv_number: r.get(6)?,
        zone_id: r.get(7)?,
        floor_x: r.get(8)?,
        floor_y: r.get(9)?,
        state: r.get(10)?,
        prior_state: r.get(11)?,
        maintenance_note: r.get(12)?,
    })
}

pub fn devices_meta_exists(conn: &Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'devices_meta')",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn find_device_meta(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<DeviceMetaRow>, AppError> {
    conn.query_row(
        &format!("SELECT {META_COLUMNS} FROM devices_meta WHERE device_id = ?1"),
        [device_id],
        meta_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_device_meta(conn: &Connection) -> Result<Vec<DeviceMetaRow>, AppError> {
    if !devices_meta_exists(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {META_COLUMNS} FROM devices_meta ORDER BY device_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], meta_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_device_meta(
    conn: &Connection,
    device_id: &str,
    kind: &str,
    label: &str,
    group_id: Option<&str>,
    tv_number: Option<i64>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO devices_meta (device_id, kind, label, group_id, rev, deleted_at,
                                   created_at, updated_at, tv_number, state)
         VALUES (?1, ?2, ?3, ?4, 0, NULL, 0, 0, ?5,
                 CASE WHEN EXISTS (SELECT 1 FROM devices WHERE device_id = ?1 AND token_hash = '')
                      THEN 'online' ELSE 'offline' END)",
        params![device_id, kind, label, group_id, tv_number],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_device_meta(
    conn: &Connection,
    device_id: &str,
    kind: &str,
    label: &str,
    tv_number: Option<i64>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices_meta SET kind = ?2, label = ?3, tv_number = ?4 WHERE device_id = ?1",
            params![device_id, kind, label, tv_number],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

pub fn set_device_meta_deleted(
    conn: &Connection,
    device_id: &str,
    at_ms: i64,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices_meta SET deleted_at = ?2 WHERE device_id = ?1",
            params![device_id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

pub fn tv_number_taken(
    conn: &Connection,
    tv_number: i64,
    except: Option<&str>,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM devices_meta WHERE kind = 'console' AND tv_number = ?1
                        AND deleted_at IS NULL AND device_id IS NOT ?2)",
        params![tv_number, except],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn count_live_devices(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM devices_meta WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn set_device_label(conn: &Connection, device_id: &str, label: &str) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices SET label = ?2 WHERE device_id = ?1",
            params![device_id, label],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub group_id: String,
    pub name: String,
    pub kind: String,
    pub colour: String,
    pub sort_order: i64,
    pub rev: i64,
    pub deleted_at: Option<i64>,
}

const GROUP_COLUMNS: &str = "group_id, name, kind, colour, sort_order, rev, deleted_at";

fn group_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<GroupRow> {
    Ok(GroupRow {
        group_id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        colour: r.get(3)?,
        sort_order: r.get(4)?,
        rev: r.get(5)?,
        deleted_at: r.get(6)?,
    })
}

pub fn find_group(conn: &Connection, group_id: &str) -> Result<Option<GroupRow>, AppError> {
    conn.query_row(
        &format!("SELECT {GROUP_COLUMNS} FROM device_groups WHERE group_id = ?1"),
        [group_id],
        group_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn find_group_by_name(conn: &Connection, name: &str) -> Result<Option<GroupRow>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {GROUP_COLUMNS} FROM device_groups
             WHERE name = ?1 COLLATE NOCASE AND deleted_at IS NULL"
        ),
        [name],
        group_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_groups(conn: &Connection) -> Result<Vec<GroupRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {GROUP_COLUMNS} FROM device_groups WHERE deleted_at IS NULL
             ORDER BY sort_order, name COLLATE NOCASE, group_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], group_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn count_group_devices(conn: &Connection, group_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM devices_meta WHERE group_id = ?1 AND deleted_at IS NULL",
        [group_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_group(
    conn: &Connection,
    group_id: &str,
    name: &str,
    kind: &str,
    colour: &str,
    sort_order: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO device_groups (group_id, name, kind, colour, sort_order, rev, deleted_at,
                                    created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, 0, 0)",
        params![group_id, name, kind, colour, sort_order],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_group(
    conn: &Connection,
    group_id: &str,
    name: &str,
    kind: &str,
    colour: &str,
    sort_order: i64,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE device_groups SET name = ?2, kind = ?3, colour = ?4, sort_order = ?5
             WHERE group_id = ?1",
            params![group_id, name, kind, colour, sort_order],
        )
        .map_err(db_error)?;
    expect_one(n, group_id)
}

pub fn set_group_deleted(conn: &Connection, group_id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE device_groups SET deleted_at = ?2 WHERE group_id = ?1",
            params![group_id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, group_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateRow {
    pub rate_id: String,
    pub group_id: String,
    pub price_per_hour_minor: i64,
}

fn rate_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<RateRow> {
    Ok(RateRow {
        rate_id: r.get(0)?,
        group_id: r.get(1)?,
        price_per_hour_minor: r.get(2)?,
    })
}

pub fn find_active_rate(conn: &Connection, group_id: &str) -> Result<Option<RateRow>, AppError> {
    conn.query_row(
        "SELECT rate_id, group_id, price_per_hour_minor FROM rates
         WHERE group_id = ?1 AND active = 1",
        [group_id],
        rate_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_active_rates(conn: &Connection) -> Result<Vec<RateRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT rate_id, group_id, price_per_hour_minor FROM rates WHERE active = 1
             ORDER BY group_id",
        )
        .map_err(db_error)?;
    let rows = stmt.query_map([], rate_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn count_groups_without_rate(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM device_groups g
         WHERE g.deleted_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM rates r WHERE r.group_id = g.group_id AND r.active = 1)",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_rate(
    conn: &Connection,
    rate_id: &str,
    group_id: &str,
    price_per_hour_minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO rates (rate_id, group_id, price_per_hour_minor, active, rev, created_at,
                            updated_at)
         VALUES (?1, ?2, ?3, 1, 0, 0, 0)",
        params![rate_id, group_id, price_per_hour_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn deactivate_rate(conn: &Connection, rate_id: &str) -> Result<(), AppError> {
    let n = conn
        .execute("UPDATE rates SET active = 0 WHERE rate_id = ?1", [rate_id])
        .map_err(db_error)?;
    expect_one(n, rate_id)
}

pub fn set_device_group(
    conn: &Connection,
    device_id: &str,
    group_id: Option<&str>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices_meta SET group_id = ?2 WHERE device_id = ?1",
            params![device_id, group_id],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRow {
    pub zone_id: String,
    pub name: String,
    pub sort_order: i64,
    pub rev: i64,
    pub deleted_at: Option<i64>,
}

const ZONE_COLUMNS: &str = "zone_id, name, sort_order, rev, deleted_at";

fn zone_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ZoneRow> {
    Ok(ZoneRow {
        zone_id: r.get(0)?,
        name: r.get(1)?,
        sort_order: r.get(2)?,
        rev: r.get(3)?,
        deleted_at: r.get(4)?,
    })
}

pub fn find_zone(conn: &Connection, zone_id: &str) -> Result<Option<ZoneRow>, AppError> {
    conn.query_row(
        &format!("SELECT {ZONE_COLUMNS} FROM floor_zones WHERE zone_id = ?1"),
        [zone_id],
        zone_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn find_zone_by_name(conn: &Connection, name: &str) -> Result<Option<ZoneRow>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {ZONE_COLUMNS} FROM floor_zones
             WHERE name = ?1 COLLATE NOCASE AND deleted_at IS NULL"
        ),
        [name],
        zone_from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_zones(conn: &Connection) -> Result<Vec<ZoneRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ZONE_COLUMNS} FROM floor_zones WHERE deleted_at IS NULL
             ORDER BY sort_order, name COLLATE NOCASE, zone_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], zone_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn count_zone_devices(conn: &Connection, zone_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM devices_meta WHERE zone_id = ?1 AND deleted_at IS NULL",
        [zone_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn insert_zone(
    conn: &Connection,
    zone_id: &str,
    name: &str,
    sort_order: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO floor_zones (zone_id, name, sort_order, rev, deleted_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, NULL, 0, 0)",
        params![zone_id, name, sort_order],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_zone(
    conn: &Connection,
    zone_id: &str,
    name: &str,
    sort_order: i64,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE floor_zones SET name = ?2, sort_order = ?3 WHERE zone_id = ?1",
            params![zone_id, name, sort_order],
        )
        .map_err(db_error)?;
    expect_one(n, zone_id)
}

pub fn set_zone_deleted(conn: &Connection, zone_id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE floor_zones SET deleted_at = ?2 WHERE zone_id = ?1",
            params![zone_id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, zone_id)
}

pub fn cell_taken(
    conn: &Connection,
    zone_id: &str,
    x: i64,
    y: i64,
    except: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM devices_meta WHERE zone_id = ?1 AND floor_x = ?2
                        AND floor_y = ?3 AND deleted_at IS NULL AND device_id != ?4)",
        params![zone_id, x, y, except],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn set_device_place(
    conn: &Connection,
    device_id: &str,
    zone_id: Option<&str>,
    x: Option<i64>,
    y: Option<i64>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices_meta SET zone_id = ?2, floor_x = ?3, floor_y = ?4 WHERE device_id = ?1",
            params![device_id, zone_id, x, y],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

pub fn set_device_state(
    conn: &Connection,
    device_id: &str,
    state: &str,
    prior_state: Option<&str>,
    note: Option<&str>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE devices_meta SET state = ?2, prior_state = ?3, maintenance_note = ?4 WHERE device_id = ?1",
            params![device_id, state, prior_state, note],
        )
        .map_err(db_error)?;
    expect_one(n, device_id)
}

pub fn change_actor_kind(
    conn: &Connection,
    epoch: &str,
    seq: i64,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT actor_kind FROM changes WHERE epoch = ?1 AND seq = ?2",
        params![epoch, seq],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
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

    #[test]
    fn migration_0011_backfills_paired_devices_as_pc_and_meta_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            for m in crate::migrations::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 10)
            {
                conn.execute_batch(m.sql).unwrap();
                crate::record_schema_version(&conn, m.version, 0).unwrap();
            }
            insert_branch(
                &conn,
                &NewBranch {
                    branch_id: "B",
                    name: "N",
                    branch_code: "C",
                    currency: "INR",
                    tz: "UTC",
                    day_start_min: 0,
                    fy_start_month: 1,
                },
            )
            .unwrap();
            for (id, class, label) in [("K", "paired", "Dev Kiosk"), ("S", "kds", "Kitchen")] {
                insert_device(
                    &conn,
                    &NewDevice {
                        device_id: id,
                        branch_id: "B",
                        class,
                        label,
                        token_hash: "$argon2id$x",
                    },
                )
                .unwrap();
            }
        }
        let mut store = Store::open(&path, 0).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            crate::LATEST_SCHEMA_VERSION
        );
        let meta = store.devices_meta().unwrap();
        assert_eq!(meta.len(), 1);
        assert_eq!(meta[0].device_id, "K");
        assert_eq!(meta[0].kind, "pc");
        assert_eq!(meta[0].label, "Dev Kiosk");
        assert_eq!(meta[0].group_id, None);
        store
            .write_tx(|tx| update_device_meta(tx, "K", "console", "TV 1", None))
            .unwrap();
        let k = store.device_meta("K").unwrap().unwrap();
        assert_eq!((k.kind.as_str(), k.label.as_str()), ("console", "TV 1"));
        let err = store
            .write_tx(|tx| update_device_meta(tx, "K", "toaster", "x", None))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(
            store
                .write_tx(|tx| insert_device_meta(tx, "nope", "pc", "x", None, None))
                .is_err()
        );
    }

    #[test]
    fn migration_0012_keeps_devices_and_enforces_the_group_fk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            for m in crate::migrations::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 11)
            {
                conn.execute_batch(m.sql).unwrap();
                crate::record_schema_version(&conn, m.version, 0).unwrap();
            }
            insert_branch(
                &conn,
                &NewBranch {
                    branch_id: "B",
                    name: "N",
                    branch_code: "C",
                    currency: "INR",
                    tz: "UTC",
                    day_start_min: 0,
                    fy_start_month: 1,
                },
            )
            .unwrap();
            insert_device(
                &conn,
                &NewDevice {
                    device_id: "K",
                    branch_id: "B",
                    class: "paired",
                    label: "Dev Kiosk",
                    token_hash: "$argon2id$x",
                },
            )
            .unwrap();
            conn.execute(
                "INSERT INTO devices_meta VALUES ('K', 'pc', 'Dev Kiosk', NULL, 1, NULL, 0, 0)",
                [],
            )
            .unwrap();
        }
        let mut store = Store::open(&path, 0).unwrap();
        assert_eq!(store.device_meta("K").unwrap().unwrap().label, "Dev Kiosk");
        assert!(store.groups().unwrap().is_empty());
        assert!(
            store
                .write_tx(|tx| set_device_group(tx, "K", Some("nope")))
                .is_err()
        );
        store
            .write_tx(|tx| {
                insert_group(tx, "G", "PCs", "pc", "teal", 1)?;
                assert_eq!(count_group_devices(tx, "G")?, 0);
                set_device_group(tx, "K", Some("G"))?;
                assert_eq!(count_group_devices(tx, "G")?, 1);
                assert!(insert_group(tx, "H", "pcs", "pc", "teal", 2).is_err());
                set_device_group(tx, "K", None)?;
                set_group_deleted(tx, "G", 5)?;
                insert_group(tx, "H", "pcs", "pc", "teal", 2)
            })
            .unwrap();
        assert_eq!(store.groups().unwrap().len(), 1);
    }

    #[test]
    fn migration_0014_adds_a_nullable_tv_number_with_a_unique_live_console_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            for m in crate::migrations::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 13)
            {
                conn.execute_batch(m.sql).unwrap();
                crate::record_schema_version(&conn, m.version, 0).unwrap();
            }
            insert_branch(
                &conn,
                &NewBranch {
                    branch_id: "B",
                    name: "N",
                    branch_code: "C",
                    currency: "INR",
                    tz: "UTC",
                    day_start_min: 0,
                    fy_start_month: 1,
                },
            )
            .unwrap();
            for id in ["K", "C1", "C2"] {
                let device = NewDevice {
                    device_id: id,
                    branch_id: "B",
                    class: "paired",
                    label: id,
                    token_hash: "",
                };
                insert_device(&conn, &device).unwrap();
                let kind = if id == "K" { "pc" } else { "console" };
                conn.execute(
                    "INSERT INTO devices_meta VALUES (?1, ?2, ?1, NULL, 1, NULL, 0, 0)",
                    params![id, kind],
                )
                .unwrap();
            }
        }
        let mut store = Store::open(&path, 0).unwrap();
        assert_eq!(store.device_meta("K").unwrap().unwrap().tv_number, None);
        store
            .write_tx(|tx| {
                update_device_meta(tx, "C1", "console", "C1", Some(1))?;
                assert!(update_device_meta(tx, "C2", "console", "C2", Some(1)).is_err());
                Ok(())
            })
            .unwrap();
        store
            .write_tx(|tx| {
                assert!(tv_number_taken(tx, 1, None)?);
                assert!(!tv_number_taken(tx, 1, Some("C1"))?);
                set_device_meta_deleted(tx, "C1", 5)?;
                assert!(!tv_number_taken(tx, 1, None)?);
                update_device_meta(tx, "C2", "console", "C2", Some(1))
            })
            .unwrap();
        assert_eq!(count_live_devices(&store.conn).unwrap(), 2);
    }
    #[test]
    fn migration_0020_backfills_state_and_limits_it_to_the_five_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            for m in crate::migrations::MIGRATIONS
                .iter()
                .filter(|m| m.version <= 19)
            {
                conn.execute_batch(m.sql).unwrap();
                crate::record_schema_version(&conn, m.version, 0).unwrap();
            }
            insert_branch(
                &conn,
                &NewBranch {
                    branch_id: "B",
                    name: "N",
                    branch_code: "C",
                    currency: "INR",
                    tz: "UTC",
                    day_start_min: 0,
                    fy_start_month: 1,
                },
            )
            .unwrap();
            for (id, hash) in [("K", "$argon2id$x"), ("M", "")] {
                let device = NewDevice {
                    device_id: id,
                    branch_id: "B",
                    class: "paired",
                    label: id,
                    token_hash: hash,
                };
                insert_device(&conn, &device).unwrap();
                conn.execute(
                    "INSERT INTO devices_meta (device_id, kind, label, rev, created_at, updated_at) VALUES (?1, 'pc', ?1, 1, 0, 0)",
                    params![id],
                )
                .unwrap();
            }
        }
        let mut store = Store::open(&path, 0).unwrap();
        assert_eq!(store.device_meta("K").unwrap().unwrap().state, "offline");
        assert_eq!(store.device_meta("M").unwrap().unwrap().state, "online");
        store
            .write_tx(|tx| {
                for state in ["online", "offline", "in_session", "reserved", "maintenance"] {
                    set_device_state(tx, "K", state, None, None)?;
                }
                assert!(set_device_state(tx, "K", "busy", None, None).is_err());
                assert!(set_device_state(tx, "K", "online", Some("maintenance"), None).is_err());
                Ok(())
            })
            .unwrap();
    }
}

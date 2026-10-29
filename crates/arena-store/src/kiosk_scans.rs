//! The latest installed-software scan of each kiosk (Story 6.4): the `kiosk_scans` change-log
//! table, one row per device. Only `apply()` writes it, through these helpers; the entries and
//! launchers are JSON text here (the payload types live in `arena-proto`).

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `kiosk_scans` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KioskScanRow {
    pub device_id: String,
    pub scanned_at: i64,
    /// A JSON array of the executables found.
    pub entries: String,
    /// A JSON array of the launchers installed and their paths.
    pub launchers: String,
    pub skipped: i64,
    pub rev: i64,
}

/// What `apply()` writes: a scan's fields (`rev` and the timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KioskScanWrite<'a> {
    pub scanned_at: i64,
    pub entries: &'a str,
    pub launchers: &'a str,
    pub skipped: i64,
}

/// One live paired device with the summary of its last scan (the scan fields are `None` when it
/// was never scanned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KioskScanSummary {
    pub device_id: String,
    pub label: String,
    pub scanned_at: Option<i64>,
    pub entry_count: Option<i64>,
    pub skipped: Option<i64>,
    pub launchers: Option<String>,
}

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<KioskScanRow> {
    Ok(KioskScanRow {
        device_id: r.get(0)?,
        scanned_at: r.get(1)?,
        entries: r.get(2)?,
        launchers: r.get(3)?,
        skipped: r.get(4)?,
        rev: r.get(5)?,
    })
}

/// The device's stored scan, if it has one.
pub fn find_kiosk_scan(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<KioskScanRow>, AppError> {
    conn.query_row(
        "SELECT device_id, scanned_at, entries, launchers, skipped, rev
         FROM kiosk_scans WHERE device_id = ?1",
        [device_id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// Adds the device's first scan (the stamp step sets `rev = 1`).
pub fn insert_kiosk_scan(
    conn: &Connection,
    device_id: &str,
    w: &KioskScanWrite<'_>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO kiosk_scans (device_id, scanned_at, entries, launchers, skipped, rev,
                                  created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0)",
        params![device_id, w.scanned_at, w.entries, w.launchers, w.skipped],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Replaces the device's stored scan (the stamp step then bumps `rev`).
pub fn replace_kiosk_scan(
    conn: &Connection,
    device_id: &str,
    w: &KioskScanWrite<'_>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE kiosk_scans SET scanned_at = ?2, entries = ?3, launchers = ?4, skipped = ?5
             WHERE device_id = ?1",
            params![device_id, w.scanned_at, w.entries, w.launchers, w.skipped],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no kiosk_scans row {device_id}"),
        ))
    }
}

/// Every live paired device (not revoked, not deleted from the floor) with its last scan's
/// summary, ordered by label then id.
pub fn list_kiosk_scan_summaries(conn: &Connection) -> Result<Vec<KioskScanSummary>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT d.device_id, COALESCE(m.label, d.label), s.scanned_at,
                    CASE WHEN s.device_id IS NULL THEN NULL
                         ELSE json_array_length(s.entries) END,
                    s.skipped, s.launchers
             FROM devices d
             LEFT JOIN devices_meta m ON m.device_id = d.device_id
             LEFT JOIN kiosk_scans s ON s.device_id = d.device_id
             WHERE d.class = 'paired' AND d.revoked_at IS NULL AND m.deleted_at IS NULL
             ORDER BY COALESCE(m.label, d.label) COLLATE NOCASE, d.device_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(KioskScanSummary {
                device_id: r.get(0)?,
                label: r.get(1)?,
                scanned_at: r.get(2)?,
                entry_count: r.get(3)?,
                skipped: r.get(4)?,
                launchers: r.get(5)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Every device's stored scan, ordered by device id.
pub fn list_kiosk_scans(conn: &Connection) -> Result<Vec<KioskScanRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT device_id, scanned_at, entries, launchers, skipped, rev
             FROM kiosk_scans ORDER BY device_id",
        )
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// One `game_launched` device event of a live paired PC (Story 6.25): the PC, when the player
/// clicked, and the JSON detail (`game_id`, `name`, `launch_ms`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLaunchRow {
    pub device_id: String,
    pub label: String,
    pub at: i64,
    pub detail: String,
}

/// Every `game_launched` event at or after `since` on a live paired PC, oldest first.
pub fn list_game_launches(conn: &Connection, since: i64) -> Result<Vec<GameLaunchRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.device_id, COALESCE(m.label, d.label), e.at, e.detail
             FROM device_events e
             JOIN devices d ON d.device_id = e.device_id
             LEFT JOIN devices_meta m ON m.device_id = d.device_id
             WHERE e.code = 'game_launched' AND e.at >= ?1
               AND d.class = 'paired' AND d.revoked_at IS NULL AND m.deleted_at IS NULL
             ORDER BY e.at, e.event_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([since], |r| {
            Ok(GameLaunchRow {
                device_id: r.get(0)?,
                label: r.get(1)?,
                at: r.get(2)?,
                detail: r.get(3)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE devices (device_id TEXT PRIMARY KEY, class TEXT NOT NULL,
                 label TEXT NOT NULL, revoked_at INTEGER NULL);
             CREATE TABLE devices_meta (device_id TEXT PRIMARY KEY, label TEXT NOT NULL,
                 deleted_at INTEGER NULL);",
        )
        .unwrap();
        conn.execute_batch(include_str!("../migrations/0063_kiosk_scans.sql"))
            .unwrap();
        conn
    }

    fn device(c: &Connection, id: &str, class: &str, label: &str, revoked: Option<i64>) {
        c.execute(
            "INSERT INTO devices VALUES (?1, ?2, ?3, ?4)",
            params![id, class, label, revoked],
        )
        .unwrap();
        c.execute(
            "INSERT INTO devices_meta VALUES (?1, ?2, NULL)",
            params![id, label],
        )
        .unwrap();
    }

    fn write(at: i64, entries: &str) -> KioskScanWrite<'_> {
        KioskScanWrite {
            scanned_at: at,
            entries,
            launchers: r#"[{"launcher":"steam","path":"C:\\S"}]"#,
            skipped: 3,
        }
    }

    #[test]
    fn a_scan_is_inserted_then_replaced() {
        let c = conn();
        device(&c, "D1", "paired", "PC 1", None);
        assert!(find_kiosk_scan(&c, "D1").unwrap().is_none());
        insert_kiosk_scan(&c, "D1", &write(10, "[1,2]")).unwrap();
        let row = find_kiosk_scan(&c, "D1").unwrap().unwrap();
        assert_eq!(
            (row.scanned_at, row.entries.as_str(), row.skipped),
            (10, "[1,2]", 3)
        );
        replace_kiosk_scan(&c, "D1", &write(20, "[1]")).unwrap();
        let row = find_kiosk_scan(&c, "D1").unwrap().unwrap();
        assert_eq!((row.scanned_at, row.entries.as_str()), (20, "[1]"));
        assert!(replace_kiosk_scan(&c, "D9", &write(1, "[]")).is_err());
        assert!(
            insert_kiosk_scan(&c, "D1", &write(30, "[]")).is_err(),
            "one row per device"
        );
        let mut bad = write(0, "[]");
        bad.scanned_at = 0;
        assert!(insert_kiosk_scan(&c, "D2", &bad).is_err());
    }

    #[test]
    fn the_summaries_list_live_paired_devices_with_or_without_a_scan() {
        let c = conn();
        device(&c, "D1", "paired", "b PC", None);
        device(&c, "D2", "paired", "A PC", None);
        device(&c, "D3", "paired", "gone", Some(5));
        device(&c, "D4", "kds", "kitchen", None);
        device(&c, "D5", "paired", "deleted", None);
        c.execute(
            "UPDATE devices_meta SET deleted_at = 1 WHERE device_id = 'D5'",
            [],
        )
        .unwrap();
        insert_kiosk_scan(&c, "D1", &write(10, r#"[{"a":1},{"a":2},{"a":3}]"#)).unwrap();
        let list = list_kiosk_scan_summaries(&c).unwrap();
        assert_eq!(
            list.iter()
                .map(|s| s.device_id.as_str())
                .collect::<Vec<_>>(),
            ["D2", "D1"]
        );
        assert_eq!(list[0].scanned_at, None);
        assert_eq!(list[0].entry_count, None);
        assert_eq!(list[0].launchers, None);
        assert_eq!(list[1].scanned_at, Some(10));
        assert_eq!(list[1].entry_count, Some(3));
        assert_eq!(list[1].skipped, Some(3));
        assert!(list[1].launchers.as_deref().unwrap().contains("steam"));
    }

    #[test]
    fn launches_are_the_game_launched_events_of_live_paired_pcs_since_a_time() {
        let c = conn();
        c.execute_batch(
            "PRAGMA foreign_keys = OFF; CREATE TABLE branch (branch_id TEXT PRIMARY KEY);",
        )
        .unwrap();
        c.execute_batch(include_str!("../migrations/0009_device_events.sql"))
            .unwrap();
        device(&c, "P1", "paired", "PC 1", None);
        device(&c, "P2", "paired", "PC 2", Some(5));
        device(&c, "H", "hub", "Hub", None);
        let event = |id: &str, dev: &str, at: i64, code: &str| {
            c.execute(
                "INSERT INTO device_events VALUES (?1, 'B', ?2, ?3, 'info', ?4, '{}', 1, 0, 0)",
                params![id, dev, at, code],
            )
            .unwrap();
        };
        event("E1", "P1", 100, "game_launched");
        event("E2", "P1", 50, "game_launched");
        event("E3", "P1", 120, "cleanup_failed");
        event("E4", "P2", 120, "game_launched");
        event("E5", "H", 120, "game_launched");
        let rows = list_game_launches(&c, 60).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].device_id.as_str(),
                rows[0].label.as_str(),
                rows[0].at
            ),
            ("P1", "PC 1", 100)
        );
    }
}

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPackRow {
    pub id: String,
    pub plan_id: String,
    pub kind: String,
    pub name: String,
    pub price_minor: i64,
    pub minutes_total: Option<i64>,
    pub minutes_left: Option<i64>,
    pub validity_days: i64,
    pub window_start_min: Option<i64>,
    pub window_end_min: Option<i64>,
    pub group_ids: Vec<String>,
    pub bought_at: i64,
    pub expires_at: i64,
    pub order_id: String,
}

pub struct NewPlayerPack<'a> {
    pub id: &'a str,
    pub player_id: &'a str,
    pub plan_id: &'a str,
    pub kind: &'a str,
    pub name: &'a str,
    pub price_minor: i64,
    pub minutes: Option<i64>,
    pub validity_days: i64,
    pub window_start_min: Option<i64>,
    pub window_end_min: Option<i64>,
    pub group_ids: &'a [String],
    pub bought_at: i64,
    pub expires_at: i64,
    pub order_id: &'a str,
}

const COLUMNS: &str = "id, plan_id, kind, name, price_minor, minutes_total, minutes_left, \
                       validity_days, window_start_min, window_end_min, group_ids, bought_at, \
                       expires_at, order_id";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlayerPackRow> {
    let groups: String = r.get(10)?;
    Ok(PlayerPackRow {
        id: r.get(0)?,
        plan_id: r.get(1)?,
        kind: r.get(2)?,
        name: r.get(3)?,
        price_minor: r.get(4)?,
        minutes_total: r.get(5)?,
        minutes_left: r.get(6)?,
        validity_days: r.get(7)?,
        window_start_min: r.get(8)?,
        window_end_min: r.get(9)?,
        group_ids: groups
            .split(',')
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect(),
        bought_at: r.get(11)?,
        expires_at: r.get(12)?,
        order_id: r.get(13)?,
    })
}

pub fn insert_player_pack(conn: &Connection, p: &NewPlayerPack<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO player_packs (id, player_id, plan_id, kind, name, price_minor, minutes_total,
                                   minutes_left, validity_days, window_start_min, window_end_min,
                                   group_ids, bought_at, expires_at, order_id, rev, created_at,
                                   updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, 0, 0)",
        params![
            p.id,
            p.player_id,
            p.plan_id,
            p.kind,
            p.name,
            p.price_minor,
            p.minutes,
            p.validity_days,
            p.window_start_min,
            p.window_end_min,
            p.group_ids.join(","),
            p.bought_at,
            p.expires_at,
            p.order_id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn list_player_packs(
    conn: &Connection,
    player_id: &str,
) -> Result<Vec<PlayerPackRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM player_packs WHERE player_id = ?1
             ORDER BY expires_at DESC, id DESC"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([player_id], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn find_pack_for_order(
    conn: &Connection,
    order_id: &str,
) -> Result<Option<PlayerPackRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM player_packs WHERE order_id = ?1"),
        [order_id],
        from,
    )
    .optional()
    .map_err(db_error)
}

pub fn has_unexpired_pass(
    conn: &Connection,
    player_id: &str,
    now_ms: i64,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM player_packs
                        WHERE player_id = ?1 AND kind = 'pass' AND expires_at > ?2)",
        params![player_id, now_ms],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub struct NewPlanLine<'a> {
    pub id: &'a str,
    pub order_id: &'a str,
    pub plan_id: &'a str,
    pub label: &'a str,
    pub minutes: i64,
    pub amount_minor: i64,
    pub at: i64,
}

pub fn insert_plan_line(conn: &Connection, l: &NewPlanLine<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO order_lines (id, order_id, session_segment_id, device_id, from_at, to_at,
                                  minutes, rate_minor_per_hour, amount_minor, position, rev,
                                  created_at, updated_at, plan_id, label)
         VALUES (?1, ?2, NULL, NULL, ?3, ?3, ?4, 0, ?5, 0, 0, 0, 0, ?6, ?7)",
        params![
            l.id,
            l.order_id,
            l.at,
            l.minutes,
            l.amount_minor,
            l.plan_id,
            l.label
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The owned plan `id` when it belongs to `player_id`.
pub fn find_player_pack(
    conn: &Connection,
    player_id: &str,
    id: &str,
) -> Result<Option<PlayerPackRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM player_packs WHERE id = ?1 AND player_id = ?2"),
        params![id, player_id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// Whether a live session is already playing on the owned plan `id`.
pub fn pack_in_use(conn: &Connection, id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sessions
                        WHERE pack_id = ?1 AND (state IN ('active', 'paused', 'grace')
                               OR (state = 'expired' AND ended_at IS NULL)))",
        [id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Takes `minutes` off the owned pack's balance, never below 0.
/// Gives minutes back to a pack (the undo of a forced end).
pub fn restore_pack_minutes(conn: &Connection, id: &str, minutes: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE player_packs SET minutes_left = COALESCE(minutes_left, 0) + ?2 WHERE id = ?1",
        params![id, minutes],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn deduct_pack_minutes(conn: &Connection, id: &str, minutes: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE player_packs SET minutes_left = MAX(COALESCE(minutes_left, 0) - ?2, 0)
         WHERE id = ?1",
        params![id, minutes],
    )
    .map_err(db_error)?;
    Ok(())
}

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub plan_id: String,
    pub kind: String,
    pub name: String,
    pub price_minor: i64,
    pub minutes: Option<i64>,
    pub validity_days: Option<i64>,
    pub window_start_min: Option<i64>,
    pub window_end_min: Option<i64>,
    pub active: bool,
    pub rev: i64,
}

const COLUMNS: &str = "plan_id, kind, name, price_minor, minutes, validity_days, window_start_min, \
                       window_end_min, active, rev";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlanRow> {
    Ok(PlanRow {
        plan_id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        price_minor: r.get(3)?,
        minutes: r.get(4)?,
        validity_days: r.get(5)?,
        window_start_min: r.get(6)?,
        window_end_min: r.get(7)?,
        active: r.get(8)?,
        rev: r.get(9)?,
    })
}

pub fn find_plan(conn: &Connection, id: &str) -> Result<Option<PlanRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM plans WHERE plan_id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_plans(conn: &Connection) -> Result<Vec<PlanRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM plans ORDER BY name COLLATE NOCASE, plan_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn list_plan_groups(conn: &Connection, plan_id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare("SELECT group_id FROM plan_groups WHERE plan_id = ?1 ORDER BY group_id")
        .map_err(db_error)?;
    let rows = stmt.query_map([plan_id], |r| r.get(0)).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_plan(conn: &Connection, p: &PlanRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO plans (plan_id, kind, name, price_minor, minutes, validity_days,
                            window_start_min, window_end_min, active, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0)",
        params![
            p.plan_id,
            p.kind,
            p.name,
            p.price_minor,
            p.minutes,
            p.validity_days,
            p.window_start_min,
            p.window_end_min,
            p.active
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_plan(conn: &Connection, p: &PlanRow) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE plans SET name = ?2, price_minor = ?3, minutes = ?4, validity_days = ?5,
                    window_start_min = ?6, window_end_min = ?7, active = ?8
             WHERE plan_id = ?1",
            params![
                p.plan_id,
                p.name,
                p.price_minor,
                p.minutes,
                p.validity_days,
                p.window_start_min,
                p.window_end_min,
                p.active
            ],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no plans row {}", p.plan_id),
        ))
    }
}

pub fn insert_plan_group(conn: &Connection, plan_id: &str, group_id: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO plan_groups (plan_id, group_id) VALUES (?1, ?2)",
        params![plan_id, group_id],
    )
    .map_err(db_error)?;
    Ok(())
}

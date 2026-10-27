use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingRuleRow {
    pub rule_id: String,
    pub name: String,
    pub group_id: String,
    pub rule_kind: String,
    pub days_mask: Option<i64>,
    pub start_min: Option<i64>,
    pub end_min: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub adjust_kind: String,
    pub adjust_value: i64,
    pub priority: i64,
    pub active: bool,
    pub rev: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "rule_id, name, group_id, rule_kind, days_mask, start_min, end_min, \
                       date_from, date_to, adjust_kind, adjust_value, priority, active, rev, \
                       updated_at";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<PricingRuleRow> {
    Ok(PricingRuleRow {
        rule_id: r.get(0)?,
        name: r.get(1)?,
        group_id: r.get(2)?,
        rule_kind: r.get(3)?,
        days_mask: r.get(4)?,
        start_min: r.get(5)?,
        end_min: r.get(6)?,
        date_from: r.get(7)?,
        date_to: r.get(8)?,
        adjust_kind: r.get(9)?,
        adjust_value: r.get(10)?,
        priority: r.get(11)?,
        active: r.get(12)?,
        rev: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

pub fn find_pricing_rule(conn: &Connection, id: &str) -> Result<Option<PricingRuleRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM pricing_rules WHERE rule_id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

pub fn list_pricing_rules(conn: &Connection) -> Result<Vec<PricingRuleRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM pricing_rules ORDER BY name COLLATE NOCASE, rule_id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

pub fn insert_pricing_rule(conn: &Connection, r: &PricingRuleRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO pricing_rules (rule_id, name, group_id, rule_kind, days_mask, start_min,
                                    end_min, date_from, date_to, adjust_kind, adjust_value,
                                    priority, active, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, 0, 0)",
        params![
            r.rule_id,
            r.name,
            r.group_id,
            r.rule_kind,
            r.days_mask,
            r.start_min,
            r.end_min,
            r.date_from,
            r.date_to,
            r.adjust_kind,
            r.adjust_value,
            r.priority,
            r.active
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_pricing_rule(conn: &Connection, r: &PricingRuleRow) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE pricing_rules SET name = ?2, group_id = ?3, rule_kind = ?4, days_mask = ?5,
                    start_min = ?6, end_min = ?7, date_from = ?8, date_to = ?9, adjust_kind = ?10,
                    adjust_value = ?11, priority = ?12, active = ?13
             WHERE rule_id = ?1",
            params![
                r.rule_id,
                r.name,
                r.group_id,
                r.rule_kind,
                r.days_mask,
                r.start_min,
                r.end_min,
                r.date_from,
                r.date_to,
                r.adjust_kind,
                r.adjust_value,
                r.priority,
                r.active
            ],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no pricing_rules row {}", r.rule_id),
        ))
    }
}

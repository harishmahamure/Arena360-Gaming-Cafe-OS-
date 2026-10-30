//! What the branch summary (Story 8.4) reads, and the `rollup_log` row a publish writes.
//!
//! Reads only: a handful of queries over stamped columns, counts and the owner-editable
//! configuration tables. Nothing here reads a transaction row's contents or a player's data.

use arena_proto::{
    AppError, CatalogueDevice, CatalogueRow, CatalogueStaff, SUMMARY_LIST_CAP, SummaryCatalogue,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::coupons::list_coupon_targets;
use crate::plans::list_plan_groups;
use crate::products::list_components;
use crate::{Store, db_error};

/// The latest business day with a daily-rollup row (`yyyy-mm-dd`), if any (Story 8.6).
pub fn last_rollup_date(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT MAX(date) FROM rollup_log
         WHERE date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// A row of `rollup_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollupLogRow {
    pub date: String,
    pub schema: u32,
    pub built_at: i64,
    pub key: String,
    pub rev: i64,
}

/// A shift closed lately, with its counted variance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedShiftVariance {
    pub shift_id: String,
    pub staff_name: String,
    pub opened_at: i64,
    pub closed_at: i64,
    pub variance_minor: i64,
}

pub fn find_rollup_log(conn: &Connection, date: &str) -> Result<Option<RollupLogRow>, AppError> {
    conn.query_row(
        "SELECT date, schema, built_at, key, rev FROM rollup_log WHERE date = ?1",
        [date],
        |r| {
            Ok(RollupLogRow {
                date: r.get(0)?,
                schema: r.get(1)?,
                built_at: r.get(2)?,
                key: r.get(3)?,
                rev: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Opens the `date` row (`rev` and the stamps are set by the apply step that follows).
pub fn insert_rollup_log(
    conn: &Connection,
    date: &str,
    schema: u32,
    built_at: i64,
    key: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO rollup_log (date, schema, built_at, key, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 0)",
        params![date, schema, built_at, key],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_rollup_log(
    conn: &Connection,
    date: &str,
    schema: u32,
    built_at: i64,
    key: &str,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE rollup_log SET schema = ?2, built_at = ?3, key = ?4 WHERE date = ?1",
        params![date, schema, built_at, key],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Sessions that are running: active, paused, in grace, or expired and not yet ended.
pub fn count_live_sessions(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM sessions
         WHERE state IN ('active', 'paused', 'grace') OR (state = 'expired' AND ended_at IS NULL)",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Shifts closed at or after `since` with a recorded variance, newest first.
pub fn closed_shift_variances(
    conn: &Connection,
    since: i64,
) -> Result<Vec<ClosedShiftVariance>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, COALESCE(st.display_name, s.staff_id), s.closed_at, s.variance_minor,
                    s.opened_at
             FROM shifts s LEFT JOIN staff st ON st.staff_id = s.staff_id
             WHERE s.state = 'closed' AND s.closed_at >= ?1 AND s.variance_minor IS NOT NULL
             ORDER BY s.closed_at DESC, s.id LIMIT ?2",
        )
        .map_err(db_error)?;
    stmt.query_map(params![since, SUMMARY_LIST_CAP as i64], |r| {
        Ok(ClosedShiftVariance {
            shift_id: r.get(0)?,
            staff_name: r.get(1)?,
            opened_at: r.get(4)?,
            closed_at: r.get(2)?,
            variance_minor: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

fn rows(conn: &Connection, sql: &str) -> Result<Vec<CatalogueRow>, AppError> {
    editable_rows(conn, sql, |_| Ok(None))
}

/// The columns after the fifth, as the `fields` object: a NULL column is left out, as the upsert
/// payload leaves an absent optional field out.
fn fields_of(r: &rusqlite::Row<'_>, names: &[&str]) -> rusqlite::Result<Option<Value>> {
    let mut fields = serde_json::Map::new();
    for (i, name) in names.iter().enumerate() {
        let v: rusqlite::types::Value = r.get(5 + i)?;
        match v {
            rusqlite::types::Value::Null => {}
            rusqlite::types::Value::Integer(n) => {
                fields.insert((*name).to_owned(), n.into());
            }
            rusqlite::types::Value::Text(t) => {
                fields.insert((*name).to_owned(), t.into());
            }
            other => {
                return Err(rusqlite::Error::InvalidColumnType(
                    5 + i,
                    (*name).to_owned(),
                    other.data_type(),
                ));
            }
        }
    }
    Ok(Some(Value::Object(fields)))
}

fn editable_rows(
    conn: &Connection,
    sql: &str,
    fields: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Option<Value>>,
) -> Result<Vec<CatalogueRow>, AppError> {
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    stmt.query_map(params![SUMMARY_LIST_CAP as i64], |r| {
        Ok(CatalogueRow {
            id: r.get(0)?,
            name: r.get(1)?,
            rev: r.get(2)?,
            active: r.get(3)?,
            detail: r.get(4)?,
            fields: fields(r)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Adds `key` (a list the row's table does not hold in a column) to a row's `fields`.
fn with_list(row: &mut CatalogueRow, key: &str, list: Value) {
    if let Some(Value::Object(fields)) = row.fields.as_mut() {
        fields.insert(key.to_owned(), list);
    }
}

/// Products (or bundles) with their editable fields: the upsert payload without `id`, `active`,
/// the image (too large for a summary: an owner command without one keeps the stored image) and
/// the kitchen notes, plus `low_stock_qty`, which `stock.threshold` sets.
fn product_rows(conn: &Connection, bundle: bool) -> Result<Vec<CatalogueRow>, AppError> {
    let sql = format!(
        "SELECT product_id, name, rev, active, category, category, price_minor, tax_rate_bp, hsn,
                track_stock, barcode, kitchen, favourite, bundle, low_stock_qty
         FROM products WHERE deleted_at IS NULL AND bundle = {}
         ORDER BY product_id LIMIT ?1",
        i32::from(bundle)
    );
    editable_rows(conn, &sql, |r| {
        fields_of(
            r,
            &[
                "category",
                "price_minor",
                "tax_rate_bp",
                "hsn",
                "track_stock",
                "barcode",
                "kitchen",
                "favourite",
                "bundle",
                "low_stock_qty",
            ],
        )
    })
}

/// The 0/1 columns of a product's `fields` as the booleans the upsert payload holds.
fn to_flags(row: &mut CatalogueRow) {
    if let Some(Value::Object(fields)) = row.fields.as_mut() {
        for key in ["track_stock", "kitchen", "favourite", "bundle"] {
            if let Some(n) = fields.get(key).and_then(Value::as_i64) {
                fields.insert(key.to_owned(), (n != 0).into());
            }
        }
    }
}

/// The owner-editable configuration: names, `rev`, flags. No credentials, no transactions, no
/// player data.
pub fn catalogue(conn: &Connection) -> Result<SummaryCatalogue, AppError> {
    let groups = rows(
        conn,
        "SELECT group_id, name, rev, NULL, kind FROM device_groups
         WHERE deleted_at IS NULL ORDER BY sort_order, group_id LIMIT ?1",
    )?;
    let rates = editable_rows(
        conn,
        "SELECT r.rate_id, COALESCE(g.name, r.group_id), r.rev, r.active,
                CAST(r.price_per_hour_minor AS TEXT), r.group_id, r.price_per_hour_minor
         FROM rates r LEFT JOIN device_groups g ON g.group_id = r.group_id
         ORDER BY r.rate_id LIMIT ?1",
        |r| fields_of(r, &["group_id", "price_per_hour_minor"]),
    )?;
    let mut plans = editable_rows(
        conn,
        "SELECT plan_id, name, rev, active, kind, kind, price_minor, minutes, validity_days,
                window_start_min, window_end_min
         FROM plans ORDER BY plan_id LIMIT ?1",
        |r| {
            fields_of(
                r,
                &[
                    "plan_kind",
                    "price_minor",
                    "minutes",
                    "validity_days",
                    "window_start_min",
                    "window_end_min",
                ],
            )
        },
    )?;
    for plan in &mut plans {
        let groups = list_plan_groups(conn, &plan.id)?;
        with_list(plan, "group_ids", json!(groups));
    }
    let pricing_rules = editable_rows(
        conn,
        "SELECT rule_id, name, rev, active, rule_kind, group_id, rule_kind, days_mask, start_min,
                end_min, date_from, date_to, adjust_kind, adjust_value, priority
         FROM pricing_rules ORDER BY priority, rule_id LIMIT ?1",
        |r| {
            fields_of(
                r,
                &[
                    "group_id",
                    "rule_kind",
                    "days_mask",
                    "start_min",
                    "end_min",
                    "date_from",
                    "date_to",
                    "adjust_kind",
                    "adjust_value",
                    "priority",
                ],
            )
        },
    )?;
    let mut coupons = editable_rows(
        conn,
        "SELECT coupon_id, code, rev, active, NULL, percent_bp, amount_minor, valid_from, valid_to,
                usage_limit, min_bill_minor
         FROM coupons ORDER BY coupon_id LIMIT ?1",
        |r| {
            fields_of(
                r,
                &[
                    "percent_bp",
                    "amount_minor",
                    "valid_from",
                    "valid_to",
                    "usage_limit",
                    "min_bill_minor",
                ],
            )
        },
    )?;
    for coupon in &mut coupons {
        let targets: Vec<Value> = list_coupon_targets(conn, &coupon.id)?
            .into_iter()
            .map(|(kind, id)| json!({ "kind": kind, "id": id }))
            .collect();
        with_list(coupon, "targets", json!(targets));
    }
    let mut products = product_rows(conn, false)?;
    for product in &mut products {
        to_flags(product);
    }
    let mut bundles = product_rows(conn, true)?;
    for bundle in &mut bundles {
        to_flags(bundle);
        let parts: Vec<Value> = list_components(conn, &bundle.id)?
            .into_iter()
            .map(|(product_id, qty)| json!({ "product_id": product_id, "qty": qty }))
            .collect();
        with_list(bundle, "components", json!(parts));
    }
    let roles = rows(
        conn,
        "SELECT role_id, name, rev, NULL, CASE WHEN builtin = 1 THEN 'builtin' END
         FROM roles ORDER BY role_id LIMIT ?1",
    )?;
    let staff = {
        let mut stmt = conn
            .prepare(
                "SELECT s.staff_id, s.display_name, COALESCE(r.name, s.role_id), s.active, s.rev
                 FROM staff s LEFT JOIN roles r ON r.role_id = s.role_id
                 ORDER BY s.staff_id LIMIT ?1",
            )
            .map_err(db_error)?;
        stmt.query_map(params![SUMMARY_LIST_CAP as i64], |r| {
            Ok(CatalogueStaff {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
                active: r.get(3)?,
                rev: r.get(4)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?
    };
    let devices = {
        let mut stmt = conn
            .prepare(
                "SELECT m.device_id, m.label, m.group_id, m.kind, d.revoked_at IS NOT NULL, m.rev
                 FROM devices_meta m LEFT JOIN devices d ON d.device_id = m.device_id
                 WHERE m.deleted_at IS NULL ORDER BY m.device_id LIMIT ?1",
            )
            .map_err(db_error)?;
        stmt.query_map(params![SUMMARY_LIST_CAP as i64], |r| {
            Ok(CatalogueDevice {
                id: r.get(0)?,
                name: r.get(1)?,
                group_id: r.get(2)?,
                kind: r.get(3)?,
                revoked: r.get(4)?,
                rev: r.get(5)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?
    };
    Ok(SummaryCatalogue {
        groups,
        rates,
        plans,
        pricing_rules,
        coupons,
        products,
        bundles,
        staff,
        roles,
        devices,
    })
}

impl Store {
    /// The days (`yyyy-mm-dd`) whose published rollup has a `schema` below `schema`, oldest
    /// first, at most `limit` (Story 8.7). The log itself is the resume point: a rebuilt day's row
    /// carries the new schema and drops out.
    pub fn rollup_days_below(&self, schema: u32, limit: usize) -> Result<Vec<String>, AppError> {
        if !self.table_exists("rollup_log")? {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT date FROM rollup_log
                 WHERE schema < ?1
                   AND date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                 ORDER BY date LIMIT ?2",
            )
            .map_err(db_error)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        stmt.query_map(params![schema, limit], |r| r.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)
    }

    /// The `rollup_log` row of `date`, if the Hub has published it.
    pub fn rollup_log(&self, date: &str) -> Result<Option<RollupLogRow>, AppError> {
        if !self.table_exists("rollup_log")? {
            return Ok(None);
        }
        find_rollup_log(&self.conn, date)
    }

    /// The open shift, if any.
    pub fn current_shift(&self) -> Result<Option<crate::ShiftRow>, AppError> {
        crate::open_shift(&self.conn)
    }

    /// The cash the drawer should hold for the shift: every cash event against it.
    pub fn shift_cash_minor(&self, shift_id: &str) -> Result<i64, AppError> {
        crate::expected_cash(&self.conn, shift_id)
    }

    /// The variance the branch tolerates before a close needs an approver.
    pub fn shift_variance_threshold(&self) -> Result<i64, AppError> {
        crate::variance_threshold(&self.conn)
    }

    /// Sessions that are running now.
    pub fn live_session_count(&self) -> Result<i64, AppError> {
        count_live_sessions(&self.conn)
    }

    /// Shifts closed at or after `since` with their variance.
    pub fn recent_shift_variances(&self, since: i64) -> Result<Vec<ClosedShiftVariance>, AppError> {
        closed_shift_variances(&self.conn, since)
    }

    /// The owner-editable configuration, for the branch summary.
    pub fn summary_catalogue(&self) -> Result<SummaryCatalogue, AppError> {
        catalogue(&self.conn)
    }
}

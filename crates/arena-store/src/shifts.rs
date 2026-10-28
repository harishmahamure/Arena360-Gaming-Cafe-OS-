use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// The shift that has the drawer open, with its opener's display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftRow {
    pub id: String,
    pub staff_id: String,
    pub staff_name: String,
    pub opened_at: i64,
    pub float_minor: i64,
    pub carried_minor: i64,
}

/// A `sale` cash event that no compensating row has cancelled yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSale {
    pub id: String,
    pub shift_id: String,
    pub amount_minor: i64,
}

pub struct NewShift<'a> {
    pub id: &'a str,
    pub staff_id: &'a str,
    pub opened_at: i64,
    pub float_minor: i64,
    pub carried_minor: i64,
}

pub struct NewCashEvent<'a> {
    pub id: &'a str,
    pub shift_id: &'a str,
    pub kind: &'a str,
    pub amount_minor: i64,
    pub staff_id: &'a str,
    pub order_id: Option<&'a str>,
    pub reverses: Option<&'a str>,
}

/// The branch's open shift, if any (at most one: the drawer is single).
pub fn open_shift(conn: &Connection) -> Result<Option<ShiftRow>, AppError> {
    conn.query_row(
        "SELECT s.id, s.staff_id, COALESCE(st.display_name, s.staff_id), s.opened_at,
                s.float_minor, s.carried_minor
         FROM shifts s LEFT JOIN staff st ON st.staff_id = s.staff_id
         WHERE s.state = 'open'",
        [],
        |r| {
            Ok(ShiftRow {
                id: r.get(0)?,
                staff_id: r.get(1)?,
                staff_name: r.get(2)?,
                opened_at: r.get(3)?,
                float_minor: r.get(4)?,
                carried_minor: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The id of the open shift, if any.
pub fn open_shift_id(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT id FROM shifts WHERE state = 'open'", [], |r| {
        r.get(0)
    })
    .optional()
    .map_err(db_error)
}

/// The float the next shift offers (0 before the first shift): the latest shift's counted cash
/// less its deposit at close once it is closed (Story 5.26), else its opening float.
pub fn last_opening_float(conn: &Connection) -> Result<i64, AppError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(next_float_minor, float_minor) FROM shifts
             ORDER BY opened_at DESC, id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(found.unwrap_or(0))
}

pub fn insert_shift(conn: &Connection, s: &NewShift<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state, rev,
                             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', 0, 0, 0)",
        params![
            s.id,
            s.staff_id,
            s.opened_at,
            s.float_minor,
            s.carried_minor
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_cash_event(conn: &Connection, e: &NewCashEvent<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO cash_events (id, shift_id, kind, amount_minor, staff_id, order_id, reverses,
                                  rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
        params![
            e.id,
            e.shift_id,
            e.kind,
            e.amount_minor,
            e.staff_id,
            e.order_id,
            e.reverses
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// How many cash events the shift has (its float counts as one).
pub fn count_cash_events(conn: &Connection, shift_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM cash_events WHERE shift_id = ?1",
        [shift_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The shift the order's cash was taken against (its first `sale` event), if it took any.
pub fn shift_of_order_sale(conn: &Connection, order_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT shift_id FROM cash_events WHERE order_id = ?1 AND kind = ?2
         ORDER BY created_at, id LIMIT 1",
        params![order_id, arena_proto::CASH_SALE],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

/// The order's `sale` cash events that no `sale_reverted` row has cancelled.
pub fn open_sales_of_order(conn: &Connection, order_id: &str) -> Result<Vec<OpenSale>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.shift_id, e.amount_minor FROM cash_events e
             WHERE e.order_id = ?1 AND e.kind = ?2
               AND NOT EXISTS (SELECT 1 FROM cash_events r WHERE r.reverses = e.id)
             ORDER BY e.created_at, e.id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![order_id, arena_proto::CASH_SALE], |r| {
        Ok(OpenSale {
            id: r.get(0)?,
            shift_id: r.get(1)?,
            amount_minor: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// A deposit or an expense (Story 5.25): a cash event with its detail columns.
pub struct NewCashMove<'a> {
    pub id: &'a str,
    pub shift_id: &'a str,
    pub kind: &'a str,
    /// The drawer's effect: negative for a deposit or a drawer expense, 0 for an owner expense.
    pub amount_minor: i64,
    pub staff_id: &'a str,
    pub reference: Option<&'a str>,
    pub category: Option<&'a str>,
    pub paid_from: Option<&'a str>,
    pub spent_minor: Option<i64>,
    pub note: Option<&'a str>,
    pub receipt_photo: Option<&'a str>,
}

pub fn insert_cash_move(conn: &Connection, e: &NewCashMove<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO cash_events (id, shift_id, kind, amount_minor, staff_id, reference,
                                  category, paid_from, spent_minor, note, receipt_photo,
                                  rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0, 0)",
        params![
            e.id,
            e.shift_id,
            e.kind,
            e.amount_minor,
            e.staff_id,
            e.reference,
            e.category,
            e.paid_from,
            e.spent_minor,
            e.note,
            e.receipt_photo
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// What closes a shift (Story 5.26).
pub struct ShiftClosing<'a> {
    pub shift_id: &'a str,
    pub closed_at: i64,
    pub counted_minor: i64,
    pub expected_minor: i64,
    pub variance_minor: i64,
    pub variance_reason: Option<&'a str>,
    pub variance_note: Option<&'a str>,
    pub closed_by: &'a str,
    pub approver_id: Option<&'a str>,
    pub next_float_minor: i64,
}

/// Closes the open shift with its count; the caller stamps the row.
pub fn close_shift(conn: &Connection, c: &ShiftClosing<'_>) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE shifts SET state = 'closed', closed_at = ?2, counted_minor = ?3,
                    expected_minor = ?4, variance_minor = ?5, variance_reason = ?6,
                    variance_note = ?7, closed_by = ?8, approver_id = ?9, next_float_minor = ?10
             WHERE id = ?1 AND state = 'open'",
            params![
                c.shift_id,
                c.closed_at,
                c.counted_minor,
                c.expected_minor,
                c.variance_minor,
                c.variance_reason,
                c.variance_note,
                c.closed_by,
                c.approver_id,
                c.next_float_minor
            ],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            arena_proto::ErrorCode::ShiftNotOpen,
            "There is no shift to close.",
        ))
    }
}

/// What made up the expected cash of a shift, by kind (signed by the drawer effect). A reversed
/// sale nets out because its compensating row counts with the sales.
pub fn shift_breakdown(
    conn: &Connection,
    shift_id: &str,
) -> Result<arena_proto::ShiftBreakdown, AppError> {
    let sum = |kinds: &[&str]| -> Result<i64, AppError> {
        let marks = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT COALESCE(SUM(amount_minor), 0) FROM cash_events
             WHERE shift_id = ? AND kind IN ({marks})"
        );
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&shift_id];
        values.extend(kinds.iter().map(|k| k as &dyn rusqlite::ToSql));
        conn.query_row(&sql, values.as_slice(), |r| r.get(0))
            .map_err(db_error)
    };
    Ok(arena_proto::ShiftBreakdown {
        float_minor: sum(&[arena_proto::CASH_FLOAT_IN])?,
        cash_sales_minor: sum(&[arena_proto::CASH_SALE, arena_proto::CASH_SALE_REVERTED])?,
        cash_refunds_minor: sum(&[arena_proto::CASH_REFUND])?,
        deposits_minor: sum(&[arena_proto::CASH_DEPOSIT])?,
        expenses_minor: sum(&[arena_proto::CASH_EXPENSE])?,
    })
}

/// The branch's variance threshold in minor units (Settings → Shift); the default when the
/// setting is missing or unreadable.
pub fn variance_threshold(conn: &Connection) -> Result<i64, AppError> {
    let default = || {
        arena_proto::DEFAULT_SHIFT_VARIANCE_THRESHOLD
            .parse::<i64>()
            .unwrap_or(10_000)
    };
    Ok(
        crate::find_setting(conn, arena_proto::KEY_SHIFT_VARIANCE_THRESHOLD)?
            .and_then(|s| s.value.trim().parse::<i64>().ok())
            .filter(|v| *v >= 0)
            .unwrap_or_else(default),
    )
}

/// The reveal for a stated count: the expected cash, the signed difference and whether it is over
/// the threshold. Only ever built from a count, so the expected cash is never read out alone.
pub fn shift_count(
    conn: &Connection,
    shift_id: &str,
    counted_minor: i64,
) -> Result<arena_proto::ShiftCount, AppError> {
    let expected_minor = expected_cash(conn, shift_id)?;
    let threshold_minor = variance_threshold(conn)?;
    let difference_minor = counted_minor - expected_minor;
    Ok(arena_proto::ShiftCount {
        counted_minor,
        expected_minor,
        difference_minor,
        threshold_minor,
        over_threshold: difference_minor.abs() > threshold_minor,
        breakdown: shift_breakdown(conn, shift_id)?,
    })
}

/// The cash a shift's drawer should hold: float plus cash sales, less cash refunds, deposits
/// and drawer expenses. It is the sum of the shift's `cash_events`, so an Undo's compensating
/// row nets the sale it cancels. Never shown to staff before the blind count.
pub fn expected_cash(conn: &Connection, shift_id: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_minor), 0) FROM cash_events WHERE shift_id = ?1",
        [shift_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

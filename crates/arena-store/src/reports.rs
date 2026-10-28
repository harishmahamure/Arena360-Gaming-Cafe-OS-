//! The rows the Reports screen is built from (Story 5.27). Everything is read by the stamped
//! `business_day` columns and the stored totals; nothing is recomputed from rates. A bill whose
//! settle was undone is open again and a voided one is `void`, so only `settled` orders count and
//! reversed pairs net to zero by construction. The aggregation (local hours, utilisation) lives in
//! `arena-core`, which knows the branch time zone.

use arena_proto::AppError;
use rusqlite::{Connection, params};

use crate::db_error;

/// A settled sales bill: a session bill, a plan sale or a counter sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesOrderRow {
    pub id: String,
    /// `session`, `plan_sale` or `pos_sale`.
    pub kind: String,
    pub total_minor: i64,
    pub tax_minor: i64,
    pub coupon_minor: i64,
    pub discount_minor: i64,
    /// When the bill settled (UTC ms).
    pub settled_at: i64,
    /// The device group the bill belongs to; empty for a counter sale with no device.
    pub group_id: String,
}

/// One line of a settled sales bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesLineRow {
    pub order_id: String,
    pub amount_minor: i64,
    pub qty: i64,
    pub is_product: bool,
    pub is_plan: bool,
    /// The product's category; empty when the product is gone or the line is not a product.
    pub category: String,
}

/// A wallet top-up or credit collection settled in the range: a payment, not a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedRow {
    pub kind: String,
    pub amount_minor: i64,
    pub count: i64,
}

/// A session that started in the range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReportRow {
    pub id: String,
    pub device_id: String,
    pub group_id: String,
    /// Minutes billed on the session's time lines; 0 when the bill has none (a pack or pass).
    pub billed_min: i64,
    /// Play time billed to the session's settled bill, in minor units.
    pub revenue_minor: i64,
}

/// One slice of a session's time on one device (UTC ms); `to_at` is the end, or `now` while open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSpan {
    pub session_id: String,
    pub group_id: String,
    pub from_at: i64,
    pub to_at: i64,
}

/// A device that can be sold time: its label and group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDevice {
    pub device_id: String,
    pub label: String,
    pub group_id: String,
}

pub fn sales_orders(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<SalesOrderRow>, AppError> {
    sales_orders_where(conn, "o.business_day BETWEEN ?1 AND ?2", &from, &to)
}

/// The sales settled from `from_ms` to `to_ms` inclusive (UTC ms), by settle time: the bills of one
/// shift (Story 5.28).
pub fn sales_orders_between(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<SalesOrderRow>, AppError> {
    sales_orders_where(conn, "o.settled_at BETWEEN ?1 AND ?2", &from_ms, &to_ms)
}

fn sales_orders_where(
    conn: &Connection,
    cond: &str,
    a: &dyn rusqlite::ToSql,
    b: &dyn rusqlite::ToSql,
) -> Result<Vec<SalesOrderRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT o.id, o.kind, o.total_minor, o.tax_minor, COALESCE(o.coupon_minor, 0),
                    COALESCE(o.discount_minor, 0), COALESCE(o.settled_at, o.created_at),
                    COALESCE(s.group_id, es.group_id, dm.group_id, '')
             FROM orders o
             LEFT JOIN sessions s ON s.id = o.session_id
             LEFT JOIN sessions es ON es.id = o.extends_session_id
             LEFT JOIN devices_meta dm ON dm.device_id = o.device_id
             WHERE o.status = 'settled' AND o.kind IN ('session', 'plan_sale', 'pos_sale')
               AND {cond}",
        ))
        .map_err(db_error)?;
    stmt.query_map(params![a, b], |r| {
        Ok(SalesOrderRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            total_minor: r.get(2)?,
            tax_minor: r.get(3)?,
            coupon_minor: r.get(4)?,
            discount_minor: r.get(5)?,
            settled_at: r.get(6)?,
            group_id: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn sales_lines(conn: &Connection, from: &str, to: &str) -> Result<Vec<SalesLineRow>, AppError> {
    sales_lines_where(conn, "o.business_day BETWEEN ?1 AND ?2", &from, &to)
}

/// The lines of the sales settled from `from_ms` to `to_ms` inclusive (Story 5.28).
pub fn sales_lines_between(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<SalesLineRow>, AppError> {
    sales_lines_where(conn, "o.settled_at BETWEEN ?1 AND ?2", &from_ms, &to_ms)
}

fn sales_lines_where(
    conn: &Connection,
    cond: &str,
    a: &dyn rusqlite::ToSql,
    b: &dyn rusqlite::ToSql,
) -> Result<Vec<SalesLineRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT l.order_id, l.amount_minor, COALESCE(l.qty, 1), l.product_id IS NOT NULL,
                    l.plan_id IS NOT NULL, COALESCE(p.category, '')
             FROM order_lines l
             JOIN orders o ON o.id = l.order_id
             LEFT JOIN products p ON p.product_id = l.product_id
             WHERE o.status = 'settled' AND o.kind IN ('session', 'plan_sale', 'pos_sale')
               AND {cond}",
        ))
        .map_err(db_error)?;
    stmt.query_map(params![a, b], |r| {
        Ok(SalesLineRow {
            order_id: r.get(0)?,
            amount_minor: r.get(1)?,
            qty: r.get(2)?,
            is_product: r.get(3)?,
            is_plan: r.get(4)?,
            category: r.get(5)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Top-ups and credit collections settled in the range, one row per kind.
pub fn received(conn: &Connection, from: &str, to: &str) -> Result<Vec<ReceivedRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, COALESCE(SUM(total_minor), 0), COUNT(*) FROM orders
             WHERE status = 'settled' AND kind IN ('topup', 'credit_collect')
               AND business_day BETWEEN ?1 AND ?2
             GROUP BY kind",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(ReceivedRow {
            kind: r.get(0)?,
            amount_minor: r.get(1)?,
            count: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// What credit notes issued in the range paid back.
pub fn refunded(conn: &Connection, from: &str, to: &str) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(SUM(amount_minor), 0) FROM refunds WHERE business_day BETWEEN ?1 AND ?2",
        params![from, to],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The name of every device group, keyed by id (deleted groups too, so old sales keep a name).
pub fn group_names(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT group_id, name FROM device_groups ORDER BY sort_order, name")
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

/// The sessions that started in the range, with the time and play-time revenue on their bills.
pub fn session_rows(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<SessionReportRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.device_id, s.group_id,
                    COALESCE(SUM(l.minutes), 0), COALESCE(SUM(l.amount_minor), 0)
             FROM sessions s
             LEFT JOIN orders o ON o.session_id = s.id AND o.status = 'settled'
             LEFT JOIN order_lines l ON l.order_id = o.id
                                    AND l.product_id IS NULL AND l.plan_id IS NULL
             WHERE s.business_day BETWEEN ?1 AND ?2 AND s.state <> 'orphaned'
             GROUP BY s.id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(SessionReportRow {
            id: r.get(0)?,
            device_id: r.get(1)?,
            group_id: r.get(2)?,
            billed_min: r.get(3)?,
            revenue_minor: r.get(4)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The time slices of the sessions that started in the range; an open slice runs to `now_ms`.
pub fn segment_spans(
    conn: &Connection,
    from: &str,
    to: &str,
    now_ms: i64,
) -> Result<Vec<SegmentSpan>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT g.session_id, s.group_id, g.from_at, COALESCE(g.to_at, ?3)
             FROM session_segments g JOIN sessions s ON s.id = g.session_id
             WHERE s.business_day BETWEEN ?1 AND ?2 AND s.state <> 'orphaned'",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from, to, now_ms], |r| {
        Ok(SegmentSpan {
            session_id: r.get(0)?,
            group_id: r.get(1)?,
            from_at: r.get(2)?,
            to_at: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Every device that is still in the branch and sits in a group.
pub fn report_devices(conn: &Connection) -> Result<Vec<ReportDevice>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT device_id, label, group_id FROM devices_meta
             WHERE deleted_at IS NULL AND group_id IS NOT NULL ORDER BY label, device_id",
        )
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(ReportDevice {
            device_id: r.get(0)?,
            label: r.get(1)?,
            group_id: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// What a player settled in the range (Story 5.30), with the sealed fields their name is opened
/// from. `name_enc` / `dek_wrapped` are `None` when the player row or its data key is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSalesRow {
    pub player_id: String,
    /// Distinct business days with a settled sale in the range.
    pub visits: i64,
    pub spend_minor: i64,
    /// The business day of the player's first settled sale ever.
    pub first_day: String,
    pub name_enc: Option<String>,
    pub dek_wrapped: Option<String>,
}

/// Bills settled in the range with no player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuestSalesRow {
    pub bills: i64,
    pub spend_minor: i64,
}

/// A player with credit outstanding, aged first in, first out as of a business day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebtorRow {
    pub player_id: String,
    pub outstanding_minor: i64,
    pub aging: arena_proto::CreditAging,
    pub name_enc: Option<String>,
    pub dek_wrapped: Option<String>,
}

const SALE_KINDS: &str = "('session', 'plan_sale', 'pos_sale')";

/// Per player: visits, spend and first settled sale ever, for the sales settled in the range.
pub fn player_sales(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<PlayerSalesRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "WITH firsts AS (
                 SELECT player_id, MIN(business_day) AS first_day FROM orders
                 WHERE status = 'settled' AND kind IN {SALE_KINDS} AND player_id IS NOT NULL
                 GROUP BY player_id)
             SELECT o.player_id, COUNT(DISTINCT o.business_day), COALESCE(SUM(o.total_minor), 0),
                    f.first_day, p.name_enc, k.dek_wrapped
             FROM orders o
             JOIN firsts f ON f.player_id = o.player_id
             LEFT JOIN players p ON p.player_id = o.player_id AND p.deleted_at IS NULL
             LEFT JOIN player_keys k ON k.player_id = o.player_id
             WHERE o.status = 'settled' AND o.kind IN {SALE_KINDS} AND o.player_id IS NOT NULL
               AND o.business_day BETWEEN ?1 AND ?2
             GROUP BY o.player_id",
        ))
        .map_err(db_error)?;
    stmt.query_map(params![from, to], |r| {
        Ok(PlayerSalesRow {
            player_id: r.get(0)?,
            visits: r.get(1)?,
            spend_minor: r.get(2)?,
            first_day: r.get(3)?,
            name_enc: r.get(4)?,
            dek_wrapped: r.get(5)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// The bills settled in the range with no player.
pub fn guest_sales(conn: &Connection, from: &str, to: &str) -> Result<GuestSalesRow, AppError> {
    conn.query_row(
        &format!(
            "SELECT COUNT(*), COALESCE(SUM(total_minor), 0) FROM orders
             WHERE status = 'settled' AND kind IN {SALE_KINDS} AND player_id IS NULL
               AND business_day BETWEEN ?1 AND ?2"
        ),
        params![from, to],
        |r| {
            Ok(GuestSalesRow {
                bills: r.get(0)?,
                spend_minor: r.get(1)?,
            })
        },
    )
    .map_err(db_error)
}

/// Every player with credit outstanding, aged as of the business day `today`.
pub fn debtors(conn: &Connection, today: &str) -> Result<Vec<DebtorRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.player_id, c.outstanding_minor, p.name_enc, k.dek_wrapped
             FROM credit_balances c
             LEFT JOIN players p ON p.player_id = c.player_id AND p.deleted_at IS NULL
             LEFT JOIN player_keys k ON k.player_id = c.player_id
             WHERE c.outstanding_minor > 0 ORDER BY c.player_id",
        )
        .map_err(db_error)?;
    let rows: Vec<(String, i64, Option<String>, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    rows.into_iter()
        .map(|(player_id, outstanding_minor, name_enc, dek_wrapped)| {
            Ok(DebtorRow {
                aging: crate::credit::credit_aging(conn, &player_id, today)?,
                player_id,
                outstanding_minor,
                name_enc,
                dek_wrapped,
            })
        })
        .collect()
}

/// One staff member's count and value in a Staff figure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffCountRow {
    pub staff_id: String,
    pub count: i64,
    pub value_minor: i64,
}

/// A closed shift and its signed variance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffShiftRow {
    pub staff_id: String,
    pub shift_id: String,
    pub closed_at: i64,
    pub variance_minor: i64,
}

fn staff_counts(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<StaffCountRow>, AppError> {
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    stmt.query_map(params, |r| {
        Ok(StaffCountRow {
            staff_id: r.get(0)?,
            count: r.get(1)?,
            value_minor: r.get(2)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// Settled sales by the staff member who settled them.
pub fn staff_sales(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<StaffCountRow>, AppError> {
    staff_counts(
        conn,
        &format!(
            "SELECT settled_by, COUNT(*), COALESCE(SUM(total_minor), 0) FROM orders
             WHERE status = 'settled' AND kind IN {SALE_KINDS} AND settled_by IS NOT NULL
               AND business_day BETWEEN ?1 AND ?2
             GROUP BY settled_by"
        ),
        &[&from, &to],
    )
}

/// Manual discounts by the approving staff member (value in `value_minor`).
pub fn staff_discounts(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<StaffCountRow>, AppError> {
    staff_counts(
        conn,
        &format!(
            "SELECT discount_by, COUNT(*), COALESCE(SUM(discount_minor), 0) FROM orders
             WHERE status = 'settled' AND kind IN {SALE_KINDS} AND discount_minor > 0
               AND discount_by IS NOT NULL AND business_day BETWEEN ?1 AND ?2
             GROUP BY discount_by"
        ),
        &[&from, &to],
    )
}

/// Voided orders by the staff member who voided them, at the order's total.
pub fn staff_voids(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<StaffCountRow>, AppError> {
    staff_counts(
        conn,
        "SELECT voided_by, COUNT(*), COALESCE(SUM(total_minor), 0) FROM orders
         WHERE status = 'void' AND voided_by IS NOT NULL AND business_day BETWEEN ?1 AND ?2
         GROUP BY voided_by",
        &[&from, &to],
    )
}

/// Refunds by the staff member who approved them.
pub fn staff_refunds(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<StaffCountRow>, AppError> {
    staff_counts(
        conn,
        "SELECT approved_by, COUNT(*), COALESCE(SUM(amount_minor), 0) FROM refunds
         WHERE business_day BETWEEN ?1 AND ?2
         GROUP BY approved_by",
        &[&from, &to],
    )
}

/// Sessions that started in the range, by the staff member who inserted them (the actor of the
/// change-log insert; a session has no `started_by` column). Sessions started by a device, a
/// player or the system, and ones since orphaned, are not counted.
pub fn staff_sessions(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<StaffCountRow>, AppError> {
    staff_counts(
        conn,
        "SELECT c.actor_id, COUNT(DISTINCT s.id), 0
         FROM sessions s
         JOIN changes c ON c.tbl = 'sessions' AND c.row_id = s.id AND c.op = 'insert'
                       AND c.actor_kind = 'staff'
         WHERE s.business_day BETWEEN ?1 AND ?2 AND s.state <> 'orphaned'
         GROUP BY c.actor_id",
        &[&from, &to],
    )
}

/// Closed shifts whose `closed_at` is from `from_ms` to `to_ms` inclusive (UTC ms), oldest first.
/// The caller keeps the ones whose local business day is in range.
pub fn closed_shift_variances(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<StaffShiftRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT staff_id, id, closed_at, COALESCE(variance_minor, 0) FROM shifts
             WHERE state = 'closed' AND closed_at BETWEEN ?1 AND ?2
             ORDER BY closed_at, id",
        )
        .map_err(db_error)?;
    stmt.query_map(params![from_ms, to_ms], |r| {
        Ok(StaffShiftRow {
            staff_id: r.get(0)?,
            shift_id: r.get(1)?,
            closed_at: r.get(2)?,
            variance_minor: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

/// `(staff id, display name)` of every staff member.
pub fn staff_names(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare("SELECT staff_id, display_name FROM staff")
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use crate::Store;

    const T: i64 = 1_800_000_000_000;

    /// One order of the fixture.
    struct Order {
        id: &'static str,
        status: &'static str,
        kind: &'static str,
        total: i64,
        day: &'static str,
        player: Option<&'static str>,
        settled_by: Option<&'static str>,
        discount: i64,
        discount_by: &'static str,
        voided_by: &'static str,
    }

    fn order(
        id: &'static str,
        status: &'static str,
        kind: &'static str,
        total: i64,
        day: &'static str,
        player: Option<&'static str>,
        settled_by: Option<&'static str>,
    ) -> Order {
        Order {
            id,
            status,
            kind,
            total,
            day,
            player,
            settled_by,
            discount: 0,
            discount_by: "",
            voided_by: "",
        }
    }

    /// A migrated database holding the Story 5.30 fixture, written with plain SQL: a range of
    /// 2026-10-10 to 2026-10-20 with three players (one erased), a guest, new and returning
    /// players, credit aged 3, 10 and 19 days, and one of every staff figure.
    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("branch.db"), T).unwrap();
        store
            .conn
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 INSERT INTO players (player_id, username, password_hash, phone_lookup, name_enc,
                                      phone_enc, tags, rev, created_at, updated_at)
                 VALUES ('P1', 'user1', 'h', 'l1', 'n1', 'ph1', '[]', 1, 0, 0),
                        ('P2', 'user2', 'h', 'l2', 'n2', 'ph2', '[]', 1, 0, 0),
                        ('P3', 'user3', 'h', 'l3', 'n3', 'ph3', '[]', 1, 0, 0);
                 INSERT INTO player_keys (player_id, dek_wrapped) VALUES ('P1', 'k1'), ('P2', 'k2');
                 INSERT INTO staff (staff_id, branch_id, login, display_name, role_id, active, rev,
                                    created_at, updated_at)
                 VALUES ('S1', 'B', 's1', 'Sita', 'R', 1, 1, 0, 0);",
            )
            .unwrap();
        let orders = [
            order(
                "O0",
                "settled",
                "pos_sale",
                100,
                "2026-10-01",
                Some("P2"),
                Some("S1"),
            ),
            order(
                "O1",
                "settled",
                "pos_sale",
                500,
                "2026-10-10",
                Some("P1"),
                Some("S1"),
            ),
            order(
                "O2",
                "settled",
                "session",
                300,
                "2026-10-10",
                Some("P1"),
                Some("S1"),
            ),
            Order {
                discount: 50,
                discount_by: "M1",
                ..order(
                    "O3",
                    "settled",
                    "plan_sale",
                    200,
                    "2026-10-12",
                    Some("P1"),
                    Some("S2"),
                )
            },
            order(
                "O4",
                "settled",
                "pos_sale",
                400,
                "2026-10-11",
                Some("P2"),
                Some("S1"),
            ),
            order(
                "O5",
                "settled",
                "pos_sale",
                70,
                "2026-10-13",
                None,
                Some("S2"),
            ),
            order("O6", "open", "session", 999, "2026-10-13", Some("P1"), None),
            Order {
                voided_by: "M1",
                ..order("O7", "void", "session", 120, "2026-10-12", Some("P2"), None)
            },
            order(
                "O8",
                "settled",
                "topup",
                1000,
                "2026-10-14",
                Some("P1"),
                Some("S1"),
            ),
            order(
                "O9",
                "settled",
                "session",
                60,
                "2026-10-15",
                Some("P3"),
                Some("S2"),
            ),
            order(
                "OA",
                "settled",
                "pos_sale",
                30,
                "2026-10-21",
                Some("P1"),
                Some("S1"),
            ),
        ];
        for o in orders {
            store
                .conn
                .execute(
                    "INSERT INTO orders (id, status, kind, subtotal_minor, total_minor,
                                         business_day, fy, player_id, settled_by, discount_minor,
                                         discount_by, voided_by, rev, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4, ?5, 'FY', ?6, ?7, NULLIF(?8, 0),
                             NULLIF(?9, ''), NULLIF(?10, ''), 1, 0, 0)",
                    rusqlite::params![
                        o.id,
                        o.status,
                        o.kind,
                        o.total,
                        o.day,
                        o.player,
                        o.settled_by,
                        o.discount,
                        o.discount_by,
                        o.voided_by
                    ],
                )
                .unwrap();
        }
        store
            .conn
            .execute_batch(
                "INSERT INTO credit_balances (player_id, limit_minor, outstanding_minor,
                                              reminder_days, rev, created_at, updated_at)
                 VALUES ('P1', 1000, 80, '', 1, 0, 0), ('P2', 1000, 300, '', 1, 0, 0),
                        ('P3', 1000, 200, '', 1, 0, 0), ('P4', 1000, 0, '', 1, 0, 0);
                 INSERT INTO wallet_txns (id, player_id, delta_minor, reason, staff_id, order_id,
                                          business_day, fy, rev, created_at, updated_at)
                 VALUES ('W1', 'P1', 80, 'credit', 'S1', 'O1', '2026-10-17', 'FY', 1, 1, 1),
                        ('W2', 'P2', 300, 'credit', 'S1', 'O4', '2026-10-10', 'FY', 1, 2, 2),
                        ('W3', 'P3', 200, 'credit', 'S1', 'O9', '2026-10-01', 'FY', 1, 3, 3);
                 INSERT INTO sessions (id, device_id, group_id, mode, state, started_at,
                                       rounding_min, minimum_min, grace_min, time_up_mode, epoch,
                                       business_day, fy, rev, created_at, updated_at)
                 VALUES ('SA', 'D', 'G', 'postpaid', 'ended', 1, 1, 1, 0, 'x', 'e', '2026-10-10', 'FY', 1, 0, 0),
                        ('SB', 'D', 'G', 'postpaid', 'ended', 1, 1, 1, 0, 'x', 'e', '2026-10-11', 'FY', 1, 0, 0),
                        ('SC', 'D', 'G', 'postpaid', 'orphaned', 1, 1, 1, 0, 'x', 'e', '2026-10-12', 'FY', 1, 0, 0),
                        ('SD', 'D', 'G', 'postpaid', 'ended', 1, 1, 1, 0, 'x', 'e', '2026-10-30', 'FY', 1, 0, 0);
                 INSERT INTO refunds (id, order_id, credit_note_no, reason, amount_minor, lines,
                                      staff_id, approved_by, business_day, fy, rev, created_at,
                                      updated_at)
                 VALUES ('R1', 'O1', 'CN1', 'mistake', 90, '[]', 'S1', 'M1', '2026-10-12', 'FY', 1, 0, 0),
                        ('R2', 'O1', 'CN2', 'mistake', 10, '[]', 'S1', 'M1', '2026-10-25', 'FY', 1, 0, 0);
                 INSERT INTO shifts (id, staff_id, opened_at, float_minor, carried_minor, state,
                                     closed_at, variance_minor, rev, created_at, updated_at)
                 VALUES ('H1', 'S2', 1, 0, 0, 'closed', 500, -50, 1, 0, 0),
                        ('H2', 'S2', 1, 0, 0, 'open', NULL, NULL, 1, 0, 0);",
            )
            .unwrap();
        for (seq, session, actor_kind, actor_id) in [
            (1, "SA", "staff", "S1"),
            (2, "SB", "device", "D1"),
            (3, "SC", "staff", "S1"),
            (4, "SD", "staff", "S1"),
        ] {
            store
                .conn
                .execute(
                    "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                          actor_kind, actor_id, at, business_day, fy)
                     VALUES ('1:a', ?1, 'sessions', ?2, 'insert', x'7b7d', 'I', 'session.start',
                             ?3, ?4, 0, 'd', 'f')",
                    rusqlite::params![seq, session, actor_kind, actor_id],
                )
                .unwrap();
        }
        (dir, store)
    }

    fn by(rows: &[super::StaffCountRow], id: &str) -> Option<(i64, i64)> {
        rows.iter()
            .find(|r| r.staff_id == id)
            .map(|r| (r.count, r.value_minor))
    }

    #[test]
    fn the_player_loaders_sum_settled_sales_per_player_and_split_out_guests() {
        let (_dir, store) = fixture();
        let rows = store
            .players_report_rows("2026-10-10", "2026-10-20", "2026-10-20")
            .unwrap();

        let mut players = rows.players.clone();
        players.sort_by(|a, b| a.player_id.cmp(&b.player_id));
        let shape: Vec<_> = players
            .iter()
            .map(|p| {
                (
                    p.player_id.as_str(),
                    p.visits,
                    p.spend_minor,
                    p.first_day.as_str(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                // Two sales on one business day plus a plan sale on another: 2 visits. The open
                // bill, the top-up and the sale after the range are not counted.
                ("P1", 2, 1000, "2026-10-10"),
                // First ever sale before the range; the void is not a sale.
                ("P2", 1, 400, "2026-10-01"),
                ("P3", 1, 60, "2026-10-15"),
            ]
        );
        assert!(players[0].name_enc.is_some() && players[0].dek_wrapped.is_some());
        assert!(
            players[2].name_enc.is_some() && players[2].dek_wrapped.is_none(),
            "no player_keys row: the name cannot be opened"
        );
        assert_eq!((rows.guest.bills, rows.guest.spend_minor), (1, 70));

        let mut debtors: Vec<_> = rows
            .debtors
            .iter()
            .map(|d| {
                (
                    d.player_id.as_str(),
                    d.outstanding_minor,
                    d.aging.oldest_age_days,
                    (
                        d.aging.bucket_0_7_minor,
                        d.aging.bucket_8_14_minor,
                        d.aging.bucket_15_plus_minor,
                    ),
                )
            })
            .collect();
        debtors.sort();
        assert_eq!(
            debtors,
            [
                ("P1", 80, 3, (80, 0, 0)),
                ("P2", 300, 10, (0, 300, 0)),
                ("P3", 200, 19, (0, 0, 200)),
            ],
            "a player with nothing outstanding is not a debtor"
        );
    }

    #[test]
    fn the_staff_loaders_attribute_each_figure_to_the_right_member() {
        let (_dir, store) = fixture();
        let rows = store
            .staff_report_rows("2026-10-10", "2026-10-20", 0, 1000)
            .unwrap();
        // O0 and OA are outside the range; the top-up is not a sale; the void is not a sale.
        assert_eq!(by(&rows.sales, "S1"), Some((3, 500 + 300 + 400)));
        assert_eq!(by(&rows.sales, "S2"), Some((3, 200 + 70 + 60)));
        assert_eq!(by(&rows.discounts, "M1"), Some((1, 50)));
        assert_eq!(rows.discounts.len(), 1);
        assert_eq!(by(&rows.voids, "M1"), Some((1, 120)));
        assert_eq!(
            by(&rows.refunds, "M1"),
            Some((1, 90)),
            "the refund after the range is out"
        );
        // Only the staff-started, not orphaned, in-range session counts.
        assert_eq!(by(&rows.sessions, "S1"), Some((1, 0)));
        assert_eq!(rows.sessions.len(), 1);
        assert_eq!(rows.shifts.len(), 1, "an open shift has no variance yet");
        assert_eq!(
            (
                rows.shifts[0].staff_id.as_str(),
                rows.shifts[0].variance_minor
            ),
            ("S2", -50)
        );
        assert_eq!(rows.names, [("S1".to_owned(), "Sita".to_owned())]);
    }

    #[test]
    fn an_empty_range_reads_as_no_rows() {
        let (_dir, store) = fixture();
        let p = store
            .players_report_rows("2027-01-01", "2027-01-02", "2027-01-02")
            .unwrap();
        assert!(p.players.is_empty());
        assert_eq!(p.guest.bills, 0);
        let s = store
            .staff_report_rows("2027-01-01", "2027-01-02", 0, 1)
            .unwrap();
        assert!(s.sales.is_empty() && s.voids.is_empty() && s.refunds.is_empty());
    }
}

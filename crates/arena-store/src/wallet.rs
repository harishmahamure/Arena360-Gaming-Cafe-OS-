use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletRow {
    pub player_id: String,
    pub balance_minor: i64,
    pub rev: i64,
}

pub struct NewWalletTxn<'a> {
    pub id: &'a str,
    pub player_id: &'a str,
    pub delta_minor: i64,
    pub reason: &'a str,
    pub method: Option<&'a str>,
    pub staff_id: &'a str,
    pub order_id: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletTxnRow {
    pub id: String,
    pub player_id: String,
    pub at: i64,
    pub delta_minor: i64,
    pub reason: String,
    pub method: Option<String>,
    pub staff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTxn {
    pub id: String,
    pub player_id: String,
    pub delta_minor: i64,
    pub reason: String,
    pub method: Option<String>,
}

pub struct NewTopupOrder<'a> {
    pub kind: &'a str,
    pub id: &'a str,
    pub player_id: &'a str,
    pub total_minor: i64,
    pub settled_at: i64,
    pub settled_by: &'a str,
    pub business_day: &'a str,
    pub fy: &'a str,
}

pub fn find_wallet(conn: &Connection, player_id: &str) -> Result<Option<WalletRow>, AppError> {
    conn.query_row(
        "SELECT player_id, balance_minor, rev FROM wallets WHERE player_id = ?1",
        [player_id],
        |r| {
            Ok(WalletRow {
                player_id: r.get(0)?,
                balance_minor: r.get(1)?,
                rev: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_wallet(conn: &Connection, player_id: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO wallets (player_id, balance_minor, rev, created_at, updated_at)
         VALUES (?1, 0, 0, 0, 0)",
        [player_id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn adjust_wallet(conn: &Connection, player_id: &str, delta_minor: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE wallets SET balance_minor = balance_minor + ?2 WHERE player_id = ?1",
        params![player_id, delta_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_wallet_txn(conn: &Connection, t: &NewWalletTxn<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO wallet_txns (id, player_id, delta_minor, reason, method, staff_id, order_id,
                                  business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0)",
        params![
            t.id,
            t.player_id,
            t.delta_minor,
            t.reason,
            t.method,
            t.staff_id,
            t.order_id,
            t.business_day,
            t.fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn insert_topup_order(conn: &Connection, o: &NewTopupOrder<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO orders (id, session_id, player_id, guest_no, device_id, status, subtotal_minor,
                             total_minor, business_day, fy, kind, settled_at, settled_by, rev,
                             created_at, updated_at)
         VALUES (?1, NULL, ?2, NULL, NULL, 'settled', ?3, ?3, ?4, ?5, ?8, ?6, ?7, 0, 0, 0)",
        params![
            o.id,
            o.player_id,
            o.total_minor,
            o.business_day,
            o.fy,
            o.settled_at,
            o.settled_by,
            o.kind
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn order_player_and_kind(
    conn: &Connection,
    order_id: &str,
) -> Result<Option<(Option<String>, String)>, AppError> {
    conn.query_row(
        "SELECT player_id, kind FROM orders WHERE id = ?1",
        [order_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

pub fn list_order_wallet_txns(
    conn: &Connection,
    order_id: &str,
) -> Result<Vec<OrderTxn>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, player_id, delta_minor, reason, method FROM wallet_txns
             WHERE order_id = ?1 ORDER BY id",
        )
        .map_err(db_error)?;
    stmt.query_map([order_id], |r| {
        Ok(OrderTxn {
            id: r.get(0)?,
            player_id: r.get(1)?,
            delta_minor: r.get(2)?,
            reason: r.get(3)?,
            method: r.get(4)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn list_wallet_txns(conn: &Connection, player_id: &str) -> Result<Vec<WalletTxnRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.player_id, t.created_at, t.delta_minor, t.reason, t.method,
                    COALESCE(s.display_name, t.staff_id)
             FROM wallet_txns t LEFT JOIN staff s ON s.staff_id = t.staff_id
             WHERE t.player_id = ?1 AND t.reason <> 'credit' ORDER BY t.created_at DESC, t.id DESC",
        )
        .map_err(db_error)?;
    stmt.query_map([player_id], |r| {
        Ok(WalletTxnRow {
            id: r.get(0)?,
            player_id: r.get(1)?,
            at: r.get(2)?,
            delta_minor: r.get(3)?,
            reason: r.get(4)?,
            method: r.get(5)?,
            staff: r.get(6)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<_, _>>()
    .map_err(db_error)
}

pub fn wallet_balance(conn: &Connection, player_id: &str) -> Result<i64, AppError> {
    Ok(find_wallet(conn, player_id)?.map_or(0, |w| w.balance_minor))
}

/// An imported opening balance (Story 5.34): a ledger row of reason `import` that belongs to no
/// order, so there is no cash event and no invoice behind it.
pub fn insert_import_wallet_txn(
    conn: &Connection,
    id: &str,
    player_id: &str,
    delta_minor: i64,
    staff_id: &str,
    business_day: &str,
    fy: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO wallet_txns (id, player_id, delta_minor, reason, method, staff_id, order_id,
                                  business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'import', NULL, ?4, NULL, ?5, ?6, 0, 0, 0)",
        params![id, player_id, delta_minor, staff_id, business_day, fy],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The ledger row that zeroes a wallet balance at erasure (Story 9.1): reason `erase_refund` (paid
/// out at the counter) or `erase_forfeit`, negative, belonging to no order, so no cash event and no
/// invoice sits behind it.
pub fn insert_erase_wallet_txn(
    conn: &Connection,
    id: &str,
    player_id: &str,
    delta_minor: i64,
    reason: &str,
    staff_id: &str,
    (business_day, fy): (&str, &str),
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO wallet_txns (id, player_id, delta_minor, reason, method, staff_id, order_id,
                                  business_day, fy, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, ?6, ?7, 0, 0, 0)",
        params![
            id,
            player_id,
            delta_minor,
            reason,
            staff_id,
            business_day,
            fy
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

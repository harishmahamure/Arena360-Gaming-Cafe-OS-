//! The notification outbox (Story 5.19): `notify_outbox` rows a settle queues for the Epic 8
//! drain. The table is in the change log, so every write here is reported to `apply()` as a
//! [`RowKey::NotifyOutbox`](crate::RowKey::NotifyOutbox).

use arena_proto::{AppError, NOTIFY_STATE_QUEUED, NOTIFY_STATE_SKIPPED};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// A row about to be queued.
pub struct NewNotify<'a> {
    pub id: &'a str,
    pub purpose: &'a str,
    pub kind: &'a str,
    pub order_id: &'a str,
    pub player_id: &'a str,
    pub consent_policy_version: &'a str,
    pub not_before: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyRow {
    pub id: String,
    pub purpose: String,
    pub kind: String,
    pub order_id: String,
    pub player_id: String,
    pub consent_policy_version: String,
    pub not_before: i64,
    pub state: String,
}

pub fn insert_notify(conn: &Connection, n: &NewNotify<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO notify_outbox (id, purpose, kind, order_id, player_id, consent_policy_version,
                                    not_before, state, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0)",
        params![
            n.id,
            n.purpose,
            n.kind,
            n.order_id,
            n.player_id,
            n.consent_policy_version,
            n.not_before,
            NOTIFY_STATE_QUEUED,
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The ids of the rows of `order_id` still waiting to be sent.
pub fn queued_notify_ids(conn: &Connection, order_id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM notify_outbox WHERE order_id = ?1 AND state = 'queued' ORDER BY id",
        )
        .map_err(db_error)?;
    stmt.query_map([order_id], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(db_error)
}

/// Marks one queued row `skipped`: it will never be sent. False when it was not queued.
pub fn skip_notify(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE notify_outbox SET state = ?2 WHERE id = ?1 AND state = 'queued'",
            params![id, NOTIFY_STATE_SKIPPED],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// The queued rows whose `not_before` has passed at `now_ms`, in `not_before` order (the order
/// they are sent in).
pub fn list_due_notify(
    conn: &Connection,
    now_ms: i64,
    limit: i64,
) -> Result<Vec<NotifyRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state
             FROM notify_outbox WHERE state = 'queued' AND not_before <= ?1
             ORDER BY not_before, id LIMIT ?2",
        )
        .map_err(db_error)?;
    stmt.query_map(params![now_ms, limit], |r| {
        Ok(NotifyRow {
            id: r.get(0)?,
            purpose: r.get(1)?,
            kind: r.get(2)?,
            order_id: r.get(3)?,
            player_id: r.get(4)?,
            consent_policy_version: r.get(5)?,
            not_before: r.get(6)?,
            state: r.get(7)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(db_error)
}

/// How a queued row ended: its new `state` and what goes with it. False when it was not queued.
pub fn finish_notify(
    conn: &Connection,
    id: &str,
    state: &str,
    channel: Option<&str>,
    error_code: Option<&str>,
    reason: Option<&str>,
) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE notify_outbox SET state = ?2, channel = ?3, error_code = ?4, reason = ?5
             WHERE id = ?1 AND state = 'queued'",
            params![id, state, channel, error_code, reason],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// What the latest receipt message of `order_id` came to (`queued`, `sent`, `failed`, `skipped`).
pub fn latest_receipt_state(conn: &Connection, order_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT state FROM notify_outbox WHERE kind = 'receipt' AND order_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT 1",
        [order_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_error)
}

pub fn find_notify(conn: &Connection, id: &str) -> Result<Option<NotifyRow>, AppError> {
    conn.query_row(
        "SELECT id, purpose, kind, order_id, player_id, consent_policy_version, not_before, state
         FROM notify_outbox WHERE id = ?1",
        [id],
        |r| {
            Ok(NotifyRow {
                id: r.get(0)?,
                purpose: r.get(1)?,
                kind: r.get(2)?,
                order_id: r.get(3)?,
                player_id: r.get(4)?,
                consent_policy_version: r.get(5)?,
                not_before: r.get(6)?,
                state: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The player an order was sold to, if any.
pub fn order_player_id(conn: &Connection, order_id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT player_id FROM orders WHERE id = ?1",
        [order_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for m in crate::migrations::MIGRATIONS {
            conn.execute_batch(m.sql).unwrap();
        }
        // The player row is not what these tests are about.
        conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        conn
    }

    fn new<'a>(id: &'a str, order_id: &'a str) -> NewNotify<'a> {
        NewNotify {
            id,
            purpose: "transactional",
            kind: "receipt",
            order_id,
            player_id: "P1",
            consent_policy_version: "1",
            not_before: 8_000,
        }
    }

    #[test]
    fn a_queued_row_is_skipped_once_and_the_order_reports_its_latest_state() {
        let conn = conn();
        assert_eq!(latest_receipt_state(&conn, "O1").unwrap(), None);
        insert_notify(&conn, &new("N1", "O1")).unwrap();
        insert_notify(&conn, &new("N2", "O2")).unwrap();
        assert_eq!(queued_notify_ids(&conn, "O1").unwrap(), ["N1"]);
        assert_eq!(
            latest_receipt_state(&conn, "O1").unwrap().as_deref(),
            Some("queued")
        );
        assert!(skip_notify(&conn, "N1").unwrap());
        assert!(
            !skip_notify(&conn, "N1").unwrap(),
            "a skipped row stays skipped"
        );
        assert!(queued_notify_ids(&conn, "O1").unwrap().is_empty());
        assert_eq!(
            latest_receipt_state(&conn, "O1").unwrap().as_deref(),
            Some("skipped")
        );
        assert_eq!(find_notify(&conn, "N2").unwrap().unwrap().state, "queued");
    }

    #[test]
    fn due_rows_come_in_not_before_order_and_settle_once() {
        let conn = conn();
        let mut late = new("N1", "O1");
        late.not_before = 9_000;
        insert_notify(&conn, &late).unwrap();
        insert_notify(&conn, &new("N2", "O2")).unwrap();
        let mut early = new("N3", "O3");
        early.not_before = 7_000;
        insert_notify(&conn, &early).unwrap();

        assert!(list_due_notify(&conn, 6_999, 10).unwrap().is_empty());
        let ids = |now| -> Vec<String> {
            list_due_notify(&conn, now, 10)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect()
        };
        assert_eq!(ids(8_000), ["N3", "N2"]);
        assert_eq!(ids(9_000), ["N3", "N2", "N1"]);
        assert_eq!(list_due_notify(&conn, 9_000, 1).unwrap().len(), 1);

        assert!(finish_notify(&conn, "N3", "sent", Some("sms"), None, None).unwrap());
        assert!(
            !finish_notify(&conn, "N3", "failed", None, Some("X"), None).unwrap(),
            "a settled row stays settled"
        );
        assert_eq!(ids(9_000), ["N2", "N1"]);
        assert!(finish_notify(&conn, "N2", "expired", None, None, None).unwrap());
        assert!(finish_notify(&conn, "N1", "skipped", None, None, Some("order_reverted")).unwrap());
        assert_eq!(find_notify(&conn, "N2").unwrap().unwrap().state, "expired");
        assert!(ids(9_000).is_empty());
    }
    #[test]
    fn the_table_only_takes_a_transactional_receipt() {
        let conn = conn();
        let bad = |purpose: &str, kind: &str| {
            conn.execute(
                "INSERT INTO notify_outbox (id, purpose, kind, order_id, player_id,
                                            consent_policy_version, not_before, state, rev,
                                            created_at, updated_at)
                 VALUES ('X', ?1, ?2, 'O', 'P', '1', 0, 'queued', 0, 0, 0)",
                params![purpose, kind],
            )
        };
        assert!(bad("marketing", "receipt").is_err());
        assert!(bad("transactional", "low_balance").is_err());
        assert!(bad("transactional", "receipt").is_ok());
    }
}

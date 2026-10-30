//! The notification outbox (Story 5.19): `notify_outbox` rows a settle or a running session queues
//! for the Epic 8 drain. The table is in the change log, so every write here is reported to
//! `apply()` as a [`RowKey::NotifyOutbox`](crate::RowKey::NotifyOutbox).

use arena_proto::{AppError, NOTIFY_STATE_QUEUED, NOTIFY_STATE_SKIPPED};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// A row about to be queued.
pub struct NewNotify<'a> {
    pub id: &'a str,
    pub purpose: &'a str,
    pub kind: &'a str,
    /// The order a receipt is for; empty for a message about a session.
    pub order_id: Option<&'a str>,
    /// The session a `low_balance`, `session_ending` or `console_5min` message is about.
    pub session_id: Option<&'a str>,
    pub player_id: &'a str,
    pub consent_policy_version: &'a str,
    pub not_before: i64,
    /// The raw template values, a JSON object of text.
    pub params: Option<&'a str>,
    /// The business day a `credit_reminder` is raised for (at most one per player per day).
    pub reminder_day: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyRow {
    pub id: String,
    pub purpose: String,
    pub kind: String,
    pub order_id: Option<String>,
    pub session_id: Option<String>,
    pub player_id: String,
    pub consent_policy_version: String,
    pub not_before: i64,
    pub state: String,
    pub params: Option<String>,
    pub rearmed_at: Option<i64>,
    pub reminder_day: Option<String>,
}

const ROW_COLUMNS: &str = "id, purpose, kind, order_id, session_id, player_id, \
     consent_policy_version, not_before, state, params, rearmed_at, reminder_day";

fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<NotifyRow> {
    Ok(NotifyRow {
        id: r.get(0)?,
        purpose: r.get(1)?,
        kind: r.get(2)?,
        order_id: r.get(3)?,
        session_id: r.get(4)?,
        player_id: r.get(5)?,
        consent_policy_version: r.get(6)?,
        not_before: r.get(7)?,
        state: r.get(8)?,
        params: r.get(9)?,
        rearmed_at: r.get(10)?,
        reminder_day: r.get(11)?,
    })
}

pub fn insert_notify(conn: &Connection, n: &NewNotify<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO notify_outbox (id, purpose, kind, order_id, session_id, player_id,
                                    consent_policy_version, not_before, state, params,
                                    reminder_day, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0, 0)",
        params![
            n.id,
            n.purpose,
            n.kind,
            n.order_id,
            n.session_id,
            n.player_id,
            n.consent_policy_version,
            n.not_before,
            NOTIFY_STATE_QUEUED,
            n.params,
            n.reminder_day,
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
        .prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM notify_outbox
             WHERE state = 'queued' AND not_before <= ?1
             ORDER BY not_before, id LIMIT ?2"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![now_ms, limit], row_of)
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
        &format!("SELECT {ROW_COLUMNS} FROM notify_outbox WHERE id = ?1"),
        [id],
        row_of,
    )
    .optional()
    .map_err(db_error)
}

/// Whether a row of `kind` about `session_id` exists, whatever became of it: a session's
/// `session_ending` and `console_5min` messages go out once, also after an Extend.
pub fn session_notice_exists(
    conn: &Connection,
    session_id: &str,
    kind: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notify_outbox WHERE session_id = ?1 AND kind = ?2)",
        params![session_id, kind],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The latest `low_balance` row of `player_id`: its id and when the wallet was seen above the
/// level again (`None` while that crossing is still the current one).
pub fn latest_low_balance(
    conn: &Connection,
    player_id: &str,
) -> Result<Option<(String, Option<i64>)>, AppError> {
    conn.query_row(
        "SELECT id, rearmed_at FROM notify_outbox WHERE player_id = ?1 AND kind = 'low_balance'
         ORDER BY id DESC LIMIT 1",
        [player_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// Marks the `low_balance` row `id` as left behind by a wallet that rose above the level. False
/// when it already was.
pub fn rearm_notify(conn: &Connection, id: &str, now_ms: i64) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE notify_outbox SET rearmed_at = ?2
             WHERE id = ?1 AND kind = 'low_balance' AND rearmed_at IS NULL",
            params![id, now_ms],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// Whether `player_id` already has a credit reminder for the business day `day`, whatever became
/// of it: a player is reminded at most once a day.
pub fn credit_reminder_exists(
    conn: &Connection,
    player_id: &str,
    day: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notify_outbox
                        WHERE player_id = ?1 AND kind = 'credit_reminder' AND reminder_day = ?2)",
        params![player_id, day],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The players with a credit reminder for the business day `day` that is `queued` or `sent`, by id.
pub fn credit_reminded_on(conn: &Connection, day: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT player_id FROM notify_outbox
             WHERE kind = 'credit_reminder' AND reminder_day = ?1 AND state IN ('queued', 'sent')
             ORDER BY player_id",
        )
        .map_err(db_error)?;
    stmt.query_map([day], |r| r.get(0))
        .map_err(db_error)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(db_error)
}

/// The business day of the latest credit reminder of `player_id` the cloud sent.
pub fn last_credit_reminder_sent(
    conn: &Connection,
    player_id: &str,
) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT MAX(reminder_day) FROM notify_outbox
         WHERE player_id = ?1 AND kind = 'credit_reminder' AND state = 'sent'",
        [player_id],
        |r| r.get(0),
    )
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
            order_id: Some(order_id),
            session_id: None,
            player_id: "P1",
            consent_policy_version: "1",
            not_before: 8_000,
            params: None,
            reminder_day: None,
        }
    }

    fn about_session<'a>(id: &'a str, kind: &'a str, session: &'a str) -> NewNotify<'a> {
        NewNotify {
            id,
            purpose: "transactional",
            kind,
            order_id: None,
            session_id: Some(session),
            player_id: "P1",
            consent_policy_version: "1",
            not_before: 8_000,
            params: Some("{}"),
            reminder_day: None,
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
    fn the_table_only_takes_a_transactional_message_it_has_a_template_for() {
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
        assert!(bad("transactional", "newsletter").is_err());
        assert!(bad("transactional", "low_balance").is_ok());
    }

    #[test]
    fn a_session_gets_each_of_its_two_messages_once() {
        let conn = conn();
        assert!(!session_notice_exists(&conn, "S1", "session_ending").unwrap());
        insert_notify(&conn, &about_session("N1", "session_ending", "S1")).unwrap();
        assert!(session_notice_exists(&conn, "S1", "session_ending").unwrap());
        assert!(!session_notice_exists(&conn, "S1", "console_5min").unwrap());
        assert!(
            insert_notify(&conn, &about_session("N2", "session_ending", "S1")).is_err(),
            "the same message of a session is refused"
        );
        insert_notify(&conn, &about_session("N3", "session_ending", "S2")).unwrap();
        // A settled row still counts: an Extend after the send must not send it again.
        finish_notify(&conn, "N1", "skipped", None, None, Some("session_changed")).unwrap();
        assert!(session_notice_exists(&conn, "S1", "session_ending").unwrap());
        // A row about neither an order nor a session is not a row.
        let mut orphan = about_session("N4", "low_balance", "S1");
        orphan.session_id = None;
        assert!(insert_notify(&conn, &orphan).is_err());
        let stored = find_notify(&conn, "N3").unwrap().unwrap();
        assert_eq!(stored.session_id.as_deref(), Some("S2"));
        assert_eq!(stored.order_id, None);
        assert_eq!(stored.params.as_deref(), Some("{}"));
    }

    #[test]
    fn a_low_balance_crossing_is_rearmed_once() {
        let conn = conn();
        assert_eq!(latest_low_balance(&conn, "P1").unwrap(), None);
        insert_notify(&conn, &about_session("N1", "low_balance", "S1")).unwrap();
        insert_notify(&conn, &about_session("N2", "low_balance", "S1")).unwrap();
        assert_eq!(
            latest_low_balance(&conn, "P1").unwrap(),
            Some(("N2".into(), None))
        );
        assert!(rearm_notify(&conn, "N2", 5).unwrap());
        assert!(!rearm_notify(&conn, "N2", 6).unwrap());
        assert_eq!(
            latest_low_balance(&conn, "P1").unwrap(),
            Some(("N2".into(), Some(5)))
        );
    }
    fn credit<'a>(id: &'a str, player: &'a str, day: Option<&'a str>) -> NewNotify<'a> {
        NewNotify {
            id,
            purpose: "transactional",
            kind: "credit_reminder",
            order_id: None,
            session_id: None,
            player_id: player,
            consent_policy_version: "1",
            not_before: 8_000,
            params: Some(r#"{"amount_minor":"6000"}"#),
            reminder_day: day,
        }
    }

    #[test]
    fn a_player_has_one_credit_reminder_per_business_day() {
        let conn = conn();
        assert!(!credit_reminder_exists(&conn, "P1", "2027-01-18").unwrap());
        assert_eq!(last_credit_reminder_sent(&conn, "P1").unwrap(), None);
        insert_notify(&conn, &credit("N1", "P1", Some("2027-01-18"))).unwrap();
        assert!(credit_reminder_exists(&conn, "P1", "2027-01-18").unwrap());
        assert!(!credit_reminder_exists(&conn, "P2", "2027-01-18").unwrap());
        assert!(!credit_reminder_exists(&conn, "P1", "2027-01-19").unwrap());
        assert!(
            insert_notify(&conn, &credit("N2", "P1", Some("2027-01-18"))).is_err(),
            "the same day twice is refused"
        );
        // A settled row still counts for its day.
        finish_notify(&conn, "N1", "skipped", None, None, Some("credit_cleared")).unwrap();
        assert!(credit_reminder_exists(&conn, "P1", "2027-01-18").unwrap());
        assert!(
            insert_notify(&conn, &credit("N3", "P1", None)).is_err(),
            "a credit reminder without a day is not a row"
        );

        // Only a queued or sent reminder counts as "reminded today".
        // N1 was skipped on 2027-01-18, so nobody is reminded that day yet.
        assert!(credit_reminded_on(&conn, "2027-01-18").unwrap().is_empty());
        insert_notify(&conn, &credit("N6", "P2", Some("2027-01-18"))).unwrap();
        insert_notify(&conn, &credit("N7", "P3", Some("2027-01-18"))).unwrap();
        insert_notify(&conn, &credit("N8", "P4", Some("2027-01-18"))).unwrap();
        assert_eq!(
            credit_reminded_on(&conn, "2027-01-18").unwrap(),
            ["P2", "P3", "P4"]
        );
        finish_notify(&conn, "N7", "sent", Some("sms"), None, None).unwrap();
        finish_notify(&conn, "N8", "failed", None, Some("X"), None).unwrap();
        assert_eq!(
            credit_reminded_on(&conn, "2027-01-18").unwrap(),
            ["P2", "P3"]
        );
        assert!(credit_reminded_on(&conn, "2027-01-19").unwrap().is_empty());

        // Only a reminder that was sent is "the last reminder", by its day.
        assert_eq!(last_credit_reminder_sent(&conn, "P1").unwrap(), None);
        insert_notify(&conn, &credit("N4", "P1", Some("2027-01-22"))).unwrap();
        insert_notify(&conn, &credit("N5", "P1", Some("2027-01-29"))).unwrap();
        finish_notify(&conn, "N4", "sent", Some("sms"), None, None).unwrap();
        assert_eq!(
            last_credit_reminder_sent(&conn, "P1").unwrap().as_deref(),
            Some("2027-01-22")
        );
        finish_notify(&conn, "N5", "sent", Some("sms"), None, None).unwrap();
        assert_eq!(
            last_credit_reminder_sent(&conn, "P1").unwrap().as_deref(),
            Some("2027-01-29")
        );
    }
}

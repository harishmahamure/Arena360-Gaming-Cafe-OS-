use arena_proto::{AppError, CREDIT_OVERDUE_DAYS, ClearedSpend, CreditAging, CreditCollection};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditRow {
    pub player_id: String,
    pub limit_minor: i64,
    pub outstanding_minor: i64,
    pub reminder_days: Vec<u16>,
    pub paused: bool,
    pub last_override_day: Option<String>,
    pub rev: i64,
}

pub fn encode_days(days: &[u16]) -> String {
    days.iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub fn decode_days(text: &str) -> Vec<u16> {
    text.split(',')
        .filter_map(|d| d.trim().parse().ok())
        .collect()
}

pub(crate) const CREDIT_COLUMNS: &str = "player_id, limit_minor, outstanding_minor, reminder_days, credit_paused, last_override_day, rev";

pub(crate) fn credit_from(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<CreditRow> {
    let days: String = r.get(at + 3)?;
    Ok(CreditRow {
        player_id: r.get(at)?,
        limit_minor: r.get(at + 1)?,
        outstanding_minor: r.get(at + 2)?,
        reminder_days: decode_days(&days),
        paused: r.get::<_, i64>(at + 4)? != 0,
        last_override_day: r.get(at + 5)?,
        rev: r.get(at + 6)?,
    })
}

pub fn find_credit(conn: &Connection, player_id: &str) -> Result<Option<CreditRow>, AppError> {
    conn.query_row(
        &format!("SELECT {CREDIT_COLUMNS} FROM credit_balances WHERE player_id = ?1"),
        [player_id],
        |r| credit_from(r, 0),
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_credit(
    conn: &Connection,
    player_id: &str,
    limit_minor: i64,
    reminder_days: &[u16],
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO credit_balances (player_id, limit_minor, outstanding_minor, reminder_days,
                                      credit_paused, last_override_day, rev, created_at, updated_at)
         VALUES (?1, ?2, 0, ?3, 0, NULL, 0, 0, 0)",
        params![player_id, limit_minor, encode_days(reminder_days)],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn update_credit_limit(
    conn: &Connection,
    player_id: &str,
    limit_minor: i64,
    reminder_days: &[u16],
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET limit_minor = ?2, reminder_days = ?3 WHERE player_id = ?1",
        params![player_id, limit_minor, encode_days(reminder_days)],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn adjust_outstanding(
    conn: &Connection,
    player_id: &str,
    delta_minor: i64,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET outstanding_minor = outstanding_minor + ?2
         WHERE player_id = ?1",
        params![player_id, delta_minor],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn set_override_day(
    conn: &Connection,
    player_id: &str,
    day: Option<&str>,
) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET last_override_day = ?2 WHERE player_id = ?1",
        params![player_id, day],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn credit_collection(
    conn: &Connection,
    order_id: &str,
) -> Result<Option<CreditCollection>, AppError> {
    let collected: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT t.player_id, t.rowid, -t.delta_minor
             FROM wallet_txns t JOIN orders o ON o.id = t.order_id
             WHERE t.order_id = ?1 AND t.reason = 'credit' AND t.delta_minor < 0
               AND o.kind = 'credit_collect'",
            [order_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    let Some((player_id, at_rowid, amount)) = collected else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare(
            "SELECT t.rowid, t.order_id, t.delta_minor, COALESCE(o.settled_at, 0)
             FROM wallet_txns t JOIN orders o ON o.id = t.order_id
             WHERE t.player_id = ?1 AND t.reason = 'credit' AND t.rowid <= ?2
             ORDER BY t.rowid",
        )
        .map_err(db_error)?;
    let rows: Vec<(i64, String, i64, i64)> = stmt
        .query_map(params![player_id, at_rowid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    let before: i64 = rows
        .iter()
        .filter(|(rowid, _, delta, _)| *delta < 0 && *rowid < at_rowid)
        .map(|(_, _, delta, _)| -delta)
        .sum();
    let after = before + amount;
    let mut cleared = Vec::new();
    let mut start = 0i64;
    let mut spent = 0i64;
    for (_, spend_order, delta, at) in rows.iter().filter(|(_, _, delta, _)| *delta > 0) {
        let end = start + delta;
        let part = after.min(end) - before.max(start);
        if part > 0 {
            cleared.push(ClearedSpend {
                order_id: spend_order.clone(),
                at: *at,
                spend_minor: *delta,
                cleared_minor: part,
            });
        }
        start = end;
        spent += delta;
    }
    Ok(Some(CreditCollection {
        player_id,
        amount_minor: amount,
        cleared,
        outstanding_after_minor: spent - after,
    }))
}

pub fn civil_day_number(day: &str) -> Option<i64> {
    let mut parts = day.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub fn fifo_aging(spends: &[(String, i64)], collected_minor: i64, today: &str) -> CreditAging {
    let now = civil_day_number(today);
    let mut aging = CreditAging {
        oldest_unpaid_day: None,
        oldest_age_days: 0,
        bucket_0_7_minor: 0,
        bucket_8_14_minor: 0,
        bucket_15_plus_minor: 0,
    };
    let mut cleared = collected_minor.max(0);
    for (day, spend) in spends {
        let paid = cleared.min(*spend);
        cleared -= paid;
        let unpaid = spend - paid;
        if unpaid <= 0 {
            continue;
        }
        let age = match (now, civil_day_number(day)) {
            (Some(now), Some(then)) => (now - then).max(0),
            _ => 0,
        };
        if aging.oldest_unpaid_day.is_none() {
            aging.oldest_unpaid_day = Some(day.clone());
            aging.oldest_age_days = age;
        }
        let bucket = if age >= CREDIT_OVERDUE_DAYS {
            &mut aging.bucket_15_plus_minor
        } else if age >= 8 {
            &mut aging.bucket_8_14_minor
        } else {
            &mut aging.bucket_0_7_minor
        };
        *bucket += unpaid;
    }
    aging
}

pub fn credit_aging(
    conn: &Connection,
    player_id: &str,
    today: &str,
) -> Result<CreditAging, AppError> {
    credit_aging_with(conn, player_id, today, 0)
}

pub fn credit_aging_with(
    conn: &Connection,
    player_id: &str,
    today: &str,
    extra_collected_minor: i64,
) -> Result<CreditAging, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT business_day, delta_minor FROM wallet_txns
             WHERE player_id = ?1 AND reason = 'credit' ORDER BY rowid",
        )
        .map_err(db_error)?;
    let rows: Vec<(String, i64)> = stmt
        .query_map([player_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    let collected = rows
        .iter()
        .filter(|(_, d)| *d < 0)
        .map(|(_, d)| -d)
        .sum::<i64>()
        + extra_collected_minor;
    let spends: Vec<(String, i64)> = rows.into_iter().filter(|(_, d)| *d > 0).collect();
    Ok(fifo_aging(&spends, collected, today))
}

pub fn list_credit_debtors(conn: &Connection) -> Result<Vec<CreditRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CREDIT_COLUMNS} FROM credit_balances
             WHERE outstanding_minor > 0 OR credit_paused = 1 ORDER BY player_id"
        ))
        .map_err(db_error)?;
    stmt.query_map([], |r| credit_from(r, 0))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn set_credit_paused(conn: &Connection, player_id: &str, paused: bool) -> Result<(), AppError> {
    conn.execute(
        "UPDATE credit_balances SET credit_paused = ?2 WHERE player_id = ?1",
        params![player_id, i64::from(paused)],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spend(day: &str, minor: i64) -> (String, i64) {
        (day.to_owned(), minor)
    }

    #[test]
    fn day_numbers_count_calendar_days_across_months_and_leap_years() {
        let n = |d| civil_day_number(d).unwrap();
        assert_eq!(n("2026-10-28") - n("2026-10-27"), 1);
        assert_eq!(n("2026-03-01") - n("2026-02-28"), 1);
        assert_eq!(n("2028-03-01") - n("2028-02-28"), 2);
        assert_eq!(n("2027-01-01") - n("2026-12-17"), 15);
        assert_eq!(civil_day_number("nonsense"), None);
        assert_eq!(civil_day_number("2026-13-01"), None);
    }

    #[test]
    fn collections_clear_the_oldest_spend_first() {
        let spends = [spend("2026-10-01", 100), spend("2026-10-20", 200)];
        let a = fifo_aging(&spends, 150, "2026-10-27");
        assert_eq!(a.oldest_unpaid_day.as_deref(), Some("2026-10-20"));
        assert_eq!(a.oldest_age_days, 7);
        assert_eq!(
            (
                a.bucket_0_7_minor,
                a.bucket_8_14_minor,
                a.bucket_15_plus_minor
            ),
            (150, 0, 0)
        );
        assert!(!a.overdue());
    }

    #[test]
    fn unpaid_spends_fall_into_the_three_buckets() {
        let spends = [
            spend("2026-10-10", 10),
            spend("2026-10-18", 20),
            spend("2026-10-20", 30),
            spend("2026-10-27", 40),
        ];
        let a = fifo_aging(&spends, 0, "2026-10-27");
        assert_eq!(a.oldest_unpaid_day.as_deref(), Some("2026-10-10"));
        assert_eq!(a.oldest_age_days, 17);
        assert_eq!(
            (
                a.bucket_0_7_minor,
                a.bucket_8_14_minor,
                a.bucket_15_plus_minor
            ),
            (70, 20, 10)
        );
        assert!(a.overdue());
    }

    #[test]
    fn a_partly_paid_old_spend_keeps_its_age() {
        let spends = [spend("2026-10-01", 100)];
        let a = fifo_aging(&spends, 40, "2026-10-16");
        assert_eq!((a.oldest_age_days, a.bucket_15_plus_minor), (15, 60));
    }

    #[test]
    fn a_cleared_tab_has_no_aging() {
        let spends = [spend("2026-10-01", 100)];
        let a = fifo_aging(&spends, 100, "2026-12-01");
        assert_eq!(a.oldest_unpaid_day, None);
        assert_eq!(
            (
                a.bucket_0_7_minor,
                a.bucket_8_14_minor,
                a.bucket_15_plus_minor
            ),
            (0, 0, 0)
        );
    }
}

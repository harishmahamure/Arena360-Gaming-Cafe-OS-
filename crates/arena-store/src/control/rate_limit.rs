//! `rate_limits`: counters per key per bucket start, for rolling-window limits (the OTP sends
//! per phone per hour in minute buckets, the SMS sends overall per minute in second buckets).
//!
//! A bucket counts toward a window while any of it lies inside: its start is after
//! `now - window - bucket`. It leaves the window at `start + bucket + window`, which is what
//! `Retry-After` is computed from. A hit is reserved atomically ([`ControlDb::rate_reserve`]):
//! the window is summed and the bucket incremented in one transaction, only when under the
//! limit.

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, text};

/// The hits counted for a key since some time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateWindow {
    /// The sum of the counted buckets.
    pub count: u64,
    /// The oldest counted bucket's start, when any.
    pub oldest_window_start: Option<i64>,
}

/// One rolling-window limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateRule<'a> {
    /// The counter key (`otp_send:+919876543210`, `sms:global`).
    pub key: &'a str,
    /// Keys of this limiter start with this; only they are pruned (buckets older than two
    /// windows) when a hit is reserved.
    pub prune_prefix: &'a str,
    pub bucket_ms: i64,
    pub window_ms: i64,
    /// Hits allowed per window.
    pub limit: u64,
}

impl RateRule<'_> {
    fn bucket_start(&self, now: i64) -> i64 {
        now - now.rem_euclid(self.bucket_ms)
    }

    /// Buckets starting after this count.
    fn since(&self, now: i64) -> i64 {
        now - self.window_ms - self.bucket_ms
    }
}

/// Hits reserved by [`ControlDb::rate_reserve`], to give back with
/// [`ControlDb::rate_release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateReservation {
    buckets: Vec<(String, i64)>,
}

impl ControlDb {
    /// The hits for `key` in buckets that start after `since` (exclusive).
    pub async fn rate_window(&self, key: &str, since: i64) -> Result<RateWindow, ControlError> {
        let rows = self
            .query(
                "SELECT COALESCE(SUM(count), 0), MIN(window_start) FROM rate_limits \
                 WHERE key = ? AND window_start > ?",
                vec![text(key), SqlValue::Integer(since)],
            )
            .await?;
        let Some(row) = rows.first() else {
            return Ok(RateWindow::default());
        };
        let count = u64::try_from(col_int(row, 0)?).unwrap_or(0);
        let oldest = match row.get(1) {
            Some(SqlValue::Integer(n)) => Some(*n),
            _ => None,
        };
        Ok(RateWindow {
            count,
            oldest_window_start: oldest,
        })
    }

    /// Reserves one hit under every rule at `now`, all or none, in one transaction (which
    /// also prunes each limiter's old buckets). `Err(retry_after_ms)` when a rule is at its
    /// limit: nothing was counted, and the wait is until enough of the fullest window leaves.
    pub async fn rate_reserve(
        &self,
        rules: &[RateRule<'_>],
        now: i64,
    ) -> Result<Result<RateReservation, i64>, ControlError> {
        let mut stmts = Vec::new();
        for r in rules {
            stmts.push(Stmt::new(
                "DELETE FROM rate_limits WHERE substr(key, 1, ?) = ? AND window_start < ?",
                vec![
                    SqlValue::Integer(r.prune_prefix.len() as i64),
                    text(r.prune_prefix),
                    SqlValue::Integer(now - 2 * r.window_ms),
                ],
            ));
            stmts.push(
                Stmt::new(
                    "INSERT INTO rate_limits (key, window_start, count) SELECT ?, ?, 1 \
                     WHERE (SELECT COALESCE(SUM(count), 0) FROM rate_limits \
                     WHERE key = ? AND window_start > ?) < ? \
                     ON CONFLICT (key, window_start) DO UPDATE SET count = count + 1",
                    vec![
                        text(r.key),
                        SqlValue::Integer(r.bucket_start(now)),
                        text(r.key),
                        SqlValue::Integer(r.since(now)),
                        SqlValue::Integer(i64::try_from(r.limit).unwrap_or(i64::MAX)),
                    ],
                )
                .expect(1),
            );
        }
        if self.transact(stmts).await? {
            return Ok(Ok(RateReservation {
                buckets: rules
                    .iter()
                    .map(|r| (r.key.to_owned(), r.bucket_start(now)))
                    .collect(),
            }));
        }
        let mut wait = 1;
        for r in rules {
            wait = wait.max(self.rate_retry_after_ms(r, now).await?);
        }
        Ok(Err(wait))
    }

    /// How long until `rule` has room for one more hit (0: it has room now). The window is
    /// summed bucket by bucket from the oldest; the wait ends when enough have left.
    pub async fn rate_retry_after_ms(
        &self,
        rule: &RateRule<'_>,
        now: i64,
    ) -> Result<i64, ControlError> {
        let rows = self
            .query(
                "SELECT window_start, count FROM rate_limits WHERE key = ? AND window_start > ? \
                 ORDER BY window_start",
                vec![text(rule.key), SqlValue::Integer(rule.since(now))],
            )
            .await?;
        let mut total: u64 = 0;
        let mut buckets = Vec::with_capacity(rows.len());
        for r in &rows {
            let n = u64::try_from(col_int(r, 1)?).unwrap_or(0);
            total += n;
            buckets.push((col_int(r, 0)?, n));
        }
        let mut wait = 0;
        for (start, n) in buckets {
            if total < rule.limit {
                break;
            }
            total -= n;
            wait = start + rule.bucket_ms + rule.window_ms - now;
        }
        Ok(wait.max(0))
    }

    /// Gives back hits reserved by [`ControlDb::rate_reserve`] (the action they were for
    /// definitely did not happen).
    pub async fn rate_release(&self, reservation: &RateReservation) -> Result<(), ControlError> {
        let stmts = reservation
            .buckets
            .iter()
            .map(|(key, start)| {
                Stmt::new(
                    "UPDATE rate_limits SET count = count - 1 \
                     WHERE key = ? AND window_start = ? AND count > 0",
                    vec![text(key), SqlValue::Integer(*start)],
                )
            })
            .collect();
        self.transact(stmts).await?;
        Ok(())
    }

    /// Every bucket of `key`, oldest first (tests and operators).
    pub async fn rate_buckets(&self, key: &str) -> Result<Vec<(i64, u64)>, ControlError> {
        let rows = self
            .query(
                "SELECT window_start, count FROM rate_limits WHERE key = ? ORDER BY window_start",
                vec![text(key)],
            )
            .await?;
        rows.iter()
            .map(|r| Ok((col_int(r, 0)?, u64::try_from(col_int(r, 1)?).unwrap_or(0))))
            .collect()
    }
}

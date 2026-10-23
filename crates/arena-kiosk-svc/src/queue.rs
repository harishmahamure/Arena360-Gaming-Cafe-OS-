//! `queue.db`: the kiosk's offline intent queue (the ordering rules are in
//! `arena_kiosk_core::queue`).
//!
//! Each row is one intent, stored whole as JSON with the ULID idempotency key and `issued_at`
//! it got when the event happened. `seq` gives the creation order the replay follows.

use std::path::Path;

use arena_proto::Intent;
use rusqlite::{Connection, OptionalExtension, params};

/// One queued intent.
#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub seq: i64,
    pub intent: Intent,
}

/// Entries kept at most; past it the oldest are dropped (with a `warn`).
pub const QUEUE_CAP: usize = 10_000;

/// The queue file.
pub struct QueueDb {
    conn: Connection,
    cap: usize,
}

fn db(e: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(format!("queue.db: {e}"))
}

impl QueueDb {
    /// Opens (creating if needed) the queue at `path`.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let conn = Connection::open(path).map_err(db)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS queue (
                 seq             INTEGER PRIMARY KEY AUTOINCREMENT,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 issued_at       INTEGER NOT NULL,
                 intent          TEXT NOT NULL
             );",
        )
        .map_err(db)?;
        Ok(Self {
            conn,
            cap: QUEUE_CAP,
        })
    }

    /// The same queue with another cap (tests).
    pub fn with_cap(mut self, cap: usize) -> Self {
        self.cap = cap.max(1);
        self
    }

    /// Appends `intent` (its key and `issued_at` are already set). The same key twice is kept
    /// once.
    pub fn push(&self, intent: &Intent) -> std::io::Result<()> {
        let json = serde_json::to_string(intent).map_err(std::io::Error::other)?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO queue (idempotency_key, issued_at, intent)
                 VALUES (?1, ?2, ?3)",
                params![intent.idempotency_key, intent.issued_at, json],
            )
            .map_err(db)?;
        let over = self.len()?.saturating_sub(self.cap);
        if over > 0 {
            self.conn
                .execute(
                    "DELETE FROM queue WHERE seq IN (SELECT seq FROM queue ORDER BY seq LIMIT ?1)",
                    params![over as i64],
                )
                .map_err(db)?;
            tracing::warn!(
                dropped = over,
                cap = self.cap,
                "the kiosk event queue is full; dropped the oldest entries"
            );
        }
        Ok(())
    }

    /// The oldest entry. A row that no longer decodes is deleted (and logged) and skipped.
    pub fn oldest(&self) -> std::io::Result<Option<Queued>> {
        loop {
            let row: Option<(i64, String)> = self
                .conn
                .query_row(
                    "SELECT seq, intent FROM queue ORDER BY seq LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db)?;
            let Some((seq, json)) = row else {
                return Ok(None);
            };
            match serde_json::from_str::<Intent>(&json) {
                Ok(intent) => return Ok(Some(Queued { seq, intent })),
                Err(e) => {
                    tracing::warn!(seq, error = %e, "dropping a queued intent that does not decode");
                    self.delete(seq)?;
                }
            }
        }
    }

    /// Deletes entry `seq`.
    pub fn delete(&self, seq: i64) -> std::io::Result<()> {
        self.conn
            .execute("DELETE FROM queue WHERE seq = ?1", params![seq])
            .map_err(db)?;
        Ok(())
    }

    /// Deletes every entry past the queue TTL at `now_ms` and returns their keys, oldest first.
    pub fn drop_expired(&self, now_ms: i64) -> std::io::Result<Vec<String>> {
        let cutoff = now_ms.saturating_sub(arena_kiosk_core::QUEUE_TTL_MS);
        let mut stmt = self
            .conn
            .prepare("SELECT idempotency_key FROM queue WHERE issued_at < ?1 ORDER BY seq")
            .map_err(db)?;
        let keys = stmt
            .query_map(params![cutoff], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?;
        self.conn
            .execute("DELETE FROM queue WHERE issued_at < ?1", params![cutoff])
            .map_err(db)?;
        Ok(keys)
    }

    /// How many entries wait.
    pub fn len(&self) -> std::io::Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM queue", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(db)
    }

    /// Whether nothing waits.
    pub fn is_empty(&self) -> std::io::Result<bool> {
        self.len().map(|n| n == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intents::kiosk_log;
    use arena_proto::KioskLogLevel;

    #[test]
    fn entries_come_back_in_creation_order_and_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.db");
        let a = kiosk_log("D", 100, KioskLogLevel::Info, "a", "");
        let b = kiosk_log("D", 50, KioskLogLevel::Warn, "b", "");
        {
            let q = QueueDb::open(&path).unwrap();
            q.push(&a).unwrap();
            q.push(&b).unwrap();
            q.push(&a).unwrap(); // the same key again: kept once
            assert_eq!(q.len().unwrap(), 2);
        }
        let q = QueueDb::open(&path).unwrap();
        // Creation order, not issued_at order.
        let first = q.oldest().unwrap().unwrap();
        assert_eq!(first.intent, a);
        q.delete(first.seq).unwrap();
        assert_eq!(q.oldest().unwrap().unwrap().intent, b);
    }

    #[test]
    fn past_the_cap_the_oldest_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db"))
            .unwrap()
            .with_cap(3);
        let all: Vec<_> = (0..5)
            .map(|i| kiosk_log("D", i, KioskLogLevel::Info, &format!("e{i}"), ""))
            .collect();
        for i in &all {
            q.push(i).unwrap();
        }
        assert_eq!(q.len().unwrap(), 3);
        assert_eq!(q.oldest().unwrap().unwrap().intent, all[2]);
    }

    #[test]
    fn expired_entries_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        let day = 24 * 60 * 60 * 1000;
        let old = kiosk_log("D", 0, KioskLogLevel::Info, "old", "");
        let fresh = kiosk_log("D", 7 * day, KioskLogLevel::Info, "fresh", "");
        q.push(&old).unwrap();
        q.push(&fresh).unwrap();
        assert_eq!(
            q.drop_expired(7 * day + 1).unwrap(),
            vec![old.idempotency_key.clone()]
        );
        assert_eq!(q.oldest().unwrap().unwrap().intent, fresh);
        assert!(q.drop_expired(7 * day + 1).unwrap().is_empty());
    }

    #[test]
    fn an_undecodable_row_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        q.conn
            .execute(
                "INSERT INTO queue (idempotency_key, issued_at, intent) VALUES ('k', 1, '{')",
                [],
            )
            .unwrap();
        let ok = kiosk_log("D", 2, KioskLogLevel::Info, "ok", "");
        q.push(&ok).unwrap();
        assert_eq!(q.oldest().unwrap().unwrap().intent, ok);
        assert_eq!(q.len().unwrap(), 1);
    }
}

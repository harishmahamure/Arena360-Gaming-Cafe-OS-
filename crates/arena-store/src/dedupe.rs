//! `applied_intents`: the idempotency records `apply()`'s ttl_dedupe step reads and writes, and
//! the housekeeping prune.

use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `applied_intents` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedIntentRecord {
    pub idempotency_key: String,
    pub intent_id: String,
    pub intent_kind: String,
    /// SHA-256 hex of the intent body the outcome belongs to.
    pub fingerprint: String,
    /// MessagePack of the stored outcome (the `Applied` summary or the `AppError`).
    pub outcome: Vec<u8>,
    pub applied_at: i64,
}

/// A recorded key: the fingerprint of its intent body and its stored outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIntent {
    pub fingerprint: String,
    pub outcome: Vec<u8>,
}

/// The record for `idempotency_key`, if that key was already recorded.
pub fn find_applied_intent(
    conn: &Connection,
    idempotency_key: &str,
) -> Result<Option<StoredIntent>, AppError> {
    conn.query_row(
        "SELECT fingerprint, outcome FROM applied_intents WHERE idempotency_key = ?1",
        [idempotency_key],
        |r| {
            Ok(StoredIntent {
                fingerprint: r.get(0)?,
                outcome: r.get(1)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Records an outcome. Returns `false` (writing nothing) when the key is already recorded, so
/// the first recorded outcome always wins.
pub fn insert_applied_intent(
    conn: &Connection,
    rec: &AppliedIntentRecord,
) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "INSERT INTO applied_intents
                 (idempotency_key, intent_id, intent_kind, fingerprint, outcome, applied_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (idempotency_key) DO NOTHING",
            params![
                rec.idempotency_key,
                rec.intent_id,
                rec.intent_kind,
                rec.fingerprint,
                rec.outcome,
                rec.applied_at
            ],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// Deletes at most `limit` records with `applied_at < cutoff_ms`. Returns how many were
/// deleted; fewer than `limit` means none older than the cutoff remain.
pub fn prune_applied_intents(
    conn: &Connection,
    cutoff_ms: i64,
    limit: usize,
) -> Result<usize, AppError> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    conn.execute(
        "DELETE FROM applied_intents WHERE rowid IN
             (SELECT rowid FROM applied_intents WHERE applied_at < ?1 LIMIT ?2)",
        params![cutoff_ms, limit],
    )
    .map_err(db_error)
}

/// How many dedupe records exist.
pub fn count_applied_intents(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row("SELECT COUNT(*) FROM applied_intents", [], |r| r.get(0))
        .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn rec(key: &str, at: i64) -> AppliedIntentRecord {
        AppliedIntentRecord {
            idempotency_key: key.into(),
            intent_id: "I".into(),
            intent_kind: "settings.set".into(),
            fingerprint: "F".into(),
            outcome: vec![0x80],
            applied_at: at,
        }
    }

    #[test]
    fn first_outcome_wins_and_prune_uses_a_strict_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                assert!(insert_applied_intent(tx, &rec("A", 10))?);
                assert!(!insert_applied_intent(
                    tx,
                    &AppliedIntentRecord {
                        outcome: vec![0x81],
                        ..rec("A", 11)
                    }
                )?);
                assert_eq!(
                    find_applied_intent(tx, "A")?,
                    Some(StoredIntent {
                        fingerprint: "F".into(),
                        outcome: vec![0x80]
                    })
                );
                assert_eq!(find_applied_intent(tx, "B")?, None);
                insert_applied_intent(tx, &rec("B", 20))?;
                insert_applied_intent(tx, &rec("C", 30))?;
                assert_eq!(prune_applied_intents(tx, 20, 500)?, 1);
                assert_eq!(find_applied_intent(tx, "A")?, None);
                assert_eq!(count_applied_intents(tx)?, 2);
                for i in 0..7 {
                    insert_applied_intent(tx, &rec(&format!("OLD{i}"), 5))?;
                }
                assert_eq!(prune_applied_intents(tx, 20, 3)?, 3);
                assert_eq!(prune_applied_intents(tx, 20, 3)?, 3);
                assert_eq!(prune_applied_intents(tx, 20, 3)?, 1);
                assert_eq!(prune_applied_intents(tx, 20, 3)?, 0);
                assert_eq!(count_applied_intents(tx)?, 2);
                Ok(())
            })
            .unwrap();
    }
}

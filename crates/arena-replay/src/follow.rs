//! A standby applies what the main Hub committed (Story 9.19, AD-4).
//!
//! [`apply_replicated`] takes one batch of the main Hub's `changes` rows in log order, applies
//! them to state with the same replay code the audit builder and restore use, writes them
//! verbatim into this database's own `changes` (same epoch, same `seq`) and stores or deletes the
//! player keys that came with them, all in the caller's one transaction. A batch must continue
//! this log exactly: a gap, a duplicate or a change of epoch fails `REPLICATE_GAP` and writes
//! nothing, and the standby then takes a fresh snapshot.

use arena_proto::{AppError, ErrorCode};
use arena_store::{ChangeRecord, StoreTx, append_replicated, log_tip, set_player_key};

use crate::{Resume, replay_history};

fn gap(message: String) -> AppError {
    AppError::new(ErrorCode::ReplicateGap, message)
}

/// Applies `changes` (log order) and `keys` to `tx`. Returns the new log position.
///
/// `keys` is the key state of the players the batch touches: `Some(wrapped)` stores it,
/// `None` deletes it (erasure).
pub fn apply_replicated(
    changes: Vec<ChangeRecord>,
    keys: &[(String, Option<String>)],
    tx: &mut StoreTx<'_>,
) -> Result<Option<(String, i64)>, AppError> {
    let tip = log_tip(tx)?;
    let mut expect = tip.clone();
    for c in &changes {
        match &expect {
            Some((epoch, seq)) if *epoch == c.epoch => {
                if c.seq <= *seq {
                    return Err(gap(format!("duplicate change {}:{}", c.epoch, c.seq)));
                }
                if c.seq != seq + 1 {
                    return Err(gap(format!(
                        "gap in epoch {}: expected {}, found {}",
                        c.epoch,
                        seq + 1,
                        c.seq
                    )));
                }
            }
            Some((epoch, _)) => {
                return Err(gap(format!(
                    "the main Hub moved from epoch {epoch} to {}",
                    c.epoch
                )));
            }
            None if c.seq != 1 => {
                return Err(gap(format!(
                    "the log starts at {}:{}, not 1",
                    c.epoch, c.seq
                )));
            }
            None => {}
        }
        expect = Some((c.epoch.clone(), c.seq));
    }
    if changes.is_empty() {
        apply_keys(keys, tx)?;
        return Ok(tip);
    }
    let resume = tip.map(|(epoch, seq)| Resume { epoch, seq });
    let verbatim = changes.clone();
    replay_history(changes, &[], resume.as_ref(), tx)?;
    append_replicated(tx, &verbatim)?;
    apply_keys(keys, tx)?;
    Ok(expect)
}

fn apply_keys(keys: &[(String, Option<String>)], tx: &StoreTx<'_>) -> Result<(), AppError> {
    for (player_id, wrapped) in keys {
        set_player_key(tx, player_id, wrapped.as_deref())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_store::{Store, Table, build_standby_snapshot, read_changes_after};

    use crate::tests::seeded_with_device;

    fn empty() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("standby.db"), crate::tests::T0).unwrap();
        (dir, store)
    }

    fn all_changes(main: &Store) -> Vec<ChangeRecord> {
        main.changes().unwrap()
    }

    #[tokio::test]
    async fn a_follower_ends_with_the_same_rows_and_the_same_log() {
        let (_d, main) = seeded_with_device().await;
        let log = all_changes(&main);
        assert!(log.len() > 3);
        let (_s, mut standby) = empty();
        let (first, rest) = log.split_at(2);
        let players: Vec<String> = log
            .iter()
            .filter(|c| c.tbl == "players")
            .map(|c| c.row_id.clone())
            .collect();
        assert!(!players.is_empty());
        let keys = main.replication_key_states(&players).unwrap();
        assert!(keys.iter().all(|(_, k)| k.is_some()));
        standby
            .write_tx_mut(|tx| apply_replicated(first.to_vec(), &[], tx))
            .unwrap();
        let tip = standby
            .write_tx_mut(|tx| apply_replicated(rest.to_vec(), &keys, tx))
            .unwrap();
        let last = log.last().unwrap();
        assert_eq!(tip, Some((last.epoch.clone(), last.seq)));
        // The keys arrived with the rows of their players.
        assert_eq!(standby.replication_key_states(&players).unwrap(), keys);
        // An erasure on the main Hub reaches the standby in one message.
        let erased: Vec<(String, Option<String>)> =
            players.iter().map(|p| (p.clone(), None)).collect();
        standby
            .write_tx_mut(|tx| apply_replicated(Vec::new(), &erased, tx))
            .unwrap();
        assert!(
            standby
                .replication_key_states(&players)
                .unwrap()
                .iter()
                .all(|(_, k)| k.is_none())
        );
        assert_eq!(standby.changes().unwrap(), log);
        for table in Table::ALL {
            assert_eq!(
                standby.rows(table).unwrap(),
                main.rows(table).unwrap(),
                "{table:?}"
            );
        }
        assert_eq!(
            crate::state_checksum(&standby).unwrap(),
            crate::state_checksum(&main).unwrap()
        );
    }

    #[tokio::test]
    async fn a_batch_that_does_not_continue_the_log_writes_nothing() {
        let (_d, main) = seeded_with_device().await;
        let log = all_changes(&main);
        let (_s, mut standby) = empty();
        standby
            .write_tx_mut(|tx| apply_replicated(log[..2].to_vec(), &[], tx))
            .unwrap();

        // A gap.
        let err = standby
            .write_tx_mut(|tx| apply_replicated(log[3..5].to_vec(), &[], tx))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ReplicateGap, "{err}");
        // A duplicate.
        let err = standby
            .write_tx_mut(|tx| apply_replicated(log[1..4].to_vec(), &[], tx))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ReplicateGap, "{err}");
        // Another epoch.
        let mut moved = log[2].clone();
        moved.epoch = "9:other".into();
        moved.seq = 1;
        let err = standby
            .write_tx_mut(|tx| apply_replicated(vec![moved], &[], tx))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ReplicateGap, "{err}");
        // A log that does not start at 1.
        let (_t, mut fresh) = empty();
        let err = fresh
            .write_tx_mut(|tx| apply_replicated(log[1..3].to_vec(), &[], tx))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ReplicateGap, "{err}");
        assert_eq!(standby.changes().unwrap().len(), 2);
        assert!(fresh.changes().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_snapshot_then_the_rest_equals_the_main_hub() {
        let (dir, main) = seeded_with_device().await;
        let log = all_changes(&main);
        let snap = dir.path().join("snap.db");
        let tip = build_standby_snapshot(main.path(), &snap).unwrap().unwrap();
        let (_s, mut standby) = empty();
        assert_eq!(
            standby.load_standby_snapshot(&snap).unwrap(),
            Some(tip.clone())
        );
        let after =
            read_changes_after(&rusqlite_conn(&standby), Some((&tip.0, tip.1)), 10).unwrap();
        assert!(after.is_empty());
        assert_eq!(standby.changes().unwrap(), log);
        for table in Table::ALL {
            assert_eq!(
                standby.rows(table).unwrap(),
                main.rows(table).unwrap(),
                "{table:?}"
            );
        }
        assert_eq!(
            crate::state_checksum(&standby).unwrap(),
            crate::state_checksum(&main).unwrap()
        );
        // A cleared follower can load again.
        standby.clear_replicated_state().unwrap();
        assert!(standby.changes().unwrap().is_empty());
        assert_eq!(standby.load_standby_snapshot(&snap).unwrap(), Some(tip));
    }

    fn rusqlite_conn(store: &Store) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            store.path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
}

//! `arena-replay`: rebuilds branch rows from the `changes` log.
//!
//! [`replay`] applies each change onto a freshly migrated, empty database, in `(epoch, seq)`
//! order with epochs compared as `(counter, hub_instance_id)`. An insert or update upserts the
//! change's MessagePack row into its table; a delete removes the row with that key. Only the
//! change-log tables ([`Table::ALL`]: `branch`, `settings`) may be written. Replaying a
//! database's whole log must reproduce its rows exactly.

use arena_proto::{AppError, ErrorCode};
use arena_store::{
    ChangeOp, ChangeRecord, Row, RowKey, StoreTx, Table, delete_row, has_branch_data, upsert_row,
};

fn bad(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// Parses `counter:hub_instance_id` into its ordering key.
fn epoch_key(epoch: &str) -> Result<(i64, &str), AppError> {
    epoch
        .split_once(':')
        .and_then(|(c, id)| Some((c.parse::<i64>().ok()?, id)))
        .ok_or_else(|| bad(format!("malformed epoch {epoch:?}")))
}

/// Replays `changes` into `dest`. Fails `INTERNAL`, writing nothing the caller commits, on a
/// destination that already has branch, settings or change rows; a table outside the
/// allow-list; a malformed epoch or row; a row whose key does not match its `row_id`; a delete
/// of a missing row; or a `seq` gap within an epoch. Returns the number of changes applied.
pub fn replay(
    changes: impl IntoIterator<Item = ChangeRecord>,
    dest: &mut StoreTx<'_>,
) -> Result<usize, AppError> {
    if has_branch_data(dest)? {
        return Err(bad("replay destination is not empty".to_owned()));
    }
    let mut changes: Vec<ChangeRecord> = changes.into_iter().collect();
    for c in &changes {
        epoch_key(&c.epoch)?;
    }
    changes.sort_by(|a, b| {
        let ka = epoch_key(&a.epoch).expect("checked");
        let kb = epoch_key(&b.epoch).expect("checked");
        ka.cmp(&kb).then(a.seq.cmp(&b.seq))
    });

    let mut prev: Option<(&str, i64)> = None;
    for c in &changes {
        let expected = match prev {
            Some((epoch, seq)) if epoch == c.epoch => seq + 1,
            _ => 1,
        };
        if c.seq != expected {
            return Err(bad(format!(
                "seq gap in epoch {}: expected {expected}, found {}",
                c.epoch, c.seq
            )));
        }
        prev = Some((&c.epoch, c.seq));

        let table = Table::from_name(&c.tbl)
            .ok_or_else(|| bad(format!("table {:?} is not replayable", c.tbl)))?;
        let row: Row = rmp_serde::from_slice(&c.row)
            .map_err(|e| bad(format!("change {}:{} has a bad row: {e}", c.epoch, c.seq)))?;
        match RowKey::from_row(table, &row) {
            Some(key) if key.row_id() == c.row_id => {}
            _ => {
                return Err(bad(format!(
                    "change {}:{} row does not match row_id {:?}",
                    c.epoch, c.seq, c.row_id
                )));
            }
        }
        match c.op {
            ChangeOp::Insert | ChangeOp::Update => upsert_row(dest, table, &row)?,
            ChangeOp::Delete => {
                if delete_row(dest, table, &row)? == 0 {
                    return Err(bad(format!(
                        "change {}:{} deletes missing row {:?}",
                        c.epoch, c.seq, c.row_id
                    )));
                }
            }
        }
    }
    Ok(changes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use arena_core::{HubStatus, LeaseInfo, ManualClock, Role, Writer, status_channel};
    use arena_proto::{Actor, ActorKind, BranchBootstrap, Intent, IntentKind, IntentPayload};
    use arena_store::{LeaseRow, RowValue, Store, write_lease};

    const T0: i64 = 1_800_000_000_000;

    /// A database seeded through the Writer with `branch.bootstrap`, as `dev seed` does.
    async fn seeded() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, T0).unwrap();
        let lease = LeaseRow {
            counter: 1,
            hub_instance_id: "01JHUB".into(),
            provisional: true,
            renewed_at: T0,
            expires_at: T0 + 15_000,
        };
        store.write_tx(|tx| write_lease(tx, &lease)).unwrap();
        let mut status = HubStatus::opened(store.schema_version().unwrap(), "01JHUB");
        status.role = Role::Primary;
        status.lease = Some(LeaseInfo::from(&lease));
        let (_tx, rx) = status_channel(status);
        let writer = Writer::spawn(store, Arc::new(ManualClock::new(T0)), rx).unwrap();
        writer
            .submit_intent(Intent {
                intent_id: ulid::Ulid::generate().to_string(),
                kind: IntentKind::BranchBootstrap,
                idempotency_key: ulid::Ulid::generate().to_string(),
                issued_at: T0,
                base_rev: None,
                actor: Actor {
                    kind: ActorKind::System,
                    id: "dev-seed".into(),
                },
                elevated_by: None,
                payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 360,
                    fy_start_month: 4,
                }),
            })
            .await
            .unwrap();
        writer.close().await;
        let store = Store::open(&path, T0).unwrap();
        (dir, store)
    }

    fn empty() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("replica.db"), T0).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn replaying_the_seeded_log_reproduces_branch_and_settings() {
        let (_a, source) = seeded().await;
        let (_b, mut dest) = empty();
        let changes = source.changes().unwrap();
        assert!(!changes.is_empty());
        // Order of the input does not matter.
        let mut shuffled = changes.clone();
        shuffled.reverse();
        let n = dest.write_tx_mut(|tx| replay(shuffled, tx)).unwrap();
        assert_eq!(n, changes.len());
        for table in Table::ALL {
            let want = source.rows(table).unwrap();
            assert!(!want.is_empty());
            assert_eq!(dest.rows(table).unwrap(), want, "{}", table.name());
        }
        assert!(
            dest.changes().unwrap().is_empty(),
            "replay writes rows, not changes"
        );
    }

    #[tokio::test]
    async fn update_and_delete_changes_apply_in_order() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        let last = changes.last().unwrap().clone();
        let mut row: Row = rmp_serde::from_slice(&last.row).unwrap();
        row.insert("value".into(), RowValue::Text("edited".into()));
        let edited = rmp_serde::to_vec_named(&row).unwrap();
        changes.push(ChangeRecord {
            seq: last.seq + 1,
            op: ChangeOp::Update,
            row: edited.clone(),
            ..last.clone()
        });
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes.clone(), tx)).unwrap();
        assert!(dest.rows(Table::Settings).unwrap().contains(&row));

        changes.push(ChangeRecord {
            seq: last.seq + 2,
            op: ChangeOp::Delete,
            row: edited,
            ..last
        });
        let (_c, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes, tx)).unwrap();
        assert!(!dest.rows(Table::Settings).unwrap().contains(&row));
        assert_eq!(
            dest.rows(Table::Settings).unwrap().len(),
            source.rows(Table::Settings).unwrap().len() - 1
        );
    }

    #[tokio::test]
    async fn tables_outside_the_allow_list_are_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes[0].tbl = "lease".into();
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(err.message.contains("not replayable"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_seq_gap_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes.remove(1);
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("seq gap"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_empty_destination_is_refused() {
        let (_a, source) = seeded().await;
        let changes = source.changes().unwrap();
        let (_b, mut dest) = empty();
        dest.write_tx_mut(|tx| replay(changes.clone(), tx)).unwrap();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("not empty"), "{err}");
    }

    #[tokio::test]
    async fn a_row_id_mismatch_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        changes[1].row_id = "someone/else".into();
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("does not match row_id"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_missing_row_is_refused() {
        let (_a, source) = seeded().await;
        let mut changes = source.changes().unwrap();
        let last = changes.last().unwrap().clone();
        for seq in [last.seq + 1, last.seq + 2] {
            changes.push(ChangeRecord {
                seq,
                op: ChangeOp::Delete,
                ..last.clone()
            });
        }
        let (_b, mut dest) = empty();
        let err = dest.write_tx_mut(|tx| replay(changes, tx)).unwrap_err();
        assert!(err.message.contains("deletes missing row"), "{err}");
        assert!(dest.rows(Table::Branch).unwrap().is_empty());
    }

    #[test]
    fn epochs_order_numerically_by_counter() {
        assert!(epoch_key("10:A").unwrap() > epoch_key("9:Z").unwrap());
        assert!(epoch_key("1:B").unwrap() > epoch_key("1:A").unwrap());
        assert!(epoch_key("x:A").is_err());
        assert!(epoch_key("1").is_err());
    }
}

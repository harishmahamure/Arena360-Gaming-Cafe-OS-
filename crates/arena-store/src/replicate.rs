//! What the standby replication needs from the database (Story 9.19, AD-4).
//!
//! The main Hub side: which paired devices are linked as standby, the change log read in log
//! order after a position, the key state of the players a batch touches, and the snapshot (a
//! `VACUUM INTO` copy at one consistent point with the Hub-local tables cleared). The standby
//! side: loading that snapshot into an empty database (only the change-log tables, `changes` and
//! `player_keys` are copied, so the standby keeps its own identity, lease, licence and caches) and
//! storing or deleting a player key.

use std::path::Path;
use std::time::Duration;

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::branch::{CHANGE_COLUMNS, ChangeRecord, Table, change_from, insert_change};
use crate::{Store, db_error, delete_player_key, has_branch_data};

/// The tables a standby never receives from the main Hub: identity, lease, licence and backup
/// state belong to the Hub that holds them.
const HUB_LOCAL: [&str; 14] = [
    "hub_meta",
    "lease",
    "hub_identity",
    "licence_cache",
    "trusted_clock",
    "upload_cursor",
    "rollup_log",
    "snapshot_log",
    "backup_state",
    "gst_archive_log",
    "standby_links",
    "changes_prune_permit",
    "owner_overrides",
    "applied_commands",
];

fn table_exists(conn: &Connection, schema: &str, name: &str) -> Result<bool, AppError> {
    conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM {schema}.sqlite_master WHERE type = 'table' AND name = ?1)"),
        [name],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Lets `device_id` replicate from this Hub (the owner linked it as standby).
pub fn link_standby(conn: &Connection, device_id: &str, now_ms: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO standby_links (device_id, linked_at) VALUES (?1, ?2)",
        params![device_id, now_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Whether `device_id` is linked as standby.
pub fn is_standby_linked(conn: &Connection, device_id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM standby_links WHERE device_id = ?1)",
        [device_id],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// The device linked as this branch's standby, with what the Hub knows of it (Story 9.20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedStandby {
    pub device_id: String,
    /// The device's name as the owner gave it; empty when the device row is gone.
    pub label: String,
    pub linked_at: i64,
    /// The app version the device last reported.
    pub reported_version: Option<String>,
}

/// The linked standby: the newest link whose device is not revoked, if any.
pub fn linked_standby(conn: &Connection) -> Result<Option<LinkedStandby>, AppError> {
    conn.query_row(
        "SELECT l.device_id, COALESCE(d.label, ''), l.linked_at, d.reported_version \
         FROM standby_links l LEFT JOIN devices d ON d.device_id = l.device_id \
         WHERE d.revoked_at IS NULL \
         ORDER BY l.linked_at DESC, l.device_id DESC LIMIT 1",
        [],
        |r| {
            Ok(LinkedStandby {
                device_id: r.get(0)?,
                label: r.get(1)?,
                linked_at: r.get(2)?,
                reported_version: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The last change in log order (`epoch_counter`, `epoch`, `seq`), if the log has any.
pub fn log_tip(conn: &Connection) -> Result<Option<(String, i64)>, AppError> {
    conn.query_row(
        "SELECT epoch, seq FROM changes ORDER BY epoch_counter DESC, epoch DESC, seq DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db_error)
}

/// Whether the log holds the change `(epoch, seq)`.
pub fn log_holds(conn: &Connection, epoch: &str, seq: i64) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM changes WHERE epoch = ?1 AND seq = ?2)",
        params![epoch, seq],
        |r| r.get(0),
    )
    .map_err(db_error)
}

fn epoch_counter(epoch: &str) -> Result<i64, AppError> {
    epoch
        .split_once(':')
        .and_then(|(c, _)| c.parse().ok())
        .ok_or_else(|| AppError::new(ErrorCode::Internal, format!("malformed epoch {epoch:?}")))
}

/// Up to `limit` changes after `after` in log order (all of them when `after` is `None`).
pub fn read_changes_after(
    conn: &Connection,
    after: Option<(&str, i64)>,
    limit: usize,
) -> Result<Vec<ChangeRecord>, AppError> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let (counter, epoch, seq) = match after {
        Some((epoch, seq)) => (epoch_counter(epoch)?, epoch.to_owned(), seq),
        None => (i64::MIN, String::new(), i64::MIN),
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CHANGE_COLUMNS} FROM changes
             WHERE (epoch_counter, epoch, seq) > (?1, ?2, ?3)
             ORDER BY epoch_counter, epoch, seq LIMIT ?4"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![counter, epoch, seq, limit], change_from)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)
}

/// The stored key of each player in `player_ids`: `None` when the player has none (never had one,
/// or was erased).
pub fn player_key_states(
    conn: &Connection,
    player_ids: &[String],
) -> Result<Vec<(String, Option<String>)>, AppError> {
    let mut out = Vec::with_capacity(player_ids.len());
    for id in player_ids {
        out.push((id.clone(), crate::find_player_key(conn, id)?));
    }
    Ok(out)
}

/// Stores `wrapped` as the player's key, or deletes the key when it is `None`. A key for a player
/// the standby does not hold is ignored (the key arrives with the batch that creates the row).
pub fn set_player_key(
    conn: &Connection,
    player_id: &str,
    wrapped: Option<&str>,
) -> Result<(), AppError> {
    match wrapped {
        None => delete_player_key(conn, player_id),
        Some(w) => {
            let known: bool = conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM players WHERE player_id = ?1)",
                    [player_id],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if !known {
                return Ok(());
            }
            conn.execute(
                "INSERT INTO player_keys (player_id, dek_wrapped) VALUES (?1, ?2)
                 ON CONFLICT (player_id) DO UPDATE SET dek_wrapped = excluded.dek_wrapped",
                params![player_id, w],
            )
            .map_err(db_error)?;
            Ok(())
        }
    }
}

/// Writes `changes` verbatim at the end of the log: same epoch, same `seq`. The caller has checked
/// that they continue the log.
pub fn append_replicated(conn: &Connection, changes: &[ChangeRecord]) -> Result<(), AppError> {
    for c in changes {
        insert_change(conn, c)?;
    }
    Ok(())
}

/// A consistent copy of the database at `db_path` in `scratch`, ready to send to a standby: one
/// `VACUUM INTO` (a single read snapshot while the writer keeps committing), then the Hub-local
/// tables cleared. Returns the position the copy is consistent at (`None` for an empty log).
pub fn build_standby_snapshot(
    db_path: &Path,
    scratch: &Path,
) -> Result<Option<(String, i64)>, AppError> {
    let source = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    source
        .execute("VACUUM INTO ?1", [scratch.to_string_lossy().as_ref()])
        .map_err(db_error)?;
    drop(source);

    let copy = Connection::open(scratch).map_err(db_error)?;
    copy.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA secure_delete = ON;")
        .map_err(db_error)?;
    for table in HUB_LOCAL {
        // A change-log table is branch state (it replicates), whatever it holds.
        if Table::from_name(table).is_some() {
            continue;
        }
        if table_exists(&copy, "main", table)? {
            copy.execute_batch(&format!("DELETE FROM {table};"))
                .map_err(db_error)?;
        }
    }
    let tip = log_tip(&copy)?;
    copy.execute_batch("VACUUM;").map_err(db_error)?;
    Ok(tip)
}

/// The columns of `schema.table` that can be written (generated columns are left out).
fn writable_columns(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA {schema}.table_xinfo({table})"))
        .map_err(db_error)?;
    stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(6)?)))
        .map_err(db_error)?
        .filter_map(|c| match c {
            Ok((name, hidden)) => (hidden == 0).then_some(Ok(name)),
            Err(e) => Some(Err(db_error(e))),
        })
        .collect()
}

impl Store {
    /// The last change in log order.
    pub fn replication_tip(&self) -> Result<Option<(String, i64)>, AppError> {
        log_tip(&self.conn)
    }

    /// Whether the log holds the change `(epoch, seq)`.
    pub fn replication_holds(&self, epoch: &str, seq: i64) -> Result<bool, AppError> {
        log_holds(&self.conn, epoch, seq)
    }

    /// Up to `limit` changes after `after` in log order.
    pub fn replication_changes_after(
        &self,
        after: Option<(&str, i64)>,
        limit: usize,
    ) -> Result<Vec<ChangeRecord>, AppError> {
        read_changes_after(&self.conn, after, limit)
    }

    /// The stored key of each player (`None` when there is none).
    pub fn replication_key_states(
        &self,
        player_ids: &[String],
    ) -> Result<Vec<(String, Option<String>)>, AppError> {
        player_key_states(&self.conn, player_ids)
    }

    /// Whether `device_id` is linked as standby.
    pub fn replication_linked(&self, device_id: &str) -> Result<bool, AppError> {
        is_standby_linked(&self.conn, device_id)
    }

    /// The branch's name, as replicated (Story 9.20): what a standby calls the Hub it follows.
    pub fn branch_name(&self) -> Result<Option<String>, AppError> {
        if self.schema_version()? == 0 {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT name FROM branch ORDER BY branch_id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)
    }

    /// The device linked as the standby, if any (Story 9.20).
    pub fn linked_standby(&self) -> Result<Option<LinkedStandby>, AppError> {
        linked_standby(&self.conn)
    }
}

impl Store {
    /// Empties everything a snapshot fills: the change-log tables, `changes` and `player_keys`. A
    /// standby does this before it takes a fresh snapshot, because its position is gone from the
    /// main Hub's log. The Hub's own identity, lease, licence and caches stay. `changes` is
    /// append-only, so its delete guard is lifted inside this one transaction and put back as it
    /// was.
    pub fn clear_replicated_state(&mut self) -> Result<(), AppError> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")
            .map_err(db_error)?;
        let mut names: Vec<&str> = Table::ALL.iter().map(|t| t.name()).collect();
        names.push("changes");
        names.push("player_keys");
        // Delete guards (append-only tables) are lifted for this one transaction and put back.
        let guards: Vec<(String, String)> = {
            let mut stmt = tx
                .prepare("SELECT name, sql, tbl_name FROM sqlite_master WHERE type = 'trigger'")
                .map_err(db_error)?;
            stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
            .into_iter()
            .filter(|(_, sql, tbl)| {
                names.contains(&tbl.as_str()) && sql.to_ascii_uppercase().contains("BEFORE DELETE")
            })
            .map(|(name, sql, _)| (name, sql))
            .collect()
        };
        for (name, _) in &guards {
            tx.execute_batch(&format!("DROP TRIGGER \"{name}\";"))
                .map_err(db_error)?;
        }
        tx.execute_batch("DELETE FROM changes; DELETE FROM player_keys;")
            .map_err(db_error)?;
        for table in Table::ALL.iter().rev() {
            tx.execute_batch(&format!("DELETE FROM {};", table.name()))
                .map_err(db_error)?;
        }
        for (_, sql) in &guards {
            tx.execute_batch(sql).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Loads a snapshot built by [`build_standby_snapshot`] into this empty database and returns
    /// its position. Only the change-log tables, `changes` and `player_keys` are copied, in one
    /// transaction. Fails `INTERNAL` when this database already has branch data or a log, or when
    /// the snapshot's schema is not this binary's; fails `SCHEMA_BEHIND` when the snapshot is
    /// from a newer schema (this Hub must update first).
    pub fn load_standby_snapshot(
        &mut self,
        snapshot: &Path,
    ) -> Result<Option<(String, i64)>, AppError> {
        if has_branch_data(&self.conn)? {
            return Err(AppError::new(
                ErrorCode::Internal,
                "the snapshot can only be loaded into an empty database",
            ));
        }
        self.conn
            .execute(
                "ATTACH DATABASE ?1 AS snap",
                [snapshot.to_string_lossy().as_ref()],
            )
            .map_err(db_error)?;
        let loaded = self.copy_snapshot_rows();
        let _ = self.conn.execute_batch("DETACH DATABASE snap");
        loaded
    }

    fn copy_snapshot_rows(&mut self) -> Result<Option<(String, i64)>, AppError> {
        let theirs: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM snap.schema_version",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        let ours = i64::from(self.schema_version()?);
        if theirs > ours {
            return Err(AppError::new(
                ErrorCode::SchemaBehind,
                "the snapshot is from a newer schema",
            ));
        }
        if theirs < ours {
            return Err(AppError::new(
                ErrorCode::Internal,
                "the snapshot is from an older schema",
            ));
        }
        let mut tables: Vec<&str> = Table::ALL.iter().map(|t| t.name()).collect();
        tables.push("changes");
        tables.push("player_keys");
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")
            .map_err(db_error)?;
        for table in tables {
            if !table_exists(&tx, "snap", table)? {
                continue;
            }
            let cols = writable_columns(&tx, "main", table)?.join(", ");
            tx.execute_batch(&format!(
                "INSERT INTO main.{table} ({cols}) SELECT {cols} FROM snap.{table};"
            ))
            .map_err(db_error)?;
        }
        let tip = log_tip(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok(tip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("hub.db"), 1).unwrap();
        (dir, s)
    }

    fn change(epoch: &str, seq: i64) -> ChangeRecord {
        ChangeRecord {
            epoch: epoch.into(),
            seq,
            tbl: "settings".into(),
            row_id: format!("b/k{seq}"),
            op: crate::ChangeOp::Insert,
            row: vec![0x80],
            intent_id: format!("i{seq}"),
            intent_kind: "settings.set".into(),
            actor_kind: "system".into(),
            actor_id: "t".into(),
            elevated_by_kind: None,
            elevated_by_id: None,
            elevated_by_mode: None,
            elevated_by_verifier_version: None,
            at: seq,
            business_day: None,
            fy: None,
            reason: None,
        }
    }

    fn append(s: &mut Store, c: &[ChangeRecord]) {
        s.write_tx(|tx| append_replicated(tx, c)).unwrap();
    }

    #[test]
    fn the_linked_standby_is_the_newest_link_that_is_not_revoked() {
        let (_d, mut s) = store();
        assert_eq!(s.linked_standby().unwrap(), None);
        s.write_tx(|tx| link_standby(tx, "dev-a", 10)).unwrap();
        let only = s.linked_standby().unwrap().unwrap();
        assert_eq!(
            (only.device_id.as_str(), only.label.as_str()),
            ("dev-a", "")
        );
        assert_eq!(only.linked_at, 10);

        s.conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        s.conn
            .execute_batch(
                "INSERT INTO devices (device_id, branch_id, class, label, token_hash, \
                 reported_version, revoked_at, rev, created_at, updated_at) VALUES \
                 ('dev-a', 'b', 'paired', 'Back office PC', 'h', '0.9.1', NULL, 1, 1, 1), \
                 ('dev-b', 'b', 'paired', 'Old PC', 'h', NULL, 5, 1, 1, 1);",
            )
            .unwrap();
        s.write_tx(|tx| link_standby(tx, "dev-b", 20)).unwrap();
        let shown = s.linked_standby().unwrap().unwrap();
        assert_eq!(
            shown.device_id, "dev-a",
            "a revoked device is not the standby"
        );
        assert_eq!(shown.label, "Back office PC");
        assert_eq!(shown.reported_version.as_deref(), Some("0.9.1"));
    }

    #[test]
    fn changes_are_read_in_log_order_across_epochs() {
        let (_d, mut s) = store();
        append(
            &mut s,
            &[
                change("2:b", 1),
                change("10:a", 1),
                change("2:b", 2),
                change("2:a", 1),
            ],
        );
        let all = read_changes_after(&s.conn, None, 100).unwrap();
        let order: Vec<_> = all.iter().map(|c| (c.epoch.as_str(), c.seq)).collect();
        assert_eq!(order, [("2:a", 1), ("2:b", 1), ("2:b", 2), ("10:a", 1)]);
        let after = read_changes_after(&s.conn, Some(("2:b", 1)), 100).unwrap();
        assert_eq!(after.len(), 2);
        assert_eq!(
            read_changes_after(&s.conn, Some(("2:b", 1)), 1)
                .unwrap()
                .len(),
            1
        );
        assert!(
            read_changes_after(&s.conn, Some(("10:a", 1)), 5)
                .unwrap()
                .is_empty()
        );
        assert_eq!(log_tip(&s.conn).unwrap(), Some(("10:a".into(), 1)));
        assert!(log_holds(&s.conn, "2:b", 2).unwrap());
        assert!(!log_holds(&s.conn, "2:b", 3).unwrap());
    }

    #[test]
    fn standby_links_are_per_device() {
        let (_d, s) = store();
        assert!(!is_standby_linked(&s.conn, "dev").unwrap());
        link_standby(&s.conn, "dev", 5).unwrap();
        assert!(is_standby_linked(&s.conn, "dev").unwrap());
        assert!(!is_standby_linked(&s.conn, "other").unwrap());
    }

    #[test]
    fn a_snapshot_loads_into_an_empty_database_and_keeps_its_identity() {
        let (dir, mut main) = store();
        main.write_tx(|tx| crate::set_meta(tx, crate::META_HUB_INSTANCE_ID, "main-id"))
            .unwrap();
        append(&mut main, &[change("1:m", 1), change("1:m", 2)]);
        let snap = dir.path().join("snap.db");
        let tip = build_standby_snapshot(main.path(), &snap).unwrap();
        assert_eq!(tip, Some(("1:m".into(), 2)));

        let other = tempfile::tempdir().unwrap();
        let mut standby = Store::open(other.path().join("standby.db"), 1).unwrap();
        standby
            .write_tx(|tx| crate::set_meta(tx, crate::META_HUB_INSTANCE_ID, "standby-id"))
            .unwrap();
        let loaded = standby.load_standby_snapshot(&snap).unwrap();
        assert_eq!(loaded, tip);
        assert_eq!(standby.changes().unwrap().len(), 2);
        assert_eq!(
            crate::get_meta(&standby.conn, crate::META_HUB_INSTANCE_ID).unwrap(),
            Some("standby-id".into())
        );
        // A second load into the now non-empty database is refused.
        let err = standby.load_standby_snapshot(&snap).unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
    }

    #[test]
    fn player_keys_are_stored_replaced_and_deleted() {
        let (_d, s) = store();
        // A key for a player the database does not hold is ignored.
        set_player_key(&s.conn, "ghost", Some("w")).unwrap();
        assert_eq!(crate::find_player_key(&s.conn, "ghost").unwrap(), None);
        assert_eq!(
            player_key_states(&s.conn, &["ghost".to_owned()]).unwrap(),
            vec![("ghost".to_owned(), None)]
        );
        set_player_key(&s.conn, "ghost", None).unwrap();
    }

    #[test]
    fn the_standby_keeps_its_own_licence_through_a_snapshot_and_a_resync() {
        let (dir, mut main) = store();
        main.write_tx(|tx| crate::store_licence(tx, "main-licence", 5))
            .unwrap();
        append(&mut main, &[change("1:m", 1)]);
        let snap = dir.path().join("snap.db");
        build_standby_snapshot(main.path(), &snap).unwrap();
        // The licence is the main Hub's own: it is not in what the standby is given.
        let copy = Connection::open(&snap).unwrap();
        let held: i64 = copy
            .query_row("SELECT COUNT(*) FROM licence_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(held, 0);

        let other = tempfile::tempdir().unwrap();
        let mut standby = Store::open(other.path().join("standby.db"), 1).unwrap();
        standby
            .write_tx(|tx| crate::store_licence(tx, "standby-licence", 9))
            .unwrap();
        standby.load_standby_snapshot(&snap).unwrap();
        assert_eq!(
            standby.cached_licence().unwrap().unwrap().jwt,
            "standby-licence"
        );
        standby.clear_replicated_state().unwrap();
        assert_eq!(
            standby.cached_licence().unwrap().unwrap().jwt,
            "standby-licence"
        );
    }
}

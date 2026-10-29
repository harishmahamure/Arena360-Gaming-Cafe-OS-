use arena_proto::AppError;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCommand {
    pub command_id: i64,
    pub idempotency_key: String,
    pub kind: String,
    pub applied: bool,
    pub applied_seq: Option<i64>,
    pub error: Option<String>,
    pub applied_at: i64,
    pub reported_at: Option<i64>,
}

const COLUMNS: &str = "command_id, idempotency_key, kind, status, applied_seq, error, \
     applied_at, reported_at";

fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AppliedCommand> {
    Ok(AppliedCommand {
        command_id: r.get(0)?,
        idempotency_key: r.get(1)?,
        kind: r.get(2)?,
        applied: r.get::<_, String>(3)? == "applied",
        applied_seq: r.get(4)?,
        error: r.get(5)?,
        applied_at: r.get(6)?,
        reported_at: r.get(7)?,
    })
}

pub fn find_applied_command(
    conn: &Connection,
    command_id: i64,
    idempotency_key: &str,
) -> Result<Option<AppliedCommand>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM applied_commands WHERE command_id = ?1 OR idempotency_key = ?2"
        ),
        params![command_id, idempotency_key],
        from_row,
    )
    .optional()
    .map_err(db_error)
}

pub fn insert_applied_command(conn: &Connection, c: &AppliedCommand) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "INSERT INTO applied_commands (command_id, idempotency_key, kind, status, \
             applied_seq, error, applied_at, reported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT DO NOTHING",
            params![
                c.command_id,
                c.idempotency_key,
                c.kind,
                if c.applied { "applied" } else { "rejected" },
                c.applied_seq,
                c.error,
                c.applied_at,
                c.reported_at,
            ],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

pub fn max_applied_command_id(conn: &Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(command_id), 0) FROM applied_commands",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

pub fn unreported_commands(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<AppliedCommand>, AppError> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM applied_commands WHERE reported_at IS NULL \
             ORDER BY command_id LIMIT ?1"
        ))
        .map_err(db_error)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    st.query_map([limit], from_row)
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

pub fn mark_command_reported(conn: &Connection, command_id: i64, at: i64) -> Result<(), AppError> {
    conn.execute(
        "UPDATE applied_commands SET reported_at = ?2 WHERE command_id = ?1 AND reported_at IS NULL",
        params![command_id, at],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The change an owner command overwrote (Story 8.19), keyed by the owner change's
/// `(epoch, seq)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerOverride {
    pub epoch: String,
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
    /// The overwritten change's intent, actor and time (Hub clock, ms).
    pub intent_id: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub at: i64,
}

pub fn insert_owner_override(conn: &Connection, o: &OwnerOverride) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO owner_overrides (epoch, seq, tbl, row_id, intent_id, actor_kind, actor_id, at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT DO NOTHING",
        params![
            o.epoch,
            o.seq,
            o.tbl,
            o.row_id,
            o.intent_id,
            o.actor_kind,
            o.actor_id,
            o.at
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The override recorded for the owner change `(epoch, seq)`. `None` also on a database not yet
/// migrated to the table.
pub fn find_owner_override(
    conn: &Connection,
    epoch: &str,
    seq: i64,
) -> Result<Option<OwnerOverride>, AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'owner_overrides')",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Ok(None);
    }
    conn.query_row(
        "SELECT epoch, seq, tbl, row_id, intent_id, actor_kind, actor_id, at \
         FROM owner_overrides WHERE epoch = ?1 AND seq = ?2",
        params![epoch, seq],
        |r| {
            Ok(OwnerOverride {
                epoch: r.get(0)?,
                seq: r.get(1)?,
                tbl: r.get(2)?,
                row_id: r.get(3)?,
                intent_id: r.get(4)?,
                actor_kind: r.get(5)?,
                actor_id: r.get(6)?,
                at: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The newest change row of `tbl`/`row_id` (the later epoch counter, then the higher `seq`).
pub fn newest_change_of(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
) -> Result<Option<crate::ChangeRecord>, AppError> {
    conn.query_row(
        &format!(
            "SELECT {} FROM changes WHERE tbl = ?1 AND row_id = ?2 \
             ORDER BY CAST(substr(epoch, 1, instr(epoch, ':') - 1) AS INTEGER) DESC, epoch DESC, \
             seq DESC LIMIT 1",
            crate::branch::CHANGE_COLUMNS
        ),
        params![tbl, row_id],
        crate::branch::change_from,
    )
    .optional()
    .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn an_override_is_stored_once_by_the_owner_change_and_found_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 1).unwrap();
        let o = OwnerOverride {
            epoch: "1:abc".into(),
            seq: 9,
            tbl: "plans".into(),
            row_id: "P1".into(),
            intent_id: "I1".into(),
            actor_kind: "staff".into(),
            actor_id: "S1".into(),
            at: 77,
        };
        store
            .write_tx(|tx| {
                assert_eq!(find_owner_override(tx, "1:abc", 9)?, None);
                insert_owner_override(tx, &o)?;
                insert_owner_override(tx, &OwnerOverride { at: 1, ..o.clone() })?;
                assert_eq!(find_owner_override(tx, "1:abc", 9)?, Some(o.clone()));
                assert_eq!(find_owner_override(tx, "1:abc", 10)?, None);
                assert_eq!(newest_change_of(tx, "plans", "P1")?, None);
                Ok(())
            })
            .unwrap();
    }

    fn cmd(id: i64, key: &str, applied: bool) -> AppliedCommand {
        AppliedCommand {
            command_id: id,
            idempotency_key: key.into(),
            kind: "group.upsert".into(),
            applied,
            applied_seq: applied.then_some(7),
            error: (!applied).then(|| "{}".into()),
            applied_at: 10,
            reported_at: None,
        }
    }

    #[test]
    fn a_command_is_recorded_once_found_by_id_or_key_and_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db"), 1).unwrap();
        store
            .write_tx(|tx| {
                assert_eq!(max_applied_command_id(tx).unwrap(), 0);
                assert!(insert_applied_command(tx, &cmd(5, "K5", true))?);
                assert!(!insert_applied_command(tx, &cmd(5, "OTHER", false))?);
                assert!(!insert_applied_command(tx, &cmd(6, "K5", false))?);
                assert!(insert_applied_command(tx, &cmd(9, "K9", false))?);
                assert_eq!(max_applied_command_id(tx)?, 9);
                let by_id = find_applied_command(tx, 5, "nope")?.unwrap();
                assert_eq!((by_id.applied, by_id.applied_seq), (true, Some(7)));
                assert_eq!(find_applied_command(tx, 0, "K9")?.unwrap().command_id, 9);
                assert!(find_applied_command(tx, 1, "none")?.is_none());
                let open = unreported_commands(tx, 10)?;
                assert_eq!(
                    open.iter().map(|c| c.command_id).collect::<Vec<_>>(),
                    [5, 9]
                );
                mark_command_reported(tx, 5, 20)?;
                mark_command_reported(tx, 5, 30)?;
                let open = unreported_commands(tx, 10)?;
                assert_eq!(open.len(), 1);
                assert_eq!(
                    find_applied_command(tx, 5, "")?.unwrap().reported_at,
                    Some(20)
                );
                Ok(())
            })
            .unwrap();
    }
}

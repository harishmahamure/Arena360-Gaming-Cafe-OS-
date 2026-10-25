use arena_proto::CommandStatus;

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommand<'a> {
    pub branch_id: &'a str,
    pub idempotency_key: &'a str,
    pub kind: &'a str,
    pub schema_version: u32,
    pub payload: &'a str,
    pub issued_by: &'a str,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRow {
    pub id: i64,
    pub branch_id: String,
    pub idempotency_key: String,
    pub kind: String,
    pub schema_version: u32,
    pub payload: String,
    pub issued_by: String,
    pub status: CommandStatus,
    pub error: Option<String>,
    pub applied_seq: Option<i64>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

const SELECT_BY_KEY: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at FROM commands \
     WHERE branch_id = ? AND idempotency_key = ?";

const SELECT_OPEN: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at FROM commands \
     WHERE branch_id = ? AND id > ? AND status IN ('pending', 'fetched') ORDER BY id LIMIT ?";

const SELECT_BY_ID: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at FROM commands \
     WHERE id = ?";

fn command_from(row: &[SqlValue]) -> Result<CommandRow, ControlError> {
    let status = col_text(row, 7)?;
    Ok(CommandRow {
        id: col_int(row, 0)?,
        branch_id: col_text(row, 1)?,
        idempotency_key: col_text(row, 2)?,
        kind: col_text(row, 3)?,
        schema_version: u32::try_from(col_int(row, 4)?)
            .map_err(|_| ControlError::Db("command schema_version out of range".into()))?,
        payload: col_text(row, 5)?,
        issued_by: col_text(row, 6)?,
        status: CommandStatus::parse(&status)
            .ok_or_else(|| ControlError::Db(format!("unknown command status {status:?}")))?,
        error: col_opt_text(row, 8)?,
        applied_seq: col_opt_int(row, 9)?,
        created_at: col_int(row, 10)?,
        resolved_at: col_opt_int(row, 11)?,
    })
}

pub(super) fn insert_command(c: &NewCommand<'_>) -> Stmt {
    Stmt::new(
        "INSERT INTO commands (branch_id, idempotency_key, kind, schema_version, \
         payload, issued_by, status, created_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?) \
         ON CONFLICT (branch_id, idempotency_key) DO NOTHING",
        vec![
            text(c.branch_id),
            text(c.idempotency_key),
            text(c.kind),
            SqlValue::Integer(i64::from(c.schema_version)),
            text(c.payload),
            text(c.issued_by),
            SqlValue::Integer(c.now),
        ],
    )
}

impl ControlDb {
    pub async fn submit_command(&self, c: &NewCommand<'_>) -> Result<CommandRow, ControlError> {
        let rows = self
            .transact_rows(vec![
                insert_command(c),
                Stmt::new(
                    SELECT_BY_KEY,
                    vec![text(c.branch_id), text(c.idempotency_key)],
                )
                .fetch(),
            ])
            .await?;
        rows.and_then(|r| {
            r.get(1)
                .and_then(|x| x.first().map(|row| command_from(row)))
        })
        .ok_or_else(|| ControlError::Db("submitted command not found".into()))?
    }

    pub async fn fetch_commands(
        &self,
        branch_id: &str,
        after: i64,
        limit: usize,
        now: i64,
    ) -> Result<Vec<CommandRow>, ControlError> {
        let limit = SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX));
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE commands SET status = 'fetched', fetched_at = COALESCE(fetched_at, ?) \
                     WHERE id IN (SELECT id FROM commands WHERE branch_id = ? AND id > ? \
                     AND status IN ('pending', 'fetched') ORDER BY id LIMIT ?) \
                     AND status = 'pending'",
                    vec![
                        SqlValue::Integer(now),
                        text(branch_id),
                        SqlValue::Integer(after),
                        limit.clone(),
                    ],
                ),
                Stmt::new(
                    SELECT_OPEN,
                    vec![text(branch_id), SqlValue::Integer(after), limit],
                )
                .fetch(),
            ])
            .await?;
        rows.and_then(|r| {
            r.get(1)
                .map(|x| x.iter().map(|row| command_from(row)).collect())
        })
        .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub async fn resolve_command(
        &self,
        branch_id: &str,
        id: i64,
        applied_seq: Option<i64>,
        error: Option<&str>,
        now: i64,
    ) -> Result<Option<CommandStatus>, ControlError> {
        let status = if error.is_some() {
            CommandStatus::Rejected
        } else {
            CommandStatus::Applied
        };
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE commands SET status = ?, applied_seq = ?, error = ?, resolved_at = ? \
                     WHERE id = ? AND branch_id = ? AND status IN ('pending', 'fetched')",
                    vec![
                        text(status.as_str()),
                        applied_seq.map_or(SqlValue::Null, SqlValue::Integer),
                        error.map_or(SqlValue::Null, text),
                        SqlValue::Integer(now),
                        SqlValue::Integer(id),
                        text(branch_id),
                    ],
                ),
                Stmt::new(
                    "SELECT status FROM commands WHERE id = ? AND branch_id = ?",
                    vec![SqlValue::Integer(id), text(branch_id)],
                )
                .fetch(),
            ])
            .await?;
        let status = rows
            .and_then(|r| r.get(1).and_then(|x| x.first().map(|row| col_text(row, 0))))
            .transpose()?;
        status
            .map(|s| {
                CommandStatus::parse(&s)
                    .ok_or_else(|| ControlError::Db(format!("unknown command status {s:?}")))
            })
            .transpose()
    }

    pub async fn pending_commands(&self, branch_id: &str) -> Result<u32, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM commands WHERE branch_id = ? AND status = 'pending'",
                vec![text(branch_id)],
            )
            .await?;
        let n = rows
            .first()
            .map(|r| col_int(r, 0))
            .transpose()?
            .unwrap_or(0);
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

    pub async fn command(&self, id: i64) -> Result<Option<CommandRow>, ControlError> {
        let rows = self
            .query(SELECT_BY_ID, vec![SqlValue::Integer(id)])
            .await?;
        rows.first().map(|r| command_from(r)).transpose()
    }
}

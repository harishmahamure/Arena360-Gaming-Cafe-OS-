use arena_proto::{
    AppError, COMMAND_EXPIRY_MS, COMMAND_FETCH_TIMEOUT_MS, CommandStatus, ErrorCode,
};

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// What one sweep did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    /// `pending` commands past 7 days, now `expired`.
    pub expired: u64,
    /// `fetched` commands with no answer after 10 minutes, back to `pending`.
    pub requeued: u64,
    /// Waiting commands of a branch whose Hub tokens are all revoked, now `rejected` with
    /// `TOKEN_REVOKED` (Story 8.27).
    pub revoked: u64,
}

/// One branch's waiting commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBacklog {
    pub branch_id: String,
    pub pending: u64,
    pub fetched: u64,
    pub oldest_created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommand<'a> {
    pub branch_id: &'a str,
    pub idempotency_key: &'a str,
    pub kind: &'a str,
    pub schema_version: u32,
    pub payload: &'a str,
    pub issued_by: &'a str,
    /// The owner's key shared by every row of an "all branches" post (Story 8.18).
    pub group_id: Option<&'a str>,
    /// The owner id when the post followed a step-up (Story 8.25).
    pub elevated_by: Option<&'a str>,
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
    /// The owner id when the post followed a step-up (Story 8.25).
    pub elevated_by: Option<String>,
}

const SELECT_BY_KEY: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at, elevated_by FROM commands \
     WHERE branch_id = ? AND idempotency_key = ?";

const SELECT_MAILBOX: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at, elevated_by FROM commands \
     WHERE branch_id = ? AND (status = 'pending' OR (status = 'fetched' AND id > ?)) \
     ORDER BY id LIMIT ?";

const MARK_FETCHED: &str = "UPDATE commands SET status = 'fetched', \
     fetched_at = COALESCE(fetched_at, ?) WHERE status = 'pending' AND id IN (SELECT id \
     FROM commands WHERE branch_id = ? AND (status = 'pending' OR (status = 'fetched' AND id > ?)) \
     ORDER BY id LIMIT ?)";

const SELECT_BY_ID: &str = "SELECT id, branch_id, idempotency_key, kind, schema_version, \
     payload, issued_by, status, error, applied_seq, created_at, resolved_at, elevated_by FROM commands \
     WHERE id = ?";

pub(super) fn command_from(row: &[SqlValue]) -> Result<CommandRow, ControlError> {
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
        elevated_by: col_opt_text(row, 12)?,
    })
}

pub(super) fn insert_command(c: &NewCommand<'_>) -> Stmt {
    Stmt::new(
        "INSERT INTO commands (branch_id, idempotency_key, kind, schema_version, \
         payload, issued_by, status, created_at, group_id, elevated_by) \
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?) \
         ON CONFLICT (branch_id, idempotency_key) DO NOTHING",
        vec![
            text(c.branch_id),
            text(c.idempotency_key),
            text(c.kind),
            SqlValue::Integer(i64::from(c.schema_version)),
            text(c.payload),
            text(c.issued_by),
            SqlValue::Integer(c.now),
            c.group_id.map_or(SqlValue::Null, text),
            c.elevated_by.map_or(SqlValue::Null, text),
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

    /// Records a `hub.revoke` that the control role applies at once (Story 9.8): in one transaction
    /// the command row is written already `applied` and the branch's live Hub token is revoked.
    /// The answer is the row and the id of the Hub it revoked, or `None` for that id when the post
    /// was already taken (the same key finds its first row and revokes nothing again) or the branch
    /// had no live Hub (then no row is written either).
    pub async fn apply_hub_revoke(
        &self,
        c: &NewCommand<'_>,
    ) -> Result<(Option<CommandRow>, Option<String>), ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "INSERT INTO commands (branch_id, idempotency_key, kind, schema_version, \
                     payload, issued_by, status, created_at, resolved_at, group_id, elevated_by) \
                     SELECT ?, ?, ?, ?, ?, ?, 'applied', ?, ?, NULL, ? \
                     WHERE EXISTS (SELECT 1 FROM hubs WHERE branch_id = ? AND revoked_at IS NULL) \
                     ON CONFLICT (branch_id, idempotency_key) DO NOTHING",
                    vec![
                        text(c.branch_id),
                        text(c.idempotency_key),
                        text(c.kind),
                        SqlValue::Integer(i64::from(c.schema_version)),
                        text(c.payload),
                        text(c.issued_by),
                        SqlValue::Integer(c.now),
                        SqlValue::Integer(c.now),
                        c.elevated_by.map_or(SqlValue::Null, text),
                        text(c.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "UPDATE hubs SET revoked_at = ? WHERE branch_id = ? AND revoked_at IS NULL \
                     RETURNING id",
                    vec![SqlValue::Integer(c.now), text(c.branch_id)],
                )
                .fetch(),
            ])
            .await?;
        let Some(rows) = rows else {
            let existing = self
                .query(
                    SELECT_BY_KEY,
                    vec![text(c.branch_id), text(c.idempotency_key)],
                )
                .await?
                .first()
                .map(|r| command_from(r))
                .transpose()?;
            return Ok((existing, None));
        };
        let revoked = rows
            .get(1)
            .and_then(|r| r.first())
            .map(|row| col_text(row, 0))
            .transpose()?;
        let row = self
            .query(
                SELECT_BY_KEY,
                vec![text(c.branch_id), text(c.idempotency_key)],
            )
            .await?
            .first()
            .map(|r| command_from(r))
            .transpose()?;
        Ok((row, revoked))
    }

    /// The Hub's mailbox: every `pending` row of the branch in `id` order whatever `after` is (a
    /// re-queued row can sit below the Hub's cursor), then `fetched` rows past `after` (a lost
    /// reply is delivered again). Pending rows become `fetched`.
    pub async fn fetch_commands(
        &self,
        branch_id: &str,
        after: i64,
        limit: usize,
        now: i64,
    ) -> Result<Vec<CommandRow>, ControlError> {
        let limit = SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX));
        let params = || vec![text(branch_id), SqlValue::Integer(after), limit.clone()];
        let rows = self
            .transact_rows(vec![
                Stmt::new(SELECT_MAILBOX, params()).fetch(),
                Stmt::new(
                    MARK_FETCHED,
                    [vec![SqlValue::Integer(now)], params()].concat(),
                ),
            ])
            .await?;
        rows.and_then(|r| {
            r.first()
                .map(|x| x.iter().map(|row| command_from(row)).collect())
        })
        .unwrap_or_else(|| Ok(Vec::new()))
    }

    /// Records the Hub's answer, applied at `epoch`. A command already answered keeps its answer.
    pub async fn resolve_command(
        &self,
        branch_id: &str,
        id: i64,
        applied_seq: Option<i64>,
        error: Option<&str>,
        epoch: &str,
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
                    "UPDATE commands SET status = ?, applied_seq = ?, error = ?, resolved_at = ?, \
                     applied_epoch = ? \
                     WHERE id = ? AND branch_id = ? AND status IN ('pending', 'fetched')",
                    vec![
                        text(status.as_str()),
                        applied_seq.map_or(SqlValue::Null, SqlValue::Integer),
                        error.map_or(SqlValue::Null, text),
                        SqlValue::Integer(now),
                        text(epoch),
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

    /// The command sweeper (Story 8.20). Waiting rows of a branch with no live Hub token are
    /// `rejected` with `TOKEN_REVOKED` first (Story 8.27); `pending` rows older than the expiry
    /// become `expired` with `COMMAND_EXPIRED` (so a row requeued below is not expired in the same
    /// run), then `fetched` rows with no answer for the fetch timeout return to `pending`.
    pub async fn sweep_commands(&self, now: i64) -> Result<SweepReport, ControlError> {
        let expired = serde_json::to_string(&AppError::new(
            ErrorCode::CommandExpired,
            "The branch did not pick this change up within 7 days.",
        ))
        .map_err(|e| ControlError::Db(e.to_string()))?;
        let revoked = serde_json::to_string(&AppError::new(
            ErrorCode::TokenRevoked,
            "This Hub is no longer paired.",
        ))
        .map_err(|e| ControlError::Db(e.to_string()))?;
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE commands SET status = 'rejected', error = ?, resolved_at = ? \
                     WHERE status IN ('pending', 'fetched') \
                     AND EXISTS (SELECT 1 FROM hubs h WHERE h.branch_id = commands.branch_id) \
                     AND NOT EXISTS (SELECT 1 FROM hubs h WHERE h.branch_id = commands.branch_id \
                                     AND h.revoked_at IS NULL) RETURNING id",
                    vec![text(&revoked), SqlValue::Integer(now)],
                )
                .fetch(),
                Stmt::new(
                    "UPDATE commands SET status = 'expired', error = ?, resolved_at = ? \
                     WHERE status = 'pending' AND created_at < ? RETURNING id",
                    vec![
                        text(&expired),
                        SqlValue::Integer(now),
                        SqlValue::Integer(now - COMMAND_EXPIRY_MS),
                    ],
                )
                .fetch(),
                Stmt::new(
                    "UPDATE commands SET status = 'pending', fetched_at = NULL \
                     WHERE status = 'fetched' AND fetched_at < ? RETURNING id",
                    vec![SqlValue::Integer(now - COMMAND_FETCH_TIMEOUT_MS)],
                )
                .fetch(),
            ])
            .await?;
        let count = |i: usize| {
            rows.as_ref()
                .and_then(|r| r.get(i))
                .map_or(0, |x| x.len() as u64)
        };
        Ok(SweepReport {
            revoked: count(0),
            expired: count(1),
            requeued: count(2),
        })
    }

    /// Waiting commands per branch: pending, fetched and the age of the oldest (Story 8.20).
    pub async fn command_backlog(&self) -> Result<Vec<CommandBacklog>, ControlError> {
        let rows = self
            .query(
                "SELECT branch_id, SUM(status = 'pending'), SUM(status = 'fetched'), \
                 MIN(created_at) FROM commands WHERE status IN ('pending', 'fetched') \
                 GROUP BY branch_id ORDER BY branch_id",
                Vec::new(),
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(CommandBacklog {
                    branch_id: col_text(r, 0)?,
                    pending: u64::try_from(col_int(r, 1)?).unwrap_or(0),
                    fetched: u64::try_from(col_int(r, 2)?).unwrap_or(0),
                    oldest_created_at: col_int(r, 3)?,
                })
            })
            .collect()
    }

    /// Sends a command a fenced Hub fetched back to `pending` (its result was refused).
    pub async fn requeue_fetched_command(
        &self,
        branch_id: &str,
        id: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE commands SET status = 'pending', fetched_at = NULL \
             WHERE id = ? AND branch_id = ? AND status = 'fetched'",
            vec![SqlValue::Integer(id), text(branch_id)],
        )
        .await?;
        Ok(())
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

    pub async fn settings_pending(&self, branch_id: &str) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT 1 FROM commands WHERE branch_id = ? AND kind = 'settings.set' \
                 AND status IN ('pending', 'fetched') LIMIT 1",
                vec![text(branch_id)],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    /// The row a post with `idempotency_key` made on `branch_id`, if it was taken.
    pub async fn command_by_key(
        &self,
        branch_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<CommandRow>, ControlError> {
        let rows = self
            .query(SELECT_BY_KEY, vec![text(branch_id), text(idempotency_key)])
            .await?;
        rows.first().map(|r| command_from(r)).transpose()
    }

    pub async fn command(&self, id: i64) -> Result<Option<CommandRow>, ControlError> {
        let rows = self
            .query(SELECT_BY_ID, vec![SqlValue::Integer(id)])
            .await?;
        rows.first().map(|r| command_from(r)).transpose()
    }
}

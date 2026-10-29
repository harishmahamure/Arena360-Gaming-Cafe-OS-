use arena_proto::CommandStatus;

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// One command as the owner's list shows it (Story 8.18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerCommandRow {
    pub id: i64,
    pub branch_id: String,
    pub group_id: Option<String>,
    pub idempotency_key: String,
    pub kind: String,
    pub status: CommandStatus,
    pub error: Option<String>,
    pub issued_by: String,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

/// What the owner's list is narrowed to; the tenant is always applied.
#[derive(Debug, Clone, Default)]
pub struct OwnerCommandFilter<'a> {
    pub status: Option<CommandStatus>,
    pub branch_id: Option<&'a str>,
    pub group_id: Option<&'a str>,
    /// Only rows with a smaller `id`: the page cursor, newest first.
    pub before: Option<i64>,
    pub limit: usize,
}

const COLUMNS: &str = "c.id, c.branch_id, c.group_id, c.idempotency_key, c.kind, c.status, \
     c.error, c.issued_by, c.created_at, c.resolved_at";

fn row_from(row: &[SqlValue]) -> Result<OwnerCommandRow, ControlError> {
    let status = col_text(row, 5)?;
    Ok(OwnerCommandRow {
        id: col_int(row, 0)?,
        branch_id: col_text(row, 1)?,
        group_id: col_opt_text(row, 2)?,
        idempotency_key: col_text(row, 3)?,
        kind: col_text(row, 4)?,
        status: CommandStatus::parse(&status)
            .ok_or_else(|| ControlError::Db(format!("unknown command status {status:?}")))?,
        error: col_opt_text(row, 6)?,
        issued_by: col_text(row, 7)?,
        created_at: col_int(row, 8)?,
        resolved_at: col_opt_int(row, 9)?,
    })
}

impl ControlDb {
    /// The tenant's commands, newest first.
    pub async fn owner_commands(
        &self,
        tenant_id: &str,
        f: &OwnerCommandFilter<'_>,
    ) -> Result<Vec<OwnerCommandRow>, ControlError> {
        let mut sql = format!(
            "SELECT {COLUMNS} FROM commands c JOIN branches b ON b.id = c.branch_id \
             WHERE b.tenant_id = ?"
        );
        let mut args = vec![text(tenant_id)];
        if let Some(s) = f.status {
            sql.push_str(" AND c.status = ?");
            args.push(text(s.as_str()));
        }
        if let Some(b) = f.branch_id {
            sql.push_str(" AND c.branch_id = ?");
            args.push(text(b));
        }
        if let Some(g) = f.group_id {
            sql.push_str(" AND c.group_id = ?");
            args.push(text(g));
        }
        if let Some(before) = f.before {
            sql.push_str(" AND c.id < ?");
            args.push(SqlValue::Integer(before));
        }
        sql.push_str(" ORDER BY c.id DESC LIMIT ?");
        args.push(SqlValue::Integer(
            i64::try_from(f.limit).unwrap_or(i64::MAX),
        ));
        let rows = self.query(&sql, args).await?;
        rows.iter().map(|r| row_from(r)).collect()
    }

    /// Remember the newest command schema a Hub's heartbeat said it understands.
    pub async fn record_command_schema(
        &self,
        hub_id: &str,
        schema: u32,
    ) -> Result<(), ControlError> {
        let n = SqlValue::Integer(i64::from(schema));
        self.transact(vec![Stmt::new(
            "UPDATE hubs SET command_schema_max = ? WHERE id = ? AND revoked_at IS NULL              AND command_schema_max IS NOT ?",
            vec![n.clone(), text(hub_id), n],
        )])
        .await?;
        Ok(())
    }

    /// The newest command schema the branch's active Hub reported; `None` before any heartbeat
    /// carried one.
    pub async fn branch_command_schema(
        &self,
        branch_id: &str,
    ) -> Result<Option<u32>, ControlError> {
        let rows = self
            .query(
                "SELECT command_schema_max FROM hubs WHERE branch_id = ? AND revoked_at IS NULL \
                 ORDER BY paired_at DESC, id DESC LIMIT 1",
                vec![text(branch_id)],
            )
            .await?;
        Ok(rows
            .first()
            .map(|r| col_opt_int(r, 0))
            .transpose()?
            .flatten()
            .and_then(|v| u32::try_from(v).ok()))
    }
}

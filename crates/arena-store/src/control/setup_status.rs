use super::commands::{CommandRow, command_from};
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_text, text};

pub const SETUP_KINDS: [&str; 3] = ["group.upsert", "device.assign", "device.register"];

impl ControlDb {
    pub async fn record_setup(
        &self,
        branch_id: &str,
        body: &str,
        grouped: bool,
        now: i64,
    ) -> Result<(), ControlError> {
        let mut stmts = vec![Stmt::new(
            "INSERT INTO branch_setup_status (branch_id, body, reported_at) VALUES (?, ?, ?) \
             ON CONFLICT (branch_id) DO UPDATE SET body = excluded.body, \
             reported_at = excluded.reported_at",
            vec![text(branch_id), text(body), SqlValue::Integer(now)],
        )];
        if grouped {
            stmts.push(Stmt::new(
                "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
                 VALUES (?, 'devices', 'done', ?) ON CONFLICT (branch_id, step) \
                 DO UPDATE SET state = 'done', completed_at = COALESCE(\
                 onboarding_progress.completed_at, excluded.completed_at) \
                 WHERE onboarding_progress.state <> 'done'",
                vec![text(branch_id), SqlValue::Integer(now)],
            ));
        }
        self.transact(stmts).await?;
        Ok(())
    }

    pub async fn branch_setup(
        &self,
        branch_id: &str,
    ) -> Result<Option<(String, i64)>, ControlError> {
        let rows = self
            .query(
                "SELECT body, reported_at FROM branch_setup_status WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first()
            .map(|r| Ok((col_text(r, 0)?, col_int(r, 1)?)))
            .transpose()
    }

    pub async fn setup_commands(
        &self,
        branch_id: &str,
        reported_at: i64,
        rejected_since: i64,
    ) -> Result<Vec<CommandRow>, ControlError> {
        let kinds = SETUP_KINDS.map(|k| format!("'{k}'")).join(", ");
        let sql = format!(
            "SELECT id, branch_id, idempotency_key, kind, schema_version, payload, issued_by, \
             status, error, applied_seq, created_at, resolved_at FROM commands \
             WHERE branch_id = ? AND kind IN ({kinds}) AND (status IN ('pending', 'fetched') \
             OR (status = 'applied' AND resolved_at > ?) \
             OR (status = 'rejected' AND resolved_at > ?)) ORDER BY id"
        );
        let rows = self
            .query(
                &sql,
                vec![
                    text(branch_id),
                    SqlValue::Integer(reported_at),
                    SqlValue::Integer(rejected_since),
                ],
            )
            .await?;
        rows.iter().map(|r| command_from(r)).collect()
    }
}

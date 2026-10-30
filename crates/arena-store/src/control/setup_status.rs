use arena_proto::{BranchSetup, PrinterState, TestSessionState};

use super::commands::{CommandRow, command_from};
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_text, text};

pub const SETUP_KINDS: [&str; 4] = [
    "group.upsert",
    "device.assign",
    "device.register",
    "setup.test_session",
];

fn finish(step: &str, branch_id: &str, now: i64) -> Stmt {
    Stmt::new(
        "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
         VALUES (?, ?, 'done', ?) ON CONFLICT (branch_id, step) \
         DO UPDATE SET state = 'done', completed_at = COALESCE(\
         onboarding_progress.completed_at, excluded.completed_at) \
         WHERE onboarding_progress.state = 'pending'",
        vec![text(branch_id), text(step), SqlValue::Integer(now)],
    )
}

impl ControlDb {
    pub async fn record_setup(
        &self,
        branch_id: &str,
        body: &str,
        setup: &BranchSetup,
        now: i64,
    ) -> Result<(), ControlError> {
        let mut stmts = vec![Stmt::new(
            "INSERT INTO branch_setup_status (branch_id, body, reported_at) VALUES (?, ?, ?) \
             ON CONFLICT (branch_id) DO UPDATE SET body = excluded.body, \
             reported_at = excluded.reported_at",
            vec![text(branch_id), text(body), SqlValue::Integer(now)],
        )];
        if setup.grouped() {
            stmts.push(finish("devices", branch_id, now));
        }
        if setup.rated() {
            stmts.push(finish("rates", branch_id, now));
        }
        if setup
            .status
            .is_some_and(|s| matches!(s.printer, PrinterState::Ok | PrinterState::Skipped))
        {
            stmts.push(finish("printer_payments", branch_id, now));
        }
        if setup
            .status
            .is_some_and(|s| s.test_session == TestSessionState::Passed)
        {
            stmts.push(finish("test_session", branch_id, now));
        }
        self.transact(stmts).await?;
        Ok(())
    }

    pub async fn go_live(&self, branch_id: &str, now: i64) -> Result<(), ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE branches SET live_at = COALESCE(live_at, ?), updated_at = ? WHERE id = ?",
                vec![
                    SqlValue::Integer(now),
                    SqlValue::Integer(now),
                    text(branch_id),
                ],
            ),
            finish("go_live", branch_id, now),
        ])
        .await?;
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
             status, error, applied_seq, created_at, resolved_at, elevated_by FROM commands \
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

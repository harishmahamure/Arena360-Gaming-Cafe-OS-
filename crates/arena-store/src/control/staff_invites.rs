use arena_proto::CommandStatus;

use super::commands::{NewCommand, insert_command};
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_text, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStaffInvite<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub name: &'a str,
    pub phone: &'a str,
    pub role: &'a str,
    pub setup_expires_at: i64,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffInviteRow {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub role: String,
    pub setup_expires_at: i64,
    pub status: Option<CommandStatus>,
    pub error: Option<String>,
}

const SELECT: &str = "SELECT i.id, i.name, i.phone, i.role, i.setup_expires_at, c.status, c.error \
     FROM staff_invites i LEFT JOIN commands c \
     ON c.branch_id = i.branch_id AND c.idempotency_key = i.command_key \
     WHERE i.branch_id = ?";

fn invite_from(row: &[SqlValue]) -> Result<StaffInviteRow, ControlError> {
    Ok(StaffInviteRow {
        id: col_text(row, 0)?,
        name: col_text(row, 1)?,
        phone: col_text(row, 2)?,
        role: col_text(row, 3)?,
        setup_expires_at: col_int(row, 4)?,
        status: col_opt_text(row, 5)?
            .map(|s| {
                CommandStatus::parse(&s)
                    .ok_or_else(|| ControlError::Db(format!("unknown command status {s:?}")))
            })
            .transpose()?,
        error: col_opt_text(row, 6)?,
    })
}

impl ControlDb {
    pub async fn create_staff_invite(
        &self,
        i: &NewStaffInvite<'_>,
        c: &NewCommand<'_>,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "INSERT INTO staff_invites (id, branch_id, name, phone, role, command_key, \
                 setup_expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    text(i.id),
                    text(i.branch_id),
                    text(i.name),
                    text(i.phone),
                    text(i.role),
                    text(c.idempotency_key),
                    SqlValue::Integer(i.setup_expires_at),
                    SqlValue::Integer(i.now),
                ],
            )
            .expect(1),
            insert_command(c).expect(1),
        ])
        .await
    }

    pub async fn reissue_staff_invite(
        &self,
        branch_id: &str,
        id: &str,
        setup_expires_at: i64,
        c: &NewCommand<'_>,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE staff_invites SET command_key = ?, setup_expires_at = ? \
                 WHERE id = ? AND branch_id = ?",
                vec![
                    text(c.idempotency_key),
                    SqlValue::Integer(setup_expires_at),
                    text(id),
                    text(branch_id),
                ],
            )
            .expect(1),
            insert_command(c).expect(1),
        ])
        .await
    }

    pub async fn staff_invites(
        &self,
        branch_id: &str,
    ) -> Result<Vec<StaffInviteRow>, ControlError> {
        let sql = format!("{SELECT} ORDER BY i.created_at, i.id");
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.iter().map(|r| invite_from(r)).collect()
    }

    pub async fn staff_invite(
        &self,
        branch_id: &str,
        id: &str,
    ) -> Result<Option<StaffInviteRow>, ControlError> {
        let sql = format!("{SELECT} AND i.id = ?");
        let rows = self.query(&sql, vec![text(branch_id), text(id)]).await?;
        rows.first().map(|r| invite_from(r)).transpose()
    }

    pub async fn skip_step(&self, branch_id: &str, step: &str) -> Result<(), ControlError> {
        self.execute(
            "UPDATE onboarding_progress SET state = 'skipped' \
             WHERE branch_id = ? AND step = ? AND state = 'pending'",
            vec![text(branch_id), text(step)],
        )
        .await?;
        Ok(())
    }

    pub async fn finish_staff_step(
        &self,
        branch_id: &str,
        command_id: i64,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE onboarding_progress SET state = 'done', completed_at = COALESCE(completed_at, ?) \
             WHERE branch_id = ? AND step = 'staff' AND state <> 'done' AND EXISTS ( \
             SELECT 1 FROM commands c JOIN staff_invites i \
             ON i.branch_id = c.branch_id AND i.command_key = c.idempotency_key \
             WHERE c.id = ? AND c.branch_id = ? AND c.status = 'applied' AND i.role = 'manager')",
            vec![
                SqlValue::Integer(now),
                text(branch_id),
                SqlValue::Integer(command_id),
                text(branch_id),
            ],
        )
        .await?;
        Ok(())
    }
}

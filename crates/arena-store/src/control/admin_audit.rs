//! `admin_audit` (Story 6.1): the append-only record of what operators and the system did in
//! `gz_control`. Triggers refuse every UPDATE and DELETE; later epics write to the same table.

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_text, col_text, text};

/// One row to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAdminAudit<'a> {
    pub id: &'a str,
    pub at: i64,
    /// `admin` or `system`.
    pub actor_kind: &'a str,
    pub actor_id: &'a str,
    pub action: &'a str,
    pub target_kind: &'a str,
    pub target_id: &'a str,
    pub reason: Option<&'a str>,
    /// JSON with no secrets or player data.
    pub summary: &'a str,
    pub request_id: &'a str,
}

/// A stored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminAuditRow {
    pub id: String,
    pub at: i64,
    pub actor_kind: String,
    pub actor_id: String,
    pub action: String,
    pub target_kind: String,
    pub target_id: String,
    pub reason: Option<String>,
    pub summary: String,
    pub request_id: String,
}

const INSERT: &str = "INSERT INTO admin_audit (id, at, actor_kind, actor_id, action, target_kind, \
                      target_id, reason, summary, request_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

/// The insert as a statement, to run in the same transaction as the action it records.
pub(super) fn insert_stmt(a: &NewAdminAudit<'_>) -> Stmt {
    Stmt::new(
        INSERT,
        vec![
            text(a.id),
            SqlValue::Integer(a.at),
            text(a.actor_kind),
            text(a.actor_id),
            text(a.action),
            text(a.target_kind),
            text(a.target_id),
            a.reason.map_or(SqlValue::Null, text),
            text(a.summary),
            text(a.request_id),
        ],
    )
    .expect(1)
}

impl ControlDb {
    /// Appends one audit row.
    pub async fn append_admin_audit(&self, a: &NewAdminAudit<'_>) -> Result<(), ControlError> {
        self.transact(vec![insert_stmt(a)]).await?;
        Ok(())
    }

    /// Every audit row, oldest first.
    pub async fn admin_audit_rows(&self) -> Result<Vec<AdminAuditRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, at, actor_kind, actor_id, action, target_kind, target_id, reason, \
                 summary, request_id FROM admin_audit ORDER BY at, id",
                vec![],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(AdminAuditRow {
                    id: col_text(r, 0)?,
                    at: col_int(r, 1)?,
                    actor_kind: col_text(r, 2)?,
                    actor_id: col_text(r, 3)?,
                    action: col_text(r, 4)?,
                    target_kind: col_text(r, 5)?,
                    target_id: col_text(r, 6)?,
                    reason: col_opt_text(r, 7)?,
                    summary: col_text(r, 8)?,
                    request_id: col_text(r, 9)?,
                })
            })
            .collect()
    }
}

use super::{ControlDb, ControlError, SqlValue, col_opt_text, text};

/// A branch's "start from another branch's setup" choice (Story 5.38).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateChoice {
    /// `None`: the owner turned the option off.
    pub source_branch_id: Option<String>,
    /// `pending`, `retry`, `applied` or `failed`; `None` until a link went to the Hub.
    pub status: Option<String>,
    pub error: Option<String>,
}

fn opt(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, text)
}

impl ControlDb {
    pub async fn template_choice(
        &self,
        branch_id: &str,
    ) -> Result<Option<TemplateChoice>, ControlError> {
        let rows = self
            .query(
                "SELECT source_branch_id, status, error FROM branch_template_choice \
                 WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first()
            .map(|r| {
                Ok(TemplateChoice {
                    source_branch_id: col_opt_text(r, 0)?,
                    status: col_opt_text(r, 1)?,
                    error: col_opt_text(r, 2)?,
                })
            })
            .transpose()
    }

    /// Records the owner's choice: the source (or none) and the status it starts in.
    pub async fn set_template_choice(
        &self,
        branch_id: &str,
        source_branch_id: Option<&str>,
        status: Option<&str>,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "INSERT INTO branch_template_choice \
             (branch_id, source_branch_id, status, error, updated_at) VALUES (?, ?, ?, NULL, ?) \
             ON CONFLICT (branch_id) DO UPDATE SET source_branch_id = excluded.source_branch_id, \
             status = excluded.status, error = NULL, updated_at = excluded.updated_at",
            vec![
                text(branch_id),
                opt(source_branch_id),
                opt(status),
                SqlValue::Integer(now),
            ],
        )
        .await?;
        Ok(())
    }

    /// Records what the Hub reported for a branch that has a source; answers whether a row
    /// changed.
    pub async fn set_template_status(
        &self,
        branch_id: &str,
        status: &str,
        error: Option<&str>,
        now: i64,
    ) -> Result<bool, ControlError> {
        Ok(self
            .execute(
                "UPDATE branch_template_choice SET status = ?, error = ?, updated_at = ?                  WHERE branch_id = ? AND source_branch_id IS NOT NULL",
                vec![
                    text(status),
                    opt(error),
                    SqlValue::Integer(now),
                    text(branch_id),
                ],
            )
            .await?
            == 1)
    }

    /// Moves the status from `from` to `to` for a branch that has a source; answers whether
    /// it moved (exactly one caller wins a `retry` link).
    pub async fn move_template_status(
        &self,
        branch_id: &str,
        from: &str,
        to: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        Ok(self
            .execute(
                "UPDATE branch_template_choice SET status = ?, error = NULL, updated_at = ?                  WHERE branch_id = ? AND source_branch_id IS NOT NULL AND status = ?",
                vec![
                    text(to),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(from),
                ],
            )
            .await?
            == 1)
    }
}

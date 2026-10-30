//! The escrow of a Hub's branch key (Story 9.2, AD-24): `kek_escrow` in `gz_control`.
//!
//! The wrapped key is written here (Story 9.2) and read by exactly one function: the key release
//! of Story 9.10, [`ControlDb::latest_kek_escrow`], whose caller (the control role's
//! `backup.restore` handler) re-wraps it for a replacement Hub and writes an `admin_audit` row in
//! the same transaction as the decision. No endpoint returns the wrapped bytes; the only other
//! function that touches the table is a presence count that never selects the ciphertext.

use super::admin_audit::insert_stmt;
use super::{ControlDb, ControlError, NewAdminAudit, SqlValue, Stmt, col_int, col_text, text};

impl ControlDb {
    /// Stores `wrapped_kek` for `branch_id` and `hub_instance_id` (replacing that Hub's earlier
    /// one) together with `audit`, in one transaction.
    pub async fn put_kek_escrow(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
        wrapped_kek: &str,
        now: i64,
        audit: &NewAdminAudit<'_>,
    ) -> Result<(), ControlError> {
        let put = Stmt::new(
            "INSERT INTO kek_escrow (branch_id, hub_instance_id, wrapped_kek, created_at) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (branch_id, hub_instance_id) DO UPDATE SET \
                wrapped_kek = excluded.wrapped_kek, created_at = excluded.created_at",
            vec![
                text(branch_id),
                text(hub_instance_id),
                text(wrapped_kek),
                SqlValue::Integer(now),
            ],
        )
        .expect(1);
        if !self.transact(vec![put, insert_stmt(audit)]).await? {
            return Err(ControlError::Db("the key escrow was refused".to_owned()));
        }
        Ok(())
    }

    /// How many Hub instances of `branch_id` have a key in escrow. Never selects the ciphertext.
    pub async fn kek_escrow_count(&self, branch_id: &str) -> Result<i64, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM kek_escrow WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first()
            .map(|r| col_int(r, 0))
            .transpose()
            .map(|n| n.unwrap_or(0))
    }

    /// The wrapped branch key (an age file, base64) the newest Hub instance of `branch_id` left in
    /// escrow, if any. The only reader of the ciphertext: it serves the key release (Story 9.10),
    /// whose decision writes the audit row.
    pub async fn latest_kek_escrow(&self, branch_id: &str) -> Result<Option<String>, ControlError> {
        let rows = self
            .query(
                "SELECT wrapped_kek FROM kek_escrow WHERE branch_id = ? \
                 ORDER BY created_at DESC, hub_instance_id LIMIT 1",
                vec![text(branch_id)],
            )
            .await?;
        rows.first().map(|r| col_text(r, 0)).transpose()
    }
}

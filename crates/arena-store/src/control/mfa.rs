//! `owner_mfa` and `owner_backup_codes`: an owner's authenticator app (TOTP) and backup codes.
//!
//! The TOTP secret is stored only as AES-256-GCM ciphertext and its nonce, and each backup
//! code only as an argon2id PHC string; the caller encrypts and hashes. `last_step` is the
//! newest accepted 30 s step: every write that accepts a code requires a later step, in the
//! same statement, so a code is never accepted twice.

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_blob, col_int, col_opt_int, col_text, text,
};

/// An owner's `owner_mfa` row.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnerMfa {
    pub owner_id: String,
    pub secret_ct: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    pub created_at: i64,
    pub confirmed_at: Option<i64>,
    pub last_step: Option<i64>,
    pub backup_ack_at: Option<i64>,
}

impl std::fmt::Debug for OwnerMfa {
    /// Never the ciphertext or nonce.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerMfa")
            .field("owner_id", &self.owner_id)
            .field("created_at", &self.created_at)
            .field("confirmed_at", &self.confirmed_at)
            .field("last_step", &self.last_step)
            .field("backup_ack_at", &self.backup_ack_at)
            .finish_non_exhaustive()
    }
}

/// A backup code to store: its id and argon2id hash.
#[derive(Clone, PartialEq, Eq)]
pub struct NewBackupCode {
    pub id: String,
    pub code_hash: String,
}

impl std::fmt::Debug for NewBackupCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewBackupCode")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// A stored backup code.
#[derive(Clone, PartialEq, Eq)]
pub struct BackupCodeRow {
    pub id: String,
    pub owner_id: String,
    pub code_hash: String,
    pub created_at: i64,
    pub used_at: Option<i64>,
}

impl std::fmt::Debug for BackupCodeRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupCodeRow")
            .field("id", &self.id)
            .field("owner_id", &self.owner_id)
            .field("created_at", &self.created_at)
            .field("used_at", &self.used_at)
            .finish_non_exhaustive()
    }
}

const MFA_COLUMNS: &str =
    "owner_id, secret_ct, secret_nonce, created_at, confirmed_at, last_step, backup_ack_at";

fn mfa_from(row: &[SqlValue]) -> Result<OwnerMfa, ControlError> {
    Ok(OwnerMfa {
        owner_id: col_text(row, 0)?,
        secret_ct: col_blob(row, 1)?,
        secret_nonce: col_blob(row, 2)?,
        created_at: col_int(row, 3)?,
        confirmed_at: col_opt_int(row, 4)?,
        last_step: col_opt_int(row, 5)?,
        backup_ack_at: col_opt_int(row, 6)?,
    })
}

/// Deletes every backup code of `owner_id`, then inserts `codes`.
fn replace_codes(owner_id: &str, codes: &[NewBackupCode], now: i64) -> Vec<Stmt> {
    let mut stmts = vec![Stmt::new(
        "DELETE FROM owner_backup_codes WHERE owner_id = ?",
        vec![text(owner_id)],
    )];
    for c in codes {
        stmts.push(
            Stmt::new(
                "INSERT INTO owner_backup_codes (id, owner_id, code_hash, created_at, used_at) \
                 VALUES (?, ?, ?, ?, NULL)",
                vec![
                    text(&c.id),
                    text(owner_id),
                    text(&c.code_hash),
                    SqlValue::Integer(now),
                ],
            )
            .expect(1),
        );
    }
    stmts
}

impl ControlDb {
    /// The owner's `owner_mfa` row.
    pub async fn owner_mfa(&self, owner_id: &str) -> Result<Option<OwnerMfa>, ControlError> {
        let sql = format!("SELECT {MFA_COLUMNS} FROM owner_mfa WHERE owner_id = ?");
        let rows = self.query(&sql, vec![text(owner_id)]).await?;
        rows.first().map(|r| mfa_from(r)).transpose()
    }

    /// Stores a new, unconfirmed TOTP secret for the owner, replacing an unconfirmed one (and
    /// clearing its `last_step`). `false`, with nothing changed, unless the owner exists, is
    /// `pending`, has a password and has no confirmed secret.
    pub async fn start_owner_mfa(
        &self,
        owner_id: &str,
        secret_ct: &[u8],
        secret_nonce: &[u8],
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "INSERT INTO owner_mfa (owner_id, secret_ct, secret_nonce, created_at, \
                 confirmed_at, last_step, backup_ack_at) \
                 SELECT id, ?, ?, ?, NULL, NULL, NULL FROM owners \
                 WHERE id = ? AND mfa_state = 'pending' AND password_hash IS NOT NULL \
                 ON CONFLICT (owner_id) DO UPDATE SET secret_ct = excluded.secret_ct, \
                 secret_nonce = excluded.secret_nonce, created_at = excluded.created_at, \
                 last_step = NULL WHERE owner_mfa.confirmed_at IS NULL",
                vec![
                    SqlValue::Blob(secret_ct.to_vec()),
                    SqlValue::Blob(secret_nonce.to_vec()),
                    SqlValue::Integer(now),
                    text(owner_id),
                ],
            )
            .expect(1),
        ])
        .await
    }

    /// Confirms the owner's unconfirmed secret with a code for `step`, in one transaction:
    /// `confirmed_at = now`, `last_step = step`, the owner becomes `enrolled`, and the backup
    /// codes are replaced by `codes`. `secret_nonce` names the secret the code was checked
    /// against: when another start replaced it meanwhile, or the step is not after
    /// `last_step`, nothing changes and the answer is `false`.
    pub async fn confirm_owner_mfa(
        &self,
        owner_id: &str,
        secret_nonce: &[u8],
        step: i64,
        codes: &[NewBackupCode],
        now: i64,
    ) -> Result<bool, ControlError> {
        let mut stmts = vec![
            Stmt::new(
                "UPDATE owner_mfa SET confirmed_at = ?, last_step = ? \
                 WHERE owner_id = ? AND confirmed_at IS NULL AND secret_nonce = ? \
                 AND (last_step IS NULL OR last_step < ?)",
                vec![
                    SqlValue::Integer(now),
                    SqlValue::Integer(step),
                    text(owner_id),
                    SqlValue::Blob(secret_nonce.to_vec()),
                    SqlValue::Integer(step),
                ],
            )
            .expect(1),
            Stmt::new(
                "UPDATE owners SET mfa_state = 'enrolled' \
                 WHERE id = ? AND mfa_state = 'pending' AND password_hash IS NOT NULL",
                vec![text(owner_id)],
            )
            .expect(1),
        ];
        stmts.extend(replace_codes(owner_id, codes, now));
        self.transact(stmts).await
    }

    /// Replaces the backup codes of an enrolled owner with `codes`, accepting a TOTP code for
    /// `step` (which becomes `last_step`) and clearing `backup_ack_at`, in one transaction.
    /// `false`, with nothing changed,
    /// when the owner has no confirmed secret or `step` is not after `last_step` (a replay).
    pub async fn regenerate_backup_codes(
        &self,
        owner_id: &str,
        step: i64,
        codes: &[NewBackupCode],
        now: i64,
    ) -> Result<bool, ControlError> {
        let mut stmts = vec![
            Stmt::new(
                "UPDATE owner_mfa SET last_step = ?, backup_ack_at = NULL WHERE owner_id = ? \
                 AND confirmed_at IS NOT NULL AND (last_step IS NULL OR last_step < ?)",
                vec![
                    SqlValue::Integer(step),
                    text(owner_id),
                    SqlValue::Integer(step),
                ],
            )
            .expect(1),
        ];
        stmts.extend(replace_codes(owner_id, codes, now));
        self.transact(stmts).await
    }

    /// Records that the owner saved their backup codes. `false` when the owner has no
    /// confirmed secret or no unused backup code.
    pub async fn ack_backup_codes(&self, owner_id: &str, now: i64) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE owner_mfa SET backup_ack_at = ? \
                 WHERE owner_id = ? AND confirmed_at IS NOT NULL AND EXISTS \
                 (SELECT 1 FROM owner_backup_codes WHERE owner_id = ? AND used_at IS NULL)",
                vec![SqlValue::Integer(now), text(owner_id), text(owner_id)],
            )
            .await?;
        Ok(n == 1)
    }

    /// The owner's backup codes, oldest first.
    pub async fn backup_codes(&self, owner_id: &str) -> Result<Vec<BackupCodeRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, owner_id, code_hash, created_at, used_at FROM owner_backup_codes \
                 WHERE owner_id = ? ORDER BY created_at, id",
                vec![text(owner_id)],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(BackupCodeRow {
                    id: col_text(r, 0)?,
                    owner_id: col_text(r, 1)?,
                    code_hash: col_text(r, 2)?,
                    created_at: col_int(r, 3)?,
                    used_at: col_opt_int(r, 4)?,
                })
            })
            .collect()
    }
}

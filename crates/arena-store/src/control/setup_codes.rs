use arena_proto::SETUP_CODE_PLAINTEXT_TTL_MS;

use super::{ControlDb, ControlError, SqlValue, Stmt, col_blob, col_int, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSetupCode<'a> {
    pub code_hash: &'a str,
    pub hub_expires_at: i64,
    pub ciphertext: &'a [u8],
    pub nonce: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupCodeRow {
    pub code_hash: String,
    pub hub_expires_at: i64,
    pub created_at: i64,
    pub has_plaintext: bool,
    pub shown_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedSetupCode {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

pub(super) const INSERT_SETUP_CODE: &str = "INSERT INTO owner_setup_codes \
     (branch_id, code_hash, hub_expires_at, ciphertext, nonce, created_at, shown_at) \
     VALUES (?, ?, ?, ?, ?, ?, NULL) ON CONFLICT (branch_id) DO UPDATE SET \
     hub_expires_at = excluded.hub_expires_at";

pub(super) fn insert_params(branch_id: &str, c: &NewSetupCode<'_>, now: i64) -> Vec<SqlValue> {
    vec![
        text(branch_id),
        text(c.code_hash),
        SqlValue::Integer(c.hub_expires_at),
        SqlValue::Blob(c.ciphertext.to_vec()),
        SqlValue::Blob(c.nonce.to_vec()),
        SqlValue::Integer(now),
    ]
}

impl ControlDb {
    pub async fn setup_code(&self, branch_id: &str) -> Result<Option<SetupCodeRow>, ControlError> {
        let rows = self
            .query(
                "SELECT code_hash, hub_expires_at, created_at, ciphertext IS NOT NULL, shown_at \
                 FROM owner_setup_codes WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first()
            .map(|r| {
                Ok(SetupCodeRow {
                    code_hash: col_text(r, 0)?,
                    hub_expires_at: col_int(r, 1)?,
                    created_at: col_int(r, 2)?,
                    has_plaintext: col_int(r, 3)? != 0,
                    shown_at: super::col_opt_int(r, 4)?,
                })
            })
            .transpose()
    }

    pub async fn peek_setup_code(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<SealedSetupCode>, ControlError> {
        let rows = self
            .query(
                "SELECT ciphertext, nonce FROM owner_setup_codes \
                 WHERE branch_id = ? AND ciphertext IS NOT NULL AND created_at > ?",
                vec![
                    text(branch_id),
                    SqlValue::Integer(now - SETUP_CODE_PLAINTEXT_TTL_MS),
                ],
            )
            .await?;
        let Some(r) = rows.first() else {
            self.wipe_setup_code(branch_id, now).await?;
            return Ok(None);
        };
        Ok(Some(SealedSetupCode {
            ciphertext: col_blob(r, 0)?,
            nonce: col_blob(r, 1)?,
        }))
    }

    pub async fn consume_setup_code(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE owner_setup_codes SET ciphertext = NULL, nonce = NULL, shown_at = ? \
                 WHERE branch_id = ? AND ciphertext IS NOT NULL AND created_at > ?",
                vec![
                    SqlValue::Integer(now),
                    text(branch_id),
                    SqlValue::Integer(now - SETUP_CODE_PLAINTEXT_TTL_MS),
                ],
            )
            .expect(1),
        ])
        .await
    }

    async fn wipe_setup_code(&self, branch_id: &str, now: i64) -> Result<(), ControlError> {
        self.transact(vec![Stmt::new(
            "UPDATE owner_setup_codes SET ciphertext = NULL, nonce = NULL \
             WHERE branch_id = ? AND ciphertext IS NOT NULL AND created_at <= ?",
            vec![
                text(branch_id),
                SqlValue::Integer(now - SETUP_CODE_PLAINTEXT_TTL_MS),
            ],
        )])
        .await?;
        Ok(())
    }
}

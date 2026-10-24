use super::owner::{OWNER_COLUMNS, Owner, owner_from};
use super::session::{NewOwnerSession, PRUNE_SESSIONS, insert_stmt};
use super::{
    ControlDb, ControlError, OTP_RETENTION_MS, SqlValue, Stmt, col_int, col_opt_int, col_opt_text,
    col_text, text,
};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSigninChallenge {
    pub id: String,
    pub owner_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SigninChallenge {
    pub id: String,
    pub owner_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub attempts: u32,
    pub consumed_at: Option<i64>,
    pub sms_code_hash: Option<String>,
    pub sms_sent_at: Option<i64>,
}

impl std::fmt::Debug for SigninChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigninChallenge")
            .field("id", &self.id)
            .field("owner_id", &self.owner_id)
            .field("expires_at", &self.expires_at)
            .field("attempts", &self.attempts)
            .field("consumed_at", &self.consumed_at)
            .field("sms_sent_at", &self.sms_sent_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct NewOwnerDevice {
    pub id: String,
    pub token_hash: String,
    pub label: String,
    pub expires_at: i64,
    pub refresh: bool,
}

impl std::fmt::Debug for NewOwnerDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewOwnerDevice")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("expires_at", &self.expires_at)
            .field("refresh", &self.refresh)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerDevice {
    pub id: String,
    pub owner_id: String,
    pub label: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SigninFactor {
    Totp { step: i64 },
    BackupCode { id: String },
    Sms { code_hash: String, sent_after: i64 },
}

impl std::fmt::Debug for SigninFactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Totp { step } => f.debug_struct("Totp").field("step", step).finish(),
            Self::BackupCode { id } => f.debug_struct("BackupCode").field("id", id).finish(),
            Self::Sms { .. } => f.debug_struct("Sms").finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SigninCompletion<'a> {
    pub challenge_id: &'a str,
    pub owner_id: &'a str,
    pub max_attempts: u32,
    pub now: i64,
    pub factor: SigninFactor,
    pub device: Option<NewOwnerDevice>,
    pub session: &'a NewOwnerSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SigninFailure<'a> {
    pub owner_id: &'a str,
    pub challenge_id: Option<&'a str>,
    pub max_attempts: u32,
    pub max_failures: u32,
    pub lock_ms: i64,
    pub now: i64,
}

const CHALLENGE_COLUMNS: &str =
    "id, owner_id, created_at, expires_at, attempts, consumed_at, sms_code_hash, sms_sent_at";

fn challenge_from(row: &[SqlValue]) -> Result<SigninChallenge, ControlError> {
    Ok(SigninChallenge {
        id: col_text(row, 0)?,
        owner_id: col_text(row, 1)?,
        created_at: col_int(row, 2)?,
        expires_at: col_int(row, 3)?,
        attempts: u32::try_from(col_int(row, 4)?).unwrap_or(u32::MAX),
        consumed_at: col_opt_int(row, 5)?,
        sms_code_hash: col_opt_text(row, 6)?,
        sms_sent_at: col_opt_int(row, 7)?,
    })
}

const DEVICE_COLUMNS: &str =
    "id, owner_id, label, created_at, last_seen_at, expires_at, revoked_at";

fn device_from(row: &[SqlValue]) -> Result<OwnerDevice, ControlError> {
    Ok(OwnerDevice {
        id: col_text(row, 0)?,
        owner_id: col_text(row, 1)?,
        label: col_text(row, 2)?,
        created_at: col_int(row, 3)?,
        last_seen_at: col_int(row, 4)?,
        expires_at: col_int(row, 5)?,
        revoked_at: col_opt_int(row, 6)?,
    })
}

fn opt_int(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Integer)
}

fn reset_failures(owner_id: &str, now: i64, enrolled: bool) -> Stmt {
    let sql = if enrolled {
        "UPDATE owners SET failed_signins = 0, locked_until = NULL WHERE id = ? \
         AND mfa_state = 'enrolled' AND (locked_until IS NULL OR locked_until <= ?)"
    } else {
        "UPDATE owners SET failed_signins = 0, locked_until = NULL WHERE id = ? \
         AND (locked_until IS NULL OR locked_until <= ?)"
    };
    Stmt::new(sql, vec![text(owner_id), SqlValue::Integer(now)]).expect(1)
}

impl ControlDb {
    pub async fn owner_by_email(&self, email: &str) -> Result<Option<Owner>, ControlError> {
        let sql = format!(
            "SELECT {OWNER_COLUMNS} FROM owners WHERE lower(email) = lower(?) \
             AND password_hash IS NOT NULL LIMIT 2"
        );
        let rows = self.query(&sql, vec![text(email)]).await?;
        match rows.as_slice() {
            [only] => owner_from(only, 0).map(Some),
            _ => Ok(None),
        }
    }

    pub async fn record_signin_failure(
        &self,
        f: &SigninFailure<'_>,
    ) -> Result<Option<i64>, ControlError> {
        let mut stmts = Vec::new();
        if let Some(id) = f.challenge_id {
            stmts.push(Stmt::new(
                "UPDATE signin_challenges SET attempts = attempts + 1 \
                 WHERE id = ? AND owner_id = ? AND consumed_at IS NULL AND attempts < ?",
                vec![
                    text(id),
                    text(f.owner_id),
                    SqlValue::Integer(i64::from(f.max_attempts)),
                ],
            ));
        }
        let max = SqlValue::Integer(i64::from(f.max_failures));
        stmts.push(
            Stmt::new(
                "UPDATE owners SET \
                 failed_signins = CASE WHEN failed_signins + 1 >= ? THEN 0 \
                 ELSE failed_signins + 1 END, \
                 locked_until = CASE WHEN failed_signins + 1 >= ? THEN ? ELSE locked_until END \
                 WHERE id = ? AND (locked_until IS NULL OR locked_until <= ?) \
                 RETURNING locked_until",
                vec![
                    max.clone(),
                    max,
                    SqlValue::Integer(f.now + f.lock_ms),
                    text(f.owner_id),
                    SqlValue::Integer(f.now),
                ],
            )
            .fetch(),
        );
        let rows = self.transact_rows(stmts).await?.unwrap_or_default();
        let locked = rows
            .last()
            .and_then(|r| r.first())
            .map(|r| col_opt_int(r, 0))
            .transpose()?
            .flatten()
            .filter(|&at| at > f.now);
        Ok(locked)
    }

    pub async fn signin_with_password(
        &self,
        owner_id: &str,
        session: &NewOwnerSession,
        trusted_device: Option<&str>,
        now: i64,
    ) -> Result<bool, ControlError> {
        let mut s = session.clone();
        s.owner_id = owner_id.to_owned();
        s.device_id = trusted_device.map(str::to_owned);
        let mut stmts = vec![reset_failures(owner_id, now, trusted_device.is_some())];
        if let Some(device) = trusted_device {
            stmts.push(
                Stmt::new(
                    "UPDATE owner_devices SET last_seen_at = ? WHERE id = ? AND owner_id = ? \
                     AND revoked_at IS NULL AND expires_at > ?",
                    vec![
                        SqlValue::Integer(now),
                        text(device),
                        text(owner_id),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
            );
        }
        stmts.push(Stmt::new(PRUNE_SESSIONS, vec![SqlValue::Integer(now)]));
        stmts.push(insert_stmt(&s).expect(1));
        self.transact(stmts).await
    }

    pub async fn insert_signin_challenge(
        &self,
        c: &NewSigninChallenge,
    ) -> Result<(), ControlError> {
        self.transact(vec![
            Stmt::new(
                "DELETE FROM signin_challenges WHERE expires_at < ?",
                vec![SqlValue::Integer(c.created_at - OTP_RETENTION_MS)],
            ),
            Stmt::new(
                "UPDATE signin_challenges SET consumed_at = ? WHERE owner_id = ? \
                 AND consumed_at IS NULL",
                vec![SqlValue::Integer(c.created_at), text(&c.owner_id)],
            ),
            Stmt::new(
                "INSERT INTO signin_challenges (id, owner_id, created_at, expires_at, attempts, \
                 consumed_at, sms_code_hash, sms_sent_at) VALUES (?, ?, ?, ?, 0, NULL, NULL, NULL)",
                vec![
                    text(&c.id),
                    text(&c.owner_id),
                    SqlValue::Integer(c.created_at),
                    SqlValue::Integer(c.expires_at),
                ],
            )
            .expect(1),
        ])
        .await?;
        Ok(())
    }

    pub async fn signin_challenge(
        &self,
        id: &str,
    ) -> Result<Option<SigninChallenge>, ControlError> {
        let sql = format!("SELECT {CHALLENGE_COLUMNS} FROM signin_challenges WHERE id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| challenge_from(r)).transpose()
    }

    pub async fn set_signin_sms(
        &self,
        id: &str,
        seen_sent_at: Option<i64>,
        code_hash: &str,
        now: i64,
        expires_at: i64,
        max_attempts: u32,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE signin_challenges SET sms_code_hash = ?, sms_sent_at = ?, \
                 expires_at = MAX(expires_at, ?) WHERE id = ? AND consumed_at IS NULL \
                 AND attempts < ? AND expires_at > ? AND sms_sent_at IS ?",
                vec![
                    text(code_hash),
                    SqlValue::Integer(now),
                    SqlValue::Integer(expires_at),
                    text(id),
                    SqlValue::Integer(i64::from(max_attempts)),
                    SqlValue::Integer(now),
                    opt_int(seen_sent_at),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn revert_signin_sms(
        &self,
        id: &str,
        new_hash: &str,
        previous: &SigninChallenge,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE signin_challenges SET sms_code_hash = ?, sms_sent_at = ?, expires_at = ? \
             WHERE id = ? AND sms_code_hash = ?",
            vec![
                previous
                    .sms_code_hash
                    .as_deref()
                    .map_or(SqlValue::Null, text),
                opt_int(previous.sms_sent_at),
                SqlValue::Integer(previous.expires_at),
                text(id),
                text(new_hash),
            ],
        )
        .await?;
        Ok(())
    }

    pub async fn trusted_device(
        &self,
        owner_id: &str,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<OwnerDevice>, ControlError> {
        let sql = format!(
            "SELECT {DEVICE_COLUMNS} FROM owner_devices WHERE owner_id = ? AND token_hash = ? \
             AND revoked_at IS NULL AND expires_at > ?"
        );
        let rows = self
            .query(
                &sql,
                vec![text(owner_id), text(token_hash), SqlValue::Integer(now)],
            )
            .await?;
        rows.first().map(|r| device_from(r)).transpose()
    }

    pub async fn complete_signin(&self, c: &SigninCompletion<'_>) -> Result<bool, ControlError> {
        let mut session = c.session.clone();
        session.owner_id = c.owner_id.to_owned();
        session.device_id = c.device.as_ref().map(|d| d.id.clone());
        let now = SqlValue::Integer(c.now);
        let mut stmts = vec![
            Stmt::new(
                "UPDATE signin_challenges SET consumed_at = ? WHERE id = ? AND owner_id = ? \
                 AND consumed_at IS NULL AND attempts < ? AND expires_at > ?",
                vec![
                    now.clone(),
                    text(c.challenge_id),
                    text(c.owner_id),
                    SqlValue::Integer(i64::from(c.max_attempts)),
                    now.clone(),
                ],
            )
            .expect(1),
        ];
        stmts.push(match &c.factor {
            SigninFactor::Totp { step } => Stmt::new(
                "UPDATE owner_mfa SET last_step = ? WHERE owner_id = ? \
                 AND confirmed_at IS NOT NULL AND (last_step IS NULL OR last_step < ?)",
                vec![
                    SqlValue::Integer(*step),
                    text(c.owner_id),
                    SqlValue::Integer(*step),
                ],
            )
            .expect(1),
            SigninFactor::BackupCode { id } => Stmt::new(
                "UPDATE owner_backup_codes SET used_at = ? WHERE id = ? AND owner_id = ? \
                 AND used_at IS NULL",
                vec![now.clone(), text(id), text(c.owner_id)],
            )
            .expect(1),
            SigninFactor::Sms {
                code_hash,
                sent_after,
            } => Stmt::new(
                "SELECT 1 FROM signin_challenges WHERE id = ? AND sms_code_hash = ? \
                 AND sms_sent_at > ?",
                vec![
                    text(c.challenge_id),
                    text(code_hash),
                    SqlValue::Integer(*sent_after),
                ],
            )
            .fetch()
            .expect(1),
        });
        stmts.push(reset_failures(c.owner_id, c.now, true));
        if let Some(d) = c.device.as_ref().filter(|d| d.refresh) {
            stmts.push(
                Stmt::new(
                    "UPDATE owner_devices SET token_hash = ?, label = ?, last_seen_at = ?, \
                     expires_at = ? WHERE id = ? AND owner_id = ? AND revoked_at IS NULL \
                     AND expires_at > ?",
                    vec![
                        text(&d.token_hash),
                        text(&d.label),
                        now.clone(),
                        SqlValue::Integer(d.expires_at),
                        text(&d.id),
                        text(c.owner_id),
                        now.clone(),
                    ],
                )
                .expect(1),
            );
        }
        if let Some(d) = c.device.as_ref().filter(|d| !d.refresh) {
            stmts.push(Stmt::new(
                "DELETE FROM owner_devices WHERE owner_id = ? AND (expires_at < ? \
                 OR revoked_at < ?)",
                vec![
                    text(c.owner_id),
                    SqlValue::Integer(c.now - DAY_MS),
                    SqlValue::Integer(c.now - DAY_MS),
                ],
            ));
            stmts.push(
                Stmt::new(
                    "INSERT INTO owner_devices (id, owner_id, token_hash, label, created_at, \
                     last_seen_at, expires_at, revoked_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
                    vec![
                        text(&d.id),
                        text(c.owner_id),
                        text(&d.token_hash),
                        text(&d.label),
                        now.clone(),
                        now.clone(),
                        SqlValue::Integer(d.expires_at),
                    ],
                )
                .expect(1),
            );
        }
        stmts.push(Stmt::new(PRUNE_SESSIONS, vec![now]));
        stmts.push(insert_stmt(&session).expect(1));
        self.transact(stmts).await
    }

    pub async fn backup_codes_remaining(&self, owner_id: &str) -> Result<u32, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM owner_backup_codes WHERE owner_id = ? AND used_at IS NULL",
                vec![text(owner_id)],
            )
            .await?;
        let n = rows
            .first()
            .map(|r| col_int(r, 0))
            .transpose()?
            .unwrap_or(0);
        Ok(u32::try_from(n).unwrap_or(0))
    }

    pub async fn set_sms_fallback(
        &self,
        owner_id: &str,
        enabled: bool,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE owners SET sms_fallback = ? WHERE id = ?",
                vec![SqlValue::Integer(i64::from(enabled)), text(owner_id)],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn owner_devices(
        &self,
        owner_id: &str,
        now: i64,
    ) -> Result<Vec<OwnerDevice>, ControlError> {
        let sql = format!(
            "SELECT {DEVICE_COLUMNS} FROM owner_devices WHERE owner_id = ? \
             AND revoked_at IS NULL AND expires_at > ? ORDER BY last_seen_at DESC, id"
        );
        let rows = self
            .query(&sql, vec![text(owner_id), SqlValue::Integer(now)])
            .await?;
        rows.iter().map(|r| device_from(r)).collect()
    }

    pub async fn revoke_owner_device(
        &self,
        owner_id: &str,
        id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE owner_devices SET revoked_at = ? WHERE id = ? AND owner_id = ? \
                 AND revoked_at IS NULL AND expires_at > ?",
                vec![
                    SqlValue::Integer(now),
                    text(id),
                    text(owner_id),
                    SqlValue::Integer(now),
                ],
            )
            .expect(1),
            Stmt::new(
                "UPDATE owner_sessions SET revoked_at = ? WHERE owner_id = ? \
                 AND revoked_at IS NULL AND (device_id = ? OR family_id IN \
                 (SELECT family_id FROM owner_sessions WHERE owner_id = ? AND device_id = ?))",
                vec![
                    SqlValue::Integer(now),
                    text(owner_id),
                    text(id),
                    text(owner_id),
                    text(id),
                ],
            ),
        ])
        .await
    }

    pub async fn sign_out_everywhere(&self, owner_id: &str, now: i64) -> Result<(), ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE owner_sessions SET revoked_at = ? WHERE owner_id = ? AND revoked_at IS NULL",
                vec![SqlValue::Integer(now), text(owner_id)],
            ),
            Stmt::new(
                "UPDATE owner_devices SET revoked_at = ? WHERE owner_id = ? AND revoked_at IS NULL",
                vec![SqlValue::Integer(now), text(owner_id)],
            ),
            Stmt::new(
                "UPDATE signin_challenges SET consumed_at = ? WHERE owner_id = ? \
                 AND consumed_at IS NULL",
                vec![SqlValue::Integer(now), text(owner_id)],
            ),
        ])
        .await?;
        Ok(())
    }
}

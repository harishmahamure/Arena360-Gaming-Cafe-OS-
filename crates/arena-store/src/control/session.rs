//! `owner_sessions`: an owner's browser sessions. Tokens are stored only as SHA-256 hashes
//! (computed by the caller). Refresh tokens rotate; presenting a rotated one again revokes
//! its whole family.

use super::owner::{OWNER_COLUMNS, Owner, owner_from};
use super::signin::SigninFactor;
use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// A session to create.
#[derive(Clone)]
pub struct NewOwnerSession {
    pub id: String,
    pub owner_id: String,
    pub family_id: String,
    pub access_hash: String,
    pub access_expires_at: i64,
    pub refresh_hash: String,
    pub refresh_expires_at: i64,
    pub created_at: i64,
    pub device_id: Option<String>,
}

impl std::fmt::Debug for NewOwnerSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewOwnerSession")
            .field("id", &self.id)
            .field("family_id", &self.family_id)
            .finish_non_exhaustive()
    }
}

/// A stored session (hashes omitted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerSession {
    pub id: String,
    pub owner_id: String,
    pub family_id: String,
    pub access_expires_at: i64,
    pub refresh_expires_at: i64,
    pub rotated_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
    pub device_id: Option<String>,
    /// When the owner last passed a step-up on this session (Story 8.25); a refresh carries it over.
    pub stepup_at: Option<i64>,
}

/// A live session and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWithOwner {
    pub session: OwnerSession,
    pub owner: Owner,
}

/// Deletes the sessions whose refresh token expired more than a day before `?` (now); run
/// whenever a session is created.
pub(super) const PRUNE_SESSIONS: &str =
    "DELETE FROM owner_sessions WHERE refresh_expires_at < ? - 86400000";

/// The owner of session `?`, in [`OWNER_COLUMNS`] order.
const OWNER_OF_SESSION: &str = "SELECT o.id, o.name, o.phone_e164, o.email, o.cafe_name, o.city, \
     o.mfa_state, o.created_at, o.password_set_at, o.sms_fallback, o.failed_signins, \
     o.locked_until FROM owners o JOIN owner_sessions s ON s.owner_id = o.id WHERE s.id = ?";

/// A refresh token presented again within this long of its rotation is a concurrent refresh
/// (two tabs), not theft: refused without revoking the family.
pub const REFRESH_RACE_MS: i64 = 10_000;

/// What [`ControlDb::rotate_owner_session`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The refresh token was live: it is now rotated and the new session exists, for this
    /// owner (read inside the same transaction).
    Rotated(Owner),
    /// The refresh token was rotated less than [`REFRESH_RACE_MS`] ago: refused, nothing
    /// revoked (the request that rotated it set the new cookies).
    RecentlyRotated,
    /// The refresh token had been rotated longer ago: its whole family is now revoked.
    Reused,
    /// Unknown, expired or revoked: nothing changed.
    Invalid,
}

const SESSION_COLUMNS: &str = "s.id, s.owner_id, s.family_id, s.access_expires_at, \
                               s.refresh_expires_at, s.rotated_at, s.revoked_at, s.created_at, \
                               s.device_id, s.stepup_at";

fn session_from(row: &[SqlValue]) -> Result<OwnerSession, ControlError> {
    Ok(OwnerSession {
        id: col_text(row, 0)?,
        owner_id: col_text(row, 1)?,
        family_id: col_text(row, 2)?,
        access_expires_at: col_int(row, 3)?,
        refresh_expires_at: col_int(row, 4)?,
        rotated_at: col_opt_int(row, 5)?,
        revoked_at: col_opt_int(row, 6)?,
        created_at: col_int(row, 7)?,
        device_id: col_opt_text(row, 8)?,
        stepup_at: col_opt_int(row, 9)?,
    })
}

pub(super) fn insert_stmt(s: &NewOwnerSession) -> Stmt {
    Stmt::new(
        "INSERT INTO owner_sessions (id, owner_id, family_id, access_hash, access_expires_at, \
         refresh_hash, refresh_expires_at, rotated_at, revoked_at, created_at, device_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?)",
        vec![
            text(&s.id),
            text(&s.owner_id),
            text(&s.family_id),
            text(&s.access_hash),
            SqlValue::Integer(s.access_expires_at),
            text(&s.refresh_hash),
            SqlValue::Integer(s.refresh_expires_at),
            SqlValue::Integer(s.created_at),
            s.device_id.as_deref().map_or(SqlValue::Null, text),
        ],
    )
}

impl ControlDb {
    /// Creates a session.
    pub async fn insert_owner_session(&self, s: &NewOwnerSession) -> Result<(), ControlError> {
        self.transact(vec![
            Stmt::new(PRUNE_SESSIONS, vec![SqlValue::Integer(s.created_at)]),
            insert_stmt(s),
        ])
        .await?;
        Ok(())
    }

    /// The session with this id, whatever its state.
    pub async fn owner_session(&self, id: &str) -> Result<Option<OwnerSession>, ControlError> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM owner_sessions s WHERE s.id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| session_from(r)).transpose()
    }

    /// Every session of a family, oldest first.
    pub async fn owner_session_family(
        &self,
        family_id: &str,
    ) -> Result<Vec<OwnerSession>, ControlError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM owner_sessions s WHERE s.family_id = ? \
             ORDER BY s.created_at, s.id"
        );
        let rows = self.query(&sql, vec![text(family_id)]).await?;
        rows.iter().map(|r| session_from(r)).collect()
    }

    /// The live session (not rotated, not revoked, access not expired at `now`) whose access
    /// token hashes to `access_hash`, with its owner.
    pub async fn owner_session_by_access(
        &self,
        access_hash: &str,
        now: i64,
    ) -> Result<Option<SessionWithOwner>, ControlError> {
        let owner_cols = OWNER_COLUMNS
            .split(", ")
            .map(|c| format!("o.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {SESSION_COLUMNS}, {owner_cols} FROM owner_sessions s \
             JOIN owners o ON o.id = s.owner_id \
             WHERE s.access_hash = ? AND s.rotated_at IS NULL AND s.revoked_at IS NULL \
             AND s.access_expires_at > ?"
        );
        let rows = self
            .query(&sql, vec![text(access_hash), SqlValue::Integer(now)])
            .await?;
        rows.first()
            .map(|r| {
                Ok(SessionWithOwner {
                    session: session_from(r)?,
                    owner: owner_from(r, 10)?,
                })
            })
            .transpose()
    }

    /// Rotates the refresh token hashing to `refresh_hash`: in one transaction, marks its
    /// session `rotated_at = now`, prunes long-expired sessions, creates `next` in the same
    /// family for the same owner (`next.owner_id` and `next.family_id` are taken from the old
    /// session) and reads that owner. A token rotated within [`REFRESH_RACE_MS`] is refused
    /// as is; one rotated earlier revokes its whole family ([`RefreshOutcome::Reused`]).
    pub async fn rotate_owner_session(
        &self,
        refresh_hash: &str,
        next: &NewOwnerSession,
        now: i64,
    ) -> Result<RefreshOutcome, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE owner_sessions SET rotated_at = ? WHERE refresh_hash = ? \
                     AND rotated_at IS NULL AND revoked_at IS NULL AND refresh_expires_at > ?",
                    vec![
                        SqlValue::Integer(now),
                        text(refresh_hash),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
                Stmt::new(PRUNE_SESSIONS, vec![SqlValue::Integer(now)]),
                Stmt::new(
                    "INSERT INTO owner_sessions (id, owner_id, family_id, access_hash, \
                     access_expires_at, refresh_hash, refresh_expires_at, rotated_at, revoked_at, \
                     created_at, device_id, stepup_at) SELECT ?, owner_id, family_id, ?, ?, ?, ?, \
                     NULL, NULL, ?, device_id, stepup_at FROM owner_sessions \
                     WHERE refresh_hash = ?",
                    vec![
                        text(&next.id),
                        text(&next.access_hash),
                        SqlValue::Integer(next.access_expires_at),
                        text(&next.refresh_hash),
                        SqlValue::Integer(next.refresh_expires_at),
                        SqlValue::Integer(next.created_at),
                        text(refresh_hash),
                    ],
                )
                .expect(1),
                Stmt::new(OWNER_OF_SESSION, vec![text(&next.id)])
                    .fetch()
                    .expect(1),
            ])
            .await?;
        if let Some(rows) = rows {
            return Ok(RefreshOutcome::Rotated(owner_from(&rows[3][0], 0)?));
        }
        let rows = self
            .query(
                "SELECT family_id, rotated_at, revoked_at FROM owner_sessions \
                 WHERE refresh_hash = ?",
                vec![text(refresh_hash)],
            )
            .await?;
        let Some(row) = rows.first() else {
            return Ok(RefreshOutcome::Invalid);
        };
        let family = col_text(row, 0)?;
        let rotated_at = col_opt_int(row, 1)?;
        let revoked = col_opt_int(row, 2)?.is_some();
        match rotated_at {
            Some(at) if !revoked && now - at < REFRESH_RACE_MS => {
                Ok(RefreshOutcome::RecentlyRotated)
            }
            Some(_) if !revoked => {
                self.revoke_owner_session_family(&family, now).await?;
                Ok(RefreshOutcome::Reused)
            }
            _ => Ok(RefreshOutcome::Invalid),
        }
    }

    /// Records a passed step-up (Story 8.25): in one transaction, consumes the factor (a TOTP step
    /// newer than the last accepted one, an unused backup code, or the session's own text-message
    /// code sent after `sent_after`) and sets `stepup_at = now` on the live session `session_id` of
    /// `owner_id`. `false`, with nothing changed, when the factor was already used or the session is
    /// gone.
    pub async fn complete_stepup(
        &self,
        session_id: &str,
        owner_id: &str,
        factor: &SigninFactor,
        now: i64,
    ) -> Result<bool, ControlError> {
        let consume = match factor {
            SigninFactor::Totp { step } => Stmt::new(
                "UPDATE owner_mfa SET last_step = ? WHERE owner_id = ? \
                 AND confirmed_at IS NOT NULL AND (last_step IS NULL OR last_step < ?)",
                vec![
                    SqlValue::Integer(*step),
                    text(owner_id),
                    SqlValue::Integer(*step),
                ],
            )
            .expect(1),
            SigninFactor::BackupCode { id } => Stmt::new(
                "UPDATE owner_backup_codes SET used_at = ? WHERE id = ? AND owner_id = ? \
                 AND used_at IS NULL",
                vec![SqlValue::Integer(now), text(id), text(owner_id)],
            )
            .expect(1),
            SigninFactor::Sms {
                code_hash,
                sent_after,
            } => Stmt::new(
                "SELECT 1 FROM owner_sessions WHERE id = ? AND stepup_sms_hash = ? \
                 AND stepup_sms_sent_at > ?",
                vec![
                    text(session_id),
                    text(code_hash),
                    SqlValue::Integer(*sent_after),
                ],
            )
            .fetch()
            .expect(1),
        };
        self.transact(vec![
            consume,
            Stmt::new(
                "UPDATE owner_sessions SET stepup_at = ?, stepup_sms_hash = NULL, \
                 stepup_sms_sent_at = NULL WHERE id = ? AND owner_id = ? \
                 AND rotated_at IS NULL AND revoked_at IS NULL",
                vec![SqlValue::Integer(now), text(session_id), text(owner_id)],
            )
            .expect(1),
        ])
        .await
    }

    /// The text-message code stored for the session's step-up: its hash and when it was sent.
    pub async fn stepup_sms(
        &self,
        session_id: &str,
    ) -> Result<Option<(String, i64)>, ControlError> {
        let rows = self
            .query(
                "SELECT stepup_sms_hash, stepup_sms_sent_at FROM owner_sessions WHERE id = ? \
                 AND stepup_sms_hash IS NOT NULL AND stepup_sms_sent_at IS NOT NULL",
                vec![text(session_id)],
            )
            .await?;
        rows.first()
            .map(|r| Ok((col_text(r, 0)?, col_int(r, 1)?)))
            .transpose()
    }

    /// Stores (or, with `None`, clears) the session's step-up text-message code. When `seen_sent_at`
    /// is given it must still be the stored send time, so two sends racing store one code.
    pub async fn set_stepup_sms(
        &self,
        session_id: &str,
        seen_sent_at: Option<i64>,
        code: Option<(&str, i64)>,
    ) -> Result<bool, ControlError> {
        let (hash, sent) = match code {
            Some((h, at)) => (text(h), SqlValue::Integer(at)),
            None => (SqlValue::Null, SqlValue::Null),
        };
        let n = self
            .execute(
                "UPDATE owner_sessions SET stepup_sms_hash = ?, stepup_sms_sent_at = ? \
                 WHERE id = ? AND rotated_at IS NULL AND revoked_at IS NULL \
                 AND stepup_sms_sent_at IS ?",
                vec![
                    hash,
                    sent,
                    text(session_id),
                    seen_sent_at.map_or(SqlValue::Null, SqlValue::Integer),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    /// Revokes every live session of a family.
    pub async fn revoke_owner_session_family(
        &self,
        family_id: &str,
        now: i64,
    ) -> Result<u64, ControlError> {
        self.execute(
            "UPDATE owner_sessions SET revoked_at = ? WHERE family_id = ? AND revoked_at IS NULL",
            vec![SqlValue::Integer(now), text(family_id)],
        )
        .await
    }
}

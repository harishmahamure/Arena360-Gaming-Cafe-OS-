//! `otp_challenges`: hashed one-time codes, and the atomic end of a sign-up.

use super::owner::{NewOwner, Owner, owner_from};
use super::session::{NewOwnerSession, PRUNE_SESSIONS};
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

/// Challenges expired for longer than this are deleted when a new one is stored.
pub const OTP_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

/// `purpose` of a sign-up challenge.
pub const OTP_PURPOSE_SIGNUP: &str = "signup";

/// A challenge to store. `code_hash` is computed by the caller (never the code itself).
#[derive(Clone)]
pub struct NewOtpChallenge {
    pub id: String,
    pub phone_e164: String,
    pub code_hash: String,
    /// The submitted form, kept to create the owner once the code is verified.
    pub payload: NewOwner,
    pub created_at: i64,
    pub expires_at: i64,
}

/// A stored challenge.
#[derive(Clone, PartialEq, Eq)]
pub struct OtpChallenge {
    pub id: String,
    pub purpose: String,
    pub phone_e164: String,
    pub code_hash: String,
    /// The submitted form; `None` once the challenge was consumed or voided.
    pub payload: Option<NewOwner>,
    pub created_at: i64,
    pub expires_at: i64,
    pub attempts: u32,
    pub consumed_at: Option<i64>,
}

impl std::fmt::Debug for OtpChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtpChallenge")
            .field("id", &self.id)
            .field("expires_at", &self.expires_at)
            .field("attempts", &self.attempts)
            .field("consumed_at", &self.consumed_at)
            .finish_non_exhaustive()
    }
}

/// What [`ControlDb::complete_signup`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignupOutcome {
    /// The challenge was not consumable any more (consumed, void, expired or re-issued since
    /// it was read): nothing was written.
    NotConsumed,
    /// Consumed; the owner (new, or existing and still `pending`) has the new session.
    Session(Owner),
    /// Consumed; the phone belongs to an `enrolled` owner, so no session was created.
    Enrolled(Owner),
}

/// The inputs of [`ControlDb::complete_signup`].
pub struct SignupCompletion<'a> {
    pub challenge_id: &'a str,
    /// The hash the caller verified the code against (a guard against a concurrent resend).
    pub code_hash: &'a str,
    /// Wrong codes that void a challenge.
    pub max_attempts: u32,
    pub now: i64,
    /// The id a new owner row gets (unused when the phone already has an owner).
    pub new_owner_id: &'a str,
    pub owner: &'a NewOwner,
    /// The session to create; its `owner_id` is ignored (the owner is found by phone).
    pub session: &'a NewOwnerSession,
}

const CHALLENGE_COLUMNS: &str = "id, purpose, phone_e164, code_hash, payload_json, created_at, \
                                 expires_at, attempts, consumed_at";

fn challenge_from(row: &[SqlValue]) -> Result<OtpChallenge, ControlError> {
    let payload = match row.get(4) {
        Some(SqlValue::Null) => None,
        _ => Some(
            serde_json::from_str(&col_text(row, 4)?)
                .map_err(|e| ControlError::Db(format!("otp_challenges.payload_json: {e}")))?,
        ),
    };
    Ok(OtpChallenge {
        id: col_text(row, 0)?,
        purpose: col_text(row, 1)?,
        phone_e164: col_text(row, 2)?,
        code_hash: col_text(row, 3)?,
        payload,
        created_at: col_int(row, 5)?,
        expires_at: col_int(row, 6)?,
        attempts: u32::try_from(col_int(row, 7)?).unwrap_or(u32::MAX),
        consumed_at: col_opt_int(row, 8)?,
    })
}

impl ControlDb {
    /// Stores a new sign-up challenge, deleting (in the same transaction) every challenge that
    /// expired more than [`OTP_RETENTION_MS`] before `c.created_at`.
    pub async fn insert_otp_challenge(&self, c: &NewOtpChallenge) -> Result<(), ControlError> {
        let payload = serde_json::to_string(&c.payload)
            .map_err(|e| ControlError::Db(format!("payload_json: {e}")))?;
        self.transact(vec![
            Stmt::new(
                "DELETE FROM otp_challenges WHERE expires_at < ?",
                vec![SqlValue::Integer(c.created_at - OTP_RETENTION_MS)],
            ),
            Stmt::new(
                "INSERT INTO otp_challenges (id, purpose, phone_e164, code_hash, payload_json, \
                 created_at, expires_at, attempts, consumed_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, 0, NULL)",
                vec![
                    text(&c.id),
                    text(OTP_PURPOSE_SIGNUP),
                    text(&c.phone_e164),
                    text(&c.code_hash),
                    SqlValue::Text(payload),
                    SqlValue::Integer(c.created_at),
                    SqlValue::Integer(c.expires_at),
                ],
            ),
        ])
        .await?;
        Ok(())
    }

    /// The challenge with this id.
    pub async fn otp_challenge(&self, id: &str) -> Result<Option<OtpChallenge>, ControlError> {
        let sql = format!("SELECT {CHALLENGE_COLUMNS} FROM otp_challenges WHERE id = ?");
        let rows = self.query(&sql, vec![text(id)]).await?;
        rows.first().map(|r| challenge_from(r)).transpose()
    }

    /// Deletes an unconsumed challenge (its code never reached the phone).
    pub async fn delete_otp_challenge(&self, id: &str) -> Result<(), ControlError> {
        self.execute(
            "DELETE FROM otp_challenges WHERE id = ? AND consumed_at IS NULL",
            vec![text(id)],
        )
        .await?;
        Ok(())
    }

    /// Puts a new code on a challenge that is unconsumed, not void and still expires at
    /// `seen_expires_at` (what the caller read): a new hash and expiry, attempts back to 0.
    /// `false` when any of that no longer holds (another resend won the race).
    pub async fn reissue_otp_challenge(
        &self,
        id: &str,
        seen_expires_at: i64,
        code_hash: &str,
        expires_at: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE otp_challenges SET code_hash = ?, expires_at = ?, attempts = 0 \
                 WHERE id = ? AND consumed_at IS NULL AND payload_json IS NOT NULL \
                 AND expires_at = ?",
                vec![
                    text(code_hash),
                    SqlValue::Integer(expires_at),
                    text(id),
                    SqlValue::Integer(seen_expires_at),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    /// Undoes [`ControlDb::reissue_otp_challenge`] after the new code could not be sent: puts
    /// the previous hash, expiry and attempts back, if the challenge still carries the new
    /// ones.
    pub async fn revert_otp_reissue(
        &self,
        id: &str,
        new_hash: &str,
        new_expires_at: i64,
        previous: &OtpChallenge,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE otp_challenges SET code_hash = ?, expires_at = ?, attempts = ? \
             WHERE id = ? AND consumed_at IS NULL AND code_hash = ? AND expires_at = ?",
            vec![
                text(&previous.code_hash),
                SqlValue::Integer(previous.expires_at),
                SqlValue::Integer(i64::from(previous.attempts)),
                text(id),
                text(new_hash),
                SqlValue::Integer(new_expires_at),
            ],
        )
        .await?;
        Ok(())
    }

    /// Counts one wrong code on an unconsumed challenge that still has attempts left, and
    /// answers the attempts made so far (`None`: nothing was counted). The wrong code that
    /// voids the challenge also clears its stored form.
    pub async fn record_otp_failure(
        &self,
        id: &str,
        max_attempts: u32,
    ) -> Result<Option<u32>, ControlError> {
        let max = SqlValue::Integer(i64::from(max_attempts));
        let rows = self
            .query(
                "UPDATE otp_challenges SET attempts = attempts + 1, payload_json = \
                 CASE WHEN attempts + 1 >= ? THEN NULL ELSE payload_json END \
                 WHERE id = ? AND consumed_at IS NULL AND attempts < ? RETURNING attempts",
                vec![max.clone(), text(id), max],
            )
            .await?;
        rows.first()
            .map(|r| col_int(r, 0).map(|n| u32::try_from(n).unwrap_or(u32::MAX)))
            .transpose()
    }

    /// The end of a sign-up, in one transaction: consumes the challenge (only if it is still
    /// unconsumed, unexpired, has attempts left and still carries `code_hash`) and clears its
    /// stored form, creates the owner when the phone has none (`pending`; an existing row is
    /// never overwritten), prunes sessions whose refresh expired over a day ago and creates the
    /// session when that owner is `pending`. The outcome is read inside the transaction.
    pub async fn complete_signup(
        &self,
        c: &SignupCompletion<'_>,
    ) -> Result<SignupOutcome, ControlError> {
        let s = c.session;
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE otp_challenges SET consumed_at = ?, payload_json = NULL \
                     WHERE id = ? AND code_hash = ? AND consumed_at IS NULL AND attempts < ? \
                     AND expires_at > ?",
                    vec![
                        SqlValue::Integer(c.now),
                        text(c.challenge_id),
                        text(c.code_hash),
                        SqlValue::Integer(i64::from(c.max_attempts)),
                        SqlValue::Integer(c.now),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "INSERT INTO owners (id, name, phone_e164, email, cafe_name, city, mfa_state, \
                     created_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?) \
                     ON CONFLICT (phone_e164) DO NOTHING",
                    vec![
                        text(c.new_owner_id),
                        text(&c.owner.name),
                        text(&c.owner.phone_e164),
                        text(&c.owner.email),
                        text(&c.owner.cafe_name),
                        text(&c.owner.city),
                        SqlValue::Integer(c.now),
                    ],
                ),
                Stmt::new(PRUNE_SESSIONS, vec![SqlValue::Integer(c.now)]),
                Stmt::new(
                    "INSERT INTO owner_sessions (id, owner_id, family_id, access_hash, \
                     access_expires_at, refresh_hash, refresh_expires_at, rotated_at, revoked_at, \
                     created_at) SELECT ?, id, ?, ?, ?, ?, ?, NULL, NULL, ? FROM owners \
                     WHERE phone_e164 = ? AND mfa_state = 'pending'",
                    vec![
                        text(&s.id),
                        text(&s.family_id),
                        text(&s.access_hash),
                        SqlValue::Integer(s.access_expires_at),
                        text(&s.refresh_hash),
                        SqlValue::Integer(s.refresh_expires_at),
                        SqlValue::Integer(s.created_at),
                        text(&c.owner.phone_e164),
                    ],
                ),
                Stmt::new(OWNER_BY_PHONE, vec![text(&c.owner.phone_e164)])
                    .fetch()
                    .expect(1),
                Stmt::new(
                    "SELECT COUNT(*) FROM owner_sessions WHERE id = ?",
                    vec![text(&s.id)],
                )
                .fetch(),
            ])
            .await?;
        let Some(rows) = rows else {
            return Ok(SignupOutcome::NotConsumed);
        };
        let owner = owner_from(&rows[4][0], 0)?;
        let sessions = rows[5].first().map(|r| col_int(r, 0)).transpose()?;
        if sessions == Some(1) {
            Ok(SignupOutcome::Session(owner))
        } else {
            Ok(SignupOutcome::Enrolled(owner))
        }
    }
}

const OWNER_BY_PHONE: &str = "SELECT id, name, phone_e164, email, cafe_name, city, mfa_state, \
                              created_at, password_set_at FROM owners WHERE phone_e164 = ?";

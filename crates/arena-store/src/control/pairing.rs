use arena_proto::{HubRole, Tier};

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPairingCode<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub code_hash: &'a str,
    /// `Standby` issues a standby code (Story 9.18): a branch keeps one live code per role.
    pub role: HubRole,
    pub now: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueOutcome {
    Issued,
    NotReady,
    HashTaken,
    /// A standby code was asked for while the branch already has a live standby Hub.
    StandbyExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCodeRow {
    pub id: String,
    pub branch_id: String,
    pub code_hash: String,
    pub tier_at_issue: Tier,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
    pub invalidated_at: Option<i64>,
    pub role: HubRole,
    /// `Migrate` for the code that pairs the Windows Hub of a Hub move (Story 10.16).
    pub purpose: CodePurpose,
}

/// What a pairing code is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePurpose {
    /// A Hub code or a standby code (by role).
    Pair,
    /// The Windows Hub of a move from an Android Hub (Story 10.16).
    Migrate,
}

impl CodePurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Migrate => "migrate",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Pair, Self::Migrate]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
}

impl PairingCodeRow {
    pub fn is_valid_at(&self, now: i64) -> bool {
        self.used_at.is_none() && self.invalidated_at.is_none() && now < self.expires_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeemed {
    pub branch_id: String,
    pub tenant_id: String,
    pub tier: Tier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemInvalid;

pub const PAIRING_HISTORY_MS: i64 = 24 * 60 * 60 * 1000;

const PAIRING_COLUMNS: &str = "id, branch_id, code_hash, tier_at_issue, created_at, expires_at, \
     used_at, invalidated_at, role, purpose";

fn tier_of(raw: &str) -> Result<Tier, ControlError> {
    Tier::parse(raw).ok_or_else(|| ControlError::Db(format!("unknown tier {raw:?}")))
}

fn pairing_from(row: &[SqlValue]) -> Result<PairingCodeRow, ControlError> {
    Ok(PairingCodeRow {
        id: col_text(row, 0)?,
        branch_id: col_text(row, 1)?,
        code_hash: col_text(row, 2)?,
        tier_at_issue: tier_of(&col_text(row, 3)?)?,
        created_at: col_int(row, 4)?,
        expires_at: col_int(row, 5)?,
        used_at: col_opt_int(row, 6)?,
        invalidated_at: col_opt_int(row, 7)?,
        role: {
            let role = col_text(row, 8)?;
            HubRole::parse(&role)
                .ok_or_else(|| ControlError::Db(format!("unknown hub role {role:?}")))?
        },
        purpose: {
            let purpose = col_text(row, 9)?;
            CodePurpose::parse(&purpose)
                .ok_or_else(|| ControlError::Db(format!("unknown code purpose {purpose:?}")))?
        },
    })
}

impl ControlDb {
    pub async fn pairing_steps_ready(&self, branch_id: &str) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM onboarding_progress WHERE branch_id = ? \
                 AND step IN ('cafe', 'tier') AND state = 'done'",
                vec![text(branch_id)],
            )
            .await?;
        Ok(rows.first().map(|r| col_int(r, 0)).transpose()? == Some(2))
    }

    pub async fn issue_pairing_code(
        &self,
        c: &NewPairingCode<'_>,
    ) -> Result<IssueOutcome, ControlError> {
        let issued = self
            .transact(vec![
                Stmt::new(
                    "DELETE FROM pairing_codes WHERE expires_at < ?",
                    vec![SqlValue::Integer(c.now - PAIRING_HISTORY_MS)],
                ),
                Stmt::new(
                    "UPDATE pairing_codes SET invalidated_at = ? WHERE branch_id = ? AND role = ? \
                     AND purpose = 'pair' AND used_at IS NULL AND invalidated_at IS NULL",
                    vec![
                        SqlValue::Integer(c.now),
                        text(c.branch_id),
                        text(c.role.as_str()),
                    ],
                ),
                Stmt::new(
                    "INSERT INTO pairing_codes (id, branch_id, code_hash, tier_at_issue, \
                     created_at, expires_at, used_at, invalidated_at, role) \
                     SELECT ?, ?, ?, COALESCE((SELECT CASE WHEN e.until IS NOT NULL \
                     AND ? > e.until THEN 'free' ELSE e.tier END FROM entitlements e \
                     WHERE e.branch_id = ?), 'free'), ?, ?, NULL, NULL, ? \
                     WHERE (SELECT COUNT(*) FROM onboarding_progress WHERE branch_id = ? \
                     AND step IN ('cafe', 'tier') AND state = 'done') = 2 \
                     AND NOT EXISTS (SELECT 1 FROM pairing_codes WHERE code_hash = ?) \
                     AND (? <> 'standby' OR NOT EXISTS (SELECT 1 FROM hubs \
                     WHERE branch_id = ? AND role = 'standby' AND revoked_at IS NULL))",
                    vec![
                        text(c.id),
                        text(c.branch_id),
                        text(c.code_hash),
                        SqlValue::Integer(c.now),
                        text(c.branch_id),
                        SqlValue::Integer(c.now),
                        SqlValue::Integer(c.expires_at),
                        text(c.role.as_str()),
                        text(c.branch_id),
                        text(c.code_hash),
                        text(c.role.as_str()),
                        text(c.branch_id),
                    ],
                )
                .expect(1),
            ])
            .await?;
        if issued {
            return Ok(IssueOutcome::Issued);
        }
        if !self.pairing_steps_ready(c.branch_id).await? {
            return Ok(IssueOutcome::NotReady);
        }
        if c.role == HubRole::Standby && self.standby_hub(c.branch_id).await?.is_some() {
            return Ok(IssueOutcome::StandbyExists);
        }
        Ok(IssueOutcome::HashTaken)
    }

    pub async fn pairing_codes_of(
        &self,
        branch_id: &str,
    ) -> Result<Vec<PairingCodeRow>, ControlError> {
        let sql = format!(
            "SELECT {PAIRING_COLUMNS} FROM pairing_codes WHERE branch_id = ? \
             ORDER BY created_at, id"
        );
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.iter().map(|r| pairing_from(r)).collect()
    }

    pub async fn active_pairing_code(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<PairingCodeRow>, ControlError> {
        self.active_code_of(branch_id, HubRole::Main, now).await
    }

    /// The branch's unused, unexpired standby code (Story 9.18).
    pub async fn active_standby_code(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<PairingCodeRow>, ControlError> {
        self.active_code_of(branch_id, HubRole::Standby, now).await
    }

    async fn active_code_of(
        &self,
        branch_id: &str,
        role: HubRole,
        now: i64,
    ) -> Result<Option<PairingCodeRow>, ControlError> {
        let sql = format!(
            "SELECT {PAIRING_COLUMNS} FROM pairing_codes WHERE branch_id = ? AND role = ? \
             AND purpose = 'pair' AND used_at IS NULL AND invalidated_at IS NULL AND expires_at > ? \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        );
        let rows = self
            .query(
                &sql,
                vec![text(branch_id), text(role.as_str()), SqlValue::Integer(now)],
            )
            .await?;
        rows.first().map(|r| pairing_from(r)).transpose()
    }

    pub async fn pairing_code_by_hash(
        &self,
        code_hash: &str,
    ) -> Result<Option<PairingCodeRow>, ControlError> {
        let sql = format!("SELECT {PAIRING_COLUMNS} FROM pairing_codes WHERE code_hash = ?");
        let rows = self.query(&sql, vec![text(code_hash)]).await?;
        rows.first().map(|r| pairing_from(r)).transpose()
    }

    pub async fn invalidate_failed_pairing_code(
        &self,
        code_hash: &str,
        failures_key: &str,
        since: i64,
        limit: u64,
        now: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE pairing_codes SET invalidated_at = ? WHERE code_hash = ? \
                 AND used_at IS NULL AND invalidated_at IS NULL \
                 AND (SELECT COALESCE(SUM(count), 0) FROM rate_limits \
                 WHERE key = ? AND window_start > ?) >= ?",
                vec![
                    SqlValue::Integer(now),
                    text(code_hash),
                    text(failures_key),
                    SqlValue::Integer(since),
                    SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn redeem_pairing_code(
        &self,
        code_hash: &str,
        now: i64,
    ) -> Result<Result<Redeemed, RedeemInvalid>, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE pairing_codes SET used_at = ? WHERE code_hash = ? AND purpose = 'pair' \
                     AND used_at IS NULL AND invalidated_at IS NULL AND expires_at > ?",
                    vec![
                        SqlValue::Integer(now),
                        text(code_hash),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "SELECT p.branch_id, b.tenant_id, p.tier_at_issue FROM pairing_codes p \
                     JOIN branches b ON b.id = p.branch_id WHERE p.code_hash = ?",
                    vec![text(code_hash)],
                )
                .fetch()
                .expect(1),
            ])
            .await?;
        let Some(rows) = rows else {
            return Ok(Err(RedeemInvalid));
        };
        let row = rows
            .get(1)
            .and_then(|r| r.first())
            .ok_or_else(|| ControlError::Db("redeemed code has no row".into()))?;
        Ok(Ok(Redeemed {
            branch_id: col_text(row, 0)?,
            tenant_id: col_text(row, 1)?,
            tier: tier_of(&col_text(row, 2)?)?,
        }))
    }
}

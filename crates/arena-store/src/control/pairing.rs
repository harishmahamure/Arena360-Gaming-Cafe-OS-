use arena_proto::Tier;

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPairingCode<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub code_hash: &'a str,
    pub now: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueOutcome {
    Issued,
    NotReady,
    HashTaken,
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

const PAIRING_COLUMNS: &str =
    "id, branch_id, code_hash, tier_at_issue, created_at, expires_at, used_at, invalidated_at";

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
                    "UPDATE pairing_codes SET invalidated_at = ? WHERE branch_id = ? \
                     AND used_at IS NULL AND invalidated_at IS NULL",
                    vec![SqlValue::Integer(c.now), text(c.branch_id)],
                ),
                Stmt::new(
                    "INSERT INTO pairing_codes (id, branch_id, code_hash, tier_at_issue, \
                     created_at, expires_at, used_at, invalidated_at) \
                     SELECT ?, ?, ?, COALESCE((SELECT CASE WHEN e.until IS NOT NULL \
                     AND ? > e.until THEN 'free' ELSE e.tier END FROM entitlements e \
                     WHERE e.branch_id = ?), 'free'), ?, ?, NULL, NULL \
                     WHERE (SELECT COUNT(*) FROM onboarding_progress WHERE branch_id = ? \
                     AND step IN ('cafe', 'tier') AND state = 'done') = 2 \
                     AND NOT EXISTS (SELECT 1 FROM pairing_codes WHERE code_hash = ?)",
                    vec![
                        text(c.id),
                        text(c.branch_id),
                        text(c.code_hash),
                        SqlValue::Integer(c.now),
                        text(c.branch_id),
                        SqlValue::Integer(c.now),
                        SqlValue::Integer(c.expires_at),
                        text(c.branch_id),
                        text(c.code_hash),
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
        let sql = format!(
            "SELECT {PAIRING_COLUMNS} FROM pairing_codes WHERE branch_id = ? \
             AND used_at IS NULL AND invalidated_at IS NULL AND expires_at > ? \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        );
        let rows = self
            .query(&sql, vec![text(branch_id), SqlValue::Integer(now)])
            .await?;
        rows.first().map(|r| pairing_from(r)).transpose()
    }

    pub async fn redeem_pairing_code(
        &self,
        code_hash: &str,
        now: i64,
    ) -> Result<Result<Redeemed, RedeemInvalid>, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE pairing_codes SET used_at = ? WHERE code_hash = ? \
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

use arena_proto::{EntitlementSource, Tier};

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitlement {
    pub branch_id: String,
    pub tier: Tier,
    pub source: EntitlementSource,
    pub addons: Vec<String>,
    pub issued_at: i64,
    pub until: Option<i64>,
    pub updated_at: i64,
}

impl Entitlement {
    pub fn tier_at(&self, now: i64) -> Tier {
        match self.until {
            Some(until) if now > until => Tier::Free,
            _ => self.tier,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeTierOutcome {
    Written,
    AlreadyFree,
    OtherFree,
    OtherPlan(Entitlement),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantOutcome {
    Granted { previous: Option<Entitlement> },
    UnknownBranch,
    Paid(Entitlement),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompGrant<'a> {
    pub branch_id: &'a str,
    pub tier: Tier,
    pub until: i64,
    pub audit_id: &'a str,
    pub operator: &'a str,
    pub reason: &'a str,
    pub details_json: &'a str,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsAuditRow {
    pub id: String,
    pub at: i64,
    pub operator: String,
    pub action: String,
    pub target: String,
    pub reason: String,
    pub details_json: String,
}

const ENTITLEMENT_COLUMNS: &str =
    "branch_id, tier, source, addons_json, issued_at, until, updated_at";

fn entitlement_from(row: &[SqlValue]) -> Result<Entitlement, ControlError> {
    let tier = col_text(row, 1)?;
    let source = col_text(row, 2)?;
    let addons = col_text(row, 3)?;
    Ok(Entitlement {
        branch_id: col_text(row, 0)?,
        tier: Tier::parse(&tier)
            .ok_or_else(|| ControlError::Db(format!("unknown tier {tier:?}")))?,
        source: EntitlementSource::parse(&source)
            .ok_or_else(|| ControlError::Db(format!("unknown source {source:?}")))?,
        addons: serde_json::from_str(&addons)
            .map_err(|e| ControlError::Db(format!("addons_json: {e}")))?,
        issued_at: col_int(row, 4)?,
        until: col_opt_int(row, 5)?,
        updated_at: col_int(row, 6)?,
    })
}

const PREVIOUS_ENTITLEMENT: &str = "SELECT branch_id, tier, source, addons_json, issued_at, \
     until, updated_at FROM entitlements WHERE branch_id = ?";

const MARK_TIER_DONE: &str = "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
     VALUES (?, 'tier', 'done', ?) ON CONFLICT (branch_id, step) DO UPDATE SET \
     state = 'done', completed_at = COALESCE(onboarding_progress.completed_at, \
     excluded.completed_at)";

impl ControlDb {
    pub async fn entitlement(&self, branch_id: &str) -> Result<Option<Entitlement>, ControlError> {
        let sql = format!("SELECT {ENTITLEMENT_COLUMNS} FROM entitlements WHERE branch_id = ?");
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| entitlement_from(r)).transpose()
    }

    pub async fn other_free_branch(
        &self,
        tenant_id: &str,
        branch_id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT 1 FROM entitlements e WHERE e.tenant_id = ? AND e.branch_id <> ? \
                 AND (e.tier = 'free' OR (e.until IS NOT NULL AND e.until < ?)) LIMIT 1",
                vec![text(tenant_id), text(branch_id), SqlValue::Integer(now)],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    pub async fn choose_free_tier(
        &self,
        tenant_id: &str,
        branch_id: &str,
        now: i64,
    ) -> Result<FreeTierOutcome, ControlError> {
        let attempt = self
            .transact(vec![
                Stmt::new(
                    "INSERT INTO entitlements (branch_id, tenant_id, tier, source, addons_json, \
                     issued_at, until, updated_at) SELECT b.id, b.tenant_id, 'free', 'free', \
                     '[]', ?, NULL, ? FROM branches b WHERE b.id = ? AND b.tenant_id = ? \
                     AND NOT EXISTS (SELECT 1 FROM entitlements e \
                     WHERE e.tenant_id = b.tenant_id AND e.branch_id <> b.id \
                     AND (e.tier = 'free' OR (e.until IS NOT NULL AND e.until < ?))) \
                     ON CONFLICT (branch_id) DO UPDATE SET tier = 'free', source = 'free', \
                     issued_at = excluded.issued_at, until = NULL, \
                     updated_at = excluded.updated_at \
                     WHERE entitlements.tier <> 'free' AND entitlements.until IS NOT NULL \
                     AND entitlements.until < ?",
                    vec![
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                        text(branch_id),
                        text(tenant_id),
                        SqlValue::Integer(now),
                        SqlValue::Integer(now),
                    ],
                )
                .expect(1),
                Stmt::new(
                    MARK_TIER_DONE,
                    vec![text(branch_id), SqlValue::Integer(now)],
                )
                .expect(1),
            ])
            .await;
        match attempt {
            Ok(true) => return Ok(FreeTierOutcome::Written),
            Ok(false) => {}
            Err(ControlError::Db(m)) if m.contains("UNIQUE") => {}
            Err(e) => return Err(e),
        }
        if let Some(e) = self.entitlement(branch_id).await? {
            if e.tier == Tier::Free && e.source == EntitlementSource::Free {
                self.execute(
                    MARK_TIER_DONE,
                    vec![text(branch_id), SqlValue::Integer(now)],
                )
                .await?;
                return Ok(FreeTierOutcome::AlreadyFree);
            }
            if e.tier_at(now) != Tier::Free {
                return Ok(FreeTierOutcome::OtherPlan(e));
            }
        }
        if self.other_free_branch(tenant_id, branch_id, now).await? {
            return Ok(FreeTierOutcome::OtherFree);
        }
        Ok(FreeTierOutcome::Missing)
    }

    pub async fn grant_comp(&self, g: &CompGrant<'_>) -> Result<GrantOutcome, ControlError> {
        let sql = format!("SELECT {ENTITLEMENT_COLUMNS} FROM entitlements WHERE branch_id = ?");
        let rows = self
            .transact_rows(vec![
                Stmt::new(PREVIOUS_ENTITLEMENT, vec![text(g.branch_id)]).fetch(),
                Stmt::new(
                    "INSERT INTO ops_audit (id, at, operator, action, target, reason, \
                     details_json) SELECT ?, ?, ?, 'grant_comp', b.id, ?, \
                     json_set(?, '$.previous', (SELECT json_object('tier', e.tier, \
                     'source', e.source, 'until', e.until) FROM entitlements e \
                     WHERE e.branch_id = b.id)) FROM branches b WHERE b.id = ? \
                     AND NOT EXISTS (SELECT 1 FROM entitlements p WHERE p.branch_id = b.id \
                     AND p.source = 'paid')",
                    vec![
                        text(g.audit_id),
                        SqlValue::Integer(g.now),
                        text(g.operator),
                        text(g.reason),
                        text(g.details_json),
                        text(g.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "INSERT INTO entitlements (branch_id, tenant_id, tier, source, addons_json, \
                     issued_at, until, updated_at) SELECT id, tenant_id, ?, 'comp', '[]', ?, ?, ? \
                     FROM branches WHERE id = ? ON CONFLICT (branch_id) DO UPDATE SET \
                     tier = excluded.tier, source = 'comp', issued_at = excluded.issued_at, \
                     until = excluded.until, updated_at = excluded.updated_at \
                     WHERE entitlements.source <> 'paid'",
                    vec![
                        text(g.tier.as_str()),
                        SqlValue::Integer(g.now),
                        SqlValue::Integer(g.until),
                        SqlValue::Integer(g.now),
                        text(g.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    MARK_TIER_DONE,
                    vec![text(g.branch_id), SqlValue::Integer(g.now)],
                )
                .expect(1),
            ])
            .await?;
        if let Some(rows) = rows {
            let previous = rows
                .first()
                .and_then(|r| r.first())
                .map(|r| entitlement_from(r))
                .transpose()?;
            return Ok(GrantOutcome::Granted { previous });
        }
        let current = self.query(&sql, vec![text(g.branch_id)]).await?;
        match current.first().map(|r| entitlement_from(r)).transpose()? {
            Some(e) if e.source == EntitlementSource::Paid => Ok(GrantOutcome::Paid(e)),
            _ => Ok(GrantOutcome::UnknownBranch),
        }
    }

    pub async fn ops_audit_for(&self, target: &str) -> Result<Vec<OpsAuditRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, at, operator, action, target, reason, details_json FROM ops_audit \
                 WHERE target = ? ORDER BY at, id",
                vec![text(target)],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(OpsAuditRow {
                    id: col_text(r, 0)?,
                    at: col_int(r, 1)?,
                    operator: col_text(r, 2)?,
                    action: col_text(r, 3)?,
                    target: col_text(r, 4)?,
                    reason: col_text(r, 5)?,
                    details_json: col_text(r, 6)?,
                })
            })
            .collect()
    }
}

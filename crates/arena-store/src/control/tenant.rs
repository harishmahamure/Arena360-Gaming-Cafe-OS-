use arena_proto::{StepState, WizardStep};

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub city: String,
    pub country: String,
    pub currency: String,
    pub timezone: String,
    pub gst_registered: bool,
    pub gstin: Option<String>,
    pub branch_code: String,
    pub code_locked_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchProfile {
    pub name: String,
    pub city: String,
    pub country: String,
    pub currency: String,
    pub timezone: String,
    pub gst_registered: bool,
    pub gstin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTenantBranch<'a> {
    pub tenant_id: &'a str,
    pub owner_id: &'a str,
    pub tenant_name: &'a str,
    pub branch_id: &'a str,
    pub branch_code: &'a str,
    pub profile: &'a BranchProfile,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressRow {
    pub step: WizardStep,
    pub state: StepState,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCodeOutcome {
    Updated,
    Locked,
    Taken,
    Missing,
}

pub(super) const LOCK_BRANCH_CODE: &str =
    "UPDATE branches SET code_locked_at = ? WHERE id = ? AND code_locked_at IS NULL";

const TENANT_COLUMNS: &str = "id, owner_id, name, created_at";

const BRANCH_COLUMNS: &str = "b.id, b.tenant_id, b.name, b.city, b.country, b.currency, \
     b.timezone, b.gst_registered, b.gstin, b.branch_code, b.code_locked_at, b.created_at, \
     b.updated_at";

fn tenant_from(row: &[SqlValue]) -> Result<Tenant, ControlError> {
    Ok(Tenant {
        id: col_text(row, 0)?,
        owner_id: col_text(row, 1)?,
        name: col_text(row, 2)?,
        created_at: col_int(row, 3)?,
    })
}

fn branch_from(row: &[SqlValue]) -> Result<Branch, ControlError> {
    Ok(Branch {
        id: col_text(row, 0)?,
        tenant_id: col_text(row, 1)?,
        name: col_text(row, 2)?,
        city: col_text(row, 3)?,
        country: col_text(row, 4)?,
        currency: col_text(row, 5)?,
        timezone: col_text(row, 6)?,
        gst_registered: col_int(row, 7)? != 0,
        gstin: col_opt_text(row, 8)?,
        branch_code: col_text(row, 9)?,
        code_locked_at: col_opt_int(row, 10)?,
        created_at: col_int(row, 11)?,
        updated_at: col_int(row, 12)?,
    })
}

fn opt_text(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, text)
}

fn progress_rows(branch_id: &str, now: i64) -> Vec<Stmt> {
    WizardStep::ALL
        .into_iter()
        .map(|step| {
            let done = step == WizardStep::Cafe;
            Stmt::new(
                "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
                 VALUES (?, ?, ?, ?)",
                vec![
                    text(branch_id),
                    text(step.as_str()),
                    text(if done {
                        StepState::Done.as_str()
                    } else {
                        StepState::Pending.as_str()
                    }),
                    if done {
                        SqlValue::Integer(now)
                    } else {
                        SqlValue::Null
                    },
                ],
            )
            .expect(1)
        })
        .collect()
}

impl ControlDb {
    pub async fn tenant_of_owner(&self, owner_id: &str) -> Result<Option<Tenant>, ControlError> {
        let sql = format!("SELECT {TENANT_COLUMNS} FROM tenants WHERE owner_id = ?");
        let rows = self.query(&sql, vec![text(owner_id)]).await?;
        rows.first().map(|r| tenant_from(r)).transpose()
    }

    pub async fn branches_of_tenant(&self, tenant_id: &str) -> Result<Vec<Branch>, ControlError> {
        let sql = format!(
            "SELECT {BRANCH_COLUMNS} FROM branches b WHERE b.tenant_id = ? \
             ORDER BY b.created_at, b.id"
        );
        let rows = self.query(&sql, vec![text(tenant_id)]).await?;
        rows.iter().map(|r| branch_from(r)).collect()
    }

    pub async fn branch(&self, branch_id: &str) -> Result<Option<Branch>, ControlError> {
        let sql = format!("SELECT {BRANCH_COLUMNS} FROM branches b WHERE b.id = ?");
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| branch_from(r)).transpose()
    }

    pub async fn owned_branch(
        &self,
        owner_id: &str,
        branch_id: &str,
    ) -> Result<Option<Branch>, ControlError> {
        let sql = format!(
            "SELECT {BRANCH_COLUMNS} FROM branches b JOIN tenants t ON t.id = b.tenant_id \
             WHERE b.id = ? AND t.owner_id = ?"
        );
        let rows = self
            .query(&sql, vec![text(branch_id), text(owner_id)])
            .await?;
        rows.first().map(|r| branch_from(r)).transpose()
    }

    pub async fn onboarding_progress(
        &self,
        branch_id: &str,
    ) -> Result<Vec<ProgressRow>, ControlError> {
        let rows = self
            .query(
                "SELECT step, state, completed_at FROM onboarding_progress WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        let mut out: Vec<ProgressRow> = rows
            .iter()
            .map(|r| {
                let step = col_text(r, 0)?;
                let state = col_text(r, 1)?;
                Ok(ProgressRow {
                    step: WizardStep::parse(&step)
                        .ok_or_else(|| ControlError::Db(format!("unknown step {step:?}")))?,
                    state: StepState::parse(&state)
                        .ok_or_else(|| ControlError::Db(format!("unknown state {state:?}")))?,
                    completed_at: col_opt_int(r, 2)?,
                })
            })
            .collect::<Result<_, ControlError>>()?;
        out.sort_by_key(|p| p.step.number());
        Ok(out)
    }

    pub async fn create_tenant_with_branch(
        &self,
        n: &NewTenantBranch<'_>,
    ) -> Result<bool, ControlError> {
        let p = n.profile;
        let mut stmts = vec![
            Stmt::new(
                "INSERT INTO tenants (id, owner_id, name, created_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (owner_id) DO NOTHING",
                vec![
                    text(n.tenant_id),
                    text(n.owner_id),
                    text(n.tenant_name),
                    SqlValue::Integer(n.now),
                ],
            )
            .expect(1),
            Stmt::new(
                "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \
                 gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
                vec![
                    text(n.branch_id),
                    text(n.tenant_id),
                    text(&p.name),
                    text(&p.city),
                    text(&p.country),
                    text(&p.currency),
                    text(&p.timezone),
                    SqlValue::Integer(i64::from(p.gst_registered)),
                    opt_text(p.gstin.as_deref()),
                    text(n.branch_code),
                    SqlValue::Integer(n.now),
                    SqlValue::Integer(n.now),
                ],
            )
            .expect(1),
        ];
        stmts.extend(progress_rows(n.branch_id, n.now));
        self.transact(stmts).await
    }

    pub async fn create_first_branch(
        &self,
        tenant_id: &str,
        tenant_name: &str,
        branch_id: &str,
        branch_code: &str,
        profile: &BranchProfile,
        now: i64,
    ) -> Result<bool, ControlError> {
        let p = profile;
        let mut stmts = vec![
            Stmt::new(
                "UPDATE tenants SET name = ? WHERE id = ?",
                vec![text(tenant_name), text(tenant_id)],
            )
            .expect(1),
            Stmt::new(
                "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \
                 gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at) \
                 SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ? \
                 WHERE NOT EXISTS (SELECT 1 FROM branches WHERE tenant_id = ?)",
                vec![
                    text(branch_id),
                    text(tenant_id),
                    text(&p.name),
                    text(&p.city),
                    text(&p.country),
                    text(&p.currency),
                    text(&p.timezone),
                    SqlValue::Integer(i64::from(p.gst_registered)),
                    opt_text(p.gstin.as_deref()),
                    text(branch_code),
                    SqlValue::Integer(now),
                    SqlValue::Integer(now),
                    text(tenant_id),
                ],
            )
            .expect(1),
        ];
        stmts.extend(progress_rows(branch_id, now));
        self.transact(stmts).await
    }

    pub async fn update_cafe(
        &self,
        tenant_id: &str,
        tenant_name: &str,
        branch_id: &str,
        profile: &BranchProfile,
        now: i64,
    ) -> Result<bool, ControlError> {
        let p = profile;
        self.transact(vec![
            Stmt::new(
                "UPDATE tenants SET name = ? WHERE id = ?",
                vec![text(tenant_name), text(tenant_id)],
            )
            .expect(1),
            Stmt::new(
                "UPDATE branches SET name = ?, city = ?, country = ?, currency = ?, \
                 timezone = ?, gst_registered = ?, gstin = ?, updated_at = ? \
                 WHERE id = ? AND tenant_id = ?",
                vec![
                    text(&p.name),
                    text(&p.city),
                    text(&p.country),
                    text(&p.currency),
                    text(&p.timezone),
                    SqlValue::Integer(i64::from(p.gst_registered)),
                    opt_text(p.gstin.as_deref()),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(tenant_id),
                ],
            )
            .expect(1),
            Stmt::new(
                "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
                 VALUES (?, 'cafe', 'done', ?) ON CONFLICT (branch_id, step) DO UPDATE SET \
                 state = 'done', completed_at = COALESCE(onboarding_progress.completed_at, \
                 excluded.completed_at)",
                vec![text(branch_id), SqlValue::Integer(now)],
            )
            .expect(1),
        ])
        .await
    }

    pub async fn set_branch_code(
        &self,
        tenant_id: &str,
        branch_id: &str,
        code: &str,
        now: i64,
    ) -> Result<BranchCodeOutcome, ControlError> {
        let n = self
            .execute(
                "UPDATE branches SET branch_code = ?, updated_at = ? \
                 WHERE id = ? AND tenant_id = ? AND code_locked_at IS NULL \
                 AND NOT EXISTS (SELECT 1 FROM branches o WHERE o.tenant_id = ? \
                 AND o.branch_code = ? AND o.id <> ?)",
                vec![
                    text(code),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(tenant_id),
                    text(tenant_id),
                    text(code),
                    text(branch_id),
                ],
            )
            .await?;
        if n == 1 {
            return Ok(BranchCodeOutcome::Updated);
        }
        let rows = self
            .query(
                "SELECT code_locked_at FROM branches WHERE id = ? AND tenant_id = ?",
                vec![text(branch_id), text(tenant_id)],
            )
            .await?;
        match rows.first() {
            None => return Ok(BranchCodeOutcome::Missing),
            Some(r) if col_opt_int(r, 0)?.is_some() => return Ok(BranchCodeOutcome::Locked),
            Some(_) => {}
        }
        let clash = self
            .query(
                "SELECT 1 FROM branches WHERE tenant_id = ? AND branch_code = ? AND id <> ?",
                vec![text(tenant_id), text(code), text(branch_id)],
            )
            .await?;
        if clash.is_empty() {
            return Err(ControlError::Db(
                "branch code update changed no row without a clash".to_owned(),
            ));
        }
        Ok(BranchCodeOutcome::Taken)
    }

    pub async fn lock_branch_code(&self, branch_id: &str, now: i64) -> Result<bool, ControlError> {
        let n = self
            .execute(
                LOCK_BRANCH_CODE,
                vec![SqlValue::Integer(now), text(branch_id)],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn tenant_branch_codes(&self, tenant_id: &str) -> Result<Vec<String>, ControlError> {
        let rows = self
            .query(
                "SELECT branch_code FROM branches WHERE tenant_id = ?",
                vec![text(tenant_id)],
            )
            .await?;
        rows.iter().map(|r| col_text(r, 0)).collect()
    }
}

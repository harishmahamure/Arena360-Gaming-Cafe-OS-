use std::ffi::OsString;

use arena_proto::{Tier, TierLimits};
use arena_store::control::{CompGrant, ControlDb, GrantOutcome};

use crate::config::{Database, GrantCompConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantSummary {
    pub branch_id: String,
    pub branch_name: String,
    pub tenant_id: String,
    pub tier: Tier,
    pub until_date: String,
    pub until_ms: i64,
    pub limits: TierLimits,
    pub audit_id: String,
    pub operator: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsError {
    Config(String),
    Db(String),
    UnknownBranch(String),
    PastDate(String),
    PaidPlan { branch_id: String, tier: Tier },
}

impl std::fmt::Display for OpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(m) => write!(f, "CONFIG_INVALID: {m}"),
            Self::Db(m) => write!(f, "CONTROL_DB_FAILED: {m}"),
            Self::UnknownBranch(id) => write!(f, "UNKNOWN_BRANCH: no branch {id:?} in gz_control"),
            Self::PaidPlan { branch_id, tier } => write!(
                f,
                "PAID_PLAN: branch {branch_id:?} has a paid {} plan; grant-comp never replaces a paid plan",
                tier.as_str()
            ),
            Self::PastDate(d) => write!(
                f,
                "PAST_DATE: --until {d} has already ended in Asia/Kolkata"
            ),
        }
    }
}

impl std::error::Error for OpsError {}

pub async fn open_migrated(database: &Database, now_ms: i64) -> Result<ControlDb, OpsError> {
    let db = match database {
        Database::Remote { url, token } => ControlDb::connect(url, token)
            .await
            .map_err(|e| OpsError::Db(e.to_string()))?,
        Database::Local { data_dir } => {
            std::fs::create_dir_all(data_dir)
                .map_err(|e| OpsError::Db(format!("{}: {e}", data_dir.display())))?;
            ControlDb::open_local(&data_dir.join(crate::config::DEV_DB_FILE))
                .await
                .map_err(|e| OpsError::Db(e.to_string()))?
        }
    };
    db.migrate(now_ms)
        .await
        .map_err(|e| OpsError::Db(e.to_string()))?;
    Ok(db)
}

pub async fn grant_comp(
    cfg: &GrantCompConfig,
    env: impl Fn(&str) -> Option<OsString>,
    now_ms: i64,
) -> Result<GrantSummary, OpsError> {
    let database = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env).map_err(OpsError::Config)?,
    };
    if cfg.until_ms <= now_ms {
        return Err(OpsError::PastDate(cfg.until_date.clone()));
    }
    let db = open_migrated(&database, now_ms).await?;
    let branch = db
        .branch(&cfg.branch_id)
        .await
        .map_err(|e| OpsError::Db(e.to_string()))?
        .ok_or_else(|| OpsError::UnknownBranch(cfg.branch_id.clone()))?;
    let audit_id = ulid::Ulid::generate().to_string();
    let details = serde_json::json!({
        "tier": cfg.tier.as_str(),
        "until": cfg.until_date,
        "until_ms": cfg.until_ms,
        "tenant_id": branch.tenant_id,
    })
    .to_string();
    match db
        .grant_comp(&CompGrant {
            branch_id: &branch.id,
            tier: cfg.tier,
            until: cfg.until_ms,
            audit_id: &audit_id,
            operator: &cfg.operator,
            reason: &cfg.reason,
            details_json: &details,
            now: now_ms,
        })
        .await
        .map_err(|e| OpsError::Db(e.to_string()))?
    {
        GrantOutcome::Granted { .. } => {}
        GrantOutcome::UnknownBranch => {
            return Err(OpsError::UnknownBranch(cfg.branch_id.clone()));
        }
        GrantOutcome::Paid(e) => {
            return Err(OpsError::PaidPlan {
                branch_id: branch.id,
                tier: e.tier,
            });
        }
    }
    Ok(GrantSummary {
        branch_id: branch.id,
        branch_name: branch.name,
        tenant_id: branch.tenant_id,
        tier: cfg.tier,
        until_date: cfg.until_date.clone(),
        until_ms: cfg.until_ms,
        limits: cfg.tier.defaults().limits,
        audit_id,
        operator: cfg.operator.clone(),
    })
}

impl GrantSummary {
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!(
                "granted comp {} to branch {} ({:?}, tenant {}) until the end of {} Asia/Kolkata",
                self.tier.as_str(),
                self.branch_id,
                self.branch_name,
                self.tenant_id,
                self.until_date
            ),
            format!(
                "licence limits: devices {}, users {}, branches {}",
                self.limits.devices, self.limits.users, self.limits.branches
            ),
            format!(
                "ops_audit {} by {}; wizard step tier: done",
                self.audit_id, self.operator
            ),
        ]
    }
}

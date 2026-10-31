use std::ffi::OsString;

use arena_proto::{Tier, TierLimits};
use arena_store::control::{CompGrant, ControlDb, GrantOutcome};

use crate::catalog::{self, ImportError, ImportSummary};
use crate::config::{
    CatalogImportConfig, Database, GrantCompConfig, PriceBookAction, PriceBookConfig,
};
use crate::price_book::{self, BookError};
use crate::templates::TemplateStore;
use crate::webhook::ReviewError;

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
    PaidPlan {
        branch_id: String,
        tier: Tier,
    },
    Book(BookError),
    Refund(crate::refund::DecideError),
    UnknownSubscription(String),
    NotInReview {
        subscription_id: String,
        state: &'static str,
    },
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
            Self::Book(e) => e.fmt(f),
            Self::Refund(e) => e.fmt(f),
            Self::UnknownSubscription(id) => {
                write!(
                    f,
                    "UNKNOWN_SUBSCRIPTION: no subscription {id:?} in gz_control"
                )
            }
            Self::NotInReview {
                subscription_id,
                state,
            } => write!(
                f,
                "NOT_IN_REVIEW: subscription {subscription_id:?} is {state}, not pending_review"
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

/// `catalog import` (Story 6.1): reads the list and its media, uploads what changed once by
/// hash and writes the games, the version and one `admin_audit` row in one transaction.
pub async fn catalog_import(
    cfg: &CatalogImportConfig,
    env: impl Fn(&str) -> Option<OsString>,
    now_ms: i64,
) -> Result<ImportSummary, OpsError> {
    let database = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env).map_err(OpsError::Config)?,
    };
    let file_name = cfg
        .file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| OpsError::Config("the import file name is not text".to_owned()))?
        .to_owned();
    let text = std::fs::read_to_string(&cfg.file)
        .map_err(|e| OpsError::Config(format!("{}: {e}", cfg.file.display())))?;
    let games = catalog::parse_import(&file_name, &text).map_err(OpsError::Config)?;
    let games = catalog::resolve(games, &cfg.media_dir).map_err(OpsError::Config)?;
    let store = TemplateStore::from_env(&env).map_err(OpsError::Config)?;
    let db = open_migrated(&database, now_ms).await?;
    catalog::import(
        &db,
        store.as_ref(),
        &file_name,
        &games,
        &cfg.operator,
        now_ms,
    )
    .await
    .map_err(|e| match e {
        ImportError::File(m) => OpsError::Config(m),
        ImportError::Backend(m) => OpsError::Db(m),
    })
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

/// A limit as the operator reads it: a number, or `unlimited` for a tier that does not cap it.
fn limit_text(limit: u32) -> String {
    if limit == arena_proto::UNLIMITED {
        "unlimited".to_owned()
    } else {
        limit.to_string()
    }
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
                limit_text(self.limits.devices),
                limit_text(self.limits.users),
                limit_text(self.limits.branches)
            ),
            format!(
                "ops_audit {} by {}; wizard step tier: done",
                self.audit_id, self.operator
            ),
        ]
    }
}

/// The operator an ops command is audited under: `--operator`, else the environment's user.
fn operator_of(
    cfg: &PriceBookConfig,
    env: &impl Fn(&str) -> Option<OsString>,
) -> Result<String, OpsError> {
    let from_env = || {
        ["ARENA_OPS_OPERATOR", "USER", "USERNAME"]
            .into_iter()
            .filter_map(|k| crate::config::get_env(env, k))
            .find(|v| !v.is_empty() && v.chars().count() <= 200 && !v.chars().any(char::is_control))
    };
    cfg.operator.clone().or_else(from_env).ok_or_else(|| {
        OpsError::Config(
            "price-book needs --operator (or ARENA_OPS_OPERATOR in the environment)".to_owned(),
        )
    })
}

/// `price-book …` (Story 10.1): the lines to print.
pub async fn price_book(
    cfg: &PriceBookConfig,
    env: impl Fn(&str) -> Option<OsString>,
    now_ms: i64,
) -> Result<Vec<String>, OpsError> {
    let database = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env).map_err(OpsError::Config)?,
    };
    let operator = if cfg.action == PriceBookAction::List {
        String::new()
    } else {
        operator_of(cfg, &env)?
    };
    // The provider is read before the database is touched, so a bad key fails the command at once.
    let provider = match &cfg.action {
        PriceBookAction::Publish { .. } => Some(
            crate::billing::provider_from_env(cfg.dev_data_dir.is_some(), &env)
                .map_err(OpsError::Config)?,
        ),
        _ => None,
    };
    let db = open_migrated(&database, now_ms).await?;
    let book = OpsError::Book;
    match &cfg.action {
        PriceBookAction::List => price_book::list_lines(&db).await.map_err(book),
        PriceBookAction::Draft => {
            let (id, version) = price_book::open_draft(&db, &operator, &cfg.reason, now_ms)
                .await
                .map_err(book)?;
            Ok(vec![format!(
                "opened draft price book {id} (version {version}) as a copy of the live book"
            )])
        }
        PriceBookAction::Set {
            id,
            band,
            item,
            monthly_minor,
        } => {
            price_book::set_amount(
                &db,
                id,
                *band,
                *item,
                *monthly_minor,
                &operator,
                &cfg.reason,
                now_ms,
            )
            .await
            .map_err(book)?;
            Ok(vec![format!(
                "{id}: {} {} is now {monthly_minor} minor units a month, {} a year",
                band.as_str(),
                item.as_str(),
                monthly_minor * 10
            )])
        }
        PriceBookAction::Publish { id } => {
            let provider = provider.expect("publish reads the provider");
            Ok(
                price_book::publish(&db, &provider, id, &operator, &cfg.reason, now_ms)
                    .await
                    .map_err(book)?
                    .lines(),
            )
        }
        PriceBookAction::Band { band, enabled } => {
            price_book::set_band(&db, *band, *enabled, &operator, &cfg.reason, now_ms)
                .await
                .map_err(book)?;
            Ok(vec![format!(
                "band {} is now {}",
                band.as_str(),
                if *enabled { "on sale" } else { "off sale" }
            )])
        }
    }
}

/// `subscription review …` (Story 10.3): the lines to print.
pub async fn subscription_review(
    cfg: &crate::config::SubscriptionReviewConfig,
    env: impl Fn(&str) -> Option<OsString>,
    now_ms: i64,
) -> Result<Vec<String>, OpsError> {
    let database = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env).map_err(OpsError::Config)?,
    };
    let operator = cfg
        .operator
        .clone()
        .or_else(|| {
            ["ARENA_OPS_OPERATOR", "USER", "USERNAME"]
                .into_iter()
                .filter_map(|k| crate::config::get_env(&env, k))
                .find(|v| {
                    !v.is_empty() && v.chars().count() <= 200 && !v.chars().any(char::is_control)
                })
        })
        .ok_or_else(|| {
            OpsError::Config(
                "subscription review needs --operator (or ARENA_OPS_OPERATOR in the environment)"
                    .to_owned(),
            )
        })?;
    let db = open_migrated(&database, now_ms).await?;
    let sub =
        crate::webhook::approve_review(&db, &cfg.subscription_id, &operator, &cfg.reason, now_ms)
            .await
            .map_err(|e| match e {
                ReviewError::Unknown(id) => OpsError::UnknownSubscription(id),
                ReviewError::NotInReview(state) => OpsError::NotInReview {
                    subscription_id: cfg.subscription_id.clone(),
                    state: state.as_str(),
                },
                ReviewError::Busy => {
                    OpsError::Db("the subscription kept changing; run it again".to_owned())
                }
                ReviewError::Db(m) => OpsError::Db(m),
            })?;
    Ok(vec![
        format!(
            "subscription {} of branch {} is {}: {} {} entitled",
            sub.id,
            sub.branch_id,
            sub.state.as_str(),
            sub.tier.as_str(),
            sub.cycle.as_str()
        ),
        format!("ops_audit written for {} by {operator}", sub.id),
    ])
}

/// `refund approve|reject …` (Story 10.15): the lines to print. Approving refunds every payment of
/// the subscription at Razorpay and cancels it; a provider error leaves the request waiting.
pub async fn refund(
    cfg: &crate::config::RefundConfig,
    env: impl Fn(&str) -> Option<OsString>,
    now_ms: i64,
) -> Result<Vec<String>, OpsError> {
    let database = match &cfg.dev_data_dir {
        Some(dir) => Database::Local {
            data_dir: dir.clone(),
        },
        None => crate::config::remote_database(&env).map_err(OpsError::Config)?,
    };
    let operator = cfg
        .operator
        .clone()
        .or_else(|| {
            ["ARENA_OPS_OPERATOR", "USER", "USERNAME"]
                .into_iter()
                .filter_map(|k| crate::config::get_env(&env, k))
                .find(|v| {
                    !v.is_empty() && v.chars().count() <= 200 && !v.chars().any(char::is_control)
                })
        })
        .ok_or_else(|| {
            OpsError::Config(
                "refund needs --operator (or ARENA_OPS_OPERATOR in the environment)".to_owned(),
            )
        })?;
    // The provider is read before the database is touched, so a bad key fails the command at once.
    let provider = if cfg.approve {
        Some(
            crate::billing::provider_from_env(cfg.dev_data_dir.is_some(), &env)
                .map_err(OpsError::Config)?,
        )
    } else {
        None
    };
    let db = open_migrated(&database, now_ms).await?;
    if let Some(provider) = provider {
        let done = crate::refund::approve(
            &db,
            &provider,
            &cfg.request_id,
            &operator,
            &cfg.reason,
            now_ms,
        )
        .await
        .map_err(OpsError::Refund)?;
        let mut lines = vec![format!(
            "refund request {} of subscription {} is {}: {} {} refunded, subscription cancelled at once",
            done.request.id,
            done.request.subscription_id,
            done.request.status,
            done.request.currency,
            done.request.amount_minor,
        )];
        for (payment, amount, refund_id) in &done.refunds {
            lines.push(format!(
                "payment {payment}: refunded {amount} minor units as {refund_id}"
            ));
        }
        lines.push(format!(
            "ops_audit written by {operator}; the credit note follows refund.processed"
        ));
        Ok(lines)
    } else {
        let req = crate::refund::reject(&db, &cfg.request_id, &operator, &cfg.reason, now_ms)
            .await
            .map_err(OpsError::Refund)?;
        Ok(vec![
            format!(
                "refund request {} of subscription {} is {}; the plan stays active",
                req.id, req.subscription_id, req.status
            ),
            format!("ops_audit written by {operator}; the owner reads the reason"),
        ])
    }
}

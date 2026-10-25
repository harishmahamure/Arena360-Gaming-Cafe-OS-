use arena_proto::HubPlatform;

use super::tenant::LOCK_BRANCH_CODE;
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHub<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub platform: HubPlatform,
    pub hub_instance_id: &'a str,
    pub version: &'a str,
    pub token_hash: &'a str,
    pub code_hash: &'a str,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairOutcome {
    Paired { revoked: Vec<String> },
    CodeRefused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beat<'a> {
    pub hub_id: &'a str,
    pub branch_id: &'a str,
    pub epoch: &'a str,
    pub version: &'a str,
    pub schema_version: u32,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubRow {
    pub id: String,
    pub branch_id: String,
    pub platform: HubPlatform,
    pub hub_instance_id: String,
    pub version: String,
    pub token_hash: String,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

const HUB_COLUMNS: &str = "id, branch_id, platform, hub_instance_id, version, token_hash, \
     paired_at, last_seen_at, revoked_at";

fn hub_from(row: &[SqlValue]) -> Result<HubRow, ControlError> {
    let platform = col_text(row, 2)?;
    Ok(HubRow {
        id: col_text(row, 0)?,
        branch_id: col_text(row, 1)?,
        platform: HubPlatform::parse(&platform)
            .ok_or_else(|| ControlError::Db(format!("unknown platform {platform:?}")))?,
        hub_instance_id: col_text(row, 3)?,
        version: col_text(row, 4)?,
        token_hash: col_text(row, 5)?,
        paired_at: col_int(row, 6)?,
        last_seen_at: col_opt_int(row, 7)?,
        revoked_at: col_opt_int(row, 8)?,
    })
}

impl ControlDb {
    pub async fn pair_hub(&self, h: &NewHub<'_>) -> Result<PairOutcome, ControlError> {
        let now = SqlValue::Integer(h.now);
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE pairing_codes SET used_at = ? WHERE code_hash = ? AND branch_id = ? \
                     AND used_at IS NULL AND invalidated_at IS NULL AND expires_at > ? \
                     AND (? <> 'windows' OR COALESCE((SELECT CASE \
                     WHEN e.until IS NOT NULL AND ? > e.until THEN 'free' ELSE e.tier END \
                     FROM entitlements e WHERE e.branch_id = ?), 'free') <> 'free')",
                    vec![
                        now.clone(),
                        text(h.code_hash),
                        text(h.branch_id),
                        now.clone(),
                        text(h.platform.as_str()),
                        now.clone(),
                        text(h.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(LOCK_BRANCH_CODE, vec![now.clone(), text(h.branch_id)]),
                Stmt::new(
                    "DELETE FROM branch_fence WHERE branch_id = ?",
                    vec![text(h.branch_id)],
                ),
                Stmt::new(
                    "UPDATE hubs SET revoked_at = ? WHERE branch_id = ? AND revoked_at IS NULL \
                     RETURNING id",
                    vec![now.clone(), text(h.branch_id)],
                )
                .fetch(),
                Stmt::new(
                    "INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, \
                     token_hash, paired_at, last_seen_at, revoked_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)",
                    vec![
                        text(h.id),
                        text(h.branch_id),
                        text(h.platform.as_str()),
                        text(h.hub_instance_id),
                        text(h.version),
                        text(h.token_hash),
                        now,
                    ],
                )
                .expect(1),
            ])
            .await?;
        let Some(rows) = rows else {
            return Ok(PairOutcome::CodeRefused);
        };
        let revoked = rows
            .get(3)
            .map(|r| r.iter().map(|row| col_text(row, 0)).collect())
            .transpose()?
            .unwrap_or_default();
        Ok(PairOutcome::Paired { revoked })
    }

    pub async fn heartbeat(&self, b: &Beat<'_>) -> Result<Option<String>, ControlError> {
        let now = SqlValue::Integer(b.now);
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE hubs SET last_seen_at = ?, version = ?, schema_version = ? \
                     WHERE id = ? AND revoked_at IS NULL",
                    vec![
                        now.clone(),
                        text(b.version),
                        SqlValue::Integer(i64::from(b.schema_version)),
                        text(b.hub_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "INSERT INTO branch_fence (branch_id, epoch, hub_id, created_at) \
                     VALUES (?, ?, ?, ?) ON CONFLICT (branch_id) DO NOTHING",
                    vec![
                        text(b.branch_id),
                        text(b.epoch),
                        text(b.hub_id),
                        now.clone(),
                    ],
                ),
                Stmt::new(
                    "INSERT INTO onboarding_progress (branch_id, step, state, completed_at) \
                     VALUES (?, 'install_hub', 'done', ?) ON CONFLICT (branch_id, step) \
                     DO UPDATE SET state = 'done', completed_at = COALESCE(\
                     onboarding_progress.completed_at, excluded.completed_at) \
                     WHERE onboarding_progress.state <> 'done'",
                    vec![text(b.branch_id), now],
                ),
                Stmt::new(
                    "SELECT epoch FROM branch_fence WHERE branch_id = ?",
                    vec![text(b.branch_id)],
                )
                .fetch(),
            ])
            .await?;
        rows.and_then(|r| r.get(3).and_then(|x| x.first().map(|row| col_text(row, 0))))
            .transpose()
    }

    pub async fn hub(&self, hub_id: &str) -> Result<Option<HubRow>, ControlError> {
        let sql = format!("SELECT {HUB_COLUMNS} FROM hubs WHERE id = ?");
        let rows = self.query(&sql, vec![text(hub_id)]).await?;
        rows.first().map(|r| hub_from(r)).transpose()
    }

    pub async fn hubs_of_branch(&self, branch_id: &str) -> Result<Vec<HubRow>, ControlError> {
        let sql =
            format!("SELECT {HUB_COLUMNS} FROM hubs WHERE branch_id = ? ORDER BY paired_at, id");
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.iter().map(|r| hub_from(r)).collect()
    }

    pub async fn active_hub(&self, branch_id: &str) -> Result<Option<HubRow>, ControlError> {
        let sql = format!(
            "SELECT {HUB_COLUMNS} FROM hubs WHERE branch_id = ? AND revoked_at IS NULL \
             ORDER BY paired_at DESC, id DESC LIMIT 1"
        );
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| hub_from(r)).transpose()
    }
}

use arena_proto::HubPlatform;

use super::setup_codes::{INSERT_SETUP_CODE, NewSetupCode, insert_params};
use super::tenant::LOCK_BRANCH_CODE;
use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHub<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub platform: HubPlatform,
    pub hub_instance_id: &'a str,
    pub version: &'a str,
    pub token_hash: &'a str,
    pub code_hash: &'a str,
    pub setup: NewSetupCode<'a>,
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
                        now.clone(),
                    ],
                )
                .expect(1),
                Stmt::new(
                    INSERT_SETUP_CODE,
                    insert_params(h.branch_id, &h.setup, h.now),
                ),
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
                Stmt::new(
                    "UPDATE owner_setup_codes SET ciphertext = NULL, nonce = NULL \
                     WHERE branch_id = ? AND ciphertext IS NOT NULL AND created_at <= ?",
                    vec![
                        text(b.branch_id),
                        SqlValue::Integer(b.now - arena_proto::SETUP_CODE_PLAINTEXT_TTL_MS),
                    ],
                ),
            ])
            .await?;
        rows.and_then(|r| r.get(3).and_then(|x| x.first().map(|row| col_text(row, 0))))
            .transpose()
    }

    /// The branch's fence epoch (the first epoch a Hub of the branch reported since it was last
    /// paired), or `None` before any heartbeat.
    pub async fn branch_fence_epoch(
        &self,
        branch_id: &str,
    ) -> Result<Option<String>, ControlError> {
        let rows = self
            .query(
                "SELECT epoch FROM branch_fence WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first().map(|r| col_text(r, 0)).transpose()
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

/// What the cloud remembers of one Hub's change-log upload (Story 8.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HubUploadView {
    /// The epoch and `seq` of the last heartbeat the cloud found complete.
    pub epoch_seen: Option<String>,
    pub seq_seen: Option<i64>,
    /// The Hub's own clock, as its heartbeat reported it.
    pub last_upload_at: Option<i64>,
    pub last_summary_at: Option<i64>,
    pub last_rollup_date: Option<String>,
    /// The last run the cloud asked the Hub to send again, with its epoch.
    pub resend: Option<(String, arena_proto::SeqRange)>,
    /// A run the Hub said it no longer holds.
    pub unavailable: Option<arena_proto::ResendUnavailable>,
    /// Gaps found for this Hub since it paired.
    pub gaps_found: i64,
}

const VIEW_COLUMNS: &str = "last_epoch_seen, last_seq_seen, last_upload_at, last_summary_at, \
     last_rollup_date, resend_epoch, resend_from, resend_to, unavailable_epoch, \
     unavailable_from, unavailable_to, gaps_found";

fn opt_int(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Integer)
}

fn opt_text(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, text)
}

impl ControlDb {
    /// The Hub's cached upload view (empty before its first checked heartbeat).
    pub async fn hub_upload_view(&self, hub_id: &str) -> Result<HubUploadView, ControlError> {
        let sql = format!("SELECT {VIEW_COLUMNS} FROM hubs WHERE id = ?");
        let rows = self.query(&sql, vec![text(hub_id)]).await?;
        let Some(r) = rows.first() else {
            return Ok(HubUploadView::default());
        };
        let resend = match (col_opt_text(r, 5)?, col_opt_int(r, 6)?, col_opt_int(r, 7)?) {
            (Some(epoch), Some(from), Some(to)) => {
                Some((epoch, arena_proto::SeqRange { from, to }))
            }
            _ => None,
        };
        let unavailable = match (col_opt_text(r, 8)?, col_opt_int(r, 9)?, col_opt_int(r, 10)?) {
            (Some(epoch), Some(from), Some(to)) => {
                Some(arena_proto::ResendUnavailable { epoch, from, to })
            }
            _ => None,
        };
        Ok(HubUploadView {
            epoch_seen: col_opt_text(r, 0)?,
            seq_seen: col_opt_int(r, 1)?,
            last_upload_at: col_opt_int(r, 2)?,
            last_summary_at: col_opt_int(r, 3)?,
            last_rollup_date: col_opt_text(r, 4)?,
            resend,
            unavailable,
            gaps_found: col_int(r, 11)?,
        })
    }

    /// Replaces the Hub's cached upload view.
    pub async fn save_hub_upload_view(
        &self,
        hub_id: &str,
        v: &HubUploadView,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE hubs SET last_epoch_seen = ?, last_seq_seen = ?, last_upload_at = ?, \
             last_summary_at = ?, last_rollup_date = ?, resend_epoch = ?, resend_from = ?, \
             resend_to = ?, unavailable_epoch = ?, unavailable_from = ?, unavailable_to = ?, \
             gaps_found = ? WHERE id = ?",
            vec![
                opt_text(v.epoch_seen.as_deref()),
                opt_int(v.seq_seen),
                opt_int(v.last_upload_at),
                opt_int(v.last_summary_at),
                opt_text(v.last_rollup_date.as_deref()),
                opt_text(v.resend.as_ref().map(|(e, _)| e.as_str())),
                opt_int(v.resend.as_ref().map(|(_, r)| r.from)),
                opt_int(v.resend.as_ref().map(|(_, r)| r.to)),
                opt_text(v.unavailable.as_ref().map(|u| u.epoch.as_str())),
                opt_int(v.unavailable.as_ref().map(|u| u.from)),
                opt_int(v.unavailable.as_ref().map(|u| u.to)),
                SqlValue::Integer(v.gaps_found),
                text(hub_id),
            ],
        )
        .await?;
        Ok(())
    }
}

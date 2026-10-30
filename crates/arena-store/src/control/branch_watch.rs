//! The offline watch's state (Story 8.35): the opening hours a paid Hub last sent, the episode
//! keys of the alerts already raised, and the query that lists every paid branch with a live Hub
//! for `arena-cloud --task offline-watch`.

use arena_proto::Tier;

use super::{ControlDb, ControlError, SqlValue, col_opt_int, col_opt_text, col_text, text};

/// The alerts already raised for a branch: the `last_seen_at` of the offline episode and the
/// newest upload time of the not-uploading episode. `None` is no open episode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchState {
    pub offline_key: Option<i64>,
    pub upload_key: Option<i64>,
}

/// One paid branch with a live (not revoked) Hub, as the watch reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRow {
    pub branch_id: String,
    /// The Hub's last heartbeat (cloud time, ms); `None` before its first one.
    pub last_seen_at: Option<i64>,
    /// The Hub's clock when it last had a batch accepted.
    pub last_upload_at: Option<i64>,
    /// The Hub's clock when it last published a summary.
    pub last_summary_at: Option<i64>,
    /// The opening hours JSON and IANA zone the Hub last sent, when it has.
    pub opening: Option<(String, String)>,
    pub state: WatchState,
}

impl WatchRow {
    /// The newer of the last upload and the last summary.
    pub fn newest_upload(&self) -> Option<i64> {
        match (self.last_upload_at, self.last_summary_at) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
}

impl ControlDb {
    /// Stores the opening hours and zone of `branch_id`. Writes nothing when they are unchanged;
    /// answers whether a row was written.
    pub async fn record_branch_opening(
        &self,
        branch_id: &str,
        opening_hours: &str,
        timezone: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "INSERT INTO branch_opening (branch_id, opening_hours, timezone, updated_at) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (branch_id) DO UPDATE SET opening_hours = excluded.opening_hours, \
                    timezone = excluded.timezone, updated_at = excluded.updated_at \
                 WHERE branch_opening.opening_hours <> excluded.opening_hours \
                    OR branch_opening.timezone <> excluded.timezone",
                vec![
                    text(branch_id),
                    text(opening_hours),
                    text(timezone),
                    SqlValue::Integer(now),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// The cached opening hours and zone of `branch_id`.
    pub async fn branch_opening(
        &self,
        branch_id: &str,
    ) -> Result<Option<(String, String)>, ControlError> {
        let rows = self
            .query(
                "SELECT opening_hours, timezone FROM branch_opening WHERE branch_id = ?",
                vec![text(branch_id)],
            )
            .await?;
        rows.first()
            .map(|r| Ok((col_text(r, 0)?, col_text(r, 1)?)))
            .transpose()
    }

    /// Stores the episode keys of `branch_id`.
    pub async fn save_branch_watch(
        &self,
        branch_id: &str,
        state: WatchState,
    ) -> Result<(), ControlError> {
        let opt = |v: Option<i64>| v.map_or(SqlValue::Null, SqlValue::Integer);
        self.execute(
            "INSERT INTO branch_watch (branch_id, offline_key, upload_key) VALUES (?, ?, ?) \
             ON CONFLICT (branch_id) DO UPDATE SET offline_key = excluded.offline_key, \
                upload_key = excluded.upload_key",
            vec![
                text(branch_id),
                opt(state.offline_key),
                opt(state.upload_key),
            ],
        )
        .await?;
        Ok(())
    }

    /// Every branch on a paid tier at `now` (an entitlement past its `until` is Free) that has a
    /// Hub that is not revoked, in branch id order.
    pub async fn watch_rows(&self, now: i64) -> Result<Vec<WatchRow>, ControlError> {
        let rows = self
            .query(
                "SELECT b.id, h.last_seen_at, h.last_upload_at, h.last_summary_at, \
                        o.opening_hours, o.timezone, w.offline_key, w.upload_key, \
                        e.tier, e.until \
                 FROM branches b \
                 JOIN hubs h ON h.branch_id = b.id AND h.revoked_at IS NULL \
                      AND h.role = 'main' \
                 JOIN entitlements e ON e.branch_id = b.id \
                 LEFT JOIN branch_opening o ON o.branch_id = b.id \
                 LEFT JOIN branch_watch w ON w.branch_id = b.id \
                 WHERE e.tier <> 'free' \
                 ORDER BY b.id",
                vec![],
            )
            .await?;
        let mut out = Vec::new();
        for r in &rows {
            let tier = col_text(r, 8)?;
            let tier = Tier::parse(&tier)
                .ok_or_else(|| ControlError::Db(format!("unknown tier {tier:?}")))?;
            let expired = col_opt_int(r, 9)?.is_some_and(|until| now > until);
            if tier == Tier::Free || expired {
                continue;
            }
            let opening = match (col_opt_text(r, 4)?, col_opt_text(r, 5)?) {
                (Some(hours), Some(zone)) => Some((hours, zone)),
                _ => None,
            };
            out.push(WatchRow {
                branch_id: col_text(r, 0)?,
                last_seen_at: col_opt_int(r, 1)?,
                last_upload_at: col_opt_int(r, 2)?,
                last_summary_at: col_opt_int(r, 3)?,
                opening,
                state: WatchState {
                    offline_key: col_opt_int(r, 6)?,
                    upload_key: col_opt_int(r, 7)?,
                },
            });
        }
        Ok(out)
    }
}

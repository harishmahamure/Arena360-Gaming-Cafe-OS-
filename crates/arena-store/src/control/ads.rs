//! Ad metrics (Story 4.14): a Free branch's day-and-slot totals of ad impressions and ad
//! requests, added under a per-branch cursor, and the fill-rate query over them.

use arena_proto::AdCounts;

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_text, text};

/// Which of the two totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdMetric {
    Impressions,
    Requests,
}

impl AdMetric {
    fn kind(self) -> &'static str {
        match self {
            Self::Impressions => "impression",
            Self::Requests => "request",
        }
    }

    fn add_sql(self) -> &'static str {
        match self {
            Self::Impressions => {
                "INSERT INTO ad_impression_daily (branch_id, day, slot, count) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (branch_id, day, slot) DO UPDATE SET count = count + excluded.count"
            }
            Self::Requests => {
                "INSERT INTO ad_request_daily (branch_id, day, slot, count) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (branch_id, day, slot) DO UPDATE SET count = count + excluded.count"
            }
        }
    }
}

/// One day and slot across every branch: no branch, tenant or device in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdFillRow {
    pub day: String,
    pub slot: String,
    pub requests: i64,
    pub impressions: i64,
}

impl AdFillRow {
    /// Impressions divided by requests (`None` with no requests): the NFR27 fill rate.
    pub fn fill_rate(&self) -> Option<f64> {
        (self.requests > 0).then(|| self.impressions as f64 / self.requests as f64)
    }
}

impl ControlDb {
    /// The last Hub row id added to `branch_id`'s totals (0: none).
    pub async fn ad_cursor(&self, branch_id: &str, metric: AdMetric) -> Result<i64, ControlError> {
        let rows = self
            .query(
                "SELECT through_id FROM ad_cursor WHERE branch_id = ? AND kind = ?",
                vec![text(branch_id), text(metric.kind())],
            )
            .await?;
        rows.first().map_or(Ok(0), |r| col_int(r, 0))
    }

    /// Adds `block`'s counts to `branch_id`'s totals and stores its `through_id` as the cursor,
    /// in one transaction, only when `through_id` is beyond the stored cursor and, if the block
    /// names its `after_id`, that is the stored cursor (so a resent range never counts twice).
    /// Answers the stored cursor afterwards: the block's `through_id` when it was added, else
    /// the one the cloud already held, with nothing added.
    pub async fn record_ads(
        &self,
        branch_id: &str,
        metric: AdMetric,
        block: &AdCounts,
    ) -> Result<i64, ControlError> {
        let after = block.after_id.map_or(SqlValue::Null, SqlValue::Integer);
        let mut stmts = vec![
            Stmt::new(
                "INSERT INTO ad_cursor (branch_id, kind, through_id) VALUES (?, ?, 0) \
                 ON CONFLICT (branch_id, kind) DO NOTHING",
                vec![text(branch_id), text(metric.kind())],
            ),
            Stmt::new(
                "UPDATE ad_cursor SET through_id = ? WHERE branch_id = ? AND kind = ? \
                 AND ? > through_id AND (? IS NULL OR through_id = ?)",
                vec![
                    SqlValue::Integer(block.through_id),
                    text(branch_id),
                    text(metric.kind()),
                    SqlValue::Integer(block.through_id),
                    after.clone(),
                    after,
                ],
            )
            .expect(1),
        ];
        for c in &block.counts {
            stmts.push(Stmt::new(
                metric.add_sql(),
                vec![
                    text(branch_id),
                    text(&c.business_day),
                    text(c.slot.as_str()),
                    SqlValue::Integer(c.n),
                ],
            ));
        }
        if self.transact(stmts).await? {
            return Ok(block.through_id);
        }
        self.ad_cursor(branch_id, metric).await
    }

    /// One branch's stored totals for `metric`: `(day, slot, count)`, oldest first.
    pub async fn ad_totals(
        &self,
        branch_id: &str,
        metric: AdMetric,
    ) -> Result<Vec<(String, String, i64)>, ControlError> {
        let rows = self
            .query(
                match metric {
                    AdMetric::Impressions => {
                        "SELECT day, slot, count FROM ad_impression_daily WHERE branch_id = ? ORDER BY day, slot"
                    }
                    AdMetric::Requests => {
                        "SELECT day, slot, count FROM ad_request_daily WHERE branch_id = ? ORDER BY day, slot"
                    }
                },
                vec![text(branch_id)],
            )
            .await?;
        rows.iter()
            .map(|r| Ok((col_text(r, 0)?, col_text(r, 1)?, col_int(r, 2)?)))
            .collect()
    }

    /// Requests and impressions per business day and slot, summed over all branches, for the
    /// days `from..=to` (`YYYY-MM-DD`), oldest first.
    pub async fn ad_fill(&self, from: &str, to: &str) -> Result<Vec<AdFillRow>, ControlError> {
        let rows = self
            .query(
                "SELECT day, slot, SUM(requests), SUM(impressions) FROM (\
                 SELECT day, slot, count AS requests, 0 AS impressions FROM ad_request_daily \
                 UNION ALL \
                 SELECT day, slot, 0, count FROM ad_impression_daily) \
                 WHERE day >= ? AND day <= ? GROUP BY day, slot ORDER BY day, slot",
                vec![text(from), text(to)],
            )
            .await?;
        rows.iter()
            .map(|r| {
                Ok(AdFillRow {
                    day: col_text(r, 0)?,
                    slot: col_text(r, 1)?,
                    requests: col_int(r, 2)?,
                    impressions: col_int(r, 3)?,
                })
            })
            .collect()
    }
}

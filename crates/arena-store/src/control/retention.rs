//! The state of the cloud backup retention sweep (Story 9.15): which branches there are, the tier
//! each is on, and the retention the sweep last saw for it.

use arena_proto::Tier;

use super::{
    ControlDb, ControlError, SqlValue, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// What the sweep last saw of a branch's paid retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionState {
    /// Retention days of the last paid tier seen.
    pub paid_days: u32,
    /// Retention days before the last change of `paid_days` (equal to it when never changed).
    pub prev_days: u32,
    /// When the sweep saw `paid_days` change (ms).
    pub changed_at: i64,
}

/// One branch as the sweep reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionBranch {
    pub tenant_id: String,
    pub branch_id: String,
    /// The tier at the sweep's time (an entitlement past its `until` is Free); Free without one.
    pub tier: Tier,
    pub state: Option<RetentionState>,
}

fn days(row: &[SqlValue], i: usize) -> Result<u32, ControlError> {
    u32::try_from(col_int(row, i)?)
        .map_err(|_| ControlError::Db(format!("column {i}: retention days out of range")))
}

impl ControlDb {
    /// Every branch with its tier at `now` and its saved retention state, in branch id order.
    pub async fn retention_branches(&self, now: i64) -> Result<Vec<RetentionBranch>, ControlError> {
        let rows = self
            .query(
                "SELECT b.tenant_id, b.id, e.tier, e.until, r.paid_days, r.prev_days, \
                        r.changed_at \
                 FROM branches b \
                 LEFT JOIN entitlements e ON e.branch_id = b.id \
                 LEFT JOIN branch_retention r ON r.branch_id = b.id \
                 ORDER BY b.id",
                vec![],
            )
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let tier = match col_opt_text(r, 2)? {
                None => Tier::Free,
                Some(t) => {
                    let tier = Tier::parse(&t)
                        .ok_or_else(|| ControlError::Db(format!("unknown tier {t:?}")))?;
                    if col_opt_int(r, 3)?.is_some_and(|until| now > until) {
                        Tier::Free
                    } else {
                        tier
                    }
                }
            };
            let state = match col_opt_int(r, 4)? {
                None => None,
                Some(_) => Some(RetentionState {
                    paid_days: days(r, 4)?,
                    prev_days: days(r, 5)?,
                    changed_at: col_int(r, 6)?,
                }),
            };
            out.push(RetentionBranch {
                tenant_id: col_text(r, 0)?,
                branch_id: col_text(r, 1)?,
                tier,
                state,
            });
        }
        Ok(out)
    }

    /// Stores the retention state of `branch_id`.
    pub async fn save_branch_retention(
        &self,
        branch_id: &str,
        state: RetentionState,
    ) -> Result<(), ControlError> {
        self.execute(
            "INSERT INTO branch_retention (branch_id, paid_days, prev_days, changed_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT (branch_id) DO UPDATE SET paid_days = excluded.paid_days, \
                prev_days = excluded.prev_days, changed_at = excluded.changed_at",
            vec![
                text(branch_id),
                SqlValue::Integer(i64::from(state.paid_days)),
                SqlValue::Integer(i64::from(state.prev_days)),
                SqlValue::Integer(state.changed_at),
            ],
        )
        .await?;
        Ok(())
    }
}

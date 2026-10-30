//! The audit builder's job table (Story 8.36, AD-37): `builder_jobs`, one row per branch and
//! month. The control role writes a `queued` row when an owner (or, from Story 8.37, a support
//! agent) asks for a month; the builder role claims queued rows, moves their progress and marks
//! them `ready` or `failed`. A claim is a compare-and-set on the row's `updated_at`, so two
//! builders never take the same job; a `building` row nobody has touched for the stale window
//! belongs to a builder that died and is claimed again.

use arena_proto::AuditBuildState;

use super::{
    ControlDb, ControlError, SqlValue, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// One build job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderJob {
    pub branch_id: String,
    /// `yyyy-mm`.
    pub month: String,
    pub state: AuditBuildState,
    /// 0 to 100.
    pub progress: u8,
    /// The R2 key of the built month, once `ready`.
    pub key: Option<String>,
    pub built_at: Option<i64>,
    /// Why the last attempt failed.
    pub error: Option<String>,
    pub attempt: i64,
    pub requested_by: String,
    pub reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "branch_id, month, state, progress, key, built_at, error, attempt, \
                       requested_by, reason, created_at, updated_at";

fn job_from(r: &[SqlValue]) -> Result<BuilderJob, ControlError> {
    let state = col_text(r, 2)?;
    let state = AuditBuildState::parse(&state)
        .ok_or_else(|| ControlError::Db(format!("unknown builder job state {state:?}")))?;
    Ok(BuilderJob {
        branch_id: col_text(r, 0)?,
        month: col_text(r, 1)?,
        state,
        progress: col_int(r, 3)?.clamp(0, 100) as u8,
        key: col_opt_text(r, 4)?,
        built_at: col_opt_int(r, 5)?,
        error: col_opt_text(r, 6)?,
        attempt: col_int(r, 7)?,
        requested_by: col_text(r, 8)?,
        reason: col_opt_text(r, 9)?,
        created_at: col_int(r, 10)?,
        updated_at: col_int(r, 11)?,
    })
}

impl ControlDb {
    /// The job for `branch_id` and `month`.
    pub async fn builder_job(
        &self,
        branch_id: &str,
        month: &str,
    ) -> Result<Option<BuilderJob>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM builder_jobs WHERE branch_id = ? AND month = ?");
        let rows = self.query(&sql, vec![text(branch_id), text(month)]).await?;
        rows.first().map(|r| job_from(r)).transpose()
    }

    /// Queues a build of `month` unless one is already usable: a `queued` or live `building`
    /// job, or a `ready` one built after `ready_after`. A `failed` job, a `ready` one built at
    /// or before `ready_after` and a `building` one untouched since `stale_before` start a new
    /// attempt (the row is reset to `queued`, progress 0). Answers whether a job was queued.
    #[allow(clippy::too_many_arguments)]
    pub async fn queue_builder_job(
        &self,
        branch_id: &str,
        month: &str,
        requested_by: &str,
        reason: Option<&str>,
        now: i64,
        ready_after: i64,
        stale_before: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "INSERT INTO builder_jobs (branch_id, month, state, progress, attempt, \
                    requested_by, reason, created_at, updated_at) \
                 VALUES (?1, ?2, 'queued', 0, 1, ?3, ?4, ?5, ?5) \
                 ON CONFLICT (branch_id, month) DO UPDATE SET state = 'queued', progress = 0, \
                    key = NULL, built_at = NULL, error = NULL, attempt = attempt + 1, \
                    requested_by = excluded.requested_by, reason = excluded.reason, \
                    created_at = excluded.created_at, updated_at = excluded.updated_at \
                 WHERE builder_jobs.state = 'failed' \
                    OR (builder_jobs.state = 'ready' AND builder_jobs.built_at <= ?6) \
                    OR (builder_jobs.state = 'building' AND builder_jobs.updated_at <= ?7)",
                vec![
                    text(branch_id),
                    text(month),
                    text(requested_by),
                    reason.map_or(SqlValue::Null, text),
                    SqlValue::Integer(now),
                    SqlValue::Integer(ready_after),
                    SqlValue::Integer(stale_before),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// Takes the oldest job (its `attempt` goes up by one, which fences a builder that died) that is `queued` or `building` and untouched since `stale_before`,
    /// marking it `building` with progress 0 and counting a new attempt (which fences the writes
    /// of a builder that died). `None` when there is none, or another builder won the race.
    pub async fn claim_builder_job(
        &self,
        now: i64,
        stale_before: i64,
    ) -> Result<Option<BuilderJob>, ControlError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM builder_jobs \
             WHERE state = 'queued' OR (state = 'building' AND updated_at <= ?) \
             ORDER BY created_at, branch_id, month LIMIT 1"
        );
        let rows = self
            .query(&sql, vec![SqlValue::Integer(stale_before)])
            .await?;
        let Some(job) = rows.first().map(|r| job_from(r)).transpose()? else {
            return Ok(None);
        };
        let n = self
            .execute(
                "UPDATE builder_jobs SET state = 'building', progress = 0, \
                    attempt = attempt + 1, updated_at = ? \
                 WHERE branch_id = ? AND month = ? AND updated_at = ? \
                    AND state IN ('queued', 'building')",
                vec![
                    SqlValue::Integer(now),
                    text(&job.branch_id),
                    text(&job.month),
                    SqlValue::Integer(job.updated_at),
                ],
            )
            .await?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(BuilderJob {
            state: AuditBuildState::Building,
            progress: 0,
            attempt: job.attempt + 1,
            updated_at: now,
            ..job
        }))
    }

    /// Records the progress (0 to 100) of a `building` job. Answers whether the job was still
    /// `building` and untouched by another builder since `attempt` started.
    pub async fn set_builder_progress(
        &self,
        branch_id: &str,
        month: &str,
        attempt: i64,
        progress: u8,
        now: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE builder_jobs SET progress = ?, updated_at = ? \
                 WHERE branch_id = ? AND month = ? AND attempt = ? AND state = 'building'",
                vec![
                    SqlValue::Integer(i64::from(progress.min(100))),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(month),
                    SqlValue::Integer(attempt),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// Marks a `building` job `ready` with its R2 `key`.
    pub async fn finish_builder_job(
        &self,
        branch_id: &str,
        month: &str,
        attempt: i64,
        key: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE builder_jobs SET state = 'ready', progress = 100, key = ?, built_at = ?, \
                    error = NULL, updated_at = ? \
                 WHERE branch_id = ? AND month = ? AND attempt = ? AND state = 'building'",
                vec![
                    text(key),
                    SqlValue::Integer(now),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(month),
                    SqlValue::Integer(attempt),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// Marks a `building` job `failed` with the reason.
    pub async fn fail_builder_job(
        &self,
        branch_id: &str,
        month: &str,
        attempt: i64,
        error: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE builder_jobs SET state = 'failed', error = ?, updated_at = ? \
                 WHERE branch_id = ? AND month = ? AND attempt = ? AND state = 'building'",
                vec![
                    text(error),
                    SqlValue::Integer(now),
                    text(branch_id),
                    text(month),
                    SqlValue::Integer(attempt),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// The `ready` months built at or before `built_before` whose cache object still exists
    /// (`key` set): `(branch_id, month, key)`.
    pub async fn expired_builder_caches(
        &self,
        built_before: i64,
    ) -> Result<Vec<(String, String, String)>, ControlError> {
        let rows = self
            .query(
                "SELECT branch_id, month, key FROM builder_jobs \
                 WHERE state = 'ready' AND key IS NOT NULL AND built_at <= ? \
                 ORDER BY built_at LIMIT 100",
                vec![SqlValue::Integer(built_before)],
            )
            .await?;
        rows.iter()
            .map(|r| Ok((col_text(r, 0)?, col_text(r, 1)?, col_text(r, 2)?)))
            .collect()
    }

    /// Forgets the cache object of a `ready` month built at or before `built_before`, so the
    /// caller may delete it. Answers whether the row was still that old build.
    pub async fn clear_builder_cache(
        &self,
        branch_id: &str,
        month: &str,
        built_before: i64,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE builder_jobs SET key = NULL \
                 WHERE branch_id = ? AND month = ? AND state = 'ready' AND key IS NOT NULL \
                    AND built_at <= ?",
                vec![
                    text(branch_id),
                    text(month),
                    SqlValue::Integer(built_before),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// Jobs waiting for a builder (`queued`): the "builder queue depth" metric.
    pub async fn builder_queue_depth(&self) -> Result<i64, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM builder_jobs WHERE state = 'queued'",
                vec![],
            )
            .await?;
        rows.first().map_or(Ok(0), |r| col_int(r, 0))
    }
}

//! What the cloud makes of a branch's rollup files (Story 8.7, AD-35): it reads a file only when
//! its `schema` is the newest one it knows (N) or the one before (N−1), and a day that is not
//! there, or is older, counts as missing, with `ROLLUP_SCHEMA_OLD` recorded for that branch and
//! day. The "rollup missing-day count" per branch is the metric the daily pass exports.
//!
//! The cloud never holds a copy of a branch's rows; this keeps only which days it has seen at an
//! unreadable schema, in process, and logs the metric like the heartbeat gaps do
//! ([`crate::gaps::GapMetrics`]); the OpenTelemetry export is deferred with the Story 2.3
//! exporter. Reading the files themselves is the owner read API (Story 8.9) and the jobs pass
//! (Story 11.31), which call [`RollupMetrics::record_file`] for every file they open.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use arena_proto::{
    AppError, ErrorCode, TIERS, is_rollup_date, rollup_object_key, rollup_schema_accepted,
};

/// The newest rollup schema the cloud knows: the highest any tier pins in its licence. It
/// rises with the cloud's pins, never with a Hub release, so a Hub that adds a report field
/// needs no cloud deploy until a tier pins the new schema.
pub fn current_rollup_schema() -> u32 {
    TIERS.iter().map(|t| t.rollup_schema).max().unwrap_or(1)
}

/// `Ok` when a file of `schema` is read (N or N−1), `ROLLUP_SCHEMA_OLD` otherwise.
pub fn check_rollup_schema(schema: u32, current: u32) -> Result<(), AppError> {
    if rollup_schema_accepted(schema, current) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::RollupSchemaOld,
            "This day's report is being rebuilt.",
        ))
    }
}

/// The business days a bucket listing holds a rollup for. `keys` are object keys under the
/// branch's prefix (`rollups/<yyyy-mm-dd>.json`); anything else, and `latest`, is ignored.
pub fn rollup_days(keys: &[String]) -> BTreeSet<String> {
    keys.iter()
        .filter_map(|k| {
            let date = k.strip_prefix("rollups/")?.strip_suffix(".json")?;
            (is_rollup_date(date) && rollup_object_key(date) == *k).then(|| date.to_owned())
        })
        .collect()
}

/// What the cloud has seen of one branch's rollup files.
#[derive(Debug, Default)]
struct BranchDays {
    /// Days read at an accepted schema.
    readable: BTreeSet<String>,
    /// Days seen at a schema the cloud does not read, with that schema.
    old: BTreeMap<String, u32>,
}

/// The "rollup missing-day count" metric per tenant and branch, kept in process and logged.
#[derive(Debug, Default)]
pub struct RollupMetrics {
    seen: Mutex<BTreeMap<(String, String), BranchDays>>,
}

impl RollupMetrics {
    fn with<R>(&self, tenant_id: &str, branch_id: &str, f: impl FnOnce(&mut BranchDays) -> R) -> R {
        let mut map = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        f(map
            .entry((tenant_id.to_owned(), branch_id.to_owned()))
            .or_default())
    }

    /// Notes that the file of `day` carries `schema`, under the cloud's `current`. A readable
    /// file clears any earlier `ROLLUP_SCHEMA_OLD` for the day; an unreadable one records it and
    /// answers the error, so the caller treats the day as missing.
    pub fn record_file(
        &self,
        tenant_id: &str,
        branch_id: &str,
        day: &str,
        schema: u32,
        current: u32,
    ) -> Result<(), AppError> {
        let verdict = check_rollup_schema(schema, current);
        let fresh = self.with(tenant_id, branch_id, |b| match &verdict {
            Ok(()) => {
                b.old.remove(day);
                b.readable.insert(day.to_owned());
                false
            }
            Err(_) => {
                b.readable.remove(day);
                b.old.insert(day.to_owned(), schema) != Some(schema)
            }
        });
        if fresh {
            tracing::warn!(
                tenant_id,
                branch_id,
                day,
                schema,
                current,
                code = ErrorCode::RollupSchemaOld.as_str(),
                "a rollup file is at a schema the cloud no longer reads"
            );
        }
        verdict
    }

    /// The days `ROLLUP_SCHEMA_OLD` is recorded for, oldest first.
    pub fn old_days(&self, tenant_id: &str, branch_id: &str) -> Vec<String> {
        self.with(tenant_id, branch_id, |b| b.old.keys().cloned().collect())
    }

    /// How many of the `expected` business days have no readable rollup: not in the bucket
    /// listing (`held`), or recorded as `ROLLUP_SCHEMA_OLD`. A held day whose file has not been
    /// opened yet counts as present.
    pub fn missing_days(
        &self,
        tenant_id: &str,
        branch_id: &str,
        expected: &BTreeSet<String>,
        held: &BTreeSet<String>,
    ) -> usize {
        self.with(tenant_id, branch_id, |b| {
            expected
                .iter()
                .filter(|d| !held.contains(*d) || b.old.contains_key(*d))
                .count()
        })
    }

    /// The daily pass for one branch: counts the missing days and exports the figure.
    pub fn export_missing(
        &self,
        tenant_id: &str,
        branch_id: &str,
        expected: &BTreeSet<String>,
        held: &BTreeSet<String>,
    ) -> usize {
        let missing = self.missing_days(tenant_id, branch_id, expected, held);
        tracing::info!(
            tenant_id,
            branch_id,
            rollup_missing_days = missing,
            "rollup missing-day count"
        );
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(days: &[&str]) -> BTreeSet<String> {
        days.iter().map(|d| (*d).to_owned()).collect()
    }

    #[test]
    fn the_cloud_reads_n_and_n_minus_one_and_calls_the_rest_old() {
        assert!(check_rollup_schema(3, 3).is_ok());
        assert!(check_rollup_schema(2, 3).is_ok());
        for old in [1, 4, 0] {
            let e = check_rollup_schema(old, 3).unwrap_err();
            assert_eq!(e.code, ErrorCode::RollupSchemaOld, "schema {old}");
        }
        assert_eq!(current_rollup_schema(), 1);
        assert!(check_rollup_schema(1, current_rollup_schema()).is_ok());
    }

    #[test]
    fn the_days_a_listing_holds_are_the_dated_rollup_keys_only() {
        let keys: Vec<String> = [
            "rollups/2026-10-28.json",
            "rollups/2026-10-29.json",
            "rollups/latest.json",
            "rollups/2026-13-01.json",
            "rollups/2026-10-30.json.tmp",
            "state/latest.json",
            "rollups/sub/2026-10-27.json",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(rollup_days(&keys), set(&["2026-10-28", "2026-10-29"]));
    }

    #[test]
    fn an_old_day_is_recorded_per_branch_and_day_and_counts_as_missing() {
        let m = RollupMetrics::default();
        assert!(m.record_file("T", "B", "2026-10-27", 3, 3).is_ok());
        assert!(m.record_file("T", "B", "2026-10-28", 2, 3).is_ok());
        let e = m.record_file("T", "B", "2026-10-26", 1, 3).unwrap_err();
        assert_eq!(e.code, ErrorCode::RollupSchemaOld);
        assert_eq!(m.old_days("T", "B"), ["2026-10-26"]);
        assert!(
            m.old_days("T", "OTHER").is_empty(),
            "recorded for that branch only"
        );

        let expected = set(&["2026-10-26", "2026-10-27", "2026-10-28", "2026-10-29"]);
        // The 29th is not in the bucket; the 26th is, but at an old schema.
        let held = set(&["2026-10-26", "2026-10-27", "2026-10-28"]);
        assert_eq!(m.missing_days("T", "B", &expected, &held), 2);
        assert_eq!(m.export_missing("T", "B", &expected, &held), 2);
        assert_eq!(m.missing_days("T", "OTHER", &expected, &held), 1);

        // The Hub regenerated the 26th: it reads again and the metric drops.
        assert!(m.record_file("T", "B", "2026-10-26", 3, 3).is_ok());
        assert!(m.old_days("T", "B").is_empty());
        assert_eq!(m.missing_days("T", "B", &expected, &held), 1);
    }
}

//! The "summary age per branch" metric (Story 8.13): how old a branch's figures were each time
//! the owner opened Today, kept in process per tenant and branch and logged. The 99th percentile
//! is over the last [`WINDOW`] opens. The OpenTelemetry/Grafana export is deferred with the
//! Story 2.3 exporter.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use arena_proto::Tier;

/// Opens kept per branch for the percentile.
const WINDOW: usize = 200;

/// What a branch's opens look like so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataAgeStats {
    pub opens: u64,
    pub last_s: u64,
    pub p99_s: u64,
}

#[derive(Debug, Default)]
struct Samples {
    recent: VecDeque<u64>,
    opens: u64,
}

#[derive(Debug, Default)]
pub struct DataAgeMetrics {
    seen: Mutex<BTreeMap<(String, String), Samples>>,
}

/// The value at the 99th percentile (nearest rank) of `ages`, 0 when empty.
fn p99(ages: &VecDeque<u64>) -> u64 {
    let mut sorted: Vec<u64> = ages.iter().copied().collect();
    sorted.sort_unstable();
    let rank = (sorted.len() * 99).div_ceil(100);
    rank.checked_sub(1)
        .and_then(|i| sorted.get(i))
        .copied()
        .unwrap_or(0)
}

impl DataAgeMetrics {
    /// Records one open of `age_s` seconds under `tier` and logs the branch's figures.
    pub fn record(&self, tenant_id: &str, branch_id: &str, tier: Tier, age_s: u64) -> DataAgeStats {
        let stats = {
            let mut map = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            let s = map
                .entry((tenant_id.to_owned(), branch_id.to_owned()))
                .or_default();
            s.opens += 1;
            if s.recent.len() == WINDOW {
                s.recent.pop_front();
            }
            s.recent.push_back(age_s);
            DataAgeStats {
                opens: s.opens,
                last_s: age_s,
                p99_s: p99(&s.recent),
            }
        };
        tracing::info!(
            tenant_id,
            branch_id,
            tier = tier.as_str(),
            summary_age_s = age_s,
            summary_age_p99_s = stats.p99_s,
            "owner opened Today"
        );
        stats
    }

    /// The branch's figures so far.
    pub fn stats(&self, tenant_id: &str, branch_id: &str) -> DataAgeStats {
        let map = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        map.get(&(tenant_id.to_owned(), branch_id.to_owned()))
            .map(|s| DataAgeStats {
                opens: s.opens,
                last_s: s.recent.back().copied().unwrap_or(0),
                p99_s: p99(&s.recent),
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_p99_follows_the_slowest_opens() {
        let m = DataAgeMetrics::default();
        for age in 1..=100 {
            m.record("t", "b", Tier::Basic, age);
        }
        let s = m.stats("t", "b");
        assert_eq!((s.opens, s.last_s, s.p99_s), (100, 100, 99));
        assert_eq!(m.stats("t", "other"), DataAgeStats::default());
    }

    #[test]
    fn only_the_last_window_counts() {
        let m = DataAgeMetrics::default();
        m.record("t", "b", Tier::Premium, 10_000);
        for _ in 0..WINDOW {
            m.record("t", "b", Tier::Premium, 5);
        }
        assert_eq!(m.stats("t", "b").p99_s, 5);
    }
}

//! Upload gap detection (Story 8.3, AD-4): finds the first run of change-log `seq`s a Hub says it
//! uploaded that the data bucket does not hold. There is no manifest: the heartbeat names how far
//! the Hub got, the bucket's own `changes/<epoch>/` listing says what landed, and the difference
//! is asked for again.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use arena_proto::SeqRange;

/// `(first, last)` of a batch key tail `<first:012>-<last:012>.zst`; `None` for any other name.
pub fn parse_batch_key(name: &str) -> Option<(i64, i64)> {
    let stem = name.strip_suffix(".zst")?;
    let (first, last) = stem.split_once('-')?;
    let digits = |s: &str| s.len() == 12 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(first) || !digits(last) {
        return None;
    }
    let (first, last) = (first.parse::<i64>().ok()?, last.parse::<i64>().ok()?);
    (first <= last).then_some((first, last))
}

/// The first run inside `base..=through` that no range of `held` covers, or `None` when the
/// whole span is held (or empty). Ranges may overlap and arrive in any order.
pub fn first_missing(mut held: Vec<(i64, i64)>, base: i64, through: i64) -> Option<SeqRange> {
    held.sort_unstable();
    let mut next = base;
    for (first, last) in held {
        if next > through {
            return None;
        }
        if first > next {
            return Some(SeqRange {
                from: next,
                to: (first - 1).min(through),
            });
        }
        next = next.max(last.saturating_add(1));
    }
    (next <= through).then_some(SeqRange {
        from: next,
        to: through,
    })
}

/// Gaps seen for one branch.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GapCounts {
    /// Distinct gaps the cloud found and asked the Hub to send again.
    pub found: u64,
    /// Gaps the Hub could not fill because it had pruned the rows.
    pub unavailable: u64,
}

/// The "heartbeat gaps" metric per tenant and branch, kept in process and logged. The
/// OpenTelemetry/Grafana export is deferred with the Story 2.3 exporter.
#[derive(Debug, Default)]
pub struct GapMetrics {
    counts: Mutex<HashMap<(String, String), GapCounts>>,
}

impl GapMetrics {
    fn with<R>(&self, tenant_id: &str, branch_id: &str, f: impl FnOnce(&mut GapCounts) -> R) -> R {
        let mut map = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        f(map
            .entry((tenant_id.to_owned(), branch_id.to_owned()))
            .or_default())
    }

    /// Counts a gap found at `range` of `epoch`.
    pub fn found(&self, tenant_id: &str, branch_id: &str, epoch: &str, range: SeqRange) {
        let c = self.with(tenant_id, branch_id, |c| {
            c.found += 1;
            *c
        });
        tracing::warn!(
            tenant_id,
            branch_id,
            epoch,
            from = range.from,
            to = range.to,
            heartbeat_gaps = c.found,
            "heartbeat found a gap in the uploaded change log"
        );
    }

    /// Counts a gap the Hub reported it can no longer fill.
    pub fn unavailable(&self, tenant_id: &str, branch_id: &str, epoch: &str, range: SeqRange) {
        let c = self.with(tenant_id, branch_id, |c| {
            c.unavailable += 1;
            *c
        });
        tracing::warn!(
            tenant_id,
            branch_id,
            epoch,
            from = range.from,
            to = range.to,
            heartbeat_gaps_unavailable = c.unavailable,
            "a gap in the uploaded change log cannot be resent"
        );
    }

    /// The branch's counts so far.
    pub fn counts(&self, tenant_id: &str, branch_id: &str) -> GapCounts {
        self.with(tenant_id, branch_id, |c| *c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(from: i64, to: i64) -> Option<SeqRange> {
        Some(SeqRange { from, to })
    }

    #[test]
    fn a_batch_key_tail_is_two_twelve_digit_numbers() {
        assert_eq!(
            parse_batch_key("000000000001-000000005000.zst"),
            Some((1, 5000))
        );
        for bad in [
            "1-5000.zst",
            "000000000001-000000005000",
            "000000000001-000000005000.zst.tmp",
            "000000005000-000000000001.zst",
            "00000000000a-000000005000.zst",
            "",
        ] {
            assert_eq!(parse_batch_key(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nothing_is_missing_when_the_batches_cover_the_span() {
        assert_eq!(first_missing(vec![(1, 5000), (5001, 7000)], 1, 7000), None);
        assert_eq!(first_missing(vec![(1, 9000)], 1, 7000), None);
        assert_eq!(first_missing(vec![], 1, 0), None);
        assert_eq!(first_missing(vec![], 41, 40), None);
    }

    #[test]
    fn the_first_hole_is_found_wherever_it_is() {
        let held = vec![(10_001, 12_000), (1, 5000)];
        assert_eq!(first_missing(held, 1, 12_000), range(5001, 10_000));
        assert_eq!(first_missing(vec![(1, 5000)], 1, 7000), range(5001, 7000));
        assert_eq!(first_missing(vec![(5001, 6000)], 1, 6000), range(1, 5000));
        assert_eq!(first_missing(vec![], 1, 10), range(1, 10));
    }

    #[test]
    fn a_hole_is_cut_at_what_the_hub_says_it_uploaded() {
        assert_eq!(
            first_missing(vec![(1, 100), (900, 1000)], 1, 500),
            range(101, 500)
        );
    }

    #[test]
    fn overlapping_and_repeated_batches_do_not_hide_or_invent_holes() {
        assert_eq!(
            first_missing(vec![(1, 100), (50, 150), (1, 100)], 1, 150),
            None
        );
        assert_eq!(
            first_missing(vec![(1, 100), (50, 150), (160, 200)], 1, 200),
            range(151, 159)
        );
    }

    #[test]
    fn a_fork_starts_after_its_parent() {
        assert_eq!(first_missing(vec![(41, 100)], 41, 100), None);
        assert_eq!(first_missing(vec![(61, 100)], 41, 100), range(41, 60));
    }
}

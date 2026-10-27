use arena_proto::{AppError, BillLine, BillingRules};
use arena_store::{SegmentRow, StoreTx, list_settings};

pub fn read(tx: &StoreTx<'_>) -> Result<BillingRules, AppError> {
    let rows = list_settings(tx)?;
    Ok(BillingRules::from_lookup(|key| {
        rows.iter().find(|r| r.key == key).map(|r| r.value.clone())
    }))
}

const MIN_MS: i64 = 60_000;

pub fn bill(
    segments: &[SegmentRow],
    rounding_min: u32,
    minimum_min: u32,
    paused_since: Option<i64>,
    now_ms: i64,
) -> Vec<BillLine> {
    let last = segments.len().saturating_sub(1);
    let open_pause = paused_since.map_or(0, |at| {
        let from = segments.last().map_or(at, |s| s.from_at);
        (now_ms - at.max(from)).max(0)
    });
    let mut minutes: Vec<i64> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let open = if i == last { open_pause } else { 0 };
            let ms = (s.to_at.unwrap_or(now_ms) - s.from_at - s.paused_ms - open).max(0);
            (ms + MIN_MS - 1) / MIN_MS
        })
        .collect();
    if let Some(first) = minutes.first_mut() {
        *first = (*first).max(i64::from(minimum_min));
    }
    let unit = i64::from(rounding_min);
    let total: i64 = minutes.iter().sum();
    if unit > 1
        && let Some(end) = minutes.last_mut()
    {
        *end += (unit - total % unit) % unit;
    }
    segments
        .iter()
        .zip(minutes)
        .map(|(s, m)| BillLine {
            device_id: s.device_id.clone(),
            from_at: s.from_at,
            to_at: s.to_at.unwrap_or(now_ms),
            minutes: m,
            rate_minor_per_hour: s.rate_minor_per_hour,
            amount_minor: (m * s.rate_minor_per_hour + 30) / 60,
            rule_name: None,
        })
        .collect()
}

/// The longest a wallet session is ever sized to, in minutes (one year).
const HORIZON_CAP_MIN: i128 = 366 * 24 * 60;

/// How long a balance plays on a wallet session: the whole minutes of play and the bill they cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletHorizon {
    pub minutes: i64,
    pub cost_minor: i64,
}

/// The last whole-rounding-unit moment at which [`bill`] on a single segment at
/// `rate_minor_per_hour` still costs no more than `available_minor`, or `None` when the balance
/// cannot cover the first (minimum) bill. The cost is the half-up amount of those minutes.
pub fn wallet_horizon(
    rate_minor_per_hour: i64,
    rounding_min: u32,
    minimum_min: u32,
    available_minor: i64,
) -> Option<WalletHorizon> {
    let unit = i128::from(rounding_min.max(1));
    let rate = i128::from(rate_minor_per_hour.max(0));
    let cost = |minutes: i128| (minutes * rate + 30) / 60;
    let reachable = if rate == 0 {
        HORIZON_CAP_MIN
    } else {
        ((i128::from(available_minor.max(0)) * 60 + 29) / rate).min(HORIZON_CAP_MIN)
    };
    let minutes = reachable / unit * unit;
    let first = ((i128::from(minimum_min) + unit - 1) / unit).max(1) * unit;
    if minutes < first || cost(minutes) > i128::from(available_minor) {
        return None;
    }
    Some(WalletHorizon {
        minutes: i64::try_from(minutes).ok()?,
        cost_minor: i64::try_from(cost(minutes)).ok()?,
    })
}

/// A wallet session's end and set-aside amount once a rate change has split its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletFlip {
    pub ends_at: i64,
    pub reserved_minor: i64,
}

/// Re-sizes a wallet session at a rate change: `closed` are its segments up to `flip_at`, and a
/// new segment at `rate_minor_per_hour` opens there. The new segment runs the largest whole
/// minutes `k` for which [`bill`] of all the segments stays within `reserved_minor`, rounded down
/// so the total minutes are a whole rounding unit, and never fewer than none: the session ends at
/// `flip_at + k` minutes, not before the flip. The set-aside becomes that bill (never more than it
/// was).
pub fn wallet_end_after_flip(
    closed: &[SegmentRow],
    flip_at: i64,
    rate_minor_per_hour: i64,
    rounding_min: u32,
    minimum_min: u32,
    reserved_minor: i64,
) -> WalletFlip {
    let before = bill(closed, 0, minimum_min, None, flip_at);
    let closed_min: i64 = before.iter().map(|l| l.minutes).sum();
    let closed_minor: i64 = before.iter().map(|l| l.amount_minor).sum();
    let unit = i128::from(rounding_min.max(1));
    let rate = i128::from(rate_minor_per_hour.max(0));
    let left = i128::from((reserved_minor - closed_minor).max(0));
    let reachable = if rate == 0 {
        HORIZON_CAP_MIN
    } else {
        ((left * 60 + 29) / rate).min(HORIZON_CAP_MIN)
    };
    let total = (i128::from(closed_min) + reachable) / unit * unit;
    let k = i64::try_from((total - i128::from(closed_min)).max(0)).unwrap_or(0);
    let mut all = closed.to_vec();
    if let Some(first) = closed.first() {
        all.push(SegmentRow {
            id: String::new(),
            from_at: flip_at,
            to_at: Some(flip_at + k * MIN_MS),
            rate_minor_per_hour,
            paused_ms: 0,
            rule_id: None,
            ..first.clone()
        });
    }
    let cost: i64 = bill(&all, rounding_min, minimum_min, None, flip_at + k * MIN_MS)
        .iter()
        .map(|l| l.amount_minor)
        .sum();
    WalletFlip {
        ends_at: flip_at + k * MIN_MS,
        reserved_minor: cost.min(reserved_minor),
    }
}

/// The cost of the first (minimum) bill on a single segment, in minutes and money.
pub fn minimum_bill(
    rate_minor_per_hour: i64,
    rounding_min: u32,
    minimum_min: u32,
) -> WalletHorizon {
    let unit = i64::from(rounding_min.max(1));
    let minutes = ((i64::from(minimum_min) + unit - 1) / unit).max(1) * unit;
    WalletHorizon {
        minutes,
        cost_minor: (minutes * rate_minor_per_hour.max(0) + 30) / 60,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(from: i64, to: Option<i64>, rate: i64) -> SegmentRow {
        paused_seg(from, to, rate, 0)
    }

    fn paused_seg(from: i64, to: Option<i64>, rate: i64, paused_ms: i64) -> SegmentRow {
        SegmentRow {
            id: "G".into(),
            session_id: "S".into(),
            from_at: from,
            to_at: to,
            device_id: "D".into(),
            rate_minor_per_hour: rate,
            paused_ms,
            rule_id: None,
        }
    }

    fn amounts(lines: &[BillLine]) -> Vec<(i64, i64)> {
        lines.iter().map(|l| (l.minutes, l.amount_minor)).collect()
    }

    #[test]
    fn golden_rounds_up_to_the_unit_and_applies_the_minimum() {
        let s = [seg(0, None, 10_000)];
        assert_eq!(amounts(&bill(&s, 15, 30, None, 41 * MIN_MS)), [(45, 7500)]);
        assert_eq!(amounts(&bill(&s, 15, 30, None, 10 * MIN_MS)), [(30, 5000)]);
        assert_eq!(
            amounts(&bill(&s, 15, 30, None, 41 * MIN_MS + 1)),
            [(45, 7500)]
        );
        assert_eq!(
            amounts(&bill(&s, 1, 0, None, 41 * MIN_MS + 1)),
            [(42, 7000)]
        );
        assert_eq!(amounts(&bill(&s, 0, 0, None, 41 * MIN_MS)), [(41, 6833)]);
    }

    #[test]
    fn minimum_is_on_the_first_segment_rounding_on_the_last_and_pause_off_each_segment() {
        let s = [
            seg(0, Some(10 * MIN_MS), 6_000),
            seg(10 * MIN_MS, None, 12_000),
        ];
        assert_eq!(
            amounts(&bill(&s, 15, 30, None, 21 * MIN_MS)),
            [(30, 3000), (15, 3000)]
        );
        let s = [
            paused_seg(0, Some(10 * MIN_MS), 6_000, 0),
            paused_seg(10 * MIN_MS, None, 12_000, 5 * MIN_MS),
        ];
        assert_eq!(
            amounts(&bill(&s, 15, 30, None, 40 * MIN_MS)),
            [(30, 3000), (30, 6000)]
        );
        let s = [paused_seg(0, None, 12_000, 99 * MIN_MS)];
        assert_eq!(amounts(&bill(&s, 0, 0, None, 40 * MIN_MS))[0], (0, 0));
        let s = [
            paused_seg(0, Some(30 * MIN_MS), 6_000, 20 * MIN_MS),
            seg(30 * MIN_MS, None, 12_000),
        ];
        assert_eq!(
            amounts(&bill(&s, 0, 0, None, 40 * MIN_MS)),
            [(10, 1000), (10, 2000)]
        );
    }

    #[test]
    fn golden_a_sixty_minute_session_with_a_twenty_minute_pause_bills_forty() {
        let s = [paused_seg(0, Some(60 * MIN_MS), 6_000, 20 * MIN_MS)];
        assert_eq!(amounts(&bill(&s, 0, 0, None, 60 * MIN_MS)), [(40, 4000)]);
    }

    #[test]
    fn a_pause_still_open_is_excluded_up_to_the_bill_end() {
        let s = [paused_seg(0, None, 6_000, 10 * MIN_MS)];
        assert_eq!(
            amounts(&bill(&s, 0, 0, Some(40 * MIN_MS), 60 * MIN_MS)),
            [(30, 3000)]
        );
        assert_eq!(
            amounts(&bill(&s, 0, 0, Some(70 * MIN_MS), 60 * MIN_MS)),
            [(50, 5000)]
        );
    }

    fn horizon(rate: i64, rounding: u32, minimum: u32, available: i64) -> Option<(i64, i64)> {
        wallet_horizon(rate, rounding, minimum, available).map(|h| (h.minutes, h.cost_minor))
    }

    #[test]
    fn the_horizon_floors_to_the_rounding_unit() {
        assert_eq!(horizon(6_000, 15, 30, 10_000), Some((90, 9_000)));
        assert_eq!(horizon(6_000, 15, 30, 9_000), Some((90, 9_000)));
        assert_eq!(horizon(6_000, 15, 30, 8_999), Some((75, 7_500)));
        assert_eq!(horizon(6_000, 1, 0, 10_000), Some((100, 10_000)));
        assert_eq!(horizon(6_000, 0, 0, 10_050), Some((100, 10_000)));
    }

    #[test]
    fn the_horizon_needs_the_minimum_bill() {
        assert_eq!(horizon(6_000, 15, 30, 2_999), None);
        assert_eq!(horizon(6_000, 15, 30, 3_000), Some((30, 3_000)));
        assert_eq!(horizon(6_000, 15, 20, 3_000), Some((30, 3_000)));
        assert_eq!(horizon(6_000, 15, 0, 1_499), None);
        assert_eq!(horizon(6_000, 15, 0, 1_500), Some((15, 1_500)));
        assert_eq!(horizon(6_000, 0, 0, 0), None);
        assert_eq!(horizon(6_000, 0, 0, -5), None);
    }

    #[test]
    fn the_horizon_rounds_the_amount_half_up_like_the_bill() {
        assert_eq!(horizon(90, 0, 0, 1), None);
        assert_eq!(horizon(90, 0, 0, 2), Some((1, 2)));
        assert_eq!(horizon(90, 0, 0, 3), Some((2, 3)));
        assert_eq!(horizon(90, 0, 0, 4), Some((2, 3)));
        assert_eq!(horizon(90, 0, 0, 5), Some((3, 5)));
    }

    #[test]
    fn the_horizon_cost_matches_the_postpaid_bill_at_its_end() {
        for (rate, unit, minimum, available) in [
            (6_000, 15, 30, 10_000),
            (9_000, 15, 30, 12_345),
            (7_700, 1, 5, 4_321),
            (10_000, 0, 0, 99_999),
            (3_333, 10, 30, 5_000),
        ] {
            let h = wallet_horizon(rate, unit, minimum, available).unwrap();
            let s = [seg(0, None, rate)];
            let lines = bill(&s, unit, minimum, None, h.minutes * MIN_MS);
            assert_eq!(lines[0].amount_minor, h.cost_minor);
            assert!(h.cost_minor <= available);
            let past = bill(&s, unit, minimum, None, (h.minutes + 1) * MIN_MS);
            assert!(past[0].amount_minor > available || unit > 1);
        }
    }

    #[test]
    fn a_free_rate_is_capped_not_endless() {
        let h = wallet_horizon(0, 15, 30, 10).unwrap();
        assert_eq!((h.minutes, h.cost_minor), (366 * 24 * 60, 0));
    }

    #[test]
    fn a_pause_open_at_a_boundary_counts_only_from_the_new_segment() {
        let s = [
            paused_seg(0, Some(10 * MIN_MS), 6_000, 2 * MIN_MS),
            seg(10 * MIN_MS, None, 12_000),
        ];
        assert_eq!(
            amounts(&bill(&s, 0, 0, Some(8 * MIN_MS), 30 * MIN_MS)),
            [(8, 800), (0, 0)]
        );
        let s = [
            paused_seg(0, Some(10 * MIN_MS), 6_000, 5 * MIN_MS),
            seg(10 * MIN_MS, None, 12_000),
        ];
        assert_eq!(
            amounts(&bill(&s, 0, 0, Some(5 * MIN_MS), 30 * MIN_MS)),
            [(5, 500), (0, 0)]
        );
    }

    fn flipped(first: (i64, i64), flip_min: i64, rate: i64, reserved: i64) -> (i64, i64) {
        let closed = [seg(0, Some(flip_min * MIN_MS), first.1)];
        let f = wallet_end_after_flip(&closed, flip_min * MIN_MS, rate, 15, 30, reserved);
        (f.ends_at / MIN_MS, f.reserved_minor)
    }

    #[test]
    fn a_rate_that_rises_ends_the_wallet_session_sooner() {
        assert_eq!(flipped((0, 6_000), 30, 12_000, 9_000), (60, 9_000));
    }

    #[test]
    fn a_rate_that_drops_lets_the_reserve_play_longer() {
        assert_eq!(flipped((0, 6_000), 30, 3_000, 9_000), (150, 9_000));
    }

    #[test]
    fn the_reservation_shrinks_to_the_bill_of_the_new_end() {
        assert_eq!(flipped((0, 6_000), 30, 7_000, 9_000), (75, 8_250));
    }

    #[test]
    fn a_session_never_ends_before_the_flip_and_never_costs_more_than_reserved() {
        assert_eq!(flipped((0, 6_000), 30, 12_000, 3_000), (30, 3_000));
        assert_eq!(flipped((0, 6_000), 45, 60_000, 4_500), (45, 4_500));
    }

    #[test]
    fn the_new_end_bills_within_the_reserve_to_the_unit() {
        for (rate, reserved) in [
            (7_700, 9_000),
            (3_333, 12_345),
            (12_000, 20_000),
            (0, 5_000),
        ] {
            let closed = [seg(0, Some(30 * MIN_MS), 6_000)];
            let f = wallet_end_after_flip(&closed, 30 * MIN_MS, rate, 15, 30, reserved);
            let all = [closed[0].clone(), seg(30 * MIN_MS, Some(f.ends_at), rate)];
            let lines = bill(&all, 15, 30, None, f.ends_at);
            let total: i64 = lines.iter().map(|l| l.amount_minor).sum();
            let minutes: i64 = lines.iter().map(|l| l.minutes).sum();
            assert!(total <= reserved, "{rate} {reserved}");
            assert_eq!(total, f.reserved_minor);
            assert_eq!(minutes % 15, 0);
            assert!(f.ends_at >= 30 * MIN_MS);
        }
    }

    #[test]
    fn the_minimum_bill_is_the_first_whole_unit() {
        let b = minimum_bill(6_000, 15, 20);
        assert_eq!((b.minutes, b.cost_minor), (30, 3_000));
        let b = minimum_bill(6_000, 0, 0);
        assert_eq!((b.minutes, b.cost_minor), (1, 100));
    }
}

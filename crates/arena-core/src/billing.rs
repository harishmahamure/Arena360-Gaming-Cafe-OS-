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
    paused_total_ms: i64,
    now_ms: i64,
) -> Vec<BillLine> {
    let last = segments.len().saturating_sub(1);
    let mut minutes: Vec<i64> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let paused = if i == last { paused_total_ms } else { 0 };
            let ms = (s.to_at.unwrap_or(now_ms) - s.from_at - paused).max(0);
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
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(from: i64, to: Option<i64>, rate: i64) -> SegmentRow {
        SegmentRow {
            id: "G".into(),
            session_id: "S".into(),
            from_at: from,
            to_at: to,
            device_id: "D".into(),
            rate_minor_per_hour: rate,
        }
    }

    fn amounts(lines: &[BillLine]) -> Vec<(i64, i64)> {
        lines.iter().map(|l| (l.minutes, l.amount_minor)).collect()
    }

    #[test]
    fn golden_rounds_up_to_the_unit_and_applies_the_minimum() {
        let s = [seg(0, None, 10_000)];
        assert_eq!(amounts(&bill(&s, 15, 30, 0, 41 * MIN_MS)), [(45, 7500)]);
        assert_eq!(amounts(&bill(&s, 15, 30, 0, 10 * MIN_MS)), [(30, 5000)]);
        assert_eq!(amounts(&bill(&s, 15, 30, 0, 41 * MIN_MS + 1)), [(45, 7500)]);
        assert_eq!(amounts(&bill(&s, 1, 0, 0, 41 * MIN_MS + 1)), [(42, 7000)]);
        assert_eq!(amounts(&bill(&s, 0, 0, 0, 41 * MIN_MS)), [(41, 6833)]);
    }

    #[test]
    fn minimum_is_on_the_first_segment_rounding_on_the_last_and_pause_off_the_last() {
        let s = [
            seg(0, Some(10 * MIN_MS), 6_000),
            seg(10 * MIN_MS, None, 12_000),
        ];
        assert_eq!(
            amounts(&bill(&s, 15, 30, 0, 21 * MIN_MS)),
            [(30, 3000), (15, 3000)]
        );
        assert_eq!(
            amounts(&bill(&s, 15, 30, 5 * MIN_MS, 40 * MIN_MS)),
            [(30, 3000), (30, 6000)]
        );
        assert_eq!(
            amounts(&bill(&s, 0, 0, 99 * MIN_MS, 40 * MIN_MS))[1],
            (0, 0)
        );
    }
}

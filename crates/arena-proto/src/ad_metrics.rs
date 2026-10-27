//! Ad requests and impressions (Story 4.14): the two system intents that record one row each,
//! and the day-and-slot totals a Free branch's heartbeat carries to the cloud.
//!
//! Nothing here names a device, a creative, an advertiser or an advertising id: a row is its
//! order (`id`), the slot (`banner` or `interstitial`) and when it happened.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The most rows one heartbeat's block may total.
pub const AD_BATCH_ROWS: usize = 5_000;

/// Where an ad was asked for or shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AdSlot {
    Banner,
    Interstitial,
}

impl AdSlot {
    /// Every slot.
    pub const ALL: [AdSlot; 2] = [Self::Banner, Self::Interstitial];

    /// The stored and wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Banner => "banner",
            Self::Interstitial => "interstitial",
        }
    }

    /// The slot with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// The payload of `ad.impression_recorded` (system only): one ad was shown in `slot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdImpressionRecorded {
    pub slot: AdSlot,
}

/// The payload of `ad.request_recorded` (system only): an ad was asked for in `slot`, whether or
/// not one came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdRequestRecorded {
    pub slot: AdSlot,
}

/// What the Hub's loopback API accepts from the page for one plugin event
/// (`POST /ui/ads/event`). `id` is the event's ULID and becomes the intent's idempotency key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiAdEvent {
    pub kind: AdEventKind,
    pub slot: AdSlot,
    pub id: String,
}

/// Which of the two counted events a [`UiAdEvent`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AdEventKind {
    Request,
    Impression,
}

/// One business day's total for one slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdDayCount {
    /// `YYYY-MM-DD`, the branch's business day.
    pub business_day: String,
    pub slot: AdSlot,
    #[ts(type = "number")]
    pub n: i64,
}

/// The totals of the rows after a cursor, as a heartbeat block. `through_id` is the last row
/// included; the cloud acknowledges it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdCounts {
    /// The Hub's cursor when the block was built: the last id the cloud acknowledged. The cloud
    /// adds the block only when this is its own stored cursor, so a lost answer never makes a
    /// resent row count twice. Absent: no such check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub after_id: Option<i64>,
    #[ts(type = "number")]
    pub through_id: i64,
    pub counts: Vec<AdDayCount>,
}

impl AdCounts {
    /// Whether the block is well formed: a positive `through_id`, at most [`AD_BATCH_ROWS`] rows
    /// in total, every count positive, every day a `YYYY-MM-DD` date and no day-and-slot twice.
    pub fn valid(&self) -> bool {
        if self.through_id < 1
            || self.after_id.is_some_and(|a| a < 0 || a >= self.through_id)
            || self.counts.is_empty()
            || self.counts.len() > AD_BATCH_ROWS
        {
            return false;
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut total: i64 = 0;
        for c in &self.counts {
            if c.n < 1 || !valid_day(&c.business_day) || !seen.insert((&c.business_day, c.slot)) {
                return false;
            }
            total = total.saturating_add(c.n);
        }
        total <= AD_BATCH_ROWS as i64
    }
}

/// `YYYY-MM-DD` with a real calendar date.
fn valid_day(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &b[r];
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1 && (1..=last).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(through: i64, rows: &[(&str, AdSlot, i64)]) -> AdCounts {
        AdCounts {
            after_id: None,
            through_id: through,
            counts: rows
                .iter()
                .map(|(d, s, n)| AdDayCount {
                    business_day: (*d).to_owned(),
                    slot: *s,
                    n: *n,
                })
                .collect(),
        }
    }

    #[test]
    fn a_slot_has_one_stored_form() {
        for s in AdSlot::ALL {
            assert_eq!(AdSlot::parse(s.as_str()), Some(s));
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::json!(s.as_str())
            );
        }
        assert_eq!(AdSlot::parse("rewarded"), None);
    }

    #[test]
    fn the_payloads_carry_only_the_slot() {
        let p = crate::IntentPayload::AdImpressionRecorded(AdImpressionRecorded {
            slot: AdSlot::Banner,
        });
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({"kind": "ad.impression_recorded", "slot": "banner"})
        );
        let r: crate::IntentPayload =
            serde_json::from_str(r#"{"kind":"ad.request_recorded","slot":"interstitial"}"#)
                .unwrap();
        assert_eq!(
            r,
            crate::IntentPayload::AdRequestRecorded(AdRequestRecorded {
                slot: AdSlot::Interstitial
            })
        );
    }

    #[test]
    fn a_block_needs_a_cursor_real_days_and_positive_counts() {
        let ok = counts(
            9,
            &[
                ("2026-10-26", AdSlot::Banner, 4),
                ("2026-10-27", AdSlot::Banner, 4),
                ("2026-10-27", AdSlot::Interstitial, 1),
            ],
        );
        assert!(ok.valid());
        assert!(!counts(0, &[("2026-10-26", AdSlot::Banner, 1)]).valid());
        assert!(!counts(1, &[]).valid());
        assert!(!counts(1, &[("2026-10-26", AdSlot::Banner, 0)]).valid());
        assert!(!counts(1, &[("2026-02-30", AdSlot::Banner, 1)]).valid());
        assert!(!counts(1, &[("26-10-26", AdSlot::Banner, 1)]).valid());
        assert!(!counts(1, &[("2026-10-26", AdSlot::Banner, 5_001)]).valid());
        let twice = counts(
            2,
            &[
                ("2026-10-26", AdSlot::Banner, 1),
                ("2026-10-26", AdSlot::Banner, 1),
            ],
        );
        assert!(!twice.valid());
        let mut after = ok;
        after.after_id = Some(9);
        assert!(!after.valid(), "a block cannot start at or after its end");
        after.after_id = Some(0);
        assert!(after.valid());
    }

    #[test]
    fn a_block_has_no_id_of_anything_but_its_rows() {
        let json = serde_json::to_string(&counts(3, &[("2026-10-26", AdSlot::Banner, 3)])).unwrap();
        assert_eq!(
            json,
            r#"{"through_id":3,"counts":[{"business_day":"2026-10-26","slot":"banner","n":3}]}"#
        );
    }
}

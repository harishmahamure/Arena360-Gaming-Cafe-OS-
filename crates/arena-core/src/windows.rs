//! Play windows of passes and blocks: minutes from local midnight, which may wrap midnight.

use chrono::{Duration, NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;

const MIN_MS: i64 = 60_000;
const DAY_MIN: i64 = 1440;

/// A window of the local day, `start_min` to `end_min`, both minutes from local midnight. An end
/// before the start wraps to the next day (22:00–06:00).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayWindow {
    pub start_min: i64,
    pub end_min: i64,
}

impl PlayWindow {
    pub fn new(start_min: Option<i64>, end_min: Option<i64>) -> Option<Self> {
        let (start_min, end_min) = (start_min?, end_min?);
        let ok = |m: i64| (0..DAY_MIN).contains(&m);
        (ok(start_min) && ok(end_min) && start_min != end_min)
            .then_some(Self { start_min, end_min })
    }

    /// `22:00–06:00`
    pub fn label(&self) -> String {
        let clock = |m: i64| format!("{:02}:{:02}", m / 60, m % 60);
        format!("{}–{}", clock(self.start_min), clock(self.end_min))
    }

    /// The instant (UTC ms) the window ends, when `now_ms` is inside it, else `None`. The window
    /// is read on the wall clock of `tz`, so it does not move with the business-day start.
    pub fn ends_at(&self, now_ms: i64, tz: Tz) -> Option<i64> {
        let local = chrono::DateTime::from_timestamp_millis(now_ms)?
            .with_timezone(&tz)
            .naive_local();
        let tod = i64::from(local.time().num_seconds_from_midnight()) * 1000
            + i64::from(local.time().nanosecond() / 1_000_000);
        let (start, end) = (self.start_min * MIN_MS, self.end_min * MIN_MS);
        let end_day = if self.start_min < self.end_min {
            (start..end).contains(&tod).then_some(local.date())?
        } else if tod >= start {
            local.date() + Duration::days(1)
        } else if tod < end {
            local.date()
        } else {
            return None;
        };
        let end = local_ms(tz, end_day, self.end_min, now_ms)?;
        (end > now_ms).then_some(end)
    }
}

/// The instant of local `min` on `day`. When the clock shows that minute twice, the first time
/// it does, unless that has already passed at `now_ms`.
pub(crate) fn local_ms(tz: Tz, day: NaiveDate, min: i64, now_ms: i64) -> Option<i64> {
    let at = day.and_hms_opt(0, 0, 0)? + Duration::minutes(min);
    let t = match tz.from_local_datetime(&at) {
        chrono::LocalResult::Single(t) => t,
        chrono::LocalResult::Ambiguous(first, second) => {
            if first.timestamp_millis() > now_ms {
                first
            } else {
                second
            }
        }
        chrono::LocalResult::None => tz
            .from_local_datetime(&(at + Duration::hours(1)))
            .earliest()?,
    };
    Some(t.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn at(tz: Tz, local: &str) -> i64 {
        let naive = NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
        tz.from_local_datetime(&naive).unwrap().timestamp_millis()
    }

    fn night() -> PlayWindow {
        PlayWindow::new(Some(22 * 60), Some(6 * 60)).unwrap()
    }

    const IST: Tz = Tz::Asia__Kolkata;

    #[test]
    fn a_late_start_ends_at_six_the_next_morning() {
        assert_eq!(
            night().ends_at(at(IST, "2027-03-10 23:30"), IST),
            Some(at(IST, "2027-03-11 06:00"))
        );
    }

    #[test]
    fn after_midnight_the_same_window_ends_that_morning() {
        assert_eq!(
            night().ends_at(at(IST, "2027-03-11 02:15"), IST),
            Some(at(IST, "2027-03-11 06:00"))
        );
    }

    #[test]
    fn the_start_is_inside_and_the_end_is_outside() {
        assert!(night().ends_at(at(IST, "2027-03-10 22:00"), IST).is_some());
        assert!(night().ends_at(at(IST, "2027-03-10 21:59"), IST).is_none());
        assert!(night().ends_at(at(IST, "2027-03-11 05:59"), IST).is_some());
        assert!(night().ends_at(at(IST, "2027-03-11 06:00"), IST).is_none());
    }

    #[test]
    fn a_same_day_window_ends_the_same_day() {
        let day = PlayWindow::new(Some(9 * 60), Some(17 * 60 + 30)).unwrap();
        assert_eq!(
            day.ends_at(at(IST, "2027-03-10 09:00"), IST),
            Some(at(IST, "2027-03-10 17:30"))
        );
        assert_eq!(day.ends_at(at(IST, "2027-03-10 17:30"), IST), None);
        assert_eq!(day.ends_at(at(IST, "2027-03-10 08:59"), IST), None);
    }

    #[test]
    fn a_window_ending_at_midnight_ends_at_the_next_local_midnight() {
        let evening = PlayWindow::new(Some(18 * 60), Some(0)).unwrap();
        assert_eq!(
            evening.ends_at(at(IST, "2027-03-10 20:00"), IST),
            Some(at(IST, "2027-03-11 00:00"))
        );
    }

    #[test]
    fn the_window_is_read_in_the_branch_zone_not_utc() {
        let dubai = Tz::Asia__Dubai;
        let now = at(dubai, "2027-03-10 21:00");
        assert_eq!(night().ends_at(now, dubai), None);
        assert_eq!(night().ends_at(now, IST), Some(at(IST, "2027-03-11 06:00")));
    }

    #[test]
    fn a_window_ending_in_a_repeated_hour_never_ends_before_now() {
        let ny = Tz::America__New_York;
        let w = PlayWindow::new(Some(23 * 60), Some(60 + 30)).unwrap();
        let first = ny
            .from_local_datetime(
                &NaiveDateTime::parse_from_str("2027-11-07 01:30", "%Y-%m-%d %H:%M").unwrap(),
            )
            .earliest()
            .unwrap()
            .timestamp_millis();
        let second = first + 3_600_000;
        assert_eq!(w.ends_at(first - 600_000, ny), Some(first));
        assert_eq!(w.ends_at(second - 1_200_000, ny), Some(second));
        assert_eq!(w.ends_at(second, ny), None);
    }

    #[test]
    fn the_business_day_start_does_not_move_the_window() {
        let rows = |day_start: &str| {
            use crate::locale::{LocaleSetting, resolve};
            let l = [
                LocaleSetting::now("branch.tz", "Asia/Kolkata"),
                LocaleSetting::now("branch.day_start_min", day_start),
            ];
            resolve(&l, 0).1.tz
        };
        let now = at(IST, "2027-03-11 02:15");
        for day_start in ["0", "240", "360"] {
            assert_eq!(
                night().ends_at(now, rows(day_start)),
                Some(at(IST, "2027-03-11 06:00"))
            );
        }
    }

    #[test]
    fn a_window_needs_both_ends_inside_a_day_and_apart() {
        assert!(PlayWindow::new(None, Some(60)).is_none());
        assert!(PlayWindow::new(Some(60), Some(60)).is_none());
        assert!(PlayWindow::new(Some(1440), Some(60)).is_none());
        assert_eq!(night().label(), "22:00–06:00");
    }
}

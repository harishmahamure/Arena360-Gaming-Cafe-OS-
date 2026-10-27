use arena_proto::{
    AppError, DEFAULT_OPENING_HOURS, KEY_OPENING_HOURS, OpeningDay, SessionScreen,
    parse_opening_hours,
};
use arena_store::Store;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::locale::{locale_rows, resolve};

const DAY_MS: u64 = 86_400_000;

fn closes_in(week: &[OpeningDay; 7], tz: Tz, now_ms: i64) -> Option<u64> {
    let today: NaiveDate = tz.timestamp_millis_opt(now_ms).single()?.date_naive();
    (-1..=7)
        .filter_map(|d| {
            let date = today + Duration::days(d);
            let OpeningDay::Open {
                open_min,
                close_min,
            } = week[date.weekday().num_days_from_monday() as usize]
            else {
                return None;
            };
            let date = date + Duration::days(i64::from(close_min < open_min));
            let at = date.and_time(NaiveTime::from_hms_opt(
                u32::from(close_min) / 60,
                u32::from(close_min) % 60,
                0,
            )?);
            let ms = tz.from_local_datetime(&at).earliest()?.timestamp_millis();
            u64::try_from(ms - now_ms).ok().filter(|m| *m > 0)
        })
        .min()
}

pub fn session_screens(
    store: &Store,
    now_ms: i64,
) -> Result<Vec<(String, SessionScreen)>, AppError> {
    let (sessions, devices, meta, settings, branch) = store.read_consistent(|s| {
        Ok((
            s.live_sessions()?,
            s.devices()?,
            s.devices_meta()?,
            s.settings()?,
            s.branch_locale()?,
        ))
    })?;
    let lock_after = branch
        .and_then(|b| {
            let tz = resolve(&locale_rows(&b, &settings), now_ms).1.tz;
            let hours = settings
                .iter()
                .find(|r| r.key == KEY_OPENING_HOURS)
                .map_or(DEFAULT_OPENING_HOURS, |r| r.value.as_str());
            closes_in(&parse_opening_hours(hours).ok()?, tz, now_ms)
        })
        .unwrap_or(DAY_MS);
    Ok(sessions
        .into_iter()
        .map(|s| {
            let name = meta
                .iter()
                .find(|m| m.device_id == s.device_id && m.deleted_at.is_none())
                .map(|m| m.label.clone())
                .or_else(|| {
                    devices
                        .iter()
                        .find(|d| d.device_id == s.device_id)
                        .map(|d| d.label.clone())
                })
                .unwrap_or_default();
            let screen = SessionScreen {
                session_id: s.id,
                device_name: name,
                elapsed_ms: u64::try_from(now_ms - s.started_at).unwrap_or(0),
                lock_after_ms: lock_after,
                time_up_mode: s.time_up_mode,
            };
            (s.device_id, screen)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn week(day: OpeningDay) -> [OpeningDay; 7] {
        [day; 7]
    }

    fn at(tz: Tz, local: &str) -> i64 {
        let t = chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap();
        tz.from_local_datetime(&t).unwrap().timestamp_millis()
    }

    #[test]
    fn a_session_locks_at_the_next_closing_time_wrapping_midnight() {
        let tz: Tz = "Asia/Kolkata".parse().unwrap();
        let day = OpeningDay::Open {
            open_min: 600,
            close_min: 1320,
        };
        let now = at(tz, "2026-10-27 20:00");
        assert_eq!(closes_in(&week(day), tz, now), Some(2 * 3_600_000));
        let late = OpeningDay::Open {
            open_min: 600,
            close_min: 120,
        };
        assert_eq!(
            closes_in(&week(late), tz, at(tz, "2026-10-27 23:30")),
            Some(150 * 60_000)
        );
        assert_eq!(
            closes_in(&week(late), tz, at(tz, "2026-10-28 01:00")),
            Some(60 * 60_000)
        );
    }

    #[test]
    fn no_opening_hours_means_no_closing_time() {
        let tz: Tz = "UTC".parse().unwrap();
        assert_eq!(closes_in(&week(OpeningDay::Closed), tz, 0), None);
    }
}

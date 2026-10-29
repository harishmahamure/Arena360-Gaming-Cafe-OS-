use arena_proto::{
    AppError, DEFAULT_OPENING_HOURS, KEY_OPENING_HOURS, OpeningDay, SessionScreen,
    parse_opening_hours,
};
use arena_store::Store;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::locale::{locale_rows, resolve};
use crate::session_timer::TimedSession;

const DAY_MS: u64 = 86_400_000;

fn grace_ms(grace_min: u32) -> i64 {
    i64::from(grace_min) * 60_000
}

pub(crate) fn closes_in(week: &[OpeningDay; 7], tz: Tz, now_ms: i64) -> Option<u64> {
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

fn closing_in(
    branch: Option<arena_store::BranchLocale>,
    settings: &[arena_store::SettingRow],
    now_ms: i64,
) -> Option<u64> {
    let b = branch?;
    let tz = resolve(&locale_rows(&b, settings), now_ms).1.tz;
    let hours = settings
        .iter()
        .find(|r| r.key == KEY_OPENING_HOURS)
        .map_or(DEFAULT_OPENING_HOURS, |r| r.value.as_str());
    closes_in(&parse_opening_hours(hours).ok()?, tz, now_ms)
}

/// The running sessions the timer engine tracks: a timed session by its `ends_at`, a postpaid
/// one by the branch's next closing time.
pub fn timed_sessions(store: &Store, now_ms: i64) -> Result<Vec<TimedSession>, AppError> {
    let (sessions, settings, branch) =
        store.read_consistent(|s| Ok((s.live_sessions()?, s.settings()?, s.branch_locale()?)))?;
    let closing = closing_in(branch, &settings, now_ms)
        .and_then(|ms| i64::try_from(ms).ok())
        .map(|ms| now_ms + ms);
    Ok(sessions
        .into_iter()
        .filter(|s| s.state == "active" || s.state == "grace")
        .filter_map(|s| match (s.ends_at, closing) {
            (Some(at), _) => Some((s, at, true)),
            (None, Some(at)) if s.mode == "postpaid" && s.state == "active" => Some((s, at, false)),
            _ => None,
        })
        .map(|(s, ends_at, expires)| TimedSession {
            ends_at,
            due_at: if s.state == "grace" {
                ends_at + grace_ms(s.grace_min)
            } else {
                ends_at
            },
            expires,
            session_id: s.id,
            device_id: s.device_id,
        })
        .collect())
}

pub fn session_screens(
    store: &Store,
    now_ms: i64,
) -> Result<Vec<(String, SessionScreen)>, AppError> {
    let (sessions, devices, meta, settings, branch, ordered) = store.read_consistent(|s| {
        Ok((
            s.live_sessions()?,
            s.devices()?,
            s.devices_meta()?,
            s.settings()?,
            s.branch_locale()?,
            s.ordered_counts()?,
        ))
    })?;
    let lock_after = closing_in(branch, &settings, now_ms).unwrap_or(DAY_MS);
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
            let own_lock = match (s.state.as_str(), s.ends_at) {
                ("active" | "grace", Some(at)) => {
                    Some(u64::try_from(at + grace_ms(s.grace_min) - now_ms).unwrap_or(0))
                }
                _ => None,
            };
            let remaining_ms = s
                .ends_at
                .map(|at| u64::try_from(at - s.paused_at.unwrap_or(now_ms)).unwrap_or(0));
            let screen = SessionScreen {
                username: s.username,
                guest_no: s.guest_no,
                remaining_ms,
                ordered_items: ordered
                    .get(&s.id)
                    .map_or(0, |n| u64::try_from(*n).unwrap_or(0)),
                session_id: s.id,
                device_name: name,
                elapsed_ms: u64::try_from(
                    s.paused_at.unwrap_or(now_ms) - s.started_at - s.paused_total_ms,
                )
                .unwrap_or(0),
                lock_after_ms: match (s.state.as_str(), own_lock) {
                    ("expired", _) => 0,
                    (_, Some(own)) => own.min(lock_after),
                    _ => lock_after,
                },
                time_up_mode: s.time_up_mode,
                // The grace phase shows only while the session's own end drives the lock: a
                // held session, or one capped by closing time, has none.
                grace_ms: if own_lock.is_some_and(|own| own <= lock_after) {
                    u64::from(s.grace_min) * 60_000
                } else {
                    0
                },
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

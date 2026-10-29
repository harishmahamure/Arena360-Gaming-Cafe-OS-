//! The session timer engine: a min-heap of end times that a 1 s tick drains. It holds no
//! database handle and no clock; the Hub task feeds it the running sessions and the time.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use arena_proto::SESSION_WARNING_MARKS_MIN;

const MINUTE_MS: i64 = 60_000;

/// A running session with an end time. `due_at` is when the engine next acts on it: `ends_at` for
/// an active session, the end of its grace for one in grace. A postpaid session carries its
/// branch's closing time instead and is only warned, never expired (its kiosk locks at closing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedSession {
    pub session_id: String,
    pub device_id: String,
    pub ends_at: i64,
    pub due_at: i64,
    pub expires: bool,
}

/// What the engine asks the Hub to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerAction {
    Warn {
        session_id: String,
        device_id: String,
        minutes_left: u8,
    },
    Expire(TimedSession),
}

/// What the Hub tells its kiosks after the engine acted (never persisted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerEvent {
    Warning {
        session_id: String,
        device_id: String,
        minutes_left: u8,
    },
    /// The time-up action ran: the kiosk locks, asking the player to see the counter when staff
    /// (`held`) must end the session.
    Expired {
        session_id: String,
        device_id: String,
        held: bool,
    },
}

#[derive(Debug)]
struct Entry {
    device_id: String,
    ends_at: i64,
    due_at: i64,
    expires: bool,
    next_mark: usize,
}

fn first_unpassed(marks: &[u8], ends_at: i64, now_ms: i64) -> usize {
    let left = ends_at - now_ms;
    marks
        .iter()
        .position(|m| left > i64::from(*m) * MINUTE_MS)
        .unwrap_or(marks.len())
}

#[derive(Debug)]
pub struct TimerEngine {
    heap: BinaryHeap<Reverse<(i64, String)>>,
    entries: HashMap<String, Entry>,
    /// The minutes-left marks sessions are warned at, longest first.
    marks: Vec<u8>,
}

impl Default for TimerEngine {
    fn default() -> Self {
        Self {
            heap: BinaryHeap::new(),
            entries: HashMap::new(),
            marks: SESSION_WARNING_MARKS_MIN.to_vec(),
        }
    }
}

impl TimerEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Makes the engine track exactly `sessions`. A session whose end time did not change keeps
    /// its warning progress; a new or moved one starts after the marks already passed, so a
    /// restart or a resume never repeats a warning.
    pub fn sync(&mut self, sessions: impl IntoIterator<Item = TimedSession>, now_ms: i64) {
        let mut next = HashMap::new();
        for s in sessions {
            let kept = self
                .entries
                .remove(&s.session_id)
                .filter(|e| e.ends_at == s.ends_at && e.due_at == s.due_at)
                .map(|e| e.next_mark);
            if kept.is_none() {
                self.heap.push(Reverse((s.due_at, s.session_id.clone())));
            }
            next.insert(
                s.session_id,
                Entry {
                    device_id: s.device_id,
                    ends_at: s.ends_at,
                    due_at: s.due_at,
                    expires: s.expires,
                    next_mark: kept
                        .unwrap_or_else(|| first_unpassed(&self.marks, s.ends_at, now_ms)),
                },
            );
        }
        self.entries = next;
        if self.heap.len() > 2 * self.entries.len() + 16 {
            self.heap = self
                .entries
                .iter()
                .map(|(id, e)| Reverse((e.due_at, id.clone())))
                .collect();
        }
    }

    /// Changes the marks sessions are warned at (longest first; the branch's setting). Every
    /// session carries on from the first new mark still ahead of it, so a mark it has already
    /// passed is not raised late and one it has already been warned at is not raised again.
    pub fn set_marks(&mut self, marks: &[u8], now_ms: i64) {
        if self.marks == marks {
            return;
        }
        self.marks = marks.to_vec();
        for e in self.entries.values_mut() {
            e.next_mark = first_unpassed(&self.marks, e.ends_at, now_ms);
        }
    }

    /// Puts back a session whose expiry could not be recorded, so the next tick retries it.
    pub fn requeue(&mut self, s: &TimedSession, now_ms: i64) {
        self.heap.push(Reverse((s.due_at, s.session_id.clone())));
        self.entries.insert(
            s.session_id.clone(),
            Entry {
                device_id: s.device_id.clone(),
                ends_at: s.ends_at,
                due_at: s.due_at,
                expires: s.expires,
                next_mark: first_unpassed(&self.marks, s.ends_at, now_ms),
            },
        );
    }

    /// Everything due at `now_ms`: expiries in `ends_at` order (each session once), then the
    /// warnings that came due, soonest to end first.
    pub fn tick(&mut self, now_ms: i64) -> Vec<TimerAction> {
        let mut actions = Vec::new();
        while let Some(Reverse((at, id))) = self.heap.peek().cloned() {
            if at > now_ms {
                break;
            }
            self.heap.pop();
            if self.entries.get(&id).is_some_and(|e| e.due_at == at)
                && let Some(e) = self.entries.remove(&id)
                && e.expires
            {
                actions.push(TimerAction::Expire(TimedSession {
                    session_id: id,
                    device_id: e.device_id,
                    ends_at: e.ends_at,
                    due_at: at,
                    expires: true,
                }));
            }
        }
        let mut warnings = Vec::new();
        let marks = &self.marks;
        for (id, e) in &mut self.entries {
            let left = e.ends_at - now_ms;
            let mut due = None;
            while let Some(mark) = marks.get(e.next_mark) {
                if left > i64::from(*mark) * MINUTE_MS {
                    break;
                }
                due = Some(*mark);
                e.next_mark += 1;
            }
            if let Some(minutes_left) = due {
                warnings.push((
                    e.ends_at,
                    id.clone(),
                    TimerAction::Warn {
                        session_id: id.clone(),
                        device_id: e.device_id.clone(),
                        minutes_left,
                    },
                ));
            }
        }
        warnings.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        actions.extend(warnings.into_iter().map(|(_, _, a)| a));
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000_000;

    fn timed(id: &str, ends_at: i64) -> TimedSession {
        TimedSession {
            session_id: id.into(),
            device_id: format!("D-{id}"),
            ends_at,
            due_at: ends_at,
            expires: true,
        }
    }

    fn expired(actions: &[TimerAction]) -> Vec<&str> {
        actions
            .iter()
            .filter_map(|a| match a {
                TimerAction::Expire(s) => Some(s.session_id.as_str()),
                TimerAction::Warn { .. } => None,
            })
            .collect()
    }

    fn marks(actions: &[TimerAction]) -> Vec<u8> {
        actions
            .iter()
            .filter_map(|a| match a {
                TimerAction::Warn { minutes_left, .. } => Some(*minutes_left),
                TimerAction::Expire(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_session_expires_once_at_its_end_time() {
        let mut e = TimerEngine::new();
        e.sync([timed("A", T0 + 5_000)], T0);
        assert!(expired(&e.tick(T0 + 4_999)).is_empty());
        assert_eq!(expired(&e.tick(T0 + 5_000)), ["A"]);
        assert!(e.tick(T0 + 6_000).is_empty());
        assert!(e.is_empty());
    }

    #[test]
    fn warnings_fire_at_ten_five_and_one_minute_once_each() {
        let mut e = TimerEngine::new();
        let end = T0 + 20 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        assert!(e.tick(end - 10 * MINUTE_MS - 1).is_empty());
        assert_eq!(marks(&e.tick(end - 10 * MINUTE_MS)), [10]);
        assert!(e.tick(end - 9 * MINUTE_MS).is_empty());
        assert_eq!(marks(&e.tick(end - 5 * MINUTE_MS)), [5]);
        assert_eq!(marks(&e.tick(end - MINUTE_MS)), [1]);
        assert!(e.tick(end - 30_000).is_empty());
    }

    #[test]
    fn configured_marks_replace_the_defaults_from_the_next_tick() {
        let mut e = TimerEngine::new();
        let end = T0 + 40 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        e.set_marks(&[20, 3], T0);
        assert!(e.tick(end - 20 * MINUTE_MS - 1).is_empty());
        assert_eq!(marks(&e.tick(end - 20 * MINUTE_MS)), [20]);
        assert!(e.tick(end - 10 * MINUTE_MS).is_empty());
        assert_eq!(marks(&e.tick(end - 3 * MINUTE_MS)), [3]);
        assert!(e.tick(end - MINUTE_MS).is_empty());
    }

    #[test]
    fn changing_the_marks_mid_session_skips_marks_already_passed() {
        let mut e = TimerEngine::new();
        let end = T0 + 30 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        // 12 minutes left: the 10-minute mark is still ahead and the new 15 has gone by.
        let now = end - 12 * MINUTE_MS;
        e.set_marks(&[15, 10, 2], now);
        assert!(e.tick(now).is_empty());
        assert_eq!(marks(&e.tick(end - 10 * MINUTE_MS)), [10]);
        assert_eq!(marks(&e.tick(end - 2 * MINUTE_MS)), [2]);
        // The same marks again change nothing, even after the session moved on.
        e.set_marks(&[15, 10, 2], end - MINUTE_MS);
        assert!(e.tick(end - 30_000).is_empty());
    }

    #[test]
    fn a_late_tick_sends_only_the_shortest_passed_mark() {
        let mut e = TimerEngine::new();
        let end = T0 + 20 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        assert_eq!(marks(&e.tick(end - 4 * MINUTE_MS)), [5]);
    }

    #[test]
    fn marks_already_passed_are_not_sent_after_a_restart() {
        let mut e = TimerEngine::new();
        let end = T0 + 3 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        assert!(e.tick(T0).is_empty());
        assert_eq!(marks(&e.tick(end - MINUTE_MS)), [1]);
    }

    #[test]
    fn a_sync_keeps_warning_progress_when_the_end_time_is_unchanged() {
        let mut e = TimerEngine::new();
        let end = T0 + 20 * MINUTE_MS;
        e.sync([timed("A", end)], T0);
        assert_eq!(marks(&e.tick(end - 10 * MINUTE_MS)), [10]);
        e.sync([timed("A", end)], end - 10 * MINUTE_MS);
        assert!(e.tick(end - 9 * MINUTE_MS).is_empty());
    }

    #[test]
    fn a_resume_moves_the_end_time_and_expires_at_the_new_one() {
        let mut e = TimerEngine::new();
        e.sync([timed("A", T0 + 10_000)], T0);
        e.sync([timed("A", T0 + 70_000)], T0 + 5_000);
        assert!(expired(&e.tick(T0 + 10_000)).is_empty());
        assert_eq!(expired(&e.tick(T0 + 70_000)), ["A"]);
    }

    #[test]
    fn a_postpaid_session_is_warned_before_closing_but_never_expired() {
        let mut e = TimerEngine::new();
        let end = T0 + 20 * MINUTE_MS;
        e.sync(
            [TimedSession {
                expires: false,
                ..timed("P", end)
            }],
            T0,
        );
        assert_eq!(marks(&e.tick(end - 10 * MINUTE_MS)), [10]);
        assert!(e.tick(end).is_empty());
        assert!(e.is_empty());
    }

    #[test]
    fn a_session_that_left_the_set_never_expires() {
        let mut e = TimerEngine::new();
        e.sync([timed("A", T0 + 1_000), timed("B", T0 + 2_000)], T0);
        e.sync([timed("B", T0 + 2_000)], T0);
        assert_eq!(expired(&e.tick(T0 + 5_000)), ["B"]);
    }

    #[test]
    fn overdue_sessions_end_at_once_in_end_time_order() {
        let mut e = TimerEngine::new();
        e.sync(
            [
                timed("C", T0 - 1_000),
                timed("A", T0 - 9_000),
                timed("B", T0 - 5_000),
            ],
            T0,
        );
        assert_eq!(expired(&e.tick(T0)), ["A", "B", "C"]);
        assert!(e.tick(T0 + 1_000).is_empty());
    }

    #[test]
    fn a_session_in_grace_is_acted_on_at_the_end_of_its_grace_and_never_warned() {
        let mut e = TimerEngine::new();
        let ends = T0 - 30_000;
        let due = ends + 2 * MINUTE_MS;
        e.sync(
            [TimedSession {
                due_at: due,
                ..timed("G", ends)
            }],
            T0,
        );
        assert!(e.tick(T0).is_empty());
        assert!(e.tick(due - 1).is_empty());
        assert_eq!(expired(&e.tick(due)), ["G"]);
        assert!(e.tick(due + 1_000).is_empty());
    }

    #[test]
    fn a_failed_expiry_is_retried_on_the_next_tick() {
        let mut e = TimerEngine::new();
        e.sync([timed("A", T0)], T0);
        let TimerAction::Expire(s) = e.tick(T0).remove(0) else {
            panic!("expected an expiry");
        };
        e.requeue(&s, T0);
        assert_eq!(expired(&e.tick(T0 + 1_000)), ["A"]);
    }

    #[test]
    fn five_hundred_random_sessions_each_end_within_a_second_of_their_end_time() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let sessions: Vec<TimedSession> = (0..500)
            .map(|i| timed(&format!("S{i:03}"), T0 + (next() % 3_600_000) as i64))
            .collect();
        let mut e = TimerEngine::new();
        e.sync(sessions.clone(), T0);
        let mut ended: HashMap<String, i64> = HashMap::new();
        let mut now = T0;
        while now <= T0 + 3_601_000 {
            for a in e.tick(now) {
                if let TimerAction::Expire(s) = a {
                    assert!(ended.insert(s.session_id, now).is_none(), "ended twice");
                }
            }
            now += 1_000;
        }
        assert_eq!(ended.len(), 500);
        for s in &sessions {
            let at = ended[&s.session_id];
            assert!(
                at >= s.ends_at && at - s.ends_at < 1_000,
                "{} late",
                s.session_id
            );
        }
    }
}

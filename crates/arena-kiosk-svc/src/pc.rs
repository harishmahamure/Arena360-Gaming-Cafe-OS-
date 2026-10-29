use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use arena_proto::{LockReason, PcAction};
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

use crate::queue::QueueDb;
use crate::runtime::Ctx;

const LOCKED_KEY: &str = "pc.locked";
/// Kept in the queue kv while the player's programs are suspended (Story 6.22), so a restart
/// still thaws them on the next unlock.
const FROZEN_KEY: &str = "pc.frozen";
const SEEN_MAX: usize = 64;
/// The most staff messages one session keeps; the oldest go first.
pub const INBOX_MAX: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Screen {
    pub locked: bool,
    pub reason: Option<LockReason>,
    pub session: Option<ScreenSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSession {
    pub device_name: String,
    pub started: Instant,
    pub warning_min: Option<u8>,
    /// When the grace after the end time runs out and the kiosk locks.
    pub lock_at: Instant,
    pub grace_ms: u64,
    pub ordered_items: u64,
    pub username: Option<String>,
    pub guest_no: Option<i64>,
    /// When a prepaid session's time runs out (`None` for postpaid, which counts up).
    pub ends_at: Option<Instant>,
    /// The wallet balance (minor units) and plan name the overlay HUD shows.
    pub balance_minor: Option<i64>,
    pub plan_name: Option<String>,
    /// What is left of the wallet when a wallet-paid session is at or below the branch's
    /// low-balance amount (minor units), else `None` (Story 6.20).
    pub low_balance_minor: Option<i64>,
    /// The branch's time-up mode is "Staff handles".
    pub hold: bool,
}

/// One staff message of the session on screen (Story 6.16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffNote {
    /// This service's own counter, so the app can tell a replay from a new message.
    pub id: u64,
    pub text: String,
    /// When it arrived, UTC ms.
    pub at: i64,
    pub read: bool,
}

#[derive(Debug, Default)]
struct Inbox {
    last_id: u64,
    /// Newest first.
    notes: Vec<StaffNote>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Ack,
    AckThenPower(PcAction),
    Refused,
}

pub struct PcState {
    pub screen: watch::Sender<Screen>,
    pub deadline: watch::Sender<Option<Instant>>,
    seen: Mutex<VecDeque<String>>,
    inbox: Mutex<Inbox>,
    /// The player's programs are suspended under a Staff-handles time-up lock.
    frozen: AtomicBool,
    /// Each message as it arrives (the supervisor forwards it as a fresh `staff_message`).
    pub notes: broadcast::Sender<StaffNote>,
}

impl PcState {
    pub fn load(queue: &QueueDb) -> Self {
        let locked = matches!(queue.kv_get(LOCKED_KEY), Ok(Some((v, _))) if v == "1");
        let frozen = matches!(queue.kv_get(FROZEN_KEY), Ok(Some((v, _))) if v == "1");
        Self {
            frozen: AtomicBool::new(frozen),
            screen: watch::channel(Screen {
                locked,
                ..Screen::default()
            })
            .0,
            deadline: watch::channel(None).0,
            seen: Mutex::default(),
            inbox: Mutex::default(),
            notes: broadcast::channel(16).0,
        }
    }

    fn inbox(&self) -> std::sync::MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Keeps a message at the front of the list and tells the supervisor.
    fn keep(&self, text: String, at: i64) -> StaffNote {
        let note = {
            let mut inbox = self.inbox();
            inbox.last_id += 1;
            let note = StaffNote {
                id: inbox.last_id,
                text,
                at,
                read: false,
            };
            inbox.notes.insert(0, note.clone());
            inbox.notes.truncate(INBOX_MAX);
            note
        };
        let _ = self.notes.send(note.clone());
        note
    }

    /// The session's messages, oldest first.
    pub fn staff_notes(&self) -> Vec<StaffNote> {
        self.inbox().notes.iter().rev().cloned().collect()
    }

    /// Marks every message read; whether any was unread.
    pub fn read_all(&self) -> bool {
        let mut inbox = self.inbox();
        let unread = inbox.notes.iter().any(|n| !n.read);
        inbox.notes.iter_mut().for_each(|n| n.read = true);
        unread
    }

    /// Forgets the session's messages (it ended, or a new one begins).
    pub fn clear_notes(&self) {
        self.inbox().notes.clear();
    }

    fn first_time(&self, cmd_id: &str) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        if seen.iter().any(|s| s == cmd_id) {
            return false;
        }
        if seen.len() == SEEN_MAX {
            seen.pop_front();
        }
        seen.push_back(cmd_id.to_owned());
        true
    }
}

pub(crate) fn set_locked(ctx: &Ctx, locked: bool, reason: Option<LockReason>) {
    // While a session-end cleanup runs the screen shows its own lock (Story 6.11); the lock
    // asked for is remembered and shown when it is done.
    let shown = crate::cleanup::intercept(ctx, locked, reason);
    apply_lock(ctx, locked, reason, shown);
}

/// Shows `reason` as the lock, as the cleanup's last act (no cleanup is asked again).
pub(crate) fn show_lock(ctx: &Ctx, reason: Option<LockReason>) {
    apply_lock(ctx, true, reason, reason);
}

/// Shows the cleanup's own lock (after a restart that found a cleanup under way).
pub(crate) fn show_cleanup_lock(ctx: &Ctx) {
    apply_lock(ctx, true, None, Some(LockReason::PreparingPc));
}

/// Sets the screen. `asked` is the reason the Hub or a timer gave (it decides what the lock
/// ends: the session, the deadline); `shown` is the reason the screen carries.
fn apply_lock(ctx: &Ctx, locked: bool, asked: Option<LockReason>, shown: Option<LockReason>) {
    let ends = locked
        && matches!(
            asked,
            Some(
                LockReason::PayAtCounter
                    | LockReason::EndedByStaff
                    | LockReason::TimesUp
                    | LockReason::TimesUpSeeCounter
                    | LockReason::LoggedOut
            )
        );
    // A player who logged out shows no lock once the PC is clean: the attract screen.
    let (locked, shown) = if shown == Some(LockReason::LoggedOut) {
        (false, None)
    } else {
        (locked, shown)
    };
    let saved = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
        LOCKED_KEY,
        if locked { "1" } else { "0" },
        0,
    );
    if let Err(e) = saved {
        tracing::error!(error = %e, "could not save the lock state");
    }
    if ends || (locked && asked == Some(LockReason::Paused)) {
        ctx.pc.deadline.send_replace(None);
    }
    if ends {
        ctx.pc.clear_notes();
    }
    ctx.pc.screen.send_if_modified(|s| {
        let changed = s.locked != locked || s.reason != shown || (ends && s.session.is_some());
        s.locked = locked;
        s.reason = shown;
        if ends {
            s.session = None;
        }
        changed
    });
    follow_freeze(ctx, locked, shown);
}

/// Staff handles (Story 6.22): the time-up lock holds the game suspended behind it. It thaws on
/// an unlock or a new session (a staff Extend); a staff End lock keeps it frozen while the
/// cleanup runs, which ends the programs, and the lock shown afterwards thaws whatever is left.
fn follow_freeze(ctx: &Ctx, locked: bool, shown: Option<LockReason>) {
    let frozen = ctx.pc.frozen.load(Ordering::SeqCst);
    let want = locked
        && match shown {
            Some(LockReason::TimesUpSeeCounter) => true,
            Some(LockReason::PreparingPc) => frozen,
            _ => false,
        };
    if want == frozen {
        return;
    }
    let freezer = ctx
        .freezer
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(freezer) = freezer else {
        tracing::warn!(
            want,
            "process control is not running; programs not frozen or thawed"
        );
        return;
    };
    match freezer(want) {
        Ok(n) => tracing::info!(
            want,
            programs = n,
            "the player's programs were frozen or thawed"
        ),
        Err(e) => {
            tracing::warn!(want, error = %e, "could not freeze or thaw the player's programs");
            // Nothing changed as far as we know: a thaw is tried again at the next unlock, and a
            // freeze that could not list the programs is not remembered.
            return;
        }
    }
    ctx.pc.frozen.store(want, Ordering::SeqCst);
    let saved = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
        FROZEN_KEY,
        if want { "1" } else { "0" },
        0,
    );
    if let Err(e) = saved {
        tracing::error!(error = %e, "could not save the frozen state");
    }
}

pub fn apply(
    ctx: &Ctx,
    cmd_id: &str,
    action: PcAction,
    text: Option<String>,
    reason: Option<LockReason>,
) -> Step {
    if action.is_power() && !crate::power::supported() {
        tracing::warn!(
            action = action.as_str(),
            "power commands are not supported on this OS; refused"
        );
        return Step::Refused;
    }
    if !ctx.pc.first_time(cmd_id) {
        tracing::info!(
            cmd_id,
            action = action.as_str(),
            "a repeated pc command; acknowledged only"
        );
        return Step::Ack;
    }
    tracing::info!(cmd_id, action = action.as_str(), "pc command");
    match action {
        PcAction::Lock => set_locked(ctx, true, reason),
        PcAction::Unlock => set_locked(ctx, false, None),
        PcAction::Message => {
            let text = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
            let in_session = ctx.pc.screen.borrow().session.is_some();
            match text {
                Some(text) if in_session => {
                    ctx.pc.keep(text, ctx.clock.now_ms());
                }
                _ => tracing::info!(cmd_id, "a staff message with no session on screen; dropped"),
            }
        }
        PcAction::CleanupRetry => crate::cleanup::retry(ctx),
        PcAction::CleanupChecked => crate::cleanup::checked(ctx),
        PcAction::Restart | PcAction::Shutdown => return Step::AckThenPower(action),
    }
    Step::Ack
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PcState {
        let dir = tempfile::tempdir().unwrap();
        PcState::load(&QueueDb::open(&dir.path().join("queue.db")).unwrap())
    }

    #[test]
    fn a_cmd_id_is_acted_on_once() {
        let s = state();
        assert!(s.first_time("a"));
        assert!(!s.first_time("a"));
        for i in 0..SEEN_MAX {
            assert!(s.first_time(&format!("n{i}")));
        }
        assert!(s.first_time("a"), "the oldest ids age out");
    }

    #[test]
    fn messages_keep_newest_first_read_once_and_clear() {
        let s = state();
        let a = s.keep("one".into(), 10);
        let b = s.keep("two".into(), 20);
        assert_eq!((a.id, b.id), (1, 2));
        let texts = |s: &PcState| {
            s.staff_notes()
                .into_iter()
                .map(|n| n.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&s), ["one", "two"], "replayed oldest first");
        assert!(s.read_all());
        assert!(!s.read_all(), "nothing left unread");
        assert!(s.staff_notes().iter().all(|n| n.read));
        let c = s.keep("three".into(), 30);
        assert!(!c.read && c.id == 3);
        s.clear_notes();
        assert!(s.staff_notes().is_empty());
        assert_eq!(s.keep("four".into(), 40).id, 4, "ids keep counting");
    }

    #[test]
    fn the_inbox_keeps_only_the_newest_messages() {
        let s = state();
        for i in 0..INBOX_MAX + 5 {
            s.keep(format!("m{i}"), i as i64);
        }
        let notes = s.staff_notes();
        assert_eq!(notes.len(), INBOX_MAX);
        assert_eq!(notes[0].text, "m5");
        assert_eq!(notes[INBOX_MAX - 1].text, format!("m{}", INBOX_MAX + 4));
    }

    #[test]
    fn the_lock_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert!(!PcState::load(&q).screen.borrow().locked);
        q.kv_set(LOCKED_KEY, "1", 0).unwrap();
        assert!(PcState::load(&q).screen.borrow().locked);
    }
}

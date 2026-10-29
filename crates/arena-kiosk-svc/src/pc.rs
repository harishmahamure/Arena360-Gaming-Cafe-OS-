use std::collections::{BTreeMap, VecDeque};
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
    /// How many ordered lines are not served yet (Story 6.33).
    pub unserved_items: u64,
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

/// The notes the kiosk raises for an order (Story 6.33); the app words each from its own strings.
pub const NOTE_ORDER_PLACED: &str = "order.placed";
pub const NOTE_ORDER_READY_SEAT: &str = "order.ready_seat";
pub const NOTE_ORDER_READY_COUNTER: &str = "order.ready_counter";
pub const NOTE_ORDER_CANCELLED: &str = "order.cancelled";

/// One staff message of the session on screen (Story 6.16), or an order note (Story 6.33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffNote {
    /// This service's own counter, so the app can tell a replay from a new message.
    pub id: u64,
    /// The staff's text; for an order note, the item's name (cancelled) or empty.
    pub text: String,
    /// When it arrived, UTC ms.
    pub at: i64,
    pub read: bool,
    /// Set for an order note: which one (`NOTE_ORDER_*`).
    pub code: Option<&'static str>,
    /// For `order.placed`, the items just ordered.
    pub count: u64,
}

/// What the kiosk last knew of one open order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeenOrder {
    items: u64,
    ready: bool,
}

/// The order note one tick raises for `order` given what was last seen of it, if any.
fn order_note(
    prev: Option<SeenOrder>,
    order: &arena_proto::OrderStatus,
) -> Option<(&'static str, u64)> {
    let ready = |o: &arena_proto::OrderStatus| match o.ready.as_deref() {
        Some("seat") => (NOTE_ORDER_READY_SEAT, 0),
        _ => (NOTE_ORDER_READY_COUNTER, 0),
    };
    match prev {
        None if order.ready.is_some() => Some(ready(order)),
        None => (order.items > 0).then_some((NOTE_ORDER_PLACED, order.items)),
        Some(p) if order.ready.is_some() && (!p.ready || order.items > p.items) => {
            Some(ready(order))
        }
        Some(p) if order.items > p.items => Some((NOTE_ORDER_PLACED, order.items - p.items)),
        Some(_) => None,
    }
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
    /// The open orders the last tick carried, by id (Story 6.33).
    orders: Mutex<BTreeMap<String, SeenOrder>>,
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
            orders: Mutex::default(),
            notes: broadcast::channel(16).0,
        }
    }

    fn inbox(&self) -> std::sync::MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Keeps a message at the front of the list and tells the supervisor.
    fn keep(&self, text: String, at: i64) -> StaffNote {
        self.keep_note(text, at, None, 0)
    }

    fn keep_note(
        &self,
        text: String,
        at: i64,
        code: Option<&'static str>,
        count: u64,
    ) -> StaffNote {
        let note = {
            let mut inbox = self.inbox();
            inbox.last_id += 1;
            let note = StaffNote {
                id: inbox.last_id,
                text,
                at,
                read: false,
                code,
                count,
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

    /// Forgets the session's messages and what it knew of its orders (it ended, or a new one
    /// begins).
    pub fn clear_notes(&self) {
        self.inbox().notes.clear();
        self.orders
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// Compares a tick's open orders with the last it saw and raises at most one note per order
    /// (Story 6.33): "ordered" when items were added, "ready" when staff marked it ready. An
    /// unchanged order raises nothing; the first tick after a restart raises one note per order
    /// from its latest state. Nothing is replayed for ticks missed while the Hub was away.
    pub fn track_orders(&self, orders: &[arena_proto::OrderStatus], at: i64) {
        let notes: Vec<_> = {
            let mut seen = self.orders.lock().unwrap_or_else(|p| p.into_inner());
            let notes = orders
                .iter()
                .filter_map(|o| order_note(seen.get(&o.order_id).copied(), o))
                .collect();
            *seen = orders
                .iter()
                .map(|o| {
                    (
                        o.order_id.clone(),
                        SeenOrder {
                            items: o.items,
                            ready: o.ready.is_some(),
                        },
                    )
                })
                .collect();
            notes
        };
        for (code, count) in notes {
            self.keep_note(String::new(), at, Some(code), count);
        }
    }

    /// Raises "<item> was cancelled" for an unserved line the Hub cancelled.
    pub fn order_cancelled(&self, item: String, at: i64) {
        self.keep_note(item, at, Some(NOTE_ORDER_CANCELLED), 0);
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

    fn order(id: &str, items: u64, unserved: u64, ready: Option<&str>) -> arena_proto::OrderStatus {
        arena_proto::OrderStatus {
            order_id: id.into(),
            items,
            unserved,
            ready: ready.map(Into::into),
        }
    }

    fn codes(s: &PcState) -> Vec<(Option<&'static str>, u64)> {
        s.staff_notes()
            .into_iter()
            .map(|n| (n.code, n.count))
            .collect()
    }

    #[test]
    fn a_new_order_a_ready_mark_and_more_items_each_raise_one_note() {
        let s = state();
        s.track_orders(&[order("A", 3, 1, None)], 1);
        assert_eq!(codes(&s), [(Some(NOTE_ORDER_PLACED), 3)]);
        // Unchanged state raises nothing, and a served line only drops the HUD tag.
        s.track_orders(&[order("A", 3, 1, None)], 2);
        s.track_orders(&[order("A", 3, 0, None)], 3);
        assert_eq!(s.staff_notes().len(), 1);
        s.track_orders(&[order("A", 3, 1, Some("seat"))], 4);
        s.track_orders(&[order("A", 3, 1, Some("seat"))], 5);
        // More items clear the mark at the Hub: a note for the new items only.
        s.track_orders(&[order("A", 5, 2, None)], 6);
        s.track_orders(&[order("A", 5, 2, Some("counter"))], 7);
        assert_eq!(
            codes(&s),
            [
                (Some(NOTE_ORDER_PLACED), 3),
                (Some(NOTE_ORDER_READY_SEAT), 0),
                (Some(NOTE_ORDER_PLACED), 2),
                (Some(NOTE_ORDER_READY_COUNTER), 0),
            ]
        );
    }

    #[test]
    fn a_restart_or_reconnect_raises_one_note_per_order_from_its_latest_state() {
        let s = state();
        s.track_orders(&[order("A", 2, 1, Some("seat")), order("B", 4, 0, None)], 1);
        assert_eq!(
            codes(&s),
            [
                (Some(NOTE_ORDER_READY_SEAT), 0),
                (Some(NOTE_ORDER_PLACED), 4)
            ]
        );
        // A tick that grew an order while the Hub was away is one note, not one per change.
        s.track_orders(&[order("A", 9, 3, Some("seat")), order("B", 4, 0, None)], 2);
        assert_eq!(s.staff_notes().len(), 3);
        // Cancelled lines and settled orders raise nothing from a tick.
        s.track_orders(&[order("A", 8, 2, Some("seat"))], 3);
        s.track_orders(&[], 4);
        assert_eq!(s.staff_notes().len(), 3);
        // Ending the session forgets the orders: the next player's order is new again.
        s.clear_notes();
        s.track_orders(&[order("A", 8, 2, None)], 5);
        assert_eq!(codes(&s), [(Some(NOTE_ORDER_PLACED), 8)]);
    }

    #[test]
    fn a_cancelled_line_is_a_note_with_the_item_name() {
        let s = state();
        s.order_cancelled("Burger".into(), 9);
        let n = s.staff_notes().remove(0);
        assert_eq!(
            (n.code, n.text.as_str(), n.count),
            (Some(NOTE_ORDER_CANCELLED), "Burger", 0)
        );
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

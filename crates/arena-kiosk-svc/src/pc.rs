use std::collections::VecDeque;
use std::sync::Mutex;

use arena_proto::{LockReason, PcAction};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::queue::QueueDb;
use crate::runtime::Ctx;

const LOCKED_KEY: &str = "pc.locked";
const SEEN_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Screen {
    pub locked: bool,
    pub reason: Option<LockReason>,
    pub message: Option<String>,
    pub session: Option<ScreenSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSession {
    pub device_name: String,
    pub started: Instant,
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
}

impl PcState {
    pub fn load(queue: &QueueDb) -> Self {
        let locked = matches!(queue.kv_get(LOCKED_KEY), Ok(Some((v, _))) if v == "1");
        Self {
            screen: watch::channel(Screen {
                locked,
                ..Screen::default()
            })
            .0,
            deadline: watch::channel(None).0,
            seen: Mutex::default(),
        }
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
    let saved = ctx.queue.lock().unwrap_or_else(|p| p.into_inner()).kv_set(
        LOCKED_KEY,
        if locked { "1" } else { "0" },
        0,
    );
    if let Err(e) = saved {
        tracing::error!(error = %e, "could not save the lock state");
    }
    let ends = locked
        && matches!(
            reason,
            Some(LockReason::PayAtCounter | LockReason::EndedByStaff | LockReason::TimesUp)
        );
    if ends || (locked && reason == Some(LockReason::Paused)) {
        ctx.pc.deadline.send_replace(None);
    }
    ctx.pc.screen.send_if_modified(|s| {
        let changed = s.locked != locked || s.reason != reason || (ends && s.session.is_some());
        s.locked = locked;
        s.reason = reason;
        if ends {
            s.session = None;
        }
        changed
    });
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
            let message = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
            ctx.pc.screen.send_modify(|s| s.message = message);
        }
        PcAction::Restart | PcAction::Shutdown => return Step::AckThenPower(action),
    }
    Step::Ack
}

pub fn dismiss(ctx: &Ctx) {
    ctx.pc
        .screen
        .send_if_modified(|s| s.message.take().is_some());
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
    fn the_lock_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
        assert!(!PcState::load(&q).screen.borrow().locked);
        q.kv_set(LOCKED_KEY, "1", 0).unwrap();
        assert!(PcState::load(&q).screen.borrow().locked);
    }
}

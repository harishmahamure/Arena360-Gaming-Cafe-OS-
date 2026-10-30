//! When a running session asks for a player message (Story 8.30).
//!
//! [`triggers`] looks at every running session and says which messages to queue now:
//!
//! * `low_balance`: a profiled player's wallet session whose remaining balance (the free wallet
//!   plus the unused part of the share the session set aside) has fallen to the branch's
//!   low-balance level. It is one message per crossing: a later top-up above the level is
//!   reported as a [`Trigger::Rearm`], after which the next fall is a new crossing.
//! * `session_ending`: a PC session with an end time that is five minutes or less from it.
//! * `console_5min`: the same for a console session, naming the TV.
//!
//! Nothing here writes. A guest, a player without current consent, a Free branch and a message
//! switched off yield nothing; the task applies each trigger as a `notify.requested` or
//! `notify.rearm` system intent, which checks the same rules again and refuses a repeat.

use std::collections::{BTreeMap, HashSet};

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, KEY_BILLING_LOW_BALANCE, KEY_LICENCE_TIER,
    KEY_NOTIFY_CREDIT_REMINDER, KEY_NOTIFY_LOW_BALANCE, KEY_NOTIFY_SESSION_ENDING,
    NOTIFY_KIND_CONSOLE_5MIN, NOTIFY_KIND_CREDIT_REMINDER, NOTIFY_KIND_LOW_BALANCE,
    NOTIFY_KIND_SESSION_ENDING, NotifyRequested, SESSION_NEAR_END_MS, low_balance_from,
    whatsapp_refusal,
};
use arena_store::{LiveSession, SettingRow, Store};

use crate::SnapshotReader;
use crate::session_screen::wallet_left;

/// The branch setting that switches a message kind on or off. "Session ending" also covers the
/// console's five-minute message: a branch has one switch for both.
pub fn switch_key(kind: &str) -> Option<&'static str> {
    match kind {
        NOTIFY_KIND_LOW_BALANCE => Some(KEY_NOTIFY_LOW_BALANCE),
        NOTIFY_KIND_CREDIT_REMINDER => Some(KEY_NOTIFY_CREDIT_REMINDER),
        NOTIFY_KIND_SESSION_ENDING | NOTIFY_KIND_CONSOLE_5MIN => Some(KEY_NOTIFY_SESSION_ENDING),
        _ => None,
    }
}

/// The raw values a `notify.requested` of this kind carries, by name.
pub fn param_names(kind: &str) -> &'static [&'static str] {
    match kind {
        NOTIFY_KIND_LOW_BALANCE | NOTIFY_KIND_CREDIT_REMINDER => &["amount_minor"],
        NOTIFY_KIND_SESSION_ENDING => &["minutes"],
        NOTIFY_KIND_CONSOLE_5MIN => &["console"],
        _ => &[],
    }
}

/// What the Hub should record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// Queue this message.
    Request(NotifyRequested),
    /// The wallet is above the level again: the `low_balance` row with this id is over.
    Rearm(String),
}

fn value<'a>(settings: &'a [SettingRow], key: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.value.as_str())
}

fn request(kind: &str, s: &LiveSession, player_id: &str, param: (&str, String)) -> Trigger {
    Trigger::Request(NotifyRequested {
        kind: kind.to_owned(),
        player_id: player_id.to_owned(),
        session_id: Some(s.id.clone()),
        params: BTreeMap::from([(param.0.to_owned(), param.1)]),
    })
}

/// The messages to queue (and crossings to close) at `now_ms`.
pub fn triggers(store: &Store, now_ms: i64) -> Result<Vec<Trigger>, AppError> {
    let (sessions, settings, metas) =
        store.read_consistent(|s| Ok((s.live_sessions()?, s.settings()?, s.devices_meta()?)))?;
    let tier = value(&settings, KEY_LICENCE_TIER);
    let ending_on = value(&settings, KEY_NOTIFY_SESSION_ENDING) == Some("true");
    let low_on = value(&settings, KEY_NOTIFY_LOW_BALANCE) == Some("true");
    let level = low_balance_from(value(&settings, KEY_BILLING_LOW_BALANCE));
    let mut out = Vec::new();
    let mut seen_players = HashSet::new();
    for s in sessions
        .iter()
        .filter(|s| matches!(s.state.as_str(), "active" | "paused" | "grace"))
    {
        let Some(player_id) = &s.player_id else {
            continue;
        };
        let consent = store
            .latest_consent(player_id, CONSENT_TRANSACTIONAL)?
            .map(|c| c.0);
        let allowed = |on: bool| whatsapp_refusal(tier, on, true, consent).is_none();
        let remaining = s.ends_at.map(|at| at - s.paused_at.unwrap_or(now_ms));

        if s.mode == "wallet" && seen_players.insert(player_id.clone()) {
            let free = (store.wallet_balance(player_id)? - store.reserved_minor(player_id)?).max(0);
            let left = wallet_left(free, s, remaining.map(|r| u64::try_from(r).unwrap_or(0)));
            let last = store.latest_low_balance(player_id)?;
            if level.is_some_and(|level| left <= level) {
                // A new crossing: the last row (if any) was closed by a wallet above the level.
                if allowed(low_on) && last.is_none_or(|(_, rearmed)| rearmed.is_some()) {
                    out.push(request(
                        NOTIFY_KIND_LOW_BALANCE,
                        s,
                        player_id,
                        ("amount_minor", left.to_string()),
                    ));
                }
            } else if let Some((id, None)) = last {
                out.push(Trigger::Rearm(id));
            }
        }

        let Some(left_ms) = remaining.filter(|r| (1..=SESSION_NEAR_END_MS).contains(r)) else {
            continue;
        };
        if s.state == "grace" || !allowed(ending_on) {
            continue;
        }
        let console = metas
            .iter()
            .find(|m| m.device_id == s.device_id && m.deleted_at.is_none())
            .filter(|m| m.kind == "console");
        let (kind, param) = match console {
            Some(m) => (NOTIFY_KIND_CONSOLE_5MIN, ("console", m.label.clone())),
            None => (
                NOTIFY_KIND_SESSION_ENDING,
                ("minutes", ((left_ms + 59_999) / 60_000).to_string()),
            ),
        };
        if !store.session_notice_exists(&s.id, kind)? {
            out.push(request(kind, s, player_id, param));
        }
    }
    Ok(out)
}

impl SnapshotReader {
    /// The session messages to queue at `now_ms` (see [`triggers`]).
    pub fn notify_triggers(&self, now_ms: i64) -> Result<Vec<Trigger>, AppError> {
        self.with_store(|store| triggers(store, now_ms))
    }
}

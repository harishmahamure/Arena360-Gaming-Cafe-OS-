//! The daily credit check (Story 8.31).
//!
//! From 11:00 branch time, [`due`] names every player who is to be reminded of unpaid credit
//! today: a consenting profiled player with credit outstanding whose oldest unpaid spend (first in,
//! first out, as the aging in Story 3.20 counts it) is exactly as many days old as one of their
//! reminder cycle days (default 3, 7, 14). Nothing here writes: the task applies each result as a
//! `notify.requested` system intent, which checks the tier, the "Credit reminder" switch, the
//! consent and "already reminded today" again.
//!
//! "Once per day" is the outbox's own rule (one `credit_reminder` row per player per business
//! day), so the check may run as often as it likes. A Hub that was off at 11:00 and starts later
//! the same business day finds the same answer and queues the same reminders; a day it missed
//! altogether is never made up, because only today's age is compared with the cycle days.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, CREDIT_REMINDER_MINUTE, KEY_LICENCE_TIER,
    KEY_NOTIFY_CREDIT_REMINDER, NOTIFY_KIND_CREDIT_REMINDER, NotifyRequested, whatsapp_refusal,
};
use arena_store::Store;
use chrono::Timelike;

use crate::SnapshotReader;
use crate::locale::{locale_rows, resolve};

/// Whether the daily check has started at `now_ms`: it is 11:00 or later on the branch's clock, or
/// the business day is still the previous date (a day that starts after midnight).
fn started(store: &Store, now_ms: i64) -> Result<Option<String>, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(None);
    };
    let rows = locale_rows(&branch, &store.settings()?);
    let (day, locale) = resolve(&rows, now_ms);
    let local = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| AppError::new(arena_proto::ErrorCode::Internal, "time out of range"))?
        .with_timezone(&locale.tz)
        .naive_local();
    let minute = local.hour() * 60 + local.minute();
    let late_night = local.date() > day;
    Ok((minute >= CREDIT_REMINDER_MINUTE || late_night).then(|| day.to_string()))
}

/// The credit reminders to queue at `now_ms`. Empty before 11:00, on a Free branch and with the
/// "Credit reminder" switch off.
pub fn due(store: &Store, now_ms: i64) -> Result<Vec<NotifyRequested>, AppError> {
    let Some(today) = started(store, now_ms)? else {
        return Ok(Vec::new());
    };
    let settings = store.settings()?;
    let setting = |key: &str| {
        settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.as_str())
    };
    let tier = setting(KEY_LICENCE_TIER);
    let on = setting(KEY_NOTIFY_CREDIT_REMINDER) == Some("true");
    let mut out = Vec::new();
    for credit in store.credit_debtors()? {
        if credit.outstanding_minor <= 0 {
            continue;
        }
        let aging = store.credit_aging(&credit.player_id, &today)?;
        let on_cycle_day = aging.oldest_unpaid_day.is_some()
            && u16::try_from(aging.oldest_age_days)
                .is_ok_and(|d| credit.reminder_days.contains(&d));
        if !on_cycle_day {
            continue;
        }
        let consent = store
            .latest_consent(&credit.player_id, CONSENT_TRANSACTIONAL)?
            .map(|c| c.0);
        if whatsapp_refusal(tier, on, true, consent).is_some()
            || store.credit_reminder_exists(&credit.player_id, &today)?
        {
            continue;
        }
        out.push(NotifyRequested {
            kind: NOTIFY_KIND_CREDIT_REMINDER.to_owned(),
            player_id: credit.player_id,
            session_id: None,
            params: BTreeMap::from([(
                "amount_minor".to_owned(),
                credit.outstanding_minor.to_string(),
            )]),
        });
    }
    Ok(out)
}

impl SnapshotReader {
    /// The credit reminders to queue at `now_ms` (see [`due`]).
    pub fn credit_reminders(&self, now_ms: i64) -> Result<Vec<NotifyRequested>, AppError> {
        self.with_store(|store| due(store, now_ms))
    }
}

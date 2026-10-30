//! `POST /v1/notify` (Story 8.28): a paid Hub asks the cloud to send one player message. The
//! cloud sends it from Arena 360's own account, once per `outbox_id`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// The message kinds the cloud has templates for.
pub const MESSAGE_KINDS: [&str; 5] = [
    "credit_reminder",
    "low_balance",
    "session_ending",
    "console_5min",
    "receipt",
];

/// The longest value of one template parameter, in characters.
pub const NOTIFY_PARAM_MAX_CHARS: usize = 200;

/// What the Hub sends. `outbox_id` is the `notify_outbox` row's ULID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyRequest {
    pub outbox_id: String,
    pub kind: String,
    pub purpose: String,
    pub consent_policy_version: String,
    /// The player's phone, E.164 (`+919876543210`).
    pub recipient: String,
    /// The template's parameters by name, all text.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// Where a send stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum NotifyState {
    /// Not sent yet, or sent with an unknown outcome. The cloud never sends a row twice.
    Queued,
    Sent,
    Failed,
}

/// How a message went (or goes) out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum NotifyChannel {
    Whatsapp,
    Sms,
}

/// The ledger's answer, the same for every post of one `outbox_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyResponse {
    pub outbox_id: String,
    pub state: NotifyState,
    pub channel: NotifyChannel,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provider_message_id: Option<String>,
    /// Why it failed, for `notify.failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_code: Option<String>,
}

/// How long a queued message may wait for the cloud before it is marked `expired` (Story 8.29).
pub const NOTIFY_EXPIRY_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How often a paid Hub looks for due messages, in seconds.
pub const NOTIFY_DRAIN_INTERVAL_S: u64 = 10;
/// A `notify_outbox` row the cloud sent.
pub const NOTIFY_STATE_SENT: &str = "sent";
/// A `notify_outbox` row the cloud could not send.
pub const NOTIFY_STATE_FAILED: &str = "failed";
/// A `notify_outbox` row that waited more than [`NOTIFY_EXPIRY_MS`].
pub const NOTIFY_STATE_EXPIRED: &str = "expired";
/// The message kind of a wallet that ran low (Story 8.30).
pub const NOTIFY_KIND_LOW_BALANCE: &str = "low_balance";
/// The message kind of a PC session five minutes from its end (Story 8.30).
pub const NOTIFY_KIND_SESSION_ENDING: &str = "session_ending";
/// The message kind of a console session five minutes from its end (Story 8.30).
pub const NOTIFY_KIND_CONSOLE_5MIN: &str = "console_5min";
/// The message kind of a player's unpaid credit (Story 8.31).
pub const NOTIFY_KIND_CREDIT_REMINDER: &str = "credit_reminder";
/// The minute of the branch's local day (11:00) at which the daily credit check starts.
pub const CREDIT_REMINDER_MINUTE: u32 = 11 * 60;
/// How often a primary Hub looks for credit reminders to queue, in seconds. The check itself is
/// once per business day per player; the interval only bounds how late a Hub that was off at 11:00
/// catches up.
pub const CREDIT_REMINDER_INTERVAL_S: u64 = 300;
/// The kinds a running session raises through `notify.requested`.
pub const SESSION_NOTIFY_KINDS: [&str; 3] = [
    NOTIFY_KIND_LOW_BALANCE,
    NOTIFY_KIND_SESSION_ENDING,
    NOTIFY_KIND_CONSOLE_5MIN,
];
/// How close to its end a session is when "session ending" and "console 5 minutes" go out.
pub const SESSION_NEAR_END_MS: i64 = 5 * 60_000;
/// How often a Hub looks for session messages to queue, in seconds.
pub const NOTIFY_TRIGGER_INTERVAL_S: u64 = 10;

/// The payload of `notify.sent` (system): the cloud sent the row's message. The forward action is
/// now an irreversible effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifySent {
    pub outbox_id: String,
    pub channel: NotifyChannel,
}

/// The payload of `notify.failed` (system): the cloud could not send the row's message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyFailed {
    pub outbox_id: String,
    pub code: String,
}

/// The payload of `notify.skip` (system): the row's message is never sent, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifySkipped {
    pub outbox_id: String,
    pub reason: String,
}

/// The payload of `notify.expire` (system): the row waited more than 7 days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyExpired {
    pub outbox_id: String,
}

/// The payload of `notify.requested` (system, Stories 8.30 and 8.31): a running session, a wallet
/// that ran low during one, or the daily credit check asks for a player message. It queues one
/// `notify_outbox` row for the end of the Undo window. `params` holds the raw values the drain turns
/// into template parameters: `amount_minor` for `low_balance` and `credit_reminder`, `minutes` for
/// `session_ending`, `console` (the TV name) for `console_5min`. `session_id` names the session of
/// the three session messages and is absent for a `credit_reminder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyRequested {
    pub kind: String,
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// The payload of `notify.rearm` (system, Story 8.30): the player's wallet rose above the
/// low-balance level again, so the next fall below it is a new crossing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NotifyRearm {
    pub outbox_id: String,
}

/// Whether `s` is a phone in E.164: `+`, then 8 to 15 digits, the first not 0.
pub fn valid_e164(s: &str) -> bool {
    let Some(d) = s.strip_prefix('+') else {
        return false;
    };
    (8..=15).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()) && !d.starts_with('0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e164_shapes() {
        assert!(valid_e164("+919876543210"));
        assert!(!valid_e164("919876543210"));
        assert!(!valid_e164("+0919876543"));
        assert!(!valid_e164("+91 98765"));
        assert!(!valid_e164("+1234567"));
    }

    #[test]
    fn request_refuses_unknown_fields() {
        let r = serde_json::from_str::<NotifyRequest>(
            r#"{"outbox_id":"a","kind":"receipt","purpose":"transactional","consent_policy_version":"1","recipient":"+919876543210","extra":1}"#,
        );
        assert!(r.is_err());
    }
}

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

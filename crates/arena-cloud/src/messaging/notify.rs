//! The player-message adapter (Story 8.28): one template message goes out on WhatsApp or SMS
//! through [`Notifier`]. The Gupshup WhatsApp template API is the live provider; the recorded
//! [`FakeNotifier`] serves dev and CI, and `manual` mode logs the rendered message for operators
//! instead of sending. A failure says whether the message definitely did not go out
//! ([`SendFailure::Rejected`] when retrying cannot help, [`SendFailure::Transient`] when it may),
//! or whether delivery is unknown ([`SendFailure::Uncertain`]).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use super::{GupshupConfig, GupshupSms, MAX_REPLY_BYTES};
use crate::Clock;
use crate::config::get_env;

/// How long one WhatsApp send may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// The Gupshup WhatsApp template endpoint, unless `ARENA_GUPSHUP_WA_ENDPOINT` says otherwise.
const DEFAULT_WA_ENDPOINT: &str = "https://api.gupshup.io/wa/api/v1/template/msg";

pub const NOTIFY_MODE_ENV: &str = "ARENA_NOTIFY_MODE";
pub const WA_ENDPOINT_ENV: &str = "ARENA_GUPSHUP_WA_ENDPOINT";
pub const WA_API_KEY_ENV: &str = "ARENA_GUPSHUP_WA_API_KEY";
pub const WA_SOURCE_ENV: &str = "ARENA_GUPSHUP_WA_SOURCE";
pub const WA_APP_ENV: &str = "ARENA_GUPSHUP_WA_APP";

/// Which way a message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Whatsapp,
    Sms,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Whatsapp => "whatsapp",
            Self::Sms => "sms",
        }
    }
}

/// One message, ready to send on either channel.
#[derive(Debug, Clone, Copy)]
pub struct Outgoing<'a> {
    pub outbox_id: &'a str,
    /// E.164.
    pub to: &'a str,
    pub whatsapp_template_id: &'a str,
    pub dlt_template_id: &'a str,
    /// The template parameters in WhatsApp order.
    pub params: &'a [String],
    /// The SMS text, rendered.
    pub sms_text: &'a str,
}

/// Why a message did not go out. `code` is stable and never carries the number or the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendFailure {
    /// Not sent, and the provider will not take it as it is.
    Rejected { code: String },
    /// Not sent; trying again later may work.
    Transient { code: String },
    /// It may have been delivered.
    Uncertain { code: String },
}

impl SendFailure {
    pub fn code(&self) -> &str {
        match self {
            Self::Rejected { code } | Self::Transient { code } | Self::Uncertain { code } => code,
        }
    }
}

/// What the recorded fake does on a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    Deliver,
    Reject,
    Transient,
    Uncertain,
}

/// One message the fake recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentMessage {
    pub outbox_id: String,
    pub channel: Channel,
    pub to: String,
    /// The WhatsApp template id, or the DLT template id for SMS.
    pub template_id: String,
    pub params: Vec<String>,
    pub text: String,
    /// Unix ms.
    pub at: i64,
}

/// The recorded fake: captures what would be sent; nothing leaves the process.
pub struct FakeNotifier {
    sent: Mutex<Vec<SentMessage>>,
    behaviour: Mutex<HashMap<&'static str, Behaviour>>,
    clock: Clock,
}

impl FakeNotifier {
    pub fn new(clock: Clock) -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
            behaviour: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// Every message sent so far, oldest first.
    pub fn sent(&self) -> Vec<SentMessage> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Sets what later sends on `channel` do.
    pub fn set_behaviour(&self, channel: Channel, behaviour: Behaviour) {
        if let Ok(mut b) = self.behaviour.lock() {
            b.insert(channel.as_str(), behaviour);
        }
    }

    fn send(&self, channel: Channel, m: &Outgoing<'_>) -> Result<String, SendFailure> {
        let behaviour = self
            .behaviour
            .lock()
            .ok()
            .and_then(|b| b.get(channel.as_str()).copied())
            .unwrap_or(Behaviour::Deliver);
        let code = |c: &str| c.to_owned();
        match behaviour {
            Behaviour::Reject => {
                return Err(SendFailure::Rejected {
                    code: code("FAKE_REJECTED"),
                });
            }
            Behaviour::Transient => {
                return Err(SendFailure::Transient {
                    code: code("FAKE_UNAVAILABLE"),
                });
            }
            Behaviour::Uncertain => {
                return Err(SendFailure::Uncertain {
                    code: code("FAKE_TIMEOUT"),
                });
            }
            Behaviour::Deliver => {}
        }
        let mut sent = self.sent.lock().map_err(|_| SendFailure::Transient {
            code: code("FAKE_POISONED"),
        })?;
        sent.push(SentMessage {
            outbox_id: m.outbox_id.to_owned(),
            channel,
            to: m.to.to_owned(),
            template_id: match channel {
                Channel::Whatsapp => m.whatsapp_template_id.to_owned(),
                Channel::Sms => m.dlt_template_id.to_owned(),
            },
            params: m.params.to_vec(),
            text: m.sms_text.to_owned(),
            at: (self.clock)(),
        });
        Ok(format!("fake-{}", sent.len()))
    }
}

/// `ARENA_GUPSHUP_WA_*`.
#[derive(Clone, PartialEq, Eq)]
pub struct WhatsappConfig {
    pub endpoint: String,
    pub api_key: String,
    /// The business number the messages come from.
    pub source: String,
    pub app_name: String,
}

impl std::fmt::Debug for WhatsappConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhatsappConfig")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[redacted]")
            .field("source", &self.source)
            .field("app_name", &self.app_name)
            .finish()
    }
}

/// Gupshup: WhatsApp template messages, and SMS through the DLT-registered account.
pub struct GupshupNotifier {
    wa: WhatsappConfig,
    sms: GupshupSms,
    client: reqwest::Client,
}

impl GupshupNotifier {
    pub fn new(wa: WhatsappConfig, sms: &GupshupConfig) -> Result<Self, String> {
        Ok(Self {
            client: crate::alert::client(SEND_TIMEOUT)?,
            sms: GupshupSms::new(sms.clone()).map_err(|e| e.to_string())?,
            wa,
        })
    }

    /// The form fields of one WhatsApp template send.
    pub fn whatsapp_form(&self, m: &Outgoing<'_>) -> Vec<(&'static str, String)> {
        let template = serde_json::json!({"id": m.whatsapp_template_id, "params": m.params});
        vec![
            ("channel", "whatsapp".to_owned()),
            ("source", self.wa.source.clone()),
            ("destination", m.to.trim_start_matches('+').to_owned()),
            ("src.name", self.wa.app_name.clone()),
            ("template", template.to_string()),
        ]
    }

    async fn whatsapp(&self, m: &Outgoing<'_>) -> Result<String, SendFailure> {
        let mut res = self
            .client
            .post(&self.wa.endpoint)
            .header("apikey", &self.wa.api_key)
            .form(&self.whatsapp_form(m))
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    SendFailure::Transient {
                        code: "WHATSAPP_UNREACHABLE".into(),
                    }
                } else {
                    SendFailure::Uncertain {
                        code: "WHATSAPP_TIMEOUT".into(),
                    }
                }
            })?;
        let status = res.status();
        let mut body = Vec::new();
        while body.len() < MAX_REPLY_BYTES {
            match res.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(_) => {
                    return Err(SendFailure::Uncertain {
                        code: "WHATSAPP_REPLY_UNREADABLE".into(),
                    });
                }
            }
        }
        body.truncate(MAX_REPLY_BYTES);
        if status.as_u16() == 429 {
            return Err(SendFailure::Transient {
                code: "WHATSAPP_RATE_LIMITED".into(),
            });
        }
        if status.is_server_error() {
            // The provider may have queued it before failing.
            return Err(SendFailure::Uncertain {
                code: format!("WHATSAPP_HTTP_{}", status.as_u16()),
            });
        }
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let id = json.get("messageId").and_then(|v| v.as_str());
        match (
            status.is_success(),
            json.get("status").and_then(|v| v.as_str()),
            id,
        ) {
            (true, Some("submitted"), Some(id)) if !id.is_empty() => Ok(id.to_owned()),
            _ => Err(SendFailure::Rejected {
                code: format!("WHATSAPP_HTTP_{}", status.as_u16()),
            }),
        }
    }

    async fn text(&self, m: &Outgoing<'_>) -> Result<String, SendFailure> {
        match self
            .sms
            .send_with_template(m.to, m.sms_text, m.dlt_template_id)
            .await
        {
            Ok(()) => Ok("sms".to_owned()),
            Err(e) if e.uncertain => Err(SendFailure::Uncertain {
                code: "SMS_SEND_UNCERTAIN".into(),
            }),
            Err(_) => Err(SendFailure::Rejected {
                code: "SMS_SEND_FAILED".into(),
            }),
        }
    }
}

/// The configured way messages go out.
pub enum Notifier {
    Fake(FakeNotifier),
    /// Logs the rendered message for an operator to send by hand.
    Manual,
    Gupshup(Box<GupshupNotifier>),
}

impl Notifier {
    /// `fake`, `manual` or `gupshup`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Fake(_) => "fake",
            Self::Manual => "manual",
            Self::Gupshup(_) => "gupshup",
        }
    }

    /// The recorded fake, when that is the adapter.
    pub fn fake(&self) -> Option<&FakeNotifier> {
        match self {
            Self::Fake(f) => Some(f),
            _ => None,
        }
    }

    /// Production: `ARENA_NOTIFY_MODE=manual` logs messages; otherwise the Gupshup WhatsApp
    /// variables are required (the SMS fallback uses `sms`, the account that sends codes).
    pub fn from_env(
        sms: &GupshupConfig,
        env: &impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, String> {
        match get_env(env, NOTIFY_MODE_ENV).as_deref() {
            Some("manual") => return Ok(Self::Manual),
            None | Some("") | Some("gupshup") => {}
            Some(_) => return Err(format!("{NOTIFY_MODE_ENV} must be gupshup or manual")),
        }
        let req = |k: &str| {
            get_env(env, k)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{k} is not set"))
        };
        let endpoint = get_env(env, WA_ENDPOINT_ENV)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_WA_ENDPOINT.to_owned());
        if !endpoint.starts_with("https://") {
            return Err(format!("{WA_ENDPOINT_ENV} must be an https:// URL"));
        }
        let wa = WhatsappConfig {
            endpoint,
            api_key: req(WA_API_KEY_ENV)?,
            source: req(WA_SOURCE_ENV)?,
            app_name: req(WA_APP_ENV)?,
        };
        Ok(Self::Gupshup(Box::new(GupshupNotifier::new(wa, sms)?)))
    }

    /// Sends `m` on `channel`; answers the provider's message id.
    pub async fn send(&self, channel: Channel, m: &Outgoing<'_>) -> Result<String, SendFailure> {
        match self {
            Self::Fake(f) => f.send(channel, m),
            Self::Manual => {
                // Operator mode: the one place the text is logged, because the operator sends it.
                tracing::info!(
                    outbox_id = m.outbox_id,
                    channel = channel.as_str(),
                    to = m.to,
                    text = m.sms_text,
                    "notify (manual): send this message by hand"
                );
                Ok(format!("manual-{}", m.outbox_id))
            }
            Self::Gupshup(g) => match channel {
                Channel::Whatsapp => g.whatsapp(m).await,
                Channel::Sms => g.text(m).await,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn sms() -> GupshupConfig {
        GupshupConfig {
            endpoint: "https://enterprise.smsgupshup.com/GatewayAPI/rest".into(),
            userid: "u".into(),
            password: "p".into(),
            sender: "ARENAX".into(),
            dlt_template_id: "dlt".into(),
        }
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    const WA: [(&str, &str); 3] = [
        (WA_API_KEY_ENV, "secret-key"),
        (WA_SOURCE_ENV, "919000000000"),
        (WA_APP_ENV, "Arena360"),
    ];

    #[test]
    fn production_needs_the_whatsapp_variables_unless_manual() {
        assert!(matches!(
            Notifier::from_env(&sms(), &env(&WA)),
            Ok(Notifier::Gupshup(_))
        ));
        let e = Notifier::from_env(&sms(), &env(&[])).err().unwrap();
        assert!(e.contains(WA_API_KEY_ENV), "{e}");
        assert!(matches!(
            Notifier::from_env(&sms(), &env(&[(NOTIFY_MODE_ENV, "manual")])),
            Ok(Notifier::Manual)
        ));
        let e = Notifier::from_env(&sms(), &env(&[(NOTIFY_MODE_ENV, "fake")]))
            .err()
            .unwrap();
        assert!(e.contains(NOTIFY_MODE_ENV), "{e}");
        let mut http = WA.to_vec();
        http.push((WA_ENDPOINT_ENV, "http://api.example.test"));
        assert!(Notifier::from_env(&sms(), &env(&http)).is_err());
    }

    #[test]
    fn the_whatsapp_form_carries_the_template_and_ordered_params_but_no_key() {
        let Ok(Notifier::Gupshup(g)) = Notifier::from_env(&sms(), &env(&WA)) else {
            panic!("a Gupshup notifier");
        };
        let params = vec!["Ravi".to_owned(), "Rs 20".to_owned()];
        let form = g.whatsapp_form(&Outgoing {
            outbox_id: "OUT1",
            to: "+919811122233",
            whatsapp_template_id: "tpl_1",
            dlt_template_id: "dlt_1",
            params: &params,
            sms_text: "text",
        });
        let get = |k: &str| form.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("destination"), Some("919811122233"));
        assert_eq!(get("channel"), Some("whatsapp"));
        assert_eq!(get("source"), Some("919000000000"));
        assert_eq!(get("src.name"), Some("Arena360"));
        assert_eq!(
            get("template"),
            Some(r#"{"id":"tpl_1","params":["Ravi","Rs 20"]}"#)
        );
        assert!(form.iter().all(|(_, v)| !v.contains("secret-key")));
    }

    #[test]
    fn config_debug_hides_the_api_key() {
        let wa = WhatsappConfig {
            endpoint: "https://x".into(),
            api_key: "secret-key".into(),
            source: "1".into(),
            app_name: "a".into(),
        };
        assert!(!format!("{wa:?}").contains("secret-key"));
    }

    #[tokio::test]
    async fn manual_mode_answers_a_manual_message_id() {
        let params = vec!["x".to_owned()];
        let out = Outgoing {
            outbox_id: "OUT1",
            to: "+919811122233",
            whatsapp_template_id: "t",
            dlt_template_id: "d",
            params: &params,
            sms_text: "hello",
        };
        assert_eq!(
            Notifier::Manual.send(Channel::Whatsapp, &out).await,
            Ok("manual-OUT1".to_owned())
        );
    }
}

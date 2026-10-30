//! [`GupshupSms`]: the Gupshup Enterprise SMS HTTP API (`method=SendMessage`), with the DLT
//! template id India requires. The password is never logged or put in an error.

use std::time::Duration;

use super::{Sms, SmsError};

/// How long one send may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// `ARENA_GUPSHUP_*`.
#[derive(Clone, PartialEq, Eq)]
pub struct GupshupConfig {
    /// The API URL, e.g. `https://enterprise.smsgupshup.com/GatewayAPI/rest`.
    pub endpoint: String,
    pub userid: String,
    pub password: String,
    /// The DLT-registered sender id (header), sent as `mask`.
    pub sender: String,
    /// The DLT template id of the sign-up message.
    pub dlt_template_id: String,
}

impl std::fmt::Debug for GupshupConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GupshupConfig")
            .field("endpoint", &self.endpoint)
            .field("userid", &self.userid)
            .field("password", &"[redacted]")
            .field("sender", &self.sender)
            .field("dlt_template_id", &self.dlt_template_id)
            .finish()
    }
}

/// The Gupshup client.
pub struct GupshupSms {
    config: GupshupConfig,
    client: reqwest::Client,
}

impl GupshupSms {
    pub fn new(config: GupshupConfig) -> Result<Self, SmsError> {
        let client = crate::alert::client(SEND_TIMEOUT).map_err(SmsError::failed)?;
        Ok(Self { config, client })
    }

    /// The form fields of one send (the recipient without its `+`).
    pub fn form(&self, to_e164: &str, text: &str) -> Vec<(&'static str, String)> {
        self.form_with(to_e164, text, &self.config.dlt_template_id)
    }

    /// [`Self::form`] under another DLT template id.
    pub fn form_with(
        &self,
        to_e164: &str,
        text: &str,
        dlt_template_id: &str,
    ) -> Vec<(&'static str, String)> {
        let c = &self.config;
        vec![
            ("method", "SendMessage".to_owned()),
            ("send_to", to_e164.trim_start_matches('+').to_owned()),
            ("msg", text.to_owned()),
            ("msg_type", "TEXT".to_owned()),
            ("userid", c.userid.clone()),
            ("password", c.password.clone()),
            ("auth_scheme", "plain".to_owned()),
            ("v", "1.1".to_owned()),
            ("format", "text".to_owned()),
            ("mask", c.sender.clone()),
            ("dltTemplateId", dlt_template_id.to_owned()),
        ]
    }
}

/// The most of a reply that is read.
pub const MAX_REPLY_BYTES: usize = 8 * 1024;

/// Whether a text-format reply (`success | 919876543210 | <id>`, `error | <code> | <msg>`)
/// reports success: its first `|` field is exactly `success`.
pub fn is_success_reply(body: &str) -> bool {
    body.split('|').next().map(str::trim) == Some("success")
}

impl Sms for GupshupSms {
    async fn send(&self, to_e164: &str, text: &str) -> Result<(), SmsError> {
        self.send_with_template(to_e164, text, &self.config.dlt_template_id)
            .await
    }
}

impl GupshupSms {
    /// Sends `text` under the DLT template `dlt_template_id`.
    pub async fn send_with_template(
        &self,
        to_e164: &str,
        text: &str,
        dlt_template_id: &str,
    ) -> Result<(), SmsError> {
        let mut res = self
            .client
            .post(&self.config.endpoint)
            .form(&self.form_with(to_e164, text, dlt_template_id))
            .send()
            .await
            .map_err(|e| {
                // Only a failure to connect proves nothing went out.
                let connect = e.is_connect();
                let msg = format!("gupshup unreachable: {}", e.without_url());
                if connect {
                    SmsError::failed(msg)
                } else {
                    SmsError::uncertain(msg)
                }
            })?;
        let status = res.status();
        let mut body = Vec::new();
        while body.len() < MAX_REPLY_BYTES {
            match res.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(e) => {
                    return Err(SmsError::uncertain(format!(
                        "gupshup reply unreadable: {}",
                        e.without_url()
                    )));
                }
            }
        }
        body.truncate(MAX_REPLY_BYTES);
        let body = String::from_utf8_lossy(&body);
        if status.is_success() && is_success_reply(&body) {
            return Ok(());
        }
        let code = body
            .split('|')
            .nth(1)
            .map(str::trim)
            .filter(|c| c.chars().all(|ch| ch.is_ascii_alphanumeric()) && c.len() <= 6)
            .unwrap_or("?");
        Err(SmsError::failed(format!(
            "gupshup refused the send (HTTP {}, code {code})",
            status.as_u16()
        )))
    }
}

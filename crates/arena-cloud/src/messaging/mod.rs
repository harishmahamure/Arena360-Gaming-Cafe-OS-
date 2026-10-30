//! The messaging adapter: an SMS goes out through [`Sms`], implemented by the recorded
//! [`FakeSms`] (dev and tests) and the Gupshup Enterprise SMS API ([`GupshupSms`]). Which one
//! runs is `ARENA_SMS_PROVIDER` (`fake` or `gupshup`); `--dev` forces the fake.

mod fake;
mod gupshup;
mod notify;

use std::fmt;
use std::future::Future;

pub use fake::{FakeSms, SentSms};
pub use gupshup::{GupshupConfig, GupshupSms, MAX_REPLY_BYTES, is_success_reply};
pub use notify::{
    Behaviour, Channel, FakeNotifier, GupshupNotifier, Notifier, Outgoing, SendFailure,
    SentMessage, WhatsappConfig,
};

/// Why an SMS was not sent. The message never carries the recipient, the text or a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsError {
    pub message: String,
    /// The provider may have delivered it anyway (a timeout after the request went out).
    pub uncertain: bool,
}

impl SmsError {
    /// The provider definitely did not send it.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            uncertain: false,
        }
    }

    /// Delivery is unknown.
    pub fn uncertain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            uncertain: true,
        }
    }
}

impl fmt::Display for SmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.uncertain {
            "SMS_SEND_UNCERTAIN"
        } else {
            "SMS_SEND_FAILED"
        };
        write!(f, "{kind}: {}", self.message)
    }
}

impl std::error::Error for SmsError {}

/// Sends one SMS.
pub trait Sms: Send + Sync {
    /// Sends `text` to `to_e164` (`+919876543210`).
    fn send(&self, to_e164: &str, text: &str) -> impl Future<Output = Result<(), SmsError>> + Send;
}

/// The configured provider.
pub enum Messenger {
    Fake(FakeSms),
    Gupshup(GupshupSms),
}

impl Messenger {
    /// The recorded fake, when that is the provider (for `GET /v1/dev/sms`).
    pub fn fake(&self) -> Option<&FakeSms> {
        match self {
            Self::Fake(f) => Some(f),
            Self::Gupshup(_) => None,
        }
    }

    /// `fake` or `gupshup`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Fake(_) => "fake",
            Self::Gupshup(_) => "gupshup",
        }
    }
}

impl Sms for Messenger {
    async fn send(&self, to_e164: &str, text: &str) -> Result<(), SmsError> {
        match self {
            Self::Fake(f) => f.send(to_e164, text).await,
            Self::Gupshup(g) => g.send(to_e164, text).await,
        }
    }
}

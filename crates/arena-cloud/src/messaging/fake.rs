//! [`FakeSms`]: records every message in memory instead of sending it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use super::{Sms, SmsError};
use crate::Clock;

/// One recorded message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentSms {
    pub to: String,
    pub text: String,
    /// Unix ms.
    pub at: i64,
}

/// The recorded fake. [`FakeSms::set_failing`] makes every send fail (tests of a provider
/// outage), [`FakeSms::set_uncertain`] makes it time out after "sending" (delivery unknown;
/// nothing is recorded).
pub struct FakeSms {
    sent: Mutex<Vec<SentSms>>,
    failing: AtomicBool,
    uncertain: AtomicBool,
    clock: Clock,
}

impl FakeSms {
    pub fn new(clock: Clock) -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
            failing: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
            clock,
        }
    }

    /// Every message sent so far, oldest first.
    pub fn sent(&self) -> Vec<SentSms> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Makes later sends fail (`true`) or succeed again.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    /// Makes later sends end with an uncertain outcome (`true`) or succeed again.
    pub fn set_uncertain(&self, uncertain: bool) {
        self.uncertain.store(uncertain, Ordering::SeqCst);
    }
}

impl Sms for FakeSms {
    async fn send(&self, to_e164: &str, text: &str) -> Result<(), SmsError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(SmsError::failed("the fake provider is set to fail"));
        }
        if self.uncertain.load(Ordering::SeqCst) {
            return Err(SmsError::uncertain("the fake provider is set to time out"));
        }
        let msg = SentSms {
            to: to_e164.to_owned(),
            text: text.to_owned(),
            at: (self.clock)(),
        };
        self.sent
            .lock()
            .map_err(|_| SmsError::failed("the fake outbox is poisoned"))?
            .push(msg);
        Ok(())
    }
}

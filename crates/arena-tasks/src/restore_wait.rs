//! A replacement Hub asks for the branch key (Story 9.9, AD-11, AD-24).
//!
//! A Hub paired with a branch that already has history opens in "Restore this branch". At pairing
//! it makes a one-time age X25519 key pair and keeps the private half in the OS keystore
//! ([`restore_public_key`]). While it waits, [`KeyRequestTask`] sends the public half to
//! `POST /v1/kek/request`, so the owner app can show the pending request (Story 9.10):
//!
//! * once when the Hub starts waiting (a start or a restart both count),
//! * again after the WAN has been down and comes back,
//! * with a backoff after a failed attempt.
//!
//! Nothing is written to the database and the private key is never logged or sent.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{HubStatus, PairGate, PairingState};
use arena_proto::{ConnectivityState, ErrorCode, KekRequest};
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;

/// The longest wait between two failed attempts.
const RETRY_CEILING: Duration = Duration::from_secs(15 * 60);
/// How long to wait when the cloud limits the calls and does not say how long.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);
/// How often the task looks at the link while it waits for it to drop and return.
const LINK_POLL: Duration = Duration::from_secs(5);

/// The public key of this Hub's one-time pair (`age1…`). The pair is made, and its private half
/// stored in `store`, the first time; an existing one is reused. A stored value that is not an age
/// identity is replaced.
pub fn restore_public_key(store: &dyn Keystore) -> std::io::Result<String> {
    use age::secrecy::ExposeSecret;
    if let Some(bytes) = store.get()?
        && let Ok(text) = std::str::from_utf8(&bytes)
        && let Ok(identity) = age::x25519::Identity::from_str(text.trim())
    {
        return Ok(identity.to_public().to_string());
    }
    let identity = age::x25519::Identity::generate();
    store.put(identity.to_string().expose_secret().as_bytes())?;
    Ok(identity.to_public().to_string())
}

/// Sends the replacement Hub's key request while the Hub waits for a restore.
pub struct KeyRequestTask {
    cloud: CloudClient,
    token: Arc<dyn Keystore>,
    restore_key: Arc<dyn Keystore>,
    hub_instance_id: String,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    fails: u32,
}

impl KeyRequestTask {
    pub fn new(
        cloud: CloudClient,
        token: Arc<dyn Keystore>,
        restore_key: Arc<dyn Keystore>,
        hub_instance_id: String,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            cloud,
            token,
            restore_key,
            hub_instance_id,
            gate,
            status,
            fails: 0,
        }
    }

    fn waiting(&self) -> bool {
        self.status.borrow().restore_pending.is_some()
    }

    fn online(&self) -> bool {
        self.status.borrow().connectivity.state == ConnectivityState::Online
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        let mut status = self.status.subscribe();
        loop {
            if status
                .wait_for(|s| s.restore_pending.is_some())
                .await
                .is_err()
            {
                return;
            }
            self.wait_in_restore().await;
        }
    }

    /// Asks once, then again each time the WAN returns, until the Hub stops waiting.
    async fn wait_in_restore(&mut self) {
        self.fails = 0;
        while self.waiting() {
            match self.ask().await {
                Ok(()) => {
                    self.fails = 0;
                    self.until_link_returns().await;
                }
                Err(wait) => tokio::time::sleep(wait).await,
            }
        }
    }

    /// Returns once the link has dropped and come back, or the Hub stops waiting.
    async fn until_link_returns(&self) {
        while self.waiting() && self.online() {
            tokio::time::sleep(LINK_POLL).await;
        }
        while self.waiting() && !self.online() {
            tokio::time::sleep(LINK_POLL).await;
        }
    }

    fn failed(&mut self) -> Duration {
        self.fails = self.fails.saturating_add(1);
        backoff(self.fails, RETRY_CEILING, unit()).max(Duration::from_secs(5))
    }

    /// One attempt. `Err` is how long to wait before the next.
    async fn ask(&mut self) -> Result<(), Duration> {
        if self.status.borrow().pairing == PairingState::Revoked {
            return Err(REVOKED_BACKOFF);
        }
        let (token, key) = (Arc::clone(&self.token), Arc::clone(&self.restore_key));
        let read = tokio::task::spawn_blocking(move || {
            let token = token.get()?;
            restore_public_key(key.as_ref()).map(|k| (token, k))
        })
        .await;
        let Ok(Ok((Some(token), public_key))) = read else {
            tracing::warn!("restore: no hub token or one-time key yet");
            return Err(Duration::from_secs(30));
        };
        let req = KekRequest {
            hub_instance_id: self.hub_instance_id.clone(),
            public_key,
        };
        match self
            .cloud
            .kek_request(&String::from_utf8_lossy(&token), &req)
            .await
        {
            Ok(res) => {
                tracing::info!(state = ?res.state, "restore: branch key requested");
                Ok(())
            }
            Err(CloudFailure::Refused {
                error,
                retry_after_s,
                ..
            }) => match error.code {
                ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => {
                    self.status
                        .send_modify(|s| s.pairing = PairingState::Revoked);
                    Err(REVOKED_BACKOFF)
                }
                ErrorCode::RateLimited => {
                    Err(retry_after_s.map_or(RATE_LIMIT_WAIT, |s| Duration::from_secs(s.into())))
                }
                code => {
                    tracing::warn!(code = code.as_str(), "restore: the key request was refused");
                    Err(self.failed())
                }
            },
            Err(CloudFailure::Unreachable(_)) => Err(self.failed()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Mem(Mutex<Option<Vec<u8>>>);

    impl Keystore for Mem {
        fn put(&self, secret: &[u8]) -> std::io::Result<()> {
            *self.0.lock().unwrap() = Some(secret.to_vec());
            Ok(())
        }
        fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    #[test]
    fn the_one_time_pair_is_made_once_and_only_the_public_half_leaves() {
        let store = Mem::default();
        let public = restore_public_key(&store).unwrap();
        assert!(arena_proto::valid_age_recipient(&public));
        let secret = store.get().unwrap().unwrap();
        assert!(secret.starts_with(b"AGE-SECRET-KEY-1"));
        assert_eq!(restore_public_key(&store).unwrap(), public);
        assert_eq!(store.get().unwrap().unwrap(), secret);
    }

    #[test]
    fn a_stored_value_that_is_not_an_age_key_is_replaced() {
        let store = Mem::default();
        store.put(b"garbage").unwrap();
        let public = restore_public_key(&store).unwrap();
        assert!(arena_proto::valid_age_recipient(&public));
        assert!(
            store
                .get()
                .unwrap()
                .unwrap()
                .starts_with(b"AGE-SECRET-KEY-1")
        );
    }
}

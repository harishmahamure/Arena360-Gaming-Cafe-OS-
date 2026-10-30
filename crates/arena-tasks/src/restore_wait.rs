//! A replacement Hub asks for the branch key and takes it (Stories 9.9 and 9.10, AD-11, AD-24).
//!
//! A Hub paired with a branch that already has history opens in "Restore this branch". At pairing
//! it makes a one-time age X25519 key pair and keeps the private half in the OS keystore
//! ([`restore_public_key`]). While it waits, [`KeyRequestTask`]:
//!
//! * sends the public half to `POST /v1/kek/request` once when the Hub starts waiting (a start or
//!   a restart both count) and again after the WAN has been down and comes back, so the owner app
//!   can show the pending request;
//! * while the request is `pending`, polls `POST /v1/kek/fetch` (a failed attempt backs off);
//! * once the owner approved, takes the re-wrapped branch key (handed out once), opens it with the
//!   one-time private key, stores it as the branch key in the keystore, deletes the one-time key
//!   and sets `restore_key_released`. The Hub stays in "Restore this branch": restoring from a
//!   file is a later step.
//!
//! A request the owner denied, or that expired, keeps the Hub waiting; it asks again after the WAN
//! returns and never receives a key. Nothing is written to the database and neither private key
//! nor branch key is logged or sent.

use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use age::secrecy::{ExposeSecret, ExposeSecretMut, SecretBox};
use arena_core::players::KEK_LEN;
use arena_core::{HubStatus, PairGate, PairingState};
use arena_proto::{
    ConnectivityState, ErrorCode, KekFetchRequest, KekRequest, KeyRequestState, sanitise_pc_name,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
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
/// How often a pending request is checked for the owner's decision.
const DECISION_POLL: Duration = Duration::from_secs(30);

/// The public key of this Hub's one-time pair (`age1…`). The pair is made, and its private half
/// stored in `store`, the first time; an existing one is reused. A stored value that is not an age
/// identity is replaced.
pub fn restore_public_key(store: &dyn Keystore) -> std::io::Result<String> {
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

/// Why the released key could not be taken. Never carries key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeError {
    /// The base64 or the age file is damaged.
    Damaged,
    /// The one-time private key is missing or does not open the file.
    WrongKey,
    /// The file does not hold a branch key.
    NotAKey,
    /// The keystore could not be read or written.
    Store,
}

impl std::fmt::Display for TakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Damaged => "the released key is damaged",
            Self::WrongKey => "the one-time key does not open the released key",
            Self::NotAKey => "the released file is not a branch key",
            Self::Store => "the keystore could not be used",
        })
    }
}

impl std::error::Error for TakeError {}

/// Opens the released key (`wrapped_kek`: an age file, base64) with the one-time private key in
/// `restore_key`. The key lives in one zeroizing buffer until it is stored.
fn open_released(
    restore_key: &dyn Keystore,
    wrapped_kek: &str,
) -> Result<SecretBox<[u8; KEK_LEN]>, TakeError> {
    let wrapped = STANDARD
        .decode(wrapped_kek.trim())
        .map_err(|_| TakeError::Damaged)?;
    let secret = restore_key
        .get()
        .map_err(|_| TakeError::Store)?
        .ok_or(TakeError::WrongKey)?;
    let identity = std::str::from_utf8(&secret)
        .ok()
        .and_then(|t| age::x25519::Identity::from_str(t.trim()).ok())
        .ok_or(TakeError::WrongKey)?;
    let decryptor = age::Decryptor::new_buffered(&wrapped[..]).map_err(|_| TakeError::Damaged)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| TakeError::WrongKey)?;
    let mut kek: SecretBox<[u8; KEK_LEN]> = SecretBox::new(Box::new([0u8; KEK_LEN]));
    reader
        .read_exact(kek.expose_secret_mut())
        .map_err(|_| TakeError::NotAKey)?;
    let mut extra = [0u8; 1];
    if !matches!(reader.read(&mut extra), Ok(0)) {
        return Err(TakeError::NotAKey);
    }
    Ok(kek)
}

/// Takes the released key: opens it with the one-time key, stores it as the branch key in `kek`
/// and deletes the one-time key. A one-time key that cannot be deleted is logged, not fatal: the
/// branch key is stored and the key it protected has been used.
pub fn take_released_key(
    restore_key: &dyn Keystore,
    kek: &dyn Keystore,
    wrapped_kek: &str,
) -> Result<(), TakeError> {
    let key = open_released(restore_key, wrapped_kek)?;
    kek.put(key.expose_secret()).map_err(|_| TakeError::Store)?;
    if restore_key.delete().is_err() {
        tracing::warn!("restore: the one-time key could not be deleted");
    }
    Ok(())
}

/// Whether the Hub holds the branch key a waiting Hub is released (start-up check).
pub fn holds_released_key(kek: &dyn Keystore) -> bool {
    match kek.get() {
        Ok(Some(bytes)) => bytes.len() == KEK_LEN,
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(error = %e, "restore: the branch key could not be read at start");
            false
        }
    }
}

/// What one fetch found.
enum Fetched {
    /// The key was taken and stored.
    Released,
    /// Where the request stands, without a key.
    State(KeyRequestState),
    /// The cloud says the key was handed out already and this Hub has none: only replacing the Hub
    /// again helps.
    Gone,
}

/// How following a request ended.
enum Settled {
    /// The key was taken and stored.
    Released,
    /// The WAN dropped and returned while the request was pending: ask again.
    Resend,
    /// Nothing more to ask until the link has been down and returns.
    Idle,
}

/// Asks for, and takes, the branch key while the Hub waits for a restore.
pub struct KeyRequestTask {
    cloud: CloudClient,
    token: Arc<dyn Keystore>,
    restore_key: Arc<dyn Keystore>,
    kek: Arc<dyn Keystore>,
    hub_instance_id: String,
    pc_name: Option<String>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    fails: u32,
    /// How often a pending request is checked for the owner's decision.
    poll: Duration,
}

impl KeyRequestTask {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cloud: CloudClient,
        token: Arc<dyn Keystore>,
        restore_key: Arc<dyn Keystore>,
        kek: Arc<dyn Keystore>,
        hub_instance_id: String,
        pc_name: &str,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
    ) -> Self {
        Self {
            cloud,
            token,
            restore_key,
            kek,
            hub_instance_id,
            pc_name: sanitise_pc_name(pc_name),
            gate,
            status,
            fails: 0,
            poll: DECISION_POLL,
        }
    }

    /// Checks a pending request every `poll` instead of every 30 s (tests).
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// Waiting for the owner's key: the Hub is in "Restore this branch" and has no key yet.
    fn waiting(&self) -> bool {
        let s = self.status.borrow();
        s.restore_pending.is_some() && !s.restore_key_released
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
                .wait_for(|s| s.restore_pending.is_some() && !s.restore_key_released)
                .await
                .is_err()
            {
                return;
            }
            self.wait_in_restore().await;
        }
    }

    /// Asks once, then checks for the decision, and asks again each time the WAN returns, until
    /// the key is taken or the Hub stops waiting.
    async fn wait_in_restore(&mut self) {
        self.fails = 0;
        while self.waiting() {
            let state = match self.ask().await {
                Ok(state) => state,
                Err(wait) => {
                    tokio::time::sleep(wait).await;
                    continue;
                }
            };
            self.fails = 0;
            match self.settle(state).await {
                Settled::Released => return,
                Settled::Resend => {}
                Settled::Idle => self.until_link_returns().await,
            }
        }
    }

    /// Follows a request the cloud holds: while it is pending, checks for the decision; once
    /// approved, takes the key. A denied or expired request, or a key sent already, ends it.
    async fn settle(&mut self, mut state: KeyRequestState) -> Settled {
        let mut dropped = false;
        loop {
            if !self.waiting() {
                return Settled::Idle;
            }
            match state {
                KeyRequestState::Denied | KeyRequestState::Expired => return Settled::Idle,
                KeyRequestState::Approved => {}
                KeyRequestState::Pending => {
                    tokio::time::sleep(self.poll).await;
                    if !self.online() {
                        dropped = true;
                    } else if dropped {
                        // The WAN is back: the request is sent again to keep it fresh.
                        return Settled::Resend;
                    }
                    if !self.waiting() {
                        return Settled::Idle;
                    }
                }
            }
            match self.fetch().await {
                Ok(Fetched::Released) => return Settled::Released,
                Ok(Fetched::State(next)) => {
                    self.fails = 0;
                    state = next;
                }
                Ok(Fetched::Gone) => return Settled::Idle,
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

    /// The token as text, or how long to wait for it.
    async fn token(&self) -> Result<String, Duration> {
        let token = Arc::clone(&self.token);
        match tokio::task::spawn_blocking(move || token.get()).await {
            Ok(Ok(Some(token))) => Ok(String::from_utf8_lossy(&token).into_owned()),
            _ => {
                tracing::warn!("restore: no hub token yet");
                Err(Duration::from_secs(30))
            }
        }
    }

    /// How a refused call is waited out; marks the Hub revoked for a token the cloud refuses.
    fn refused(&mut self, failure: CloudFailure, what: &str) -> Duration {
        match failure {
            CloudFailure::Refused {
                error,
                retry_after_s,
                ..
            } => match error.code {
                ErrorCode::TokenRevoked | ErrorCode::TokenInvalid => {
                    self.status
                        .send_modify(|s| s.pairing = PairingState::Revoked);
                    REVOKED_BACKOFF
                }
                ErrorCode::RateLimited => {
                    retry_after_s.map_or(RATE_LIMIT_WAIT, |s| Duration::from_secs(s.into()))
                }
                code => {
                    tracing::warn!(code = code.as_str(), "restore: the {what} was refused");
                    self.failed()
                }
            },
            CloudFailure::Unreachable(_) => self.failed(),
        }
    }

    /// One request. `Err` is how long to wait before the next attempt.
    async fn ask(&mut self) -> Result<KeyRequestState, Duration> {
        if self.status.borrow().pairing == PairingState::Revoked {
            return Err(REVOKED_BACKOFF);
        }
        let token = self.token().await?;
        let key = Arc::clone(&self.restore_key);
        let public_key =
            match tokio::task::spawn_blocking(move || restore_public_key(key.as_ref())).await {
                Ok(Ok(k)) => k,
                _ => {
                    tracing::warn!("restore: no one-time key yet");
                    return Err(Duration::from_secs(30));
                }
            };
        let req = KekRequest {
            hub_instance_id: self.hub_instance_id.clone(),
            public_key,
            pc_name: self.pc_name.clone(),
        };
        match self.cloud.kek_request(&token, &req).await {
            Ok(res) => {
                tracing::info!(state = ?res.state, "restore: branch key requested");
                Ok(res.state)
            }
            Err(failure) => Err(self.refused(failure, "key request")),
        }
    }

    /// One fetch of the owner's decision. `Err` is how long to wait before the next attempt.
    async fn fetch(&mut self) -> Result<Fetched, Duration> {
        if self.status.borrow().pairing == PairingState::Revoked {
            return Err(REVOKED_BACKOFF);
        }
        let token = self.token().await?;
        let req = KekFetchRequest {
            hub_instance_id: self.hub_instance_id.clone(),
        };
        let res = match self.cloud.kek_fetch(&token, &req).await {
            Ok(res) => res,
            Err(CloudFailure::Refused { error, .. })
                if error.code == ErrorCode::KeyAlreadyReleased =>
            {
                tracing::warn!(
                    "restore: the branch key was already sent to this Hub; replace the Hub again"
                );
                return Ok(Fetched::Gone);
            }
            Err(failure) => return Err(self.refused(failure, "key fetch")),
        };
        let Some(wrapped) = res.wrapped_kek else {
            return Ok(Fetched::State(res.state));
        };
        let (restore_key, kek) = (Arc::clone(&self.restore_key), Arc::clone(&self.kek));
        let taken = tokio::task::spawn_blocking(move || {
            take_released_key(restore_key.as_ref(), kek.as_ref(), &wrapped)
        })
        .await;
        match taken {
            Ok(Ok(())) => {
                tracing::info!("restore: the branch key was released to this Hub");
                self.status.send_modify(|s| s.restore_key_released = true);
                Ok(Fetched::Released)
            }
            Ok(Err(e)) => {
                // The cloud will not hand the key out again: only replacing the Hub helps.
                tracing::error!(error = %e, "restore: the released branch key could not be taken");
                Ok(Fetched::Gone)
            }
            Err(_) => Ok(Fetched::Gone),
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
        fn delete(&self) -> std::io::Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
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

    fn wrapped_for(public: &str, plain: &[u8]) -> String {
        let recipient = age::x25519::Recipient::from_str(public).unwrap();
        let enc =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .unwrap();
        let mut out = Vec::new();
        let mut w = enc.wrap_output(&mut out).unwrap();
        std::io::Write::write_all(&mut w, plain).unwrap();
        w.finish().unwrap();
        STANDARD.encode(out)
    }

    #[test]
    fn the_released_key_is_stored_as_the_branch_key_and_the_one_time_key_is_deleted() {
        let (restore, kek) = (Mem::default(), Mem::default());
        let public = restore_public_key(&restore).unwrap();
        let blob = wrapped_for(&public, &[7u8; KEK_LEN]);
        assert!(!holds_released_key(&kek));
        take_released_key(&restore, &kek, &blob).unwrap();
        assert_eq!(kek.get().unwrap().unwrap(), vec![7u8; KEK_LEN]);
        assert!(restore.get().unwrap().is_none());
        assert!(holds_released_key(&kek));
    }

    #[test]
    fn a_key_for_another_pair_or_a_damaged_or_wrong_sized_one_is_never_stored() {
        let (restore, kek) = (Mem::default(), Mem::default());
        let public = restore_public_key(&restore).unwrap();
        let other = age::x25519::Identity::generate().to_public().to_string();
        let cases = [
            (wrapped_for(&other, &[7u8; KEK_LEN]), TakeError::WrongKey),
            ("not base64!".to_owned(), TakeError::Damaged),
            (STANDARD.encode(b"not an age file"), TakeError::Damaged),
            (wrapped_for(&public, &[7u8; 31]), TakeError::NotAKey),
            (wrapped_for(&public, &[7u8; 33]), TakeError::NotAKey),
        ];
        for (blob, why) in cases {
            assert_eq!(take_released_key(&restore, &kek, &blob), Err(why));
            assert!(kek.get().unwrap().is_none(), "{why}");
            assert!(restore.get().unwrap().is_some(), "{why}");
        }
        // No one-time key at all.
        let none = Mem::default();
        assert_eq!(
            take_released_key(&none, &kek, &wrapped_for(&public, &[7u8; KEK_LEN])),
            Err(TakeError::WrongKey)
        );
    }

    use crate::cloud::CloudEndpoint;
    use arena_core::status_channel;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = Arc<Mutex<Vec<(String, String)>>>;
    type Handler = Box<dyn FnMut(&str) -> (u16, String) + Send>;

    /// A one-request-per-connection HTTP server: (request line, body) of each call.
    async fn serve(mut handler: Handler) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                let body_at = loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..at]).to_ascii_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        while raw.len() < at + 4 + len {
                            let n = sock.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            raw.extend_from_slice(&buf[..n]);
                        }
                        break at + 4;
                    }
                    if n == 0 {
                        break raw.len();
                    }
                };
                let text = String::from_utf8_lossy(&raw).into_owned();
                let line = text.lines().next().unwrap_or("").to_owned();
                let body = text.get(body_at..).unwrap_or("").to_owned();
                log.lock().unwrap().push((line.clone(), body));
                let (status, reply) = handler(&line);
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
            }
        });
        (port, seen)
    }

    /// The Hub stores: the one-time key made, the branch key store empty.
    struct Stores {
        restore: Arc<Mem>,
        kek: Arc<Mem>,
        public: String,
    }

    fn stores() -> Stores {
        let restore = Arc::new(Mem::default());
        let public = restore_public_key(restore.as_ref()).unwrap();
        Stores {
            restore,
            kek: Arc::new(Mem::default()),
            public,
        }
    }

    fn make_task(port: u16, s: &Stores) -> (KeyRequestTask, watch::Sender<HubStatus>) {
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(85, "01INSTANCE"));
        status.send_modify(|st| {
            st.restore_pending = Some("Pixel Arena".into());
            st.connectivity.state = ConnectivityState::Online;
        });
        let token = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = KeyRequestTask::new(
            cloud,
            token,
            Arc::clone(&s.restore) as Arc<dyn Keystore>,
            Arc::clone(&s.kek) as Arc<dyn Keystore>,
            "01INSTANCE".into(),
            "FRONT\u{7}-DESK",
            PairGate::new(true),
            status.clone(),
        )
        .with_poll(Duration::from_millis(20));
        (task, status)
    }

    async fn until(what: &str, mut ok: impl FnMut() -> bool) {
        for _ in 0..200 {
            if ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("never happened: {what}");
    }

    fn count(seen: &Seen, prefix: &str) -> usize {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(l, _)| l.starts_with(prefix))
            .count()
    }

    #[tokio::test]
    async fn a_pending_request_is_polled_until_approved_then_the_key_is_taken_and_flagged() {
        let s = stores();
        let blob = wrapped_for(&s.public, &[3u8; KEK_LEN]);
        let fetches = Arc::new(AtomicUsize::new(0));
        let n = Arc::clone(&fetches);
        let (port, seen) = serve(Box::new(move |line| {
            if line.starts_with("POST /v1/kek/request") || n.fetch_add(1, Ordering::SeqCst) < 2 {
                (200, r#"{"state":"pending"}"#.into())
            } else {
                (
                    200,
                    serde_json::json!({"state": "approved", "wrapped_kek": blob}).to_string(),
                )
            }
        }))
        .await;
        let (task, status) = make_task(port, &s);
        let job = tokio::spawn(task.run());
        until("the flag", || status.borrow().restore_key_released).await;

        assert_eq!(s.kek.get().unwrap().unwrap(), vec![3u8; KEK_LEN]);
        assert!(s.restore.get().unwrap().is_none(), "one-time key deleted");
        let calls = seen.lock().unwrap().clone();
        let request = calls
            .iter()
            .find(|(l, _)| l.starts_with("POST /v1/kek/request"))
            .unwrap();
        assert!(request.1.contains(&s.public), "{}", request.1);
        assert!(request.1.contains("FRONT -DESK"), "{}", request.1);
        assert_eq!(count(&seen, "POST /v1/kek/request"), 1);
        assert_eq!(count(&seen, "POST /v1/kek/fetch"), 3);
        // Released: the task stops calling.
        let before = calls.len();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(seen.lock().unwrap().len(), before);
        job.abort();
    }

    #[tokio::test]
    async fn a_denied_request_never_gets_a_key_and_keeps_the_hub_waiting() {
        let s = stores();
        let (port, seen) = serve(Box::new(|line| {
            let state = if line.starts_with("POST /v1/kek/request") {
                "pending"
            } else {
                "denied"
            };
            (200, format!(r#"{{"state":"{state}"}}"#))
        }))
        .await;
        let (task, status) = make_task(port, &s);
        let job = tokio::spawn(task.run());
        until("the fetch", || count(&seen, "POST /v1/kek/fetch") == 1).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(count(&seen, "POST /v1/kek/fetch"), 1, "it stops asking");
        {
            let st = status.borrow();
            assert!(st.restore_pending.is_some() && !st.restore_key_released);
        }
        assert!(s.kek.get().unwrap().is_none());
        assert!(s.restore.get().unwrap().is_some());
        job.abort();
    }

    #[tokio::test]
    async fn a_key_for_another_pair_or_already_sent_is_not_stored_and_not_asked_for_again() {
        let s = stores();
        let other = age::x25519::Identity::generate().to_public().to_string();
        let blob = wrapped_for(&other, &[3u8; KEK_LEN]);
        let (port, seen) = serve(Box::new(move |line| {
            if line.starts_with("POST /v1/kek/request") {
                (200, r#"{"state":"approved"}"#.into())
            } else {
                (
                    200,
                    serde_json::json!({"state": "approved", "wrapped_kek": blob}).to_string(),
                )
            }
        }))
        .await;
        let (task, status) = make_task(port, &s);
        let job = tokio::spawn(task.run());
        until("the fetch", || count(&seen, "POST /v1/kek/fetch") == 1).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(count(&seen, "POST /v1/kek/fetch"), 1);
        assert!(!status.borrow().restore_key_released);
        assert!(
            s.kek.get().unwrap().is_none(),
            "a wrong key is never stored"
        );
        job.abort();

        let s = stores();
        let (port, seen) = serve(Box::new(|line| {
            if line.starts_with("POST /v1/kek/request") {
                (200, r#"{"state":"approved"}"#.into())
            } else {
                (
                    409,
                    r#"{"code":"KEY_ALREADY_RELEASED","message":"sent"}"#.into(),
                )
            }
        }))
        .await;
        let (task, status) = make_task(port, &s);
        let job = tokio::spawn(task.run());
        until("the fetch", || count(&seen, "POST /v1/kek/fetch") == 1).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(count(&seen, "POST /v1/kek/fetch"), 1);
        assert!(!status.borrow().restore_key_released);
        job.abort();
    }

    #[tokio::test]
    async fn a_hub_that_already_holds_the_key_does_not_ask() {
        let s = stores();
        let (port, seen) = serve(Box::new(|_| (200, r#"{"state":"pending"}"#.into()))).await;
        let (task, status) = make_task(port, &s);
        status.send_modify(|st| st.restore_key_released = true);
        let job = tokio::spawn(task.run());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(seen.lock().unwrap().is_empty());
        job.abort();
    }
}

//! Keeps a paid branch's setup template in the cloud (Story 5.37).
//!
//! When the catalogue changes (groups, rates, products, bundles, plans, rules, coupons) the
//! primary Hub sends its Story 5.36 setup file to `PUT /v1/branches/{id}/setup-template` with
//! its device token, at most `debounce` after the first change of a burst, and once more after
//! the Hub starts. A Free Hub sends nothing; a file equal to the last accepted one is not sent
//! again; a failed send keeps the change pending and is retried with a growing back-off.

use std::sync::Arc;
use std::time::Duration;

use arena_core::{
    Applied, Clock, HubStatus, PairGate, PairingState, Role, SnapshotReader, WriterHandle,
};
use arena_proto::{ErrorCode, SETUP_MAX_BYTES, Tier};
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;

/// The quiet time after the first change of a burst before the file is sent; well inside the
/// 5 minutes the template is promised to be current within.
pub const UPLOAD_DEBOUNCE: Duration = Duration::from_secs(30);
/// How often a Hub that may not upload yet (Free, no writer yet) looks again.
pub const TIER_RECHECK: Duration = Duration::from_secs(60);
/// How long to wait after the cloud says the branch is not entitled.
pub const NOT_ENTITLED_RETRY: Duration = Duration::from_secs(15 * 60);
/// The longest wait between two tries of a failing upload.
pub const RETRY_CEILING: Duration = Duration::from_secs(60);

/// The tables whose rows the setup file is made of.
const CATALOGUE_TABLES: [&str; 9] = [
    "device_groups",
    "rates",
    "products",
    "bundle_components",
    "plans",
    "plan_groups",
    "pricing_rules",
    "coupons",
    "coupon_targets",
];

fn touches_catalogue(applied: &Applied) -> bool {
    applied
        .changes
        .iter()
        .any(|c| CATALOGUE_TABLES.contains(&c.tbl.as_str()))
}

pub struct SetupTemplateTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    snapshots: Arc<SnapshotReader>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    clock: Arc<dyn Clock>,
    debounce: Duration,
    /// SHA-256 of the file the cloud last accepted.
    last_hash: Option<String>,
    fails: u32,
}

impl SetupTemplateTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        snapshots: Arc<SnapshotReader>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            snapshots,
            gate,
            status,
            clock,
            debounce: UPLOAD_DEBOUNCE,
            last_hash: None,
            fails: 0,
        }
    }

    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        let Some(writer) = self.writer.clone() else {
            return;
        };
        let mut events = writer.subscribe();
        // The first look: the cloud may hold an older file than this Hub's.
        let mut due = Some(tokio::time::Instant::now() + self.debounce);
        loop {
            let at = due.unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(3600));
            tokio::select! {
                event = events.recv() => {
                    let changed = match event {
                        Ok(applied) => touches_catalogue(&applied),
                        Err(broadcast::error::RecvError::Lagged(_)) => true,
                        Err(broadcast::error::RecvError::Closed) => return,
                    };
                    if changed && due.is_none() {
                        due = Some(tokio::time::Instant::now() + self.debounce);
                    }
                }
                () = tokio::time::sleep_until(at), if due.is_some() => {
                    due = self
                        .upload()
                        .await
                        .map(|wait| tokio::time::Instant::now() + wait);
                }
            }
        }
    }

    /// Sends the setup file if this Hub may and it differs from the last one the cloud
    /// accepted. Answers how long to wait before trying again, or `None` when nothing is left
    /// to send.
    pub async fn upload(&mut self) -> Option<Duration> {
        let (role, lease, pairing, tier) = {
            let s = self.status.borrow();
            (s.role, s.lease.clone(), s.pairing, s.tier)
        };
        let (Some(writer), Some(_), Role::Primary) = (self.writer.clone(), lease, role) else {
            return Some(TIER_RECHECK);
        };
        if pairing == PairingState::Revoked {
            return Some(REVOKED_BACKOFF);
        }
        if tier.is_none_or(|t| t == Tier::Free) {
            return Some(TIER_RECHECK);
        }
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "setup template: no hub token");
                return Some(TIER_RECHECK);
            }
        };
        let Some(branch_id) = writer
            .licence_state()
            .await
            .ok()
            .and_then(|s| s.identity)
            .map(|i| i.branch_id)
        else {
            return Some(TIER_RECHECK);
        };
        let snapshots = Arc::clone(&self.snapshots);
        let now = self.clock.now_ms();
        let file = match tokio::task::spawn_blocking(move || snapshots.setup_export(now)).await {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "setup template: could not read the setup"
                );
                return Some(self.failed());
            }
            Err(e) => {
                tracing::warn!(error = %e, "setup template: read task failed");
                return Some(self.failed());
            }
        };
        if file.len() > SETUP_MAX_BYTES {
            tracing::warn!(
                bytes = file.len(),
                "setup template: larger than 1 MB, not sent"
            );
            return None;
        }
        let hash = crate_hex(&Sha256::digest(file.as_bytes()));
        if self.last_hash.as_deref() == Some(hash.as_str()) {
            return None;
        }
        match self
            .cloud
            .put_setup_template(&token, &branch_id, file)
            .await
        {
            Ok(stored) => {
                tracing::info!(
                    bytes = stored.bytes,
                    version = stored.version,
                    "setup template kept in the cloud"
                );
                self.last_hash = Some(hash);
                self.fails = 0;
                None
            }
            Err(CloudFailure::Refused { error, .. })
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!("setup template: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                Some(REVOKED_BACKOFF)
            }
            Err(CloudFailure::Refused { error, .. })
                if error.code == ErrorCode::FeatureNotEntitled =>
            {
                tracing::info!("setup template: the cloud has this branch on Free");
                Some(NOT_ENTITLED_RETRY)
            }
            Err(CloudFailure::Refused { error, .. })
                if error.code == ErrorCode::PayloadTooLarge =>
            {
                tracing::warn!("setup template: the cloud refused the file as too large");
                None
            }
            Err(f) => {
                tracing::warn!(failure = ?f, "setup template: upload failed");
                Some(self.failed())
            }
        }
    }

    fn failed(&mut self) -> Duration {
        self.fails += 1;
        backoff(self.fails, RETRY_CEILING, unit())
    }
}

fn crate_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{ManualClock, status_channel};
    use arena_proto::licence::{LicenceClaims, dev_sign};
    use arena_store::Store;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

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

    type Seen = Arc<Mutex<Vec<(String, String)>>>;
    type Handler = Box<dyn FnMut(&str, &str) -> (u16, String) + Send>;

    /// A one-request-per-connection HTTP server; `Seen` collects (request line, body).
    async fn serve(mut handler: Handler) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 16384];
                let (head_end, length) = loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    if let Some(at) = text.find("\r\n\r\n") {
                        let length = text[..at]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        break (at + 4, length);
                    }
                    if n == 0 {
                        break (raw.len(), 0);
                    }
                };
                while raw.len() < head_end + length {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&raw).into_owned();
                let line = text.lines().next().unwrap_or("").to_owned();
                let body = text.get(head_end..).unwrap_or("").to_owned();
                log.lock().unwrap().push((line.clone(), body.clone()));
                let (status, reply) = handler(&line, &body);
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
            }
        });
        (port, seen)
    }

    fn stored() -> (u16, String) {
        (
            200,
            r#"{"version":1,"sha256":"00","bytes":1,"uploaded_at":1}"#.to_owned(),
        )
    }

    fn refused(status: u16, code: &str) -> (u16, String) {
        (status, format!(r#"{{"code":"{code}","message":"no"}}"#))
    }

    struct Fx {
        _dir: tempfile::TempDir,
        status: watch::Sender<HubStatus>,
        task: SetupTemplateTask,
        writer: WriterHandle,
    }

    async fn fx(port: u16, tier: Tier) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(12, OWN));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
        let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        let claims = LicenceClaims::for_tier("T", "B", Tier::Free, T0 / 1000);
        writer
            .record_pairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H".into(),
                    tier: Tier::Free,
                    paired_at: T0,
                },
                dev_sign(&claims).unwrap(),
                None,
            )
            .await
            .unwrap();
        writer
            .submit(|tx| {
                arena_store::insert_branch(
                    tx,
                    &arena_store::NewBranch {
                        branch_id: "B",
                        name: "Arena",
                        branch_code: "ARN",
                        currency: "INR",
                        tz: "Asia/Kolkata",
                        day_start_min: 0,
                        fy_start_month: 4,
                    },
                )
            })
            .await
            .unwrap();
        status.send_modify(|s| s.tier = Some(tier));
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = SetupTemplateTask::new(
            cloud,
            keystore,
            Some(writer.clone()),
            Arc::new(SnapshotReader::open(&path).unwrap()),
            PairGate::new(true),
            status.clone(),
            clock,
        );
        Fx {
            _dir: dir,
            status,
            task,
            writer,
        }
    }

    /// A `group.upsert` through `apply()`, so the change is broadcast like any other.
    async fn add_group(writer: &WriterHandle, name: &str) {
        static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        writer
            .apply_command(arena_proto::CommandWire {
                id,
                idempotency_key: ulid::Ulid::generate().to_string(),
                kind: "group.upsert".into(),
                schema_version: arena_proto::COMMAND_SCHEMA_VERSION,
                payload: serde_json::json!({
                    "name": name, "group_kind": "pc", "colour": "teal", "sort_order": id
                }),
                issued_by: "owner-1".into(),
                elevated_by: None,
            })
            .await
            .unwrap();
    }

    fn puts(seen: &Seen) -> Vec<(String, String)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(line, _)| line.starts_with("PUT"))
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn a_paid_hub_sends_its_setup_file_once_and_skips_an_unchanged_one() {
        let (port, seen) = serve(Box::new(|_, _| stored())).await;
        let mut f = fx(port, Tier::Basic).await;
        add_group(&f.writer, "Front row").await;

        assert_eq!(f.task.upload().await, None);
        let sent = puts(&seen);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(
            sent[0].0.starts_with("PUT /v1/branches/B/setup-template"),
            "{sent:?}"
        );
        let file: serde_json::Value = serde_json::from_str(&sent[0].1).unwrap();
        assert_eq!(file["format"], "arena360-setup");
        assert_eq!(file["groups"][0]["name"], "Front row");

        // Nothing changed: no second request.
        assert_eq!(f.task.upload().await, None);
        assert_eq!(puts(&seen).len(), 1);

        // A change that alters the file sends the new one.
        add_group(&f.writer, "Back row").await;
        assert_eq!(f.task.upload().await, None);
        let sent = puts(&seen);
        assert_eq!(sent.len(), 2);
        assert!(sent[1].1.contains("Back row"));
    }

    #[tokio::test]
    async fn a_free_hub_uploads_nothing_and_sends_after_an_upgrade() {
        let (port, seen) = serve(Box::new(|_, _| stored())).await;
        let mut f = fx(port, Tier::Free).await;
        add_group(&f.writer, "Front row").await;
        assert_eq!(f.task.upload().await, Some(TIER_RECHECK));
        f.status.send_modify(|s| s.tier = None);
        assert_eq!(f.task.upload().await, Some(TIER_RECHECK));
        assert!(puts(&seen).is_empty());

        f.status.send_modify(|s| s.tier = Some(Tier::Basic));
        assert_eq!(f.task.upload().await, None);
        assert_eq!(puts(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_failed_upload_is_retried_and_the_latest_file_is_sent_once() {
        let mut attempts = 0;
        let (port, seen) = serve(Box::new(move |_, _| {
            attempts += 1;
            if attempts <= 2 {
                refused(503, "INTERNAL")
            } else {
                stored()
            }
        }))
        .await;
        let mut f = fx(port, Tier::Basic).await;
        add_group(&f.writer, "Front row").await;
        let wait = f.task.upload().await.expect("retry");
        assert!(
            wait <= RETRY_CEILING && wait >= Duration::from_secs(1),
            "{wait:?}"
        );
        // More edits while the cloud is out of reach.
        add_group(&f.writer, "Back row").await;
        assert!(f.task.upload().await.is_some());
        assert_eq!(f.task.upload().await, None);
        assert_eq!(f.task.upload().await, None);
        let sent = puts(&seen);
        assert_eq!(sent.len(), 3, "two failures and one success: {sent:?}");
        assert!(sent[2].1.contains("Front row") && sent[2].1.contains("Back row"));
    }

    #[tokio::test]
    async fn the_clouds_refusals_are_told_apart() {
        for (status, code, expect) in [
            (403, "FEATURE_NOT_ENTITLED", Some(NOT_ENTITLED_RETRY)),
            (413, "PAYLOAD_TOO_LARGE", None),
            (401, "TOKEN_REVOKED", Some(REVOKED_BACKOFF)),
            (401, "TOKEN_INVALID", Some(REVOKED_BACKOFF)),
        ] {
            let (port, seen) = serve(Box::new(move |_, _| refused(status, code))).await;
            let mut f = fx(port, Tier::Basic).await;
            assert_eq!(f.task.upload().await, expect, "{code}");
            assert_eq!(puts(&seen).len(), 1, "{code}");
            let revoked = f.status.borrow().pairing == PairingState::Revoked;
            assert_eq!(revoked, code.starts_with("TOKEN"), "{code}");
            if revoked {
                // A revoked Hub stops asking.
                assert_eq!(f.task.upload().await, Some(REVOKED_BACKOFF));
                assert_eq!(puts(&seen).len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn catalogue_changes_trigger_one_upload_after_the_debounce() {
        let (port, seen) = serve(Box::new(|_, _| stored())).await;
        let f = fx(port, Tier::Basic).await;
        let writer = f.writer.clone();
        let job = tokio::spawn(f.task.with_debounce(Duration::from_millis(150)).run());
        let wait_for = async |n: usize| {
            for _ in 0..100 {
                if puts(&seen).len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("expected {n} uploads, saw {:?}", puts(&seen));
        };
        // The first look after start.
        wait_for(1).await;

        // A burst of edits is one upload.
        for name in ["One", "Two", "Three"] {
            add_group(&writer, name).await;
        }
        wait_for(2).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let sent = puts(&seen);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert!(sent[1].1.contains("One") && sent[1].1.contains("Three"));
        job.abort();
    }
}

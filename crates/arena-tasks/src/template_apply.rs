//! Applies the setup a new branch was told to start from (Story 5.38).
//!
//! `branch.bootstrap` (the pairing answer) and, after the owner presses Retry, a heartbeat answer
//! carry a short-lived link to another branch's stored setup file. [`SetupTemplateInbox`] hands
//! the link to [`SetupApplyTask`], which downloads the file, checks it against the SHA-256 in
//! the link, imports it through the Story 5.36 `import.setup` intent as the owner, and tells the
//! cloud how it went so the wizard can show Retry or the file path. The branch is already
//! bootstrapped by then: a failure here costs the template, never the pairing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{Clock, HubStatus, PairGate, PairingState, Role, WriterHandle};
use arena_proto::{
    Actor, ActorKind, ErrorCode, Intent, IntentPayload, SETUP_UPDATE_HUB, SetupTemplateLink,
    SetupTemplateOutcome,
};
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::keystore::Keystore;

/// How many times the file is fetched before the Hub tells the owner it could not.
pub const DOWNLOAD_ATTEMPTS: u32 = 3;
/// The wait after the first failed download; the second wait is twice as long.
pub const DOWNLOAD_RETRY: Duration = Duration::from_secs(3);
/// How long to wait before telling the cloud the outcome again when it could not be told.
pub const REPORT_RETRY: Duration = Duration::from_secs(30);

/// Where the pairing and the heartbeat leave the link for [`SetupApplyTask`]. A newer link
/// replaces one that has not been taken yet.
#[derive(Default)]
pub struct SetupTemplateInbox {
    slot: Mutex<Option<SetupTemplateLink>>,
    wake: Notify,
}

impl SetupTemplateInbox {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    pub fn offer(&self, link: SetupTemplateLink) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(link);
        }
        self.wake.notify_one();
    }

    pub(crate) fn take(&self) -> Option<SetupTemplateLink> {
        self.slot.lock().ok()?.take()
    }
}

pub struct SetupApplyTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    clock: Arc<dyn Clock>,
    inbox: Arc<SetupTemplateInbox>,
    download_retry: Duration,
    report_retry: Duration,
}

impl SetupApplyTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        clock: Arc<dyn Clock>,
        inbox: Arc<SetupTemplateInbox>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            gate,
            status,
            clock,
            inbox,
            download_retry: DOWNLOAD_RETRY,
            report_retry: REPORT_RETRY,
        }
    }

    pub fn with_waits(mut self, download_retry: Duration, report_retry: Duration) -> Self {
        self.download_retry = download_retry;
        self.report_retry = report_retry;
        self
    }

    pub async fn run(self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        loop {
            match self.inbox.take() {
                Some(link) => self.handle(&link).await,
                None => self.inbox.wake.notified().await,
            }
        }
    }

    /// Downloads, imports and reports one link.
    pub async fn handle(&self, link: &SetupTemplateLink) {
        let outcome = match self.import(link).await {
            Ok(()) => {
                tracing::info!("setup template imported");
                SetupTemplateOutcome {
                    applied: true,
                    message: None,
                }
            }
            Err(message) => {
                tracing::warn!(%message, "setup template not imported");
                SetupTemplateOutcome {
                    applied: false,
                    message: Some(message),
                }
            }
        };
        self.report(&outcome).await;
    }

    async fn import(&self, link: &SetupTemplateLink) -> Result<(), String> {
        let (Some(writer), Role::Primary) = (self.writer.clone(), self.status.borrow().role) else {
            return Err("This Hub is not the main Hub of the branch.".into());
        };
        if self.clock.now_ms() >= link.expires_at {
            return Err("The link expired before the Hub could use it.".into());
        }
        let bytes = self.download(&link.path).await?;
        if hex(&Sha256::digest(&bytes)) != link.sha256 {
            return Err("The saved setup changed while it was being sent.".into());
        }
        let text =
            String::from_utf8(bytes).map_err(|_| "The saved setup is not readable.".to_owned())?;
        let file = arena_core::setup_file::parse(&text).map_err(|e| match e.code {
            ErrorCode::Unsupported => SETUP_UPDATE_HUB.to_owned(),
            _ => e.message,
        })?;
        let tenant = writer
            .licence_state()
            .await
            .ok()
            .and_then(|s| s.identity)
            .map(|i| i.tenant_id)
            .ok_or_else(|| "This Hub is not paired.".to_owned())?;
        let intent = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: arena_proto::IntentKind::ImportSetup,
            idempotency_key: key_for(&link.sha256),
            issued_at: self.clock.now_ms(),
            base_rev: None,
            actor: Actor {
                kind: ActorKind::Owner,
                id: tenant,
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::ImportSetup(arena_proto::ImportSetup { file }),
        };
        match writer.submit_intent(intent).await {
            Ok(_) => Ok(()),
            // Nothing left to add, because the branch already has all of it.
            Err(e)
                if e.code == ErrorCode::ValidationFailed && e.field.as_deref() == Some("file") =>
            {
                Ok(())
            }
            Err(e) => Err(e.message),
        }
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut last = String::new();
        for attempt in 0..DOWNLOAD_ATTEMPTS {
            match self.cloud.download_setup_template(path).await {
                Ok(bytes) => return Ok(bytes),
                Err(CloudFailure::Refused { status: 404, .. }) => {
                    return Err("The link expired. Try again.".into());
                }
                Err(f) => {
                    tracing::warn!(attempt, failure = ?f, "setup template download failed");
                    last =
                        "The Hub could not download the setup. Check the internet and try again."
                            .into();
                }
            }
            if attempt + 1 < DOWNLOAD_ATTEMPTS {
                tokio::time::sleep(self.download_retry * (attempt + 1)).await;
            }
        }
        Err(last)
    }

    /// Tells the cloud until it has heard, or until this Hub is no longer paired.
    async fn report(&self, outcome: &SetupTemplateOutcome) {
        loop {
            if self.status.borrow().pairing == PairingState::Revoked {
                return;
            }
            let keystore = Arc::clone(&self.keystore);
            let token = tokio::task::spawn_blocking(move || keystore.get()).await;
            let branch_id = match &self.writer {
                Some(w) => w
                    .licence_state()
                    .await
                    .ok()
                    .and_then(|s| s.identity)
                    .map(|i| i.branch_id),
                None => None,
            };
            if let (Ok(Ok(Some(token))), Some(branch_id)) = (token, branch_id) {
                let token = String::from_utf8_lossy(&token).into_owned();
                match self
                    .cloud
                    .setup_template_outcome(&token, &branch_id, outcome)
                    .await
                {
                    Ok(()) => return,
                    Err(CloudFailure::Refused { error, .. })
                        if matches!(
                            error.code,
                            ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                        ) =>
                    {
                        self.status
                            .send_modify(|s| s.pairing = PairingState::Revoked);
                        return;
                    }
                    Err(f) => tracing::warn!(failure = ?f, "setup template outcome not sent"),
                }
            }
            tokio::time::sleep(self.report_retry).await;
        }
    }
}

/// The idempotency key of the import of one file: the same file imported twice is one intent.
fn key_for(sha256: &str) -> String {
    let d = Sha256::digest(format!("setup-template\0{sha256}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[0] &= 0x7f;
    ulid::Ulid::from(u128::from_be_bytes(b)).to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{ManualClock, SnapshotReader, status_channel};
    use arena_proto::Tier;
    use arena_proto::licence::{LicenceClaims, dev_sign};
    use arena_store::Store;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
    const FILE: &str = r#"{"format":"arena360-setup","version":1,"currency":"INR","groups":[{"name":"Row A","kind":"pc","colour":"teal","sort_order":1,"rate_minor":12000}]}"#;
    const PATH: &str = "/v1/setup-template/B2.1.2.3";

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

    async fn serve(mut handler: Handler) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = text.lines().next().unwrap_or("").to_owned();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
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

    fn link(file: &str) -> SetupTemplateLink {
        SetupTemplateLink {
            path: PATH.into(),
            version: 1,
            sha256: hex(&Sha256::digest(file.as_bytes())),
            bytes: file.len() as u64,
            expires_at: T0 + 60_000,
        }
    }

    fn answer(file: &'static str) -> Handler {
        Box::new(move |line, _| {
            if line.starts_with("GET") {
                (200, file.to_owned())
            } else {
                (200, r#"{"status":"applied"}"#.to_owned())
            }
        })
    }

    fn outcomes(seen: &Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(line, _)| line.starts_with("POST /v1/branches/B/setup-template/outcome"))
            .map(|(_, body)| serde_json::from_str(body).unwrap())
            .collect()
    }

    fn gets(seen: &Seen) -> usize {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(line, _)| line.starts_with("GET"))
            .count()
    }

    struct Fx {
        _dir: tempfile::TempDir,
        task: SetupApplyTask,
        snapshots: SnapshotReader,
        status: watch::Sender<HubStatus>,
    }

    async fn fx(port: u16) -> Fx {
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
        let claims = LicenceClaims::for_tier("T", "B", Tier::Basic, T0 / 1000);
        writer
            .record_pairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H".into(),
                    tier: Tier::Basic,
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
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = SetupApplyTask::new(
            cloud,
            keystore,
            Some(writer),
            PairGate::new(true),
            status.clone(),
            clock,
            SetupTemplateInbox::new(),
        )
        .with_waits(Duration::from_millis(1), Duration::from_millis(5));
        Fx {
            snapshots: SnapshotReader::open(&path).unwrap(),
            _dir: dir,
            task,
            status,
        }
    }

    fn groups(f: &Fx) -> Vec<String> {
        let text = f.snapshots.setup_export(T0).unwrap();
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        file["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_link_is_downloaded_imported_and_reported_applied() {
        let (port, seen) = serve(answer(FILE)).await;
        let f = fx(port).await;
        f.task.handle(&link(FILE)).await;

        assert_eq!(groups(&f), ["Row A"]);
        let sent = outcomes(&seen);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["applied"], true);

        // The same link again adds nothing and still reports applied.
        f.task.handle(&link(FILE)).await;
        assert_eq!(groups(&f), ["Row A"]);
        assert_eq!(outcomes(&seen).len(), 2);
        assert_eq!(outcomes(&seen)[1]["applied"], true);
    }

    #[tokio::test]
    async fn a_failed_download_is_retried_then_reported_failed_without_touching_the_branch() {
        let (port, seen) = serve(Box::new(|line, _| {
            if line.starts_with("GET") {
                (503, r#"{"code":"INTERNAL","message":"x"}"#.into())
            } else {
                (200, r#"{"status":"failed"}"#.into())
            }
        }))
        .await;
        let f = fx(port).await;
        f.task.handle(&link(FILE)).await;

        assert_eq!(gets(&seen), DOWNLOAD_ATTEMPTS as usize);
        assert!(groups(&f).is_empty());
        let sent = outcomes(&seen);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["applied"], false);
        assert!(sent[0]["message"].as_str().unwrap().contains("download"));
    }

    #[tokio::test]
    async fn an_expired_link_is_not_followed_and_a_changed_file_is_refused() {
        let (port, seen) = serve(Box::new(|line, _| {
            if line.starts_with("GET") {
                (404, r#"{"code":"NOT_FOUND","message":"gone"}"#.into())
            } else {
                (200, r#"{"status":"failed"}"#.into())
            }
        }))
        .await;
        let f = fx(port).await;
        let mut old = link(FILE);
        old.expires_at = T0;
        f.task.handle(&old).await;
        assert_eq!(gets(&seen), 0, "an expired link is not even tried");

        // The cloud says it is gone: one try, no retries.
        f.task.handle(&link(FILE)).await;
        assert_eq!(gets(&seen), 1);
        assert_eq!(outcomes(&seen).len(), 2);
        assert!(outcomes(&seen).iter().all(|o| o["applied"] == false));

        // A file whose hash is not the link's.
        let (port, seen) = serve(answer(FILE)).await;
        let g = fx(port).await;
        let mut wrong = link(FILE);
        wrong.sha256 = "00".repeat(32);
        g.task.handle(&wrong).await;
        assert!(groups(&g).is_empty());
        assert_eq!(outcomes(&seen)[0]["applied"], false);
    }

    #[tokio::test]
    async fn a_file_the_hub_cannot_import_is_reported_with_its_reason() {
        const NEWER: &str = r#"{"format":"arena360-setup","version":99,"currency":"INR"}"#;
        let (port, seen) = serve(answer(NEWER)).await;
        let f = fx(port).await;
        f.task.handle(&link(NEWER)).await;
        let sent = outcomes(&seen);
        assert_eq!(sent[0]["applied"], false);
        assert_eq!(sent[0]["message"], SETUP_UPDATE_HUB);

        const OTHER: &str = r#"{"format":"arena360-setup","version":1,"currency":"USD"}"#;
        let (port, seen) = serve(answer(OTHER)).await;
        let f = fx(port).await;
        f.task.handle(&link(OTHER)).await;
        assert_eq!(outcomes(&seen)[0]["applied"], false);
    }

    #[tokio::test]
    async fn an_outcome_the_cloud_did_not_hear_is_sent_again_and_a_revoked_hub_stops() {
        let mut tries = 0;
        let (port, seen) = serve(Box::new(move |line, _| {
            if line.starts_with("GET") {
                return (200, FILE.to_owned());
            }
            tries += 1;
            if tries < 3 {
                (503, r#"{"code":"INTERNAL","message":"x"}"#.into())
            } else {
                (200, r#"{"status":"applied"}"#.into())
            }
        }))
        .await;
        let f = fx(port).await;
        f.task.handle(&link(FILE)).await;
        assert_eq!(outcomes(&seen).len(), 3, "two refusals and one success");

        let (port, seen) = serve(Box::new(|line, _| {
            if line.starts_with("GET") {
                (200, FILE.to_owned())
            } else {
                (401, r#"{"code":"TOKEN_REVOKED","message":"x"}"#.into())
            }
        }))
        .await;
        let g = fx(port).await;
        g.task.handle(&link(FILE)).await;
        assert_eq!(outcomes(&seen).len(), 1);
        assert_eq!(g.status.borrow().pairing, PairingState::Revoked);
    }

    #[tokio::test]
    async fn the_task_takes_a_link_left_before_and_after_it_starts() {
        let (port, seen) = serve(answer(FILE)).await;
        let f = fx(port).await;
        let inbox = Arc::clone(&f.task.inbox);
        inbox.offer(link(FILE));
        let job = tokio::spawn(f.task.run());
        for _ in 0..100 {
            if !outcomes(&seen).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(outcomes(&seen).len(), 1);

        inbox.offer(link(FILE));
        for _ in 0..100 {
            if outcomes(&seen).len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(outcomes(&seen).len(), 2);
        job.abort();
    }
}

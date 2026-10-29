use std::sync::Arc;
use std::time::Duration;

use arena_core::{
    BACKUP_NOW, HubStatus, PairGate, PairingState, Role, WriterHandle, backup_now_check,
};
use arena_proto::{
    AppError, COMMANDS_PAGE, CommandResult, CommandWire, ErrorCode, HEARTBEAT_LIVE_INTERVAL_S, Tier,
};
use arena_store::AppliedCommand;
use tokio::sync::{Notify, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{Presence, REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;
use crate::upload::{BACKUP_NOW_WAIT, BackupRequester};

pub const COMMAND_POLL_INTERVAL: Duration = Duration::from_secs(60);
const REPORT_BATCH: usize = 50;

pub struct CommandTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    poke: Arc<Notify>,
    presence: Option<Presence>,
    interval: Duration,
    fails: u32,
    backup: Option<BackupRequester>,
    backup_wait: Duration,
}

enum Stop {
    Revoked,
    Failed,
}

impl CommandTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        poke: Arc<Notify>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            gate,
            status,
            poke,
            presence: None,
            interval: COMMAND_POLL_INTERVAL,
            fails: 0,
            backup: None,
            backup_wait: BACKUP_NOW_WAIT,
        }
    }

    pub fn with_presence(mut self, presence: Presence) -> Self {
        self.presence = Some(presence);
        self
    }

    /// Where `backup.now` asks the upload task for an immediate cycle (Story 8.19).
    pub fn with_backup(mut self, backup: BackupRequester) -> Self {
        self.backup = Some(backup);
        self
    }

    /// Tests: how long `backup.now` waits for its cycle.
    pub fn with_backup_wait(mut self, wait: Duration) -> Self {
        self.backup_wait = wait;
        self
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        loop {
            let wait = self.poll().await;
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.poke.notified() => {}
            }
        }
    }

    pub async fn poll(&mut self) -> Duration {
        let (role, lease, pairing) = {
            let s = self.status.borrow();
            (s.role, s.lease.clone(), s.pairing)
        };
        let (Some(writer), Some(lease), Role::Primary) = (self.writer.clone(), lease, role) else {
            return self.interval;
        };
        if pairing == PairingState::Revoked {
            return REVOKED_BACKOFF;
        }
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "commands: no hub token");
                return self.interval;
            }
        };
        match self.sync(&writer, &token, &lease.epoch().to_string()).await {
            Ok(()) => {
                self.fails = 0;
                self.interval
            }
            Err(Stop::Revoked) => {
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                REVOKED_BACKOFF
            }
            Err(Stop::Failed) => {
                self.fails += 1;
                backoff(
                    self.fails,
                    self.interval
                        .min(Duration::from_secs(HEARTBEAT_LIVE_INTERVAL_S)),
                    unit(),
                )
            }
        }
    }

    async fn sync(&self, writer: &WriterHandle, token: &str, epoch: &str) -> Result<(), Stop> {
        let open = writer
            .unreported_commands(REPORT_BATCH)
            .await
            .map_err(|e| failed("read unreported results", &e))?;
        for row in open {
            self.report(writer, token, epoch, &row).await?;
        }
        loop {
            let cursor = writer
                .command_cursor()
                .await
                .map_err(|e| failed("read the cursor", &e))?;
            let page = self
                .cloud
                .commands(token, cursor, epoch)
                .await
                .map_err(|f| cloud_failed("fetch", f))?;
            let n = page.commands.len();
            for cmd in page.commands.into_iter().filter(|c| c.id > cursor) {
                self.run_one(writer, token, epoch, cmd).await?;
            }
            if n < COMMANDS_PAGE {
                return Ok(());
            }
        }
    }

    async fn run_one(
        &self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        cmd: CommandWire,
    ) -> Result<(), Stop> {
        let (id, kind) = (cmd.id, cmd.kind.clone());
        let offline = self.offline_target(&cmd);
        let row = match offline {
            Some(error) => writer.reject_command(cmd, error).await,
            None if cmd.kind == BACKUP_NOW => self.backup_now(writer, cmd).await,
            None => writer.apply_command(cmd).await,
        }
        .map_err(|e| failed("apply a command", &e))?;
        tracing::info!(command_id = id, kind = %kind, applied = row.applied, "owner command resolved");
        self.report(writer, token, epoch, &row).await
    }

    /// `backup.now` runs outside `apply()`: an upload and summary cycle at once, answered `applied`
    /// when its uploads succeeded and `rejected` with the transport error otherwise. It writes no
    /// change rows. A command already answered is not run again.
    async fn backup_now(
        &self,
        writer: &WriterHandle,
        cmd: CommandWire,
    ) -> Result<AppliedCommand, AppError> {
        if let Some(done) = writer
            .recorded_command(cmd.id, cmd.idempotency_key.clone())
            .await?
        {
            return Ok(done);
        }
        let tier = self.status.borrow().tier;
        let outcome = match backup_now_check(&cmd) {
            Err(e) => Err(e),
            Ok(()) => match (&self.backup, tier) {
                (Some(backup), Some(tier)) if tier != Tier::Free => {
                    backup.run(self.backup_wait).await
                }
                _ => Err(AppError::new(
                    ErrorCode::FeatureNotEntitled,
                    "this branch does not upload",
                )),
            },
        };
        writer.record_command(cmd, outcome.err()).await
    }

    fn offline_target(&self, cmd: &CommandWire) -> Option<AppError> {
        let presence = self.presence.as_ref()?;
        let device = cmd.payload.get("device_id")?.as_str()?;
        (cmd.kind == "setup.test_session" && !presence(device).0).then(|| {
            AppError::with_field(
                ErrorCode::ValidationFailed,
                "device_id",
                "the device is offline",
            )
        })
    }

    async fn report(
        &self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        row: &AppliedCommand,
    ) -> Result<(), Stop> {
        let body = CommandResult {
            epoch: epoch.to_owned(),
            applied_seq: row.applied_seq,
            error: row.error.as_deref().map(parse_error),
        };
        match self
            .cloud
            .command_result(token, row.command_id, &body)
            .await
        {
            Ok(_) => {}
            Err(CloudFailure::Refused { error, .. }) if error.code == ErrorCode::NotFound => {
                tracing::warn!(
                    command_id = row.command_id,
                    "commands: the cloud no longer has it"
                );
            }
            Err(f) => return Err(cloud_failed("report", f)),
        }
        writer
            .mark_command_reported(row.command_id)
            .await
            .map_err(|e| failed("mark a result reported", &e))
    }
}

fn parse_error(raw: &str) -> AppError {
    serde_json::from_str(raw)
        .unwrap_or_else(|_| AppError::new(ErrorCode::Internal, "the command failed at the branch"))
}

fn failed(what: &str, e: &AppError) -> Stop {
    tracing::warn!(code = e.code.as_str(), "commands: could not {what}");
    Stop::Failed
}

fn cloud_failed(what: &str, f: CloudFailure) -> Stop {
    if let CloudFailure::Refused { error, .. } = &f
        && matches!(
            error.code,
            ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
        )
    {
        tracing::warn!(
            code = error.code.as_str(),
            "commands: this hub is no longer paired"
        );
        return Stop::Revoked;
    }
    tracing::warn!(failure = ?f, "commands: {what} failed");
    Stop::Failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{Clock, ManualClock, status_channel};
    use arena_proto::Tier;
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

    fn command(id: i64, name: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "idempotency_key": ulid::Ulid::generate().to_string(),
            "kind": "group.upsert",
            "schema_version": 1,
            "payload": {"name": name, "group_kind": "pc", "colour": "teal", "sort_order": id},
            "issued_by": "owner-1",
        })
    }

    fn page(commands: Vec<serde_json::Value>) -> (u16, String) {
        (200, serde_json::json!({"commands": commands}).to_string())
    }

    fn ack() -> (u16, String) {
        (200, r#"{"status":"applied"}"#.to_owned())
    }

    struct Fx {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        status: watch::Sender<HubStatus>,
        task: CommandTask,
        writer: WriterHandle,
    }

    async fn fx(port: u16) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(12, OWN));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
        let lease = LeaseTask::start(store, OWN.into(), clock, status.clone())
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
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = CommandTask::new(
            cloud,
            keystore,
            Some(writer.clone()),
            PairGate::new(true),
            status.clone(),
            Arc::new(Notify::new()),
        );
        Fx {
            _dir: dir,
            path,
            status,
            task,
            writer,
        }
    }

    fn group_names(path: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = Store::open(path, T0)
            .unwrap()
            .rows(arena_store::Table::DeviceGroups)
            .unwrap()
            .into_iter()
            .filter_map(|r| match &r["name"] {
                arena_store::RowValue::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        names.sort();
        names
    }

    fn calls(seen: &Seen, prefix: &str) -> Vec<(String, String)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(line, _)| line.starts_with(prefix))
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn fetched_commands_apply_in_order_and_each_result_is_posted() {
        let mut served = false;
        let (port, seen) = serve(Box::new(move |line, _| {
            if line.starts_with("GET /v1/commands") {
                if served {
                    return page(vec![]);
                }
                served = true;
                return page(vec![command(1, "Row A"), command(2, "Row B")]);
            }
            ack()
        }))
        .await;
        let mut f = fx(port).await;
        assert_eq!(f.task.poll().await, COMMAND_POLL_INTERVAL);
        assert_eq!(group_names(&f.path), ["Row A", "Row B"]);
        let posts = calls(&seen, "POST");
        assert_eq!(posts.len(), 2);
        assert!(posts[0].0.contains("/v1/commands/1/result"), "{posts:?}");
        assert!(posts[1].0.contains("/v1/commands/2/result"), "{posts:?}");
        assert!(posts[0].1.contains(r#""applied_seq""#), "{posts:?}");
        assert!(
            posts[0].1.contains(&format!(r#""epoch":"1:{OWN}""#)),
            "{posts:?}"
        );
        assert!(f.writer.unreported_commands(10).await.unwrap().is_empty());
        f.task.poll().await;
        let gets = calls(&seen, "GET");
        assert!(gets[0].0.contains("after=0"), "{gets:?}");
        assert!(gets.last().unwrap().0.contains("after=2"), "{gets:?}");
        assert_eq!(group_names(&f.path).len(), 2);
    }

    #[tokio::test]
    async fn a_lost_result_is_posted_again_and_the_command_is_not_applied_twice() {
        let mut posts = 0;
        let mut served = false;
        let (port, seen) = serve(Box::new(move |line, _| {
            if line.starts_with("GET /v1/commands") {
                if served {
                    return page(vec![]);
                }
                served = true;
                return page(vec![command(1, "Row A")]);
            }
            posts += 1;
            if posts == 1 {
                (500, r#"{"code":"INTERNAL","message":"x"}"#.to_owned())
            } else {
                ack()
            }
        }))
        .await;
        let mut f = fx(port).await;
        assert!(f.task.poll().await < COMMAND_POLL_INTERVAL);
        assert_eq!(group_names(&f.path), ["Row A"]);
        assert_eq!(f.writer.unreported_commands(10).await.unwrap().len(), 1);
        assert_eq!(f.task.poll().await, COMMAND_POLL_INTERVAL);
        assert!(f.writer.unreported_commands(10).await.unwrap().is_empty());
        assert_eq!(group_names(&f.path), ["Row A"]);
        assert_eq!(calls(&seen, "POST").len(), 2);
    }

    #[tokio::test]
    async fn a_command_rejected_at_the_hub_posts_its_error() {
        let mut served = false;
        let (port, seen) = serve(Box::new(move |line, _| {
            if line.starts_with("GET /v1/commands") && !served {
                served = true;
                let mut bad = command(4, "x");
                bad["kind"] = "device.assign".into();
                bad["payload"] = serde_json::json!({"device_id": "01JNOSUCHDEVICE"});
                return page(vec![bad]);
            }
            if line.starts_with("GET") {
                return page(vec![]);
            }
            ack()
        }))
        .await;
        let mut f = fx(port).await;
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert_eq!(posts.len(), 1);
        assert!(posts[0].1.contains("VALIDATION_FAILED"), "{posts:?}");
        assert!(!posts[0].1.contains("applied_seq"), "{posts:?}");
        assert_eq!(f.writer.command_cursor().await.unwrap(), 4);
    }

    #[tokio::test]
    async fn a_test_session_for_an_offline_device_is_rejected_unapplied() {
        let mut served = false;
        let (port, seen) = serve(Box::new(move |line, _| {
            if line.starts_with("GET /v1/commands") && !served {
                served = true;
                let mut c = command(7, "x");
                c["kind"] = "setup.test_session".into();
                c["payload"] = serde_json::json!({"device_id": "01JKIOSKAAAAAAAAAAAAAAAAAA"});
                return page(vec![c]);
            }
            if line.starts_with("GET") {
                return page(vec![]);
            }
            ack()
        }))
        .await;
        let mut f = fx(port).await;
        f.task = f.task.with_presence(Arc::new(|_| (false, None)));
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert_eq!(posts.len(), 1);
        assert!(posts[0].1.contains("the device is offline"), "{posts:?}");
        assert!(!posts[0].1.contains("applied_seq"), "{posts:?}");
        assert_eq!(f.writer.command_cursor().await.unwrap(), 7);
        assert!(f.writer.unreported_commands(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_revoked_token_marks_the_hub_unpaired_and_backs_off() {
        let (port, _) = serve(Box::new(|_, _| {
            (401, r#"{"code":"TOKEN_REVOKED","message":"no"}"#.to_owned())
        }))
        .await;
        let mut f = fx(port).await;
        assert_eq!(f.task.poll().await, REVOKED_BACKOFF);
        assert_eq!(f.status.borrow().pairing, PairingState::Revoked);
        assert_eq!(f.task.poll().await, REVOKED_BACKOFF);
    }

    #[tokio::test]
    async fn nothing_is_fetched_without_a_lease_or_while_a_follower() {
        let (port, seen) = serve(Box::new(|_, _| page(vec![]))).await;
        let mut f = fx(port).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        assert_eq!(f.task.poll().await, COMMAND_POLL_INTERVAL);
        assert!(seen.lock().unwrap().is_empty());
    }

    fn backup_command(id: i64, payload: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "idempotency_key": ulid::Ulid::generate().to_string(),
            "kind": "backup.now",
            "schema_version": 1,
            "payload": payload,
            "issued_by": "owner-1",
        })
    }

    /// A cloud that serves one `backup.now` command, then nothing.
    async fn backup_cloud(payload: serde_json::Value) -> (u16, Seen) {
        let mut served = false;
        serve(Box::new(move |line, _| {
            if line.starts_with("GET /v1/commands") && !served {
                served = true;
                return page(vec![backup_command(5, payload.clone())]);
            }
            if line.starts_with("GET") {
                return page(vec![]);
            }
            ack()
        }))
        .await
    }

    /// Answers each `backup.now` ask with `answer`, and counts them.
    fn answering(
        mut responder: crate::upload::BackupResponder,
        answer: Result<(), AppError>,
    ) -> Arc<std::sync::atomic::AtomicUsize> {
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&asked);
        tokio::spawn(async move {
            while let Some(reply) = responder.rx.recv().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = reply.send(answer.clone());
            }
        });
        asked
    }

    fn with_uploader(f: Fx, tier: Tier) -> (Fx, crate::upload::BackupResponder) {
        let (requester, responder) = crate::upload::backup_channel();
        let Fx {
            _dir,
            path,
            status,
            task,
            writer,
        } = f;
        status.send_modify(|s| s.tier = Some(tier));
        let task = task
            .with_backup(requester)
            .with_backup_wait(Duration::from_millis(200));
        (
            Fx {
                _dir,
                path,
                status,
                task,
                writer,
            },
            responder,
        )
    }

    #[tokio::test]
    async fn backup_now_is_applied_when_its_cycle_succeeds_and_writes_no_change_row() {
        let (port, seen) = backup_cloud(serde_json::json!({})).await;
        let (mut f, responder) = with_uploader(fx(port).await, Tier::Basic);
        let asked = answering(responder, Ok(()));
        let rows = Store::open(&f.path, T0).unwrap().changes().unwrap().len();
        f.task.poll().await;
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        let posts = calls(&seen, "POST");
        assert_eq!(posts.len(), 1, "{posts:?}");
        assert!(posts[0].0.contains("/v1/commands/5/result"), "{posts:?}");
        assert!(!posts[0].1.contains("applied_seq"), "{posts:?}");
        assert!(!posts[0].1.contains("error"), "{posts:?}");
        assert_eq!(
            Store::open(&f.path, T0).unwrap().changes().unwrap().len(),
            rows
        );
        assert!(f.writer.unreported_commands(10).await.unwrap().is_empty());
        assert_eq!(f.writer.command_cursor().await.unwrap(), 5);
    }

    #[tokio::test]
    async fn backup_now_is_rejected_with_the_transport_error_of_its_cycle() {
        let (port, seen) = backup_cloud(serde_json::json!({})).await;
        let (mut f, responder) = with_uploader(fx(port).await, Tier::Premium);
        answering(
            responder,
            Err(AppError::new(ErrorCode::CloudUnreachable, "no route")),
        );
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert_eq!(posts.len(), 1, "{posts:?}");
        assert!(posts[0].1.contains("CLOUD_UNREACHABLE"), "{posts:?}");
    }

    #[tokio::test]
    async fn backup_now_is_refused_on_free_with_extra_fields_and_when_the_cycle_hangs() {
        let (port, seen) = backup_cloud(serde_json::json!({})).await;
        let (mut f, responder) = with_uploader(fx(port).await, Tier::Free);
        let asked = answering(responder, Ok(()));
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert!(posts[0].1.contains("FEATURE_NOT_ENTITLED"), "{posts:?}");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (port, seen) = backup_cloud(serde_json::json!({"path": "/tmp"})).await;
        let (mut f, responder) = with_uploader(fx(port).await, Tier::Basic);
        let asked = answering(responder, Ok(()));
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert!(posts[0].1.contains("VALIDATION_FAILED"), "{posts:?}");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (port, seen) = backup_cloud(serde_json::json!({})).await;
        let (mut f, responder) = with_uploader(fx(port).await, Tier::Basic);
        let _held = responder;
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert!(posts[0].1.contains("HUB_SLOW"), "{posts:?}");

        let (port, seen) = backup_cloud(serde_json::json!({})).await;
        let f = fx(port).await;
        f.status.send_modify(|s| s.tier = Some(Tier::Basic));
        let mut f = f;
        f.task.poll().await;
        let posts = calls(&seen, "POST");
        assert!(posts[0].1.contains("FEATURE_NOT_ENTITLED"), "{posts:?}");
    }
}

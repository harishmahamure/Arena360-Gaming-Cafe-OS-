//! Ships the branch's change log to the cloud (Story 8.2).
//!
//! A paid primary Hub wakes every `upload_interval_s` of its licence (never under 30 s), or as
//! soon as 500 change rows are waiting, and uploads what the `upload_cursor` has not covered:
//! up to 5,000 rows of one epoch per batch, as a zstd file (see `arena_store::encode_batch`),
//! PUT to a link `POST /v1/presign` issued for exactly that `first`..`last`. A backlog goes back
//! to back, one batch after another. The cursor moves, through the Writer, only after the PUT
//! answered 2xx; a failed cycle leaves it, so the next one rebuilds the same batch under the
//! same key (a repeated PUT of the same bytes is harmless).
//!
//! A Free Hub (or one whose tier is not known yet), a follower and an unpaired Hub make no
//! request at all. The task reads through the [`SnapshotReader`], never inside `apply()`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::{
    HubStatus, PairGate, PairingState, Role, SnapshotReader, UploadStatus, WriterHandle,
};
use arena_proto::{
    AppError, ErrorCode, PresignBatch, PresignRequest, ResendUnavailable, SeqRange, Tier,
    VerifyKey, licence::verify_claims,
};
use arena_store::{BATCH_MAX_ROWS, RangeRead, encode_batch};
use tokio::sync::{broadcast, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;
use crate::template::{NOT_ENTITLED_RETRY, TIER_RECHECK};

/// The shortest time between two scheduled uploads, whatever the licence says.
pub const MIN_UPLOAD_INTERVAL: Duration = Duration::from_secs(30);
/// Waiting change rows that wake the task before its interval is up.
pub const BACKLOG_TRIGGER_ROWS: i64 = 500;
/// How long to wait when the cloud limits the calls but names no time.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);
/// The longest Retry-After honoured.
const RATE_LIMIT_CEILING: Duration = Duration::from_secs(3600);

/// What the task does after a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Next {
    /// How long to sleep.
    pub wait: Duration,
    /// This Hub is uploading (paid, primary): waiting rows are counted.
    pub active: bool,
    /// A pile of waiting rows may cut the wait short (the last cycle did not fail).
    pub early: bool,
}

impl Next {
    fn idle(wait: Duration) -> Self {
        Self {
            wait,
            active: false,
            early: false,
        }
    }

    fn backing_off(wait: Duration) -> Self {
        Self {
            wait,
            active: true,
            early: false,
        }
    }
}

/// The run of the change log the cloud asked for again (Story 8.3) and the run the Hub could not
/// give. The heartbeat task fills it from its answers and reports from it; the uploader empties it.
#[derive(Default)]
pub struct ResendInbox {
    state: Mutex<ResendState>,
}

#[derive(Default)]
struct ResendState {
    pending: Option<(String, SeqRange)>,
    unavailable: Option<ResendUnavailable>,
}

impl ResendInbox {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut ResendState) -> R) -> R {
        f(&mut self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    /// The cloud asked for the range of the epoch. The same ask again (the answer to a heartbeat
    /// sent before the resend landed) keeps the progress already made.
    pub fn offer(&self, epoch: &str, range: SeqRange) {
        if !range.valid() {
            return;
        }
        self.with(|s| {
            let same = s
                .pending
                .as_ref()
                .is_some_and(|(e, r)| e == epoch && r.to == range.to && range.from <= r.from);
            if !same {
                s.pending = Some((epoch.to_owned(), range));
            }
        });
    }

    /// What is still to resend.
    pub fn pending(&self) -> Option<(String, SeqRange)> {
        self.with(|s| s.pending.clone())
    }

    /// Everything up to the given seq of the pending run is done.
    fn advance(&self, through: i64) {
        self.with(|s| {
            if let Some((_, r)) = s.pending.as_mut() {
                r.from = r.from.max(through.saturating_add(1));
                if r.from > r.to {
                    s.pending = None;
                }
            }
        });
    }

    fn drop_pending(&self) {
        self.with(|s| s.pending = None);
    }

    pub(crate) fn report_unavailable(&self, u: ResendUnavailable) {
        self.with(|s| s.unavailable = Some(u));
    }

    /// The run to tell the cloud about in the next heartbeat.
    pub fn unavailable(&self) -> Option<ResendUnavailable> {
        self.with(|s| s.unavailable.clone())
    }

    /// The heartbeat that carried the run was answered: it is not reported again.
    pub fn acknowledge(&self, sent: &ResendUnavailable) {
        self.with(|s| {
            if s.unavailable.as_ref() == Some(sent) {
                s.unavailable = None;
            }
        });
    }
}

/// A batch read and encoded, ready to presign and PUT.
struct Built {
    epoch: String,
    first: i64,
    last: i64,
    rows: usize,
    bytes: Vec<u8>,
}

fn build(snapshots: &SnapshotReader, limit: usize, schema: u32) -> Result<Option<Built>, AppError> {
    let Some(batch) = snapshots.pending_batch(limit)? else {
        return Ok(None);
    };
    let bytes = encode_batch(schema, &batch)?;
    Ok(Some(Built {
        rows: batch.rows.len(),
        epoch: batch.epoch,
        first: batch.first,
        last: batch.last,
        bytes,
    }))
}

pub struct UploadTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    snapshots: Arc<SnapshotReader>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    /// The licence verify keys, to read `upload_interval_s` from the stored licence.
    keys: Vec<VerifyKey<'static>>,
    batch_rows: usize,
    trigger_rows: i64,
    trigger_gap: Duration,
    fails: u32,
    resend: Arc<ResendInbox>,
}

impl UploadTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        snapshots: Arc<SnapshotReader>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        keys: Vec<VerifyKey<'static>>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            snapshots,
            gate,
            status,
            keys,
            batch_rows: BATCH_MAX_ROWS,
            trigger_rows: BACKLOG_TRIGGER_ROWS,
            trigger_gap: MIN_UPLOAD_INTERVAL,
            fails: 0,
            resend: ResendInbox::new(),
        }
    }

    /// Where the heartbeat leaves the runs the cloud asked for again (Story 8.3).
    pub fn with_resend(mut self, inbox: Arc<ResendInbox>) -> Self {
        self.resend = inbox;
        self
    }

    /// Tests: the rows per batch.
    pub fn with_batch_rows(mut self, rows: usize) -> Self {
        self.batch_rows = rows.clamp(1, BATCH_MAX_ROWS);
        self
    }

    /// Tests: how many waiting rows wake the task early, and the least time after a cycle they
    /// may do so.
    pub fn with_trigger(mut self, rows: i64, gap: Duration) -> Self {
        self.trigger_rows = rows;
        self.trigger_gap = gap;
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
        loop {
            let next = self.cycle().await;
            let ended = tokio::time::Instant::now();
            let deadline = ended + next.wait;
            let mut pending = self.status.borrow().upload.pending_rows;
            let mut triggered = next.early && pending >= self.trigger_rows;
            loop {
                let wake = if triggered {
                    deadline.min(ended + self.trigger_gap)
                } else {
                    deadline
                };
                tokio::select! {
                    () = tokio::time::sleep_until(wake) => break,
                    event = events.recv() => {
                        match event {
                            Ok(applied) => pending += applied.changes.len() as i64,
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                if let Some(p) = self.progress().await {
                                    pending = p.pending_rows;
                                }
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                        if next.active {
                            self.status.send_if_modified(|s| {
                                let changed = s.upload.pending_rows != pending;
                                s.upload.pending_rows = pending;
                                changed
                            });
                            triggered |= next.early && pending >= self.trigger_rows;
                        }
                    }
                }
            }
        }
    }

    /// One wake: uploads every waiting batch, one after another, until nothing is left or one
    /// fails. Answers what to do next.
    pub async fn cycle(&mut self) -> Next {
        let (role, lease, pairing, tier, schema) = {
            let s = self.status.borrow();
            (s.role, s.lease.clone(), s.pairing, s.tier, s.schema_version)
        };
        let (Some(writer), Some(lease), Role::Primary) = (self.writer.clone(), lease, role) else {
            return Next::idle(TIER_RECHECK);
        };
        if pairing == PairingState::Revoked {
            return Next::idle(REVOKED_BACKOFF);
        }
        let Some(tier) = tier.filter(|t| *t != Tier::Free) else {
            return Next::idle(TIER_RECHECK);
        };
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "upload: no hub token");
                return Next::idle(TIER_RECHECK);
            }
        };
        let interval = self.interval(&writer, tier).await;
        if let Some(next) = self
            .resend_pass(&token, &lease.epoch().to_string(), interval, schema)
            .await
        {
            self.refresh_progress().await;
            return next;
        }
        loop {
            let snapshots = Arc::clone(&self.snapshots);
            let rows = self.batch_rows;
            let built = match tokio::task::spawn_blocking(move || build(&snapshots, rows, schema))
                .await
            {
                Ok(Ok(Some(b))) => b,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    tracing::warn!(code = e.code.as_str(), error = %e, "upload: could not read a batch");
                    self.refresh_progress().await;
                    return self.failed(interval);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "upload: read task failed");
                    return self.failed(interval);
                }
            };
            if let Some(next) = self.send(&writer, &token, built, interval).await {
                self.refresh_progress().await;
                return next;
            }
            self.refresh_progress().await;
        }
        self.refresh_progress().await;
        Next {
            wait: interval,
            active: true,
            early: true,
        }
    }

    /// Sends again the run the cloud asked for (Story 8.3), batch by batch under keys for exactly
    /// its seqs, and never touches the cursor. Rows that were pruned are left for the next
    /// heartbeat to report. `None` when nothing is left to do; otherwise what to do instead.
    async fn resend_pass(
        &mut self,
        token: &str,
        lease_epoch: &str,
        interval: Duration,
        schema: u32,
    ) -> Option<Next> {
        while let Some((epoch, range)) = self.resend.pending() {
            if epoch != lease_epoch {
                tracing::info!(%epoch, "upload: dropping a resend of another epoch");
                self.resend.drop_pending();
                return None;
            }
            let snapshots = Arc::clone(&self.snapshots);
            let rows = self.batch_rows;
            let read_epoch = epoch.clone();
            let read = tokio::task::spawn_blocking(move || {
                snapshots.range_batch(&read_epoch, range.from, range.to, rows)
            })
            .await;
            match read {
                Ok(Ok(RangeRead::Unavailable { from, to })) => {
                    tracing::warn!(%epoch, from, to, "upload: rows to resend are no longer held");
                    self.resend
                        .report_unavailable(ResendUnavailable { epoch, from, to });
                    self.resend.advance(to);
                }
                Ok(Ok(RangeRead::Batch(batch))) => {
                    let bytes = match encode_batch(schema, &batch) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!(
                                code = e.code.as_str(),
                                "upload: could not encode a resend"
                            );
                            return Some(self.failed(interval));
                        }
                    };
                    let built = Built {
                        rows: batch.rows.len(),
                        epoch: batch.epoch,
                        first: batch.first,
                        last: batch.last,
                        bytes,
                    };
                    let (first, last, count) = (built.first, built.last, built.rows);
                    if let Err(next) = self.put(token, built, interval).await {
                        return Some(next);
                    }
                    tracing::info!(%epoch, first, last, rows = count, "change log batch resent");
                    self.fails = 0;
                    self.resend.advance(last);
                }
                Ok(Err(e)) => {
                    tracing::warn!(code = e.code.as_str(), error = %e, "upload: could not read a resend");
                    return Some(self.failed(interval));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "upload: resend read task failed");
                    return Some(self.failed(interval));
                }
            }
        }
        None
    }

    /// Presigns and PUTs one batch; the error is what to do instead of going on.
    async fn put(&mut self, token: &str, batch: Built, interval: Duration) -> Result<(), Next> {
        let req = PresignRequest {
            epoch: batch.epoch.clone(),
            batch: Some(PresignBatch {
                first: batch.first,
                last: batch.last,
            }),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(links) => links,
            Err(f) => return Err(self.refused(f, interval)),
        };
        let Some(url) = links.batch_url else {
            tracing::warn!("upload: the cloud issued no batch link");
            return Err(self.failed(interval));
        };
        if let Err(f) = self.cloud.put_object(&url, batch.bytes).await {
            return Err(self.refused(f, interval));
        }
        Ok(())
    }

    /// Presigns, PUTs and records one batch. `None` when the cursor moved; otherwise what to do
    /// instead.
    async fn send(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        batch: Built,
        interval: Duration,
    ) -> Option<Next> {
        let (epoch, first, last, rows) = (batch.epoch.clone(), batch.first, batch.last, batch.rows);
        if let Err(next) = self.put(token, batch, interval).await {
            return Some(next);
        }
        match writer.record_upload(epoch.clone(), last).await {
            Ok(()) => {
                tracing::info!(
                    epoch = %epoch,
                    first,
                    last,
                    rows,
                    "change log batch uploaded"
                );
                self.fails = 0;
                None
            }
            Err(e) if e.code == ErrorCode::NotPrimary => {
                tracing::info!("upload: this hub is no longer the primary");
                Some(Next::idle(TIER_RECHECK))
            }
            Err(e) => {
                // The cloud has the batch; the next cycle sends it again under the same key.
                tracing::warn!(code = e.code.as_str(), "upload: could not move the cursor");
                Some(self.failed(interval))
            }
        }
    }

    /// How the cloud's refusal changes the schedule.
    fn refused(&mut self, f: CloudFailure, interval: Duration) -> Next {
        match f {
            CloudFailure::Refused { error, .. }
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!("upload: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                Next::backing_off(REVOKED_BACKOFF)
            }
            CloudFailure::Refused { error, .. } if error.code == ErrorCode::FeatureNotEntitled => {
                tracing::info!("upload: the cloud has this branch on Free");
                Next::backing_off(NOT_ENTITLED_RETRY)
            }
            CloudFailure::Refused {
                status,
                error,
                retry_after_s,
            } if status == 429 || error.code == ErrorCode::RateLimited => {
                let wait = retry_after_s
                    .map_or(RATE_LIMIT_WAIT, |s| Duration::from_secs(u64::from(s)))
                    .clamp(Duration::from_secs(1), RATE_LIMIT_CEILING);
                tracing::info!(
                    wait_s = wait.as_secs(),
                    "upload: the cloud limits the calls"
                );
                Next::backing_off(wait)
            }
            f => {
                tracing::warn!(failure = ?f, "upload failed");
                self.failed(interval)
            }
        }
    }

    fn failed(&mut self, interval: Duration) -> Next {
        self.fails += 1;
        Next::backing_off(backoff(self.fails, interval, unit()))
    }

    /// `upload_interval_s` from the stored, verified licence (the tier's default when it cannot
    /// be read), never under [`MIN_UPLOAD_INTERVAL`].
    pub async fn interval(&self, writer: &WriterHandle, tier: Tier) -> Duration {
        let from_licence = match writer.licence_state().await {
            Ok(state) => state
                .jwt
                .and_then(|jwt| verify_claims(&jwt, &self.keys).ok())
                .map(|c| c.upload_interval_s),
            Err(_) => None,
        };
        clamp_interval(from_licence.unwrap_or(tier.defaults().upload_interval_s))
    }

    async fn progress(&self) -> Option<UploadStatus> {
        let snapshots = Arc::clone(&self.snapshots);
        match tokio::task::spawn_blocking(move || snapshots.upload_progress()).await {
            Ok(Ok(p)) => Some(p),
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "upload: could not count the waiting rows"
                );
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "upload: count task failed");
                None
            }
        }
    }

    async fn refresh_progress(&self) {
        if let Some(p) = self.progress().await {
            self.status.send_if_modified(|s| {
                let changed = s.upload != p;
                s.upload = p;
                changed
            });
        }
    }
}

/// An interval in seconds as a wait, at least [`MIN_UPLOAD_INTERVAL`].
pub fn clamp_interval(secs: u32) -> Duration {
    Duration::from_secs(u64::from(secs)).max(MIN_UPLOAD_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{Clock, ManualClock, status_channel};
    use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
    use arena_store::{
        BatchFork, ChangeOp, ChangeRecord, Store, Table, decode_batch, insert_change,
        next_change_seq, write_lease_fork,
    };
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";
    /// The epoch the fixture's lease gives its writer.
    const EPOCH: &str = "1:01JAAAAAAAAAAAAAAAAAAAAAAA";

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

    /// (request line, body bytes).
    type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
    type Handler = Box<dyn FnMut(&str, &[u8]) -> (u16, String) + Send>;

    /// A one-request-per-connection HTTP server. `make` gets the port, to build links back to
    /// the server itself.
    async fn serve(make: impl FnOnce(u16) -> Handler) -> (u16, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut handler = make(port);
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                let (head_end, length) = loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..at]).into_owned();
                        let length = head
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
                let line = String::from_utf8_lossy(&raw)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned();
                let body = raw.get(head_end..).unwrap_or(&[]).to_vec();
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

    /// A cloud that issues a link per batch and takes every PUT.
    fn cloud(port: u16) -> Handler {
        Box::new(move |line, body| {
            if line.starts_with("POST /v1/presign") {
                let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                let b = &req["batch"];
                let url = format!(
                    "http://127.0.0.1:{port}/put/{}/{}-{}",
                    req["epoch"].as_str().unwrap(),
                    b["first"],
                    b["last"]
                );
                (200, serde_json::json!({ "batch_url": url }).to_string())
            } else {
                (200, "{}".into())
            }
        })
    }

    fn refused(status: u16, code: &str) -> (u16, String) {
        (status, format!(r#"{{"code":"{code}","message":"no"}}"#))
    }

    fn presigns(seen: &Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(l, _)| l.starts_with("POST /v1/presign"))
            .map(|(_, b)| serde_json::from_slice(b).unwrap())
            .collect()
    }

    /// (key, body) of every PUT.
    fn puts(seen: &Seen) -> Vec<(String, Vec<u8>)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|(l, b)| {
                l.strip_prefix("PUT ")
                    .map(|rest| (rest.split(' ').next().unwrap().to_owned(), b.clone()))
            })
            .collect()
    }

    struct Fx {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        status: watch::Sender<HubStatus>,
        task: UploadTask,
        writer: WriterHandle,
    }

    async fn fx(port: u16, tier: Tier) -> Fx {
        fx_with(port, tier, false).await
    }

    /// `bootstrap`: the branch comes from `branch.bootstrap` through `apply()`, so its change
    /// rows replay; otherwise it is inserted directly and the log starts empty.
    async fn fx_with(port: u16, tier: Tier, bootstrap: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(73, OWN));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
        let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        let claims = LicenceClaims::for_tier("T", "B", tier, T0 / 1000);
        writer
            .record_pairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H".into(),
                    tier,
                    paired_at: T0,
                },
                dev_sign(&claims).unwrap(),
                None,
            )
            .await
            .unwrap();
        if bootstrap {
            writer
                .submit_intent(intent(
                    arena_proto::IntentPayload::BranchBootstrap(arena_proto::BranchBootstrap {
                        name: "Arena".into(),
                        branch_code: "ARN".into(),
                        currency: "INR".into(),
                        tz: "Asia/Kolkata".into(),
                        day_start_min: 360,
                        fy_start_month: 4,
                        onboarding: None,
                    }),
                    arena_proto::ActorKind::System,
                    None,
                ))
                .await
                .unwrap();
        } else {
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
        }
        status.send_modify(|s| s.tier = Some(tier));
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = UploadTask::new(
            cloud,
            keystore,
            Some(writer.clone()),
            Arc::new(SnapshotReader::open(&path).unwrap()),
            PairGate::new(true),
            status.clone(),
            DEV_LICENCE_VERIFY_KEYS.to_vec(),
        );
        Fx {
            _dir: dir,
            path,
            status,
            task,
            writer,
        }
    }

    fn intent(
        payload: arena_proto::IntentPayload,
        kind: arena_proto::ActorKind,
        base_rev: Option<i64>,
    ) -> arena_proto::Intent {
        arena_proto::Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: payload.kind(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: T0,
            base_rev,
            actor: arena_proto::Actor {
                kind,
                id: "test".into(),
            },
            elevated_by: None,
            reason: None,
            payload,
        }
    }

    /// Appends `n` change rows to `epoch`, from the next free `seq`, directly.
    async fn add_rows(writer: &WriterHandle, epoch: &str, n: i64) {
        let epoch = epoch.to_owned();
        writer
            .submit(move |tx| {
                let first = next_change_seq(tx, &epoch)?;
                for seq in first..first + n {
                    insert_change(
                        tx,
                        &ChangeRecord {
                            epoch: epoch.clone(),
                            seq,
                            tbl: "settings".into(),
                            row_id: format!("k{seq}"),
                            op: ChangeOp::Insert,
                            row: vec![0x80],
                            intent_id: "I".into(),
                            intent_kind: "settings.set".into(),
                            actor_kind: "system".into(),
                            actor_id: "test".into(),
                            elevated_by_kind: None,
                            elevated_by_id: None,
                            elevated_by_mode: None,
                            elevated_by_verifier_version: None,
                            at: T0,
                            business_day: None,
                            fy: None,
                            reason: None,
                        },
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();
    }

    fn progress(f: &Fx) -> UploadStatus {
        f.status.borrow().upload
    }

    fn cursor(f: &Fx) -> Option<arena_store::UploadCursor> {
        Store::open_reader(&f.path)
            .unwrap()
            .read_with(arena_store::read_upload_cursor)
            .unwrap()
    }

    #[tokio::test]
    async fn a_paid_hub_uploads_its_rows_and_only_then_moves_the_cursor() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;

        let next = f.task.cycle().await;
        assert_eq!(
            next,
            Next {
                wait: Duration::from_secs(300),
                active: true,
                early: true
            }
        );
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(asked[0]["epoch"], EPOCH);
        assert_eq!(
            asked[0]["batch"],
            serde_json::json!({"first": 1, "last": 3})
        );
        let sent = puts(&seen);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, format!("/put/{EPOCH}/1-3"));
        let batch = decode_batch(&sent[0].1).unwrap();
        assert_eq!(batch.header.schema, 73);
        assert_eq!((batch.header.first, batch.header.last), (1, 3));
        assert_eq!(batch.rows.len(), 3);

        let c = cursor(&f).unwrap();
        assert_eq!(
            (c.epoch.as_str(), c.last_seq_uploaded, c.uploaded_at),
            (EPOCH, 3, T0)
        );
        assert_eq!(
            progress(&f),
            UploadStatus {
                last_upload_at: Some(T0),
                pending_rows: 0,
                last_seq_uploaded: 3,
                fork_seq: None,
            }
        );

        // Nothing new: no further request.
        f.task.cycle().await;
        assert_eq!(presigns(&seen).len(), 1);
        assert_eq!(puts(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_batch_read_back_matches_its_header_and_replays_into_an_empty_database() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx_with(port, Tier::Premium, true).await;
        f.writer
            .submit_intent(intent(
                arena_proto::IntentPayload::SettingsSet(arena_proto::SettingsSet {
                    key: "hub.locale".into(),
                    value: "en-NZ".into(),
                }),
                arena_proto::ActorKind::Owner,
                Some(1),
            ))
            .await
            .unwrap();
        f.task.cycle().await;

        let sent = puts(&seen);
        assert_eq!(sent.len(), 1);
        let batch = decode_batch(&sent[0].1).expect("sha256 and row count match the header");
        assert_eq!(
            batch.rows.len() as i64,
            batch.header.last - batch.header.first + 1
        );
        assert_eq!(batch.header.epoch, EPOCH);

        let source = Store::open_reader(&f.path).unwrap();
        assert_eq!(batch.rows, source.changes().unwrap());

        let dir = tempfile::tempdir().unwrap();
        let mut dest = Store::open(dir.path().join("replica.db"), T0).unwrap();
        let n = dest
            .write_tx_mut(|tx| arena_replay::replay(batch.rows.clone(), tx))
            .unwrap();
        assert_eq!(n, batch.rows.len());
        for table in Table::ALL {
            assert_eq!(
                dest.rows(table).unwrap(),
                source.rows(table).unwrap(),
                "{}",
                table.name()
            );
        }
        assert!(!dest.rows(Table::Branch).unwrap().is_empty());
        assert!(!dest.rows(Table::Settings).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_backlog_ships_back_to_back_in_contiguous_keys() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 12_000).await;

        f.task.cycle().await;
        let keys: Vec<String> = puts(&seen).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            [
                format!("/put/{EPOCH}/1-5000"),
                format!("/put/{EPOCH}/5001-10000"),
                format!("/put/{EPOCH}/10001-12000"),
            ]
        );
        let sizes: Vec<usize> = puts(&seen)
            .iter()
            .map(|(_, b)| decode_batch(b).unwrap().rows.len())
            .collect();
        assert_eq!(sizes, [5000, 5000, 2000]);
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 12_000);
        assert_eq!(progress(&f).pending_rows, 0);
    }

    #[tokio::test]
    async fn the_first_batch_of_a_forked_epoch_carries_the_fork_and_starts_after_the_parent() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        // The parent's rows 1..=5 are in the cloud already; this epoch continues at 6.
        f.writer.record_upload("0:OLD".into(), 5).await.unwrap();
        f.writer
            .submit(|tx| {
                write_lease_fork(
                    tx,
                    Some(&BatchFork {
                        parent_epoch: "0:OLD".into(),
                        parent_seq: 5,
                    }),
                )
            })
            .await
            .unwrap();
        f.writer
            .submit(|tx| {
                for seq in 6..=8 {
                    insert_change(
                        tx,
                        &ChangeRecord {
                            epoch: EPOCH.into(),
                            seq,
                            tbl: "settings".into(),
                            row_id: format!("k{seq}"),
                            op: ChangeOp::Insert,
                            row: vec![0x80],
                            intent_id: "I".into(),
                            intent_kind: "settings.set".into(),
                            actor_kind: "system".into(),
                            actor_id: "test".into(),
                            elevated_by_kind: None,
                            elevated_by_id: None,
                            elevated_by_mode: None,
                            elevated_by_verifier_version: None,
                            at: T0,
                            business_day: None,
                            fy: None,
                            reason: None,
                        },
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();

        f.task.cycle().await;
        let asked = presigns(&seen);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["epoch"], EPOCH);
        assert_eq!(
            asked[0]["batch"],
            serde_json::json!({"first": 6, "last": 8})
        );
        let sent = puts(&seen);
        assert_eq!(sent[0].0, format!("/put/{EPOCH}/6-8"));
        let batch = decode_batch(&sent[0].1).unwrap();
        assert_eq!(
            batch.header.fork,
            Some(BatchFork {
                parent_epoch: "0:OLD".into(),
                parent_seq: 5
            })
        );
        assert_eq!((batch.header.first, batch.header.last), (6, 8));

        // Later rows of the epoch carry no fork.
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;
        let batch = decode_batch(&puts(&seen)[1].1).unwrap();
        assert_eq!(batch.header.fork, None);
        assert_eq!((batch.header.first, batch.header.last), (9, 10));
    }

    #[tokio::test]
    async fn a_failed_put_leaves_the_cursor_and_the_next_cycle_sends_the_same_key() {
        let mut puts_seen = 0;
        let (port, seen) = serve(move |port| {
            let mut ok = cloud(port);
            Box::new(move |line, body| {
                if line.starts_with("PUT ") {
                    puts_seen += 1;
                    if puts_seen == 1 {
                        return refused(503, "INTERNAL");
                    }
                }
                ok(line, body)
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;

        let next = f.task.cycle().await;
        assert!(!next.early && next.active);
        assert!(
            next.wait >= Duration::from_secs(1) && next.wait <= Duration::from_secs(300),
            "{next:?}"
        );
        assert_eq!(cursor(&f), None, "the cursor stays");
        assert_eq!(
            progress(&f),
            UploadStatus {
                last_upload_at: None,
                pending_rows: 3,
                last_seq_uploaded: 0,
                fork_seq: None,
            }
        );

        let next = f.task.cycle().await;
        assert!(next.early);
        let sent = puts(&seen);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, sent[1].0, "the same key");
        assert_eq!(sent[0].1, sent[1].1, "the same bytes");
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 3);
        assert_eq!(progress(&f).pending_rows, 0);
    }

    #[tokio::test]
    async fn an_unreachable_cloud_is_a_failure_that_leaves_the_rows_waiting() {
        // Nothing listens on this port.
        let spare = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = spare.local_addr().unwrap().port();
        drop(spare);
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 2).await;
        let next = f.task.cycle().await;
        assert!(!next.early && next.active);
        assert_eq!(cursor(&f), None);
        assert_eq!(progress(&f).pending_rows, 2);
    }

    #[tokio::test]
    async fn a_free_hub_or_one_without_a_tier_makes_no_request() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Free).await;
        add_rows(&f.writer, EPOCH, 3).await;
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        f.status.send_modify(|s| s.tier = None);
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(cursor(&f), None);
        assert_eq!(progress(&f), UploadStatus::default());

        // An upgrade starts the upload.
        f.status.send_modify(|s| s.tier = Some(Tier::Basic));
        f.task.cycle().await;
        assert_eq!(puts(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_follower_makes_no_request() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        assert_eq!(f.task.cycle().await, Next::idle(TIER_RECHECK));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_clouds_refusals_are_told_apart() {
        for (status, code, wait, revoked) in [
            (401, "TOKEN_REVOKED", REVOKED_BACKOFF, true),
            (401, "TOKEN_INVALID", REVOKED_BACKOFF, true),
            (403, "FEATURE_NOT_ENTITLED", NOT_ENTITLED_RETRY, false),
        ] {
            let (port, seen) = serve(move |_| Box::new(move |_, _| refused(status, code))).await;
            let mut f = fx(port, Tier::Basic).await;
            add_rows(&f.writer, EPOCH, 3).await;
            assert_eq!(f.task.cycle().await.wait, wait, "{code}");
            assert_eq!(presigns(&seen).len(), 1, "{code}");
            assert!(puts(&seen).is_empty(), "{code}");
            assert_eq!(cursor(&f), None, "{code}");
            let now_revoked = f.status.borrow().pairing == PairingState::Revoked;
            assert_eq!(now_revoked, revoked, "{code}");
            if revoked {
                // A revoked Hub stops asking.
                assert_eq!(f.task.cycle().await, Next::idle(REVOKED_BACKOFF));
                assert_eq!(presigns(&seen).len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn a_rate_limited_presign_without_a_retry_after_waits_a_minute() {
        let (port, seen) = serve(|_| {
            Box::new(|_, _| {
                (
                    429,
                    r#"{"code":"RATE_LIMITED","message":"slow down"}"#.into(),
                )
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        let next = f.task.cycle().await;
        assert_eq!(next.wait, RATE_LIMIT_WAIT);
        assert!(!next.early);
        assert!(puts(&seen).is_empty());
        assert_eq!(cursor(&f), None);
        assert_ne!(f.status.borrow().pairing, PairingState::Revoked);
    }

    #[tokio::test]
    async fn a_429_with_a_retry_after_header_is_honoured() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let body = r#"{"code":"RATE_LIMITED","message":"slow down"}"#;
                let _ = sock
                    .write_all(
                        format!(
                            "HTTP/1.1 429 X\r\nretry-after: 120\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            }
        });
        let mut f = fx(port, Tier::Basic).await;
        add_rows(&f.writer, EPOCH, 3).await;
        assert_eq!(f.task.cycle().await.wait, Duration::from_secs(120));
    }

    #[tokio::test]
    async fn the_interval_comes_from_the_verified_licence_and_never_goes_under_30_seconds() {
        let (port, _seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        assert_eq!(
            f.task.interval(&f.writer, Tier::Basic).await,
            Duration::from_secs(300)
        );
        let mut claims = LicenceClaims::for_tier("T", "B", Tier::Basic, T0 / 1000);
        claims.upload_interval_s = 45;
        f.writer
            .record_heartbeat(Some(dev_sign(&claims).unwrap()), None, None)
            .await
            .unwrap();
        assert_eq!(
            f.task.interval(&f.writer, Tier::Basic).await,
            Duration::from_secs(45)
        );
        claims.upload_interval_s = 5;
        f.writer
            .record_heartbeat(Some(dev_sign(&claims).unwrap()), None, None)
            .await
            .unwrap();
        assert_eq!(
            f.task.interval(&f.writer, Tier::Premium).await,
            MIN_UPLOAD_INTERVAL
        );

        // A licence that cannot be verified falls back to the tier's default.
        let blind = UploadTask::new(
            f.task.cloud.clone(),
            Arc::clone(&f.task.keystore),
            Some(f.writer.clone()),
            Arc::clone(&f.task.snapshots),
            PairGate::new(true),
            f.status.clone(),
            Vec::new(),
        );
        assert_eq!(
            blind.interval(&f.writer, Tier::Premium).await,
            Duration::from_secs(300)
        );
        assert_eq!(clamp_interval(0), MIN_UPLOAD_INTERVAL);
        assert_eq!(clamp_interval(900), Duration::from_secs(900));
    }

    #[tokio::test]
    async fn the_cursor_never_moves_backwards() {
        let (port, _seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        f.writer.record_upload("2:B".into(), 10).await.unwrap();
        f.writer.record_upload("2:B".into(), 4).await.unwrap();
        f.writer.record_upload("1:Z".into(), 99).await.unwrap();
        let c = cursor(&f).unwrap();
        assert_eq!((c.epoch.as_str(), c.last_seq_uploaded), ("2:B", 10));
        f.writer.record_upload("10:A".into(), 1).await.unwrap();
        assert_eq!(cursor(&f).unwrap().epoch, "10:A");
        assert!(f.writer.record_upload("nonsense".into(), 1).await.is_err());
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
            })
            .await
            .unwrap();
    }

    fn change_rows(path: &std::path::Path) -> i64 {
        Store::open_reader(path).unwrap().changes().unwrap().len() as i64
    }

    #[tokio::test]
    async fn waiting_rows_are_counted_and_enough_of_them_wake_the_task_early() {
        let (port, seen) = serve(cloud).await;
        let f = fx(port, Tier::Basic).await;
        let status = f.status.clone();
        let writer = f.writer.clone();
        let path = f.path.clone();
        let job = tokio::spawn(f.task.with_trigger(8, Duration::from_millis(100)).run());
        let wait_for = async |n: usize| {
            for _ in 0..200 {
                if puts(&seen).len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("expected {n} uploads, saw {:?}", puts(&seen));
        };
        // The first look after start: nothing waiting, nothing sent.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(puts(&seen).is_empty());

        // Under the trigger the rows wait for the interval (300 s), and /healthz counts them.
        add_group(&writer, "One").await;
        let mut rows = change_rows(&path);
        assert!(rows > 0 && rows < 8, "{rows}");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(puts(&seen).is_empty());
        assert_eq!(status.borrow().upload.pending_rows, rows);

        // Enough rows: the batch goes without waiting out the interval.
        let mut i = 0;
        while rows < 8 {
            i += 1;
            add_group(&writer, &format!("More {i}")).await;
            rows = change_rows(&path);
        }
        wait_for(1).await;
        let batch = decode_batch(&puts(&seen)[0].1).unwrap();
        assert!(batch.rows.len() as i64 >= 8);
        let mut done = false;
        for _ in 0..100 {
            if status.borrow().upload.pending_rows == 0 {
                done = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(done, "{:?}", status.borrow().upload);
        assert_eq!(status.borrow().upload.last_upload_at, Some(T0));
        job.abort();
    }

    fn with_inbox(f: &mut Fx, batch_rows: usize) -> Arc<ResendInbox> {
        let inbox = ResendInbox::new();
        let task = std::mem::replace(
            &mut f.task,
            UploadTask::new(
                CloudClient::new(&CloudEndpoint::parse("http://127.0.0.1:9", true).unwrap())
                    .unwrap(),
                Arc::new(Mem(Mutex::new(None))),
                None,
                Arc::new(SnapshotReader::open(&f.path).unwrap()),
                PairGate::new(false),
                f.status.clone(),
                Vec::new(),
            ),
        );
        f.task = task
            .with_resend(Arc::clone(&inbox))
            .with_batch_rows(batch_rows);
        inbox
    }

    #[tokio::test]
    async fn a_resend_ships_exactly_the_asked_seqs_first_and_leaves_the_cursor_alone() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 3);
        add_rows(&f.writer, EPOCH, 12).await;
        f.task.cycle().await;
        let before = cursor(&f).unwrap();
        assert_eq!(before.last_seq_uploaded, 12);
        let sent_before = puts(&seen).len();

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 4, to: 8 });
        add_rows(&f.writer, EPOCH, 2).await;
        f.task.cycle().await;

        let keys: Vec<String> = puts(&seen)
            .iter()
            .skip(sent_before)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(
            keys,
            [
                format!("/put/{EPOCH}/4-6"),
                format!("/put/{EPOCH}/7-8"),
                format!("/put/{EPOCH}/13-14"),
            ],
            "the resend first, then the new rows"
        );
        let resent = decode_batch(&puts(&seen)[sent_before].1).unwrap();
        assert_eq!((resent.header.first, resent.header.last), (4, 6));
        assert_eq!(resent.rows.len(), 3);
        assert_eq!(inbox.pending(), None);
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 14);
        assert_eq!(cursor(&f).unwrap().uploaded_at, before.uploaded_at);
    }

    #[tokio::test]
    async fn pruned_rows_are_reported_not_resent() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        let sent_before = puts(&seen).len();

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 5, to: 9 });
        f.task.cycle().await;

        assert_eq!(puts(&seen).len(), sent_before, "nothing to send");
        assert_eq!(
            inbox.unavailable(),
            Some(arena_proto::ResendUnavailable {
                epoch: EPOCH.into(),
                from: 5,
                to: 9
            })
        );
        assert_eq!(inbox.pending(), None, "not asked for again by the Hub");
    }

    #[tokio::test]
    async fn rows_missing_in_the_middle_are_sent_where_held_and_reported_where_not() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        add_rows(&f.writer, EPOCH, 3).await;
        f.task.cycle().await;
        let sent_before = puts(&seen).len();

        // Rows 1..=3 are held; 4..=6 are not.
        inbox.offer(EPOCH, arena_proto::SeqRange { from: 2, to: 6 });
        f.task.cycle().await;

        let keys: Vec<String> = puts(&seen)
            .iter()
            .skip(sent_before)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(keys, [format!("/put/{EPOCH}/2-3")]);
        assert_eq!(inbox.unavailable().map(|u| (u.from, u.to)), Some((4, 6)));
        assert_eq!(inbox.pending(), None);
    }

    #[tokio::test]
    async fn a_resend_of_another_epoch_is_dropped() {
        let (port, seen) = serve(cloud).await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 5);
        inbox.offer("9:OTHER", arena_proto::SeqRange { from: 1, to: 4 });
        f.task.cycle().await;
        assert_eq!(inbox.pending(), None);
        assert!(presigns(&seen).is_empty());
        assert_eq!(inbox.unavailable(), None);
    }

    #[tokio::test]
    async fn a_failed_resend_put_keeps_the_run_for_the_next_cycle() {
        let mut puts_seen = 0;
        let (port, seen) = serve(move |port| {
            let mut ok = cloud(port);
            Box::new(move |line, body| {
                if line.starts_with("PUT ") {
                    puts_seen += 1;
                    if puts_seen == 3 {
                        return refused(503, "INTERNAL");
                    }
                }
                ok(line, body)
            })
        })
        .await;
        let mut f = fx(port, Tier::Basic).await;
        let inbox = with_inbox(&mut f, 3);
        add_rows(&f.writer, EPOCH, 6).await;
        f.task.cycle().await;
        assert_eq!(puts(&seen).len(), 2, "6 rows in two batches of 3");

        inbox.offer(EPOCH, arena_proto::SeqRange { from: 1, to: 6 });
        let next = f.task.cycle().await;
        assert!(!next.early && next.active, "{next:?}");
        assert_eq!(
            inbox.pending().map(|(_, r)| (r.from, r.to)),
            Some((1, 6)),
            "the first batch of the resend failed"
        );

        f.task.cycle().await;
        assert_eq!(inbox.pending(), None);
        let keys: Vec<String> = puts(&seen).iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys.last().unwrap(), &format!("/put/{EPOCH}/4-6"));
        assert_eq!(cursor(&f).unwrap().last_seq_uploaded, 6);
    }

    #[test]
    fn the_same_ask_again_keeps_the_progress_already_made() {
        let inbox = ResendInbox::new();
        inbox.offer("1:A", arena_proto::SeqRange { from: 1, to: 10 });
        inbox.advance(4);
        inbox.offer("1:A", arena_proto::SeqRange { from: 1, to: 10 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(5));
        inbox.offer("1:A", arena_proto::SeqRange { from: 20, to: 30 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(20));
        inbox.offer("1:A", arena_proto::SeqRange { from: 9, to: 3 });
        assert_eq!(inbox.pending().map(|(_, r)| r.from), Some(20), "invalid");
        inbox.advance(30);
        assert_eq!(inbox.pending(), None);
    }
}

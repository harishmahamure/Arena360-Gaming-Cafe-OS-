//! The Android Hub hands its branch to a Windows Hub (Story 10.17).
//!
//! [`Handover`] owns the state of one handover on either Hub. On the Android Hub a manager's PIN
//! starts it ([`Handover::begin`]) and a 6-digit code appears; the Windows Hub's claim
//! ([`Handover::claim`]) stops the Hub (every intent is refused `HUB_HANDING_OVER`), drains the
//! writer queue and writes the final `.a360` snapshot with `purpose: migrate`; the Windows Hub
//! pulls it ([`Handover::snapshot_for`]) and acknowledges ([`Handover::acknowledge`]), after
//! which the Hub is read-only and says so ([`HandoverPhase::Moved`]). Without a LAN path the
//! manager writes the snapshot to the backup folder ([`Handover::use_file`]); either way the Hub
//! stays stopped until the Windows Hub confirms or the manager cancels ([`Handover::cancel`]).
//!
//! The state is kept in `hub_meta` outside the change log, so a restart finds the Hub still
//! stopped. Nothing here changes the epoch or the change log: a cancelled move leaves the Hub as
//! it was.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, HANDOVER_CODE_TTL_MS, HANDOVER_FAILURES_PER_CODE, HANDOVER_PURPOSE,
    HANDOVER_STATE_KEY, HandedOver, HandoverAck, HandoverClaim, HandoverClaimed, HandoverPhase,
    HandoverRefused, HandoverSnapshotFacts, HandoverView, KEY_BACKUP_DIR,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, watch};

use crate::{Clock, HubStatus, WriterHandle};

/// What a proof is about: the claim the Windows Hub sends.
pub const PROOF_CLAIM: &str = "claim";
/// What a proof is about: the Android Hub's answer.
pub const PROOF_REPLY: &str = "reply";

/// The proof both Hubs send to show they know `code` over the TLS channel whose certificate has
/// `fingerprint`: lower-case hex of SHA-256 over the four parts, each length-prefixed. A
/// certificate in the middle shows each side a different fingerprint, so it cannot forge either
/// proof without the code, and the code itself never leaves the screen.
pub fn handover_proof(code: &str, fingerprint: &str, about: &str, extra: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"arena360-handover-v1");
    for part in [code, fingerprint, about, extra] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A snapshot the Hub made, kept until the handover ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Made {
    path: PathBuf,
    file_name: String,
    facts: HandoverSnapshotFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    phase: HandoverPhase,
    migration_id: String,
    target_hub_name: String,
    code: String,
    code_expires_at: i64,
    failures: u32,
    token_hash: Option<String>,
    made: Option<Made>,
    /// The file written for the manager to copy, in phase `file`.
    backup_file: Option<String>,
}

impl State {
    fn view(&self) -> HandoverView {
        HandoverView {
            phase: self.phase,
            target_hub_name: self.target_hub_name.clone(),
            code: (self.phase == HandoverPhase::Code).then(|| self.code.clone()),
            last_seq: self.made.as_ref().map(|m| m.facts.last_seq),
            file_name: self.backup_file.clone(),
        }
    }
}

fn refused(code: ErrorCode, message: &str, replaced: bool) -> HandoverRefused {
    HandoverRefused {
        code,
        message: message.into(),
        code_replaced: replaced,
    }
}

fn new_code() -> Result<String, AppError> {
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let bytes = crate::devices::random_bytes(4)?;
        let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if n < LIMIT {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

fn corrupt(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("the handover state: {e}"))
}

/// One Hub's handover.
pub struct Handover {
    writer: WriterHandle,
    reader: Arc<crate::SnapshotReader>,
    status: watch::Sender<HubStatus>,
    clock: Arc<dyn Clock>,
    state: Mutex<Option<State>>,
}

impl Handover {
    /// Opens the handover with what `hub_meta` stored (`None` for no handover) and publishes it
    /// in `status`, so a Hub that restarts mid-handover is still stopped.
    pub fn open(
        writer: WriterHandle,
        reader: Arc<crate::SnapshotReader>,
        status: watch::Sender<HubStatus>,
        clock: Arc<dyn Clock>,
        stored: Option<&str>,
    ) -> Result<Self, AppError> {
        let state = match stored.filter(|s| !s.is_empty()) {
            Some(json) => Some(serde_json::from_str::<State>(json).map_err(corrupt)?),
            None => None,
        };
        let this = Self {
            writer,
            reader,
            status,
            clock,
            state: Mutex::new(state),
        };
        let view = this
            .state
            .try_lock()
            .ok()
            .and_then(|s| s.as_ref().map(State::view));
        this.publish(view);
        Ok(this)
    }

    fn publish(&self, view: Option<HandoverView>) {
        self.status.send_if_modified(|s| {
            if s.handover == view {
                return false;
            }
            s.handover = view;
            true
        });
    }

    async fn store(&self, state: &Option<State>) -> Result<(), AppError> {
        let json = match state {
            Some(s) => serde_json::to_string(s).map_err(corrupt)?,
            None => String::new(),
        };
        self.writer.record_meta(HANDOVER_STATE_KEY, json).await?;
        self.publish(state.as_ref().map(State::view));
        Ok(())
    }

    /// The handover as the UI shows it.
    pub async fn view(&self) -> Option<HandoverView> {
        self.state.lock().await.as_ref().map(State::view)
    }

    /// What the heartbeat reports once the Windows Hub holds the snapshot.
    pub async fn handed_over(&self) -> Option<HandedOver> {
        let state = self.state.lock().await;
        let s = state.as_ref()?;
        (s.phase == HandoverPhase::Moved).then(|| HandedOver {
            migration_id: s.migration_id.clone(),
            last_seq: s.made.as_ref().map_or(0, |m| m.facts.last_seq),
        })
    }

    /// The manager confirmed the move named by the last heartbeat: a fresh 6-digit code. Asking
    /// again while a code is showing and good answers it unchanged; an expired code is replaced.
    /// `VALIDATION_FAILED` when no move was asked for, `HUB_HANDING_OVER` once the Hub stopped.
    pub async fn begin(&self) -> Result<HandoverView, AppError> {
        let notice = self.status.borrow().move_notice.clone().ok_or_else(|| {
            AppError::new(
                ErrorCode::ValidationFailed,
                "no move to a Windows Hub has been asked for",
            )
        })?;
        let mut guard = self.state.lock().await;
        if let Some(s) = guard.as_ref() {
            if s.phase != HandoverPhase::Code {
                return Err(AppError::new(
                    ErrorCode::HubHandingOver,
                    "this hub is already handing the branch over",
                ));
            }
            if s.migration_id == notice.id && s.code_expires_at > self.clock.now_ms() {
                return Ok(s.view());
            }
        }
        let next = Some(State {
            phase: HandoverPhase::Code,
            migration_id: notice.id,
            target_hub_name: notice.target_hub_name,
            code: new_code()?,
            code_expires_at: self.clock.now_ms() + HANDOVER_CODE_TTL_MS,
            failures: 0,
            token_hash: None,
            made: None,
            backup_file: None,
        });
        self.store(&next).await?;
        *guard = next;
        Ok(guard
            .as_ref()
            .map(State::view)
            .unwrap_or_else(|| unreachable!()))
    }

    /// The Windows Hub claims the code from `fingerprint`'s channel. On a good claim the Hub stops,
    /// the writer queue drains and the snapshot is written; the answer carries the transfer token.
    /// The sixth wrong attempt on a code invalidates it and starts a new one.
    pub async fn claim(
        &self,
        claim: &HandoverClaim,
        fingerprint: &str,
        scratch: &Path,
    ) -> Result<HandoverClaimed, HandoverRefused> {
        let mut guard = self.state.lock().await;
        let now = self.clock.now_ms();
        let internal = |e: AppError| refused(e.code, &e.message, false);
        let Some(state) = guard.as_mut() else {
            return Err(refused(
                ErrorCode::PairCodeInvalid,
                "no handover is waiting for a code",
                false,
            ));
        };
        if state.phase != HandoverPhase::Code {
            return Err(refused(
                ErrorCode::HubHandingOver,
                "this hub is already handing the branch over",
                false,
            ));
        }
        if state.failures >= HANDOVER_FAILURES_PER_CODE {
            let mut next = state.clone();
            next.code = new_code().map_err(internal)?;
            next.code_expires_at = now + HANDOVER_CODE_TTL_MS;
            next.failures = 0;
            let next = Some(next);
            self.store(&next).await.map_err(internal)?;
            *guard = next;
            return Err(refused(
                ErrorCode::PairCodeInvalid,
                "too many wrong codes; the tablet shows a new one",
                true,
            ));
        }
        let expected = handover_proof(&state.code, fingerprint, PROOF_CLAIM, &claim.hub_name);
        let good = state.code_expires_at > now
            && claim.hub_name == state.target_hub_name
            && same(&expected, &claim.proof);
        if !good {
            state.failures += 1;
            let expired = state.code_expires_at <= now;
            let snapshot = guard.clone();
            self.store(&snapshot).await.map_err(internal)?;
            return Err(refused(
                if expired {
                    ErrorCode::PairCodeExpired
                } else {
                    ErrorCode::PairCodeInvalid
                },
                "that handover code is not valid",
                false,
            ));
        }

        // Stop: from here no intent is applied. Then wait for the ones already queued, so the
        // snapshot holds the last change this Hub will ever write.
        let code = state.code.clone();
        let mut stopped = state.clone();
        stopped.phase = HandoverPhase::Transferring;
        let stopped = Some(stopped);
        self.store(&stopped).await.map_err(internal)?;
        *guard = stopped;
        let made = match self.make_snapshot(scratch, None).await {
            Ok(m) => m,
            Err(e) => {
                // Nothing was handed over: serve again with the same code.
                if let Some(s) = guard.as_mut() {
                    s.phase = HandoverPhase::Code;
                }
                let back = guard.clone();
                let _ = self.store(&back).await;
                return Err(internal(e));
            }
        };
        let token = hex(&crate::devices::random_bytes(32).map_err(internal)?);
        let facts = made.facts.clone();
        if let Some(s) = guard.as_mut() {
            s.token_hash = Some(hex(&Sha256::digest(token.as_bytes())));
            s.made = Some(made);
        }
        let kept = guard.clone();
        self.store(&kept).await.map_err(internal)?;
        Ok(HandoverClaimed {
            proof: handover_proof(&code, fingerprint, PROOF_REPLY, &token),
            transfer_token: token,
            fingerprint: fingerprint.to_owned(),
            snapshot: facts,
        })
    }

    /// Drains the writer queue and writes the final snapshot into `dir` (named for a manager's
    /// copy when `copy_name` is set, else a scratch name). The Hub must already refuse intents.
    async fn make_snapshot(&self, dir: &Path, copy_name: Option<String>) -> Result<Made, AppError> {
        self.writer.barrier().await?;
        let db = self.reader.db_path()?;
        let now = self.clock.now_ms();
        let name = copy_name.unwrap_or_else(|| format!("handover-{}.a360", ulid::Ulid::generate()));
        let target = dir.to_path_buf();
        let file_name = name.clone();
        let done = tokio::task::spawn_blocking(move || {
            arena_store::write_export_for(&db, &target, &file_name, now, Some(HANDOVER_PURPOSE))
        })
        .await
        .map_err(|_| AppError::new(ErrorCode::Internal, "the snapshot task failed"))??;
        let path = done.path.clone();
        let (sha256, bytes) = tokio::task::spawn_blocking(move || file_sha256(&path))
            .await
            .map_err(|_| AppError::new(ErrorCode::Internal, "the snapshot task failed"))??;
        Ok(Made {
            path: done.path,
            file_name: name,
            facts: HandoverSnapshotFacts {
                branch_id: done.header.branch_id,
                epoch: done.header.epoch,
                last_seq: done.header.last_seq,
                sha256,
                bytes,
            },
        })
    }

    /// The snapshot file for the Windows Hub's pull, when `token` is the one the claim answered.
    pub async fn snapshot_for(&self, token: &str) -> Option<(PathBuf, HandoverSnapshotFacts)> {
        let state = self.state.lock().await;
        let s = state.as_ref()?;
        let made = s.made.as_ref()?;
        (s.phase == HandoverPhase::Transferring && token_is(s, token))
            .then(|| (made.path.clone(), made.facts.clone()))
    }

    /// The Windows Hub holds the snapshot and its hash, branch and `seq` matched: this Hub moves
    /// to read-only "moved". `PAIR_CODE_INVALID` for a token that is not the transfer token,
    /// `VALIDATION_FAILED` for facts that are not the snapshot's.
    pub async fn acknowledge(&self, token: &str, ack: &HandoverAck) -> Result<(), HandoverRefused> {
        let mut guard = self.state.lock().await;
        let internal = |e: AppError| refused(e.code, &e.message, false);
        let Some(s) = guard.as_ref().filter(|s| {
            matches!(s.phase, HandoverPhase::Transferring | HandoverPhase::Moved)
                && token_is(s, token)
                && s.made.is_some()
        }) else {
            return Err(refused(
                ErrorCode::TokenInvalid,
                "the transfer token is not valid",
                false,
            ));
        };
        let made = s.made.clone().unwrap_or_else(|| unreachable!());
        if made.facts.sha256 != ack.sha256 || made.facts.last_seq != ack.last_seq {
            return Err(refused(
                ErrorCode::ValidationFailed,
                "the acknowledgement is not for this snapshot",
                false,
            ));
        }
        if s.phase == HandoverPhase::Moved {
            // The PC asks again because our answer was lost: the same facts, the same answer.
            return Ok(());
        }
        // Store first: a failed write leaves the Hub as it was, and the PC may ask again.
        let mut moved = s.clone();
        moved.phase = HandoverPhase::Moved;
        moved.code.clear();
        moved.made = Some(Made {
            path: PathBuf::new(),
            ..made.clone()
        });
        let next = Some(moved);
        self.store(&next).await.map_err(internal)?;
        *guard = next;
        let _ = std::fs::remove_file(&made.path);
        Ok(())
    }

    /// "Use a file instead": the Hub stops (if the Windows Hub has not claimed yet), drains, and
    /// writes the snapshot into the backup folder for copying to the PC. It stays stopped until
    /// the PC confirms the import or the manager cancels. Answers the file's name.
    pub async fn use_file(&self) -> Result<HandoverView, AppError> {
        let mut guard = self.state.lock().await;
        let Some(state) = guard.as_mut() else {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "no handover has been started",
            ));
        };
        if !matches!(
            state.phase,
            HandoverPhase::Code | HandoverPhase::Transferring | HandoverPhase::File
        ) {
            return Err(AppError::new(
                ErrorCode::HubHandingOver,
                "this branch has already moved",
            ));
        }
        if state.phase == HandoverPhase::File && state.backup_file.is_some() {
            return Ok(state.view());
        }
        let folder = self
            .reader
            .setting_value(KEY_BACKUP_DIR)?
            .filter(|f| !f.trim().is_empty())
            .ok_or_else(|| {
                AppError::with_field(ErrorCode::ValidationFailed, "folder", "not_set")
            })?;
        if arena_proto::is_backup_tree_uri(&folder) {
            return Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                "folder",
                "unsupported",
            ));
        }
        let branch = self
            .reader
            .branch()?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "the branch is not set up"))?;
        let now = self.clock.now_ms();
        let tz = self
            .reader
            .locale(now)?
            .map_or_else(|| "UTC".to_owned(), |l| l.tz);
        let name = arena_proto::export_file_name(&branch.branch_code, now, &tz)
            .replace(".a360", "-move.a360");
        let previous = state.phase;
        // The Hub stops before the copy is made, so the file holds its last change.
        let mut stopped = state.clone();
        stopped.phase = HandoverPhase::Transferring;
        let stopped = Some(stopped);
        self.store(&stopped).await?;
        *guard = stopped;
        let made = match self.make_snapshot(Path::new(&folder), Some(name)).await {
            Ok(m) => m,
            Err(e) => {
                if let Some(s) = guard.as_mut() {
                    s.phase = previous;
                }
                let back = guard.clone();
                let _ = self.store(&back).await;
                return Err(e);
            }
        };
        if let Some(s) = guard.as_mut() {
            // A pulled copy in the scratch folder is no longer served.
            if let Some(old) = s.made.take() {
                let _ = std::fs::remove_file(old.path);
            }
            s.phase = HandoverPhase::File;
            s.token_hash = None;
            s.backup_file = Some(made.file_name.clone());
            s.made = Some(made);
        }
        let next = guard.clone();
        self.store(&next).await?;
        Ok(guard
            .as_ref()
            .map(State::view)
            .unwrap_or_else(|| unreachable!()))
    }

    /// "Cancel move" (the caller checked the manager's PIN): the Hub serves again as main with
    /// the epoch and change log it had; the snapshot it made is deleted. A Hub that has already
    /// moved cannot cancel.
    pub async fn cancel(&self) -> Result<(), AppError> {
        let mut guard = self.state.lock().await;
        match guard.as_ref() {
            None => return Ok(()),
            Some(s) if s.phase == HandoverPhase::Moved => {
                return Err(AppError::new(
                    ErrorCode::HubHandingOver,
                    "this branch has already moved",
                ));
            }
            Some(s) if s.phase == HandoverPhase::Received => {
                return Err(AppError::new(
                    ErrorCode::ValidationFailed,
                    "this Hub holds the snapshot; there is no move to cancel here",
                ));
            }
            Some(_) => {}
        }
        self.store(&None).await?;
        if let Some(made) = guard.take().and_then(|s| s.made) {
            let _ = std::fs::remove_file(made.path);
        }
        Ok(())
    }

    /// The heartbeat answered with `notice`. A move the cloud no longer asks for (the owner
    /// cancelled it) ends a handover that has not stopped the Hub yet; one that has stopped it
    /// waits for the manager's "Cancel move".
    pub async fn on_notice(&self, notice: Option<&arena_proto::MigrationNotice>) {
        if notice.is_some() {
            return;
        }
        // One lock for the check and the end, so a claim cannot land in between.
        let mut guard = self.state.lock().await;
        let waiting = guard
            .as_ref()
            .is_some_and(|s| s.phase == HandoverPhase::Code);
        if waiting && self.store(&None).await.is_ok() {
            *guard = None;
        }
    }
}

fn token_is(s: &State, token: &str) -> bool {
    s.token_hash
        .as_deref()
        .is_some_and(|h| same(h, &hex(&Sha256::digest(token.as_bytes()))))
}

/// SHA-256 (lower-case hex) and size of the file at `path`. Blocking (disk).
pub fn file_sha256(path: &Path) -> Result<(String, u64), AppError> {
    use std::io::Read;
    let io =
        |e: std::io::Error| AppError::new(ErrorCode::Internal, format!("the snapshot file: {e}"));
    let mut file = std::fs::File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        bytes += n as u64;
    }
    Ok((hex(&hasher.finalize()), bytes))
}

/// The answer of a join: where the Windows Hub got to.
pub type JoinFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<HandoverView, arena_proto::UiError>> + Send + 'a>,
>;

/// The Windows Hub's handle on "Waiting for your Android Hub" (Story 10.17): the loopback route
/// calls it with the Android Hub's address and the code on its screen; `arena-tasks` implements it
/// (the claim, the pull and the checks run there).
pub trait HandoverJoinControl: Send + Sync {
    /// Claims the code at `req.address`, pulls the snapshot and checks it. Answers the `received`
    /// view, or why the move did not start.
    fn join(&self, req: arena_proto::HandoverJoinRequest) -> JoinFuture<'_>;
}

impl Handover {
    /// The Windows Hub holds a snapshot that checked out (hash, branch, `seq`): remembered across a
    /// restart for Story 10.18's import. `branch_name` is shown on the screen.
    pub async fn record_received(
        &self,
        path: PathBuf,
        file_name: String,
        facts: HandoverSnapshotFacts,
        branch_name: &str,
    ) -> Result<HandoverView, AppError> {
        let mut guard = self.state.lock().await;
        let next = Some(State {
            phase: HandoverPhase::Received,
            migration_id: String::new(),
            target_hub_name: branch_name.to_owned(),
            code: String::new(),
            code_expires_at: 0,
            failures: 0,
            token_hash: None,
            made: Some(Made {
                path,
                file_name,
                facts,
            }),
            backup_file: None,
        });
        self.store(&next).await?;
        *guard = next;
        Ok(guard
            .as_ref()
            .map(State::view)
            .unwrap_or_else(|| unreachable!()))
    }

    /// The snapshot a Windows Hub received: its file and facts (Story 10.18 imports it).
    pub async fn received(&self) -> Option<(PathBuf, HandoverSnapshotFacts)> {
        let state = self.state.lock().await;
        let s = state
            .as_ref()
            .filter(|s| s.phase == HandoverPhase::Received)?;
        s.made.as_ref().map(|m| (m.path.clone(), m.facts.clone()))
    }
}

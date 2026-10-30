//! Keeps the keys that protect players' data safe off the Hub (Story 9.2, AD-24).
//!
//! Two jobs, one task:
//!
//! * **Escrow, every tier.** Until the cloud has accepted this Hub's branch key (`backup_state.
//!   kek_escrowed_at`), the task wraps the key for the cloud escrow public key (an age X25519
//!   recipient, compiled in) and sends it to `POST /v1/kek/escrow`. It runs at pairing and, for a
//!   Hub paired before this existed or re-paired with a new key, on the next cycle. This is the
//!   one key-only exception to "Free uploads nothing".
//! * **Key backup, paid tiers.** When `player_keys` changed since the last successful backup, the
//!   task seals the whole table with the branch key and PUTs it to `keys/player-keys.bin` through a
//!   `kind: keys` presign. The object is overwritten each time, so an erased player's key is gone
//!   from the next one. A failed attempt leaves the table marked changed; the retry backs off and
//!   uploads only the table as it is then. The time of the last success and the last error are in
//!   `backup_state`.
//!
//! A follower, an unpaired Hub and a revoked one make no request. Free never uploads the table.
//! Nothing here logs a key, a player id or the wrapped bytes.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use arena_core::key_backup::seal_key_table;
use arena_core::players::{KEK_LEN, Kek};
use arena_core::{HubStatus, PairGate, PairingState, Role, SnapshotReader, WriterHandle};
use arena_proto::{
    ErrorCode, KekEscrowRequest, PRESIGN_KIND_KEYS, PresignRequest, Tier, VerifyKey,
    licence::verify_claims,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tokio::sync::{broadcast, watch};

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;
use crate::template::{NOT_ENTITLED_RETRY, TIER_RECHECK};
use crate::upload::{MIN_UPLOAD_INTERVAL, clamp_interval};

/// The longest Retry-After honoured.
const RATE_LIMIT_CEILING: Duration = Duration::from_secs(3600);
/// How long to wait when the cloud limits the calls and does not say how long.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);

/// Wraps the branch key for the escrow recipients (age, binary format). Fails for an entry that is
/// not an age X25519 public key; the error never echoes the entry.
pub fn wrap_kek(kek: &[u8; KEK_LEN], recipients: &[String]) -> Result<Vec<u8>, String> {
    let parsed: Vec<age::x25519::Recipient> = recipients
        .iter()
        .map(|r| {
            age::x25519::Recipient::from_str(r)
                .map_err(|_| "an escrow recipient is not an age public key".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let enc = age::Encryptor::with_recipients(parsed.iter().map(|r| r as &dyn age::Recipient))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(256);
    let mut w = enc.wrap_output(&mut out).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut w, kek).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// The key backup and escrow task.
pub struct KeyBackupTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    kek: Arc<dyn Kek>,
    writer: Option<WriterHandle>,
    snapshots: Arc<SnapshotReader>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    /// The licence verify keys, to read `upload_interval_s` from the stored licence.
    keys: Vec<VerifyKey<'static>>,
    /// The escrow public keys (`age1…`).
    recipients: Vec<String>,
    /// The least time between a player change and the backup it wakes.
    min_gap: Duration,
    fails: u32,
    warned_no_recipient: bool,
}

impl KeyBackupTask {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        kek: Arc<dyn Kek>,
        writer: Option<WriterHandle>,
        snapshots: Arc<SnapshotReader>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        keys: Vec<VerifyKey<'static>>,
        recipients: Vec<String>,
    ) -> Self {
        Self {
            cloud,
            keystore,
            kek,
            writer,
            snapshots,
            gate,
            status,
            keys,
            recipients,
            min_gap: MIN_UPLOAD_INTERVAL,
            fails: 0,
            warned_no_recipient: false,
        }
    }

    /// Tests: how soon a player change may wake the next cycle.
    pub fn with_min_gap(mut self, gap: Duration) -> Self {
        self.min_gap = gap;
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
            let wait = self.cycle().await;
            let ended = tokio::time::Instant::now();
            let mut deadline = ended + wait;
            loop {
                match tokio::time::timeout_at(deadline, events.recv()).await {
                    Err(_) => break,
                    Ok(Ok(applied)) => {
                        // A player was added, changed or erased: the key table may have moved.
                        // After a failure the backoff stands.
                        if self.fails == 0 && applied.changes.iter().any(|c| c.tbl == "players") {
                            deadline = deadline.min(ended + self.min_gap);
                        }
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                        if self.fails == 0 {
                            deadline = deadline.min(ended + self.min_gap);
                        }
                    }
                    Ok(Err(broadcast::error::RecvError::Closed)) => return,
                }
            }
        }
    }

    /// One wake: escrows the branch key if the cloud lacks it, then (paid) backs up the key table
    /// if it changed. Answers how long to sleep.
    pub async fn cycle(&mut self) -> Duration {
        let (role, lease, pairing, tier, restoring) = {
            let s = self.status.borrow();
            (
                s.role,
                s.lease.clone(),
                s.pairing,
                s.tier,
                s.restore_pending.is_some(),
            )
        };
        // A Hub waiting to be restored has no branch key of its own to escrow or back up.
        if restoring {
            return TIER_RECHECK;
        }
        let (Some(writer), Some(lease), Role::Primary) = (self.writer.clone(), lease, role) else {
            return TIER_RECHECK;
        };
        if pairing == PairingState::Revoked {
            return REVOKED_BACKOFF;
        }
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "keys: no hub token");
                return TIER_RECHECK;
            }
        };
        let snapshots = Arc::clone(&self.snapshots);
        let plan = match tokio::task::spawn_blocking(move || snapshots.key_backup_plan()).await {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "keys: could not read the backup state"
                );
                return self.failed(TIER_RECHECK);
            }
            Err(e) => {
                tracing::warn!(error = %e, "keys: read task failed");
                return self.failed(TIER_RECHECK);
            }
        };
        let tier = tier.filter(|t| *t != Tier::Free);
        let interval = match tier {
            Some(tier) => self.interval(&writer, tier).await,
            None => TIER_RECHECK,
        };

        // The escrow does not wait for the table, and a failed escrow does not hold the backup.
        let mut retry = None;
        if plan.state.kek_escrowed_at.is_none()
            && let Err(wait) = self.escrow(&writer, &token, interval).await
        {
            retry = Some(wait);
        }
        if tier.is_some()
            && let Some(rows) = plan.rows
        {
            let version = plan.state.keys_version;
            if let Err(wait) = self
                .back_up(
                    &writer,
                    &token,
                    &lease.epoch().to_string(),
                    rows,
                    version,
                    interval,
                )
                .await
            {
                retry = Some(retry.map_or(wait, |r: Duration| r.max(wait)));
            }
        }
        match retry {
            Some(wait) => wait,
            None => {
                self.fails = 0;
                interval
            }
        }
    }

    /// Wraps the branch key and leaves it with the cloud. `Err` is how long to wait before trying
    /// again.
    async fn escrow(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        interval: Duration,
    ) -> Result<(), Duration> {
        if self.recipients.is_empty() {
            if !self.warned_no_recipient {
                self.warned_no_recipient = true;
                tracing::warn!(
                    "keys: this build has no escrow key, the branch key is not escrowed"
                );
            }
            return Err(NOT_ENTITLED_RETRY);
        }
        let kek = Arc::clone(&self.kek);
        let recipients = self.recipients.clone();
        let wrapped = tokio::task::spawn_blocking(move || {
            let key = kek.kek().map_err(|e| e.code.as_str().to_owned())?;
            wrap_kek(&key, &recipients)
        })
        .await;
        let wrapped = match wrapped {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(reason)) => {
                tracing::warn!(reason, "keys: could not wrap the branch key");
                return Err(self.failed(interval));
            }
            Err(e) => {
                tracing::warn!(error = %e, "keys: wrap task failed");
                return Err(self.failed(interval));
            }
        };
        let req = KekEscrowRequest {
            wrapped_kek: STANDARD.encode(wrapped),
        };
        if let Err(f) = self.cloud.kek_escrow(token, &req).await {
            return Err(self.refused(f, interval, "escrow"));
        }
        match writer.record_kek_escrow().await {
            Ok(()) => tracing::info!("branch key escrowed"),
            // The cloud has it; the next cycle sends it again.
            Err(e) => tracing::warn!(code = e.code.as_str(), "keys: could not record the escrow"),
        }
        Ok(())
    }

    /// Seals the table and PUTs it. `Err` is how long to wait before trying again.
    async fn back_up(
        &mut self,
        writer: &WriterHandle,
        token: &str,
        epoch: &str,
        rows: Vec<arena_store::PlayerKeyRow>,
        version: i64,
        interval: Duration,
    ) -> Result<(), Duration> {
        let kek = Arc::clone(&self.kek);
        let sealed = tokio::task::spawn_blocking(move || {
            let key = kek.kek()?;
            seal_key_table(&key, &rows)
        })
        .await;
        let bytes = match sealed {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "keys: could not seal the key table");
                self.note_error(writer, e.code.as_str()).await;
                return Err(self.failed(interval));
            }
            Err(e) => {
                tracing::warn!(error = %e, "keys: seal task failed");
                return Err(self.failed(interval));
            }
        };
        let req = PresignRequest {
            epoch: epoch.to_owned(),
            kind: Some(PRESIGN_KIND_KEYS.to_owned()),
            ..PresignRequest::default()
        };
        let links = match self.cloud.presign(token, &req).await {
            Ok(l) => l,
            Err(f) => return Err(self.back_up_refused(writer, f, interval).await),
        };
        let Some(url) = links.url else {
            tracing::warn!("keys: the cloud issued no key backup link");
            self.note_error(writer, ErrorCode::Internal.as_str()).await;
            return Err(self.failed(interval));
        };
        let size = bytes.len();
        if let Err(f) = self.cloud.put_object(&url, bytes).await {
            return Err(self.back_up_refused(writer, f, interval).await);
        }
        self.fails = 0;
        match writer.record_key_backup(version).await {
            Ok(()) => tracing::info!(bytes = size, "player key backup stored"),
            // The cloud has it; the next cycle sends the table again under the same key.
            Err(e) => tracing::warn!(code = e.code.as_str(), "keys: could not record the backup"),
        }
        Ok(())
    }

    async fn back_up_refused(
        &mut self,
        writer: &WriterHandle,
        f: CloudFailure,
        interval: Duration,
    ) -> Duration {
        let code = match &f {
            CloudFailure::Refused { error, .. } => error.code,
            CloudFailure::Unreachable(_) => ErrorCode::CloudUnreachable,
        };
        self.note_error(writer, code.as_str()).await;
        self.refused(f, interval, "key backup")
    }

    async fn note_error(&self, writer: &WriterHandle, code: &str) {
        if let Err(e) = writer.record_key_backup_error(code.to_owned()).await {
            tracing::warn!(code = e.code.as_str(), "keys: could not record the error");
        }
    }

    /// How the cloud's refusal changes the schedule.
    fn refused(&mut self, f: CloudFailure, interval: Duration, what: &str) -> Duration {
        match f {
            CloudFailure::Refused { error, .. }
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!(what, "keys: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                REVOKED_BACKOFF
            }
            CloudFailure::Refused { error, .. } if error.code == ErrorCode::FeatureNotEntitled => {
                tracing::info!(what, "keys: the cloud has this branch on Free");
                NOT_ENTITLED_RETRY
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
                    what,
                    wait_s = wait.as_secs(),
                    "keys: the cloud limits the calls"
                );
                wait
            }
            f => {
                tracing::warn!(what, failure = ?f, "keys: the cloud refused or was unreachable");
                self.failed(interval)
            }
        }
    }

    fn failed(&mut self, interval: Duration) -> Duration {
        self.fails += 1;
        backoff(self.fails, interval, unit())
    }

    /// `upload_interval_s` from the stored, verified licence (the tier's default when it cannot be
    /// read), never under [`MIN_UPLOAD_INTERVAL`].
    async fn interval(&self, writer: &WriterHandle, tier: Tier) -> Duration {
        let from_licence = match writer.licence_state().await {
            Ok(state) => state
                .jwt
                .and_then(|jwt| verify_claims(&jwt, &self.keys).ok())
                .map(|c| c.upload_interval_s),
            Err(_) => None,
        };
        clamp_interval(from_licence.unwrap_or(tier.defaults().upload_interval_s))
    }
}

#[cfg(test)]
mod tests;

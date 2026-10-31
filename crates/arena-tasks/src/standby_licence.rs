//! The standby keeps its own licence (Story 9.20).
//!
//! A standby Hub asks the cloud for the branch's signed licence with its own token (`POST
//! /v1/standby/licence`), checks it against the branch it belongs to and keeps it in its own
//! `licence_cache`, which is Hub-local and never replicated. It writes nothing else: no fence, no
//! epoch, no settings; the branch's settings come through replication from the main Hub.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arena_core::{Clock, HubStatus, PairingState};
use arena_proto::{Entitlement, ErrorCode, LicenceStanding, StandbyLicenceRequest, VerifyKey};
use arena_store::Store;
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::heartbeat::{REVOKED_BACKOFF, backoff, unit};
use crate::keystore::Keystore;

/// The wait after a good refresh: about the main Hub's live heartbeat.
pub const LICENCE_INTERVAL: Duration = Duration::from_secs(60);
/// How often a Hub that is not a standby (yet) looks again.
pub const ROLE_RECHECK: Duration = Duration::from_secs(10);
const RETRY_CEILING: Duration = Duration::from_secs(60);

pub struct StandbyLicenceTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    store: Arc<Mutex<Store>>,
    status: watch::Sender<HubStatus>,
    keys: Vec<VerifyKey<'static>>,
    clock: Arc<dyn Clock>,
    version: String,
    /// The cloud's clock at the last good answer, and when it came.
    cloud_time: Option<(i64, Instant)>,
    fails: u32,
}

impl StandbyLicenceTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        store: Arc<Mutex<Store>>,
        status: watch::Sender<HubStatus>,
        keys: Vec<VerifyKey<'static>>,
        clock: Arc<dyn Clock>,
        version: String,
    ) -> Self {
        Self {
            cloud,
            keystore,
            store,
            status,
            keys,
            clock,
            version,
            cloud_time: None,
            fails: 0,
        }
    }

    pub async fn run(mut self) {
        loop {
            let wait = self.beat().await;
            tokio::time::sleep(wait).await;
        }
    }

    fn trusted_now(&self) -> i64 {
        self.cloud_time.map_or_else(
            || self.clock.now_ms(),
            |(ms, at)| ms.saturating_add(at.elapsed().as_millis() as i64),
        )
    }

    /// Reads the cached licence and shows its standing and tier in the Hub status.
    async fn settle(&self) {
        let store = Arc::clone(&self.store);
        let keys = self.keys.clone();
        let trusted = self.trusted_now();
        let read = tokio::task::spawn_blocking(move || {
            let guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(id) = guard.hub_identity()? else {
                return Ok(None);
            };
            let jwt = guard.cached_licence()?.map(|c| c.jwt);
            Ok::<_, arena_proto::AppError>(Some(Entitlement::evaluate(
                jwt.as_deref(),
                &keys,
                &id.tenant_id,
                &id.branch_id,
                trusted,
            )))
        })
        .await;
        if let Ok(Ok(Some(e))) = read {
            self.status.send_if_modified(|s| {
                if s.licence == Some(e.standing) && s.tier == Some(e.tier) {
                    return false;
                }
                s.licence = Some(e.standing);
                s.tier = Some(e.tier);
                true
            });
        }
    }

    /// One refresh. Answers how long to wait before the next.
    pub async fn beat(&mut self) -> Duration {
        let (standby, pairing, schema_version) = {
            let s = self.status.borrow();
            (s.following.is_some(), s.pairing, s.schema_version)
        };
        if !standby {
            return ROLE_RECHECK;
        }
        if pairing == PairingState::Revoked {
            return REVOKED_BACKOFF;
        }
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            _ => return ROLE_RECHECK,
        };
        let request = StandbyLicenceRequest {
            version: self.version.clone(),
            schema_version,
        };
        match self.cloud.standby_licence(&token, &request).await {
            Ok(res) => {
                self.cloud_time = Some((res.cloud_time, Instant::now()));
                if self.keep(&res.licence).await {
                    self.status
                        .send_modify(|s| s.pairing = PairingState::Active);
                }
                self.fails = 0;
                self.settle().await;
                LICENCE_INTERVAL
            }
            Err(CloudFailure::Refused { error, .. })
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!("standby licence: this hub is no longer paired");
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                REVOKED_BACKOFF
            }
            Err(f) => {
                tracing::warn!(failure = ?f, "standby licence: the refresh failed");
                self.fails += 1;
                self.settle().await;
                backoff(self.fails, RETRY_CEILING, unit())
            }
        }
    }

    /// Stores `jwt` in the Hub's own cache when it is this branch's genuine licence. `false`
    /// (and logged) when it is not trusted: the cache keeps what it had.
    async fn keep(&self, jwt: &str) -> bool {
        let store = Arc::clone(&self.store);
        let keys = self.keys.clone();
        let jwt = jwt.to_owned();
        let cloud_ms = self.trusted_now();
        let now = self.clock.now_ms();
        let stored = tokio::task::spawn_blocking(move || {
            let mut guard = store
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(id) = guard.hub_identity()? else {
                return Ok(false);
            };
            let genuine =
                Entitlement::evaluate(Some(&jwt), &keys, &id.tenant_id, &id.branch_id, cloud_ms)
                    .standing
                    != LicenceStanding::Unverified;
            if genuine {
                guard.write_tx(|tx| arena_store::store_licence(tx, &jwt, now))?;
            }
            Ok::<_, arena_proto::AppError>(genuine)
        })
        .await;
        match stored {
            Ok(Ok(true)) => true,
            Ok(Ok(false)) => {
                tracing::warn!("standby licence: licence not trusted; ignored");
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use arena_core::{ManualClock, status_channel};
    use arena_proto::Tier;
    use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
    use arena_store::HubIdentity;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;

    struct Mem;

    impl Keystore for Mem {
        fn put(&self, _: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn get(&self) -> std::io::Result<Option<Vec<u8>>> {
            Ok(Some(b"hub_x.y".to_vec()))
        }
    }

    fn standby_report() -> arena_proto::StandbyReport {
        arena_proto::StandbyReport {
            applied_epoch: None,
            applied_seq: None,
            lag_rows: 0,
            lag_seconds: 0,
            last_row_at: None,
            schema_version: 90,
            behind: None,
        }
    }

    fn licence_reply(tenant: &str, branch: &str) -> (u16, String) {
        let claims = LicenceClaims::for_tier(tenant, branch, Tier::Premium, T0 / 1000);
        let body = serde_json::json!({
            "licence": dev_sign(&claims).unwrap(),
            "cloud_time": T0 + 7,
        });
        (200, body.to_string())
    }

    async fn serve(reply: (u16, String), seen: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let (status, body) = &reply;
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(format!("{head}{body}").as_bytes()).await;
            }
        });
        port
    }

    struct Fx {
        _dir: tempfile::TempDir,
        store: Arc<Mutex<Store>>,
        status: watch::Sender<HubStatus>,
        task: StandbyLicenceTask,
        seen: Arc<Mutex<Vec<String>>>,
    }

    async fn fx(reply: (u16, String), standby: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("hub.db"), T0).unwrap();
        store
            .write_tx(|tx| {
                arena_store::insert_hub_identity(
                    tx,
                    &HubIdentity {
                        tenant_id: "T".into(),
                        branch_id: "B".into(),
                        hub_id: "H2".into(),
                        tier: Tier::Free,
                        paired_at: T0,
                    },
                )
            })
            .unwrap();
        let store = Arc::new(Mutex::new(store));
        let (status, _rx) = status_channel(HubStatus::opened(90, "01JSELF"));
        if standby {
            status.send_modify(|s| s.following = Some(standby_report()));
        }
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let port = serve(reply, Arc::clone(&seen)).await;
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let task = StandbyLicenceTask::new(
            cloud,
            Arc::new(Mem),
            Arc::clone(&store),
            status.clone(),
            DEV_LICENCE_VERIFY_KEYS.to_vec(),
            Arc::new(ManualClock::new(T0)),
            "0.2.0".into(),
        );
        Fx {
            _dir: dir,
            store,
            status,
            task,
            seen,
        }
    }

    #[tokio::test]
    async fn a_standby_keeps_the_branchs_licence_in_its_own_cache() {
        let mut f = fx(licence_reply("T", "B"), true).await;
        assert_eq!(f.task.beat().await, LICENCE_INTERVAL);
        let cached = f.store.lock().unwrap().cached_licence().unwrap();
        assert!(cached.is_some());
        let s = f.status.borrow();
        assert_eq!(s.licence, Some(LicenceStanding::Verified));
        assert_eq!(s.tier, Some(Tier::Premium));
        let sent = f.seen.lock().unwrap().clone();
        assert!(sent[0].starts_with("POST /v1/standby/licence"), "{sent:?}");
        assert!(sent[0].to_lowercase().contains("bearer hub_x.y"));
    }

    #[tokio::test]
    async fn another_branchs_licence_is_not_kept() {
        let mut f = fx(licence_reply("T", "OTHER"), true).await;
        f.task.beat().await;
        assert!(f.store.lock().unwrap().cached_licence().unwrap().is_none());
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
    }

    #[tokio::test]
    async fn only_a_standby_asks() {
        let mut f = fx(licence_reply("T", "B"), false).await;
        assert_eq!(f.task.beat().await, ROLE_RECHECK);
        assert!(f.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_revoked_token_marks_the_pairing_revoked() {
        let refused = serde_json::json!({"code": "TOKEN_REVOKED", "message": "no"}).to_string();
        let mut f = fx((403, refused), true).await;
        assert_eq!(f.task.beat().await, REVOKED_BACKOFF);
        assert_eq!(f.status.borrow().pairing, PairingState::Revoked);
        assert_eq!(f.task.beat().await, REVOKED_BACKOFF);
        assert_eq!(f.seen.lock().unwrap().len(), 1, "it stops asking");
    }

    #[tokio::test]
    async fn a_dead_cloud_keeps_the_cached_licence_standing() {
        let mut f = fx(licence_reply("T", "B"), true).await;
        f.task.beat().await;
        let err = serde_json::json!({"code": "INTERNAL", "message": "no"}).to_string();
        let port = serve((500, err), Arc::default()).await;
        f.task.cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let wait = f.task.beat().await;
        assert!(wait <= RETRY_CEILING);
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
    }
}

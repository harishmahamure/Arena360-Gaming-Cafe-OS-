use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_core::{CloudTime, HubStatus, PairGate, PairingState, Role, WriterHandle};
use arena_proto::{
    ErrorCode, HEARTBEAT_BOOT_INTERVAL_S, HEARTBEAT_LIVE_INTERVAL_S, HeartbeatRequest,
    HeartbeatResponse, VerifyKey, licence,
};
use tokio::sync::watch;

use crate::cloud::{CloudClient, CloudFailure};
use crate::keystore::Keystore;

pub const REVOKED_BACKOFF: Duration = Duration::from_secs(300);
const BACKOFF_FLOOR: Duration = Duration::from_secs(2);

pub fn backoff(fails: u32, interval: Duration, unit: f64) -> Duration {
    let ceil = BACKOFF_FLOOR
        .saturating_mul(1 << fails.min(16))
        .min(interval);
    ceil.mul_f64(0.5 + 0.5 * unit.clamp(0.0, 1.0))
}

fn unit() -> f64 {
    use std::hash::BuildHasher;
    (std::collections::hash_map::RandomState::new().hash_one(0u8) % 1000) as f64 / 1000.0
}

pub struct HeartbeatTask {
    cloud: CloudClient,
    keystore: Arc<dyn Keystore>,
    writer: Option<WriterHandle>,
    gate: PairGate,
    status: watch::Sender<HubStatus>,
    keys: Vec<VerifyKey<'static>>,
    version: String,
    fails: u32,
}

impl HeartbeatTask {
    pub fn new(
        cloud: CloudClient,
        keystore: Arc<dyn Keystore>,
        writer: Option<WriterHandle>,
        gate: PairGate,
        status: watch::Sender<HubStatus>,
        keys: Vec<VerifyKey<'static>>,
        version: String,
    ) -> Self {
        Self {
            cloud,
            keystore,
            writer,
            gate,
            status,
            keys,
            version,
            fails: 0,
        }
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        loop {
            let wait = self.beat().await;
            tokio::time::sleep(wait).await;
        }
    }

    pub async fn beat(&mut self) -> Duration {
        let (live, schema_version, role, lease) = {
            let s = self.status.borrow();
            (s.live, s.schema_version, s.role, s.lease.clone())
        };
        let interval = Duration::from_secs(if live {
            HEARTBEAT_LIVE_INTERVAL_S
        } else {
            HEARTBEAT_BOOT_INTERVAL_S
        });
        let (Some(writer), Some(lease), Role::Primary) = (self.writer.clone(), lease, role) else {
            return interval;
        };
        let keystore = Arc::clone(&self.keystore);
        let token = match tokio::task::spawn_blocking(move || keystore.get()).await {
            Ok(Ok(Some(t))) => String::from_utf8_lossy(&t).into_owned(),
            other => {
                tracing::warn!(ok = other.is_ok(), "heartbeat: no hub token");
                return interval;
            }
        };
        let req = HeartbeatRequest {
            epoch: lease.epoch().to_string(),
            version: self.version.clone(),
            schema_version,
            last_seq_uploaded: 0,
        };
        let sent = Instant::now();
        match self.cloud.heartbeat(&token, &req).await {
            Ok(res) => {
                self.fails = 0;
                let at = sent + sent.elapsed() / 2;
                self.accept(&writer, &lease, res, at).await;
                interval
            }
            Err(CloudFailure::Refused { error, .. })
                if matches!(
                    error.code,
                    ErrorCode::TokenInvalid | ErrorCode::TokenRevoked
                ) =>
            {
                tracing::warn!(
                    code = error.code.as_str(),
                    "heartbeat: this hub is no longer paired"
                );
                self.status
                    .send_modify(|s| s.pairing = PairingState::Revoked);
                REVOKED_BACKOFF
            }
            Err(e) => {
                self.fails += 1;
                tracing::warn!(fails = self.fails, failure = ?e, "heartbeat failed");
                backoff(self.fails, interval, unit())
            }
        }
    }

    async fn accept(
        &self,
        writer: &WriterHandle,
        lease: &arena_core::LeaseInfo,
        res: HeartbeatResponse,
        at: Instant,
    ) {
        let claims = match licence::verify(&res.licence, &self.keys, res.cloud_time) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!(code = e.code(), "heartbeat: licence not trusted; ignored");
                None
            }
        };
        let confirm = res.fence.confirmed;
        let sent = confirm.then(|| (lease.counter, lease.hub_instance_id.clone()));
        let stored = claims.is_some().then(|| res.licence.clone());
        if let Err(e) = writer.record_heartbeat(stored, sent).await {
            tracing::warn!(
                code = e.code.as_str(),
                "heartbeat: could not record the answer"
            );
            return;
        }
        if let Some(claims) = &claims {
            self.status.send_modify(|s| s.tier = Some(claims.tier));
        }
        self.status.send_modify(|s| {
            s.pairing = PairingState::Active;
            s.cloud_time = Some(CloudTime {
                cloud_ms: res.cloud_time,
                at,
            });
            if let (true, Some(l)) = (confirm, s.lease.as_mut())
                && l.counter == lease.counter
                && l.hub_instance_id == lease.hub_instance_id
            {
                l.provisional = false;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{Clock, ManualClock, status_channel};
    use arena_proto::Tier;
    use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
    use arena_store::Store;
    use std::collections::VecDeque;
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

    fn answer(confirmed: bool, tier: Tier) -> (u16, String) {
        let claims = LicenceClaims::for_tier("T", "B", tier, T0 / 1000);
        let body = serde_json::json!({
            "licence": dev_sign(&claims).unwrap(),
            "fence": {"epoch": format!("1:{OWN}"), "confirmed": confirmed},
            "cloud_time": T0 + 7,
        });
        (200, body.to_string())
    }

    fn refused(status: u16, code: &str) -> (u16, String) {
        (
            status,
            serde_json::json!({"code": code, "message": "no"}).to_string(),
        )
    }

    async fn serve(replies: Vec<(u16, String)>, hang: bool) -> u16 {
        serve_with(replies, hang, Arc::default()).await
    }

    async fn serve_with(
        replies: Vec<(u16, String)>,
        hang: bool,
        seen: Arc<Mutex<Vec<String>>>,
    ) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut replies: VecDeque<_> = replies.into();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                if hang {
                    std::future::pending::<()>().await;
                }
                let (status, body) = if replies.len() > 1 {
                    replies.pop_front().unwrap()
                } else {
                    replies.front().cloned().unwrap()
                };
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
        path: std::path::PathBuf,
        status: watch::Sender<HubStatus>,
        task: HeartbeatTask,
        writer: WriterHandle,
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
        let cloud = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let keystore: Arc<dyn Keystore> = Arc::new(Mem(Mutex::new(Some(b"hub_x.y".to_vec()))));
        let task = HeartbeatTask::new(
            cloud,
            keystore,
            Some(writer.clone()),
            PairGate::new(true),
            status.clone(),
            DEV_LICENCE_VERIFY_KEYS.to_vec(),
            "0.2.0".into(),
        );
        Fx {
            _dir: dir,
            path,
            status,
            task,
            writer,
        }
    }

    fn row(path: &std::path::Path) -> arena_store::LeaseRow {
        Store::open(path, T0).unwrap().lease().unwrap().unwrap()
    }

    #[test]
    fn backoff_doubles_from_two_seconds_and_stops_at_the_interval() {
        let iv = Duration::from_secs(10);
        for (fails, ceil) in [(1, 4), (2, 8), (3, 10), (9, 10)] {
            let lo = backoff(fails, iv, 0.0);
            let hi = backoff(fails, iv, 1.0);
            assert_eq!(hi, Duration::from_secs(ceil), "{fails}");
            assert_eq!(lo, hi / 2, "{fails}");
        }
        assert_eq!(backoff(0, iv, 1.0), Duration::from_secs(2));
    }

    #[tokio::test]
    async fn the_interval_is_ten_seconds_until_the_branch_is_live_then_sixty() {
        let port = serve(vec![answer(true, Tier::Free)], false).await;
        let mut f = fx(port).await;
        assert_eq!(f.task.beat().await, Duration::from_secs(10));
        f.status.send_modify(|s| s.live = true);
        assert_eq!(f.task.beat().await, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn a_confirmed_fence_clears_provisional_and_the_licence_is_stored() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        assert!(row(&f.path).provisional);
        f.task.beat().await;
        let s = f.status.borrow().clone();
        assert_eq!(s.tier, Some(Tier::Basic));
        assert_eq!(s.pairing, PairingState::Active);
        assert_eq!(s.cloud_time.unwrap().cloud_ms, T0 + 7);
        assert!(!s.lease.unwrap().provisional);
        assert!(!row(&f.path).provisional);
        let stale = arena_store::LeaseRow {
            provisional: true,
            ..row(&f.path)
        };
        let renewed = f
            .writer
            .renew_lease(stale, 15_000, Arc::new(ManualClock::new(T0 + 3_000)))
            .await
            .unwrap();
        assert!(matches!(renewed, arena_core::LeaseRenewal::Renewed(r) if !r.provisional));
        let cached = Store::open(&f.path, T0).unwrap().cached_licence().unwrap();
        assert!(cached.is_some());
    }

    #[tokio::test]
    async fn the_request_carries_the_token_epoch_and_versions() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let port = serve_with(vec![answer(true, Tier::Free)], false, Arc::clone(&seen)).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        let raw = seen.lock().unwrap()[0].to_ascii_lowercase();
        assert!(raw.starts_with("post /v1/heartbeat "), "{raw}");
        assert!(raw.contains("authorization: bearer hub_x.y"), "{raw}");
        let body: serde_json::Value =
            serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "epoch": format!("1:{}", OWN.to_ascii_lowercase()),
                "version": "0.2.0",
                "schema_version": 12,
                "last_seq_uploaded": 0
            })
        );
    }

    #[tokio::test]
    async fn an_unknown_token_is_revoked_too() {
        let port = serve(vec![refused(401, "TOKEN_INVALID")], false).await;
        let mut f = fx(port).await;
        assert_eq!(f.task.beat().await, REVOKED_BACKOFF);
        assert_eq!(f.status.borrow().pairing, PairingState::Revoked);
    }

    #[tokio::test]
    async fn other_refusals_and_garbage_back_off_without_revoking() {
        for reply in [
            refused(429, "RATE_LIMITED"),
            refused(500, "INTERNAL"),
            (200, "not json".to_owned()),
        ] {
            let port = serve(vec![reply], false).await;
            let mut f = fx(port).await;
            let wait = f.task.beat().await;
            assert!(wait <= Duration::from_secs(4), "{wait:?}");
            assert_eq!(f.status.borrow().pairing, PairingState::Active);
        }
    }

    #[tokio::test]
    async fn an_unconfirmed_fence_keeps_provisional() {
        let port = serve(vec![answer(false, Tier::Free)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert!(f.status.borrow().lease.as_ref().unwrap().provisional);
        assert!(row(&f.path).provisional);
    }

    #[tokio::test]
    async fn an_untrusted_licence_is_ignored() {
        let (_, mut body) = answer(true, Tier::Basic);
        body = body.replacen("eyJ", "xyz", 1);
        let port = serve(vec![(200, body)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert_eq!(f.status.borrow().tier, None);
        assert!(!f.status.borrow().lease.as_ref().unwrap().provisional);
        let cached = Store::open(&f.path, T0).unwrap().cached_licence().unwrap();
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn a_revoked_token_shows_the_banner_state_and_backs_off_five_minutes() {
        let port = serve(
            vec![refused(401, "TOKEN_REVOKED"), answer(true, Tier::Free)],
            false,
        )
        .await;
        let mut f = fx(port).await;
        assert_eq!(f.task.beat().await, REVOKED_BACKOFF);
        assert_eq!(f.status.borrow().pairing, PairingState::Revoked);
        assert!(f.status.borrow().lease.is_some());
        f.task.beat().await;
        assert_eq!(f.status.borrow().pairing, PairingState::Active);
    }

    #[tokio::test]
    async fn a_dead_network_backs_off_with_jitter_inside_the_bounds() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut f = fx(port).await;
        let first = f.task.beat().await;
        assert!(
            (Duration::from_secs(2)..=Duration::from_secs(4)).contains(&first),
            "{first:?}"
        );
        let second = f.task.beat().await;
        assert!(
            (Duration::from_secs(4)..=Duration::from_secs(8)).contains(&second),
            "{second:?}"
        );
        assert_eq!(f.status.borrow().pairing, PairingState::Active);
    }

    #[tokio::test]
    async fn an_intent_completes_while_the_heartbeat_hangs() {
        let port = serve(vec![answer(true, Tier::Free)], true).await;
        let mut f = fx(port).await;
        let hung = tokio::spawn(async move { f.task.beat().await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!hung.is_finished());
        let done = tokio::time::timeout(Duration::from_secs(2), f.writer.submit(|_tx| Ok(7))).await;
        assert_eq!(done.unwrap().unwrap(), 7);
        hung.abort();
    }
}

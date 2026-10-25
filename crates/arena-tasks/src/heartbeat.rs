use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_core::{CloudTime, HubStatus, PairGate, PairingState, Role, WriterHandle};
use arena_proto::{
    Entitlement, ErrorCode, HEARTBEAT_BOOT_INTERVAL_S, HEARTBEAT_LIVE_INTERVAL_S, HeartbeatRequest,
    HeartbeatResponse, LicenceStanding, VerifyKey,
};
use arena_store::TrustedClockRow;
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

fn mono_ms() -> i64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
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
    rejected: bool,
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
            rejected: false,
        }
    }

    pub async fn run(mut self) {
        let mut paired = self.gate.subscribe();
        if paired.wait_for(|p| *p).await.is_err() {
            return;
        }
        self.restore().await;
        loop {
            let wait = self.beat().await;
            tokio::time::sleep(wait).await;
        }
    }

    async fn restore(&mut self) {
        if let Some(writer) = &self.writer
            && let Ok(state) = writer.licence_state().await
        {
            self.rejected = state.status.as_deref() == Some(LicenceStanding::Unverified.as_str());
            if let Some(t) = state.trusted {
                let base = t
                    .cloud_ms
                    .max(writer.now_ms().saturating_sub(t.wall_delta_ms));
                self.status.send_modify(|s| {
                    s.cloud_time.get_or_insert(CloudTime {
                        cloud_ms: base,
                        at: Instant::now(),
                    });
                });
            }
        }
        self.settle().await;
    }

    fn trusted_now(&self) -> Option<i64> {
        self.status
            .borrow()
            .cloud_time
            .map(|c| c.cloud_ms.saturating_add(c.at.elapsed().as_millis() as i64))
    }

    pub async fn beat(&mut self) -> Duration {
        let wait = self.exchange().await;
        self.settle().await;
        wait
    }

    async fn settle(&self) {
        let Some(writer) = &self.writer else { return };
        let Ok(state) = writer.licence_state().await else {
            return;
        };
        let Some(id) = state.identity else { return };
        let wall = writer.now_ms();
        let cloud = self.trusted_now();
        let trusted = cloud.unwrap_or(wall);
        let mut e = Entitlement::evaluate(
            state.jwt.as_deref(),
            &self.keys,
            &id.tenant_id,
            &id.branch_id,
            trusted,
        );
        if self.rejected {
            e = Entitlement::free(LicenceStanding::Unverified);
        }
        let clock_wrong = cloud.is_some() && arena_proto::clock_wrong(wall, trusted);
        self.status.send_modify(|s| {
            s.tier = Some(e.tier);
            s.licence = Some(e.standing);
            s.clock_wrong = clock_wrong;
        });
        if let Err(err) = writer.apply_licence(e.apply_payload(clock_wrong)).await {
            tracing::warn!(
                code = err.code.as_str(),
                "heartbeat: could not apply the licence"
            );
        }
    }

    async fn exchange(&mut self) -> Duration {
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
        &mut self,
        writer: &WriterHandle,
        lease: &arena_core::LeaseInfo,
        res: HeartbeatResponse,
        at: Instant,
    ) {
        let Ok(state) = writer.licence_state().await else {
            return;
        };
        let cloud_ms = self
            .trusted_now()
            .map_or(res.cloud_time, |p| res.cloud_time.max(p));
        let genuine = state.identity.is_some_and(|id| {
            Entitlement::evaluate(
                Some(&res.licence),
                &self.keys,
                &id.tenant_id,
                &id.branch_id,
                cloud_ms,
            )
            .standing
                != LicenceStanding::Unverified
        });
        if !genuine {
            tracing::warn!("heartbeat: licence not trusted; ignored");
        }
        let confirm = res.fence.confirmed;
        let sent = confirm.then(|| (lease.counter, lease.hub_instance_id.clone()));
        let stored = genuine.then(|| res.licence.clone());
        let trusted_ms = cloud_ms.saturating_add(at.elapsed().as_millis() as i64);
        let row = TrustedClockRow {
            cloud_ms: trusted_ms,
            mono_ms: mono_ms(),
            wall_delta_ms: writer.now_ms().saturating_sub(trusted_ms),
        };
        if let Err(e) = writer.record_heartbeat(stored, sent, Some(row)).await {
            tracing::warn!(
                code = e.code.as_str(),
                "heartbeat: could not record the answer"
            );
            return;
        }
        self.rejected = !genuine;
        self.status.send_modify(|s| {
            s.pairing = PairingState::Active;
            s.cloud_time = Some(CloudTime { cloud_ms, at });
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
        answer_for("T", "B", confirmed, tier)
    }

    fn answer_for(tenant: &str, branch: &str, confirmed: bool, tier: Tier) -> (u16, String) {
        answer_at(tenant, branch, confirmed, tier, T0 + 7)
    }

    fn answer_at(
        tenant: &str,
        branch: &str,
        confirmed: bool,
        tier: Tier,
        cloud_ms: i64,
    ) -> (u16, String) {
        let claims = LicenceClaims::for_tier(tenant, branch, tier, T0 / 1000);
        let body = serde_json::json!({
            "licence": dev_sign(&claims).unwrap(),
            "fence": {"epoch": format!("1:{OWN}"), "confirmed": confirmed},
            "cloud_time": cloud_ms,
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
        wall: Arc<ManualClock>,
    }

    async fn fx(port: u16) -> Fx {
        fx_with(port, true).await
    }

    async fn fx_with(port: u16, with_branch: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(12, OWN));
        let wall = Arc::new(ManualClock::new(T0));
        let clock: Arc<dyn Clock> = wall.clone();
        let lease = LeaseTask::start(store, OWN.into(), Arc::clone(&clock), status.clone())
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        let paired = LicenceClaims::for_tier("T", "B", Tier::Free, T0 / 1000);
        writer
            .record_pairing(
                arena_store::HubIdentity {
                    tenant_id: "T".into(),
                    branch_id: "B".into(),
                    hub_id: "H".into(),
                    tier: Tier::Free,
                    paired_at: T0,
                },
                dev_sign(&paired).unwrap(),
            )
            .await
            .unwrap();
        if with_branch {
            add_branch(&writer).await;
        }
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
            wall,
        }
    }

    async fn add_branch(writer: &WriterHandle) {
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

    fn setting(path: &std::path::Path, key: &str) -> Option<String> {
        Store::open(path, T0)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| s.value)
    }

    fn audit_kinds(path: &std::path::Path) -> Vec<String> {
        let mut kinds: Vec<_> = Store::open(path, T0)
            .unwrap()
            .changes()
            .unwrap()
            .into_iter()
            .filter(|c| c.intent_kind == "licence.apply")
            .map(|c| c.row_id)
            .collect();
        kinds.sort();
        kinds
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
        assert_eq!(f.status.borrow().tier, Some(Tier::Free));
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
        assert!(!f.status.borrow().lease.as_ref().unwrap().provisional);
        let cached = Store::open(&f.path, T0).unwrap().cached_licence().unwrap();
        let paired = LicenceClaims::for_tier("T", "B", Tier::Free, T0 / 1000);
        assert_eq!(cached.unwrap().jwt, dev_sign(&paired).unwrap());
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

    #[tokio::test]
    async fn a_genuine_paid_licence_writes_its_limits_and_flags_once() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("60")
        );
        assert_eq!(setting(&f.path, "licence.user_cap").as_deref(), Some("10"));
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("verified")
        );
        assert_eq!(setting(&f.path, "licence.clock").as_deref(), Some("ok"));
        let rows = audit_kinds(&f.path);
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert!(rows.contains(&"B/licence.status".to_owned()));
        f.task.beat().await;
        assert_eq!(audit_kinds(&f.path), rows);
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
    }

    #[tokio::test]
    async fn another_branchs_licence_means_free_limits_a_warning_and_an_audit_row() {
        let port = serve(vec![answer_for("T", "OTHER", true, Tier::Premium)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        let s = f.status.borrow().clone();
        assert_eq!(s.licence, Some(LicenceStanding::Unverified));
        assert_eq!(s.tier, Some(Tier::Free));
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("20")
        );
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("unverified")
        );
        let rows = audit_kinds(&f.path);
        assert!(rows.contains(&"B/licence.status".to_owned()), "{rows:?}");
        assert!(
            rows.contains(&"B/licence.device_cap".to_owned()),
            "{rows:?}"
        );
    }

    #[tokio::test]
    async fn a_later_genuine_licence_clears_the_warning() {
        let port = serve(
            vec![
                answer_for("T", "OTHER", true, Tier::Premium),
                answer(true, Tier::Basic),
            ],
            false,
        )
        .await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("verified")
        );
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("60")
        );
    }

    #[tokio::test]
    async fn a_wall_clock_far_from_trusted_time_warns_until_the_clocks_agree() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert!(!f.status.borrow().clock_wrong);
        f.wall.set(T0 - 7 * 86_400_000);
        f.task.beat().await;
        assert!(f.status.borrow().clock_wrong);
        assert_eq!(setting(&f.path, "licence.clock").as_deref(), Some("wrong"));
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        f.wall.set(T0 + 5_000);
        f.task.beat().await;
        assert!(!f.status.borrow().clock_wrong);
        assert_eq!(setting(&f.path, "licence.clock").as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn a_skew_inside_five_minutes_is_not_a_warning() {
        let port = serve(vec![answer(true, Tier::Free)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        f.wall.set(T0 + 4 * 60_000);
        f.task.beat().await;
        assert!(!f.status.borrow().clock_wrong);
    }

    #[tokio::test]
    async fn expiry_follows_trusted_time_not_the_wall_clock() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        f.wall.set(T0 + 30 * 86_400_000);
        f.task.settle().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        assert_eq!(f.status.borrow().tier, Some(Tier::Basic));
        f.wall.set(T0 - 30 * 86_400_000);
        f.task.settle().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        let past = T0 + 8 * 86_400_000;
        f.status.send_modify(|s| {
            s.cloud_time = Some(CloudTime {
                cloud_ms: past,
                at: Instant::now(),
            })
        });
        f.task.settle().await;
        let s = f.status.borrow().clone();
        assert_eq!(
            (s.licence, s.tier),
            (Some(LicenceStanding::Expired), Some(Tier::Free))
        );
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("20")
        );
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("expired")
        );
    }

    #[tokio::test]
    async fn the_trusted_clock_survives_a_restart_and_never_runs_behind_the_last_cloud_time() {
        let port = serve(vec![answer(true, Tier::Free)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        let saved = Store::open(&f.path, T0)
            .unwrap()
            .trusted_clock()
            .unwrap()
            .unwrap();
        assert_eq!(saved.cloud_ms / 1000, (T0 + 7) / 1000);
        assert!((saved.wall_delta_ms + 7).abs() < 1000, "{saved:?}");
        f.status.send_modify(|s| s.cloud_time = None);
        f.wall.set(T0 - 7 * 86_400_000);
        f.task.restore().await;
        let c = f.status.borrow().cloud_time.unwrap();
        assert_eq!(c.cloud_ms, saved.cloud_ms);
        f.status.send_modify(|s| s.cloud_time = None);
        f.wall.set(T0 + 3_600_000);
        f.task.restore().await;
        let c = f.status.borrow().cloud_time.unwrap();
        assert_eq!(c.cloud_ms, T0 + 3_600_000 - saved.wall_delta_ms);
    }

    fn cached(path: &std::path::Path) -> String {
        Store::open(path, T0)
            .unwrap()
            .cached_licence()
            .unwrap()
            .unwrap()
            .jwt
    }

    fn free_jwt() -> String {
        dev_sign(&LicenceClaims::for_tier("T", "B", Tier::Free, T0 / 1000)).unwrap()
    }

    fn basic_jwt() -> String {
        dev_sign(&LicenceClaims::for_tier("T", "B", Tier::Basic, T0 / 1000)).unwrap()
    }

    #[tokio::test]
    async fn a_rejected_licence_is_never_cached_but_the_clock_still_is() {
        let tampered = answer(true, Tier::Basic).1.replacen("eyJ", "xyz", 1);
        let port = serve(
            vec![
                answer_for("T", "OTHER", true, Tier::Premium),
                (200, tampered),
            ],
            false,
        )
        .await;
        let mut f = fx(port).await;
        for _ in 0..2 {
            f.task.beat().await;
            assert_eq!(cached(&f.path), free_jwt());
            assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
        }
        assert!(
            Store::open(&f.path, T0)
                .unwrap()
                .trusted_clock()
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_follower_publishes_the_standing_and_writes_nothing() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.status.send_modify(|s| s.role = Role::Follower);
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        assert_eq!(setting(&f.path, "licence.device_cap"), None);
        assert!(audit_kinds(&f.path).is_empty());
    }

    #[tokio::test]
    async fn before_the_branch_exists_the_standing_is_published_and_the_rows_follow() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx_with(port, false).await;
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        assert_eq!(setting(&f.path, "licence.device_cap"), None);
        add_branch(&f.writer).await;
        f.task.beat().await;
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("60")
        );
    }

    #[tokio::test]
    async fn trusted_time_is_cloud_time_plus_monotonic_elapsed() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        let long_ago = Instant::now()
            .checked_sub(Duration::from_secs(8 * 86_400))
            .unwrap();
        f.status.send_modify(|s| {
            s.cloud_time = Some(CloudTime {
                cloud_ms: T0,
                at: long_ago,
            })
        });
        f.task.settle().await;
        let s = f.status.borrow().clone();
        assert_eq!(s.licence, Some(LicenceStanding::Expired));
        assert!(s.clock_wrong);
    }

    #[tokio::test]
    async fn without_a_cloud_time_expiry_uses_the_wall_clock_and_nothing_warns() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        f.status.send_modify(|s| s.cloud_time = None);
        f.wall.set(T0 + 30 * 86_400_000);
        f.task.settle().await;
        let s = f.status.borrow().clone();
        assert_eq!(
            (s.licence, s.clock_wrong),
            (Some(LicenceStanding::Expired), false)
        );
        f.wall.set(T0);
        f.task.settle().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
    }

    #[tokio::test]
    async fn start_up_publishes_the_tier_and_writes_the_rows_before_any_beat() {
        let port = serve(vec![answer(true, Tier::Basic)], false).await;
        let mut f = fx(port).await;
        f.task.beat().await;
        f.status.send_modify(|s| {
            s.tier = None;
            s.licence = None;
            s.cloud_time = None;
        });
        f.task.restore().await;
        let s = f.status.borrow().clone();
        assert_eq!(
            (s.tier, s.licence),
            (Some(Tier::Basic), Some(LicenceStanding::Verified))
        );
        assert!(s.cloud_time.is_some());
        assert_eq!(
            setting(&f.path, "licence.device_cap").as_deref(),
            Some("60")
        );
    }

    #[tokio::test]
    async fn a_failed_beat_still_settles_and_keeps_the_rejection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut f = fx(port).await;
        f.writer
            .record_heartbeat(Some(basic_jwt()), None, None)
            .await
            .unwrap();
        f.status.send_modify(|s| {
            s.cloud_time = Some(CloudTime {
                cloud_ms: T0 + 8 * 86_400_000,
                at: Instant::now(),
            })
        });
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Expired));
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("expired")
        );
        f.task.rejected = true;
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
    }

    #[tokio::test]
    async fn the_warning_survives_a_restart_until_a_licence_verifies() {
        let port = serve(
            vec![
                answer_for("T", "OTHER", true, Tier::Premium),
                answer(true, Tier::Basic),
            ],
            false,
        )
        .await;
        let mut f = fx(port).await;
        f.task.beat().await;
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("unverified")
        );
        f.task.rejected = false;
        f.task.restore().await;
        assert!(f.task.rejected);
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Unverified));
        f.task.beat().await;
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        assert!(!f.task.rejected);
    }

    #[tokio::test]
    async fn an_expired_paid_licence_from_the_cloud_is_kept_and_does_not_warn() {
        let port = serve(
            vec![answer_at("T", "B", true, Tier::Basic, T0 + 8 * 86_400_000)],
            false,
        )
        .await;
        let mut f = fx(port).await;
        f.task.beat().await;
        let s = f.status.borrow().clone();
        assert_eq!(
            (s.licence, s.tier),
            (Some(LicenceStanding::Expired), Some(Tier::Free))
        );
        assert_eq!(cached(&f.path), basic_jwt());
        assert_eq!(
            setting(&f.path, "licence.status").as_deref(),
            Some("expired")
        );
    }

    #[tokio::test]
    async fn a_cloud_time_that_goes_backwards_never_moves_trusted_time_back() {
        let port = serve(
            vec![
                answer_at("T", "B", true, Tier::Basic, T0 + 1_000),
                answer_at("T", "B", true, Tier::Basic, T0 - 9 * 86_400_000),
            ],
            false,
        )
        .await;
        let mut f = fx(port).await;
        f.task.beat().await;
        f.task.beat().await;
        let c = f.status.borrow().cloud_time.unwrap();
        assert!(c.cloud_ms >= T0 + 1_000, "{}", c.cloud_ms);
        assert_eq!(f.status.borrow().licence, Some(LicenceStanding::Verified));
        let saved = Store::open(&f.path, T0)
            .unwrap()
            .trusted_clock()
            .unwrap()
            .unwrap();
        assert!(saved.cloud_ms >= T0 + 1_000);
    }
}

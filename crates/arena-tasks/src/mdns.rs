use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{Applied, HubStatus, Role, SnapshotReader};
use arena_proto::{HubPlatform, MDNS_SERVICE, PROTOCOL_VERSION, TXT_EPOCH, txt_records};
use tokio::sync::{Notify, broadcast, watch};
use tokio::time::{Instant, MissedTickBehavior};

pub const MDNS_POLL_INTERVAL: Duration = Duration::from_secs(30);
pub const MDNS_FAILURE_LOG_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advert {
    pub instance: String,
    pub host: String,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

impl Advert {
    pub fn new(
        hub_name: &str,
        port: u16,
        web_port: u16,
        branch_name: &str,
        fingerprint: &str,
    ) -> Self {
        Self {
            instance: hub_name.to_owned(),
            host: mdns_host_name(hub_name, fingerprint),
            port,
            txt: txt_records(
                branch_name,
                hub_name,
                PROTOCOL_VERSION,
                fingerprint,
                web_port,
            ),
        }
    }
}

impl Advert {
    /// Adds the Hub's epoch to the TXT record (Story 9.21), so a kiosk that re-resolves after a
    /// promote sees the new one without opening a connection. `None` leaves the record as it was.
    pub fn with_epoch(mut self, epoch: Option<String>) -> Self {
        if let Some(epoch) = epoch {
            self.txt.push((TXT_EPOCH.to_owned(), epoch));
        }
        self
    }
}

pub fn mdns_host_name(hub_name: &str, fingerprint: &str) -> String {
    let mut label = String::new();
    for c in hub_name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            label.push(c);
        } else if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    label.truncate(40);
    let label = label.trim_end_matches('-');
    let label = if label.is_empty() {
        "arena360-hub"
    } else {
        label
    };
    let suffix: String = fingerprint.chars().take(8).collect();
    format!("{label}-{suffix}.local.")
}

pub trait Advertiser: Send + Sync + 'static {
    fn register(&mut self, advert: &Advert) -> Result<(), String>;
    fn unregister(&mut self) -> Result<(), String>;
    fn shutdown(&mut self) {}
}

impl Advertiser for Box<dyn Advertiser> {
    fn register(&mut self, advert: &Advert) -> Result<(), String> {
        (**self).register(advert)
    }
    fn unregister(&mut self) -> Result<(), String> {
        (**self).unregister()
    }
    fn shutdown(&mut self) {
        (**self).shutdown();
    }
}

/// Names the network the Hub is on, for hosts that can (Android: "Wi-Fi", "Ethernet").
pub trait NetworkName: Send + Sync + 'static {
    fn name(&self) -> Option<String>;
}

pub trait BranchSource: Send + Sync + 'static {
    fn branch_name(&self) -> impl Future<Output = Result<Option<String>, String>> + Send;
}

impl BranchSource for Arc<SnapshotReader> {
    async fn branch_name(&self) -> Result<Option<String>, String> {
        let reader = Arc::clone(self);
        match tokio::task::spawn_blocking(move || reader.branch()).await {
            Ok(Ok(b)) => Ok(b.map(|b| b.name)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub trait Interfaces: Send + Sync + 'static {
    fn ipv4(&self) -> Vec<Ipv4Addr>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemInterfaces;

impl Interfaces for SystemInterfaces {
    fn ipv4(&self) -> Vec<Ipv4Addr> {
        lan_ipv4_addresses()
    }
}

pub fn lan_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => Some(v4),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[derive(Default)]
pub struct MdnsAdvertiser {
    daemon: Option<mdns_sd::ServiceDaemon>,
    registered: Option<String>,
}

impl MdnsAdvertiser {
    fn daemon(&mut self) -> Result<&mdns_sd::ServiceDaemon, String> {
        if self.daemon.is_none() {
            let d = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
            let _ = d.disable_interface(mdns_sd::IfKind::IPv6);
            let _ = d.disable_interface(mdns_sd::IfKind::LoopbackV4);
            self.daemon = Some(d);
        }
        self.daemon
            .as_ref()
            .ok_or_else(|| "no mdns daemon".to_owned())
    }
}

impl Advertiser for MdnsAdvertiser {
    fn register(&mut self, advert: &Advert) -> Result<(), String> {
        let props: HashMap<String, String> = advert.txt.iter().cloned().collect();
        let info = mdns_sd::ServiceInfo::new(
            MDNS_SERVICE,
            &advert.instance,
            &advert.host,
            (),
            advert.port,
            props,
        )
        .map_err(|e| e.to_string())?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_owned();
        let result = self.daemon()?.register(info).map_err(|e| e.to_string());
        if result.is_err() {
            self.daemon = None;
        } else {
            self.registered = Some(fullname);
        }
        result
    }

    fn unregister(&mut self) -> Result<(), String> {
        let Some(fullname) = self.registered.take() else {
            return Ok(());
        };
        let Some(d) = self.daemon.as_ref() else {
            return Ok(());
        };
        d.unregister(&fullname).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(d) = self.daemon.take()
            && let Ok(rx) = d.shutdown()
        {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }
}

pub struct MdnsTask<A: Advertiser, B: BranchSource, I: Interfaces> {
    advertiser: A,
    branch: B,
    interfaces: I,
    status: watch::Sender<HubStatus>,
    hub_name: String,
    lan_port: u16,
    web_port: u16,
    applied: Option<broadcast::Receiver<Applied>>,
    poll: Duration,
    wake: Option<Arc<Notify>>,
    network: Option<Arc<dyn NetworkName>>,
}

impl<A: Advertiser, B: BranchSource, I: Interfaces> MdnsTask<A, B, I> {
    pub fn new(
        advertiser: A,
        branch: B,
        interfaces: I,
        status: watch::Sender<HubStatus>,
        hub_name: impl Into<String>,
        lan_port: u16,
        web_port: u16,
    ) -> Self {
        Self {
            advertiser,
            branch,
            interfaces,
            status,
            hub_name: hub_name.into(),
            lan_port,
            web_port,
            applied: None,
            poll: MDNS_POLL_INTERVAL,
            wake: None,
            network: None,
        }
    }

    /// The host calls `notify_one` when the network changes (an IP renewal, Wi-Fi back), so the
    /// addresses are re-read now instead of at the next poll.
    pub fn with_wake(mut self, wake: Option<Arc<Notify>>) -> Self {
        self.wake = wake;
        self
    }

    pub fn with_network(mut self, network: Option<Arc<dyn NetworkName>>) -> Self {
        self.network = network;
        self
    }

    pub fn with_applied(mut self, applied: Option<broadcast::Receiver<Applied>>) -> Self {
        self.applied = applied;
        self
    }

    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    fn refresh_addresses(&self) {
        let addrs = self.interfaces.ipv4();
        let network = self.network.as_ref().and_then(|n| n.name());
        self.status.send_if_modified(|s| {
            if s.lan_addresses == addrs && s.lan_network == network {
                false
            } else {
                s.lan_addresses = addrs.clone();
                s.lan_network = network.clone();
                true
            }
        });
    }

    async fn desired(&self, current: &Option<Advert>) -> Option<Advert> {
        let (fingerprint, primary, no_lan, epoch) = {
            let s = self.status.borrow();
            (
                s.tls_fingerprint.clone(),
                s.role == Role::Primary,
                // An Android Hub with no address withdraws its advert and re-advertises when
                // the network returns; elsewhere the advert stays as it always did.
                s.platform == HubPlatform::Android && s.lan_addresses.is_empty(),
                s.lease.as_ref().map(|l| l.epoch().to_string()),
            )
        };
        let (Some(fingerprint), true, false) = (fingerprint, primary, no_lan) else {
            return None;
        };
        match self.branch.branch_name().await {
            Ok(Some(name)) => Some(
                Advert::new(
                    &self.hub_name,
                    self.lan_port,
                    self.web_port,
                    &name,
                    &fingerprint,
                )
                .with_epoch(epoch),
            ),
            Ok(None) => None,
            Err(e) => {
                tracing::debug!(error = %e, "mdns: the branch read failed; keeping the advert");
                current.clone()
            }
        }
    }

    pub async fn run(mut self, shutdown: impl Future<Output = ()> + Send) {
        let mut status_rx = self.status.subscribe();
        let mut tick = tokio::time::interval(self.poll);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await;
        let mut current: Option<Advert> = None;
        let mut last_failure_log: Option<Instant> = None;
        let mut failed: Option<Advert> = None;
        let mut by_tick = true;
        tokio::pin!(shutdown);
        loop {
            self.refresh_addresses();
            status_rx.borrow_and_update();
            let want = self.desired(&current).await;
            let waiting_for_tick = !by_tick && want.is_some() && failed == want;
            if want != current && !waiting_for_tick {
                failed = None;
                if current.is_some() {
                    if let Err(e) = self.advertiser.unregister() {
                        tracing::debug!(error = %e, "mdns: unregister failed");
                    }
                    current = None;
                }
                if let Some(advert) = want {
                    match self.advertiser.register(&advert) {
                        Ok(()) => {
                            tracing::info!(
                                instance = %advert.instance,
                                port = advert.port,
                                "mdns: hub advertised"
                            );
                            current = Some(advert);
                        }
                        Err(e) => {
                            failed = Some(advert);
                            let due = last_failure_log
                                .is_none_or(|at| at.elapsed() >= MDNS_FAILURE_LOG_INTERVAL);
                            if due {
                                last_failure_log = Some(Instant::now());
                                tracing::warn!(error = %e, "mdns: cannot advertise the hub; kiosks can still use its IP");
                            }
                        }
                    }
                } else {
                    tracing::info!("mdns: hub not advertised (no branch yet, or not primary)");
                }
            }
            by_tick = false;
            tokio::select! {
                () = &mut shutdown => break,
                _ = tick.tick() => by_tick = true,
                () = async {
                    match self.wake.as_ref() {
                        Some(w) => w.notified().await,
                        None => std::future::pending().await,
                    }
                } => by_tick = true,
                changed = status_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                r = async {
                    match self.applied.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Err(broadcast::error::RecvError::Closed) = r {
                        self.applied = None;
                    }
                }
            }
        }
        if current.is_some()
            && let Err(e) = self.advertiser.unregister()
        {
            tracing::debug!(error = %e, "mdns: unregister at shutdown failed");
        }
        self.advertiser.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Register(Advert),
        Unregister,
        Shutdown,
    }

    #[derive(Clone, Default)]
    struct FakeDaemon {
        calls: Arc<Mutex<Vec<Call>>>,
        fail: Arc<Mutex<bool>>,
        attempts: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FakeDaemon {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Advertiser for FakeDaemon {
        fn register(&mut self, advert: &Advert) -> Result<(), String> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if *self.fail.lock().unwrap() {
                return Err("no multicast".into());
            }
            self.calls
                .lock()
                .unwrap()
                .push(Call::Register(advert.clone()));
            Ok(())
        }
        fn unregister(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push(Call::Unregister);
            Ok(())
        }
        fn shutdown(&mut self) {
            self.calls.lock().unwrap().push(Call::Shutdown);
        }
    }

    #[derive(Clone, Default)]
    struct FakeBranch(Arc<Mutex<Option<String>>>);

    impl BranchSource for FakeBranch {
        async fn branch_name(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    #[derive(Clone, Default)]
    struct FakeIfs(Arc<Mutex<Vec<Ipv4Addr>>>);

    impl Interfaces for FakeIfs {
        fn ipv4(&self) -> Vec<Ipv4Addr> {
            self.0.lock().unwrap().clone()
        }
    }

    fn status() -> watch::Sender<HubStatus> {
        let mut s = HubStatus::opened(1, "01JHUB");
        s.role = Role::Primary;
        s.tls_fingerprint = Some("ab".repeat(32));
        watch::channel(s).0
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn the_advert_carries_the_txt_keys() {
        let a = Advert::new("front-desk", 7400, 7401, "Koramangala", &"cd".repeat(32));
        assert_eq!(a.instance, "front-desk");
        assert_eq!(a.port, 7400);
        let keys: Vec<&str> = a.txt.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["branch", "hub", "proto", "fp", "web"]);
        assert_eq!(a.txt[0].1, "Koramangala");
        assert_eq!(a.txt[2].1, PROTOCOL_VERSION.to_string());
        assert_eq!(a.txt[3].1, "cd".repeat(32));
        assert_eq!(a.txt[4].1, "7401");
        assert_eq!(a.host, "front-desk-cdcdcdcd.local.");
    }

    #[test]
    fn the_epoch_joins_the_txt_record_only_when_there_is_one() {
        let plain = Advert::new("front-desk", 7400, 7401, "B", &"ab".repeat(32));
        assert_eq!(plain.clone().with_epoch(None), plain);
        let with = plain.with_epoch(Some("4:01JSTBY".into()));
        assert_eq!(
            with.txt.last().unwrap(),
            &("epoch".to_owned(), "4:01JSTBY".to_owned())
        );
        let get = |k: &str| {
            with.txt
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(arena_proto::txt_epoch(get).as_deref(), Some("4:01JSTBY"));
        assert_eq!(arena_proto::txt_epoch(|_| Some("nonsense".into())), None);
        assert_eq!(arena_proto::txt_epoch(|_| None), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_epoch_is_advertised_again_and_a_renewal_is_not() {
        let daemon = FakeDaemon::default();
        let status = status();
        status.send_modify(|s| {
            s.lease = Some(arena_core::LeaseInfo {
                counter: 3,
                hub_instance_id: "01JHUB".into(),
                provisional: false,
                expires_at: 100,
            });
        });
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let job = tokio::spawn(
            MdnsTask::new(
                daemon.clone(),
                FakeBranch(Arc::new(Mutex::new(Some("B".into())))),
                FakeIfs::default(),
                status.clone(),
                "h",
                7400,
                7401,
            )
            .run(async {
                let _ = stop_rx.await;
            }),
        );
        settle().await;
        let epoch_of = |call: &Call| match call {
            Call::Register(a) => a
                .txt
                .iter()
                .find(|(k, _)| k == "epoch")
                .map(|(_, v)| v.clone()),
            _ => None,
        };
        assert_eq!(daemon.calls().len(), 1);
        assert_eq!(epoch_of(&daemon.calls()[0]).as_deref(), Some("3:01JHUB"));

        // A lease renewal changes the expiry only.
        status.send_modify(|s| s.lease.as_mut().unwrap().expires_at = 200);
        settle().await;
        assert_eq!(daemon.calls().len(), 1, "a renewal is not a new epoch");

        // A promote opens a new one.
        status.send_modify(|s| s.lease.as_mut().unwrap().counter = 4);
        settle().await;
        let calls = daemon.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1], Call::Unregister);
        assert_eq!(epoch_of(&calls[2]).as_deref(), Some("4:01JHUB"));
        stop_tx.send(()).unwrap();
        job.await.unwrap();
    }

    #[test]
    fn two_hubs_get_different_host_names() {
        let a = Advert::new("Front Desk", 7400, 7401, "B", &"ab".repeat(32));
        let b = Advert::new("Front Desk", 7400, 7401, "B", &"cd".repeat(32));
        assert_ne!(a.host, b.host);
        assert_eq!(a.host, "front-desk-abababab.local.");
        assert_eq!(
            mdns_host_name("éé!!", &"12".repeat(32)),
            "arena360-hub-12121212.local."
        );
    }

    #[tokio::test(start_paused = true)]
    async fn registers_once_a_branch_exists_and_re_registers_on_change() {
        let daemon = FakeDaemon::default();
        let branch = FakeBranch::default();
        let ifs = FakeIfs::default();
        *ifs.0.lock().unwrap() = vec![Ipv4Addr::new(192, 168, 1, 10)];
        let status = status();
        let (events_tx, events_rx) = broadcast::channel(4);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let task = MdnsTask::new(
            daemon.clone(),
            branch.clone(),
            ifs.clone(),
            status.clone(),
            "front-desk",
            7400,
            7401,
        )
        .with_applied(Some(events_rx))
        .with_poll(Duration::from_secs(30));
        let job = tokio::spawn(task.run(async {
            let _ = stop_rx.await;
        }));
        settle().await;
        assert!(
            daemon.calls().is_empty(),
            "nothing before the branch exists"
        );
        assert_eq!(
            status.borrow().lan_addresses,
            [Ipv4Addr::new(192, 168, 1, 10)]
        );

        *branch.0.lock().unwrap() = Some("Koramangala".into());
        events_tx
            .send(Applied {
                intent_id: "i1".into(),
                changes: Vec::new(),
            })
            .unwrap();
        settle().await;
        let first = Advert::new("front-desk", 7400, 7401, "Koramangala", &"ab".repeat(32));
        assert_eq!(daemon.calls(), [Call::Register(first.clone())]);

        tokio::time::advance(Duration::from_secs(31)).await;
        settle().await;
        assert_eq!(daemon.calls().len(), 1, "no change, no re-register");

        *branch.0.lock().unwrap() = Some("Indiranagar".into());
        tokio::time::advance(Duration::from_secs(31)).await;
        settle().await;
        let renamed = Advert::new("front-desk", 7400, 7401, "Indiranagar", &"ab".repeat(32));
        assert_eq!(
            daemon.calls(),
            [
                Call::Register(first),
                Call::Unregister,
                Call::Register(renamed.clone())
            ]
        );

        status.send_modify(|s| s.tls_fingerprint = Some("ef".repeat(32)));
        settle().await;
        let refp = Advert::new("front-desk", 7400, 7401, "Indiranagar", &"ef".repeat(32));
        assert_eq!(
            daemon.calls()[3..],
            [Call::Unregister, Call::Register(refp)]
        );

        *ifs.0.lock().unwrap() = vec![Ipv4Addr::new(10, 0, 0, 5)];
        tokio::time::advance(Duration::from_secs(31)).await;
        settle().await;
        assert_eq!(status.borrow().lan_addresses, [Ipv4Addr::new(10, 0, 0, 5)]);

        stop_tx.send(()).unwrap();
        job.await.unwrap();
        let calls = daemon.calls();
        assert_eq!(calls[calls.len() - 2..], [Call::Unregister, Call::Shutdown]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_follower_and_a_failing_daemon_never_stop_the_task() {
        let daemon = FakeDaemon::default();
        *daemon.fail.lock().unwrap() = true;
        let branch = FakeBranch(Arc::new(Mutex::new(Some("B".into()))));
        let status = status();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let job = tokio::spawn(
            MdnsTask::new(
                daemon.clone(),
                branch,
                FakeIfs::default(),
                status.clone(),
                "h",
                7400,
                7401,
            )
            .run(async {
                let _ = stop_rx.await;
            }),
        );
        settle().await;
        let attempts = || daemon.attempts.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(attempts(), 1);
        for i in 0..3 {
            status.send_modify(|s| s.lan_addresses = vec![Ipv4Addr::new(10, 0, 0, i)]);
            settle().await;
        }
        assert_eq!(attempts(), 1, "a failure is retried only on the tick");
        for _ in 0..5 {
            tokio::time::advance(Duration::from_secs(31)).await;
            settle().await;
        }
        assert_eq!(attempts(), 6);
        assert!(daemon.calls().is_empty());
        assert!(!job.is_finished());
        *daemon.fail.lock().unwrap() = false;
        tokio::time::advance(Duration::from_secs(31)).await;
        settle().await;
        assert_eq!(daemon.calls().len(), 1, "it retries until it works");
        status.send_modify(|s| s.role = Role::Follower);
        settle().await;
        assert_eq!(
            daemon.calls()[1],
            Call::Unregister,
            "a follower never advertises"
        );
        stop_tx.send(()).unwrap();
        job.await.unwrap();
    }

    struct FakeNetwork(Arc<Mutex<Option<String>>>);

    impl NetworkName for FakeNetwork {
        fn name(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_android_hub_withdraws_without_a_lan_and_readvertises_when_it_returns() {
        let daemon = FakeDaemon::default();
        let ifs = FakeIfs(Arc::new(Mutex::new(vec![Ipv4Addr::new(192, 168, 1, 7)])));
        let net = Arc::new(Mutex::new(Some("Wi-Fi".to_owned())));
        let status = status();
        status.send_modify(|s| s.platform = HubPlatform::Android);
        let wake = Arc::new(Notify::new());
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let job = tokio::spawn(
            MdnsTask::new(
                daemon.clone(),
                FakeBranch(Arc::new(Mutex::new(Some("Koramangala".into())))),
                ifs.clone(),
                status.clone(),
                "front-desk",
                7400,
                7401,
            )
            .with_wake(Some(Arc::clone(&wake)))
            .with_network(Some(Arc::new(FakeNetwork(Arc::clone(&net)))))
            .run(async {
                let _ = stop_rx.await;
            }),
        );
        settle().await;
        let advert = Advert::new("front-desk", 7400, 7401, "Koramangala", &"ab".repeat(32));
        assert_eq!(daemon.calls(), [Call::Register(advert.clone())]);
        assert_eq!(status.borrow().lan_network.as_deref(), Some("Wi-Fi"));

        // The Wi-Fi goes: the shell wakes the task, with no poll tick in between.
        *ifs.0.lock().unwrap() = vec![];
        *net.lock().unwrap() = None;
        wake.notify_one();
        settle().await;
        assert!(status.borrow().lan_addresses.is_empty());
        assert_eq!(daemon.calls()[1..], [Call::Unregister]);

        // It comes back on a new address: the advert returns at once.
        *ifs.0.lock().unwrap() = vec![Ipv4Addr::new(192, 168, 1, 9)];
        *net.lock().unwrap() = Some("Wi-Fi".into());
        wake.notify_one();
        settle().await;
        assert_eq!(daemon.calls()[2..], [Call::Register(advert)]);
        assert_eq!(
            status.borrow().lan_addresses,
            [Ipv4Addr::new(192, 168, 1, 9)]
        );

        stop_tx.send(()).unwrap();
        job.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_windows_hub_keeps_its_advert_without_an_address() {
        let daemon = FakeDaemon::default();
        let status = status();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let job = tokio::spawn(
            MdnsTask::new(
                daemon.clone(),
                FakeBranch(Arc::new(Mutex::new(Some("B".into())))),
                FakeIfs::default(),
                status,
                "h",
                7400,
                7401,
            )
            .run(async {
                let _ = stop_rx.await;
            }),
        );
        settle().await;
        assert_eq!(daemon.calls().len(), 1);
        stop_tx.send(()).unwrap();
        job.await.unwrap();
    }
}

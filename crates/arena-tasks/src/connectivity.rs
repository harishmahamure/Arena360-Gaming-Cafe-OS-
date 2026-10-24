//! The WAN connectivity probe: tells "the internet is unreachable" apart from "the internet
//! works but the cloud does not answer", once at start and every [`PROBE_INTERVAL`].
//!
//! Each round runs two independent checks at the same time, each bounded by
//! [`PROBE_TIMEOUT`]:
//!
//! - **Internet:** a TCP connect to each internet endpoint (by default `1.1.1.1:443` and
//!   `8.8.8.8:443`); any success means the internet is up.
//! - **Cloud:** a TCP connect plus a TLS handshake (rustls, the webpki roots) to the cloud host,
//!   then `GET /healthz`, which must answer 200.
//!
//! [`decide`] turns the two answers into a [`ConnectivityState`], published into
//! [`HubStatus::connectivity`] with `send_if_modified`. The task runs on its own and never
//! touches the Writer, so no intent ever waits on it. These are the branch's only outbound
//! connections.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, HubStatus};
use arena_proto::{Connectivity, ConnectivityState};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior, timeout};
use tokio_rustls::TlsConnector;

/// How often the probe runs after the first round at start.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(30);
/// The bound on each check of a round.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// The internet endpoints tried by default (`--probe-internet` overrides them).
pub const DEFAULT_INTERNET_ENDPOINTS: [&str; 2] = ["1.1.1.1:443", "8.8.8.8:443"];
/// The cloud host probed by default (`--cloud-host` overrides it): the production
/// `arena-cloud` control role behind Cloudflare, which answers `GET /healthz`.
pub const DEFAULT_CLOUD_HOST: &str = "api.arena360.cloud:443";
/// The longest status line + headers read from the cloud's answer.
const MAX_RESPONSE_HEAD: usize = 8 * 1024;

/// The pure decision: no internet is `internet_offline` whatever the cloud check said; the
/// internet without the cloud is `cloud_unreachable`; both is `online`.
pub fn decide(internet_ok: bool, cloud_ok: bool) -> ConnectivityState {
    match (internet_ok, cloud_ok) {
        (false, _) => ConnectivityState::InternetOffline,
        (true, false) => ConnectivityState::CloudUnreachable,
        (true, true) => ConnectivityState::Online,
    }
}

/// The two network checks. [`NetProber`] is the real one; tests use fakes.
pub trait Prober: Send + Sync + 'static {
    /// Whether any internet endpoint accepted a TCP connection.
    fn internet(&self) -> impl Future<Output = bool> + Send;
    /// Whether the cloud answered `GET /healthz` with 200 over TLS.
    fn cloud(&self) -> impl Future<Output = bool> + Send;
}

/// A `host:port` the cloud check connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudHost {
    pub host: String,
    pub port: u16,
}

impl CloudHost {
    /// Parses `host:port` (an IPv6 literal in brackets: `[::1]:443`). The port is required and
    /// not 0; the host is a DNS name or an IP address.
    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || format!("invalid cloud host {s:?} (host:port)");
        let (host, port) = s.rsplit_once(':').ok_or_else(bad)?;
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(bad)?;
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if host.is_empty() || host.contains(['/', ' ', '[', ']']) {
            return Err(bad());
        }
        ServerName::try_from(host.to_owned()).map_err(|_| bad())?;
        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }
}

/// Parses `--probe-internet`'s `addr,addr` (socket addresses, at least one).
pub fn parse_internet_endpoints(s: &str) -> Result<Vec<SocketAddr>, String> {
    let addrs = s
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.parse::<SocketAddr>()
                .map_err(|_| format!("invalid probe endpoint {a:?} (ip:port)"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if addrs.is_empty() {
        return Err("--probe-internet needs at least one ip:port".to_owned());
    }
    Ok(addrs)
}

/// The default internet endpoints as socket addresses.
pub fn default_internet_endpoints() -> Vec<SocketAddr> {
    DEFAULT_INTERNET_ENDPOINTS
        .iter()
        .map(|a| a.parse().expect("valid default endpoint"))
        .collect()
}

/// The real probe: plain TCP connects for the internet, TLS + `GET /healthz` for the cloud.
pub struct NetProber {
    internet: Vec<SocketAddr>,
    cloud: CloudHost,
    tls: Arc<ClientConfig>,
}

impl NetProber {
    /// Trusts the webpki (Mozilla) roots for the cloud.
    pub fn new(internet: Vec<SocketAddr>, cloud: CloudHost) -> Result<Self, String> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::with_roots(internet, cloud, roots)
    }

    /// Trusts only `roots` (tests).
    pub fn with_roots(
        internet: Vec<SocketAddr>,
        cloud: CloudHost,
        roots: rustls::RootCertStore,
    ) -> Result<Self, String> {
        let tls =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| e.to_string())?
                .with_root_certificates(roots)
                .with_no_client_auth();
        Ok(Self {
            internet,
            cloud,
            tls: Arc::new(tls),
        })
    }

    async fn healthz(&self) -> Result<bool, String> {
        let CloudHost { host, port } = &self.cloud;
        let name = ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
        let tcp = TcpStream::connect((host.as_str(), *port))
            .await
            .map_err(|e| e.to_string())?;
        let _ = tcp.set_nodelay(true);
        let mut tls = TlsConnector::from(Arc::clone(&self.tls))
            .connect(name, tcp)
            .await
            .map_err(|e| e.to_string())?;
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let request = format!(
            "GET /healthz HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: arena-hub-probe\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        tls.write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut head = Vec::with_capacity(256);
        let mut buf = [0u8; 512];
        while !head.windows(2).any(|w| w == b"\r\n") && head.len() < MAX_RESPONSE_HEAD {
            let n = tls.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&buf[..n]);
        }
        Ok(status_is_200(&head))
    }
}

/// Whether an HTTP/1.x response starts with status 200.
fn status_is_200(head: &[u8]) -> bool {
    let line = head.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    let mut parts = line.split_ascii_whitespace();
    matches!(
        (parts.next(), parts.next()),
        (Some(v), Some("200")) if v.starts_with("HTTP/1.")
    )
}

impl Prober for NetProber {
    async fn internet(&self) -> bool {
        let mut attempts = JoinSet::new();
        for addr in self.internet.iter().copied() {
            attempts.spawn(async move { TcpStream::connect(addr).await.is_ok() });
        }
        while let Some(done) = attempts.join_next().await {
            if done.unwrap_or(false) {
                return true;
            }
        }
        false
    }

    async fn cloud(&self) -> bool {
        match self.healthz().await {
            Ok(ok) => ok,
            Err(e) => {
                tracing::debug!(detail = %e, "connectivity: cloud check failed");
                false
            }
        }
    }
}

/// The probe task for one Hub process.
pub struct ConnectivityTask<P: Prober> {
    prober: Arc<P>,
    clock: Arc<dyn Clock>,
    status: watch::Sender<HubStatus>,
    interval: Duration,
    timeout: Duration,
}

impl<P: Prober> ConnectivityTask<P> {
    pub fn new(prober: P, clock: Arc<dyn Clock>, status: watch::Sender<HubStatus>) -> Self {
        Self {
            prober: Arc::new(prober),
            clock,
            status,
            interval: PROBE_INTERVAL,
            timeout: PROBE_TIMEOUT,
        }
    }

    /// Overrides the interval and the per-check timeout (tests).
    pub fn with_timings(mut self, interval: Duration, timeout: Duration) -> Self {
        self.interval = interval;
        self.timeout = timeout;
        self
    }

    /// One round: both checks at once, each bounded by the timeout (a hung check counts as
    /// failed), then the decision is published. A state change is logged once.
    pub async fn tick(&self) -> Connectivity {
        let (internet, cloud) = tokio::join!(
            timeout(self.timeout, self.prober.internet()),
            timeout(self.timeout, self.prober.cloud()),
        );
        let state = decide(internet.unwrap_or(false), cloud.unwrap_or(false));
        let now = Connectivity {
            state,
            checked_at: Some(self.clock.now_ms()),
        };
        self.status.send_if_modified(|s| {
            if s.connectivity == now {
                return false;
            }
            if s.connectivity.state != state {
                tracing::info!(
                    from = s.connectivity.state.as_str(),
                    to = state.as_str(),
                    "connectivity changed"
                );
            }
            s.connectivity = now;
            true
        });
        now
    }

    /// Probes at once, then every interval, until the status channel closes.
    pub async fn run(self) {
        let mut interval = tokio::time::interval_at(Instant::now(), self.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        while !self.status.is_closed() {
            interval.tick().await;
            self.tick().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{ManualClock, status_channel};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn decide_covers_every_pair() {
        assert_eq!(decide(true, true), ConnectivityState::Online);
        assert_eq!(decide(true, false), ConnectivityState::CloudUnreachable);
        assert_eq!(decide(false, true), ConnectivityState::InternetOffline);
        assert_eq!(decide(false, false), ConnectivityState::InternetOffline);
    }

    /// A probe whose answers can be flipped; `hang` makes both checks never finish.
    #[derive(Default)]
    struct Fake {
        internet: AtomicBool,
        cloud: AtomicBool,
        hang: AtomicBool,
    }

    impl Prober for Arc<Fake> {
        async fn internet(&self) -> bool {
            if self.hang.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            self.internet.load(Ordering::SeqCst)
        }
        async fn cloud(&self) -> bool {
            if self.hang.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            self.cloud.load(Ordering::SeqCst)
        }
    }

    fn task(
        fake: &Arc<Fake>,
    ) -> (
        ConnectivityTask<Arc<Fake>>,
        watch::Receiver<HubStatus>,
        Arc<ManualClock>,
    ) {
        let (tx, rx) = status_channel(HubStatus::opened(1, "H"));
        let clock = Arc::new(ManualClock::new(1_000));
        let t = ConnectivityTask::new(Arc::clone(fake), clock.clone(), tx);
        (t, rx, clock)
    }

    #[tokio::test]
    async fn each_round_publishes_the_decided_state() {
        let fake = Arc::new(Fake::default());
        let (t, rx, clock) = task(&fake);
        assert_eq!(rx.borrow().connectivity, Connectivity::default());
        let c = t.tick().await;
        assert_eq!(c.state, ConnectivityState::InternetOffline);
        assert_eq!(rx.borrow().connectivity.checked_at, Some(1_000));
        fake.internet.store(true, Ordering::SeqCst);
        clock.advance(30_000);
        assert_eq!(t.tick().await.state, ConnectivityState::CloudUnreachable);
        fake.cloud.store(true, Ordering::SeqCst);
        clock.advance(30_000);
        assert_eq!(t.tick().await.state, ConnectivityState::Online);
        assert_eq!(
            rx.borrow().connectivity,
            Connectivity {
                state: ConnectivityState::Online,
                checked_at: Some(61_000)
            }
        );
    }

    #[tokio::test]
    async fn the_same_answer_at_the_same_time_does_not_wake_watchers() {
        let fake = Arc::new(Fake::default());
        let (t, mut rx, _clock) = task(&fake);
        t.tick().await;
        rx.borrow_and_update();
        t.tick().await;
        assert!(!rx.has_changed().unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_probe_times_out_as_failed() {
        let fake = Arc::new(Fake::default());
        fake.internet.store(true, Ordering::SeqCst);
        fake.cloud.store(true, Ordering::SeqCst);
        fake.hang.store(true, Ordering::SeqCst);
        let (t, rx, _clock) = task(&fake);
        let started = Instant::now();
        let c = t.tick().await;
        assert_eq!(c.state, ConnectivityState::InternetOffline);
        let took = started.elapsed();
        assert!(
            took >= PROBE_TIMEOUT && took < PROBE_TIMEOUT + Duration::from_millis(100),
            "{took:?}"
        );
        assert_eq!(rx.borrow().connectivity.state, c.state);
    }

    #[tokio::test(start_paused = true)]
    async fn run_probes_at_start_and_every_interval() {
        let fake = Arc::new(Fake::default());
        let (t, mut rx, _clock) = task(&fake);
        let job = tokio::spawn(t.run());
        rx.changed().await.unwrap();
        assert_eq!(
            rx.borrow_and_update().connectivity.state,
            ConnectivityState::InternetOffline
        );
        fake.internet.store(true, Ordering::SeqCst);
        fake.cloud.store(true, Ordering::SeqCst);
        let before = Instant::now();
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().connectivity.state, ConnectivityState::Online);
        assert!(before.elapsed() <= PROBE_INTERVAL, "{:?}", before.elapsed());
        job.abort();
    }

    #[tokio::test]
    async fn an_intent_applies_while_the_probe_hangs() {
        use arena_proto::{Actor, ActorKind, BranchBootstrap, Intent, IntentKind, IntentPayload};
        use arena_store::Store;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, 1_000).unwrap();
        let (tx, _rx) = status_channel(HubStatus::opened(1, "01JAAAAAAAAAAAAAAAAAAAAAAA"));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_000));
        let lease = crate::LeaseTask::start(
            store,
            "01JAAAAAAAAAAAAAAAAAAAAAAA".into(),
            Arc::clone(&clock),
            tx.clone(),
        )
        .await
        .unwrap();
        let writer = lease.writer().unwrap();
        let fake = Arc::new(Fake::default());
        fake.hang.store(true, Ordering::SeqCst);
        let probe = tokio::spawn(
            ConnectivityTask::new(Arc::clone(&fake), Arc::clone(&clock), tx.clone()).run(),
        );
        // Let the probe start its (hanging) first round.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let started = std::time::Instant::now();
        let applied = writer
            .submit_intent(Intent {
                intent_id: "01JAAAAAAAAAAAAAAAAAAAAAAB".into(),
                kind: IntentKind::BranchBootstrap,
                idempotency_key: "01JAAAAAAAAAAAAAAAAAAAAAAC".into(),
                issued_at: 1_000,
                base_rev: None,
                actor: Actor {
                    kind: ActorKind::System,
                    id: "test".into(),
                },
                elevated_by: None,
                reason: None,
                payload: IntentPayload::BranchBootstrap(BranchBootstrap {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    currency: "INR".into(),
                    tz: "UTC".into(),
                    day_start_min: 0,
                    fy_start_month: 1,
                }),
            })
            .await;
        assert!(applied.is_ok(), "{applied:?}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        // The probe is still inside its first round.
        assert_eq!(tx.borrow().connectivity.checked_at, None);
        probe.abort();
        writer.close().await;
    }

    mod net {
        use super::*;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio::net::TcpListener;

        #[test]
        fn cloud_hosts_and_endpoints_parse() {
            assert_eq!(
                CloudHost::parse("api.arena360.cloud:443").unwrap(),
                CloudHost {
                    host: "api.arena360.cloud".into(),
                    port: 443
                }
            );
            assert_eq!(CloudHost::parse("[::1]:8443").unwrap().host, "::1");
            for bad in ["api.arena360.cloud", "h:0", ":443", "h:x", "a b:1", "h/x:1"] {
                assert!(CloudHost::parse(bad).is_err(), "{bad}");
            }
            assert_eq!(
                parse_internet_endpoints("1.1.1.1:443, 8.8.8.8:53").unwrap(),
                vec![
                    "1.1.1.1:443".parse::<SocketAddr>().unwrap(),
                    "8.8.8.8:53".parse().unwrap()
                ]
            );
            for bad in ["", ",", "1.1.1.1", "one.one:443"] {
                assert!(parse_internet_endpoints(bad).is_err(), "{bad}");
            }
            assert_eq!(default_internet_endpoints().len(), 2);
            CloudHost::parse(DEFAULT_CLOUD_HOST).unwrap();
        }

        #[test]
        fn only_a_200_status_line_counts() {
            assert!(status_is_200(b"HTTP/1.1 200 OK\r\n"));
            assert!(status_is_200(b"HTTP/1.0 200\r\n"));
            for bad in [
                &b"HTTP/1.1 503 Service Unavailable\r\n"[..],
                b"HTTP/1.1 2000\r\n",
                b"SSH-2.0 200\r\n",
                b"",
            ] {
                assert!(!status_is_200(bad), "{}", String::from_utf8_lossy(bad));
            }
        }

        /// A CA and a TLS server for `localhost` signed by it, answering every request with
        /// `status`.
        async fn cloud(status: &'static str) -> (u16, rustls::RootCertStore) {
            let ca_key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
            let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
            ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            let ca = ca_params.self_signed(&ca_key).unwrap();
            let issuer = rcgen::Issuer::new(ca_params, ca_key);
            let leaf_key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
            let leaf = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
                .unwrap()
                .signed_by(&leaf_key, &issuer)
                .unwrap();
            let mut roots = rustls::RootCertStore::empty();
            roots.add(CertificateDer::from(ca.der().to_vec())).unwrap();
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(leaf.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
            )
            .unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(async move {
                while let Ok((tcp, _)) = listener.accept().await {
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        let Ok(mut tls) = acceptor.accept(tcp).await else {
                            return;
                        };
                        let mut buf = vec![0u8; 1024];
                        let n = tls.read(&mut buf).await.unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                        let status = if req.starts_with("GET /healthz HTTP/1.1\r\n") {
                            status
                        } else {
                            "400 Bad Request"
                        };
                        let res = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n");
                        let _ = tls.write_all(res.as_bytes()).await;
                        let _ = tls.shutdown().await;
                    });
                }
            });
            (port, roots)
        }

        fn localhost(port: u16) -> CloudHost {
            CloudHost {
                host: "localhost".into(),
                port,
            }
        }

        async fn closed_port() -> SocketAddr {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap()
        }

        #[tokio::test]
        async fn cloud_is_up_only_on_a_trusted_200() {
            let (port, roots) = cloud("200 OK").await;
            let p = NetProber::with_roots(vec![], localhost(port), roots.clone()).unwrap();
            assert!(p.cloud().await);

            let (port, roots) = cloud("503 Service Unavailable").await;
            let p = NetProber::with_roots(vec![], localhost(port), roots.clone()).unwrap();
            assert!(!p.cloud().await);

            // The real roots do not trust the test CA.
            let (port, _) = cloud("200 OK").await;
            let p = NetProber::new(vec![], localhost(port)).unwrap();
            assert!(!p.cloud().await);

            let p = NetProber::with_roots(vec![], localhost(closed_port().await.port()), roots)
                .unwrap();
            assert!(!p.cloud().await);
        }

        #[tokio::test]
        async fn internet_is_up_when_any_endpoint_accepts() {
            let open = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let up = open.local_addr().unwrap();
            let down = closed_port().await;
            let cloud = localhost(1);
            let p = NetProber::new(vec![down, up], cloud.clone()).unwrap();
            assert!(p.internet().await);
            let p = NetProber::new(vec![down], cloud.clone()).unwrap();
            assert!(!p.internet().await);
            let p = NetProber::new(vec![], cloud).unwrap();
            assert!(!p.internet().await);
        }
    }
}

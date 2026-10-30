use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{
    AppError, DiscoveredHub, ErrorCode, HUB_PRODUCT, HubInfo, MDNS_SERVICE, PROTOCOL_VERSION,
    SelectedHub, is_fingerprint,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};

use crate::paths::DataDir;

pub const DISCOVERY_WINDOW: Duration = Duration::from_millis(arena_proto::DISCOVERY_WINDOW_MS);
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(arena_proto::PROBE_TIMEOUT_MS);
pub const HUB_WEB_PORT: u16 = arena_proto::DEFAULT_WEB_PORT;
const MAX_INFO_BYTES: usize = 16 * 1024;
/// What a remote branch is called where a Hub name goes.
pub const REMOTE_HUB_NAME: &str = "Remote";

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowsedService {
    pub addrs: Vec<IpAddr>,
    pub port: u16,
    pub txt: HashMap<String, String>,
}

pub struct Browse {
    pub rx: mpsc::Receiver<BrowsedService>,
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl Browse {
    pub fn new(rx: mpsc::Receiver<BrowsedService>, stop: impl FnOnce() + Send + 'static) -> Self {
        Self {
            rx,
            stop: Some(Box::new(stop)),
        }
    }
}

impl Drop for Browse {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

pub trait Browser: Send + Sync + 'static {
    fn browse(&self) -> Result<Browse, String>;
}

pub trait Prober: Send + Sync + 'static {
    fn hub_info(&self, addr: SocketAddr) -> BoxFuture<'_, Result<HubInfo, String>>;
    fn fingerprint(&self, addr: SocketAddr) -> BoxFuture<'_, Result<String, String>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MdnsBrowser;

impl Browser for MdnsBrowser {
    fn browse(&self) -> Result<Browse, String> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
        let events = daemon.browse(MDNS_SERVICE).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel(64);
        let pump = tokio::spawn(async move {
            while let Ok(ev) = events.recv_async().await {
                if let mdns_sd::ServiceEvent::ServiceResolved(svc) = ev
                    && tx.send(browsed_from(&svc)).await.is_err()
                {
                    break;
                }
            }
        });
        Ok(Browse::new(rx, move || {
            pump.abort();
            let _ = daemon.stop_browse(MDNS_SERVICE);
            let _ = daemon.shutdown();
        }))
    }
}

pub fn browsed_from(svc: &mdns_sd::ResolvedService) -> BrowsedService {
    let mut addrs: Vec<IpAddr> = svc.addresses.iter().map(|a| a.to_ip_addr()).collect();
    addrs.sort_by_key(|a| (a.is_ipv6(), *a));
    BrowsedService {
        addrs,
        port: svc.port,
        txt: svc
            .txt_properties
            .iter()
            .map(|p| (p.key().to_owned(), p.val_str().to_owned()))
            .collect(),
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NetProber;

impl Prober for NetProber {
    fn hub_info(&self, addr: SocketAddr) -> BoxFuture<'_, Result<HubInfo, String>> {
        Box::pin(get_hub_info(addr))
    }

    fn fingerprint(&self, addr: SocketAddr) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            arena_kiosk_core::capture_fingerprint(
                &addr.ip().to_string(),
                addr.port(),
                PROBE_TIMEOUT,
            )
            .await
            .map_err(|e| e.to_string())
        })
    }
}

async fn get_hub_info(addr: SocketAddr) -> Result<HubInfo, String> {
    let mut tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    let request = format!(
        "GET /hub-info HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        host_of(addr.ip())
    );
    tcp.write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = tcp.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_INFO_BYTES {
            return Err("the answer is too long".into());
        }
    }
    let text = String::from_utf8(buf).map_err(|e| e.to_string())?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "not an http answer".to_owned())?;
    let status = head.lines().next().unwrap_or_default();
    if status.split(' ').nth(1) != Some("200") {
        return Err(format!("status {status}"));
    }
    serde_json::from_str(body).map_err(|e| e.to_string())
}

fn host_of(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

pub fn hub_addr(ip: IpAddr, port: u16) -> String {
    format!("{}:{port}", host_of(ip))
}

pub fn parse_ip(text: &str) -> Option<IpAddr> {
    let t = text.trim();
    let t = t
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(t);
    let ip: IpAddr = t.parse().ok()?;
    let unusable = ip.is_unspecified()
        || ip.is_multicast()
        || matches!(ip, IpAddr::V4(v4) if v4.is_broadcast())
        || matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local());
    (!unusable).then_some(ip)
}

fn is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unicast_link_local(),
    }
}

pub fn local_ipv4_nets() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) if !v4.ip.is_loopback() => Some((v4.ip, v4.netmask)),
            _ => None,
        })
        .collect()
}

fn same_subnet(ip: Ipv4Addr, (net_ip, mask): (Ipv4Addr, Ipv4Addr)) -> bool {
    let m = u32::from(mask);
    m != 0 && u32::from(ip) & m == u32::from(net_ip) & m
}

pub fn choose_addr(addrs: &[IpAddr], nets: &[(Ipv4Addr, Ipv4Addr)]) -> Option<IpAddr> {
    let usable: Vec<IpAddr> = addrs
        .iter()
        .copied()
        .filter(|a| !a.is_loopback() && !is_link_local(a) && !a.is_unspecified())
        .collect();
    if usable.is_empty() {
        return addrs.iter().copied().find(IpAddr::is_loopback);
    }
    usable
        .iter()
        .copied()
        .find(|a| matches!(a, IpAddr::V4(v4) if nets.iter().any(|n| same_subnet(*v4, *n))))
        .or_else(|| usable.first().copied())
}

fn no_hub() -> AppError {
    AppError::new(ErrorCode::NoHub, "no hub at that address")
}

fn to_hub(svc: &BrowsedService, nets: &[(Ipv4Addr, Ipv4Addr)]) -> Option<DiscoveredHub> {
    let ip = choose_addr(&svc.addrs, nets)?;
    DiscoveredHub::from_txt(hub_addr(ip, svc.port), |k| svc.txt.get(k).cloned())
}

pub async fn discover(
    browser: &dyn Browser,
    window: Duration,
    on_found: impl FnMut(&[DiscoveredHub]),
) -> Vec<DiscoveredHub> {
    discover_on(browser, window, &local_ipv4_nets(), on_found).await
}

pub async fn discover_on(
    browser: &dyn Browser,
    window: Duration,
    nets: &[(Ipv4Addr, Ipv4Addr)],
    mut on_found: impl FnMut(&[DiscoveredHub]),
) -> Vec<DiscoveredHub> {
    let mut found: Vec<DiscoveredHub> = Vec::new();
    let mut browse = match browser.browse() {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "mdns browse failed; offering the typed ip");
            return found;
        }
    };
    let deadline = Instant::now() + window;
    loop {
        let next = tokio::select! {
            () = tokio::time::sleep_until(deadline) => break,
            next = browse.rx.recv() => next,
        };
        let Some(svc) = next else {
            tokio::time::sleep_until(deadline).await;
            break;
        };
        let Some(hub) = to_hub(&svc, nets) else {
            continue;
        };
        match found.iter_mut().find(|h| h.fingerprint == hub.fingerprint) {
            Some(same) if *same == hub => continue,
            Some(same) => *same = hub,
            None => found.push(hub),
        }
        on_found(&found);
    }
    found
}

/// A branch reached through its tunnel hostname (Story 8.41). The Hub is not probed: it answers
/// nothing on the tunnel but a pairing code and a device token, so pairing is the proof. There is
/// no fingerprint to pin (Cloudflare terminates the TLS).
fn remote_hub(host: &str) -> DiscoveredHub {
    DiscoveredHub {
        addr: format!("{host}:{}", arena_kiosk_core::REMOTE_PORT),
        branch_name: host.split('.').next().unwrap_or(host).to_owned(),
        hub_name: REMOTE_HUB_NAME.to_owned(),
        fingerprint: String::new(),
        protocol_version: PROTOCOL_VERSION,
        compatible: true,
        web_port: arena_kiosk_core::REMOTE_PORT,
    }
}

/// Whether `addr` (`host:port`) is a branch tunnel address.
pub fn is_remote_addr(addr: &str) -> bool {
    addr.rsplit_once(':')
        .is_some_and(|(host, _)| arena_proto::parse_tunnel_host(host).is_some())
}

pub async fn probe_ip(
    prober: &dyn Prober,
    text: &str,
    web_port: u16,
) -> Result<DiscoveredHub, AppError> {
    if let Some(host) = arena_proto::parse_tunnel_host(text) {
        return Ok(remote_hub(&host));
    }
    let ip = parse_ip(text).ok_or_else(no_hub)?;
    let web = SocketAddr::new(ip, web_port);
    let info = match timeout(PROBE_TIMEOUT, prober.hub_info(web)).await {
        Ok(Ok(info)) => info,
        Ok(Err(e)) => {
            tracing::info!(error = %e, "probe: no hub-info");
            return Err(no_hub());
        }
        Err(_) => {
            tracing::info!("probe: hub-info timed out");
            return Err(no_hub());
        }
    };
    let mut info = info;
    info.fingerprint = info.fingerprint.trim().to_ascii_lowercase();
    if info.product != HUB_PRODUCT || !is_fingerprint(&info.fingerprint) {
        tracing::info!("probe: the answer is not an arena 360 hub");
        return Err(no_hub());
    }
    let presented = match timeout(PROBE_TIMEOUT, prober.fingerprint(web)).await {
        Ok(Ok(fp)) => fp,
        Ok(Err(e)) => {
            tracing::info!(error = %e, "probe: tls handshake failed");
            return Err(no_hub());
        }
        Err(_) => return Err(no_hub()),
    };
    if presented.to_ascii_lowercase() != info.fingerprint {
        tracing::warn!(
            claimed = %info.fingerprint,
            presented = %presented,
            "probe: the hub-info fingerprint does not match the certificate; refused"
        );
        return Err(no_hub());
    }
    if info.protocol_version != PROTOCOL_VERSION {
        return Err(AppError::new(
            ErrorCode::HubIncompatible,
            "the hub speaks another protocol version",
        ));
    }
    Ok(DiscoveredHub::from_info(
        hub_addr(ip, info.lan_port),
        &info,
        web_port,
    ))
}

pub struct Discovery {
    pub browser: Arc<dyn Browser>,
    pub prober: Arc<dyn Prober>,
    pub window: Duration,
    pub web_port: u16,
    known: Mutex<HashMap<(String, String), DiscoveredHub>>,
    running: Mutex<Option<tokio::task::JoinHandle<()>>>,
    search: AtomicU64,
}

impl Discovery {
    pub fn new(browser: Arc<dyn Browser>, prober: Arc<dyn Prober>) -> Self {
        Self {
            browser,
            prober,
            window: DISCOVERY_WINDOW,
            web_port: HUB_WEB_PORT,
            known: Mutex::default(),
            running: Mutex::default(),
            search: AtomicU64::new(0),
        }
    }

    pub fn system() -> Self {
        Self::new(Arc::new(MdnsBrowser), Arc::new(NetProber))
    }

    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    pub fn with_web_port(mut self, port: u16) -> Self {
        self.web_port = port;
        self
    }

    pub fn remember(&self, hubs: &[DiscoveredHub]) {
        let mut known = self.known.lock().unwrap_or_else(|p| p.into_inner());
        for h in hubs {
            known.insert(
                (h.fingerprint.to_ascii_lowercase(), h.addr.clone()),
                h.clone(),
            );
        }
    }

    pub fn known(&self, fingerprint: &str, addr: &str) -> Option<DiscoveredHub> {
        self.known
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(fingerprint.to_ascii_lowercase(), addr.to_owned()))
            .cloned()
    }

    pub fn next_search(&self) -> u64 {
        self.search.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current_search(&self) -> u64 {
        self.search.load(Ordering::SeqCst)
    }

    pub fn replace_running(&self, job: tokio::task::JoinHandle<()>) {
        let old = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .replace(job);
        if let Some(old) = old {
            old.abort();
        }
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        if let Some(job) = self
            .running
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            job.abort();
        }
    }
}

fn ip_of(addr: &str) -> Option<IpAddr> {
    let (host, _) = addr.rsplit_once(':')?;
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

pub async fn select_hub(
    discovery: &Discovery,
    data: &DataDir,
    addr: &str,
    fingerprint: &str,
) -> Result<SelectedHub, AppError> {
    let hub = discovery
        .known(fingerprint, addr)
        .filter(|h| h.compatible)
        .ok_or_else(no_hub)?;
    // A tunnel address has no certificate to check; pairing is the proof it is a Hub.
    if !is_remote_addr(&hub.addr) {
        let ip = ip_of(&hub.addr).ok_or_else(no_hub)?;
        let web = SocketAddr::new(ip, hub.web_port);
        let presented = match timeout(PROBE_TIMEOUT, discovery.prober.fingerprint(web)).await {
            Ok(Ok(fp)) => fp.to_ascii_lowercase(),
            Ok(Err(e)) => {
                tracing::info!(error = %e, "select: tls handshake failed");
                return Err(no_hub());
            }
            Err(_) => return Err(no_hub()),
        };
        if presented != hub.fingerprint {
            tracing::warn!(
                advertised = %hub.fingerprint,
                presented = %presented,
                "select: the hub certificate does not match its advert; refused"
            );
            return Err(no_hub());
        }
    }
    let selected = SelectedHub {
        addr: hub.addr,
        fingerprint: hub.fingerprint,
        branch_name: hub.branch_name,
        web_port: hub.web_port,
    };
    let text = serde_json::to_vec_pretty(&selected)
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
    std::fs::create_dir_all(data.root())
        .and_then(|()| crate::config::write_atomic(&data.selected_hub(), &text))
        .map_err(|e| {
            tracing::error!(error = %e, "selected_hub.json could not be written");
            AppError::new(ErrorCode::Internal, "the hub choice could not be saved")
        })?;
    tracing::info!(hub = %selected.addr, "hub selected for pairing");
    Ok(selected)
}

pub fn clear_selected(data: &DataDir) {
    match std::fs::remove_file(data.selected_hub()) {
        Ok(()) => tracing::info!("hub choice cleared"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(error = %e, "selected_hub.json could not be removed"),
    }
}

pub fn load_selected(data: &DataDir) -> Option<SelectedHub> {
    let text = std::fs::read(data.selected_hub()).ok()?;
    serde_json::from_slice(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn txt(branch: &str, hub: &str, proto: u32, fp: &str) -> HashMap<String, String> {
        arena_proto::txt_records(branch, hub, proto, fp, 7401)
            .into_iter()
            .collect()
    }

    struct FakeBrowser {
        script: Mutex<Vec<(Duration, BrowsedService)>>,
        stopped: Arc<AtomicUsize>,
    }

    impl Browser for FakeBrowser {
        fn browse(&self) -> Result<Browse, String> {
            let (tx, rx) = mpsc::channel(16);
            let script = std::mem::take(&mut *self.script.lock().unwrap());
            let job = tokio::spawn(async move {
                for (after, svc) in script {
                    tokio::time::sleep(after).await;
                    if tx.send(svc).await.is_err() {
                        return;
                    }
                }
                std::future::pending::<()>().await;
            });
            let stopped = Arc::clone(&self.stopped);
            Ok(Browse::new(rx, move || {
                job.abort();
                stopped.fetch_add(1, Ordering::SeqCst);
            }))
        }
    }

    fn svc(ip: [u8; 4], t: HashMap<String, String>) -> BrowsedService {
        BrowsedService {
            addrs: vec![IpAddr::from(ip)],
            port: 7400,
            txt: t,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn discovery_dedupes_by_fingerprint_keeps_the_newest_and_ends_after_the_window() {
        let a = "aa".repeat(32);
        let b = "bb".repeat(32);
        let browser = FakeBrowser {
            script: Mutex::new(vec![
                (
                    Duration::from_secs(1),
                    svc([192, 168, 1, 10], txt("Koramangala", "desk", 1, &a)),
                ),
                (
                    Duration::from_secs(1),
                    svc([192, 168, 1, 10], txt("Koramangala", "desk", 1, &a)),
                ),
                (
                    Duration::from_secs(1),
                    svc([192, 168, 1, 20], txt("Koramangala East", "desk", 1, &a)),
                ),
                (
                    Duration::from_secs(1),
                    svc([192, 168, 1, 11], txt("Old", "back", 99, &b)),
                ),
                (
                    Duration::from_secs(1),
                    svc([192, 168, 1, 12], txt("Bad", "x", 1, "nope")),
                ),
                (
                    Duration::from_secs(20),
                    svc([192, 168, 1, 13], txt("Late", "y", 1, &"cc".repeat(32))),
                ),
            ]),
            stopped: Arc::default(),
        };
        let started = Instant::now();
        let mut pushes = Vec::new();
        let hubs = discover_on(&browser, DISCOVERY_WINDOW, &[], |h| pushes.push(h.len())).await;
        assert_eq!(started.elapsed(), DISCOVERY_WINDOW);
        assert_eq!(pushes, [1, 1, 2]);
        assert_eq!(hubs.len(), 2);
        assert_eq!(hubs[0].addr, "192.168.1.20:7400");
        assert_eq!(hubs[0].branch_name, "Koramangala East");
        assert!(hubs[0].compatible);
        assert_eq!(hubs[1].fingerprint, b);
        assert!(!hubs[1].compatible);
        assert_eq!(browser.stopped.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_found_ends_empty_after_ten_seconds() {
        let browser = FakeBrowser {
            script: Mutex::new(vec![]),
            stopped: Arc::default(),
        };
        let started = Instant::now();
        let hubs = discover_on(&browser, DISCOVERY_WINDOW, &[], |_| panic!("nothing found")).await;
        assert!(hubs.is_empty());
        assert_eq!(started.elapsed(), Duration::from_secs(10));
    }

    #[test]
    fn the_address_skips_link_local_and_prefers_the_kiosk_subnet() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let nets = [(
            Ipv4Addr::new(192, 168, 50, 7),
            Ipv4Addr::new(255, 255, 255, 0),
        )];
        let addrs = [
            ip("10.0.0.5"),
            ip("169.254.3.4"),
            ip("192.168.50.218"),
            ip("fe80::1"),
            ip("127.0.0.1"),
        ];
        assert_eq!(choose_addr(&addrs, &nets), Some(ip("192.168.50.218")));
        assert_eq!(choose_addr(&addrs, &[]), Some(ip("10.0.0.5")));
        assert_eq!(
            choose_addr(&[ip("169.254.3.4"), ip("fd00::2")], &nets),
            Some(ip("fd00::2"))
        );
        assert_eq!(
            choose_addr(&[ip("127.0.0.1")], &nets),
            Some(ip("127.0.0.1"))
        );
        assert_eq!(
            choose_addr(&[ip("169.254.3.4"), ip("fe80::1")], &nets),
            None
        );
        assert_eq!(choose_addr(&[], &nets), None);
    }

    #[test]
    fn a_resolved_service_converts_with_addresses_port_and_every_txt_key() {
        let props: HashMap<String, String> = txt("Koramangala", "desk", 1, &"ab".repeat(32));
        let info = mdns_sd::ServiceInfo::new(
            arena_proto::MDNS_SERVICE,
            "desk",
            "desk-abababab.local.",
            "192.168.1.10,10.0.0.2",
            7400,
            props.clone(),
        )
        .unwrap();
        let browsed = browsed_from(&info.as_resolved_service());
        assert_eq!(
            browsed.addrs,
            [IpAddr::from([10, 0, 0, 2]), IpAddr::from([192, 168, 1, 10])]
        );
        assert_eq!(browsed.port, 7400);
        assert_eq!(browsed.txt, props);
        for key in ["branch", "hub", "proto", "fp", "web"] {
            assert!(browsed.txt.contains_key(key), "{key}");
        }
    }

    #[test]
    fn typed_ips_are_literals_only() {
        assert_eq!(
            parse_ip(" 192.168.1.10 "),
            Some(IpAddr::from([192, 168, 1, 10]))
        );
        assert!(parse_ip("fd00::1").is_some());
        assert!(parse_ip("[fd00::1]").is_some());
        for bad in [
            "999.1.1.1",
            "hub.local",
            "",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.251",
            "1.2.3",
            "fe80::1",
            "[fe80::1]",
            "febf::1",
        ] {
            assert_eq!(parse_ip(bad), None, "{bad}");
        }
        assert_eq!(hub_addr("fd00::1".parse().unwrap(), 7400), "[fd00::1]:7400");
    }

    struct NeverProber;

    impl Prober for NeverProber {
        fn hub_info(&self, _: SocketAddr) -> BoxFuture<'_, Result<HubInfo, String>> {
            panic!("no network for bad input")
        }
        fn fingerprint(&self, _: SocketAddr) -> BoxFuture<'_, Result<String, String>> {
            panic!("no network for bad input")
        }
    }

    #[tokio::test]
    async fn bad_input_is_no_hub_without_the_network() {
        for bad in ["999.1.1.1", "my hub", "", "fe80::1"] {
            let err = probe_ip(&NeverProber, bad, HUB_WEB_PORT).await.unwrap_err();
            assert_eq!(err.code, ErrorCode::NoHub);
        }
    }

    #[tokio::test]
    async fn a_tunnel_hostname_is_a_remote_hub_found_and_selected_without_the_network() {
        let hub = probe_ip(
            &NeverProber,
            "01JBRANCH.tunnel.arena360.cloud",
            HUB_WEB_PORT,
        )
        .await
        .unwrap();
        assert_eq!(hub.addr, "01jbranch.tunnel.arena360.cloud:443");
        assert_eq!(hub.web_port, 443);
        assert!(hub.compatible && hub.fingerprint.is_empty());
        assert!(is_remote_addr(&hub.addr));
        assert!(!is_remote_addr("10.0.0.2:7400"));
        assert!(!is_remote_addr("evil.example.com:443"));
        // Any other hostname is still not a Hub.
        let err = probe_ip(&NeverProber, "hub.example.com", HUB_WEB_PORT)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NoHub);

        // Selecting it checks no certificate (there is none to pin) and records it as chosen.
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        let d = Discovery::new(Arc::new(MdnsBrowser), Arc::new(NeverProber));
        d.remember(std::slice::from_ref(&hub));
        let selected = select_hub(&d, &data, &hub.addr, &hub.fingerprint)
            .await
            .unwrap();
        assert_eq!(selected.addr, hub.addr);
        assert_eq!(selected.web_port, 443);
        assert_eq!(load_selected(&data), Some(selected));
    }

    struct InfoProber(HubInfo, String);

    impl Prober for InfoProber {
        fn hub_info(&self, _: SocketAddr) -> BoxFuture<'_, Result<HubInfo, String>> {
            let info = self.0.clone();
            Box::pin(async move { Ok(info) })
        }
        fn fingerprint(&self, _: SocketAddr) -> BoxFuture<'_, Result<String, String>> {
            let fp = self.1.clone();
            Box::pin(async move { Ok(fp) })
        }
    }

    #[tokio::test]
    async fn a_probed_hub_has_bounded_names_and_an_upper_case_fingerprint_is_accepted() {
        let fp = "ab".repeat(32);
        let info = HubInfo::new(
            "b".repeat(300),
            "h\u{1}".repeat(80),
            fp.to_uppercase(),
            7400,
            7401,
        );
        let hub = probe_ip(&InfoProber(info, fp.to_uppercase()), "10.0.0.2", 7401)
            .await
            .unwrap();
        assert_eq!(hub.fingerprint, fp);
        assert_eq!(hub.branch_name.len(), arena_proto::MAX_BRANCH_NAME_BYTES);
        assert_eq!(hub.hub_name, "h".repeat(63));
        assert_eq!(hub.web_port, 7401);
    }

    struct CertProber(HashMap<SocketAddr, String>);

    impl Prober for CertProber {
        fn hub_info(&self, _: SocketAddr) -> BoxFuture<'_, Result<HubInfo, String>> {
            Box::pin(async { Err("unused".to_owned()) })
        }
        fn fingerprint(&self, addr: SocketAddr) -> BoxFuture<'_, Result<String, String>> {
            let fp = self
                .0
                .get(&addr)
                .cloned()
                .ok_or_else(|| "refused".to_owned());
            Box::pin(async move { fp })
        }
    }

    fn found(addr: &str, fp: &str, compatible: bool) -> DiscoveredHub {
        DiscoveredHub {
            addr: addr.into(),
            branch_name: "Koramangala".into(),
            hub_name: "desk".into(),
            fingerprint: fp.into(),
            protocol_version: PROTOCOL_VERSION,
            compatible,
            web_port: 7401,
        }
    }

    #[tokio::test]
    async fn select_checks_the_certificate_and_writes_only_selected_hub_json() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir(dir.path().join("kiosk"));
        let fp = "ab".repeat(32);
        let spoof = "ee".repeat(32);
        let certs = HashMap::from([
            ("10.0.0.2:7401".parse().unwrap(), fp.to_uppercase()),
            ("10.0.0.4:7401".parse().unwrap(), "99".repeat(32)),
        ]);
        let d = Discovery::new(Arc::new(MdnsBrowser), Arc::new(CertProber(certs)));
        assert_eq!(
            select_hub(&d, &data, "10.0.0.2:7400", &fp)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NoHub
        );
        d.remember(&[
            found("127.0.0.1:7400", &fp, true),
            found("10.0.0.2:7400", &fp, true),
            found("10.0.0.3:7400", &"cd".repeat(32), false),
            found("10.0.0.4:7400", &spoof, true),
        ]);
        for (addr, fp, why) in [
            (
                "10.0.0.9:7400",
                fp.as_str(),
                "the address must be the one found",
            ),
            (
                "10.0.0.3:7400",
                "cdcd",
                "an incompatible hub is never selected",
            ),
            (
                "10.0.0.4:7400",
                spoof.as_str(),
                "the certificate must match the advert",
            ),
            (
                "127.0.0.1:7400",
                fp.as_str(),
                "nothing answering is refused",
            ),
        ] {
            let fp = if fp == "cdcd" {
                "cd".repeat(32)
            } else {
                fp.to_owned()
            };
            assert_eq!(
                select_hub(&d, &data, addr, &fp).await.unwrap_err().code,
                ErrorCode::NoHub,
                "{why}"
            );
        }
        assert!(!data.selected_hub().exists());
        let chosen = select_hub(&d, &data, "10.0.0.2:7400", &fp.to_uppercase())
            .await
            .unwrap();
        assert_eq!(
            chosen,
            SelectedHub {
                addr: "10.0.0.2:7400".into(),
                fingerprint: fp.clone(),
                branch_name: "Koramangala".into(),
                web_port: 7401,
            }
        );
        assert_eq!(load_selected(&data), Some(chosen));
        let mut names: Vec<String> = std::fs::read_dir(data.root())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["selected_hub.json"]);
        assert!(!data.config().exists() && !data.token().exists());
        clear_selected(&data);
        assert_eq!(load_selected(&data), None);
        clear_selected(&data);
    }
}

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use arena_proto::{DEFAULT_PRINTER_PORT, FoundPrinter, PrinterKind};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout, timeout_at};

use super::PrinterError;

pub const SCAN_CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
pub const SCAN_MAX_CONCURRENT: usize = 64;
pub const SCAN_DEADLINE: Duration = Duration::from_secs(8);
pub const SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet {
    base: Ipv4Addr,
    prefix: u8,
}

impl Subnet {
    pub fn of_24(ip: Ipv4Addr) -> Self {
        let [a, b, c, _] = ip.octets();
        Self {
            base: Ipv4Addr::new(a, b, c, 0),
            prefix: 24,
        }
    }

    #[cfg(test)]
    fn single(ip: Ipv4Addr) -> Self {
        Self {
            base: ip,
            prefix: 32,
        }
    }

    pub fn hosts(self) -> Vec<Ipv4Addr> {
        if self.prefix == 32 {
            return vec![self.base];
        }
        let [a, b, c, _] = self.base.octets();
        (1..=254).map(|d| Ipv4Addr::new(a, b, c, d)).collect()
    }
}

fn scannable(interfaces: impl IntoIterator<Item = (bool, std::net::IpAddr)>) -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = interfaces
        .into_iter()
        .filter(|(loopback, _)| !loopback)
        .filter_map(|(_, ip)| match ip {
            std::net::IpAddr::V4(v4)
                if !v4.is_loopback() && !v4.is_unspecified() && !v4.is_link_local() =>
            {
                Some(v4)
            }
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn own_addresses() -> Vec<Ipv4Addr> {
    scannable(
        if_addrs::get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .map(|i| (i.is_loopback(), i.ip())),
    )
}

async fn tcp_open(addr: SocketAddrV4, connect_timeout: Duration) -> bool {
    matches!(
        timeout(connect_timeout, TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

fn scan_hosts(subnets: &[Subnet], own: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    let mut hosts: Vec<Ipv4Addr> = subnets
        .iter()
        .flat_map(|s| s.hosts())
        .filter(|h| !own.contains(h))
        .collect();
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

#[derive(Debug, Clone)]
pub struct LanScanner {
    port: u16,
    connect_timeout: Duration,
    deadline: Duration,
    #[cfg(test)]
    subnets: Option<Vec<Subnet>>,
}

impl Default for LanScanner {
    fn default() -> Self {
        Self {
            port: DEFAULT_PRINTER_PORT,
            connect_timeout: SCAN_CONNECT_TIMEOUT,
            deadline: SCAN_DEADLINE,
            #[cfg(test)]
            subnets: None,
        }
    }
}

impl LanScanner {
    pub fn new() -> Self {
        Self::default()
    }

    fn targets(&self) -> Vec<Ipv4Addr> {
        #[cfg(test)]
        if let Some(subnets) = &self.subnets {
            return scan_hosts(subnets, &[]);
        }
        let own = own_addresses();
        let subnets: Vec<Subnet> = own.iter().map(|ip| Subnet::of_24(*ip)).collect();
        scan_hosts(&subnets, &own)
    }

    pub async fn scan(&self) -> Vec<FoundPrinter> {
        let hosts = self.targets();
        let wait = self.connect_timeout;
        let found = probe_all(hosts, self.port, self.deadline, move |addr| {
            tcp_open(addr, wait)
        })
        .await;
        found
            .into_iter()
            .map(|addr| FoundPrinter {
                id: format!("lan:{addr}"),
                kind: PrinterKind::Lan,
                name: "Network printer".to_owned(),
                addr: Some(addr.to_string()),
            })
            .collect()
    }
}

async fn probe_all<F, Fut>(
    hosts: Vec<Ipv4Addr>,
    port: u16,
    deadline: Duration,
    probe: F,
) -> Vec<SocketAddrV4>
where
    F: Fn(SocketAddrV4) -> Fut,
    Fut: std::future::Future<Output = bool> + Send + 'static,
{
    let until = Instant::now() + deadline;
    let permits = Arc::new(tokio::sync::Semaphore::new(SCAN_MAX_CONCURRENT));
    let mut set = JoinSet::new();
    for ip in hosts {
        let permits = Arc::clone(&permits);
        let addr = SocketAddrV4::new(ip, port);
        let check = probe(addr);
        set.spawn(async move {
            let _permit = permits.acquire_owned().await.ok()?;
            check.await.then_some(addr)
        });
    }
    let mut found = Vec::new();
    loop {
        match timeout_at(until, set.join_next()).await {
            Ok(Some(Ok(Some(addr)))) => found.push(addr),
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                tracing::info!("printer scan reached its deadline");
                set.abort_all();
                break;
            }
        }
    }
    found.sort_unstable();
    found
}

pub async fn send_lan(
    addr: SocketAddrV4,
    bytes: &[u8],
    limit: Duration,
) -> Result<(), PrinterError> {
    let classify = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::ConnectionRefused => PrinterError::Refused(addr),
        std::io::ErrorKind::TimedOut => PrinterError::Timeout(addr, limit),
        _ => PrinterError::Network(addr, e.to_string()),
    };
    let mut stream = timeout(limit, TcpStream::connect(addr))
        .await
        .map_err(|_| PrinterError::Timeout(addr, limit))?
        .map_err(classify)?;
    timeout(limit, async {
        stream.write_all(bytes).await?;
        stream.flush().await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| PrinterError::Timeout(addr, limit))?
    .map_err(classify)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    const LOCAL: Ipv4Addr = Ipv4Addr::LOCALHOST;

    #[test]
    fn a_24_lists_its_254_hosts_and_skips_the_hub() {
        let hub = Ipv4Addr::new(192, 168, 1, 20);
        let s = Subnet::of_24(hub);
        let hosts = s.hosts();
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));
        let targets = scan_hosts(&[s, Subnet::of_24(Ipv4Addr::new(192, 168, 1, 99))], &[hub]);
        assert_eq!(targets.len(), 253);
        assert!(!targets.contains(&hub));
        assert!(targets.iter().all(|t| t.octets()[..3] == [192, 168, 1]));
        assert_eq!(Subnet::single(LOCAL).hosts(), vec![LOCAL]);
    }

    #[test]
    fn the_hub_scans_only_its_own_subnets() {
        let own = own_addresses();
        let targets = LanScanner::new().targets();
        for t in &targets {
            assert!(
                own.iter().any(|o| o.octets()[..3] == t.octets()[..3]),
                "{t} is outside the hub's own /24s"
            );
            assert!(!own.contains(t));
        }
        assert!(targets.len() <= own.len() * 254);
    }

    async fn listener() -> (TcpListener, u16) {
        let l = TcpListener::bind((LOCAL, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    #[tokio::test]
    async fn a_listener_in_the_scanned_subnet_is_found_within_the_deadline() {
        let (l, port) = listener().await;
        let accept = tokio::spawn(async move { while l.accept().await.is_ok() {} });
        let scanner = LanScanner {
            port,
            subnets: Some(vec![Subnet::single(LOCAL)]),
            ..LanScanner::default()
        };
        let started = std::time::Instant::now();
        let found = scanner.scan().await;
        assert!(started.elapsed() < SCAN_DEADLINE);
        assert_eq!(
            found,
            vec![FoundPrinter {
                id: format!("lan:127.0.0.1:{port}"),
                kind: PrinterKind::Lan,
                name: "Network printer".into(),
                addr: Some(format!("127.0.0.1:{port}")),
            }]
        );
        accept.abort();
    }

    #[tokio::test]
    async fn nothing_listening_finds_nothing() {
        let (l, port) = listener().await;
        drop(l);
        let scanner = LanScanner {
            port,
            subnets: Some(vec![Subnet::single(LOCAL)]),
            ..LanScanner::default()
        };
        assert!(scanner.scan().await.is_empty());
    }

    #[test]
    fn loopback_and_link_local_interfaces_are_not_scanned() {
        use std::net::IpAddr;
        let lan = Ipv4Addr::new(192, 168, 1, 20);
        let got = scannable([
            (true, IpAddr::V4(Ipv4Addr::LOCALHOST)),
            (false, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))),
            (false, IpAddr::V4(Ipv4Addr::new(169, 254, 10, 3))),
            (false, IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            (false, IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
            (true, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            (false, IpAddr::V4(lan)),
            (false, IpAddr::V4(lan)),
            (false, IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
        ]);
        assert_eq!(got, vec![Ipv4Addr::new(10, 1, 2, 3), lan]);
    }

    #[tokio::test]
    async fn the_scan_stops_at_its_deadline() {
        let hosts: Vec<Ipv4Addr> = (1..=200).map(|d| Ipv4Addr::new(10, 0, 0, d)).collect();
        let deadline = Duration::from_millis(400);
        let started = std::time::Instant::now();
        let found = probe_all(hosts, 9100, deadline, |addr| async move {
            if addr.ip().octets()[3] == 7 {
                return true;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
            true
        })
        .await;
        let took = started.elapsed();
        assert_eq!(found, vec!["10.0.0.7:9100".parse().unwrap()]);
        assert!(took >= deadline, "{took:?}");
        assert!(took < deadline + Duration::from_secs(2), "{took:?}");
    }

    #[tokio::test]
    async fn a_manual_address_receives_the_bytes() {
        let (l, port) = listener().await;
        let got = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).await.unwrap();
            buf
        });
        let addr = SocketAddrV4::new(LOCAL, port);
        send_lan(addr, b"\x1b@hello", SEND_TIMEOUT).await.unwrap();
        assert_eq!(got.await.unwrap(), b"\x1b@hello");
    }

    #[tokio::test]
    async fn a_refused_connection_is_reported() {
        let (l, port) = listener().await;
        drop(l);
        let addr = SocketAddrV4::new(LOCAL, port);
        assert_eq!(
            send_lan(addr, b"x", SEND_TIMEOUT).await,
            Err(PrinterError::Refused(addr))
        );
    }

    #[tokio::test]
    async fn a_printer_that_stops_reading_times_out() {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(4096).unwrap();
        socket.bind((LOCAL, 0).into()).unwrap();
        let l = socket.listen(1).unwrap();
        let port = l.local_addr().unwrap().port();
        let hold = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
            drop(s);
        });
        let addr = SocketAddrV4::new(LOCAL, port);
        let limit = Duration::from_millis(300);
        let big = vec![0u8; 16 * 1024 * 1024];
        assert_eq!(
            send_lan(addr, &big, limit).await,
            Err(PrinterError::Timeout(addr, limit))
        );
        hold.abort();
    }
}

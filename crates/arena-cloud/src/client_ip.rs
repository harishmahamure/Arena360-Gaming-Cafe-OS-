use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;

use axum::http::HeaderMap;

const CLOUDFLARE_V4: &str = include_str!("../../../deploy/cloudflare/ips-v4.txt");
const CLOUDFLARE_V6: &str = include_str!("../../../deploy/cloudflare/ips-v6.txt");

pub const CF_CONNECTING_IP: &str = "cf-connecting-ip";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    net: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(s: &str) -> Option<Self> {
        let (addr, prefix) = s.trim().split_once('/')?;
        let net: IpAddr = addr.parse().ok()?;
        let prefix: u8 = prefix.parse().ok()?;
        let max = if net.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { net, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

pub fn parse_ranges(text: &str) -> Vec<Cidr> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(Cidr::parse)
        .collect()
}

pub fn cloudflare_ranges() -> &'static [Cidr] {
    static RANGES: OnceLock<Vec<Cidr>> = OnceLock::new();
    RANGES.get_or_init(|| {
        let mut all = parse_ranges(CLOUDFLARE_V4);
        all.extend(parse_ranges(CLOUDFLARE_V6));
        all
    })
}

pub fn is_cloudflare(ip: IpAddr) -> bool {
    cloudflare_ranges().iter().any(|c| c.contains(ip))
}

pub fn client_ip(peer: Option<SocketAddr>, headers: &HeaderMap) -> Option<IpAddr> {
    let peer = peer?.ip().to_canonical();
    if !is_cloudflare(peer) {
        return Some(peer);
    }
    let forwarded = headers
        .get(CF_CONNECTING_IP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical());
    Some(forwarded.unwrap_or(peer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cf: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = cf {
            h.insert(CF_CONNECTING_IP, v.parse().unwrap());
        }
        h
    }

    fn peer(s: &str) -> Option<SocketAddr> {
        Some(SocketAddr::new(s.parse().unwrap(), 443))
    }

    fn ranges_in(t: &str) -> usize {
        t.lines()
            .filter(|l| !l.split('#').next().unwrap_or("").trim().is_empty())
            .count()
    }

    #[test]
    fn every_committed_range_parses() {
        assert_eq!(parse_ranges(CLOUDFLARE_V4).len(), ranges_in(CLOUDFLARE_V4));
        assert_eq!(parse_ranges(CLOUDFLARE_V6).len(), ranges_in(CLOUDFLARE_V6));
        assert!(cloudflare_ranges().len() > 10);
    }

    #[test]
    fn ranges_match_by_prefix() {
        let c = Cidr::parse("173.245.48.0/20").unwrap();
        assert!(c.contains("173.245.48.1".parse().unwrap()));
        assert!(c.contains("173.245.63.255".parse().unwrap()));
        assert!(!c.contains("173.245.64.0".parse().unwrap()));
        assert!(c.contains("::ffff:173.245.48.9".parse().unwrap()));
        let v6 = Cidr::parse("2400:cb00::/32").unwrap();
        assert!(v6.contains("2400:cb00:1::1".parse().unwrap()));
        assert!(!v6.contains("2400:cb01::1".parse().unwrap()));
        assert!(!v6.contains("173.245.48.1".parse().unwrap()));
        assert!(
            Cidr::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        assert_eq!(Cidr::parse("1.2.3.4/33"), None);
        assert_eq!(Cidr::parse("nope/8"), None);
    }

    #[test]
    fn the_header_counts_only_from_a_cloudflare_peer() {
        assert_eq!(
            client_ip(peer("173.245.48.10"), &headers(Some("203.0.113.7"))),
            Some("203.0.113.7".parse().unwrap())
        );
        assert_eq!(
            client_ip(peer("198.51.100.4"), &headers(Some("203.0.113.7"))),
            Some("198.51.100.4".parse().unwrap())
        );
        assert_eq!(
            client_ip(peer("173.245.48.10"), &headers(Some("garbage"))),
            Some("173.245.48.10".parse().unwrap())
        );
        assert_eq!(
            client_ip(peer("173.245.48.10"), &headers(None)),
            Some("173.245.48.10".parse().unwrap())
        );
        assert_eq!(client_ip(None, &headers(Some("203.0.113.7"))), None);
    }
}

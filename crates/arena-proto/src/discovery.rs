use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::frame::PROTOCOL_VERSION;

pub const MDNS_SERVICE: &str = "_arena360-hub._tcp.local.";
pub const HUB_PRODUCT: &str = "arena360-hub";
pub const TXT_BRANCH: &str = "branch";
pub const TXT_HUB: &str = "hub";
pub const TXT_PROTO: &str = "proto";
pub const TXT_FP: &str = "fp";
pub const TXT_WEB: &str = "web";
/// The Hub's epoch, `<counter>:<instance>` (Story 9.21): a promoted standby advertises its new one.
pub const TXT_EPOCH: &str = "epoch";
pub const DEFAULT_WEB_PORT: u16 = 7401;
pub const MAX_HUB_NAME_BYTES: usize = 63;
pub const MAX_BRANCH_NAME_BYTES: usize = 80;
pub const DISCOVERY_WINDOW_MS: u64 = 10_000;
pub const PROBE_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HubInfo {
    pub product: String,
    pub branch_name: String,
    pub hub_name: String,
    pub protocol_version: u32,
    pub fingerprint: String,
    pub lan_port: u16,
    pub web_port: u16,
}

impl HubInfo {
    pub fn new(
        branch_name: impl Into<String>,
        hub_name: impl Into<String>,
        fingerprint: impl Into<String>,
        lan_port: u16,
        web_port: u16,
    ) -> Self {
        Self {
            product: HUB_PRODUCT.to_owned(),
            branch_name: branch_name.into(),
            hub_name: hub_name.into(),
            protocol_version: PROTOCOL_VERSION,
            fingerprint: fingerprint.into(),
            lan_port,
            web_port,
        }
    }

    pub fn txt(&self) -> Vec<(String, String)> {
        txt_records(
            &self.branch_name,
            &self.hub_name,
            self.protocol_version,
            &self.fingerprint,
            self.web_port,
        )
    }
}

pub fn txt_records(
    branch_name: &str,
    hub_name: &str,
    protocol_version: u32,
    fingerprint: &str,
    web_port: u16,
) -> Vec<(String, String)> {
    vec![
        (TXT_BRANCH.to_owned(), branch_name.to_owned()),
        (TXT_HUB.to_owned(), hub_name.to_owned()),
        (TXT_PROTO.to_owned(), protocol_version.to_string()),
        (TXT_FP.to_owned(), fingerprint.to_owned()),
        (TXT_WEB.to_owned(), web_port.to_string()),
    ]
}

fn bounded_label(raw: &str, max_bytes: usize) -> String {
    let printable: String = raw.chars().filter(|c| !c.is_control()).collect();
    let mut out = String::new();
    for c in printable.trim().chars() {
        if out.len() + c.len_utf8() > max_bytes {
            break;
        }
        out.push(c);
    }
    out.trim_end().to_owned()
}

/// The epoch a Hub advertised in its TXT record, when it is one (`<counter>:<instance>`).
pub fn txt_epoch(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    let epoch = get(TXT_EPOCH)?;
    let epoch = epoch.trim();
    crate::epoch_parts(epoch).map(|_| epoch.to_owned())
}

pub fn sanitize_hub_name(raw: &str) -> String {
    let out = bounded_label(raw, MAX_HUB_NAME_BYTES);
    if out.is_empty() {
        HUB_PRODUCT.to_owned()
    } else {
        out
    }
}

pub fn sanitize_branch_name(raw: &str) -> String {
    bounded_label(raw, MAX_BRANCH_NAME_BYTES)
}

pub fn is_fingerprint(fp: &str) -> bool {
    fp.len() == 64
        && fp
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DiscoveredHub {
    pub addr: String,
    pub branch_name: String,
    pub hub_name: String,
    pub fingerprint: String,
    pub protocol_version: u32,
    pub compatible: bool,
    pub web_port: u16,
}

impl DiscoveredHub {
    pub fn from_txt(addr: String, get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let fingerprint = get(TXT_FP)?.trim().to_ascii_lowercase();
        if !is_fingerprint(&fingerprint) {
            return None;
        }
        let protocol_version = get(TXT_PROTO)?.trim().parse::<u32>().ok()?;
        let web_port = get(TXT_WEB)
            .and_then(|w| w.trim().parse::<u16>().ok())
            .filter(|p| *p != 0)
            .unwrap_or(DEFAULT_WEB_PORT);
        Some(Self {
            addr,
            branch_name: sanitize_branch_name(&get(TXT_BRANCH).unwrap_or_default()),
            hub_name: get(TXT_HUB).map(|h| sanitize_hub_name(&h))?,
            fingerprint,
            protocol_version,
            compatible: protocol_version == PROTOCOL_VERSION,
            web_port,
        })
    }

    pub fn from_info(addr: String, info: &HubInfo, web_port: u16) -> Self {
        Self {
            addr,
            branch_name: sanitize_branch_name(&info.branch_name),
            hub_name: sanitize_hub_name(&info.hub_name),
            fingerprint: info.fingerprint.to_ascii_lowercase(),
            protocol_version: info.protocol_version,
            compatible: info.protocol_version == PROTOCOL_VERSION,
            web_port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DiscoveryFound {
    #[ts(type = "number")]
    pub search: u64,
    pub hubs: Vec<DiscoveredHub>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProbeOk {
    pub hub: DiscoveredHub,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SelectedHub {
    pub addr: String,
    pub fingerprint: String,
    pub branch_name: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
}

fn default_web_port() -> u16 {
    DEFAULT_WEB_PORT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_info_has_the_product_and_protocol() {
        let info = HubInfo::new("Koramangala", "front-desk", "ab".repeat(32), 7400, 7401);
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "product": "arena360-hub",
                "branch_name": "Koramangala",
                "hub_name": "front-desk",
                "protocol_version": PROTOCOL_VERSION,
                "fingerprint": "ab".repeat(32),
                "lan_port": 7400,
                "web_port": 7401,
            })
        );
        assert_eq!(
            info.txt(),
            vec![
                ("branch".to_owned(), "Koramangala".to_owned()),
                ("hub".to_owned(), "front-desk".to_owned()),
                ("proto".to_owned(), PROTOCOL_VERSION.to_string()),
                ("fp".to_owned(), "ab".repeat(32)),
                ("web".to_owned(), "7401".to_owned()),
            ]
        );
    }

    #[test]
    fn hub_names_are_printable_and_short() {
        assert_eq!(sanitize_hub_name("  Front-Desk \n"), "Front-Desk");
        assert_eq!(sanitize_hub_name("a\u{7}b"), "ab");
        assert_eq!(sanitize_hub_name(""), HUB_PRODUCT);
        assert_eq!(sanitize_hub_name("\t\r"), HUB_PRODUCT);
        let long = "x".repeat(100);
        assert_eq!(sanitize_hub_name(&long).len(), MAX_HUB_NAME_BYTES);
        let wide = "é".repeat(40);
        let s = sanitize_hub_name(&wide);
        assert!(s.len() <= MAX_HUB_NAME_BYTES && s.chars().all(|c| c == 'é'));
    }

    #[test]
    fn txt_records_parse_back_with_the_compatibility_flag() {
        let txt = txt_records("B", "H", PROTOCOL_VERSION, &"AB".repeat(32), 8401);
        let get = |k: &str| txt.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let hub = DiscoveredHub::from_txt("10.0.0.2:7400".into(), get).unwrap();
        assert_eq!(hub.fingerprint, "ab".repeat(32));
        assert!(hub.compatible);
        assert_eq!(hub.web_port, 8401);
        let old = txt_records("B", "H", PROTOCOL_VERSION + 1, &"ab".repeat(32), 7401);
        let get = |k: &str| old.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let hub = DiscoveredHub::from_txt("10.0.0.2:7400".into(), get).unwrap();
        assert!(!hub.compatible);
        let bad = txt_records("B", "H", PROTOCOL_VERSION, "short", 7401);
        let get = |k: &str| bad.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(DiscoveredHub::from_txt("x".into(), get), None);
        assert_eq!(DiscoveredHub::from_txt("x".into(), |_| None), None);
        let no_web = |k: &str| match k {
            "fp" => Some("ab".repeat(32)),
            "proto" => Some("1".to_owned()),
            "hub" => Some("H".to_owned()),
            _ => None,
        };
        assert_eq!(
            DiscoveredHub::from_txt("x".into(), no_web)
                .unwrap()
                .web_port,
            DEFAULT_WEB_PORT
        );
    }

    #[test]
    fn hub_info_names_are_sanitised_and_bounded() {
        let mut info = HubInfo::new(
            "b".repeat(200),
            "h\u{7}".repeat(50),
            "AB".repeat(32),
            7400,
            7401,
        );
        info.branch_name.push('\n');
        let hub = DiscoveredHub::from_info("x".into(), &info, 7401);
        assert_eq!(hub.branch_name.len(), MAX_BRANCH_NAME_BYTES);
        assert_eq!(hub.hub_name, "h".repeat(50));
        assert_eq!(hub.fingerprint, "ab".repeat(32));
        assert_eq!(sanitize_branch_name("  Kora\u{0}mangala "), "Koramangala");
    }
}

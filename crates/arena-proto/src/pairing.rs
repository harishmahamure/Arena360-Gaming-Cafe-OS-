use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PAIRING_CODE_ALPHABET: &[u8; 30] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const PAIRING_CODE_LEN: usize = 8;
pub const PAIRING_CODE_TTL_MS: i64 = 15 * 60 * 1000;

pub fn normalize_pairing_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub fn valid_pairing_code(normalized: &str) -> bool {
    normalized.len() == PAIRING_CODE_LEN
        && normalized
            .bytes()
            .all(|b| PAIRING_CODE_ALPHABET.contains(&b))
}

pub fn format_pairing_code(code: &str) -> String {
    let half = PAIRING_CODE_LEN / 2;
    match (code.get(..half), code.get(half..)) {
        (Some(a), Some(b)) if code.len() == PAIRING_CODE_LEN => format!("{a} {b}"),
        _ => code.to_owned(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct IssuePairingCode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PairingCodeIssued {
    pub code: String,
    #[ts(type = "number")]
    pub expires_at: i64,
    #[ts(type = "number")]
    pub server_now: i64,
}

impl std::fmt::Debug for PairingCodeIssued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingCodeIssued")
            .field("code", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("server_now", &self.server_now)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ActivePairingCode {
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct InstallHubState {
    pub active: Option<ActivePairingCode>,
    pub hub_paired: bool,
    #[ts(type = "number")]
    pub server_now: i64,
}

pub const HUB_TOKEN_PREFIX: &str = "hub_";
pub const PAIR_FAILURES_PER_CODE: u32 = 5;
pub const PAIR_ATTEMPTS_PER_IP_PER_HOUR: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum HubPlatform {
    Windows,
    Android,
}

impl HubPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Android => "android",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Windows, Self::Android]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HubPairRequest {
    pub code: String,
    pub platform: HubPlatform,
    pub hub_instance_id: String,
    pub version: String,
}

impl std::fmt::Debug for HubPairRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubPairRequest")
            .field("code", &"[redacted]")
            .field("platform", &self.platform)
            .field("hub_instance_id", &self.hub_instance_id)
            .field("version", &self.version)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HubPairResponse {
    pub hub_id: String,
    pub token: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub tier: crate::tier::Tier,
    pub branch_code: String,
    pub licence: String,
    pub bootstrap: BranchBootstrapCommand,
}

pub const SETUP_CODE_HUB_TTL_MS: i64 = 72 * 60 * 60 * 1000;
pub const SETUP_CODE_PLAINTEXT_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const BOOTSTRAP_DAY_START_MIN: u16 = 0;
pub const OWNER_ROLE: &str = "Owner";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BootstrapProfile {
    pub name: String,
    pub branch_code: String,
    pub currency: String,
    pub tz: String,
    pub day_start_min: u16,
    pub country: String,
    pub gst_registered: bool,
    pub gstin: String,
    pub tax_mode: crate::tax::TaxMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BootstrapOwner {
    pub name: String,
    pub login: String,
    pub role: String,
    pub setup_code_hash: String,
    #[ts(type = "number")]
    pub setup_expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BranchBootstrapCommand {
    pub idempotency_key: String,
    pub profile: BootstrapProfile,
    pub owner_staff: BootstrapOwner,
}

pub fn check_bootstrap(
    branch_code: &str,
    country: &str,
    gst_registered: bool,
    gstin: &str,
    tax_mode: crate::tax::TaxMode,
    owner: &BootstrapOwner,
) -> Result<(), (&'static str, &'static str)> {
    if !crate::wizard::valid_branch_code(branch_code) {
        return Err((
            "branch_code",
            "branch_code must be 3–6 upper-case letters or digits",
        ));
    }
    let tax = crate::tax::TaxSettings {
        country: country.to_owned(),
        gst: gst_registered,
        gstin: gstin.to_owned(),
        ..crate::tax::TaxSettings::parse(crate::tax::DEFAULT_TAX).expect("DEFAULT_TAX parses")
    };
    tax.check()?;
    if tax.tax_mode() != tax_mode {
        return Err((
            "tax_mode",
            "tax_mode must be gst_in only for a GST-registered Indian branch",
        ));
    }
    let phone_ok = owner.login.len() <= 20
        && owner
            .login
            .strip_prefix('+')
            .is_some_and(|d| (8..=15).contains(&d.len()) && d.bytes().all(|c| c.is_ascii_digit()));
    if !phone_ok {
        return Err(("login", "login must be the owner's phone in E.164 form"));
    }
    let name = owner.name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err((
            "display_name",
            "name must be 1–80 characters with no control characters",
        ));
    }
    if owner.role != OWNER_ROLE {
        return Err(("role", "the first staff member is the Owner"));
    }
    if !owner.setup_code_hash.starts_with("$argon2id$") {
        return Err((
            "setup_code_hash",
            "setup_code_hash must be an Argon2id hash",
        ));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCodeReveal {
    pub code: String,
}

impl std::fmt::Debug for SetupCodeReveal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupCodeReveal")
            .field("code", &"[redacted]")
            .finish()
    }
}

impl std::fmt::Debug for HubPairResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubPairResponse")
            .field("hub_id", &self.hub_id)
            .field("token", &"[redacted]")
            .field("tenant_id", &self.tenant_id)
            .field("branch_id", &self.branch_id)
            .field("tier", &self.tier)
            .field("branch_code", &self.branch_code)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PairHubRequest {
    pub code: String,
}

impl std::fmt::Debug for PairHubRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairHubRequest")
            .field("code", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PairHubOk {
    pub paired: bool,
}

pub const HEARTBEAT_BOOT_INTERVAL_S: u64 = 10;
pub const HEARTBEAT_LIVE_INTERVAL_S: u64 = 60;
pub const HEARTBEAT_MIN_GAP_MS: i64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HeartbeatRequest {
    pub epoch: String,
    pub version: String,
    pub schema_version: u32,
    #[ts(type = "number")]
    pub last_seq_uploaded: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HeartbeatFence {
    pub epoch: String,
    pub confirmed: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HeartbeatResponse {
    pub licence: String,
    pub fence: HeartbeatFence,
    #[ts(type = "number")]
    pub cloud_time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_commands: Option<u32>,
}

impl std::fmt::Debug for HeartbeatResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatResponse")
            .field("fence", &self.fence)
            .field("cloud_time", &self.cloud_time)
            .field("pending_commands", &self.pending_commands)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alphabet_has_no_ambiguous_characters() {
        assert_eq!(PAIRING_CODE_ALPHABET.len(), 30);
        for c in b"01OILU" {
            assert!(!PAIRING_CODE_ALPHABET.contains(c), "{}", *c as char);
        }
        let mut sorted = PAIRING_CODE_ALPHABET.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 30);
        assert!(
            PAIRING_CODE_ALPHABET
                .iter()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        );
    }

    #[test]
    fn normalizing_uppercases_and_strips_spaces_and_hyphens() {
        assert_eq!(normalize_pairing_code("abcd efgh"), "ABCDEFGH");
        assert_eq!(normalize_pairing_code(" 7k2m-9pqr "), "7K2M9PQR");
        assert_eq!(normalize_pairing_code("AB-CD EF--GH"), "ABCDEFGH");
        assert_eq!(
            normalize_pairing_code("ab_cd\tef\u{a0}gh\u{3000}\n"),
            "ABCDEFGH"
        );
        assert_eq!(normalize_pairing_code("7k2m\u{2009}9pqr"), "7K2M9PQR");
    }

    #[test]
    fn only_eight_alphabet_characters_are_a_valid_code() {
        assert!(valid_pairing_code("7K2M9PQR"));
        assert!(valid_pairing_code(&normalize_pairing_code(" 7k2m-9pqr ")));
        for bad in [
            "",
            "7K2M9PQ",
            "7K2M9PQRS",
            "7K2M9PQ0",
            "7K2M9PQO",
            "7K2M9PQI",
            "7K2M9PQL",
            "7K2M9PQU",
            "7K2M9PQ1",
            "7K2M9PQ!",
            "7K2M9PQÉ",
            "7k2m9pqr",
        ] {
            assert!(!valid_pairing_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn pairing_codes_are_eight_alphabet_characters() {
        assert!(valid_pairing_code("7K2M9PQR"));
        assert!(!valid_pairing_code("7K2M9PQ"));
        assert!(!valid_pairing_code("7K2M9PQ0"));
        assert!(!valid_pairing_code("7k2m9pqr"));
    }

    #[test]
    fn platforms_round_trip_and_secrets_never_show_in_debug() {
        for p in [HubPlatform::Windows, HubPlatform::Android] {
            assert_eq!(HubPlatform::parse(p.as_str()), Some(p));
            assert_eq!(
                serde_json::to_string(&p).unwrap(),
                format!("\"{}\"", p.as_str())
            );
        }
        assert_eq!(HubPlatform::parse("linux"), None);
        let req = HubPairRequest {
            code: "7K2M9PQR".into(),
            platform: HubPlatform::Android,
            hub_instance_id: "i".into(),
            version: "0.1.0".into(),
        };
        assert!(!format!("{req:?}").contains("7K2M9PQR"));
        let res = HubPairResponse {
            hub_id: "h".into(),
            token: "hub_h.secret".into(),
            tenant_id: "t".into(),
            branch_id: "b".into(),
            tier: crate::tier::Tier::Free,
            branch_code: "LVL".into(),
            licence: "jwt".into(),
            bootstrap: BranchBootstrapCommand {
                idempotency_key: "k".into(),
                profile: BootstrapProfile {
                    name: "n".into(),
                    branch_code: "LVL".into(),
                    currency: "INR".into(),
                    tz: "Asia/Kolkata".into(),
                    day_start_min: 0,
                    country: "IN".into(),
                    gst_registered: false,
                    gstin: String::new(),
                    tax_mode: crate::tax::TaxMode::Simple,
                },
                owner_staff: BootstrapOwner {
                    name: "o".into(),
                    login: "+919876543210".into(),
                    role: OWNER_ROLE.into(),
                    setup_code_hash: "h".into(),
                    setup_expires_at: 1,
                },
            },
        };
        assert!(!format!("{res:?}").contains("secret"));
        let hub = PairHubRequest {
            code: "7K2M9PQR".into(),
        };
        assert!(!format!("{hub:?}").contains("7K2M9PQR"));
    }

    #[test]
    fn display_groups_four_and_four() {
        assert_eq!(format_pairing_code("7K2M9PQR"), "7K2M 9PQR");
        assert_eq!(format_pairing_code("ABC"), "ABC");
    }

    #[test]
    fn the_issued_code_never_shows_in_debug_and_active_serializes_null() {
        let issued = PairingCodeIssued {
            code: "7K2M9PQR".into(),
            expires_at: 5,
            server_now: 1,
        };
        assert!(!format!("{issued:?}").contains("7K2M9PQR"));
        assert_eq!(
            serde_json::to_string(&InstallHubState {
                active: None,
                hub_paired: false,
                server_now: 7,
            })
            .unwrap(),
            r#"{"active":null,"hub_paired":false,"server_now":7}"#
        );
    }
}

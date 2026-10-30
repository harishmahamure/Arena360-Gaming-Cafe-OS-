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
    pub tier: crate::tier::Tier,
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
    /// What a first Hub applies to start the branch. Absent when the branch already has history
    /// (`restore_pending`): nothing is queued then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bootstrap: Option<BranchBootstrapCommand>,
    /// This Hub replaces an earlier one of a branch with history (Story 9.9): it opens in
    /// "Restore this branch" and serves nothing until it is restored.
    #[serde(default)]
    pub restore_pending: bool,
    #[serde(default)]
    pub branch_name: String,
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
    /// A short-lived link to the setup file of another branch of the tenant, when the owner
    /// starts this branch from it (Story 5.38). The Hub imports it after the bootstrap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub template: Option<crate::setup_file::SetupTemplateLink>,
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
    /// The last change `seq` of `epoch` the Hub has had accepted: its upload cursor when that is
    /// in `epoch`, else the fork parent's `seq` (or 0).
    #[ts(type = "number")]
    pub last_seq_uploaded: i64,
    /// When the Hub last had a batch accepted (Hub clock, ms) (Story 8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_upload_at: Option<i64>,
    /// When the Hub last published its branch summary (Story 8.4); absent until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_summary_at: Option<i64>,
    /// The business day of the last rollup the Hub published (Story 8.5); absent until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_rollup_date: Option<String>,
    /// The `seq` `epoch` was forked from, when it was: the first row it uploads is one more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub fork_seq: Option<i64>,
    /// The last `seq` of `epoch` the Hub's upgrade snapshot holds (Story 9.11): the cloud has no
    /// batches up to it, so it looks for missing batches only after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub base_seq: Option<i64>,
    /// A range the cloud asked for that the Hub no longer holds (Story 8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resend_unavailable: Option<ResendUnavailable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub setup: Option<crate::BranchSetup>,
    /// A Free branch's ad impression totals after its cursor (Story 4.14); absent on a paid
    /// branch or when there is nothing new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ad_impressions: Option<crate::AdCounts>,
    /// The same for ad requests, with their own cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ad_requests: Option<crate::AdCounts>,
    /// The newest owner-command `schema_version` this Hub runs (Story 8.18); an older Hub omits
    /// it and counts as version 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_schema: Option<u32>,
    /// The branch's opening hours and time zone, sent by a paid Hub only, so the cloud can tell
    /// when a silent branch should be open (Story 8.35).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub opening: Option<crate::BranchOpening>,
    /// Whether the Hub holds a tunnel credential (Story 8.39); an older Hub omits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tunnel: Option<crate::TunnelReport>,
}

/// A run of change-log `seq`s of the heartbeat's epoch, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SeqRange {
    #[ts(type = "number")]
    pub from: i64,
    #[ts(type = "number")]
    pub to: i64,
}

impl SeqRange {
    pub fn valid(&self) -> bool {
        1 <= self.from && self.from <= self.to
    }
}

/// A range of `epoch` the Hub cannot resend because its rows were pruned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ResendUnavailable {
    pub epoch: String,
    #[ts(type = "number")]
    pub from: i64,
    #[ts(type = "number")]
    pub to: i64,
}

impl ResendUnavailable {
    pub fn range(&self) -> SeqRange {
        SeqRange {
            from: self.from,
            to: self.to,
        }
    }

    /// `self` widened by a later report of the same epoch. Holes are handled in seq order, so a
    /// run reported after this one means every seq between them is held or already reported:
    /// the two are one run. A report that starts earlier is merged only when it touches this
    /// one, and otherwise replaces it.
    pub fn merged(&self, next: &Self) -> Self {
        if self.epoch != next.epoch {
            return next.clone();
        }
        if next.from >= self.from || next.to.saturating_add(1) >= self.from {
            Self {
                epoch: self.epoch.clone(),
                from: self.from.min(next.from),
                to: self.to.max(next.to),
            }
        } else {
            next.clone()
        }
    }
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
    /// The cloud's stored impression cursor, when the request carried a block: the block's
    /// `through_id` once added, else the cursor the cloud already held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub ad_impressions_through: Option<i64>,
    /// The same for requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub ad_requests_through: Option<i64>,
    /// A fresh link to the setup file the owner asked this branch to start from again
    /// (Story 5.38).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub setup_template: Option<crate::setup_file::SetupTemplateLink>,
    /// The first run of the request's epoch the cloud does not hold; the Hub sends it again
    /// before anything else (Story 8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resend: Option<SeqRange>,
    /// What to do about the branch's tunnel (Story 8.39); always present for a Free or paid
    /// branch, absent from an older cloud. It may carry the tunnel credential, once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tunnel: Option<crate::TunnelAnswer>,
}

impl std::fmt::Debug for HeartbeatResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatResponse")
            .field("fence", &self.fence)
            .field("cloud_time", &self.cloud_time)
            .field("pending_commands", &self.pending_commands)
            .field("tunnel", &self.tunnel)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(epoch: &str, from: i64, to: i64) -> ResendUnavailable {
        ResendUnavailable {
            epoch: epoch.to_owned(),
            from,
            to,
        }
    }

    #[test]
    fn two_pruned_runs_of_one_epoch_merge_into_one() {
        let a = run("e1", 10, 20);
        assert_eq!(a.merged(&run("e1", 30, 40)), run("e1", 10, 40));
        assert_eq!(a.merged(&run("e1", 21, 25)), run("e1", 10, 25));
        assert_eq!(a.merged(&run("e1", 1, 9)), run("e1", 1, 20));
        assert_eq!(a.merged(&run("e1", 1, 5)), run("e1", 1, 5));
        assert_eq!(a.merged(&run("e2", 30, 40)), run("e2", 30, 40));
    }

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
            bootstrap: Some(BranchBootstrapCommand {
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
                template: None,
            }),
            restore_pending: false,
            branch_name: "n".into(),
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
                tier: crate::tier::Tier::Free,
                server_now: 7,
            })
            .unwrap(),
            r#"{"active":null,"hub_paired":false,"tier":"free","server_now":7}"#
        );
    }
}

//! Owner sign-up building blocks: the shared state of the `/v1/signup`, `/v1/session` and
//! `/v1/account` routes, form validation, one-time codes (HMAC-SHA256 with the pepper, compared in constant
//! time), opaque session tokens (stored as SHA-256) and the `phone_tag` that stands in for a
//! phone number in logs.

use std::sync::Arc;

use arena_proto::{
    AppError, OTP_DIGITS, OwnerCommandKind, SignupStart, accepted_owner_kind, owner_command_kind,
};
use arena_store::control::{ControlDb, NewOwner};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::Clock;
use crate::config::AuthConfig;
use crate::messaging::Messenger;

type HmacSha256 = Hmac<Sha256>;

/// The HMAC input label of a code hash.
const OTP_LABEL: &[u8] = b"otp\0";
/// The HMAC input label of a phone tag.
const TAG_LABEL: &[u8] = b"tag\0";
const PAIR_LABEL: &[u8] = b"pair\0";
const LINK_LABEL: &[u8] = b"tpl\0";
const NOTIFY_LABEL: &[u8] = b"notify\0";

/// What the sign-up and session routes share.
pub struct AuthState {
    pub db: Arc<ControlDb>,
    pub sms: Messenger,
    /// Sends player and owner messages (Story 8.28); set once by `auth_state`. Unset: `/v1/notify` answers 503.
    pub notifier: std::sync::OnceLock<crate::messaging::Notifier>,
    pub clock: Clock,
    pepper: Vec<u8>,
    /// Exact browser origins allowed with credentials.
    pub web_origins: Vec<String>,
    /// SMS sends allowed per rolling minute across every phone.
    pub sms_global_per_min: u64,
    /// Seals owners' TOTP secrets.
    pub mfa_key: crate::crypto::MfaKey,
    pub dummy_verifies: std::sync::atomic::AtomicU64,
    pub licence: crate::licence::LicenceSigner,
    pub billing_enabled: bool,
    pub hub_tokens: std::sync::Mutex<std::collections::HashMap<[u8; 32], i64>>,
    pub wizard_tx: tokio::sync::broadcast::Sender<String>,
    /// Where setup templates are kept (Story 5.37); set once by `serve`.
    pub templates: std::sync::OnceLock<crate::templates::TemplateStore>,
    /// The CDN base URL catalogue media are served from (Story 6.1); set once by `serve`.
    pub catalog_cdn: std::sync::OnceLock<String>,
    /// Signs the upload links of `POST /v1/presign` (Story 8.1); set once by `serve`.
    pub presigner: std::sync::OnceLock<crate::presign::Presigner>,
    /// R2 PUT links issued per tenant and branch.
    pub put_metrics: crate::presign::PutMetrics,
    /// Upload gaps found by heartbeats per tenant and branch (Story 8.3).
    pub gap_metrics: crate::gaps::GapMetrics,
    /// Rollup days the cloud cannot read, and the missing-day count per branch (Story 8.7).
    pub rollup_metrics: crate::rollups::RollupMetrics,
    /// How old the figures were each time the owner opened Today (Story 8.13).
    pub data_age: crate::data_age::DataAgeMetrics,
    /// The branch summaries and rollup months the owner read API serves from memory (Story 8.9).
    pub read_cache: crate::owner_read::ReadCache,
    /// Owner command kinds searched after `OWNER_COMMAND_KINDS` (Story 8.25): empty in production,
    /// set by tests that need a sensitive kind before one exists. A payload need only be an object.
    pub extra_kinds: std::sync::Mutex<Vec<OwnerCommandKind>>,
}

impl AuthState {
    pub fn new(db: Arc<ControlDb>, sms: Messenger, clock: Clock, cfg: &AuthConfig) -> Self {
        Self {
            db,
            sms,
            notifier: std::sync::OnceLock::new(),
            clock,
            pepper: cfg.pepper.clone(),
            web_origins: cfg.web_origins.clone(),
            sms_global_per_min: cfg.sms_global_per_min,
            mfa_key: cfg.mfa_key.clone(),
            dummy_verifies: std::sync::atomic::AtomicU64::new(0),
            licence: cfg.licence.clone(),
            billing_enabled: cfg.billing_enabled,
            hub_tokens: std::sync::Mutex::default(),
            wizard_tx: tokio::sync::broadcast::channel(64).0,
            templates: std::sync::OnceLock::new(),
            catalog_cdn: std::sync::OnceLock::new(),
            presigner: std::sync::OnceLock::new(),
            put_metrics: crate::presign::PutMetrics::default(),
            gap_metrics: crate::gaps::GapMetrics::default(),
            rollup_metrics: crate::rollups::RollupMetrics::default(),
            data_age: crate::data_age::DataAgeMetrics::default(),
            read_cache: crate::owner_read::ReadCache::default(),
            extra_kinds: std::sync::Mutex::default(),
        }
    }

    /// The owner command kind the cloud accepts under `name`: a handled kind of the catalogue, else a
    /// handled one of [`Self::extra_kinds`]. `UNSUPPORTED` for anything else.
    pub fn owner_kind(&self, name: &str) -> Result<OwnerCommandKind, AppError> {
        let refused = match accepted_owner_kind(name) {
            Ok(k) => return Ok(*k),
            Err(e) => e,
        };
        let extra = self
            .extra_kinds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match extra.iter().find(|k| k.kind == name && k.handled) {
            Some(k) if owner_command_kind(name).is_none() => Ok(*k),
            _ => Err(refused),
        }
    }

    /// Unix ms now.
    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// `HMAC-SHA256(pepper, "otp\0" ‖ challenge_id ‖ code)` as lowercase hex.
    pub fn code_hash(&self, challenge_id: &str, code: &str) -> String {
        hex(&self.code_mac(challenge_id, code).finalize().into_bytes())
    }

    fn code_mac(&self, challenge_id: &str, code: &str) -> HmacSha256 {
        let mut mac = self.mac(OTP_LABEL);
        mac.update(challenge_id.as_bytes());
        mac.update(code.as_bytes());
        mac
    }

    /// An HMAC keyed by the pepper, its input starting with `label` (domain separation
    /// between the code hash and the phone tag).
    fn mac(&self, label: &[u8]) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.pepper).expect("HMAC takes any key length");
        mac.update(label);
        mac
    }

    pub fn pairing_code_hash(&self, code: &str) -> String {
        let mut mac = self.mac(PAIR_LABEL);
        mac.update(arena_proto::normalize_pairing_code(code).as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    /// The signature of a setup-template download link (Story 5.38) over `data`, as hex.
    pub fn link_signature(&self, data: &str) -> String {
        let mut mac = self.mac(LINK_LABEL);
        mac.update(data.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    /// Whether `signature` (hex) signs `data`, in constant time.
    pub fn link_signed(&self, data: &str, signature: &str) -> bool {
        let Some(expected) = unhex(signature) else {
            return false;
        };
        let mut mac = self.mac(LINK_LABEL);
        mac.update(data.as_bytes());
        mac.verify_slice(&expected).is_ok()
    }

    /// Whether `code` matches `stored_hash` (hex) for `challenge_id`, in constant time.
    pub fn code_matches(&self, challenge_id: &str, code: &str, stored_hash: &str) -> bool {
        let Some(expected) = unhex(stored_hash) else {
            return false;
        };
        self.code_mac(challenge_id, code)
            .verify_slice(&expected)
            .is_ok()
    }

    /// The log stand-in for a phone: its last 2 digits and the first 8 hex characters of
    /// HMAC-SHA256(pepper, "tag\0" ‖ phone), e.g. `10:3fa2b9c1`.
    pub fn phone_tag(&self, phone_e164: &str) -> String {
        let mut mac = self.mac(TAG_LABEL);
        mac.update(phone_e164.as_bytes());
        let digest = hex(&mac.finalize().into_bytes());
        let digits: Vec<char> = phone_e164.chars().filter(char::is_ascii_digit).collect();
        let last2: String = digits[digits.len().saturating_sub(2)..].iter().collect();
        format!("{last2}:{}", &digest[..8])
    }

    /// The ledger's stand-in for a recipient: lowercase hex of SHA-256("notify\0" ‖ pepper ‖ phone).
    pub fn recipient_hash(&self, phone_e164: &str) -> String {
        let mut h = Sha256::new();
        h.update(NOTIFY_LABEL);
        h.update(&self.pepper);
        h.update(phone_e164.as_bytes());
        hex(&h.finalize())
    }

    pub fn email_tag(&self, email: &str) -> String {
        let mut mac = self.mac(TAG_LABEL);
        mac.update(b"email\0");
        mac.update(email.to_lowercase().as_bytes());
        let digest = hex(&mac.finalize().into_bytes());
        format!("email:{}", &digest[..8])
    }
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// SHA-256 of a token, lowercase hex: what `owner_sessions` stores.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// `n` bytes from the OS CSPRNG.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| format!("os rng: {e}"))?;
    Ok(b)
}

/// A fresh 6-digit code from the OS CSPRNG, uniform over `000000`–`999999` (rejection
/// sampling, no modulo bias).
pub fn new_code() -> Result<String, String> {
    const RANGE: u32 = 1_000_000;
    // The largest multiple of RANGE that fits in u32.
    const LIMIT: u32 = u32::MAX - (u32::MAX % RANGE);
    loop {
        let n = u32::from_le_bytes(random_bytes::<4>()?);
        if n < LIMIT {
            return Ok(format!("{:0width$}", n % RANGE, width = OTP_DIGITS));
        }
    }
}

/// A fresh opaque token: 32 bytes from the OS CSPRNG, lowercase hex.
pub fn new_token() -> Result<String, String> {
    Ok(hex(&random_bytes::<32>()?))
}

/// The longest name, café name or city.
pub const MAX_TEXT_CHARS: usize = 80;
/// The longest email.
pub const MAX_EMAIL_CHARS: usize = 254;

/// A rejected form field: its name and a plain message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: &'static str,
}

/// Whether `v` is a valid name, café name or city: 1–80 characters after trim, no control
/// characters. The rules match the onboarding app's (`tests/fixtures/signup-validation.json`).
pub fn valid_text(v: &str) -> bool {
    let t = v.trim();
    let n = t.chars().count();
    n > 0 && n <= MAX_TEXT_CHARS && !t.chars().any(char::is_control)
}

fn text_field(field: &'static str, v: &str, message: &'static str) -> Result<String, FieldError> {
    if !valid_text(v) {
        return Err(FieldError { field, message });
    }
    Ok(v.trim().to_owned())
}

/// Whether `p` is E.164: `+`, a non-zero digit, 8–15 digits in all; an Indian number (`+91`)
/// has exactly 10 digits after the country code, starting 6–9.
pub fn valid_phone(p: &str) -> bool {
    let Some(digits) = p.strip_prefix('+') else {
        return false;
    };
    if !(8..=15).contains(&digits.len())
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.starts_with('0')
    {
        return false;
    }
    match digits.strip_prefix("91") {
        Some(national) => national.len() == 10 && matches!(national.as_bytes()[0], b'6'..=b'9'),
        None => true,
    }
}

/// A pragmatic email check: at most 254 characters, one `@`, a local part of 1–64 characters
/// without whitespace or control characters, and a domain of 2+ dot-separated labels of 1–63
/// ASCII letters, digits or hyphens (no leading or trailing hyphen) ending in a TLD of 2+ ASCII
/// letters.
pub fn valid_email(e: &str) -> bool {
    if e.is_empty() || e.chars().count() > MAX_EMAIL_CHARS {
        return false;
    }
    let Some((local, domain)) = e.split_once('@') else {
        return false;
    };
    let local_len = local.chars().count();
    if local_len == 0
        || local_len > 64
        || local.chars().any(|c| c.is_whitespace() || c.is_control())
        || domain.contains('@')
    {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    let label_ok = |l: &&str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    if labels.len() < 2 || !labels.iter().all(label_ok) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())
}

/// Validates the "Get started" form, in field order, and returns the trimmed details.
pub fn validate_start(s: &SignupStart) -> Result<NewOwner, FieldError> {
    let name = text_field("name", &s.name, "Enter your name (up to 80 characters).")?;
    let phone = s.phone.trim();
    if !valid_phone(phone) {
        return Err(FieldError {
            field: "phone",
            message: "Enter a mobile number with its country code.",
        });
    }
    let email = s.email.trim();
    if !valid_email(email) {
        return Err(FieldError {
            field: "email",
            message: "Enter an email address like name@example.com.",
        });
    }
    let cafe_name = text_field(
        "cafe_name",
        &s.cafe_name,
        "Enter your café's name (up to 80 characters).",
    )?;
    let city = text_field("city", &s.city, "Enter your city (up to 80 characters).")?;
    Ok(NewOwner {
        name,
        phone_e164: phone.to_owned(),
        email: email.to_owned(),
        cafe_name,
        city,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::FakeSms;

    fn state(pepper: &str) -> AuthState {
        let clock: Clock = Arc::new(|| 0);
        let cfg = AuthConfig {
            pepper: pepper.as_bytes().to_vec(),
            web_origins: vec![],
            sms: crate::config::SmsProviderConfig::Fake,
            sms_global_per_min: 60,
            mfa_key: crate::crypto::MfaKey::from_bytes([0; 32]),
            licence: crate::licence::LicenceSigner::dev(),
            billing_enabled: false,
        };
        // A database these tests never touch.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = rt
            .block_on(ControlDb::open_local(&dir.path().join("x.db")))
            .unwrap();
        AuthState::new(
            Arc::new(db),
            Messenger::Fake(FakeSms::new(Arc::clone(&clock))),
            clock,
            &cfg,
        )
    }

    #[test]
    fn codes_are_six_digits_and_tokens_64_hex() {
        for _ in 0..200 {
            let c = new_code().unwrap();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
        let t = new_token().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, new_token().unwrap());
        assert_eq!(token_hash(&t).len(), 64);
    }

    #[test]
    fn the_code_hash_binds_the_challenge_and_the_pepper() {
        let a = state("pepper-a-0123456789abcdef0123456789");
        let b = state("pepper-b-0123456789abcdef0123456789");
        let h = a.code_hash("c1", "123456");
        assert!(a.code_matches("c1", "123456", &h));
        assert!(!a.code_matches("c1", "123457", &h));
        assert!(!a.code_matches("c2", "123456", &h));
        assert!(!b.code_matches("c1", "123456", &h));
        assert!(!a.code_matches("c1", "123456", "zz"));
        assert!(!h.contains("123456"));
    }

    #[test]
    fn phone_tag_shows_two_digits_and_a_peppered_hash() {
        let a = state("pepper-a-0123456789abcdef0123456789");
        let tag = a.phone_tag("+919876543210");
        assert!(tag.starts_with("10:") && tag.len() == 11, "{tag}");
        assert!(!tag.contains("98765"));
        assert_ne!(
            tag,
            state("pepper-b-0123456789abcdef0123456789").phone_tag("+919876543210")
        );
    }

    /// The vectors shared with the onboarding app.
    #[test]
    fn the_shared_validation_vectors_hold() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/signup-validation.json"
        );
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let pairs = |k: &str| {
            v[k].as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].clone(), p[1].as_bool().unwrap()))
                .collect::<Vec<_>>()
        };
        let mut n = 0;
        for (t, ok) in pairs("text") {
            assert_eq!(valid_text(t.as_str().unwrap()), ok, "text {t}");
            n += 1;
        }
        for (len, ok) in pairs("text_lengths") {
            let t = "x".repeat(len.as_u64().unwrap() as usize);
            assert_eq!(valid_text(&t), ok, "text of {len}");
            n += 1;
        }
        for (p, ok) in pairs("phone") {
            assert_eq!(valid_phone(p.as_str().unwrap()), ok, "phone {p}");
            n += 1;
        }
        for (e, ok) in pairs("email") {
            assert_eq!(valid_email(e.as_str().unwrap()), ok, "email {e}");
            n += 1;
        }
        assert!(n > 40, "{n} vectors");
    }

    #[test]
    fn the_form_is_trimmed_and_checked_in_order() {
        let ok = SignupStart {
            name: "  Asha  ".into(),
            phone: " +919876543210 ".into(),
            email: " asha@example.in ".into(),
            cafe_name: "Level Up".into(),
            city: "Pune".into(),
        };
        let o = validate_start(&ok).unwrap();
        assert_eq!(
            (o.name.as_str(), o.phone_e164.as_str(), o.email.as_str()),
            ("Asha", "+919876543210", "asha@example.in")
        );
        let field = |f: fn(&mut SignupStart)| {
            let mut s = ok.clone();
            f(&mut s);
            validate_start(&s).unwrap_err().field
        };
        assert_eq!(field(|s| s.name = "   ".into()), "name");
        assert_eq!(field(|s| s.name = "x".repeat(81)), "name");
        assert_eq!(field(|s| s.phone = "+91123".into()), "phone");
        assert_eq!(field(|s| s.email = "nope".into()), "email");
        assert_eq!(field(|s| s.cafe_name = String::new()), "cafe_name");
        assert_eq!(field(|s| s.city = "é".repeat(81)), "city");
        let mut s = ok.clone();
        s.city = "é".repeat(80);
        assert!(validate_start(&s).is_ok());
    }
}

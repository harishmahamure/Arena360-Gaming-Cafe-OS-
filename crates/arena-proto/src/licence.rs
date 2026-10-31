use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::price_book::BillingCycle;
use crate::tier::{Tier, TierLimits};

pub const LICENCE_TTL_S: i64 = 7 * 24 * 60 * 60;
pub const LICENCE_CLOCK_SKEW_S: i64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LicenceClaims {
    pub tenant_id: String,
    pub branch_id: String,
    pub tier: Tier,
    pub flags: Vec<String>,
    pub features: Vec<String>,
    pub upload_interval_s: u32,
    pub summary_interval_s: u32,
    pub rollup_schema: u32,
    pub retention_days: u32,
    pub limits: TierLimits,
    /// How the branch is billed, on a paid licence from a subscription (Story 10.6); absent on Free
    /// and comp licences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cycle: Option<BillingCycle>,
    #[ts(type = "number")]
    pub iat: i64,
    #[ts(type = "number")]
    pub exp: i64,
}

impl LicenceClaims {
    pub fn for_tier(tenant_id: &str, branch_id: &str, tier: Tier, iat_s: i64) -> Self {
        let d = tier.defaults();
        Self {
            tenant_id: tenant_id.to_owned(),
            branch_id: branch_id.to_owned(),
            tier,
            flags: d.flags.iter().map(|f| (*f).to_owned()).collect(),
            features: d.features.iter().map(|f| (*f).to_owned()).collect(),
            upload_interval_s: d.upload_interval_s,
            summary_interval_s: d.summary_interval_s,
            rollup_schema: d.rollup_schema,
            retention_days: d.retention_days,
            limits: d.limits,
            cycle: None,
            iat: iat_s,
            exp: iat_s + LICENCE_TTL_S,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyKey<'a> {
    pub kid: &'a str,
    pub public_key: &'a str,
}

pub const LICENCE_VERIFY_KEYS: &[VerifyKey<'static>] = &[];

const _: () = assert!(LICENCE_VERIFY_KEYS.len() <= 2);

#[cfg(any(test, feature = "dev-keys"))]
pub const DEV_LICENCE_KID: &str = "arena-dev-licence-1";

#[cfg(any(test, feature = "dev-keys"))]
pub const DEV_LICENCE_VERIFY_KEYS: &[VerifyKey<'static>] = &[VerifyKey {
    kid: DEV_LICENCE_KID,
    public_key: "AgGeVhPGCGq0I3VTItyqjQKbxNFhvxrr2Mea5fU_DEc",
}];

#[cfg(any(test, feature = "dev-keys"))]
const DEV_SIGNING_KEY_PEM: &str =
    include_str!("../../arena-cloud/dev/fixtures/licence-dev-signing-key.pem");

#[cfg(any(test, feature = "dev-keys"))]
pub fn dev_sign(claims: &LicenceClaims) -> Result<String, String> {
    dev_sign_any(claims)
}

/// Signs any claims with the dev key (tests and the dev cloud).
#[cfg(any(test, feature = "dev-keys"))]
pub fn dev_sign_any<T: Serialize>(claims: &T) -> Result<String, String> {
    use base64::engine::general_purpose::STANDARD;
    let body: String = DEV_SIGNING_KEY_PEM
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der = STANDARD
        .decode(body.trim())
        .map_err(|e| format!("dev key: {e}"))?;
    let mut header = jsonwebtoken::Header::new(Algorithm::EdDSA);
    header.kid = Some(DEV_LICENCE_KID.to_owned());
    jsonwebtoken::encode(
        &header,
        claims,
        &jsonwebtoken::EncodingKey::from_ed_der(&der),
    )
    .map_err(|e| format!("dev licence sign: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenceError {
    NoKeys,
    Malformed,
    UnknownKid,
    BadKey,
    BadSignature,
    Expired,
    NotYetValid,
}

impl LicenceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoKeys => "LICENCE_NO_KEYS",
            Self::Malformed => "LICENCE_MALFORMED",
            Self::UnknownKid => "LICENCE_UNKNOWN_KID",
            Self::BadKey => "LICENCE_BAD_KEY",
            Self::BadSignature => "LICENCE_BAD_SIGNATURE",
            Self::Expired => "LICENCE_EXPIRED",
            Self::NotYetValid => "LICENCE_NOT_YET_VALID",
        }
    }
}

impl std::fmt::Display for LicenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            Self::NoKeys => "no licence verify keys",
            Self::Malformed => "the licence is not a well-formed EdDSA token",
            Self::UnknownKid => "the licence was signed with a key this build doesn't trust",
            Self::BadKey => "a licence verify key is not a 32-byte Ed25519 public key",
            Self::BadSignature => "the licence signature doesn't match its contents",
            Self::Expired => "the licence has expired",
            Self::NotYetValid => "the licence was issued in the future",
        };
        write!(f, "{}: {what}", self.code())
    }
}

impl std::error::Error for LicenceError {}

pub fn public_key_bytes(public_key: &str) -> Option<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(public_key)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
}

pub fn encode_public_key(bytes: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn verify_claims(token: &str, keys: &[VerifyKey<'_>]) -> Result<LicenceClaims, LicenceError> {
    let claims: LicenceClaims = decode_signed(token, keys)?;
    match claims.exp.checked_sub(claims.iat) {
        Some(life) if life > 0 && life <= LICENCE_TTL_S => Ok(claims),
        _ => Err(LicenceError::Malformed),
    }
}

/// Checks the EdDSA signature of `token` against `keys` (by `kid`) and decodes its claims as
/// `T`. Time is not checked; the caller applies its own rules (`exp` must be present).
pub(crate) fn decode_signed<T: serde::de::DeserializeOwned>(
    token: &str,
    keys: &[VerifyKey<'_>],
) -> Result<T, LicenceError> {
    if keys.is_empty() {
        return Err(LicenceError::NoKeys);
    }
    let header = jsonwebtoken::decode_header(token).map_err(|_| LicenceError::Malformed)?;
    if header.alg != Algorithm::EdDSA {
        return Err(LicenceError::Malformed);
    }
    let kid = header.kid.ok_or(LicenceError::UnknownKid)?;
    let key = keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or(LicenceError::UnknownKid)?;
    let raw = public_key_bytes(key.public_key).ok_or(LicenceError::BadKey)?;
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.set_required_spec_claims(&["exp"]);
    let data = jsonwebtoken::decode::<T>(token, &DecodingKey::from_ed_der(&raw), &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::InvalidSignature => LicenceError::BadSignature,
            _ => LicenceError::Malformed,
        })?;
    Ok(data.claims)
}

pub fn check_time(claims: &LicenceClaims, now_ms: i64) -> Result<(), LicenceError> {
    let now_s = now_ms.div_euclid(1000);
    if claims.iat > now_s.saturating_add(LICENCE_CLOCK_SKEW_S) {
        return Err(LicenceError::NotYetValid);
    }
    if now_s >= claims.exp {
        return Err(LicenceError::Expired);
    }
    Ok(())
}

pub fn verify(
    token: &str,
    keys: &[VerifyKey<'_>],
    now_ms: i64,
) -> Result<LicenceClaims, LicenceError> {
    let claims = verify_claims(token, keys)?;
    check_time(&claims, now_ms)?;
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dev_signer_signs_what_the_dev_verify_set_trusts() {
        let claims = LicenceClaims::for_tier("t", "b", Tier::Free, 1_800_000_000);
        let token = dev_sign(&claims).unwrap();
        assert_eq!(
            verify(&token, DEV_LICENCE_VERIFY_KEYS, 1_800_000_000_000).unwrap(),
            claims
        );
        assert!(verify(&token, LICENCE_VERIFY_KEYS, 1_800_000_000_000).is_err());
    }

    #[test]
    fn free_claims_take_the_tier_defaults_and_a_seven_day_life() {
        let c = LicenceClaims::for_tier("t1", "b1", Tier::Free, 1_000);
        assert_eq!(c.limits.devices, 20);
        assert_eq!(c.limits.users, 3);
        assert_eq!(c.limits.branches, 1);
        assert!(c.flags.is_empty() && c.features.is_empty());
        assert_eq!((c.upload_interval_s, c.summary_interval_s), (0, 0));
        assert_eq!(c.exp, 1_000 + 7 * 86_400);
    }

    #[test]
    fn the_production_set_ships_empty_and_refuses_every_token() {
        assert!(LICENCE_VERIFY_KEYS.is_empty());
        let e = verify("anything", LICENCE_VERIFY_KEYS, 0).unwrap_err();
        assert_eq!(e, LicenceError::NoKeys);
        assert!(e.to_string().contains("no licence verify keys"));
    }

    #[test]
    fn the_dev_key_is_a_valid_public_key() {
        for k in DEV_LICENCE_VERIFY_KEYS {
            assert!(public_key_bytes(k.public_key).is_some(), "{}", k.kid);
        }
        assert_eq!(public_key_bytes("short"), None);
    }

    #[test]
    fn garbage_is_malformed_and_a_bad_slot_is_reported() {
        assert_eq!(
            verify("not.a.token", DEV_LICENCE_VERIFY_KEYS, 0).unwrap_err(),
            LicenceError::Malformed
        );
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"x"}"#);
        let token = format!("{header}.e30.AAAA");
        let bad = [VerifyKey {
            kid: "x",
            public_key: "AAAA",
        }];
        assert_eq!(verify(&token, &bad, 0).unwrap_err(), LicenceError::BadKey);
    }
}

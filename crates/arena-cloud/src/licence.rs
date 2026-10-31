use arena_proto::licence::{encode_public_key, verify};
use arena_proto::{BillingCycle, EntitlementSource, LicenceClaims, Tier, VerifyKey};
use arena_store::control::{Branch, Entitlement};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use sha2::{Digest, Sha256};

pub const DEV_LICENCE_KID: &str = "arena-dev-licence-1";
pub const DEV_SIGNING_KEY_PEM: &str = include_str!("../dev/fixtures/licence-dev-signing-key.pem");

const PKCS8_V1_ED25519_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];
const PEM_LABEL: &str = "PRIVATE KEY";
const KID_MAX: usize = 64;
const PROBE_IAT_S: i64 = 1_000_000_000;

#[derive(Clone)]
pub struct LicenceSigner {
    kid: String,
    key: EncodingKey,
    public_key: [u8; 32],
}

impl PartialEq for LicenceSigner {
    fn eq(&self, other: &Self) -> bool {
        self.kid == other.kid && self.public_key == other.public_key
    }
}

impl Eq for LicenceSigner {}

impl std::fmt::Debug for LicenceSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LicenceSigner")
            .field("kid", &self.kid)
            .field("public_key", &self.public_key())
            .field("private_key", &"[redacted]")
            .finish()
    }
}

fn pem_der(pem: &str) -> Result<Vec<u8>, String> {
    let begin = format!("-----BEGIN {PEM_LABEL}-----");
    let end = format!("-----END {PEM_LABEL}-----");
    let pem = pem.replace("\\n", "\n");
    let body = pem
        .trim()
        .strip_prefix(&begin)
        .and_then(|rest| rest.trim_end().strip_suffix(&end))
        .ok_or("is not a PKCS#8 PEM private key")?;
    let b64: String = body.split_whitespace().collect();
    STANDARD
        .decode(b64)
        .map_err(|_| "is not valid PEM base64".to_owned())
}

pub fn valid_kid(kid: &str) -> bool {
    !kid.is_empty()
        && kid.len() <= KID_MAX
        && kid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl LicenceSigner {
    pub fn from_pem(kid: &str, pem: &str) -> Result<Self, String> {
        if !valid_kid(kid) {
            return Err(format!(
                "kid must be 1 to {KID_MAX} letters, digits, '-', '_' or '.'"
            ));
        }
        let der = pem_der(pem)?;
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der)
            .map_err(|_| "is not an Ed25519 PKCS#8 private key".to_owned())?;
        let public_key: [u8; 32] = ring::signature::KeyPair::public_key(&pair)
            .as_ref()
            .try_into()
            .map_err(|_| "has an unexpected public key length".to_owned())?;
        let signer = Self {
            kid: kid.to_owned(),
            key: EncodingKey::from_ed_der(&der),
            public_key,
        };
        signer.self_test()?;
        Ok(signer)
    }

    pub fn self_test(&self) -> Result<(), String> {
        let probe = LicenceClaims::for_tier("self-test", "self-test", Tier::Free, PROBE_IAT_S);
        let token = self
            .sign(&probe)
            .map_err(|e| format!("failed its self-test: {e}"))?;
        let public_key = self.public_key();
        let keys = [VerifyKey {
            kid: &self.kid,
            public_key: &public_key,
        }];
        match verify(&token, &keys, PROBE_IAT_S * 1000) {
            Ok(claims) if claims == probe => Ok(()),
            Ok(_) => Err("failed its self-test: the claims changed".to_owned()),
            Err(e) => Err(format!("failed its self-test: {e}")),
        }
    }

    pub fn trusted_by(&self, keys: &[VerifyKey<'_>]) -> bool {
        let public_key = self.public_key();
        keys.iter()
            .any(|k| k.kid == self.kid && k.public_key == public_key)
    }

    pub fn dev() -> Self {
        Self::from_pem(DEV_LICENCE_KID, DEV_SIGNING_KEY_PEM).expect("the committed dev key parses")
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn public_key(&self) -> String {
        encode_public_key(&self.public_key)
    }

    pub fn is_dev_key(&self) -> bool {
        self.public_key == Self::dev().public_key
    }

    pub fn sign(&self, claims: &LicenceClaims) -> Result<String, String> {
        self.sign_any(claims)
    }

    /// Signs a live view token (Story 8.40) with the licence key: the Hub trusts one key set for
    /// both, and tells them apart by their claims.
    pub fn sign_live(&self, claims: &arena_proto::LiveClaims) -> Result<String, String> {
        self.sign_any(claims)
    }

    fn sign_any<T: serde::Serialize>(&self, claims: &T) -> Result<String, String> {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, claims, &self.key).map_err(|e| format!("licence sign: {e}"))
    }
}

/// Adds what a paid entitlement's add-ons give (Stories 10.3, 10.6): `devices_20:<n>` raises the
/// device limit by 20 a pack and `tunnel` adds the tunnel and live view flags. Names it does not know change nothing.
fn apply_addons(claims: &mut LicenceClaims, addons: &[String]) {
    for addon in addons {
        match addon.split_once(':') {
            Some(("devices_20", n)) => {
                let packs = n.parse::<u32>().unwrap_or(0).min(10);
                claims.limits.devices = claims.limits.devices.saturating_add(20 * packs);
            }
            None if addon == "tunnel" => {
                for flag in ["tunnel", "live_view"] {
                    if !claims.flags.iter().any(|f| f == flag) {
                        claims.flags.push(flag.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn claims_for(
    tenant_id: &str,
    branch_id: &str,
    entitlement: Option<&Entitlement>,
    now_ms: i64,
) -> LicenceClaims {
    let tier = entitlement.map_or(Tier::Free, |e| e.tier_at(now_ms));
    let mut claims = LicenceClaims::for_tier(tenant_id, branch_id, tier, now_ms.div_euclid(1000));
    if tier != Tier::Free
        && let Some(e) = entitlement
    {
        apply_addons(&mut claims, &e.addons);
    }
    if tier != Tier::Free
        && let Some(until) = entitlement.and_then(|e| e.until)
    {
        claims.exp = claims.exp.min(until.div_euclid(1000)).max(claims.iat + 1);
    }
    claims
}

/// Records how a paid licence is billed (Story 10.6). Free, comp and unknown cycles stay absent.
pub fn with_cycle(
    mut claims: LicenceClaims,
    entitlement: Option<&Entitlement>,
    cycle: Option<BillingCycle>,
) -> LicenceClaims {
    let paid = claims.tier != Tier::Free
        && entitlement.is_some_and(|e| e.source == EntitlementSource::Paid);
    claims.cycle = cycle.filter(|_| paid);
    claims
}

/// [`claims_for`] plus the billing cycle of the branch's subscription (Story 10.6), for the licences
/// a Hub shows a plan from: the heartbeat, pairing and the standby's licence.
pub async fn claims_with_plan(
    db: &arena_store::control::ControlDb,
    tenant_id: &str,
    branch_id: &str,
    entitlement: Option<&Entitlement>,
    now_ms: i64,
) -> Result<LicenceClaims, arena_store::control::ControlError> {
    let claims = claims_for(tenant_id, branch_id, entitlement, now_ms);
    if claims.tier == Tier::Free {
        return Ok(claims);
    }
    let cycle = db.subscription_of_branch(branch_id).await?.map(|s| s.cycle);
    Ok(with_cycle(claims, entitlement, cycle))
}

pub fn sign_branch(
    signer: &LicenceSigner,
    branch: &Branch,
    entitlement: Option<&Entitlement>,
    now_ms: i64,
) -> Result<String, String> {
    signer.sign(&claims_for(
        &branch.tenant_id,
        &branch.id,
        entitlement,
        now_ms,
    ))
}

pub fn kid_for(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    format!("lk-{}", crate::auth::hex(&digest[..6]))
}

#[derive(Clone, PartialEq, Eq)]
pub struct GeneratedKey {
    pub kid: String,
    pub public_key: String,
    pub pem: String,
}

impl std::fmt::Debug for GeneratedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedKey")
            .field("kid", &self.kid)
            .field("public_key", &self.public_key)
            .field("pem", &"[redacted]")
            .finish()
    }
}

pub fn generate(kid: Option<&str>) -> Result<GeneratedKey, String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("os rng: {e}"))?;
    let mut der = PKCS8_V1_ED25519_PREFIX.to_vec();
    der.extend_from_slice(&seed);
    let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed)
        .map_err(|_| "could not derive the public key".to_owned())?;
    let public: [u8; 32] = ring::signature::KeyPair::public_key(&pair)
        .as_ref()
        .try_into()
        .map_err(|_| "unexpected public key length".to_owned())?;
    let kid = match kid {
        Some(k) if valid_kid(k) => k.to_owned(),
        Some(_) => {
            return Err(format!(
                "--kid must be 1 to {KID_MAX} letters, digits, '-', '_' or '.'"
            ));
        }
        None => kid_for(&public),
    };
    let b64 = STANDARD.encode(&der);
    let mut pem = format!("-----BEGIN {PEM_LABEL}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {PEM_LABEL}-----\n"));
    Ok(GeneratedKey {
        kid,
        public_key: encode_public_key(&public),
        pem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_key_round_trips_through_its_pem() {
        let g = generate(None).unwrap();
        assert!(g.kid.starts_with("lk-") && g.kid.len() == 15, "{}", g.kid);
        let s = LicenceSigner::from_pem(&g.kid, &g.pem).unwrap();
        assert_eq!(s.public_key(), g.public_key);
        assert!(!s.is_dev_key());
        assert!(!format!("{g:?}").contains(&g.pem));
        assert!(!format!("{s:?}").contains("BEGIN"));
        let escaped = g.pem.replace('\n', "\\n");
        assert_eq!(
            LicenceSigner::from_pem(&g.kid, &escaped)
                .unwrap()
                .public_key(),
            g.public_key
        );
        assert_eq!(generate(Some("next-2027")).unwrap().kid, "next-2027");
        assert!(generate(Some("bad kid")).is_err());
    }

    #[test]
    fn bad_pems_and_kids_are_refused() {
        assert!(LicenceSigner::from_pem("k", "nonsense").is_err());
        assert!(LicenceSigner::from_pem("", DEV_SIGNING_KEY_PEM).is_err());
        assert!(LicenceSigner::from_pem("k k", DEV_SIGNING_KEY_PEM).is_err());
        let truncated = DEV_SIGNING_KEY_PEM.replace("MC4C", "MC5C");
        assert!(LicenceSigner::from_pem("k", &truncated).is_err());
        assert!(LicenceSigner::dev().is_dev_key());
    }

    #[test]
    fn the_self_test_catches_a_signer_whose_public_key_does_not_match() {
        let dev = LicenceSigner::dev();
        assert!(dev.self_test().is_ok());
        let other = generate(Some("other")).unwrap();
        let other = LicenceSigner::from_pem("other", &other.pem).unwrap();
        let mismatched = LicenceSigner {
            public_key: other.public_key,
            ..dev.clone()
        };
        let e = mismatched.self_test().unwrap_err();
        assert!(e.contains("self-test"), "{e}");
        let broken = LicenceSigner {
            key: EncodingKey::from_ed_der(&[0x30, 0x00]),
            ..dev.clone()
        };
        assert!(broken.self_test().unwrap_err().contains("self-test"));
    }

    #[test]
    fn trust_needs_the_kid_and_the_public_key_in_the_set() {
        let dev = LicenceSigner::dev();
        let pk = dev.public_key();
        assert!(dev.trusted_by(&[VerifyKey {
            kid: DEV_LICENCE_KID,
            public_key: &pk
        }]));
        assert!(!dev.trusted_by(&[VerifyKey {
            kid: "other",
            public_key: &pk
        }]));
        assert!(!dev.trusted_by(arena_proto::licence::LICENCE_VERIFY_KEYS));
    }
}

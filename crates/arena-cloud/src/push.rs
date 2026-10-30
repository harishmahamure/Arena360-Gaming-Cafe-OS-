//! Web push for owner alerts (Story 8.33): a [`WebPush`] sender, either the recorded
//! [`FakePush`] (dev and tests) or [`HttpPush`], which posts to the browser's push service with a
//! VAPID JWT (ES256, RFC 8292) and the payload encrypted as `aes128gcm` (RFC 8291). Logs and
//! errors carry no endpoint, key or payload.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use arena_proto::PUSH_TTL_S;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::PublicKey;
use p256::SecretKey;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use serde::Serialize;
use sha2::Sha256;

use crate::Clock;
use crate::config::get_env;

/// The VAPID private key: PKCS#8 DER, base64url (a secret: never logged).
pub const VAPID_PRIVATE_KEY_ENV: &str = "ARENA_VAPID_PRIVATE_KEY";
/// The VAPID `sub` claim: a `mailto:` or `https:` contact for the push services.
pub const VAPID_SUBJECT_ENV: &str = "ARENA_VAPID_SUBJECT";

/// How long one push post may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a VAPID token stays valid (the limit is 24 h).
const JWT_TTL_S: i64 = 12 * 60 * 60;
/// The record size announced in the body; the payload always fits one record.
const RECORD_SIZE: u32 = 4096;
/// The longest payload that fits one record with its padding delimiter and the AEAD tag.
pub const MAX_PAYLOAD: usize = RECORD_SIZE as usize - 1 - 16 - 16 - 1 - 65 - 4 - 1;

/// A fixed key for the recorded fake, so the Owner app can subscribe against `--dev`.
const DEV_VAPID_SECRET: [u8; 32] = *b"arena-dev-vapid-key-not-for-prod";

/// Where one push goes: a browser's subscription.
#[derive(Clone, Copy)]
pub struct PushTarget<'a> {
    pub endpoint: &'a str,
    /// The browser's public key, base64url (65 bytes uncompressed).
    pub p256dh: &'a str,
    /// The browser's auth secret, base64url (16 bytes).
    pub auth: &'a str,
}

/// What the push service did with a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    /// The service accepted it.
    Delivered,
    /// The subscription no longer exists (404 or 410): delete it.
    Gone,
    /// Anything else. The code is stable and carries no endpoint.
    Failed(String),
}

/// A push the fake recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentPush {
    pub endpoint: String,
    /// The plain payload (what the service worker reads).
    pub payload: String,
    /// Unix ms.
    pub at: i64,
}

/// The recorded fake: captures what would be pushed, with a per-endpoint outcome; nothing leaves
/// the process.
pub struct FakePush {
    sent: Mutex<Vec<SentPush>>,
    outcomes: Mutex<HashMap<String, PushOutcome>>,
    clock: Clock,
    public_key: String,
}

impl FakePush {
    pub fn new(clock: Clock) -> Self {
        let key = SecretKey::from_slice(&DEV_VAPID_SECRET).expect("the dev key is a valid scalar");
        Self {
            sent: Mutex::new(Vec::new()),
            outcomes: Mutex::new(HashMap::new()),
            clock,
            public_key: URL_SAFE_NO_PAD.encode(public_bytes(&key)),
        }
    }

    /// Every push accepted so far, oldest first. A push whose outcome was set to something other
    /// than `Delivered` is not recorded.
    pub fn sent(&self) -> Vec<SentPush> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Sets what later pushes to `endpoint` do.
    pub fn set_outcome(&self, endpoint: &str, outcome: PushOutcome) {
        if let Ok(mut o) = self.outcomes.lock() {
            o.insert(endpoint.to_owned(), outcome);
        }
    }

    fn send(&self, target: &PushTarget<'_>, payload: &[u8]) -> PushOutcome {
        let outcome = self
            .outcomes
            .lock()
            .ok()
            .and_then(|o| o.get(target.endpoint).cloned())
            .unwrap_or(PushOutcome::Delivered);
        if outcome == PushOutcome::Delivered
            && let Ok(mut sent) = self.sent.lock()
        {
            sent.push(SentPush {
                endpoint: target.endpoint.to_owned(),
                payload: String::from_utf8_lossy(payload).into_owned(),
                at: (self.clock)(),
            });
        }
        outcome
    }
}

/// The real sender.
pub struct HttpPush {
    signing: SigningKey,
    public_key: String,
    subject: String,
    client: reqwest::Client,
    clock: Clock,
}

impl HttpPush {
    /// `private_key_b64` is the PKCS#8 DER of a P-256 key, base64url.
    pub fn new(private_key_b64: &str, subject: &str, clock: Clock) -> Result<Self, String> {
        let der = URL_SAFE_NO_PAD
            .decode(private_key_b64.trim().trim_end_matches('='))
            .map_err(|_| "it is not base64url".to_owned())?;
        let secret = SecretKey::from_pkcs8_der(&der)
            .map_err(|_| "it is not a PKCS#8 P-256 private key".to_owned())?;
        if !(subject.starts_with("mailto:") || subject.starts_with("https://")) {
            return Err("the subject must be a mailto: or https:// contact".to_owned());
        }
        Ok(Self {
            public_key: URL_SAFE_NO_PAD.encode(public_bytes(&secret)),
            signing: SigningKey::from(&secret),
            subject: subject.to_owned(),
            client: crate::alert::client_builder(SEND_TIMEOUT)?
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.without_url().to_string())?,
            clock,
        })
    }

    /// The `Authorization: vapid t=…, k=…` header value for a push service at `audience`.
    fn authorization(&self, audience: &str) -> Result<String, String> {
        let exp = (self.clock)() / 1000 + JWT_TTL_S;
        let jwt = vapid_jwt(&self.signing, audience, &self.subject, exp)?;
        Ok(format!("vapid t={jwt}, k={}", self.public_key))
    }

    async fn send(&self, target: &PushTarget<'_>, payload: &[u8]) -> PushOutcome {
        let failed = |code: &str| PushOutcome::Failed(code.to_owned());
        let Ok(url) = reqwest::Url::parse(target.endpoint) else {
            return failed("PUSH_ENDPOINT_INVALID");
        };
        if url.scheme() != "https" {
            return failed("PUSH_ENDPOINT_INVALID");
        }
        let audience = url.origin().ascii_serialization();
        let (Ok(ua_public), Ok(auth)) = (
            URL_SAFE_NO_PAD.decode(target.p256dh),
            URL_SAFE_NO_PAD.decode(target.auth),
        ) else {
            return failed("PUSH_KEYS_INVALID");
        };
        let Ok(body) = encrypt(payload, &ua_public, &auth) else {
            return failed("PUSH_KEYS_INVALID");
        };
        let Ok(authorization) = self.authorization(&audience) else {
            return failed("PUSH_SIGN_FAILED");
        };
        let res = self
            .client
            .post(url)
            .header("Authorization", authorization)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", PUSH_TTL_S.to_string())
            .header("Urgency", "high")
            .body(body)
            .send()
            .await;
        match res {
            Ok(r) => match r.status().as_u16() {
                200..=299 => PushOutcome::Delivered,
                404 | 410 => PushOutcome::Gone,
                other => PushOutcome::Failed(format!("PUSH_HTTP_{other}")),
            },
            Err(e) if e.is_timeout() => failed("PUSH_TIMEOUT"),
            Err(_) => failed("PUSH_UNREACHABLE"),
        }
    }
}

/// The configured sender.
pub enum WebPush {
    Fake(FakePush),
    Http(HttpPush),
}

impl WebPush {
    /// The recorded fake, when that is the sender (tests and `--dev`).
    pub fn fake(&self) -> Option<&FakePush> {
        match self {
            Self::Fake(f) => Some(f),
            Self::Http(_) => None,
        }
    }

    /// The VAPID public key (base64url) a browser subscribes with.
    pub fn public_key(&self) -> &str {
        match self {
            Self::Fake(f) => &f.public_key,
            Self::Http(h) => &h.public_key,
        }
    }

    /// Pushes `payload` to `target`.
    pub async fn send(&self, target: &PushTarget<'_>, payload: &[u8]) -> PushOutcome {
        match self {
            Self::Fake(f) => f.send(target, payload),
            Self::Http(h) => h.send(target, payload).await,
        }
    }

    /// The sender `ARENA_VAPID_PRIVATE_KEY` and `ARENA_VAPID_SUBJECT` describe; `None` without a
    /// key (push is then unavailable). Errors name the variable, never the key.
    pub fn from_env(
        env: &impl Fn(&str) -> Option<OsString>,
        clock: Clock,
    ) -> Result<Option<Self>, String> {
        let Some(key) = get_env(env, VAPID_PRIVATE_KEY_ENV).filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        let subject = get_env(env, VAPID_SUBJECT_ENV)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                format!("{VAPID_SUBJECT_ENV} is not set ({VAPID_PRIVATE_KEY_ENV} is)")
            })?;
        HttpPush::new(&key, &subject, clock)
            .map(|h| Some(Self::Http(h)))
            .map_err(|e| format!("{VAPID_PRIVATE_KEY_ENV} / {VAPID_SUBJECT_ENV}: {e}"))
    }
}

/// The uncompressed SEC1 form of a key's public point (65 bytes).
fn public_bytes(secret: &SecretKey) -> Vec<u8> {
    secret
        .public_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec()
}

/// A VAPID token: `base64url(header).base64url(claims).base64url(signature)`.
fn vapid_jwt(key: &SigningKey, audience: &str, subject: &str, exp: i64) -> Result<String, String> {
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = serde_json::json!({ "aud": audience, "exp": exp, "sub": subject });
    let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
    let signing_input = format!("{header}.{claims}");
    let sig: Signature = key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(sig.to_bytes())
    ))
}

/// Encrypts `plaintext` for the browser with public key `ua_public` and auth secret `auth`
/// (RFC 8291, `aes128gcm`), with a fresh key pair and salt.
pub fn encrypt(plaintext: &[u8], ua_public: &[u8], auth: &[u8]) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| format!("os rng: {e}"))?;
    let secret = loop {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| format!("os rng: {e}"))?;
        // A random 32 bytes is a valid scalar except with negligible probability.
        if let Ok(k) = SecretKey::from_slice(&bytes) {
            break k;
        }
    };
    encrypt_with(plaintext, ua_public, auth, &secret, &salt)
}

/// [`encrypt`] with the sender's key pair and the salt given (the RFC's test vector fixes them).
fn encrypt_with(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    as_secret: &SecretKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>, String> {
    if plaintext.len() > MAX_PAYLOAD {
        return Err("the payload is too large for one record".to_owned());
    }
    if auth.len() != 16 {
        return Err("the auth secret is not 16 bytes".to_owned());
    }
    let ua = PublicKey::from_sec1_bytes(ua_public)
        .map_err(|_| "the browser key is not a P-256 point".to_owned())?;
    if ua_public.len() != 65 {
        return Err("the browser key is not uncompressed".to_owned());
    }
    let as_public = public_bytes(as_secret);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua.as_affine());

    // IKM = HKDF(salt = auth, ikm = ecdh) expanded with "WebPush: info\0" ‖ ua ‖ as.
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(&as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .map_err(|_| "hkdf".to_owned())?;

    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| "hkdf".to_owned())?;
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| "hkdf".to_owned())?;

    // One record: the payload then the 0x02 delimiter of the last record.
    let mut record = plaintext.to_vec();
    record.push(0x02);
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| "aes key".to_owned())?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), record.as_slice())
        .map_err(|_| "aes-gcm".to_owned())?;

    // Header: salt ‖ record size ‖ key id length ‖ key id (the sender's public key).
    let mut body = Vec::with_capacity(21 + as_public.len() + sealed.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(&as_public);
    body.extend_from_slice(&sealed);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn b64(s: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(s).unwrap()
    }

    // RFC 8291, section 5 and appendix A.
    const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
    const UA_PUBLIC: &str =
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
    const AUTH_SECRET: &str = "BTBZMqHH6r4Tts7J_aSIgg";
    const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
    const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
    const BODY: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";
    const PLAINTEXT: &str = "When I grow up, I want to be a watermelon";

    /// Decrypts an `aes128gcm` body as the browser would (the reverse of RFC 8291 section 3.4).
    fn decrypt(body: &[u8], ua_secret: &SecretKey, auth: &[u8]) -> Option<Vec<u8>> {
        let salt = &body[..16];
        let id_len = usize::from(body[20]);
        let as_public = &body[21..21 + id_len];
        let sealed = &body[21 + id_len..];
        let as_key = PublicKey::from_sec1_bytes(as_public).ok()?;
        let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), as_key.as_affine());
        let ua_public = public_bytes(ua_secret);
        let mut key_info = b"WebPush: info\0".to_vec();
        key_info.extend_from_slice(&ua_public);
        key_info.extend_from_slice(as_public);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
            .expand(&key_info, &mut ikm)
            .ok()?;
        let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let mut cek = [0u8; 16];
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
            .ok()?;
        let mut nonce = [0u8; 12];
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
        let mut plain = Aes128Gcm::new_from_slice(&cek)
            .ok()?
            .decrypt(Nonce::from_slice(&nonce), sealed)
            .ok()?;
        assert_eq!(plain.pop(), Some(0x02), "the last record ends with 0x02");
        Some(plain)
    }

    #[test]
    fn the_rfc_8291_vector_decrypts_and_encrypts_to_the_same_bytes() {
        let ua_secret = SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap();
        assert_eq!(public_bytes(&ua_secret), b64(UA_PUBLIC));
        let plain = decrypt(&b64(BODY), &ua_secret, &b64(AUTH_SECRET))
            .expect("the RFC's message authenticates");
        assert_eq!(String::from_utf8(plain.clone()).unwrap(), PLAINTEXT);

        let as_secret = SecretKey::from_slice(&b64(AS_PRIVATE)).unwrap();
        let salt: [u8; 16] = b64(SALT).try_into().unwrap();
        let body = encrypt_with(
            &plain,
            &b64(UA_PUBLIC),
            &b64(AUTH_SECRET),
            &as_secret,
            &salt,
        )
        .unwrap();
        assert_eq!(URL_SAFE_NO_PAD.encode(body), BODY);
    }

    #[test]
    fn a_fresh_encryption_round_trips_and_differs_each_time() {
        let ua_secret = SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap();
        let auth = b64(AUTH_SECRET);
        let a = encrypt(b"hello", &b64(UA_PUBLIC), &auth).unwrap();
        let b = encrypt(b"hello", &b64(UA_PUBLIC), &auth).unwrap();
        assert_ne!(a, b);
        assert_eq!(decrypt(&a, &ua_secret, &auth).unwrap(), b"hello");
        assert_eq!(&a[16..20], &4096u32.to_be_bytes());
        assert_eq!(a[20], 65);
    }

    #[test]
    fn bad_keys_and_big_payloads_are_refused() {
        let auth = b64(AUTH_SECRET);
        assert!(encrypt(b"x", &[4; 65], &auth).is_err());
        assert!(encrypt(b"x", &b64(UA_PUBLIC), &auth[..15]).is_err());
        assert!(encrypt(&vec![0; MAX_PAYLOAD + 1], &b64(UA_PUBLIC), &auth).is_err());
        assert!(encrypt(&vec![0; MAX_PAYLOAD], &b64(UA_PUBLIC), &auth).is_ok());
    }

    #[test]
    fn the_vapid_token_verifies_against_the_public_key() {
        use p256::ecdsa::VerifyingKey;
        use p256::ecdsa::signature::Verifier;
        let clock: Clock = Arc::new(|| 1_800_000_000_000);
        let secret = SecretKey::from_slice(&DEV_VAPID_SECRET).unwrap();
        let der = {
            use p256::pkcs8::EncodePrivateKey;
            URL_SAFE_NO_PAD.encode(secret.to_pkcs8_der().unwrap().as_bytes())
        };
        let push = HttpPush::new(&der, "mailto:ops@arena360.cloud", clock).unwrap();
        let header = push.authorization("https://push.example").unwrap();
        let rest = header.strip_prefix("vapid t=").unwrap();
        let (jwt, k) = rest.split_once(", k=").unwrap();
        assert_eq!(k, push.public_key);
        let mut parts = jwt.split('.');
        let (h, c, s) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        assert_eq!(b64(h), br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims: serde_json::Value = serde_json::from_slice(&b64(c)).unwrap();
        assert_eq!(claims["aud"], "https://push.example");
        assert_eq!(claims["sub"], "mailto:ops@arena360.cloud");
        assert_eq!(claims["exp"], 1_800_000_000 + JWT_TTL_S);
        let verifying = VerifyingKey::from_sec1_bytes(&b64(k)).unwrap();
        let sig = Signature::from_slice(&b64(s)).unwrap();
        verifying
            .verify(format!("{h}.{c}").as_bytes(), &sig)
            .expect("ES256 signature");
    }

    #[test]
    fn the_environment_builds_a_sender_only_with_a_key_and_a_subject() {
        let clock: Clock = Arc::new(|| 0);
        let none = |_: &str| None;
        assert!(
            WebPush::from_env(&none, Arc::clone(&clock))
                .unwrap()
                .is_none()
        );
        let secret = SecretKey::from_slice(&DEV_VAPID_SECRET).unwrap();
        let der = {
            use p256::pkcs8::EncodePrivateKey;
            URL_SAFE_NO_PAD.encode(secret.to_pkcs8_der().unwrap().as_bytes())
        };
        let only_key = {
            let der = der.clone();
            move |k: &str| (k == VAPID_PRIVATE_KEY_ENV).then(|| OsString::from(&der))
        };
        let e = WebPush::from_env(&only_key, Arc::clone(&clock))
            .err()
            .unwrap();
        assert!(e.contains(VAPID_SUBJECT_ENV) && !e.contains(&der), "{e}");
        let both = {
            let der = der.clone();
            move |k: &str| match k {
                VAPID_PRIVATE_KEY_ENV => Some(OsString::from(&der)),
                VAPID_SUBJECT_ENV => Some(OsString::from("mailto:ops@arena360.cloud")),
                _ => None,
            }
        };
        let push = WebPush::from_env(&both, Arc::clone(&clock))
            .unwrap()
            .unwrap();
        assert_eq!(
            push.public_key(),
            WebPush::Fake(FakePush::new(clock)).public_key()
        );
        let junk = |k: &str| (k == VAPID_PRIVATE_KEY_ENV).then(|| OsString::from("not a key"));
        let e = WebPush::from_env(&junk, Arc::new(|| 0)).err().unwrap();
        assert!(!e.contains("not a key"), "{e}");
    }
}

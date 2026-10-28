//! Device tokens: minting, argon2id hashing and verification.
//!
//! A token is `<device_id>.<secret>`: the device id (a ULID minted by the writer at
//! `device.register`) selects one `devices` row, and the secret (32 random bytes as 64
//! lower-case hex characters) is verified against that row's argon2id `token_hash`. Only the
//! hash is stored. Hashing and verifying are CPU-heavy and always run outside any write
//! transaction; callers on an async runtime run them on a blocking thread.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use arena_proto::{AppError, ClientClass, ErrorCode};
use arena_store::{DeviceRow, Store};
use argon2::password_hash::{PasswordHasher, PasswordVerifier, try_generate_salt};
use argon2::{Argon2, PasswordHash};

use crate::apply::is_ulid;

/// Length of a token secret, in hex characters (32 bytes).
pub const TOKEN_SECRET_HEX_LEN: usize = 64;

fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

fn token_invalid() -> AppError {
    AppError::new(ErrorCode::TokenInvalid, "the device token is not valid")
}

/// Mints a fresh token secret from the OS RNG.
pub fn new_token_secret() -> Result<String, AppError> {
    let mut out = String::with_capacity(TOKEN_SECRET_HEX_LEN);
    for _ in 0..2 {
        let bytes = try_generate_salt().map_err(|e| internal(format!("os rng: {e}")))?;
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
    }
    Ok(out)
}

pub const TOKEN_SECRET_B64_LEN: usize = 43;

pub fn random_bytes(len: usize) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::with_capacity(len + 16);
    while out.len() < len {
        let salt = try_generate_salt().map_err(|e| internal(format!("os rng: {e}")))?;
        out.extend_from_slice(salt.as_ref());
    }
    out.truncate(len);
    Ok(out)
}

pub fn new_paired_token_secret() -> Result<String, AppError> {
    Ok(crate::staff::base64url(&random_bytes(32)?))
}

/// The argon2id PHC string of `secret` (random salt, default parameters).
pub fn hash_token_secret(secret: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(secret.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| internal(format!("argon2 hash: {e}")))
}

/// Whether `secret` matches the argon2id PHC string `hash`. A malformed hash never matches.
pub fn verify_token_secret(secret: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    parsed.algorithm == argon2::ARGON2ID_IDENT
        && Argon2::default()
            .verify_password(secret.as_bytes(), &parsed)
            .is_ok()
}

/// Whether `hash` is a well-formed argon2id PHC string.
pub fn is_argon2id_hash(hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| h.algorithm == argon2::ARGON2ID_IDENT)
}

/// The token handed to a device: `<device_id>.<secret>`.
pub fn format_token(device_id: &str, secret: &str) -> String {
    format!("{device_id}.{secret}")
}

/// Splits a token into its device id and secret (`None` when it is not shaped like one).
pub fn parse_token(token: &str) -> Option<(&str, &str)> {
    let (device_id, secret) = token.split_once('.')?;
    let hex_ok = secret.len() == TOKEN_SECRET_HEX_LEN
        && secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let b64_ok = secret.len() == TOKEN_SECRET_B64_LEN
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    let secret_ok = hex_ok || b64_ok;
    (is_ulid(device_id) && secret_ok).then_some((device_id, secret))
}

/// A device whose token verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDevice {
    pub device_id: String,
    pub class: ClientClass,
    pub reported_version: Option<String>,
    /// The row's `rev` when it was read (the `base_rev` for a version report).
    pub rev: i64,
}

/// A fixed argon2id hash checked when there is no row to check against, so a missing device
/// costs the same time as a wrong secret (no device-id timing oracle).
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        hash_token_secret("arena-360-dummy-secret").unwrap_or_else(|_| {
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNo"
                .to_owned()
        })
    })
}

/// Checks `token` against `row` (already looked up by the token's device id) for `class`:
/// `TOKEN_INVALID` when the row is missing, of another class or the secret does not match;
/// `TOKEN_REVOKED` when it matches a revoked device. Blocking (argon2id).
pub fn check_device_token(
    row: Option<&DeviceRow>,
    class: ClientClass,
    token: &str,
) -> Result<VerifiedDevice, AppError> {
    let (device_id, secret) = parse_token(token).ok_or_else(token_invalid)?;
    let Some(row) = row.filter(|r| r.device_id == device_id && r.class == class.as_str()) else {
        let _ = verify_token_secret(secret, dummy_hash());
        return Err(token_invalid());
    };
    if !verify_token_secret(secret, &row.token_hash) {
        return Err(token_invalid());
    }
    if row.revoked_at.is_some() {
        return Err(AppError::new(
            ErrorCode::TokenRevoked,
            "this device has been revoked",
        ));
    }
    Ok(VerifiedDevice {
        device_id: row.device_id.clone(),
        class,
        reported_version: row.reported_version.clone(),
        rev: row.rev,
    })
}

/// Reads the `devices` table on its own read-only connection (never through the writer), so
/// LAN sessions can verify tokens on a primary or a follower.
pub struct DeviceDirectory {
    reader: Mutex<Store>,
}

impl std::fmt::Debug for DeviceDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceDirectory").finish_non_exhaustive()
    }
}

impl DeviceDirectory {
    /// Opens a read-only connection to the branch database at `db_path`.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        Ok(Self {
            reader: Mutex::new(Store::open_reader(db_path)?),
        })
    }

    /// The device row `device_id`, if any.
    pub fn device(&self, device_id: &str) -> Result<Option<DeviceRow>, AppError> {
        self.reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?
            .device(device_id)
    }

    /// Verifies a hello credential for `class` (see [`check_device_token`]). The row is read
    /// under the lock; the argon2id check runs after it is released. Blocking.
    pub fn verify(&self, class: ClientClass, token: &str) -> Result<VerifiedDevice, AppError> {
        let Some((device_id, _)) = parse_token(token) else {
            return Err(token_invalid());
        };
        let row = self.device(device_id)?;
        check_device_token(row.as_ref(), class, token)
    }

    pub fn session_screens(
        &self,
        now_ms: i64,
    ) -> Result<Vec<(String, arena_proto::SessionScreen)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?;
        crate::session_screen::session_screens(&store, now_ms)
    }

    /// The branch settings among `keys` that exist, with their `rev`, in `keys` order (what a
    /// paired device may read: the caller checks the keys against the class scope first).
    pub fn catalog(
        &self,
        ids: &[String],
        now_ms: i64,
    ) -> Result<Vec<arena_proto::CatalogEntry>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?;
        store.read_consistent(|s| crate::catalog::entries(s, ids, now_ms))
    }

    pub fn settings(&self, keys: &[String]) -> Result<Vec<arena_proto::SettingEntry>, AppError> {
        let rows = self
            .reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?
            .settings()?;
        Ok(keys
            .iter()
            .filter_map(|k| rows.iter().find(|r| &r.key == k))
            .map(|r| arena_proto::SettingEntry {
                key: r.key.clone(),
                value: r.value.clone(),
                rev: r.rev,
            })
            .collect())
    }

    /// The manager verifier a kiosk caches for offline admin exits: the newest version and its
    /// entries (active staff with a PIN whose role allows `device.remote`, at most
    /// [`MAX_VERIFIER_ENTRIES`](arena_proto::MAX_VERIFIER_ENTRIES)), read in one snapshot. Only
    /// PIN hashes, never a PIN.
    pub fn manager_verifier(&self) -> Result<arena_proto::ManagerVerifier, AppError> {
        let (version, entries) = self
            .reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?
            .manager_verifier(
                arena_proto::Permission::DeviceRemote.as_str(),
                arena_proto::MAX_VERIFIER_ENTRIES,
            )?;
        Ok(arena_proto::ManagerVerifier {
            version,
            entries: entries
                .into_iter()
                .map(|c| arena_proto::VerifierEntry {
                    staff_id: c.staff_id,
                    pin_hash: c.pin_hash,
                    pin_verifier_version: c.pin_verifier_version,
                })
                .collect(),
        })
    }

    /// The admin exit `exit_id`, if any.
    pub fn admin_exit(&self, exit_id: &str) -> Result<Option<arena_store::AdminExitRow>, AppError> {
        self.reader
            .lock()
            .map_err(|_| internal("device directory lock poisoned"))?
            .admin_exit(exit_id)
    }

    /// Whether `device_id` is revoked (a missing device counts as revoked).
    pub fn is_revoked(&self, device_id: &str) -> Result<bool, AppError> {
        Ok(self
            .device(device_id)?
            .is_none_or(|row| row.revoked_at.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(device_id: &str, class: &str, hash: &str, revoked: bool) -> DeviceRow {
        DeviceRow {
            device_id: device_id.into(),
            branch_id: "B".into(),
            class: class.into(),
            label: "Dev Kiosk".into(),
            token_hash: hash.into(),
            reported_version: None,
            revoked_at: revoked.then_some(1),
            rev: 1,
        }
    }

    #[test]
    fn tokens_hash_verify_and_parse() {
        let secret = new_token_secret().unwrap();
        assert_eq!(secret.len(), TOKEN_SECRET_HEX_LEN);
        assert_ne!(secret, new_token_secret().unwrap());
        let hash = hash_token_secret(&secret).unwrap();
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(is_argon2id_hash(&hash));
        assert!(!is_argon2id_hash("plain"));
        assert!(verify_token_secret(&secret, &hash));
        assert!(!verify_token_secret(&"0".repeat(64), &hash));
        assert!(!verify_token_secret(&secret, "not-a-hash"));

        let id = ulid::Ulid::generate().to_string();
        let token = format_token(&id, &secret);
        assert_eq!(parse_token(&token), Some((id.as_str(), secret.as_str())));
        for bad in [
            "",
            "garbage",
            &format!("{id}.{}", "g".repeat(64)),
            &format!("{id}.{}", &secret[..63]),
            &format!("not-a-ulid.{secret}"),
            &format!("{id}{secret}"),
        ] {
            assert_eq!(parse_token(bad), None, "{bad}");
        }
    }

    #[test]
    fn paired_secrets_are_256_bit_base64url_and_parse() {
        let secret = new_paired_token_secret().unwrap();
        assert_eq!(secret.len(), TOKEN_SECRET_B64_LEN);
        assert_ne!(secret, new_paired_token_secret().unwrap());
        let id = ulid::Ulid::generate().to_string();
        let token = format_token(&id, &secret);
        assert_eq!(parse_token(&token), Some((id.as_str(), secret.as_str())));
        for bad in [
            format!("{id}.{}", &secret[..42]),
            format!("{id}.{}+", &secret[..42]),
            format!("{id}.{}=", &secret[..42]),
        ] {
            assert_eq!(parse_token(&bad), None, "{bad}");
        }
        assert_eq!(random_bytes(40).unwrap().len(), 40);
    }

    #[test]
    fn check_maps_each_failure_to_its_code() {
        let secret = new_token_secret().unwrap();
        let hash = hash_token_secret(&secret).unwrap();
        let id = ulid::Ulid::generate().to_string();
        let token = format_token(&id, &secret);
        let ok = row(&id, "paired", &hash, false);

        let v = check_device_token(Some(&ok), ClientClass::Paired, &token).unwrap();
        assert_eq!(v.device_id, id);
        let code = |r: Option<&DeviceRow>, class, t: &str| {
            check_device_token(r, class, t).unwrap_err().code
        };
        assert_eq!(
            code(None, ClientClass::Paired, &token),
            ErrorCode::TokenInvalid
        );
        assert_eq!(
            code(Some(&ok), ClientClass::Kds, &token),
            ErrorCode::TokenInvalid
        );
        assert_eq!(
            code(Some(&ok), ClientClass::Paired, "garbage"),
            ErrorCode::TokenInvalid
        );
        let wrong = format_token(&id, &"0".repeat(64));
        assert_eq!(
            code(Some(&ok), ClientClass::Paired, &wrong),
            ErrorCode::TokenInvalid
        );
        let revoked = row(&id, "paired", &hash, true);
        assert_eq!(
            code(Some(&revoked), ClientClass::Paired, &token),
            ErrorCode::TokenRevoked
        );
        // A wrong secret for a revoked device does not reveal the revocation.
        assert_eq!(
            code(Some(&revoked), ClientClass::Paired, &wrong),
            ErrorCode::TokenInvalid
        );
    }
}

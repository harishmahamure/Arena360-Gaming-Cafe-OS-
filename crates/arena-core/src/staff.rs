//! Staff credentials: argon2id hashing of passwords and PINs, sign-in verification and
//! session tokens.
//!
//! Hashing and verifying are CPU-heavy and always run outside any write transaction (the
//! caller hashes, then submits `staff.upsert` with the PHC strings). Sign-in does the same
//! argon2id work whether the login is unknown, the password is wrong or the member is inactive,
//! and every failure is the same `AUTH_FAILED`, so a caller cannot tell which it was.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use arena_proto::{AppError, ErrorCode, SessionStaff};
use arena_store::Store;
use argon2::password_hash::try_generate_salt;

use crate::devices::{hash_token_secret, verify_token_secret};

pub use crate::devices::is_argon2id_hash;

/// The one sign-in failure message. It never says which field was wrong.
pub const AUTH_FAILED_MESSAGE: &str = "sign-in failed";

/// The sign-in failure: identical for every cause.
pub fn auth_failed() -> AppError {
    AppError::new(ErrorCode::AuthFailed, AUTH_FAILED_MESSAGE)
}

/// The argon2id PHC string of a password or PIN (random salt, default parameters). Blocking.
pub fn hash_secret(secret: &str) -> Result<String, AppError> {
    hash_token_secret(secret)
}

/// Whether `secret` matches the argon2id PHC string `hash`. Blocking.
pub fn verify_secret(secret: &str, hash: &str) -> bool {
    verify_token_secret(secret, hash)
}

/// A fixed hash checked when there is no stored hash, so an unknown login costs the same
/// argon2id work as a wrong password.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        hash_secret("arena-360-dummy-password").unwrap_or_else(|_| {
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNo"
                .to_owned()
        })
    })
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(B64URL[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// A fresh session token: 32 bytes from the OS RNG, base64url (43 characters).
pub fn new_session_token() -> Result<String, AppError> {
    let mut bytes = Vec::with_capacity(32);
    for _ in 0..2 {
        let salt = try_generate_salt()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("os rng: {e}")))?;
        bytes.extend_from_slice(&salt);
    }
    Ok(base64url(&bytes[..32]))
}

/// Reads `staff` on its own read-only connection (never through the writer), for sign-in.
pub struct StaffDirectory {
    reader: Mutex<Store>,
}

impl std::fmt::Debug for StaffDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaffDirectory").finish_non_exhaustive()
    }
}

impl StaffDirectory {
    /// Opens a read-only connection to the branch database at `db_path`.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        Ok(Self {
            reader: Mutex::new(Store::open_reader(db_path)?),
        })
    }

    /// The staff member `staff_id` as a session shows them now: `None` when the member is
    /// missing or inactive. Blocking (SQLite).
    pub fn current(&self, staff_id: &str) -> Result<Option<SessionStaff>, AppError> {
        let found = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "staff directory lock poisoned"))?
            .staff_by_id_with_role(staff_id)?;
        Ok(found
            .filter(|(staff, _)| staff.active)
            .map(|(staff, role)| SessionStaff {
                staff_id: staff.staff_id,
                display_name: staff.display_name,
                role_name: role.name,
            }))
    }

    /// Verifies `login` and `password`. The row is read under the lock; the argon2id check
    /// runs after it is released. Unknown logins, members without a password, wrong passwords
    /// and inactive members all do one argon2id verification and fail `AUTH_FAILED`. Blocking.
    pub fn sign_in(&self, login: &str, password: &str) -> Result<SessionStaff, AppError> {
        let found = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "staff directory lock poisoned"))?
            .staff_with_role(login.trim())?;
        let Some((staff, role)) = found else {
            let _ = verify_secret(password, dummy_hash());
            return Err(auth_failed());
        };
        let hash = staff
            .password_hash
            .as_deref()
            .unwrap_or_else(|| dummy_hash());
        let matches = verify_secret(password, hash) && staff.password_hash.is_some();
        if !matches || !staff.active {
            return Err(auth_failed());
        }
        Ok(SessionStaff {
            staff_id: staff.staff_id,
            display_name: staff.display_name,
            role_name: role.name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_known_vectors() {
        for (input, want) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"\xfb\xff\xfe", "-__-"),
        ] {
            assert_eq!(base64url(input), want);
        }
        let t = new_session_token().unwrap();
        assert_eq!(t.len(), 43);
        assert_ne!(t, new_session_token().unwrap());
    }

    #[test]
    fn secrets_hash_and_verify() {
        let h = hash_secret("1111").unwrap();
        assert!(is_argon2id_hash(&h));
        assert!(verify_secret("1111", &h));
        assert!(!verify_secret("2222", &h));
        assert_eq!(auth_failed().code, ErrorCode::AuthFailed);
    }
}

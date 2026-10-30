//! Arena 360 staff credentials for the admin-scoped API (Story 8.37, AD-21).
//!
//! The admin app and its Cloudflare Access login are Epic 11; until then support calls
//! `/v1/admin/*` from a script with a bearer token. A token is a staff member's secret; the
//! service holds only its SHA-256, in `ARENA_ADMIN_TOKENS` as `<staff-id>:<sha256 hex>` entries
//! separated by commas. No entries, no admin API: its routes answer `NOT_FOUND`.

use std::ffi::OsString;

use sha2::{Digest, Sha256};

/// The environment variable holding the staff credentials.
pub const ADMIN_TOKENS_ENV: &str = "ARENA_ADMIN_TOKENS";
/// The longest staff id kept in an `admin_audit` row.
const STAFF_ID_MAX: usize = 64;

/// The staff members who may call the admin API.
#[derive(Clone, Default)]
pub struct AdminTokens {
    staff: Vec<(String, [u8; 32])>,
}

impl std::fmt::Debug for AdminTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminTokens")
            .field("staff", &self.staff.len())
            .finish()
    }
}

fn hash_of(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

impl AdminTokens {
    /// Parses `<staff-id>:<sha256 hex>,...`. A staff id is 1 to 64 characters of letters,
    /// digits, `-`, `_`, `.` and `@`.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut staff = Vec::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (id, hash) = entry
                .split_once(':')
                .ok_or_else(|| format!("{ADMIN_TOKENS_ENV} entries are <staff-id>:<sha256 hex>"))?;
            let id_ok = !id.is_empty()
                && id.len() <= STAFF_ID_MAX
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'));
            if !id_ok {
                return Err(format!("{ADMIN_TOKENS_ENV} has a bad staff id"));
            }
            let hash = hash_of(hash)
                .ok_or_else(|| format!("{ADMIN_TOKENS_ENV}: {id}'s credential is not a SHA-256"))?;
            staff.push((id.to_owned(), hash));
        }
        Ok(Self { staff })
    }

    /// The tokens named by the environment (none when unset).
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Self, String> {
        match env(ADMIN_TOKENS_ENV).and_then(|v| v.into_string().ok()) {
            Some(spec) => Self::parse(&spec),
            None => Ok(Self::default()),
        }
    }

    /// Whether any staff member may call the admin API.
    pub fn is_empty(&self) -> bool {
        self.staff.is_empty()
    }

    /// The staff id whose credential is `token`.
    pub fn authenticate(&self, token: &str) -> Option<&str> {
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        // Every entry is compared, so the time taken does not say which one matched.
        let mut found = None;
        for (id, hash) in &self.staff {
            if digest
                .iter()
                .zip(hash)
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
            {
                found = Some(id.as_str());
            }
        }
        found
    }
}

/// The SHA-256 of `token` in hex: what goes in [`ADMIN_TOKENS_ENV`].
pub fn token_hash_hex(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_matches_its_staff_member_only() {
        let spec = format!(
            "asha:{}, ravi@arena360.cloud:{}",
            token_hash_hex("a-secret"),
            token_hash_hex("b-secret")
        );
        let tokens = AdminTokens::parse(&spec).unwrap();
        assert_eq!(tokens.authenticate("a-secret"), Some("asha"));
        assert_eq!(tokens.authenticate("b-secret"), Some("ravi@arena360.cloud"));
        assert_eq!(tokens.authenticate("c-secret"), None);
        assert_eq!(tokens.authenticate(""), None);
    }

    #[test]
    fn bad_entries_are_refused_and_no_entries_is_empty() {
        for bad in ["asha", "asha:zz", "a b:00", ":00"] {
            assert!(AdminTokens::parse(bad).is_err(), "{bad}");
        }
        assert!(AdminTokens::parse("").unwrap().is_empty());
        assert!(AdminTokens::parse(" , ").unwrap().is_empty());
    }
}

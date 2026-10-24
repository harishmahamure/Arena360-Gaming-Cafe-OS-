//! The owner's password and backup codes: the password rule (10–128 Unicode scalar values,
//! not on the bundled common-password list), argon2id hashing (m = 19 MiB, t = 2, p = 1, PHC
//! strings) and the 10 single-use backup codes (Crockford base32, shown `XXXXX-XXXXX`).
//!
//! Hashing is CPU-heavy: async callers run it on the blocking pool.

use std::collections::HashSet;
use std::sync::OnceLock;

use arena_proto::{BACKUP_CODE_CHARS, BACKUP_CODE_COUNT, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS};
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, PasswordHash, Version};
use ring::rand::{SecureRandom, SystemRandom};

/// The bundled list (`data/common-passwords.txt`): a header of `#` lines ending at the first
/// empty line, then one lowercased password per line.
const COMMON_PASSWORDS: &str = include_str!("../data/common-passwords.txt");

/// The common-password list, parsed once.
fn common() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let body = COMMON_PASSWORDS
            .split_once("\n\n")
            .map_or(COMMON_PASSWORDS, |(_, b)| b);
        body.lines().filter(|l| !l.is_empty()).collect()
    })
}

/// How many passwords the bundled list holds.
pub fn common_password_count() -> usize {
    common().len()
}

/// Why a new password was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordRejection {
    /// Fewer than 10 characters.
    TooShort,
    /// More than 128 characters.
    TooLong,
    /// Its lowercase form is on the common-password list.
    Common,
}

/// Checks a new password against the rule.
pub fn check_password(password: &str) -> Result<(), PasswordRejection> {
    let trimmed = password.trim();
    if trimmed.chars().count() < PASSWORD_MIN_CHARS {
        return Err(PasswordRejection::TooShort);
    }
    if password.chars().count() > PASSWORD_MAX_CHARS {
        return Err(PasswordRejection::TooLong);
    }
    if common().contains(trimmed.to_lowercase().as_str()) {
        return Err(PasswordRejection::Common);
    }
    Ok(())
}

/// argon2id with the OWASP parameters: m = 19 MiB, t = 2, p = 1.
fn argon2() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("valid argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// The argon2id PHC string of `secret` (a random salt). Blocking.
pub fn hash_secret(secret: &str) -> Result<String, String> {
    argon2()
        .hash_password(secret.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| format!("argon2 hash: {e}"))
}

/// Whether `secret` matches the argon2id PHC string `hash`. A malformed hash never matches.
/// Blocking.
pub fn verify_secret(secret: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    parsed.algorithm == argon2::ARGON2ID_IDENT
        && argon2().verify_password(secret.as_bytes(), &parsed).is_ok()
}

/// Crockford base32 (no I, L, O or U).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A fresh backup code, normalised (10 Crockford base32 characters, no hyphen).
fn new_backup_code(rng: &SystemRandom) -> Result<String, String> {
    let mut b = [0u8; BACKUP_CODE_CHARS];
    rng.fill(&mut b).map_err(|_| "os rng failed".to_owned())?;
    // 256 is a multiple of 32: masking 5 bits is uniform.
    Ok(b.iter()
        .map(|x| CROCKFORD[usize::from(x & 31)] as char)
        .collect())
}

/// 10 fresh, distinct backup codes, normalised.
pub fn new_backup_codes() -> Result<Vec<String>, String> {
    let rng = SystemRandom::new();
    let mut out: Vec<String> = Vec::with_capacity(BACKUP_CODE_COUNT);
    while out.len() < BACKUP_CODE_COUNT {
        let c = new_backup_code(&rng)?;
        if !out.contains(&c) {
            out.push(c);
        }
    }
    Ok(out)
}

/// A normalised code shown as `XXXXX-XXXXX`.
pub fn display_backup_code(code: &str) -> String {
    let (a, b) = code.split_at(code.len() / 2);
    format!("{a}-{b}")
}

/// A backup code as typed, normalised: uppercased, hyphens and spaces removed, `I`/`L` read as
/// `1` and `O` as `0`. `None` when the result is not 10 Crockford base32 characters.
pub fn normalize_backup_code(input: &str) -> Option<String> {
    let s: String = input
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            u => u,
        })
        .collect();
    (s.len() == BACKUP_CODE_CHARS && s.bytes().all(|b| CROCKFORD.contains(&b))).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_list_has_10_000_lowercased_passwords() {
        assert_eq!(common_password_count(), 10_000);
        assert!(common().contains("password123"));
        assert!(common().contains("123456"));
        assert!(
            !common()
                .iter()
                .any(|p| p.starts_with('#') && p.contains("source"))
        );
        assert!(common().iter().all(|p| p.to_lowercase() == *p));
    }

    #[test]
    fn the_password_rule() {
        assert_eq!(
            check_password("abcdefghi"),
            Err(PasswordRejection::TooShort)
        );
        // Unicode scalar values, not bytes: 9 two-byte characters are still too short.
        assert_eq!(
            check_password(&"é".repeat(9)),
            Err(PasswordRejection::TooShort)
        );
        assert_eq!(check_password(&"é".repeat(10)), Ok(()));
        assert_eq!(
            check_password("password123"),
            Err(PasswordRejection::Common)
        );
        assert_eq!(
            check_password("PassWord123"),
            Err(PasswordRejection::Common)
        );
        assert_eq!(check_password("correct horse battery"), Ok(()));
        assert_eq!(check_password(&"x".repeat(128)), Ok(()));
        assert_eq!(
            check_password(&"x".repeat(129)),
            Err(PasswordRejection::TooLong)
        );
        assert_eq!(
            check_password(&" ".repeat(12)),
            Err(PasswordRejection::TooShort)
        );
        assert_eq!(
            check_password("\t\n abcdefghi \n"),
            Err(PasswordRejection::TooShort)
        );
        assert_eq!(check_password("  abcdefghij  "), Ok(()));
        assert_eq!(
            check_password(" password123 "),
            Err(PasswordRejection::Common)
        );
    }

    #[test]
    fn hashes_are_argon2id_phc_with_the_owasp_parameters() {
        let h = hash_secret("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{h}");
        assert!(verify_secret("correct horse battery", &h));
        assert!(!verify_secret("correct horse batterY", &h));
        assert!(!verify_secret("x", "not a hash"));
        assert_ne!(h, hash_secret("correct horse battery").unwrap());
    }

    #[test]
    fn backup_codes_are_ten_distinct_crockford_codes() {
        let codes = new_backup_codes().unwrap();
        assert_eq!(codes.len(), 10);
        for c in &codes {
            assert_eq!(normalize_backup_code(c).as_deref(), Some(c.as_str()));
            let shown = display_backup_code(c);
            assert_eq!(shown.len(), 11);
            assert_eq!(&shown[5..6], "-");
            assert_eq!(normalize_backup_code(&shown).as_deref(), Some(c.as_str()));
        }
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }

    #[test]
    fn typed_backup_codes_are_normalised() {
        assert_eq!(
            normalize_backup_code("abcde-fghjk").as_deref(),
            Some("ABCDEFGHJK")
        );
        assert_eq!(
            normalize_backup_code(" o1i2l 3OOO0 ").as_deref(),
            Some("0112130000")
        );
        assert_eq!(normalize_backup_code("ABCDE-FGHJ"), None);
        assert_eq!(normalize_backup_code("ABCDE-FGHJU"), None);
        assert_eq!(normalize_backup_code("ABCDE_FGHJK"), None);
    }
}

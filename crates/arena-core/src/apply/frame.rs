//! The pre-transaction frame check. It runs in `WriterHandle::submit_intent` before the intent
//! is queued, so a malformed frame never opens a transaction.

use arena_proto::{AppError, ErrorCode, Intent};

/// Crockford base32 (no I, L, O or U), matched case-insensitively.
const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Whether `s` is a ULID string: 26 Crockford base32 characters that fit in 128 bits.
pub fn is_ulid(s: &str) -> bool {
    // 26 × 5 bits is 130: the first character carries only 3 bits, so it is at most '7'.
    s.len() == 26
        && s.bytes()
            .all(|b| CROCKFORD.contains(&b.to_ascii_uppercase()))
        && s.as_bytes()[0] <= b'7'
        && s.parse::<ulid::Ulid>().is_ok()
}

/// The stored form of a valid idempotency key: upper case, so `01ja…` and `01JA…` are the same
/// key.
pub(crate) fn canonical_key(key: &str) -> String {
    key.to_ascii_uppercase()
}

/// Fails `BAD_FRAME` (`field = "idempotency_key"`) when the key is missing, empty or not a ULID.
/// Pure: reads nothing but the intent.
pub fn validate_frame(intent: &Intent) -> Result<(), AppError> {
    if is_ulid(&intent.idempotency_key) {
        Ok(())
    } else {
        Err(AppError::with_field(
            ErrorCode::BadFrame,
            "idempotency_key",
            "idempotency_key must be a ULID",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_shapes() {
        let id = ulid::Ulid::generate().to_string();
        assert!(is_ulid(&id));
        assert!(is_ulid(&id.to_ascii_lowercase()));
        assert!(is_ulid("7ZZZZZZZZZZZZZZZZZZZZZZZZZ"));
        for bad in [
            "",
            "not-a-ulid",
            "01JAAAAAAAAAAAAAAAAAAAAAA",   // 25
            "01JAAAAAAAAAAAAAAAAAAAAAAAA", // 27
            "01JAAAAAAAAAAAAAAAAAAAAAAI",  // I, L, O and U are not Crockford
            "01JAAAAAAAAAAAAAAAAAAAAAAL",
            "01JAAAAAAAAAAAAAAAAAAAAAAO",
            "01JAAAAAAAAAAAAAAAAAAAAAAU",
            "01JAAAAAAAAAAAAAAAAAAAAAAi",
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ", // overflows 128 bits
            "01JAAAAAAAAAAAAAAAAAAAAAA ",
            "01JAAAAAAAAAAAAAAAAAAAAAé", // 24 ASCII + 'é': 26 bytes
        ] {
            assert!(!is_ulid(bad), "{bad:?}");
            if !bad.is_ascii() {
                assert_eq!(bad.len(), 26, "the non-ASCII case must be 26 bytes");
            }
        }
        assert_eq!(canonical_key("01jaa"), "01JAA");
    }
}

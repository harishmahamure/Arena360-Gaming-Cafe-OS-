use arena_proto::{
    PAIRING_CODE_ALPHABET, PAIRING_CODE_LEN, normalize_pairing_code, valid_pairing_code,
};
use arena_store::control::{ControlError, RedeemInvalid, Redeemed};

use crate::auth::{AuthState, random_bytes};

const ALPHABET_LEN: u8 = PAIRING_CODE_ALPHABET.len() as u8;
const LIMIT: u8 = u8::MAX - (u8::MAX % ALPHABET_LEN);

pub fn new_pairing_code() -> Result<String, String> {
    let mut out = String::with_capacity(PAIRING_CODE_LEN);
    while out.len() < PAIRING_CODE_LEN {
        for b in random_bytes::<16>()? {
            if b < LIMIT && out.len() < PAIRING_CODE_LEN {
                out.push(char::from(
                    PAIRING_CODE_ALPHABET[usize::from(b % ALPHABET_LEN)],
                ));
            }
        }
    }
    Ok(out)
}

pub async fn redeem_pairing_code(
    st: &AuthState,
    code: &str,
) -> Result<Result<Redeemed, RedeemInvalid>, ControlError> {
    let normalized = normalize_pairing_code(code);
    if !valid_pairing_code(&normalized) {
        tracing::info!(reason = "malformed", "hub pairing code refused");
        return Ok(Err(RedeemInvalid));
    }
    let hash = st.pairing_code_hash(&normalized);
    let outcome = st.db.redeem_pairing_code(&hash, st.now()).await?;
    match &outcome {
        Ok(r) => {
            tracing::info!(branch_id = %r.branch_id, tenant_id = %r.tenant_id, tier = r.tier.as_str(), "hub pairing code redeemed")
        }
        Err(RedeemInvalid) => tracing::info!(
            reason = "unknown_used_invalidated_or_expired",
            "hub pairing code refused"
        ),
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_keeps_every_kept_byte_uniform() {
        assert_eq!(LIMIT, 240);
        assert_eq!(LIMIT % ALPHABET_LEN, 0);
    }

    #[test]
    fn codes_are_eight_characters_from_the_alphabet_and_spread_evenly() {
        let mut counts = [0u32; 30];
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let code = new_pairing_code().unwrap();
            assert_eq!(code.len(), PAIRING_CODE_LEN, "{code}");
            for c in code.bytes() {
                assert!(!b"01OILU".contains(&c), "{code}");
                let i = PAIRING_CODE_ALPHABET
                    .iter()
                    .position(|a| *a == c)
                    .unwrap_or_else(|| panic!("{code}"));
                counts[i] += 1;
            }
            seen.insert(code);
        }
        assert!(seen.len() > 9_990, "{}", seen.len());
        let expected = 80_000.0 / 30.0;
        for (i, n) in counts.iter().enumerate() {
            let off = (f64::from(*n) - expected).abs() / expected;
            assert!(
                off < 0.12,
                "{} drawn {n} times",
                PAIRING_CODE_ALPHABET[i] as char
            );
        }
    }
}

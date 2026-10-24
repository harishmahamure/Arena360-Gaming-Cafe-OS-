//! The TOTP secret at rest: AES-256-GCM (`ring::aead`) under `ARENA_MFA_KEY`, with a fresh
//! 96-bit random nonce per encryption and the owner id as the additional authenticated data,
//! so a ciphertext only opens for the owner it was sealed for.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

/// Bytes in `ARENA_MFA_KEY` (decoded).
pub const MFA_KEY_BYTES: usize = 32;

/// The key that seals TOTP secrets. A secret: `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct MfaKey([u8; MFA_KEY_BYTES]);

impl std::fmt::Debug for MfaKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MfaKey([redacted])")
    }
}

/// Opening a sealed secret failed: a wrong key, a wrong owner or tampered data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenError;

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the MFA secret could not be decrypted (wrong ARENA_MFA_KEY or tampered row)")
    }
}

impl std::error::Error for OpenError {}

/// A sealed secret: the ciphertext (with its 16-byte tag) and its nonce.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

impl MfaKey {
    pub fn from_bytes(b: [u8; MFA_KEY_BYTES]) -> Self {
        Self(b)
    }

    /// Parses standard base64 (padding optional) of exactly 32 bytes.
    pub fn from_base64(s: &str) -> Result<Self, String> {
        let bytes = base64_decode(s.trim()).ok_or("is not valid base64")?;
        let arr: [u8; MFA_KEY_BYTES] = bytes
            .try_into()
            .map_err(|_| format!("must decode to exactly {MFA_KEY_BYTES} bytes"))?;
        Ok(Self(arr))
    }

    fn key(&self) -> LessSafeKey {
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &self.0).expect("32-byte AES-256 key"))
    }

    /// Seals `plaintext` for `owner_id` under a fresh random nonce.
    pub fn seal(&self, owner_id: &str, plaintext: &[u8]) -> Result<Sealed, String> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| "os rng failed".to_owned())?;
        let mut in_out = plaintext.to_vec();
        self.key()
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(owner_id.as_bytes()),
                &mut in_out,
            )
            .map_err(|_| "aes-gcm seal failed".to_owned())?;
        Ok(Sealed {
            ciphertext: in_out,
            nonce: nonce.to_vec(),
        })
    }

    /// Opens a secret sealed for `owner_id`.
    pub fn open(
        &self,
        owner_id: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> Result<Vec<u8>, OpenError> {
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| OpenError)?;
        let mut in_out = ciphertext.to_vec();
        let plain = self
            .key()
            .open_in_place(nonce, Aad::from(owner_id.as_bytes()), &mut in_out)
            .map_err(|_| OpenError)?;
        Ok(plain.to_vec())
    }
}

/// Decodes standard base64 (`A–Z a–z 0–9 + /`, `=` padding optional).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let body = s.trim_end_matches('=');
    if s.len() - body.len() > 2 {
        return None;
    }
    let s = body;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding: 0, 2 or 4 of them.
    (bits < 6 && buf == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_keys() {
        // 32 zero bytes, with and without padding.
        let zero = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        assert_eq!(MfaKey::from_base64(zero).unwrap(), MfaKey([0; 32]));
        assert_eq!(
            MfaKey::from_base64(zero.trim_end_matches('=')).unwrap(),
            MfaKey([0; 32])
        );
        assert!(
            MfaKey::from_base64("AAAA")
                .unwrap_err()
                .contains("32 bytes")
        );
        assert!(MfaKey::from_base64("not base64!").is_err());
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("Zm9vYmE").unwrap(), b"fooba");
        assert!(base64_decode("Zm9vYh").is_none());
        assert!(base64_decode("Zm9vYg===").is_none());
        assert!(base64_decode("Zm9vYmFy====").is_none());
        assert!(base64_decode("Zm9v YmFy").is_none());
        assert!(base64_decode("Zm9v=YmFy").is_none());
        assert!(base64_decode("Zm9v-mFy").is_none());
        assert_eq!(format!("{:?}", MfaKey([7; 32])), "MfaKey([redacted])");
    }

    #[test]
    fn a_sealed_secret_opens_only_with_its_key_and_owner() {
        let k = MfaKey([1; 32]);
        let s = k.seal("owner-1", b"secret-bytes").unwrap();
        assert_eq!(s.nonce.len(), 12);
        assert_ne!(&s.ciphertext[..12], b"secret-bytes");
        assert_eq!(
            k.open("owner-1", &s.ciphertext, &s.nonce).unwrap(),
            b"secret-bytes"
        );
        assert_eq!(k.open("owner-2", &s.ciphertext, &s.nonce), Err(OpenError));
        assert_eq!(
            MfaKey([2; 32]).open("owner-1", &s.ciphertext, &s.nonce),
            Err(OpenError)
        );
        let mut tampered = s.ciphertext.clone();
        tampered[0] ^= 1;
        assert_eq!(k.open("owner-1", &tampered, &s.nonce), Err(OpenError));
        assert_eq!(k.open("owner-1", &s.ciphertext, &[0; 5]), Err(OpenError));
        // A fresh nonce each time.
        assert_ne!(k.seal("owner-1", b"x").unwrap().nonce, s.nonce);
    }
}

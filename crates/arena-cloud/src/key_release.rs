//! Re-wrapping the escrowed branch key for a replacement Hub (Story 9.10, AD-24).
//!
//! The escrow row holds the branch key (KEK) as an age file for the cloud's escrow public key. On
//! the owner's approval the control role opens it with the escrow private key
//! (`ARENA_KEK_ESCROW_KEY`) and wraps the bytes again for the request's one-time public key, so
//! only the waiting Hub's private half can open what it is sent. The plaintext key lives in one
//! zeroizing 32-byte buffer between those two steps and is dropped (and zeroed) when
//! [`rewrap`] returns; no error, log line or return value carries it.

use std::io::Read;
use std::str::FromStr;

use age::secrecy::{ExposeSecret, ExposeSecretMut, SecretBox};

use crate::backup;
use crate::config::EscrowKey;

/// The branch key's length: a 256-bit AES key.
const KEK_BYTES: usize = 32;

/// Why a key could not be re-wrapped. The text never carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewrapError {
    /// The one-time public key is not an age X25519 recipient.
    BadRecipient,
    /// The escrow key does not open the escrowed file, or it does not hold a branch key.
    EscrowUnreadable,
    /// Wrapping for the recipient failed.
    WrapFailed,
}

impl std::fmt::Display for RewrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadRecipient => "the request's public key is not an age public key",
            Self::EscrowUnreadable => "the escrowed branch key could not be opened",
            Self::WrapFailed => "the branch key could not be wrapped for the Hub",
        })
    }
}

impl std::error::Error for RewrapError {}

/// Opens `escrowed` (an age file for the escrow key's public half) and wraps the branch key it
/// holds for `recipient` (`age1…`). Returns the new age file.
pub fn rewrap(key: &EscrowKey, escrowed: &[u8], recipient: &str) -> Result<Vec<u8>, RewrapError> {
    let recipient =
        age::x25519::Recipient::from_str(recipient).map_err(|_| RewrapError::BadRecipient)?;
    let identity = key.identity().map_err(|_| RewrapError::EscrowUnreadable)?;
    let decryptor =
        age::Decryptor::new_buffered(escrowed).map_err(|_| RewrapError::EscrowUnreadable)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| RewrapError::EscrowUnreadable)?;
    let mut kek: SecretBox<[u8; KEK_BYTES]> = SecretBox::new(Box::new([0u8; KEK_BYTES]));
    reader
        .read_exact(kek.expose_secret_mut())
        .map_err(|_| RewrapError::EscrowUnreadable)?;
    let mut extra = [0u8; 1];
    if !matches!(reader.read(&mut extra), Ok(0)) {
        return Err(RewrapError::EscrowUnreadable);
    }
    backup::encrypt(kek.expose_secret(), &[recipient]).map_err(|_| RewrapError::WrapFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_for(recipient: &age::x25519::Recipient, plain: &[u8]) -> Vec<u8> {
        backup::encrypt(plain, std::slice::from_ref(recipient)).unwrap()
    }

    fn open(identity: &age::x25519::Identity, file: &[u8]) -> Vec<u8> {
        let d = age::Decryptor::new_buffered(file).unwrap();
        let mut r = d
            .decrypt(std::iter::once(identity as &dyn age::Identity))
            .unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        out
    }

    fn escrow() -> (EscrowKey, age::x25519::Identity) {
        let identity = age::x25519::Identity::generate();
        let key = EscrowKey::parse(identity.to_string().expose_secret()).unwrap();
        (key, identity)
    }

    #[test]
    fn the_hub_opens_what_the_escrow_held_and_nobody_else_can() {
        let (key, escrow_identity) = escrow();
        let kek = [7u8; KEK_BYTES];
        let escrowed = wrap_for(&escrow_identity.to_public(), &kek);
        let hub = age::x25519::Identity::generate();
        let wrapped = rewrap(&key, &escrowed, &hub.to_public().to_string()).unwrap();
        assert_eq!(open(&hub, &wrapped), kek);
        let d = age::Decryptor::new_buffered(&wrapped[..]).unwrap();
        assert!(
            d.decrypt(std::iter::once(&escrow_identity as &dyn age::Identity))
                .is_err(),
            "the escrow key does not open the Hub's copy"
        );
    }

    #[test]
    fn a_wrong_escrow_key_a_bad_recipient_and_a_wrong_sized_key_are_refused() {
        let (key, escrow_identity) = escrow();
        let hub = age::x25519::Identity::generate().to_public().to_string();
        let escrowed = wrap_for(&escrow_identity.to_public(), &[1u8; KEK_BYTES]);
        let (other, _) = escrow();
        assert_eq!(
            rewrap(&other, &escrowed, &hub),
            Err(RewrapError::EscrowUnreadable)
        );
        assert_eq!(
            rewrap(&key, &escrowed, "age1nope"),
            Err(RewrapError::BadRecipient)
        );
        for bad in [vec![1u8; 31], vec![1u8; 33]] {
            let escrowed = wrap_for(&escrow_identity.to_public(), &bad);
            assert_eq!(
                rewrap(&key, &escrowed, &hub),
                Err(RewrapError::EscrowUnreadable)
            );
        }
        assert_eq!(
            rewrap(&key, b"not an age file", &hub),
            Err(RewrapError::EscrowUnreadable)
        );
    }

    #[test]
    fn the_escrow_key_never_prints() {
        let (key, identity) = escrow();
        let text = format!("{key:?}");
        assert!(!text.contains("AGE-SECRET-KEY"));
        assert!(!text.contains(identity.to_string().expose_secret()));
        assert_eq!(key.public().unwrap(), identity.to_public().to_string());
        assert!(
            EscrowKey::parse("age1v0w7nljdjmtc27xp7rt4zyyrmryhr67skkwxemc2slu9x09dsgjq7dr5j4")
                .is_err()
        );
        let e = EscrowKey::parse("AGE-SECRET-KEY-garbage-SECRETTEXT").unwrap_err();
        assert!(!e.contains("SECRETTEXT"));
    }
}

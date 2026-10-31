//! Checking a `hub.successor` (Story 10.19): the next Hub of the branch, signed by the Hub the
//! kiosk pins.
//!
//! The kiosk trusts one thing about its Hub: the SHA-256 of its certificate. A successor carries
//! that certificate and a signature made with its key, so it is good only if the certificate hashes
//! to the pin, the signature verifies against the key inside it, the branch is the one the Hub
//! said it serves, and it has not run out.

use arena_proto::SignedSuccessor;
use ring::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};
use sha2::{Digest, Sha256};

use crate::client::{hex_lower, parse_fingerprint};

/// Why a successor was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// A field is not what it must be (hex, `host:port`, fingerprint).
    Malformed,
    /// The signing certificate is not the one the kiosk pins.
    Certificate,
    /// The signature does not verify.
    Signature,
    /// Another branch, or the kiosk has not learned its branch yet.
    Branch,
    Expired,
}

impl Rejection {
    /// The reason as it is logged.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::Certificate => "certificate not pinned",
            Self::Signature => "bad signature",
            Self::Branch => "other branch",
            Self::Expired => "expired",
        }
    }
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// The uncompressed P-256 point of a certificate's public key: the 65 bytes after the
/// `BIT STRING (66 bytes, 0 unused bits)` header, which is how an `id-ecPublicKey` key ends.
fn public_key_of(cert_der: &[u8]) -> Option<&[u8]> {
    const HEADER: [u8; 4] = [0x03, 0x42, 0x00, 0x04];
    let at = cert_der.windows(HEADER.len()).position(|w| w == HEADER)?;
    cert_der.get(at + 3..at + 3 + 65)
}

fn valid_address(a: &str) -> bool {
    a.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && port.parse::<u16>().is_ok_and(|p| p != 0)
            && (!host.contains(':') || (host.starts_with('[') && host.ends_with(']')))
    })
}

/// Checks `signed` against the kiosk's `pinned` fingerprint, the `branch_id` its Hub told it and
/// the time `now_ms`.
pub fn verify(
    signed: &SignedSuccessor,
    pinned: &str,
    branch_id: Option<&str>,
    now_ms: i64,
) -> Result<(), Rejection> {
    let s = &signed.successor;
    if !valid_address(&s.address) || parse_fingerprint(&s.fingerprint).is_err() {
        return Err(Rejection::Malformed);
    }
    let pinned = parse_fingerprint(pinned).map_err(|_| Rejection::Certificate)?;
    let cert = unhex(&signed.cert).ok_or(Rejection::Malformed)?;
    let signature = unhex(&signed.signature).ok_or(Rejection::Malformed)?;
    if hex_lower(&Sha256::digest(&cert)) != hex_lower(&pinned) {
        return Err(Rejection::Certificate);
    }
    let key = public_key_of(&cert).ok_or(Rejection::Malformed)?;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, key)
        .verify(&s.signing_bytes(), &signature)
        .map_err(|_| Rejection::Signature)?;
    if branch_id != Some(s.branch_id.as_str()) {
        return Err(Rejection::Branch);
    }
    if s.expires_at <= now_ms {
        return Err(Rejection::Expired);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::HubSuccessor;
    use rcgen::SigningKey as _;

    struct Hub {
        cert_der: Vec<u8>,
        key: rcgen::KeyPair,
    }

    fn hub() -> Hub {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = rcgen::CertificateParams::new(vec!["arena360.local".into()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        Hub {
            cert_der: cert.der().to_vec(),
            key,
        }
    }

    fn fingerprint(der: &[u8]) -> String {
        hex_lower(&Sha256::digest(der))
    }

    fn successor(expires_at: i64) -> HubSuccessor {
        HubSuccessor {
            address: "192.168.1.20:7400".into(),
            fingerprint: "ab".repeat(32),
            branch_id: "BR1".into(),
            expires_at,
        }
    }

    fn sign(hub: &Hub, s: HubSuccessor) -> SignedSuccessor {
        let signature = hub.key.sign(&s.signing_bytes()).unwrap();
        SignedSuccessor {
            successor: s,
            cert: hex_lower(&hub.cert_der),
            signature: hex_lower(&signature),
        }
    }

    #[test]
    fn a_good_successor_verifies() {
        let hub = hub();
        let signed = sign(&hub, successor(2_000));
        let pin = fingerprint(&hub.cert_der);
        assert_eq!(verify(&signed, &pin, Some("BR1"), 1_000), Ok(()));
    }

    #[test]
    fn every_way_to_be_wrong_is_refused() {
        let hub = hub();
        let pin = fingerprint(&hub.cert_der);
        let good = sign(&hub, successor(2_000));

        assert_eq!(
            verify(&good, &pin, Some("OTHER"), 1_000),
            Err(Rejection::Branch)
        );
        assert_eq!(verify(&good, &pin, None, 1_000), Err(Rejection::Branch));
        assert_eq!(
            verify(&good, &pin, Some("BR1"), 2_000),
            Err(Rejection::Expired)
        );

        // Changed after signing.
        let mut moved = good.clone();
        moved.successor.address = "10.0.0.9:7400".into();
        assert_eq!(
            verify(&moved, &pin, Some("BR1"), 1_000),
            Err(Rejection::Signature)
        );

        // Signed by a Hub the kiosk does not pin.
        let stranger = hub_for_other();
        let theirs = sign(&stranger, successor(2_000));
        assert_eq!(
            verify(&theirs, &pin, Some("BR1"), 1_000),
            Err(Rejection::Certificate)
        );

        // A pinned certificate with a signature from another key.
        let mut forged = good.clone();
        forged.signature = sign(&stranger, successor(2_000)).signature;
        assert_eq!(
            verify(&forged, &pin, Some("BR1"), 1_000),
            Err(Rejection::Signature)
        );

        let mut bad_fp = good.clone();
        bad_fp.successor.fingerprint = "zz".into();
        assert_eq!(
            verify(&bad_fp, &pin, Some("BR1"), 1_000),
            Err(Rejection::Malformed)
        );
        let mut bad_hex = good;
        bad_hex.signature = "xyz".into();
        assert_eq!(
            verify(&bad_hex, &pin, Some("BR1"), 1_000),
            Err(Rejection::Malformed)
        );
    }

    fn hub_for_other() -> Hub {
        hub()
    }
}

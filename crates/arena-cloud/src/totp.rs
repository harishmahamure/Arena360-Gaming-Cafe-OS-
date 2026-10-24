//! TOTP (RFC 6238) with HMAC-SHA1, 6 digits and a 30 s step, on `ring`; the secret's base32
//! form (RFC 4648, no padding), the `otpauth://` URI and its QR code as SVG.
//!
//! A code is accepted for the current step or one step either side, and only for a step after
//! the last accepted one (no replay); the caller stores the step it accepted.

use arena_proto::{TOTP_DIGITS, TOTP_PERIOD_S};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

/// Bytes in a TOTP secret.
pub const SECRET_BYTES: usize = 20;
/// The issuer shown in authenticator apps.
pub const ISSUER: &str = "Arena 360";

/// A fresh 20-byte secret from the OS CSPRNG.
pub fn new_secret() -> Result<[u8; SECRET_BYTES], String> {
    let mut b = [0u8; SECRET_BYTES];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "os rng failed".to_owned())?;
    Ok(b)
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 without padding.
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buf: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// `s` in groups of 4 separated by spaces, for display.
pub fn group4(s: &str) -> String {
    s.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

/// HOTP (RFC 4226) for `counter`, truncated to `digits` decimal digits.
pub fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = hmac::sign(&key, &counter.to_be_bytes());
    let mac = tag.as_ref();
    let offset = usize::from(mac[mac.len() - 1] & 0x0f);
    let bin = (u32::from(mac[offset]) & 0x7f) << 24
        | u32::from(mac[offset + 1]) << 16
        | u32::from(mac[offset + 2]) << 8
        | u32::from(mac[offset + 3]);
    bin % 10u32.pow(digits)
}

/// The 30 s step of Unix time `ms`.
pub fn step_of(ms: i64) -> i64 {
    ms.div_euclid(1000).div_euclid(TOTP_PERIOD_S as i64)
}

/// The 6-digit code for `step`, zero-padded.
pub fn code_at(secret: &[u8], step: i64) -> String {
    let counter = u64::try_from(step).unwrap_or(0);
    format!(
        "{:0width$}",
        hotp(secret, counter, TOTP_DIGITS as u32),
        width = TOTP_DIGITS
    )
}

/// Whether `code` looks like a TOTP code: exactly 6 ASCII digits.
pub fn well_formed(code: &str) -> bool {
    code.len() == TOTP_DIGITS && code.bytes().all(|b| b.is_ascii_digit())
}

/// The step `code` is valid for at `now_ms` (the current step or one either side, the newest
/// first) that is after `last_step`; `None` when it matches none of them.
pub fn matching_step(
    secret: &[u8],
    code: &str,
    now_ms: i64,
    last_step: Option<i64>,
) -> Option<i64> {
    if !well_formed(code) {
        return None;
    }
    let now = step_of(now_ms);
    let candidates = [now + 1, now, now - 1];
    let matched =
        candidates.map(|step| codes_equal(code_at(secret, step).as_bytes(), code.as_bytes()));
    candidates
        .into_iter()
        .zip(matched)
        .find(|&(step, ok)| ok && step >= 0 && last_step.is_none_or(|l| step > l))
        .map(|(step, _)| step)
}

#[allow(deprecated)]
fn codes_equal(a: &[u8], b: &[u8]) -> bool {
    ring::constant_time::verify_slices_are_equal(a, b).is_ok()
}

/// Percent-encodes `s` for a URI path segment or query value (unreserved characters kept).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `otpauth://totp/Arena%20360:<phone>?secret=…&issuer=Arena%20360&algorithm=SHA1&digits=6&period=30`.
pub fn otpauth_uri(phone_e164: &str, secret_b32: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={secret_b32}&issuer={issuer}\
         &algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_PERIOD_S}",
        issuer = pct(ISSUER),
        account = pct(phone_e164),
    )
}

/// `uri` as a QR code: a standalone SVG document (black on white, with a quiet zone).
pub fn qr_svg(uri: &str) -> Result<String, String> {
    let code = qrcode::QrCode::with_error_correction_level(uri.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| format!("qr: {e}"))?;
    Ok(code
        .render::<qrcode::render::svg::Color<'_>>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA-1: the ASCII secret "12345678901234567890".
    const RFC_SECRET: &[u8] = b"12345678901234567890";
    const RFC_VECTORS: [(i64, u32); 6] = [
        (59, 94_287_082),
        (1_111_111_109, 7_081_804),
        (1_111_111_111, 14_050_471),
        (1_234_567_890, 89_005_924),
        (2_000_000_000, 69_279_037),
        (20_000_000_000, 65_353_130),
    ];

    #[test]
    fn rfc_6238_sha1_vectors() {
        for (t, want) in RFC_VECTORS {
            let step = step_of(t * 1000);
            assert_eq!(hotp(RFC_SECRET, step as u64, 8), want, "T = {t}");
            // The 6-digit code is the last 6 digits of the 8-digit one.
            assert_eq!(
                code_at(RFC_SECRET, step),
                format!("{:06}", want % 1_000_000),
                "T = {t}"
            );
        }
    }

    #[test]
    fn rfc_4226_hotp_vectors() {
        let want = [
            755_224, 287_082, 359_152, 969_429, 338_314, 254_676, 287_922, 162_583, 399_871,
            520_489,
        ];
        for (i, w) in want.into_iter().enumerate() {
            assert_eq!(hotp(RFC_SECRET, i as u64, 6), w, "counter {i}");
        }
    }

    #[test]
    fn base32_matches_rfc_4648_without_padding() {
        for (input, want) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(input.as_bytes()), want, "{input:?}");
        }
        assert_eq!(base32(RFC_SECRET), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(base32(&[0u8; SECRET_BYTES]).len(), 32);
        assert_eq!(group4("ABCDEFGHIJ"), "ABCD EFGH IJ");
    }

    #[test]
    fn one_step_either_side_is_accepted_and_never_a_used_one() {
        let s = RFC_SECRET;
        let now_ms = 1_111_111_111_000;
        let now = step_of(now_ms);
        for d in [-1, 0, 1] {
            assert_eq!(
                matching_step(s, &code_at(s, now + d), now_ms, None),
                Some(now + d)
            );
        }
        for d in [-2, 2] {
            assert_eq!(matching_step(s, &code_at(s, now + d), now_ms, None), None);
        }
        // Replay: the accepted step and earlier ones are refused.
        assert_eq!(matching_step(s, &code_at(s, now), now_ms, Some(now)), None);
        assert_eq!(
            matching_step(s, &code_at(s, now - 1), now_ms, Some(now - 1)),
            None
        );
        assert_eq!(
            matching_step(s, &code_at(s, now + 1), now_ms, Some(now)),
            Some(now + 1)
        );
        assert_eq!(matching_step(s, "12345", now_ms, None), None);
        assert_eq!(matching_step(s, "12345a", now_ms, None), None);
    }

    #[test]
    fn secrets_are_random() {
        assert_ne!(new_secret().unwrap(), new_secret().unwrap());
    }

    #[test]
    fn the_uri_and_its_qr() {
        let uri = otpauth_uri("+919876543210", "GEZDGNBV");
        assert_eq!(
            uri,
            "otpauth://totp/Arena%20360:%2B919876543210?secret=GEZDGNBV&issuer=Arena%20360\
             &algorithm=SHA1&digits=6&period=30"
        );
        let svg = qr_svg(&uri).unwrap();
        assert!(svg.contains("<svg") && svg.contains("</svg>"), "{svg}");
        assert!(!svg.contains("GEZDGNBV"));
    }
}

const ALPHABET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn value(b: u8) -> Option<u32> {
    ALPHABET.iter().position(|&a| a == b).map(|i| i as u32)
}

pub fn check_char(first14: &[u8]) -> Option<u8> {
    let mut sum = 0u32;
    for (i, &b) in first14.iter().enumerate() {
        let product = value(b)? * if i % 2 == 0 { 1 } else { 2 };
        sum += product / 36 + product % 36;
    }
    Some(ALPHABET[((36 - sum % 36) % 36) as usize])
}

pub fn normalize(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

pub fn valid_gstin(gstin: &str) -> bool {
    let g = gstin.as_bytes();
    if g.len() != 15
        || !g
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        return false;
    }
    let state_ok = g[..2].iter().all(u8::is_ascii_digit)
        && gstin[..2]
            .parse::<u8>()
            .is_ok_and(|n| (1..=38).contains(&n));
    let pan_ok = g[2..7].iter().all(u8::is_ascii_uppercase)
        && g[7..11].iter().all(u8::is_ascii_digit)
        && g[11].is_ascii_uppercase();
    let entity_ok = matches!(g[12], b'1'..=b'9' | b'A'..=b'Z');
    state_ok && pan_ok && entity_ok && g[13] == b'Z' && check_char(&g[..14]) == Some(g[14])
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN_VALID: [&str; 5] = [
        "27AAPFU0939F1ZV",
        "29AAGCB7383J1Z4",
        "24AAACC1206D1ZM",
        "27AAACR5055K1Z7",
        "09AAACH7409R1ZZ",
    ];

    #[test]
    fn known_valid_numbers_pass() {
        for g in KNOWN_VALID {
            assert!(valid_gstin(g), "{g}");
            assert_eq!(check_char(&g.as_bytes()[..14]), Some(g.as_bytes()[14]));
        }
    }

    #[test]
    fn a_wrong_check_character_fails() {
        for g in KNOWN_VALID {
            let mut b = g.as_bytes().to_vec();
            b[14] = if b[14] == b'A' { b'B' } else { b'A' };
            assert!(!valid_gstin(std::str::from_utf8(&b).unwrap()), "{g}");
        }
    }

    #[test]
    fn the_shape_is_checked() {
        for bad in [
            "",
            "27AAPFU0939F1Z",
            "27AAPFU0939F1ZVX",
            "00AAPFU0939F1ZV",
            "39AAPFU0939F1ZV",
            "27AAPF10939F1ZV",
            "27AAPFUA939F1ZV",
            "27AAPFU093911ZV",
            "27AAPFU0939F0ZV",
            "27AAPFU0939F1YV",
            "27aapfu0939f1zv",
        ] {
            assert!(!valid_gstin(bad), "{bad}");
        }
        assert_eq!(normalize(" 27aapfu0939f1zv "), "27AAPFU0939F1ZV");
        assert!(valid_gstin(&normalize("27aapfu0939f1zv")));
    }
}

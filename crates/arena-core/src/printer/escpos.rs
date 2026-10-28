use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use super::PaperWidth;

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;
const LF: u8 = 0x0A;
const RULER_DIGITS: &str = "1234567890";
const TEST_OK: &str = "Printer test OK";

/// The drawer pulse: ESC p, pin 0, 25 ms on, 250 ms off (the RJ11 drawer port of a receipt
/// printer). The drawer opens on the pulse and nothing prints.
pub fn drawer_kick() -> Vec<u8> {
    vec![ESC, b'p', 0, 25, 250]
}

pub fn pc437_text(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .collect()
}

pub fn ruler(columns: usize) -> String {
    RULER_DIGITS.chars().cycle().take(columns).collect()
}

pub fn test_page_date(now_ms: i64, tz: &str) -> String {
    let utc = DateTime::<Utc>::from_timestamp_millis(now_ms).unwrap_or_default();
    match tz.parse::<Tz>() {
        Ok(zone) => utc.with_timezone(&zone).format("%-d %b %Y").to_string(),
        Err(_) => utc.format("%-d %b %Y").to_string(),
    }
}

pub fn test_page(branch_name: &str, date: &str, paper: PaperWidth) -> Vec<u8> {
    let columns = paper.columns();
    let mut name = pc437_text(branch_name.trim());
    name.truncate(columns / 2);
    let mut out = Vec::with_capacity(160);
    out.extend_from_slice(&[ESC, b'@']);
    out.extend_from_slice(&[ESC, b't', 0]);
    out.extend_from_slice(&[ESC, b'M', 0]);
    out.extend_from_slice(&[ESC, b'a', 1]);
    out.extend_from_slice(&[GS, b'!', 0x11]);
    out.extend_from_slice(&name);
    out.push(LF);
    out.extend_from_slice(&[GS, b'!', 0x00]);
    out.extend_from_slice(&pc437_text(date));
    out.push(LF);
    out.extend_from_slice(TEST_OK.as_bytes());
    out.push(LF);
    out.extend_from_slice(&[ESC, b'a', 0]);
    out.extend_from_slice(ruler(columns).as_bytes());
    out.push(LF);
    out.extend_from_slice(&[ESC, b'd', 4]);
    out.extend_from_slice(&[GS, b'V', 66, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drawer_pulse_is_esc_p_on_pin_0() {
        assert_eq!(drawer_kick(), b"\x1bp\x00\x19\xfa");
    }

    const GOLDEN_80: &[u8] = b"\x1b@\x1bt\x00\x1bM\x00\x1ba\x01\x1d!\x11Dev Branch\n\x1d!\x00\
5 Oct 2026\nPrinter test OK\n\x1ba\x00\
123456789012345678901234567890123456789012345678\n\x1bd\x04\x1dVB\x00";

    const GOLDEN_58: &[u8] = b"\x1b@\x1bt\x00\x1bM\x00\x1ba\x01\x1d!\x11Dev Branch\n\x1d!\x00\
5 Oct 2026\nPrinter test OK\n\x1ba\x00\
12345678901234567890123456789012\n\x1bd\x04\x1dVB\x00";

    const GOLDEN_58_LONG: &[u8] =
        b"\x1b@\x1bt\x00\x1bM\x00\x1ba\x01\x1d!\x11Caf? Ar?na Koram\n\x1d!\x00\
31 Dec 2026\nPrinter test OK\n\x1ba\x00\
12345678901234567890123456789012\n\x1bd\x04\x1dVB\x00";

    const GOLDEN_80_LONG: &[u8] =
        b"\x1b@\x1bt\x00\x1bM\x00\x1ba\x01\x1d!\x11Caf? Ar?na Koramangala G\n\x1d!\x00\
31 Dec 2026\nPrinter test OK\n\x1ba\x00\
123456789012345678901234567890123456789012345678\n\x1bd\x04\x1dVB\x00";

    #[test]
    fn the_80_mm_page_is_exactly_the_golden_bytes() {
        assert_eq!(
            test_page("Dev Branch", "5 Oct 2026", PaperWidth::Mm80),
            GOLDEN_80
        );
    }

    #[test]
    fn the_58_mm_page_has_a_32_column_ruler() {
        let page = test_page("Dev Branch", "5 Oct 2026", PaperWidth::Mm58);
        assert_eq!(page, GOLDEN_58);
        assert!(
            page.windows(33)
                .any(|w| w == b"12345678901234567890123456789012\n")
        );
    }

    #[test]
    fn a_long_non_ascii_name_is_transliterated_and_cut_to_half_the_width() {
        let name = "Café Arëna Koramangala Gaming Lounge";
        assert_eq!(
            test_page(name, "31 Dec 2026", PaperWidth::Mm58),
            GOLDEN_58_LONG
        );
        assert_eq!(
            test_page(name, "31 Dec 2026", PaperWidth::Mm80),
            GOLDEN_80_LONG
        );
    }

    #[test]
    fn text_is_pc437_safe() {
        assert_eq!(pc437_text("a\tb₹é"), b"a?b??");
        assert_eq!(ruler(12), "123456789012");
        assert_eq!(ruler(0), "");
    }

    #[test]
    fn the_date_is_the_branch_calendar_date() {
        let late_utc = 1_798_745_400_000;
        assert_eq!(test_page_date(late_utc, "Asia/Kolkata"), "1 Jan 2027");
        assert_eq!(test_page_date(late_utc, "UTC"), "31 Dec 2026");
        assert_eq!(test_page_date(late_utc, "Mars/Base"), "31 Dec 2026");
        assert_eq!(
            test_page_date(1_791_158_400_000, "Asia/Kolkata"),
            "5 Oct 2026"
        );
    }
}

//! The ISO 4217 currencies a branch may use: code, minor-unit exponent and display symbol.
//!
//! This table is the only list: `branch.bootstrap` and `settings.set` validate against it, and
//! `pnpm gen:types` writes it to `packages/ui/src/generated/currencies.json` for the shared money
//! formatter (through the `export-ts` test below). The same test run writes
//! `timezones.json`, the IANA zone names the Hub's time zone picker offers.

/// `(code, exponent, symbol)`, ordered by code. Money is stored in minor units: an amount of
/// `n` minor units is `n / 10^exponent` of the currency.
pub const CURRENCIES: &[(&str, u8, &str)] = &[
    ("AED", 2, "د.إ"),
    ("AUD", 2, "A$"),
    ("BDT", 2, "৳"),
    ("BHD", 3, "BD"),
    ("CAD", 2, "C$"),
    ("CHF", 2, "CHF"),
    ("EUR", 2, "€"),
    ("GBP", 2, "£"),
    ("INR", 2, "₹"),
    ("JPY", 0, "¥"),
    ("KWD", 3, "KD"),
    ("LKR", 2, "Rs"),
    ("MYR", 2, "RM"),
    ("NPR", 2, "रु"),
    ("OMR", 3, "RO"),
    ("QAR", 2, "QR"),
    ("SAR", 2, "SR"),
    ("SGD", 2, "S$"),
    ("THB", 2, "฿"),
    ("USD", 2, "$"),
];

/// One row of [`CURRENCIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Currency {
    pub code: &'static str,
    pub exponent: u8,
    pub symbol: &'static str,
}

/// The currency with ISO code `code` (exact, upper case), if this build knows it.
pub fn currency(code: &str) -> Option<Currency> {
    CURRENCIES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|&(code, exponent, symbol)| Currency {
            code,
            exponent,
            symbol,
        })
}

/// Whether `code` is in [`CURRENCIES`].
pub fn is_known_currency(code: &str) -> bool {
    currency(code).is_some()
}

/// The table as the JSON array the UI reads: `[{code, exponent, symbol}]`.
pub fn currencies_json() -> String {
    let rows: Vec<serde_json::Value> = CURRENCIES
        .iter()
        .map(|(code, exponent, symbol)| {
            serde_json::json!({ "code": code, "exponent": exponent, "symbol": symbol })
        })
        .collect();
    let mut out = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_required_currencies_are_present() {
        for (code, exponent, symbol) in [
            ("INR", 2, "₹"),
            ("USD", 2, "$"),
            ("EUR", 2, "€"),
            ("GBP", 2, "£"),
            ("AED", 2, "د.إ"),
            ("SGD", 2, "S$"),
            ("JPY", 0, "¥"),
            ("KWD", 3, "KD"),
            ("BHD", 3, "BD"),
        ] {
            assert_eq!(
                currency(code),
                Some(Currency {
                    code,
                    exponent,
                    symbol
                }),
                "{code}"
            );
        }
        assert!(!is_known_currency("XYZ"));
        assert!(!is_known_currency("inr"));
    }

    #[test]
    fn codes_are_unique_sorted_iso_shaped() {
        let codes: Vec<&str> = CURRENCIES.iter().map(|(c, _, _)| *c).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
        for (code, exponent, symbol) in CURRENCIES {
            assert!(code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()));
            assert!(*exponent <= 3);
            assert!(!symbol.is_empty());
        }
    }

    #[test]
    fn json_lists_every_row() {
        let v: serde_json::Value = serde_json::from_str(&currencies_json()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), CURRENCIES.len());
        assert_eq!(
            v[0],
            serde_json::json!({"code": "AED", "exponent": 2, "symbol": "د.إ"})
        );
    }

    /// Canonical IANA zone names from the bundled tz database (the one `branch.tz` is validated
    /// against): the region zones plus `UTC`, without the backward-compatibility areas
    /// (`US/…`, `Etc/…`, single-word legacy names). Sorted.
    fn canonical_timezones() -> Vec<&'static str> {
        const AREAS: [&str; 10] = [
            "Africa/",
            "America/",
            "Antarctica/",
            "Arctic/",
            "Asia/",
            "Atlantic/",
            "Australia/",
            "Europe/",
            "Indian/",
            "Pacific/",
        ];
        let mut names: Vec<&'static str> = chrono_tz::TZ_VARIANTS
            .iter()
            .map(|tz| tz.name())
            .filter(|n| *n == "UTC" || AREAS.iter().any(|a| n.starts_with(a)))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    #[test]
    fn the_timezone_list_has_utc_and_the_branch_zones() {
        let names = canonical_timezones();
        for z in [
            "UTC",
            "Asia/Kolkata",
            "Asia/Dubai",
            "Europe/London",
            "America/New_York",
        ] {
            assert!(names.contains(&z), "{z}");
        }
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("US/") || n.starts_with("Etc/"))
        );
    }

    #[cfg(feature = "export-ts")]
    #[test]
    fn export_timezones_json() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "./bindings".into());
        std::fs::create_dir_all(&dir).unwrap();
        let mut out = serde_json::to_string_pretty(&canonical_timezones()).unwrap();
        out.push('\n');
        std::fs::write(std::path::Path::new(&dir).join("timezones.json"), out).unwrap();
    }

    /// `pnpm gen:types` runs the tests with `export-ts` and `TS_RS_EXPORT_DIR` set: write the
    /// table next to the generated TypeScript types.
    #[cfg(feature = "export-ts")]
    #[test]
    fn export_currencies_json() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "./bindings".into());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("currencies.json"),
            currencies_json(),
        )
        .unwrap();
    }
}

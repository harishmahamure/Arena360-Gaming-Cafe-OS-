use serde::{Deserialize, Serialize};

pub const KEY_TAX: &str = "branch.tax";
pub const DEFAULT_TAX: &str = r#"{"country":"IN","gst":false,"gstin":"","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"","label":"Tax","rate_bp":0}"#;
pub const GST_MAX_BP: u16 = 2800;
pub const SIMPLE_MAX_BP: u16 = 10000;
pub const GSTIN_INVALID: &str = "GSTIN_INVALID";
pub const RATE_RANGE: &str = "RATE_RANGE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum TaxMode {
    GstIn,
    Simple,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxSettings {
    pub country: String,
    pub gst: bool,
    pub gstin: String,
    pub inclusive: bool,
    pub product_bp: u16,
    pub play_bp: u16,
    pub sac: String,
    pub label: String,
    pub rate_bp: u16,
}

const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn gstin_valid(s: &str) -> bool {
    let b = s.as_bytes();
    let state = s
        .get(..2)
        .filter(|v| v.bytes().all(|c| c.is_ascii_digit()))
        .and_then(|v| v.parse::<u8>().ok());
    let shape = b.len() == 15
        && matches!(state, Some(1..=38 | 97 | 99))
        && b[2..7].iter().all(u8::is_ascii_uppercase)
        && b[7..11].iter().all(u8::is_ascii_digit)
        && b[11].is_ascii_uppercase()
        && matches!(b[12], b'1'..=b'9' | b'A'..=b'Z')
        && b[13] == b'Z'
        && b[14].is_ascii_alphanumeric();
    if !shape {
        return false;
    }
    let sum: usize = b[..14]
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let p = ALPHABET.iter().position(|a| a == c).unwrap_or(0) * (1 + i % 2);
            p / 36 + p % 36
        })
        .sum();
    ALPHABET[(36 - sum % 36) % 36] == b[14]
}

/// The longest legal name a business invoice keeps.
pub const BUYER_NAME_MAX_CHARS: usize = 100;

/// The business a B2B invoice is made out to (Story 5.20): the customer's GSTIN and legal name,
/// stored on the order at settle. Only a GST branch (`TaxMode::GstIn`) takes one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BusinessBuyer {
    pub gstin: String,
    pub legal_name: String,
}

impl BusinessBuyer {
    /// The first thing wrong with the buyer as `(field, message)`: an invalid GSTIN is
    /// [`GSTIN_INVALID`], a blank, padded, over-long or control-character name is a message.
    pub fn check(&self) -> Result<(), (&'static str, &'static str)> {
        if !gstin_valid(&self.gstin) {
            return Err(("business.gstin", GSTIN_INVALID));
        }
        let name = self.legal_name.trim();
        if name.is_empty()
            || name != self.legal_name
            || name.chars().count() > BUYER_NAME_MAX_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(("business.legal_name", "legal name is 1 to 100 characters"));
        }
        Ok(())
    }
}

impl TaxSettings {
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_str(value).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn tax_mode(&self) -> TaxMode {
        if self.country == "IN" && self.gst {
            TaxMode::GstIn
        } else {
            TaxMode::Simple
        }
    }

    pub fn rates(&self) -> (u16, u16) {
        match self.tax_mode() {
            TaxMode::GstIn => (self.product_bp, self.play_bp),
            TaxMode::Simple => (self.rate_bp, self.rate_bp),
        }
    }

    pub fn check(&self) -> Result<(), (&'static str, &'static str)> {
        let india = self.country == "IN";
        if self.country.len() != 2 || !self.country.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err((
                "branch.tax.country",
                "country must be an ISO 3166-1 alpha-2 code",
            ));
        }
        if self.gst && !india {
            return Err(("branch.tax.gst", "GST applies only in India"));
        }
        if if self.gst {
            !gstin_valid(&self.gstin)
        } else {
            !self.gstin.is_empty()
        } {
            return Err(("branch.tax.gstin", GSTIN_INVALID));
        }
        let p_max = if india { GST_MAX_BP } else { SIMPLE_MAX_BP };
        for (f, v, max) in [
            ("branch.tax.product_bp", self.product_bp, p_max),
            ("branch.tax.play_bp", self.play_bp, p_max),
            ("branch.tax.rate_bp", self.rate_bp, SIMPLE_MAX_BP),
        ] {
            if v > max {
                return Err((f, RATE_RANGE));
            }
        }
        if !(self.sac.is_empty()
            || (4..=8).contains(&self.sac.len()) && self.sac.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(("branch.tax.sac", "SAC code is 4 to 8 digits"));
        }
        let label = self.label.trim();
        if label.is_empty()
            || label != self.label
            || label.chars().count() > 20
            || label.chars().any(char::is_control)
        {
            return Err(("branch.tax.label", "label is 1 to 20 characters"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tax(f: impl FnOnce(&mut TaxSettings)) -> Result<(), (&'static str, &'static str)> {
        let mut t = TaxSettings::parse(DEFAULT_TAX).unwrap();
        f(&mut t);
        t.check()
    }

    #[test]
    fn gstin_checksum_and_shape() {
        assert!(gstin_valid("27AAPFU0939F1ZV"));
        for bad in [
            "27AAPFU0939F1ZW",
            "00AAPFU0939F1ZV",
            "27aapfu0939f1zv",
            "27AAPFU0939F1Z",
            "27AAPFU0939F1XV",
            "",
        ] {
            assert!(!gstin_valid(bad), "{bad}");
        }
    }

    #[test]
    fn a_business_buyer_needs_a_valid_gstin_and_a_plain_legal_name() {
        let buyer = |gstin: &str, name: &str| BusinessBuyer {
            gstin: gstin.into(),
            legal_name: name.into(),
        };
        assert_eq!(buyer("27AAPFU0939F1ZV", "Acme Traders").check(), Ok(()));
        assert_eq!(
            buyer("27AAPFU0939F1ZW", "Acme").check(),
            Err(("business.gstin", GSTIN_INVALID))
        );
        assert_eq!(buyer("", "Acme").check().unwrap_err().1, GSTIN_INVALID);
        for bad in ["", " ", " Acme", "Acme ", "A\tB", &"x".repeat(101)] {
            assert_eq!(
                buyer("27AAPFU0939F1ZV", bad).check().unwrap_err().0,
                "business.legal_name",
                "{bad:?}"
            );
        }
        assert_eq!(buyer("27AAPFU0939F1ZV", &"x".repeat(100)).check(), Ok(()));
    }

    #[test]
    fn defaults_and_modes() {
        let d = TaxSettings::parse(DEFAULT_TAX).unwrap();
        assert_eq!(d.check(), Ok(()));
        assert_eq!(d.tax_mode(), TaxMode::Simple);
        assert_eq!(TaxSettings::parse(&d.to_json()), Some(d));
        let g = |t: &mut TaxSettings| {
            t.gst = true;
            t.gstin = "27AAPFU0939F1ZV".into();
        };
        let mut t = TaxSettings::parse(DEFAULT_TAX).unwrap();
        g(&mut t);
        assert_eq!((t.check(), t.tax_mode()), (Ok(()), TaxMode::GstIn));
        assert_eq!(
            tax(|t| t.gst = true),
            Err(("branch.tax.gstin", GSTIN_INVALID))
        );
        assert_eq!(
            tax(|t| t.gstin = "27AAPFU0939F1ZV".into()),
            Err(("branch.tax.gstin", GSTIN_INVALID))
        );
        assert_eq!(
            tax(|t| t.product_bp = 2801),
            Err(("branch.tax.product_bp", RATE_RANGE))
        );
        assert_eq!(tax(|t| t.play_bp = 2800), Ok(()));
        assert_eq!(
            tax(|t| t.sac = "99".into()).unwrap_err().0,
            "branch.tax.sac"
        );
        assert_eq!(tax(|t| t.sac = "999692".into()), Ok(()));
    }

    fn with_check(body: &str) -> String {
        let sum: usize = body
            .bytes()
            .enumerate()
            .map(|(i, c)| {
                let p = ALPHABET.iter().position(|a| *a == c).unwrap() * (1 + i % 2);
                p / 36 + p % 36
            })
            .sum();
        format!("{body}{}", ALPHABET[(36 - sum % 36) % 36] as char)
    }

    #[test]
    fn rates_follow_the_mode() {
        let mut t = TaxSettings::parse(DEFAULT_TAX).unwrap();
        assert_eq!(t.rates(), (0, 0));
        t.gst = true;
        assert_eq!(t.rates(), (500, 1800));
        t.country = "AE".into();
        t.rate_bp = 700;
        assert_eq!(t.rates(), (700, 700));
        let mut u = TaxSettings::parse(DEFAULT_TAX).unwrap();
        u.rate_bp = 300;
        assert_eq!(u.rates(), (300, 300));
    }

    #[test]
    fn gstin_shape_edges() {
        for ok in [
            "01AAPFU0939F1Z",
            "38AAPFU0939F1Z",
            "97AAPFU0939F1Z",
            "99AAPFU0939F1Z",
        ] {
            assert!(gstin_valid(&with_check(ok)), "{ok}");
        }
        for bad in [
            "39AAPFU0939F1Z",
            "27AAPFU0939F1X",
            "27AAPFU0939F0Z",
            "27AAPFUA939F1Z",
            "27AAPFU0939F1Z",
        ] {
            let g = with_check(bad);
            assert!(!gstin_valid(&g) || bad == "27AAPFU0939F1Z", "{g}");
        }
        assert!(!gstin_valid(&format!("{}0", with_check("27AAPFU0939F1Z"))));
    }

    #[test]
    fn field_bounds() {
        assert_eq!(tax(|t| t.product_bp = 2800), Ok(()));
        assert_eq!(
            tax(|t| t.play_bp = 2801),
            Err(("branch.tax.play_bp", RATE_RANGE))
        );
        for sac in ["123", "123456789", "12AB56"] {
            assert_eq!(
                tax(|t| t.sac = sac.into()).unwrap_err().0,
                "branch.tax.sac",
                "{sac}"
            );
        }
        assert_eq!(tax(|t| t.label = "X".repeat(20)), Ok(()));
        for label in ["X".repeat(21), " VAT".into(), "V\tAT".into(), String::new()] {
            assert_eq!(
                tax(|t| t.label = label.clone()).unwrap_err().0,
                "branch.tax.label"
            );
        }
        for c in ["IND", ""] {
            assert_eq!(
                tax(|t| t.country = c.into()).unwrap_err().0,
                "branch.tax.country"
            );
        }
        let mut t = TaxSettings::parse(DEFAULT_TAX).unwrap();
        t.country = "AE".into();
        t.gst = true;
        assert_eq!(t.tax_mode(), TaxMode::Simple);
        for bad in [
            DEFAULT_TAX.replace('}', r#","x":1}"#),
            DEFAULT_TAX.replace("500", "-1"),
            DEFAULT_TAX.replace("500", "70000"),
        ] {
            assert!(TaxSettings::parse(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn simple_tax_outside_india() {
        let out = |f: fn(&mut TaxSettings)| {
            tax(|t| {
                t.country = "AE".into();
                f(t);
            })
        };
        assert_eq!(out(|t| t.rate_bp = 10000), Ok(()));
        assert_eq!(
            out(|t| t.rate_bp = 10001),
            Err(("branch.tax.rate_bp", RATE_RANGE))
        );
        assert_eq!(out(|t| t.gst = true).unwrap_err().0, "branch.tax.gst");
        assert_eq!(
            out(|t| t.label = " ".into()).unwrap_err().0,
            "branch.tax.label"
        );
        assert_eq!(out(|t| t.product_bp = 5000), Ok(()));
        assert_eq!(
            out(|t| t.product_bp = 10001).unwrap_err().0,
            "branch.tax.product_bp"
        );
        assert_eq!(
            tax(|t| t.rate_bp = 10001).unwrap_err().0,
            "branch.tax.rate_bp"
        );
        assert_eq!(
            tax(|t| t.country = "in".into()).unwrap_err().0,
            "branch.tax.country"
        );
        assert!(TaxSettings::parse(r#"{"country":"IN"}"#).is_none());
    }
}

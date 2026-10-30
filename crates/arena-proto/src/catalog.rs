use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_IMAGE_BYTES: usize = 200 * 1024;
pub const MAX_IMAGE_PX: u16 = 512;
pub const IMAGE_PREFIX: &str = "data:image/jpeg;base64,";
pub const MAX_PRODUCT_NAME_CHARS: usize = 80;
pub const MAX_CATEGORY_CHARS: usize = 40;
pub const MAX_BARCODE_CHARS: usize = 48;
pub const MAX_PRICE_MINOR: i64 = 1_000_000_000_000;
pub const REASON_INACTIVE: &str = "inactive";
pub const REASON_OUT_OF_STOCK: &str = "out_of_stock";
/// Every unit left is in a lot past its expiry date (Story 5.7).
pub const REASON_EXPIRED: &str = "expired";
pub const REASON_COMPONENT_INACTIVE: &str = "component_inactive";
pub const MAX_BUNDLE_COMPONENTS: usize = 30;
pub const MAX_COMPONENT_QTY: i64 = 1000;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BundleComponent {
    pub product_id: String,
    #[ts(type = "number")]
    pub qty: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ComponentView {
    pub product_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub qty: i64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductUpsert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub name: String,
    pub category: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_rate_bp: Option<u16>,
    pub hsn: String,
    #[serde(default = "yes")]
    pub track_stock: bool,
    /// Story 8.24: an owner command without an image keeps the stored one (`IMAGE_KEEP` in
    /// `arena_core::commands`), so the field has a default.
    #[serde(default)]
    pub image: String,
    pub barcode: String,
    pub kitchen: bool,
    pub favourite: bool,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub bundle: bool,
    #[serde(default)]
    pub components: Vec<BundleComponent>,
    /// Story 7.2: the preset kitchen notes (up to 8 of up to 40 characters); `None` leaves the
    /// stored list as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kitchen_notes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductDelete {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Product {
    pub id: String,
    pub name: String,
    pub category: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    pub tax_rate_bp: u16,
    pub hsn: String,
    pub track_stock: bool,
    pub image: String,
    pub barcode: String,
    pub kitchen: bool,
    pub favourite: bool,
    pub active: bool,
    pub bundle: bool,
    pub components: Vec<ComponentView>,
    /// Story 7.2: the preset kitchen notes of a kitchen product.
    #[serde(default)]
    pub kitchen_notes: Vec<String>,
    #[ts(type = "number")]
    pub rev: i64,
    #[ts(type = "number")]
    pub quantity: i64,
    #[ts(type = "number")]
    pub avg_cost_minor: i64,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogEntry {
    pub product_id: String,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

pub fn availability(
    active: bool,
    deleted: bool,
    track_stock: bool,
    quantity: i64,
) -> (bool, Option<&'static str>) {
    if !active || deleted {
        (false, Some(REASON_INACTIVE))
    } else if track_stock && quantity <= 0 {
        (false, Some(REASON_OUT_OF_STOCK))
    } else {
        (true, None)
    }
}

pub fn bundle_quantity(parts: impl IntoIterator<Item = (i64, i64)>) -> Option<i64> {
    parts
        .into_iter()
        .map(|(have, need)| have.max(0) / need.max(1))
        .min()
}

pub fn hsn_valid(s: &str) -> bool {
    matches!(s.len(), 4 | 6 | 8) && s.bytes().all(|b| b.is_ascii_digit())
}

fn jpeg_dims(b: &[u8]) -> Option<(u16, u16)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        match m {
            0xFF => i += 1,
            0x01 | 0xD0..=0xD8 => i += 2,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                let s = b.get(i + 5..i + 9)?;
                return Some((
                    u16::from_be_bytes([s[2], s[3]]),
                    u16::from_be_bytes([s[0], s[1]]),
                ));
            }
            _ => i += 2 + usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]])),
        }
    }
    None
}

pub fn image_problem(image: &str) -> Option<&'static str> {
    if image.is_empty() {
        return None;
    }
    let Some(data) = image.strip_prefix(IMAGE_PREFIX) else {
        return Some("the image must be a JPEG");
    };
    if data.len() > MAX_IMAGE_BYTES * 4 / 3 + 4 {
        return Some("the image is over 200 KB");
    }
    let Ok(bytes) = STANDARD.decode(data) else {
        return Some("the image is not valid");
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return Some("the image is over 200 KB");
    }
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("the image must be a JPEG");
    }
    match jpeg_dims(&bytes) {
        Some((w, h)) if w == 0 || h == 0 => Some("the image is not valid"),
        Some((w, h)) if w <= MAX_IMAGE_PX && h <= MAX_IMAGE_PX => None,
        Some(_) => Some("the image is over 512 px"),
        None => Some("the image is not valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u16, h: u16) -> String {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        b.extend([0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([0x01, 0x01, 0x11, 0x00]);
        format!("{IMAGE_PREFIX}{}", STANDARD.encode(b))
    }

    #[test]
    fn hsn_is_4_6_or_8_digits() {
        assert!(hsn_valid("2106") && hsn_valid("210690") && hsn_valid("21069099"));
        assert!(
            !hsn_valid("") && !hsn_valid("210") && !hsn_valid("21069") && !hsn_valid("210690990")
        );
        assert!(!hsn_valid("21A6") && !hsn_valid("+210"));
    }

    #[test]
    fn availability_reasons() {
        assert_eq!(availability(true, false, false, 0), (true, None));
        assert_eq!(availability(true, false, true, 3), (true, None));
        assert_eq!(
            availability(true, false, true, 0),
            (false, Some("out_of_stock"))
        );
        assert_eq!(
            availability(false, false, true, 5),
            (false, Some("inactive"))
        );
        assert_eq!(
            availability(true, true, false, 5),
            (false, Some("inactive"))
        );
    }

    #[test]
    fn a_bundle_is_the_minimum_of_its_components_floored() {
        assert_eq!(bundle_quantity([]), None);
        assert_eq!(bundle_quantity([(10, 2), (7, 3)]), Some(2));
        assert_eq!(bundle_quantity([(5, 1), (1, 2)]), Some(0));
        assert_eq!(bundle_quantity([(-4, 1)]), Some(0));
        assert_eq!(bundle_quantity([(9, 3)]), Some(3));
    }

    #[test]
    fn images_are_small_jpegs() {
        assert_eq!(image_problem(""), None);
        assert_eq!(image_problem(&jpeg(512, 300)), None);
        assert_eq!(
            image_problem(&jpeg(513, 300)),
            Some("the image is over 512 px")
        );
        assert_eq!(image_problem(&jpeg(0, 10)), Some("the image is not valid"));
        assert!(image_problem("data:image/png;base64,AAAA").is_some());
        assert!(image_problem("data:image/jpeg;base64,!!!").is_some());
        let text = format!("{IMAGE_PREFIX}{}", STANDARD.encode(b"hello world"));
        assert_eq!(image_problem(&text), Some("the image must be a JPEG"));
        let mut big = vec![0xFF, 0xD8, 0xFF];
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        let big = format!("{IMAGE_PREFIX}{}", STANDARD.encode(big));
        assert_eq!(image_problem(&big), Some("the image is over 200 KB"));
        let mut edge = vec![0xFF, 0xD8, 0xFF];
        edge.resize(MAX_IMAGE_BYTES, 0);
        let edge = format!("{IMAGE_PREFIX}{}", STANDARD.encode(edge));
        assert_eq!(image_problem(&edge), Some("the image is not valid"));
    }

    #[test]
    fn real_encoder_output_is_measured_correctly() {
        const BASELINE_512: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAA0JCgsKCA0LCgsODg0PEyAVExISEyccHhcgLikxMC4pLSwzOko+MzZGNywtQFdBRkxOUlNSMj5aYVpQYEpRUk//2wBDAQ4ODhMREyYVFSZPNS01T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT0//wAARCAIAAgADASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDLooorhPqwooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigD/9k=";
        const PROGRESSIVE_512X10: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAA0JCgsKCA0LCgsODg0PEyAVExISEyccHhcgLikxMC4pLSwzOko+MzZGNywtQFdBRkxOUlNSMj5aYVpQYEpRUk//2wBDAQ4ODhMREyYVFSZPNS01T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT0//wgARCAAKAgADASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAT/xAAWAQEBAQAAAAAAAAAAAAAAAAAAAwX/2gAMAwEAAhADEAAAAZRDVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/8QAFBABAAAAAAAAAAAAAAAAAAAAcP/aAAgBAQABBQIA/8QAFBEBAAAAAAAAAAAAAAAAAAAAUP/aAAgBAwEBPwED/8QAFBEBAAAAAAAAAAAAAAAAAAAAUP/aAAgBAgEBPwED/8QAFBABAAAAAAAAAAAAAAAAAAAAcP/aAAgBAQAGPwIA/8QAFBABAAAAAAAAAAAAAAAAAAAAcP/aAAgBAQABPyEA/9oADAMBAAIAAwAAABAEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEH/xAAUEQEAAAAAAAAAAAAAAAAAAABQ/9oACAEDAQE/EAP/xAAUEQEAAAAAAAAAAAAAAAAAAABQ/9oACAECAQE/EAP/xAAUEAEAAAAAAAAAAAAAAAAAAABw/9oACAEBAAE/EAD/2Q==";
        const BASELINE_10X513: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAA0JCgsKCA0LCgsODg0PEyAVExISEyccHhcgLikxMC4pLSwzOko+MzZGNywtQFdBRkxOUlNSMj5aYVpQYEpRUk//2wBDAQ4ODhMREyYVFSZPNS01T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT0//wAARCAIBAAoDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDLooorhPqwooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKAP/2Q==";
        const PROGRESSIVE_513: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAA0JCgsKCA0LCgsODg0PEyAVExISEyccHhcgLikxMC4pLSwzOko+MzZGNywtQFdBRkxOUlNSMj5aYVpQYEpRUk//2wBDAQ4ODhMREyYVFSZPNS01T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT0//wgARCAAUAgEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAT/xAAWAQEBAQAAAAAAAAAAAAAAAAAAAwX/2gAMAwEAAhADEAAAAZRDVAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA//xAAUEAEAAAAAAAAAAAAAAAAAAABw/9oACAEBAAEFAkP/xAAUEQEAAAAAAAAAAAAAAAAAAABg/9oACAEDAQE/AQX/xAAUEQEAAAAAAAAAAAAAAAAAAABg/9oACAECAQE/AQX/xAAUEAEAAAAAAAAAAAAAAAAAAABw/9oACAEBAAY/AkP/xAAUEAEAAAAAAAAAAAAAAAAAAABw/9oACAEBAAE/IUP/2gAMAwEAAgADAAAAEAQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQf/8QAFBEBAAAAAAAAAAAAAAAAAAAAYP/aAAgBAwEBPxAF/8QAFBEBAAAAAAAAAAAAAAAAAAAAYP/aAAgBAgEBPxAF/8QAFBABAAAAAAAAAAAAAAAAAAAAcP/aAAgBAQABPxBD/9k=";

        let uri = |b: &str| format!("{IMAGE_PREFIX}{b}");
        assert_eq!(image_problem(&uri(BASELINE_512)), None);
        assert_eq!(image_problem(&uri(PROGRESSIVE_512X10)), None);
        assert_eq!(
            image_problem(&uri(BASELINE_10X513)),
            Some("the image is over 512 px")
        );
        assert_eq!(
            image_problem(&uri(PROGRESSIVE_513)),
            Some("the image is over 512 px")
        );
        assert_eq!(image_problem(&jpeg(10, 512)), None);
        assert_eq!(image_problem(&jpeg(512, 512)), None);
        assert_eq!(image_problem(&jpeg(10, 0)), Some("the image is not valid"));
    }
}

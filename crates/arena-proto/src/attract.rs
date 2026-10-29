//! The kiosk attract screen (Story 6.7): the per-device view the Hub pushes every tick and the
//! branch logo setting.
//!
//! - [`AttractView`] is computed by the Hub for one device (the kiosk does not know its group)
//!   and pushed as `HubEvent::Attract` on the 5 s fan-out tick, so a rule that starts or ends
//!   reaches the screen within seconds. The kiosk service caches the last one.
//! - The logo is the `kiosk.logo` setting (a PNG or SVG data URI, empty for none), validated
//!   by [`logo_problem`] when a manager sets it. It travels with the other `kiosk.*` settings,
//!   rev-ordered and cached on the kiosk, not on the tick.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::{SnapshotActiveRule, SnapshotLocale};

/// The setting that holds the branch logo (a data URI, or empty for none).
pub const KEY_KIOSK_LOGO: &str = "kiosk.logo";
/// `kiosk.logo` before a manager sets one.
pub const DEFAULT_KIOSK_LOGO: &str = "";
/// The logo's `data:` prefixes.
pub const LOGO_PNG_PREFIX: &str = "data:image/png;base64,";
pub const LOGO_SVG_PREFIX: &str = "data:image/svg+xml;base64,";
/// Largest decoded logo, in bytes.
pub const MAX_LOGO_BYTES: usize = 200 * 1024;
/// Largest logo side, in pixels (a PNG's header; an SVG scales).
pub const MAX_LOGO_PX: u32 = 512;
/// Longest `kiosk.logo` value, in characters: a 200 KB logo in base64 plus its prefix.
pub const MAX_LOGO_VALUE_CHARS: usize = MAX_LOGO_BYTES.div_ceil(3) * 4 + 32;
/// Most media items one attract view carries.
pub const MAX_ATTRACT_MEDIA: usize = 20;

/// Why a PC shows no rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AttractState {
    /// The PC is assigned to a group with a rate: the screen shows it.
    Ready,
    /// Unassigned, in maintenance, or its group has no active rate: "Ask at the counter".
    AskCounter,
}

/// One curated video of the branch library, by content hash, and its cover when cached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AttractMedia {
    /// The SHA-256 of the stored video (`GET /media/<sha256>` on the Hub).
    pub video: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cover: Option<String>,
}

/// What one idle kiosk shows (`event: attract`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AttractView {
    pub branch_name: String,
    /// This PC's label (the login panel's footer, Story 6.8).
    #[serde(default)]
    pub device_name: String,
    /// This PC's floor zone label, empty when it has none.
    #[serde(default)]
    pub zone: String,
    pub state: AttractState,
    /// The hourly rate in minor units now: the active rule's when one applies (`None` when
    /// `state` is `ask_counter`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub rate_minor: Option<i64>,
    /// This PC's group (`None` when it has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_name: Option<String>,
    /// The pricing rule in effect for the group (never with `ask_counter`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rule: Option<SnapshotActiveRule>,
    pub locale: SnapshotLocale,
    /// Up to [`MAX_ATTRACT_MEDIA`] videos of enabled, catalogue-linked library entries, in name
    /// order.
    pub media: Vec<AttractMedia>,
    /// What this PC owes the counter in minor units: its open unpaid counter orders (Story 6.32).
    /// When above zero the kiosk shows "Pay at counter" and takes no login.
    #[serde(default)]
    #[ts(type = "number")]
    pub unpaid_minor: i64,
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl AttractView {
    /// Whether every media hash is a SHA-256 and the list is within [`MAX_ATTRACT_MEDIA`] (the
    /// kiosk drops a view that is not).
    pub fn is_sane(&self) -> bool {
        self.media.len() <= MAX_ATTRACT_MEDIA
            && self
                .media
                .iter()
                .all(|m| is_sha256(&m.video) && m.cover.as_deref().is_none_or(is_sha256))
            && (self.state == AttractState::Ready
                || (self.rate_minor.is_none() && self.rule.is_none()))
    }
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn png_problem(bytes: &[u8]) -> Option<&'static str> {
    // Signature, then the IHDR chunk: length 13, "IHDR", width, height.
    if bytes.len() < 33 || bytes[..8] != PNG_SIGNATURE || bytes[12..16] != *b"IHDR" {
        return Some("the logo is not a valid PNG");
    }
    let side =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (w, h) = (side(16), side(20));
    if side(8) != 13 || w == 0 || h == 0 {
        return Some("the logo is not a valid PNG");
    }
    if w > MAX_LOGO_PX || h > MAX_LOGO_PX {
        return Some("the logo is over 512 px");
    }
    None
}

/// Whether the value that follows an `href` or `url(` stays inside the document (`#id`).
fn is_internal_ref(value: &str) -> bool {
    value
        .trim_start_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .starts_with('#')
}

/// The `href=` / `xlink:href=` values of `lower` (a lower-cased SVG).
fn href_values(lower: &str) -> impl Iterator<Item = &str> {
    lower.match_indices("href").filter_map(move |(at, _)| {
        let rest = lower[at + 4..].trim_start();
        let rest = rest.strip_prefix('=')?;
        Some(rest.trim_start())
    })
}

/// The arguments of every `url(` in `lower`.
fn url_values(lower: &str) -> impl Iterator<Item = &str> {
    lower
        .match_indices("url(")
        .map(move |(at, _)| &lower[at + 4..])
}

/// Whether an `on<event>=` attribute appears (an event handler).
fn has_event_handler(lower: &str) -> bool {
    let b = lower.as_bytes();
    b.windows(2).enumerate().any(|(i, w)| {
        if w != b"on"
            || i == 0
            || !(b[i - 1].is_ascii_whitespace() || matches!(b[i - 1], b'"' | b'\'' | b'/'))
        {
            return false;
        }
        let mut j = i + 2;
        while j < b.len() && b[j].is_ascii_lowercase() {
            j += 1;
        }
        if j == i + 2 {
            return false;
        }
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        b.get(j) == Some(&b'=')
    })
}

/// Skips the XML prolog (declaration, comments, a DOCTYPE with no internal subset) and whitespace.
fn after_prolog(mut s: &str) -> Option<&str> {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("<?") {
            s = &rest[rest.find("?>")? + 2..];
        } else if let Some(rest) = s.strip_prefix("<!--") {
            s = &rest[rest.find("-->")? + 3..];
        } else if s
            .get(..9)
            .is_some_and(|p| p.eq_ignore_ascii_case("<!doctype"))
        {
            let end = s.find('>')?;
            if s[..end].contains('[') {
                return None;
            }
            s = &s[end + 1..];
        } else {
            return Some(s);
        }
    }
}

fn svg_problem(bytes: &[u8]) -> Option<&'static str> {
    let invalid = Some("the logo is not a valid SVG");
    let Ok(text) = std::str::from_utf8(bytes) else {
        return invalid;
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(body) = after_prolog(text) else {
        return invalid;
    };
    let lower = body.to_ascii_lowercase();
    let root_ok = lower
        .strip_prefix("<svg")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/'));
    if !root_ok {
        return invalid;
    }
    let all = text.to_ascii_lowercase();
    const BANNED: [&str; 9] = [
        "<script",
        "<foreignobject",
        "<iframe",
        "<embed",
        "<object",
        "<!entity",
        "<!doctype",
        "javascript:",
        "@import",
    ];
    // A DOCTYPE was already vetted in the prolog; a second one in the body is not allowed.
    if BANNED.iter().any(|b| lower.contains(b)) {
        return Some("the logo has active content");
    }
    if has_event_handler(&lower) || all.contains("<!entity") {
        return Some("the logo has active content");
    }
    if href_values(&lower).any(|v| !is_internal_ref(v))
        || url_values(&lower).any(|v| !is_internal_ref(v))
    {
        return Some("the logo points outside itself");
    }
    None
}

/// Why `value` cannot be the branch logo (`None` when it can, or when it is empty = no logo):
/// a `data:image/png;base64,` or `data:image/svg+xml;base64,` URI of at most 200 KB decoded.
///
/// - A PNG has its signature and IHDR and is at most 512 px wide and high.
/// - An SVG is UTF-8 with an `<svg` root and no script, `<foreignObject`, event handler,
///   `javascript:`, entity declaration, or `href` / `url(` pointing outside the document.
pub fn logo_problem(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        return None;
    }
    let (data, png) = if let Some(d) = value.strip_prefix(LOGO_PNG_PREFIX) {
        (d, true)
    } else if let Some(d) = value.strip_prefix(LOGO_SVG_PREFIX) {
        (d, false)
    } else {
        return Some("the logo must be a PNG or SVG");
    };
    if data.len() > MAX_LOGO_BYTES.div_ceil(3) * 4 {
        return Some("the logo is over 200 KB");
    }
    let Ok(bytes) = STANDARD.decode(data) else {
        return Some("the logo is not valid");
    };
    if bytes.len() > MAX_LOGO_BYTES {
        return Some("the logo is over 200 KB");
    }
    if png {
        png_problem(&bytes)
    } else {
        svg_problem(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(prefix: &str, bytes: &[u8]) -> String {
        format!("{prefix}{}", STANDARD.encode(bytes))
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend(13u32.to_be_bytes());
        b.extend(b"IHDR");
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b.extend([0; 4]);
        b
    }

    fn svg(text: &str) -> String {
        uri(LOGO_SVG_PREFIX, text.as_bytes())
    }

    #[test]
    fn no_logo_is_fine_and_the_value_limit_fits_a_200_kb_logo() {
        assert_eq!(logo_problem(""), None);
        let biggest = uri(LOGO_PNG_PREFIX, &vec![0u8; MAX_LOGO_BYTES]);
        assert!(biggest.chars().count() <= MAX_LOGO_VALUE_CHARS);
    }

    #[test]
    fn a_png_is_checked_by_signature_ihdr_and_size() {
        assert_eq!(logo_problem(&uri(LOGO_PNG_PREFIX, &png(512, 300))), None);
        assert_eq!(logo_problem(&uri(LOGO_PNG_PREFIX, &png(1, 512))), None);
        assert_eq!(
            logo_problem(&uri(LOGO_PNG_PREFIX, &png(600, 100))),
            Some("the logo is over 512 px")
        );
        assert_eq!(
            logo_problem(&uri(LOGO_PNG_PREFIX, &png(100, 513))),
            Some("the logo is over 512 px")
        );
        for bad in [
            png(0, 10),
            png(10, 0),
            b"not a png at all, just text, 33+ bytes long".to_vec(),
            vec![0x89, b'P'],
        ] {
            assert_eq!(
                logo_problem(&uri(LOGO_PNG_PREFIX, &bad)),
                Some("the logo is not a valid PNG")
            );
        }
        let mut wrong_chunk = png(10, 10);
        wrong_chunk[12..16].copy_from_slice(b"IDAT");
        assert!(logo_problem(&uri(LOGO_PNG_PREFIX, &wrong_chunk)).is_some());
        // A PNG body under the SVG prefix is not an SVG.
        assert!(logo_problem(&uri(LOGO_SVG_PREFIX, &png(10, 10))).is_some());
    }

    #[test]
    fn size_prefix_and_encoding_are_checked() {
        let mut big = png(10, 10);
        big.resize(MAX_LOGO_BYTES + 1, 0);
        assert_eq!(
            logo_problem(&uri(LOGO_PNG_PREFIX, &big)),
            Some("the logo is over 200 KB")
        );
        let mut edge = png(10, 10);
        edge.resize(MAX_LOGO_BYTES, 0);
        assert_eq!(logo_problem(&uri(LOGO_PNG_PREFIX, &edge)), None);
        assert_eq!(
            logo_problem("data:image/jpeg;base64,AAAA"),
            Some("the logo must be a PNG or SVG")
        );
        assert_eq!(logo_problem("hello"), Some("the logo must be a PNG or SVG"));
        assert_eq!(
            logo_problem("data:image/png;base64,!!!"),
            Some("the logo is not valid")
        );
        assert_eq!(
            logo_problem(&format!("{LOGO_PNG_PREFIX}{}", "A".repeat(300_000))),
            Some("the logo is over 200 KB")
        );
    }

    #[test]
    fn a_plain_svg_passes() {
        let ok = r##"<?xml version="1.0"?><!-- logo --><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><linearGradient id="g"/></defs><rect fill="url(#g)" width="10" height="10"/><use href="#r"/><a xlink:href="#r"/></svg>"##;
        assert_eq!(logo_problem(&svg(ok)), None);
        assert_eq!(logo_problem(&svg("\u{feff}\n  <svg/>")), None);
        assert_eq!(
            logo_problem(&svg(
                r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "x.dtd"><svg></svg>"#
            )),
            None
        );
    }

    #[test]
    fn an_svg_must_be_text_with_an_svg_root() {
        for bad in [
            "",
            "hello",
            "<html><svg/></html>",
            "<svgx/>",
            "<?xml ?><div/>",
        ] {
            assert_eq!(
                logo_problem(&svg(bad)),
                Some("the logo is not a valid SVG"),
                "{bad}"
            );
        }
        assert_eq!(
            logo_problem(&uri(LOGO_SVG_PREFIX, &[b'<', b's', b'v', b'g', 0xFF, 0xFE])),
            Some("the logo is not a valid SVG")
        );
        // An internal DTD subset can declare entities.
        assert_eq!(
            logo_problem(&svg(r#"<!DOCTYPE svg [<!ENTITY a "b">]><svg/>"#)),
            Some("the logo is not a valid SVG")
        );
    }

    #[test]
    fn an_svg_with_active_content_is_refused() {
        for bad in [
            "<svg><script>alert(1)</script></svg>",
            "<svg><SCRIPT/></svg>",
            "<svg><foreignObject><div/></foreignObject></svg>",
            r#"<svg onload="x()"/>"#,
            r#"<svg><rect onclick = "x()"/></svg>"#,
            r#"<svg><rect ONMOUSEOVER='x()'/></svg>"#,
            r#"<svg><a href="javascript:alert(1)"/></svg>"#,
            r#"<svg><iframe src="x"/></svg>"#,
            r#"<svg><style>@import "x.css";</style></svg>"#,
            r#"<svg><!ENTITY x "y"></svg>"#,
        ] {
            assert_eq!(
                logo_problem(&svg(bad)),
                Some("the logo has active content"),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_svg_may_not_point_outside_the_document() {
        for bad in [
            r#"<svg><image href="https://x/y.png"/></svg>"#,
            r#"<svg><image xlink:href="data:image/png;base64,AAAA"/></svg>"#,
            r#"<svg><use href="other.svg#a"/></svg>"#,
            r#"<svg><use HREF = "//x/y.svg#a"/></svg>"#,
            r#"<svg><rect fill="url(https://x/g)"/></svg>"#,
            r#"<svg><rect style="fill:url( 'http://x/g' )"/></svg>"#,
            r#"<svg><rect style="fill:url(data:image/png;base64,AAAA)"/></svg>"#,
        ] {
            assert_eq!(
                logo_problem(&svg(bad)),
                Some("the logo points outside itself"),
                "{bad}"
            );
        }
        assert_eq!(
            logo_problem(&svg(
                r##"<svg><rect fill="url('#g')" style="clip-path:url(#c)"/></svg>"##
            )),
            None
        );
    }

    #[test]
    fn text_that_merely_contains_on_is_not_a_handler() {
        assert_eq!(
            logo_problem(&svg("<svg><text>on and on = 1, region=2</text></svg>")),
            None
        );
    }

    fn view() -> AttractView {
        AttractView {
            branch_name: "Level Up".into(),
            device_name: "PC 1".into(),
            zone: "Main hall".into(),
            state: AttractState::Ready,
            rate_minor: Some(12_000),
            group_name: Some("PC".into()),
            rule: None,
            unpaid_minor: 0,
            locale: SnapshotLocale {
                currency: "INR".into(),
                exponent: 2,
                symbol: "₹".into(),
                symbol_position: crate::SymbolPosition::Before,
                number_format: crate::NumberFormat::Indian,
                tz: "Asia/Kolkata".into(),
                day_start_min: 360,
                fy_start_month: 4,
                pending: None,
            },
            media: vec![AttractMedia {
                video: "ab".repeat(32),
                cover: None,
            }],
        }
    }

    #[test]
    fn the_view_is_snake_case_json_and_omits_what_is_absent() {
        let v = view();
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["state"], "ready");
        assert_eq!(
            (json["device_name"].as_str(), json["zone"].as_str()),
            (Some("PC 1"), Some("Main hall"))
        );
        // A view cached before the footer fields existed still reads (empty label and zone).
        let mut old = json.clone();
        old.as_object_mut().unwrap().remove("device_name");
        old.as_object_mut().unwrap().remove("zone");
        let old: AttractView = serde_json::from_value(old).unwrap();
        assert_eq!((old.device_name.as_str(), old.zone.as_str()), ("", ""));
        assert_eq!(json["rate_minor"], 12_000);
        assert!(json.get("rule").is_none());
        assert!(json["media"][0].get("cover").is_none());
        let counter = AttractView {
            state: AttractState::AskCounter,
            rate_minor: None,
            group_name: None,
            ..v
        };
        let json = serde_json::to_value(&counter).unwrap();
        assert_eq!(json["state"], "ask_counter");
        assert!(json.get("rate_minor").is_none() && json.get("group_name").is_none());
        let back: AttractView = serde_json::from_value(json).unwrap();
        assert_eq!(back, counter);
    }

    #[test]
    fn the_attract_event_is_a_tagged_frame_that_round_trips() {
        let ev = crate::Frame::new(
            "e",
            crate::FrameBody::Event(crate::EventFrame {
                event: crate::HubEvent::Attract(Box::new(view())),
            }),
        );
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["event"]["kind"], "attract");
        assert_eq!(json["event"]["branch_name"], "Level Up");
        assert_eq!(crate::parse_frame(&ev.to_text()).unwrap(), ev);
    }

    #[test]
    fn a_view_with_a_bad_hash_or_too_much_media_is_not_sane() {
        assert!(view().is_sane());
        let mut v = view();
        v.media[0].video = "nope".into();
        assert!(!v.is_sane());
        let mut v = view();
        v.media = vec![v.media[0].clone(); MAX_ATTRACT_MEDIA + 1];
        assert!(!v.is_sane());
        let mut v = view();
        v.state = AttractState::AskCounter;
        assert!(!v.is_sane(), "ask_counter carries no rate");
        v.rate_minor = None;
        assert!(v.is_sane());
    }
}

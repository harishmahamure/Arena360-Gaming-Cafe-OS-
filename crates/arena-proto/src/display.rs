//! The per-device display settings (Story 1.21): high contrast is set by a manager for each
//! device, never by the player and never from the OS.
//!
//! - `display.hc.hub` is the Hub UI on this branch's Hub.
//! - `display.hc.<device_id>` is one key per paired device (the kiosk reads only its own).
//!
//! Values are `"true"` or `"false"`.

use crate::admin::is_ulid_text;

/// The prefix every high-contrast key starts with.
pub const HC_PREFIX: &str = "display.hc.";
/// The Hub UI's own high-contrast key.
pub const KEY_HC_HUB: &str = "display.hc.hub";
/// The value a new high-contrast row starts with.
pub const HC_DEFAULT: &str = "false";

/// The high-contrast key of the device `device_id`.
pub fn hc_key(device_id: &str) -> String {
    format!("{HC_PREFIX}{device_id}")
}

/// Who a high-contrast key is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcTarget<'a> {
    /// `display.hc.hub`.
    Hub,
    /// `display.hc.<device_id>` (a ULID).
    Device(&'a str),
}

/// The target of a high-contrast key, or `None` when `key` is not one: the suffix must be
/// `hub` or a device id (an upper-case ULID).
pub fn parse_hc_key(key: &str) -> Option<HcTarget<'_>> {
    let rest = key.strip_prefix(HC_PREFIX)?;
    if rest == "hub" {
        Some(HcTarget::Hub)
    } else if is_ulid_text(rest) {
        Some(HcTarget::Device(rest))
    } else {
        None
    }
}

/// The boolean a stored high-contrast value means: only `"true"` is on, so a missing or
/// unreadable value is off.
pub fn parse_hc_value(value: Option<&str>) -> bool {
    value == Some("true")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

    #[test]
    fn keys_parse_to_their_target() {
        assert_eq!(parse_hc_key(KEY_HC_HUB), Some(HcTarget::Hub));
        assert_eq!(parse_hc_key(&hc_key(ID)), Some(HcTarget::Device(ID)));
        for bad in [
            "display.hc.",
            "display.hc.Hub",
            "display.hc.D1",
            "display.hc.01jabcdefghjkmnpqrstvwxyz0",
            "display.hc.01JABCDEFGHJKMNPQRSTVWXYZ0x",
            "display.hcx.hub",
            "kiosk.process_mode",
        ] {
            assert_eq!(parse_hc_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_true_is_on() {
        assert!(parse_hc_value(Some("true")));
        for off in [None, Some("false"), Some("TRUE"), Some("1"), Some("")] {
            assert!(!parse_hc_value(off), "{off:?}");
        }
    }
}

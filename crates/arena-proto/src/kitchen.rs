use crate::{IntentPayload, PRINTER_PAPER_WIDTHS_MM};

pub const KEY_LICENCE_FLAGS: &str = "licence.flags";
pub const KEY_DEVICE_CAP: &str = "licence.device_cap";
pub const KEY_KITCHEN_KOT_MODE: &str = "kitchen.kot_mode";
pub const KEY_KITCHEN_KOT_PRINTER: &str = "kitchen.kot_printer";
pub const KEY_KITCHEN_KOT_PAPER_MM: &str = "kitchen.kot_paper_mm";

pub const FLAG_KITCHEN: &str = "kitchen";
pub const KITCHEN_KOT_MODES: &[&str] = &["none", "receipt", "another"];
pub const DEFAULT_LICENCE_FLAGS: &str = "";
pub const DEFAULT_KITCHEN_KOT_MODE: &str = "receipt";
pub const DEFAULT_KITCHEN_KOT_PRINTER: &str = "";
pub const DEFAULT_KITCHEN_KOT_PAPER_MM: &str = PRINTER_PAPER_WIDTHS_MM[1];

pub const KITCHEN_GATED_KEYS: [&str; 3] = [
    KEY_KITCHEN_KOT_MODE,
    KEY_KITCHEN_KOT_PRINTER,
    KEY_KITCHEN_KOT_PAPER_MM,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierGate {
    New,
    ExistingWork,
}

pub fn kitchen_gate(p: &IntentPayload) -> Option<TierGate> {
    match p {
        IntentPayload::SettingsSet(s) if KITCHEN_GATED_KEYS.contains(&s.key.as_str()) => {
            Some(TierGate::New)
        }
        _ => None,
    }
}

pub fn has_flag(flags: &str, flag: &str) -> bool {
    flags.split(',').any(|f| f.trim() == flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_a_comma_list() {
        assert!(has_flag("kitchen", FLAG_KITCHEN));
        assert!(has_flag("a, kitchen", FLAG_KITCHEN));
        assert!(!has_flag("", FLAG_KITCHEN));
        assert!(!has_flag("kitchens", FLAG_KITCHEN));
    }
}

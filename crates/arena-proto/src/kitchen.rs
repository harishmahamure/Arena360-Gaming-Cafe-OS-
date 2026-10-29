use crate::{IntentPayload, PRINTER_PAPER_WIDTHS_MM};

pub const KEY_LICENCE_FLAGS: &str = "licence.flags";
pub const KEY_DEVICE_CAP: &str = "licence.device_cap";
pub const KEY_USER_CAP: &str = "licence.user_cap";
pub const KEY_LICENCE_STATUS: &str = "licence.status";
pub const KEY_LICENCE_CLOCK: &str = "licence.clock";
/// The effective tier the Hub runs under (Story 4.15): its change record is the audit row of a gate change.
pub const KEY_LICENCE_TIER: &str = "licence.tier";
pub const KEY_KITCHEN_KOT_MODE: &str = "kitchen.kot_mode";
pub const KEY_KITCHEN_KOT_PRINTER: &str = "kitchen.kot_printer";
pub const KEY_KITCHEN_KOT_PAPER_MM: &str = "kitchen.kot_paper_mm";

pub const FLAG_KITCHEN: &str = "kitchen";
pub const KITCHEN_KOT_MODES: &[&str] = &["none", "receipt", "another"];
pub const DEFAULT_LICENCE_FLAGS: &str = "";
pub const DEFAULT_KITCHEN_KOT_MODE: &str = "receipt";
pub const DEFAULT_KITCHEN_KOT_PRINTER: &str = "";
pub const DEFAULT_KITCHEN_KOT_PAPER_MM: &str = PRINTER_PAPER_WIDTHS_MM[1];

/// Story 7.2: the most preset kitchen notes a product holds.
pub const MAX_KITCHEN_PRESETS: usize = 8;
/// The longest preset kitchen note, in characters.
pub const MAX_PRESET_NOTE_CHARS: usize = 40;
/// The longest free-text kitchen note on an order line, in characters.
pub const MAX_LINE_NOTE_CHARS: usize = 60;
/// The most notes one order line carries: every preset plus one free-text note.
pub const MAX_LINE_NOTES: usize = MAX_KITCHEN_PRESETS + 1;

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
        // Story 7.2: only the kitchen-notes part is gated; the same order without notes, or a
        // product edit that leaves the list alone or empties it, is accepted on Free.
        IntentPayload::ProductUpsert(u)
            if u.kitchen_notes.as_ref().is_some_and(|n| !n.is_empty()) =>
        {
            Some(TierGate::New)
        }
        IntentPayload::PosSale(s) if s.lines.iter().any(|l| !l.kitchen_notes.is_empty()) => {
            Some(TierGate::New)
        }
        IntentPayload::OrderPlace(o) if o.lines.iter().any(|l| !l.kitchen_notes.is_empty()) => {
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

    fn line(notes: &[&str]) -> crate::PosSaleLine {
        crate::PosSaleLine {
            product_id: "P".into(),
            qty: 1,
            kitchen_notes: notes.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    #[test]
    fn only_the_kitchen_notes_part_of_an_order_or_product_is_gated() {
        let place = |notes: &[&str]| {
            IntentPayload::OrderPlace(crate::OrderPlace {
                session_id: "S".into(),
                lines: vec![line(&[]), line(notes)],
                stock_override: None,
                counter: None,
            })
        };
        assert_eq!(kitchen_gate(&place(&["No ice"])), Some(TierGate::New));
        assert_eq!(kitchen_gate(&place(&[])), None);
        let product = |notes: Option<Vec<String>>| {
            IntentPayload::ProductUpsert(crate::ProductUpsert {
                id: None,
                name: "Tea".into(),
                category: String::new(),
                price_minor: 1,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: true,
                image: String::new(),
                barcode: String::new(),
                kitchen: true,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
                kitchen_notes: notes,
            })
        };
        assert_eq!(
            kitchen_gate(&product(Some(vec!["No ice".into()]))),
            Some(TierGate::New)
        );
        assert_eq!(kitchen_gate(&product(Some(Vec::new()))), None);
        assert_eq!(kitchen_gate(&product(None)), None);
    }

    #[test]
    fn flags_are_a_comma_list() {
        assert!(has_flag("kitchen", FLAG_KITCHEN));
        assert!(has_flag("a, kitchen", FLAG_KITCHEN));
        assert!(!has_flag("", FLAG_KITCHEN));
        assert!(!has_flag("kitchens", FLAG_KITCHEN));
    }
}

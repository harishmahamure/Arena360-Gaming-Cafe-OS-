use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

/// Story 7.3: a kitchen ticket's stages, in the order it moves through them.
pub const STAGE_NEW: &str = "new";
pub const STAGE_PREPARING: &str = "preparing";
pub const STAGE_READY: &str = "ready";
pub const STAGE_SERVED: &str = "served";
/// The stage of a ticket whose last line was cancelled; it never moves again.
pub const STAGE_CANCELLED: &str = "cancelled";
/// The stages a ticket moves through, in order.
pub const KITCHEN_STAGES: [&str; 4] = [STAGE_NEW, STAGE_PREPARING, STAGE_READY, STAGE_SERVED];
/// How long a stage change stays undoable, in milliseconds (the Story 3.14 window).
pub const KITCHEN_UNDO_WINDOW_MS: i64 = 8_000;

/// The stage after `stage`, or `None` for Served and Cancelled.
pub fn next_stage(stage: &str) -> Option<&'static str> {
    let i = KITCHEN_STAGES.iter().position(|s| *s == stage)?;
    KITCHEN_STAGES.get(i + 1).copied()
}

/// The stage before `stage`, or `None` for New and Cancelled.
pub fn previous_stage(stage: &str) -> Option<&'static str> {
    let i = KITCHEN_STAGES.iter().position(|s| *s == stage)?;
    i.checked_sub(1).map(|p| KITCHEN_STAGES[p])
}

/// The payload of `kitchen.advance` (Story 7.3): a ticket moves to `to`, which must be exactly the
/// next stage. The intent's `base_rev` is the ticket's `rev` the caller saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenAdvance {
    pub ticket_id: String,
    pub to: String,
}

/// The payload of `kitchen.advance.reverted` (Story 7.3): the ticket returns to its previous
/// stage. The intent's `base_rev` is the `rev` the advance left, so only the latest advance
/// can be taken back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenAdvanceReverted {
    pub ticket_id: String,
}

/// One line of a kitchen ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenTicketLineView {
    /// The order line this ticket line stands for.
    pub line_id: String,
    pub label: String,
    #[ts(type = "number")]
    pub qty: i64,
    pub notes: Vec<String>,
}

/// A kitchen ticket as the Hub shows it (Story 7.3). It names the seat and the ticket number,
/// never a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenTicketView {
    pub id: String,
    pub order_id: String,
    #[ts(type = "number")]
    pub number: i64,
    /// The seat label: "PC 07", "TV 3" or "Counter".
    pub seat: String,
    pub stage: String,
    #[ts(type = "number")]
    pub placed_at: i64,
    #[ts(type = "number")]
    pub rev: i64,
    pub lines: Vec<KitchenTicketLineView>,
}

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
        // Story 7.3: a ticket opened before a lapse still finishes; the apply step refuses a
        // ticket that does not exist on a branch without the flag.
        IntentPayload::KitchenAdvance(_) | IntentPayload::KitchenAdvanceReverted(_) => {
            Some(TierGate::ExistingWork)
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

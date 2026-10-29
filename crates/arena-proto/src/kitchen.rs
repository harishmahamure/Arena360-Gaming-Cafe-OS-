use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{IntentPayload, PRINTER_PAPER_WIDTHS_MM, PrinterErrorKind};

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

/// The payload of `kot.reprint` (Story 7.5): print the ticket's KOT again, marked "REPRINT". The
/// audit log records who asked. A cancelled ticket, or a branch whose KOT printer is None, is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KotReprint {
    pub ticket_id: String,
}

/// The payload of `kot.printed` (system): the print task sent the KOT row to the printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KotPrinted {
    pub outbox_id: String,
}

/// The payload of `kot.print_failed` (system): the printer did not take the KOT. The ticket and
/// order are unaffected; a "Printer offline" alert is raised and the ticket row offers Reprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KotPrintFailed {
    pub outbox_id: String,
    pub error_kind: PrinterErrorKind,
    pub error: String,
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
    /// What the latest KOT of this ticket came to: `queued`, `done` or `failed` (Story 7.5). Absent
    /// when no KOT was ever queued (the KOT printer is None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kot: Option<String>,
    pub lines: Vec<KitchenTicketLineView>,
}

/// Story 7.7: how long a Served ticket stays on the board, in milliseconds.
pub const BOARD_SERVED_KEEP_MS: i64 = 30 * 60_000;
/// How long a Cancelled ticket stays on the board, greyed and untappable, in milliseconds.
pub const BOARD_CANCELLED_KEEP_MS: i64 = 60_000;

/// One ticket on the kitchen board (Story 7.7). It names the seat and the ticket number, never a
/// player. `stage_at` is when the ticket reached its stage (the placement time for a New one): a
/// Served ticket leaves the board 30 minutes after it, a Cancelled one 60 seconds after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenBoardTicket {
    pub id: String,
    #[ts(type = "number")]
    pub number: i64,
    pub seat: String,
    pub stage: String,
    #[ts(type = "number")]
    pub placed_at: i64,
    #[ts(type = "number")]
    pub stage_at: i64,
    #[ts(type = "number")]
    pub rev: i64,
    pub lines: Vec<KitchenTicketLineView>,
}

/// What a kitchen display shows (Story 7.7): the open tickets, the Served ones of the last 30
/// minutes and the Cancelled ones of the last 60 seconds, newest first, with the Hub's clock so
/// elapsed times never use the tablet's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenBoard {
    pub branch_name: String,
    /// The Hub's clock when the board was read, in milliseconds since the epoch.
    #[ts(type = "number")]
    pub now_ms: i64,
    pub tickets: Vec<KitchenBoardTicket>,
}

/// Story 7.6: the most kitchen displays a branch holds that are not revoked.
pub const KDS_MAX_DISPLAYS: i64 = 4;
/// The plain-HTTP path that serves the Kitchen display app.
pub const KDS_PAGE_PATH: &str = "/kitchen";
/// `POST` here with a pairing code to pair a display.
pub const KDS_PAIR_PATH: &str = "/kitchen/pair";
/// `GET` here tells the page whether the branch has the kitchen flag.
pub const KDS_STATUS_PATH: &str = "/kitchen/status";
/// The name a display takes when the tablet sends none.
pub const KDS_DEFAULT_NAME: &str = "Kitchen";
/// A display is refreshed in the Hub's list at most this often, in milliseconds.
pub const KDS_SEEN_INTERVAL_MS: i64 = 60_000;
/// The wrong-code message the page shows and the field clears with.
pub const KDS_CODE_WRONG: &str = "That code didn't work. Get a new one from the counter.";
/// The refusal of a fifth display.
pub const KDS_LIMIT_MESSAGE: &str =
    "This branch already has 4 kitchen displays — remove one at the counter.";

/// The payload of `kds.pair` (system, Story 7.6): a kitchen display row with the Argon2id hash of
/// its token secret. The writer mints the id; `created_by` is the Manager or Owner who issued the
/// pairing code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsPair {
    pub name: String,
    pub token_hash: String,
    pub created_by: String,
}

impl std::fmt::Debug for KdsPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdsPair")
            .field("name", &self.name)
            .field("created_by", &self.created_by)
            .finish_non_exhaustive()
    }
}

/// The payload of `kds.revoke` (`device.manage`): the display's token stops working and its live
/// socket closes `TOKEN_REVOKED`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsRevoke {
    pub display_id: String,
}

/// The payload of `kds.seen` (system): a display connected, or its last-seen is stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsSeen {
    pub display_id: String,
}

/// The body of `POST /kitchen/pair`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsPairRequest {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
}

impl std::fmt::Debug for KdsPairRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdsPairRequest")
            .field("code", &"<redacted>")
            .field("name", &self.name)
            .finish()
    }
}

/// The answer of `POST /kitchen/pair`: the `kds` token, returned this once.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsPairResponse {
    pub display_id: String,
    pub token: String,
    pub name: String,
}

impl std::fmt::Debug for KdsPairResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdsPairResponse")
            .field("display_id", &self.display_id)
            .field("token", &"<redacted>")
            .field("name", &self.name)
            .finish()
    }
}

/// The answer of `GET /kitchen/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsStatus {
    /// Whether the branch has the `kitchen` flag (Basic and above).
    pub available: bool,
    pub branch_name: String,
}

/// The answer of `POST /ui/kds-pairing-code`: the single-use code and where the tablet goes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KdsPairingCode {
    pub code: String,
    #[ts(type = "number")]
    pub expires_at: i64,
    /// `http://<hub-lan-ip>:7401/kitchen`, when the Hub knows its LAN address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub address: Option<String>,
}

impl std::fmt::Debug for KdsPairingCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdsPairingCode")
            .field("code", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("address", &self.address)
            .finish()
    }
}

/// A paired kitchen display as the Hub lists it. The token hash never leaves the Hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KitchenDisplayView {
    pub id: String,
    pub name: String,
    /// Whether a socket of this display is open now (filled by the LAN layer).
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_seen: Option<i64>,
    pub high_contrast: bool,
    #[ts(type = "number")]
    pub rev: i64,
}

/// Whether `name` is a display name the Hub accepts: 1 to 40 printable characters.
pub fn is_display_name(name: &str) -> bool {
    let n = name.chars().count();
    name.trim() == name && (1..=40).contains(&n) && !name.chars().any(char::is_control)
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
        IntentPayload::KitchenAdvance(_)
        | IntentPayload::KitchenAdvanceReverted(_)
        | IntentPayload::KotReprint(_)
        | IntentPayload::KdsRevoke(_)
        | IntentPayload::KdsSeen(_) => Some(TierGate::ExistingWork),
        // Story 7.6: pairing a display is new kitchen work.
        IntentPayload::KdsPair(_) => Some(TierGate::New),
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
    fn pairing_a_display_is_new_work_and_revoking_it_is_existing_work() {
        let pair = IntentPayload::KdsPair(KdsPair {
            name: "Pass".into(),
            token_hash: "h".into(),
            created_by: "S".into(),
        });
        assert_eq!(kitchen_gate(&pair), Some(TierGate::New));
        let revoke = IntentPayload::KdsRevoke(KdsRevoke {
            display_id: "D".into(),
        });
        assert_eq!(kitchen_gate(&revoke), Some(TierGate::ExistingWork));
        let seen = IntentPayload::KdsSeen(KdsSeen {
            display_id: "D".into(),
        });
        assert_eq!(kitchen_gate(&seen), Some(TierGate::ExistingWork));
    }

    #[test]
    fn display_names_are_one_to_forty_printable_characters() {
        for ok in ["Kitchen", "Pass 2", "Grill — left"] {
            assert!(is_display_name(ok), "{ok}");
        }
        for bad in [
            "",
            " Pass",
            "Pass ",
            "a
b",
            &"x".repeat(41),
        ] {
            assert!(!is_display_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn flags_are_a_comma_list() {
        assert!(has_flag("kitchen", FLAG_KITCHEN));
        assert!(has_flag("a, kitchen", FLAG_KITCHEN));
        assert!(!has_flag("", FLAG_KITCHEN));
        assert!(!has_flag("kitchens", FLAG_KITCHEN));
    }
}

//! Receipt and notification settings (Story 5.15): the keys, their defaults and the text rules.
//! Messages are sent by Arena 360, so no setting here holds a provider key.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{IntentPayload, PrinterErrorKind, TierGate};

pub const KEY_RECEIPT_FOOTER: &str = "receipt.footer";
pub const KEY_RECEIPT_ADDRESS: &str = "receipt.address";
pub const KEY_RECEIPT_PHONE: &str = "receipt.phone";
/// What the settle bar preselects: `print` (when a printer is available) or `none`.
pub const KEY_RECEIPT_DEFAULT: &str = "receipt.default";
/// Whether a settle with a cash row kicks the drawer attached to the receipt printer.
pub const KEY_RECEIPT_DRAWER: &str = "receipt.drawer";
pub const KEY_NOTIFY_WHATSAPP_RECEIPT: &str = "notify.whatsapp_receipt";
pub const KEY_NOTIFY_CREDIT_REMINDER: &str = "notify.credit_reminder";
pub const KEY_NOTIFY_LOW_BALANCE: &str = "notify.low_balance";
pub const KEY_NOTIFY_SESSION_ENDING: &str = "notify.session_ending";

pub const RECEIPT_DEFAULT_CHOICES: &[&str] = &["print", "none"];
pub const DEFAULT_RECEIPT_ADDRESS: &str = "";
pub const DEFAULT_RECEIPT_PHONE: &str = "";
pub const DEFAULT_RECEIPT_DEFAULT: &str = "print";
/// No drawer is assumed: the branch turns the kick on once one is cabled to the printer.
pub const DEFAULT_RECEIPT_DRAWER: &str = "false";
/// A service message is off until the branch turns it on.
pub const DEFAULT_NOTIFY: &str = "false";

/// The service messages a branch can switch on; each is paid-tier.
pub const NOTIFY_KEYS: [&str; 4] = [
    KEY_NOTIFY_WHATSAPP_RECEIPT,
    KEY_NOTIFY_CREDIT_REMINDER,
    KEY_NOTIFY_LOW_BALANCE,
    KEY_NOTIFY_SESSION_ENDING,
];

pub const RECEIPT_FOOTER_MAX_LINES: usize = 3;
pub const RECEIPT_ADDRESS_MAX_LINES: usize = 3;
pub const RECEIPT_PHONE_MAX_LINES: usize = 1;
/// A line may be longer than the paper (the printer wraps it); this only bounds the stored text.
pub const RECEIPT_LINE_MAX_CHARS: usize = 96;

/// Whether `value` is at most `max_lines` lines (separated by `\n`) of at most
/// [`RECEIPT_LINE_MAX_CHARS`] printable characters each. Empty is valid.
pub fn valid_receipt_lines(value: &str, max_lines: usize) -> bool {
    value.is_empty()
        || (value.split('\n').count() <= max_lines
            && value.split('\n').all(|line| {
                line.chars().count() <= RECEIPT_LINE_MAX_CHARS
                    && !line.chars().any(char::is_control)
            }))
}

/// The receipt choice on the settle bar (Story 5.17). `print` issues the invoice number inside the
/// settle transaction, so the bill is never undoable, and queues the receipt on the side-effect
/// outbox; `none` (and an absent choice) leaves the number to the Undo-window task. `whatsapp`
/// (Story 5.19) queues a `notify_outbox` row for the player's receipt and issues no number: the
/// number comes when the Undo window closes, and the Epic 8 drain sends after that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ReceiptChoice {
    Print,
    None,
    Whatsapp,
}

impl ReceiptChoice {
    /// The choices a branch can preselect (`receipt.default`): `whatsapp` is per player, so it is
    /// never a default.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "print" => Some(Self::Print),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// The purpose of a `notify_outbox` row sent to a player about their own transaction.
pub const NOTIFY_PURPOSE_TRANSACTIONAL: &str = "transactional";
/// The kind of a `notify_outbox` row that carries a settled order's receipt.
pub const NOTIFY_KIND_RECEIPT: &str = "receipt";
/// A `notify_outbox` row waiting for the Epic 8 drain.
pub const NOTIFY_STATE_QUEUED: &str = "queued";
/// A `notify_outbox` row that will never be sent (its settle was undone).
pub const NOTIFY_STATE_SKIPPED: &str = "skipped";

/// Why a WhatsApp receipt cannot be offered or accepted for a settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhatsappRefusal {
    /// The branch is not on a paid tier (`FEATURE_NOT_ENTITLED`).
    NotEntitled,
    /// The sale has no profiled player, or the player's current transactional consent is not
    /// granted (`CONSENT_REQUIRED`).
    ConsentRequired,
    /// The branch has not switched `notify.whatsapp_receipt` on (`VALIDATION_FAILED`).
    SwitchedOff,
}

/// Whether a settle may choose WhatsApp: a paid tier first, then a profiled player whose latest
/// transactional consent is granted, then the branch's own switch. `consent` is `None` for a guest
/// or a player with no consent row.
pub fn whatsapp_refusal(
    tier: Option<&str>,
    switch_on: bool,
    has_player: bool,
    consent: Option<bool>,
) -> Option<WhatsappRefusal> {
    if !tier.is_some_and(tier_is_paid) {
        Some(WhatsappRefusal::NotEntitled)
    } else if !has_player || consent != Some(true) {
        Some(WhatsappRefusal::ConsentRequired)
    } else if !switch_on {
        Some(WhatsappRefusal::SwitchedOff)
    } else {
        None
    }
}

/// The longest print error a result intent keeps.
pub const RECEIPT_ERROR_MAX_CHARS: usize = 200;

/// The payload of `receipt.reprint`: print the receipt of a settled, numbered order again, marked
/// "Reprint". The audit log records who asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReceiptReprint {
    pub order_id: String,
}

/// The payload of `receipt.printed` (system): the print task sent the outbox row to the printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReceiptPrinted {
    pub outbox_id: String,
}

/// The payload of `receipt.print_failed` (system): the printer did not take the job. The sale
/// stays settled; a "Printer offline" alert is raised and the order shows Reprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReceiptPrintFailed {
    pub outbox_id: String,
    pub error_kind: PrinterErrorKind,
    pub error: String,
}

/// The payload of `drawer.kicked` (system): the print task sent the drawer pulse. A `drawer_open`
/// cash event is written against the shift beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DrawerKicked {
    pub outbox_id: String,
}

/// The payload of `drawer.kick_failed` (system): the printer did not take the pulse. The sale
/// stays settled and a "Printer offline" alert is raised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DrawerKickFailed {
    pub outbox_id: String,
    pub error_kind: PrinterErrorKind,
    pub error: String,
}

/// The tier-locked keys: a write to one on a Free branch is `TIER_LOCKED`.
pub fn notify_gate(p: &IntentPayload) -> Option<TierGate> {
    match p {
        IntentPayload::SettingsSet(s) if NOTIFY_KEYS.contains(&s.key.as_str()) => {
            Some(TierGate::New)
        }
        _ => None,
    }
}

/// Whether the effective tier (`licence.tier`) is a paid one.
pub fn tier_is_paid(tier: &str) -> bool {
    matches!(tier, "basic" | "premium")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SettingsSet;

    #[test]
    fn receipt_lines_are_bounded_in_count_and_length() {
        assert!(valid_receipt_lines("", 3));
        assert!(valid_receipt_lines("a\nb\nc", 3));
        assert!(!valid_receipt_lines("a\nb\nc\nd", 3));
        assert!(!valid_receipt_lines("a\nb", 1));
        assert!(valid_receipt_lines(&"x".repeat(96), 1));
        assert!(!valid_receipt_lines(&"x".repeat(97), 1));
        assert!(!valid_receipt_lines("a\tb", 1));
        assert!(!valid_receipt_lines("a\r\nb", 2));
    }

    #[test]
    fn whatsapp_needs_a_paid_tier_a_profile_with_consent_and_the_branch_switch() {
        let ok = |tier, on, player, consent| whatsapp_refusal(tier, on, player, consent);
        assert_eq!(ok(Some("basic"), true, true, Some(true)), None);
        assert_eq!(ok(Some("premium"), true, true, Some(true)), None);
        assert_eq!(
            ok(Some("free"), true, true, Some(true)),
            Some(WhatsappRefusal::NotEntitled)
        );
        assert_eq!(
            ok(None, true, true, Some(true)),
            Some(WhatsappRefusal::NotEntitled)
        );
        assert_eq!(
            ok(Some("basic"), true, false, None),
            Some(WhatsappRefusal::ConsentRequired)
        );
        assert_eq!(
            ok(Some("basic"), true, true, Some(false)),
            Some(WhatsappRefusal::ConsentRequired)
        );
        assert_eq!(
            ok(Some("basic"), true, true, None),
            Some(WhatsappRefusal::ConsentRequired)
        );
        assert_eq!(
            ok(Some("basic"), false, true, Some(true)),
            Some(WhatsappRefusal::SwitchedOff)
        );
        assert_eq!(
            ok(Some("free"), false, false, None),
            Some(WhatsappRefusal::NotEntitled),
            "a Free branch is told about the tier first"
        );
    }

    #[test]
    fn only_the_notification_keys_are_gated() {
        let set = |key: &str| {
            IntentPayload::SettingsSet(SettingsSet {
                key: key.into(),
                value: "true".into(),
            })
        };
        for k in NOTIFY_KEYS {
            assert_eq!(notify_gate(&set(k)), Some(TierGate::New), "{k}");
        }
        assert_eq!(notify_gate(&set(KEY_RECEIPT_FOOTER)), None);
        assert!(tier_is_paid("basic") && tier_is_paid("premium"));
        assert!(!tier_is_paid("free") && !tier_is_paid(""));
    }
}

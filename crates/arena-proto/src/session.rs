use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SessionMode {
    Postpaid,
    Pack,
    Wallet,
    Pass,
    Block,
}

impl SessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postpaid => "postpaid",
            Self::Pack => "pack",
            Self::Wallet => "wallet",
            Self::Pass => "pass",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionStart {
    pub device_id: String,
    pub mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_id: Option<String>,
    /// A running postpaid session whose group bill this new postpaid session joins (a new group
    /// is made when it has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_with: Option<String>,
    /// A block with a player: pay the block price from the player's wallet in this same intent
    /// (refused `INSUFFICIENT_BALANCE` when the available wallet is short). The Hub sets it for a
    /// block a player starts at the kiosk; a staff-started block leaves it off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, type = "boolean")]
    pub pay_from_wallet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionEnd {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionExpire {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionRateFlip {
    pub session_id: String,
}

/// Minutes before a session's end at which the Hub warns the kiosk, longest first.
pub const SESSION_WARNING_MARKS_MIN: [u8; 3] = [10, 5, 1];

pub const PAUSE_NOTE_MAX: usize = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PauseReason {
    PlayerSteppedAway,
    HardwareIssue,
    PowerCut,
    Other,
}

impl PauseReason {
    pub const ALL: [PauseReason; 4] = [
        Self::PlayerSteppedAway,
        Self::HardwareIssue,
        Self::PowerCut,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlayerSteppedAway => "player_stepped_away",
            Self::HardwareIssue => "hardware_issue",
            Self::PowerCut => "power_cut",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionPause {
    pub session_id: String,
    pub reason: PauseReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

impl SessionPause {
    pub fn checked_note(&self) -> Result<Option<String>, AppError> {
        let invalid = |m: &str| AppError::with_field(ErrorCode::ValidationFailed, "note", m);
        match (self.reason, self.note.as_deref().map(str::trim)) {
            (PauseReason::Other, Some(n))
                if (1..=PAUSE_NOTE_MAX).contains(&n.chars().count())
                    && !n.chars().any(char::is_control) =>
            {
                Ok(Some(n.to_owned()))
            }
            (PauseReason::Other, _) => Err(invalid("Say why in 1 to 140 characters.")),
            (_, None) => Ok(None),
            _ => Err(invalid("Only Other takes a note.")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionResume {
    pub session_id: String,
}

/// Minutes a Hub extend may add, shortest first.
pub const EXTEND_OPTIONS_MIN: [i64; 3] = [30, 60, 120];

/// What pays for an extend: the wallet or credit (at the group's live rate) or minutes off a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ExtendSource {
    Wallet,
    Pack,
    Credit,
}

impl ExtendSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wallet => "wallet",
            Self::Pack => "pack",
            Self::Credit => "credit",
        }
    }
}

/// Adds `minutes` to a prepaid session, paid from `source`; a pack source names the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionExtend {
    pub session_id: String,
    #[ts(type = "number")]
    pub minutes: i64,
    pub source: ExtendSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credit_override: Option<bool>,
}

/// Puts `session_id` and each of `member_ids` into one group bill: the group of `session_id`, or
/// a new one paid by its player. Only running postpaid sessions share a bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionGroup {
    pub session_id: String,
    pub member_ids: Vec<String>,
}

/// Names who pays a group bill: a profile (a member's or a looked-up player) or one guest
/// member by guest number. Exactly one of the two is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GroupPayer {
    pub group_bill_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub guest_no: Option<i64>,
}

/// Moves a running or paused session to another free PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionTransfer {
    pub session_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionEndReverted {
    pub session_id: String,
}

/// Whether a console's controllers came back after a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ControllerResult {
    Returned,
    Missing,
}

impl ControllerResult {
    /// The stored and wire form, e.g. `missing`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Returned => "returned",
            Self::Missing => "missing",
        }
    }
}

/// Records the controller check of an ended console session. Marking a session twice is `STALE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ControllerCheck {
    pub session_id: String,
    pub result: ControllerResult,
}

/// Ends a session early. Needs a manager's PIN and one of the reasons of
/// [`IntentKind::SessionForceEnd`](crate::IntentKind::reasons); `note` is required with
/// `other`. The player is charged only the time used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionForceEnd {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionForceEndReverted {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionSettleReverted {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionAssignPlayer {
    pub session_id: String,
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub create: Option<crate::PlayerCreate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiSessionConvertRequest {
    pub session_id: String,
    #[ts(type = "number")]
    pub base_rev: i64,
    pub form: crate::PlayerForm,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SessionListState {
    Running,
    Unpaid,
    Settled,
}

impl SessionListState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Unpaid => "unpaid",
            Self::Settled => "settled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionListRow {
    pub id: String,
    pub device_id: String,
    pub device: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub username: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub guest_no: Option<i64>,
    pub mode: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub ended_at: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub bill_minor: Option<i64>,
    pub state: SessionListState,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub order_id: Option<String>,
    /// How a console session's controllers were marked: `returned` or `missing`.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub controller_check: Option<ControllerResult>,
    /// The invoice number of a settled bill; the row offers Reprint (`receipt.reprint`) with
    /// `receipt_order_id` while it has one (Story 5.17).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub invoice_no: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub receipt_order_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionListView {
    pub business_day: String,
    pub sessions: Vec<SessionListRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PaymentMethod {
    Cash,
    Upi,
    Card,
    Wallet,
    Credit,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Upi => "upi",
            Self::Card => "card",
            Self::Wallet => "wallet",
            Self::Credit => "credit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PaymentRow {
    pub method: PaymentMethod,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionSettle {
    pub order_id: String,
    pub payments: Vec<PaymentRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credit_override: Option<bool>,
    /// The receipt choice (Story 5.17); `print` issues the invoice number in this transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub receipt: Option<crate::ReceiptChoice>,
    /// The business the invoice is made out to (Story 5.20); a GST branch only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub business: Option<crate::BusinessBuyer>,
    /// A coupon code to apply to the bill (Story 5.21).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coupon: Option<String>,
    /// A discount staff give by hand, applied after the coupon (Story 5.22). Needs `discount.manual`
    /// on the actor or on the approving manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub discount: Option<crate::ManualDiscount>,
}

pub const UNDO_WINDOW_MS: i64 = 8_000;
pub const CARD_REFERENCE_MAX: usize = 64;
pub const WALLET_TOPUP_MAX_MINOR: i64 = 10_000_000;
pub const CREDIT_LIMIT_MAX_MINOR: i64 = 10_000_000;
pub const CREDIT_DAYS_MAX: usize = 10;
pub const CREDIT_DAY_MAX: u16 = 365;
pub const DEFAULT_REMINDER_DAYS: [u16; 3] = [3, 7, 14];
pub const CREDIT_OVERDUE_DAYS: i64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditAging {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub oldest_unpaid_day: Option<String>,
    #[ts(type = "number")]
    pub oldest_age_days: i64,
    #[ts(type = "number")]
    pub bucket_0_7_minor: i64,
    #[ts(type = "number")]
    pub bucket_8_14_minor: i64,
    #[ts(type = "number")]
    pub bucket_15_plus_minor: i64,
}

impl CreditAging {
    pub fn overdue(&self) -> bool {
        self.bucket_15_plus_minor > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditAutoPause {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditSetLimit {
    pub player_id: String,
    #[ts(type = "number")]
    pub limit_minor: i64,
    pub reminder_days: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditView {
    #[ts(type = "number")]
    pub limit_minor: i64,
    #[ts(type = "number")]
    pub outstanding_minor: i64,
    pub reminder_days: Vec<u16>,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditCollect {
    pub player_id: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub payments: Vec<PaymentRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditCollectReverted {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ClearedSpend {
    pub order_id: String,
    #[ts(type = "number")]
    pub at: i64,
    #[ts(type = "number")]
    pub spend_minor: i64,
    #[ts(type = "number")]
    pub cleared_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditCollection {
    pub player_id: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub cleared: Vec<ClearedSpend>,
    #[ts(type = "number")]
    pub outstanding_after_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WalletTopup {
    pub player_id: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    pub payments: Vec<PaymentRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WalletTopupReverted {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WalletChangedEvent {
    pub player_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WalletTxnView {
    pub id: String,
    #[ts(type = "number")]
    pub at: i64,
    #[ts(type = "number")]
    pub delta_minor: i64,
    pub reason: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub method: Option<String>,
    pub staff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct WalletView {
    pub player_id: String,
    #[ts(type = "number")]
    pub balance_minor: i64,
    pub txns: Vec<WalletTxnView>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub credit: Option<CreditView>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub aging: Option<CreditAging>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<crate::OwnedPlanView>>")]
    pub plans: Vec<crate::OwnedPlanView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettleOutcome {
    pub order_id: String,
    #[ts(type = "number")]
    pub settled_at: i64,
    #[ts(type = "number")]
    pub undo_until: i64,
    #[ts(type = "number")]
    pub order_rev: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    pub irreversible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub collection: Option<CreditCollection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EndOutcome {
    pub session_id: String,
    pub order_id: String,
    #[ts(type = "number")]
    pub ended_at: i64,
    #[ts(type = "number")]
    pub undo_until: i64,
    #[ts(type = "number")]
    pub session_rev: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    pub irreversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillSettledEvent {
    pub order_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub collection: Option<CreditCollection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillRevertedEvent {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillLine {
    pub device_id: String,
    #[ts(type = "number")]
    pub from_at: i64,
    #[ts(type = "number")]
    pub to_at: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub rate_minor_per_hour: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
    /// The pricing rule that set this line's rate, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rule_name: Option<String>,
}

/// A product line on a session bill: what was placed, how many, and what it cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BillItem {
    /// The order line (for Mark served and Cancel).
    #[serde(default)]
    pub line_id: String,
    pub label: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
    /// Whether the line was served; a placed kitchen line is not until staff mark it.
    #[serde(default = "served_by_default")]
    pub served: bool,
}

fn served_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotBill {
    pub session_id: String,
    pub device_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub order_id: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub order_rev: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub wallet_minor: Option<i64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub credit: Option<CreditView>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub session_rev: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guest: Option<bool>,
    /// The profiled player the bill is for; absent for a guest (Story 5.19: the settle bar asks
    /// the Hub whether WhatsApp is offered for them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub player_id: Option<String>,
    /// The group bill an open order settles for every member of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_bill_id: Option<String>,
    pub lines: Vec<BillLine>,
    /// The products placed on the bill, after the session lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<BillItem>>")]
    pub items: Vec<BillItem>,
    /// The order those items sit on when it is not a bill to settle yet (a running postpaid
    /// session's placed products): what `order.void` names (Story 5.23).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub items_order_id: Option<String>,
    #[ts(type = "number")]
    pub subtotal_minor: i64,
    #[ts(type = "number")]
    pub paid_minor: i64,
    #[ts(type = "number")]
    pub remaining_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotSession {
    pub id: String,
    pub device_id: String,
    pub state: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub player_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub username: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub guest_no: Option<i64>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub paused_at: Option<i64>,
    #[serde(default)]
    #[ts(type = "number")]
    pub paused_total_ms: i64,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub pause_reason: Option<PauseReason>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub pause_note: Option<String>,
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub ends_at: Option<i64>,
    /// When the grace after `ends_at` runs out (a timed session with a grace period).
    #[serde(default)]
    #[ts(optional, type = "number | null")]
    pub grace_ends_at: Option<i64>,
    /// How the session is paid for: `postpaid`, `pack`, `wallet`, `pass` or `block`.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub mode: Option<String>,
    /// The owned pack a pack or pass session plays on.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub pack_id: Option<String>,
    /// The group bill this session shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_bill_id: Option<String>,
}

/// A group bill: who pays, which running sessions share it and what the ended members owe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SnapshotGroupBill {
    pub id: String,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub payer_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub payer_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub payer_guest_no: Option<i64>,
    /// The sessions still running on the bill.
    pub member_ids: Vec<String>,
    /// What the members that already ended owe, line by line.
    pub ended_lines: Vec<BillLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionStartedEvent {
    pub session_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionScreen {
    pub session_id: String,
    pub device_name: String,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    #[ts(type = "number")]
    pub lock_after_ms: u64,
    pub time_up_mode: String,
    /// The grace at the end of `lock_after_ms`; the kiosk shows "Time's up" while it runs.
    #[serde(default)]
    #[ts(type = "number")]
    pub grace_ms: u64,
    /// How many items were placed on the session's bill; the kiosk shows "Ordered: n items".
    #[serde(default)]
    #[ts(type = "number")]
    pub ordered_items: u64,
    /// The profiled player's username; the shell's account chip greets them by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    /// A guest's number ("Guest <n>"), when there is no profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub guest_no: Option<i64>,
    /// Time left on a prepaid session (it counts down); `None` for postpaid (it counts up).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub remaining_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_round_trips_and_cash_omits_its_reference() {
        let s: SessionSettle = serde_json::from_value(serde_json::json!({
            "order_id": "O",
            "payments": [{"method": "cash", "amount_minor": 100},
                         {"method": "upi", "amount_minor": 50, "reference": "1234"}]
        }))
        .unwrap();
        assert_eq!(s.payments[0].reference, None);
        assert_eq!(s.payments[1].method.as_str(), "upi");
        assert!(
            serde_json::to_value(&s).unwrap()["payments"][0]
                .get("reference")
                .is_none()
        );
    }

    #[test]
    fn reverts_round_trip() {
        let e: SessionEndReverted =
            serde_json::from_value(serde_json::json!({"session_id": "S"})).unwrap();
        assert_eq!(e.session_id, "S");
        let s: SessionSettleReverted =
            serde_json::from_value(serde_json::json!({"order_id": "O"})).unwrap();
        assert_eq!(s.order_id, "O");
    }

    #[test]
    fn a_pause_note_belongs_to_other_alone() {
        let p = |reason, note: Option<&str>| SessionPause {
            session_id: "S".into(),
            reason,
            note: note.map(str::to_owned),
        };
        assert_eq!(
            p(PauseReason::Other, Some("  Fan noise ")).checked_note(),
            Ok(Some("Fan noise".to_owned()))
        );
        for bad in [
            None,
            Some(""),
            Some("   "),
            Some(
                "a
b",
            ),
        ] {
            assert!(p(PauseReason::Other, bad).checked_note().is_err());
        }
        assert!(
            p(PauseReason::Other, Some(&"x".repeat(141)))
                .checked_note()
                .is_err()
        );
        assert_eq!(p(PauseReason::PowerCut, None).checked_note(), Ok(None));
        assert!(p(PauseReason::PowerCut, Some("x")).checked_note().is_err());
        assert_eq!(
            PauseReason::parse("hardware_issue"),
            Some(PauseReason::HardwareIssue)
        );
    }

    #[test]
    fn start_round_trips_and_guest_omits_player() {
        let p: SessionStart =
            serde_json::from_value(serde_json::json!({"device_id": "D", "mode": "postpaid"}))
                .unwrap();
        assert_eq!(p.player_id, None);
        assert_eq!(p.mode.as_str(), "postpaid");
        assert!(serde_json::to_value(&p).unwrap().get("player_id").is_none());
    }
}

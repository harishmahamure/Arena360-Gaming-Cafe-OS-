//! The Hub's Reports screen (Story 5.27): Daily sales and Sessions over a range of business days.
//! Both are read-only views built from stored rows (`GET /ui/reports/daily-sales` and
//! `GET /ui/reports/sessions`, permission `report.view`). All amounts are minor units.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest range a report covers, in business days (a 90-day report plus slack).
pub const REPORT_MAX_DAYS: i64 = 93;

/// `DailySales.by_type[].key`: time billed to sessions.
pub const SALES_PLAY_TIME: &str = "play_time";
/// `DailySales.by_type[].key`: products and bundles.
pub const SALES_PRODUCTS: &str = "products";
/// `DailySales.by_type[].key`: time packs and passes sold.
pub const SALES_PLANS: &str = "plans";
/// `DailySales.by_type[].key`: coupons and manual discounts taken off (a negative row).
pub const SALES_DISCOUNTS: &str = "discounts";
/// `DailySales.by_type[].key`: tax added on top of tax-exclusive bills.
pub const SALES_TAX_ADDED: &str = "tax_added";

/// `DailySales.received[].key`: wallet top-ups.
pub const RECEIVED_TOPUP: &str = "topup";
/// `DailySales.received[].key`: credit collected.
pub const RECEIVED_CREDIT_COLLECT: &str = "credit_collect";

/// One labelled amount: a sales type, or a payment received that is not a sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReportAmount {
    pub key: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub count: i64,
}

/// Sales in one local hour of the day (0 to 23), summed over the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HourSales {
    pub hour: u8,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub count: i64,
}

/// Sales for a device group or a product category. `id` is the group id, or the category name;
/// `name` is what to show (`id` empty: sales with no group, shown as "Counter").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NamedSales {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub count: i64,
}

/// Daily sales for the business days `from` to `to` inclusive (`YYYY-MM-DD`).
///
/// `sales_minor` is the sum of the settled bills' stored totals (tax included); `by_type` adds up
/// to it (play time + products + plans + discounts + tax added). Undone settles and voided orders
/// are not sales. `refunds_minor` is what credit notes in the range paid back, so
/// `net_minor = sales_minor - refunds_minor`. Top-ups and credit collections are payments, not
/// sales, and sit in `received`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DailySales {
    pub from: String,
    pub to: String,
    #[ts(type = "number")]
    pub orders: i64,
    #[ts(type = "number")]
    pub sales_minor: i64,
    #[ts(type = "number")]
    pub refunds_minor: i64,
    #[ts(type = "number")]
    pub net_minor: i64,
    /// The tax inside `sales_minor`.
    #[ts(type = "number")]
    pub tax_minor: i64,
    pub by_type: Vec<ReportAmount>,
    /// Always 24 entries, hour 0 first.
    pub by_hour: Vec<HourSales>,
    pub by_group: Vec<NamedSales>,
    pub by_category: Vec<NamedSales>,
    pub received: Vec<ReportAmount>,
}

/// Sessions on one device over the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceSessions {
    pub device_id: String,
    pub label: String,
    pub group_id: String,
    #[ts(type = "number")]
    pub sessions: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub revenue_minor: i64,
}

/// Sessions in one device group over the range, with its utilisation per local hour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GroupSessions {
    pub group_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub devices: i64,
    #[ts(type = "number")]
    pub sessions: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub revenue_minor: i64,
    /// Always 24 entries, hour 0 first: minutes in session divided by minutes available (devices
    /// times days times 60), in basis points (10,000 = every device in session all hour).
    pub utilisation_bp: Vec<u16>,
}

/// Sessions for the business days `from` to `to` inclusive: count, billed minutes and revenue by
/// device and by group, and utilisation per hour for each group. Revenue is the session bills'
/// stored totals; a session bill that was never settled counts as none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionsReport {
    pub from: String,
    pub to: String,
    #[ts(type = "number")]
    pub days: i64,
    #[ts(type = "number")]
    pub sessions: i64,
    #[ts(type = "number")]
    pub minutes: i64,
    #[ts(type = "number")]
    pub revenue_minor: i64,
    pub by_group: Vec<GroupSessions>,
    pub by_device: Vec<DeviceSessions>,
}

/// How many shifts the Shifts report lists, newest first.
pub const SHIFT_LIST_LIMIT: i64 = 100;

/// A payment line of the Z-report that carries a reference (UPI and card), with when and for which
/// invoice it was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZReference {
    /// When the bill settled (UTC ms).
    #[ts(type = "number")]
    pub at: i64,
    /// The invoice number; empty while the bill has none (a top-up, or a sale not yet numbered).
    pub invoice_no: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
    /// The UPI or card reference; empty when none was typed.
    pub reference: String,
}

/// The UPI or card payment rows of a shift, in settle order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZReferenceGroup {
    /// `upi` or `card`.
    pub method: String,
    pub rows: Vec<ZReference>,
}

/// A refund made during the shift: a negative line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZRefund {
    pub credit_note_no: String,
    /// How the money went back (`cash`, `upi`, `card`, `wallet`); empty when it only cut credit.
    pub method: String,
    /// Negative: what the refund paid back.
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// Stock written off during the shift for one waste reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZWaste {
    pub reason: String,
    #[ts(type = "number")]
    pub qty: i64,
    /// The stock's value at the average cost.
    #[ts(type = "number")]
    pub value_minor: i64,
}

/// An expense or a deposit recorded against the shift. `amount_minor` is positive: what the
/// expense cost, or what went to the safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZCashLine {
    /// The expense category; empty for a deposit.
    pub category: String,
    /// The deposit's slip reference; empty for an expense.
    pub reference: String,
    pub note: String,
    /// `drawer` or `owner` for an expense; empty for a deposit.
    pub paid_from: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// Orders voided during the shift for one reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZVoid {
    pub reason: String,
    #[ts(type = "number")]
    pub count: i64,
    /// What the voided orders' lines came to.
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// The end-of-shift report (Story 5.28). It is built from the shift's stored rows inside the close
/// transaction and kept on the `shifts` row, so the screen and the print show the same figures; a
/// shift closed before the report existed gets one built on read (`rebuilt`).
///
/// Sales follow Story 5.27: `by_type` adds up to `sales_minor` (play time + products + plans +
/// discounts + tax added). `payments` are the positive payment rows of the shift's settled bills,
/// top-ups and credit collections (payments, not sales); refunds are separate negative lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ZReport {
    pub shift_id: String,
    #[ts(type = "number")]
    pub opened_at: i64,
    #[ts(type = "number")]
    pub closed_at: i64,
    pub opened_by: String,
    pub closed_by: String,
    /// True when this report was built from the rows at the moment it was read, because the shift
    /// closed before Z-reports were stored.
    #[serde(default)]
    pub rebuilt: bool,
    #[ts(type = "number")]
    pub orders: i64,
    #[ts(type = "number")]
    pub sales_minor: i64,
    /// The tax inside `sales_minor`.
    #[ts(type = "number")]
    pub tax_minor: i64,
    pub by_type: Vec<ReportAmount>,
    /// By method (`key`): amount and number of payment rows.
    pub payments: Vec<ReportAmount>,
    /// The UPI and card rows behind `payments`, grouped by method.
    pub references: Vec<ZReferenceGroup>,
    /// Top-ups and credit collections, as in Daily sales.
    pub received: Vec<ReportAmount>,
    pub refunds: Vec<ZRefund>,
    /// The sum of `refunds` (negative).
    #[ts(type = "number")]
    pub refunds_minor: i64,
    pub waste: Vec<ZWaste>,
    pub expenses: Vec<ZCashLine>,
    pub deposits: Vec<ZCashLine>,
    pub voids: Vec<ZVoid>,
    /// How many times the drawer was kicked open.
    #[ts(type = "number")]
    pub drawer_opens: i64,
    #[ts(type = "number")]
    pub float_minor: i64,
    #[ts(type = "number")]
    pub expected_minor: i64,
    #[ts(type = "number")]
    pub counted_minor: i64,
    /// Counted minus expected: negative is short.
    #[ts(type = "number")]
    pub variance_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub variance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub variance_note: Option<String>,
    /// Who approved a variance over the threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub approver: Option<String>,
}

/// One closed shift in the Shifts report (`GET /ui/reports/shifts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShiftListRow {
    pub shift_id: String,
    #[ts(type = "number")]
    pub opened_at: i64,
    #[ts(type = "number")]
    pub closed_at: i64,
    pub opened_by: String,
    pub closed_by: String,
    #[ts(type = "number")]
    pub counted_minor: i64,
    #[ts(type = "number")]
    pub variance_minor: i64,
}

/// The Shifts report: closed shifts, newest first (at most [`SHIFT_LIST_LIMIT`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShiftList {
    pub shifts: Vec<ShiftListRow>,
}

/// How many top spenders the Players report lists.
pub const PLAYERS_TOP_LIMIT: usize = 20;
/// How many debtors the Players report lists.
pub const PLAYERS_DEBTOR_LIMIT: usize = 50;

/// `CreditBucketRow.key`: credit taken 0 to 7 days ago.
pub const CREDIT_BUCKET_0_7: &str = "0_7";
/// `CreditBucketRow.key`: credit taken 8 to 14 days ago.
pub const CREDIT_BUCKET_8_14: &str = "8_14";
/// `CreditBucketRow.key`: credit taken 15 or more days ago.
pub const CREDIT_BUCKET_15_PLUS: &str = "15_plus";

/// One of the top spenders in the range. A player whose data key is absent has `erased = true` and
/// an empty `name` (the screen says "Erased player"); no other personal data is ever sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerSpend {
    pub player_id: String,
    pub name: String,
    pub erased: bool,
    /// Distinct business days with a settled sale in the range.
    #[ts(type = "number")]
    pub visits: i64,
    #[ts(type = "number")]
    pub spend_minor: i64,
}

/// Bills settled with no player: one total, never listed as a player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GuestTotals {
    #[ts(type = "number")]
    pub bills: i64,
    #[ts(type = "number")]
    pub spend_minor: i64,
}

/// A group of players (new, or returning) with what they spent in the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct NewReturning {
    #[ts(type = "number")]
    pub players: i64,
    #[ts(type = "number")]
    pub spend_minor: i64,
}

/// Credit outstanding in one age bucket (`CREDIT_BUCKET_*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditBucketRow {
    pub key: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// A player who owes credit today. `erased` and `name` as in [`PlayerSpend`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CreditDebtor {
    pub player_id: String,
    pub name: String,
    pub erased: bool,
    #[ts(type = "number")]
    pub outstanding_minor: i64,
    /// Days since the oldest unpaid credit was taken (first in, first out).
    #[ts(type = "number")]
    pub oldest_age_days: i64,
}

/// The Players report (Story 5.30) for the business days `from` to `to` inclusive
/// (`GET /ui/reports/players`). Sales are settled session, plan and counter bills at their stored
/// totals; voided orders, refunds, top-ups and credit collections are not in them. Credit
/// outstanding is as of today's business day (`credit_as_of`), not the end of the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayersReport {
    pub from: String,
    pub to: String,
    /// Up to [`PLAYERS_TOP_LIMIT`] players, spend descending then name.
    pub top: Vec<PlayerSpend>,
    pub guest: GuestTotals,
    /// Players whose first settled sale ever falls in the range.
    pub new_players: NewReturning,
    /// Players who had a settled sale before the range.
    pub returning_players: NewReturning,
    pub credit_as_of: String,
    /// Always three rows: 0-7, 8-14 and 15+ days, summed over every debtor.
    pub credit_buckets: Vec<CreditBucketRow>,
    #[ts(type = "number")]
    pub credit_outstanding_minor: i64,
    /// Up to [`PLAYERS_DEBTOR_LIMIT`] debtors, largest outstanding first.
    pub debtors: Vec<CreditDebtor>,
}

/// One closed shift's cash variance (counted minus expected; negative is short).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ShiftVariance {
    pub shift_id: String,
    /// When the shift closed (UTC ms).
    #[ts(type = "number")]
    pub closed_at: i64,
    #[ts(type = "number")]
    pub variance_minor: i64,
}

/// What one staff member did in the range. `name` is empty for a member no longer in the staff
/// table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffRow {
    pub staff_id: String,
    pub name: String,
    /// Settled bills this member settled (`orders.settled_by`), at their stored totals.
    #[ts(type = "number")]
    pub sales_minor: i64,
    #[ts(type = "number")]
    pub sales_count: i64,
    /// Sessions this member started, by the session's business day.
    #[ts(type = "number")]
    pub sessions_started: i64,
    /// Manual discounts approved (`discount_by`): count and value.
    #[ts(type = "number")]
    pub discounts_count: i64,
    #[ts(type = "number")]
    pub discounts_minor: i64,
    /// Orders voided (`voided_by`): count and the voided orders' totals (not in sales).
    #[ts(type = "number")]
    pub voids_count: i64,
    #[ts(type = "number")]
    pub voids_minor: i64,
    /// Refunds approved (`approved_by`): count and value.
    #[ts(type = "number")]
    pub refunds_count: i64,
    #[ts(type = "number")]
    pub refunds_minor: i64,
    /// Closed shifts of this member that closed in the range, oldest first.
    pub shifts: Vec<ShiftVariance>,
    /// The sum of the shifts' variances.
    #[ts(type = "number")]
    pub variance_minor: i64,
}

/// The Staff report (Story 5.30): one row per staff member with any activity in the range, sales
/// total descending then name (`GET /ui/reports/staff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StaffReport {
    pub from: String,
    pub to: String,
    pub staff: Vec<StaffRow>,
}

/// One player on the Hub's overdue credit list (Story 8.31), oldest unpaid spend first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OverdueRow {
    pub player_id: String,
    /// Empty when the player's key is gone (`erased`).
    pub name: String,
    pub erased: bool,
    #[ts(type = "number")]
    pub outstanding_minor: i64,
    /// The business day of the oldest spend still unpaid (first in, first out).
    pub oldest_unpaid_day: String,
    #[ts(type = "number")]
    pub age_days: i64,
    /// The aging bucket of that spend: `0_7`, `8_14` or `15_plus`.
    pub bucket: String,
    /// The player's latest consent to service messages: granted, withdrawn, or `None` for none.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub consent: Option<bool>,
    /// The business day of the last credit reminder the cloud sent (paid branches only).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_reminder_day: Option<String>,
}

/// `GET /ui/players/overdue` (Story 8.31): players with credit outstanding, oldest unpaid spend
/// first. `reminders` is false on a Free branch, which shows the list with a tier lock and never
/// queues a reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OverdueView {
    /// The business day credit is aged as of.
    pub as_of_day: String,
    pub reminders: bool,
    pub rows: Vec<OverdueRow>,
}

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

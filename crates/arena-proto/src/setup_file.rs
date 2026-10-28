//! The branch setup file (Story 5.36): a versioned JSON file with a branch's device groups, hourly
//! rates, products, bundles, plans, pricing rules and coupons, so a second branch can start from
//! the first without the internet. It holds no players, devices, money or stock rows and no
//! images. Everything that points at something else does so by *name* (a plan names its groups, a
//! bundle its products), because ids differ from one Hub to the next.
//!
//! Also the `import.setup` intent and the requests and answers of
//! `POST /ui/import/setup/{export,preview,commit}`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The `format` member of a setup file.
pub const SETUP_FORMAT: &str = "arena360-setup";
/// The format version this build writes and reads. A file with a higher version is refused
/// `UNSUPPORTED`.
pub const SETUP_VERSION: u32 = 1;
/// The largest setup file the Hub reads, in bytes (the Story 5.37 template limit).
pub const SETUP_MAX_BYTES: usize = 1_048_576;
/// The most entries one list of a setup file may hold.
pub const SETUP_MAX_ENTRIES: usize = 2_000;
/// The file name an export is saved under.
pub const SETUP_FILE_NAME: &str = "arena360-branch-setup.json";
/// What a Hub that is too old says about a newer file.
pub const SETUP_UPDATE_HUB: &str = "Update this Hub to import this file.";

fn yes() -> bool {
    true
}

/// A device group. `rate_minor` is its hourly rate in minor units of the file's currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupFileGroup {
    pub name: String,
    pub kind: String,
    pub colour: String,
    #[serde(default)]
    #[ts(type = "number")]
    pub sort_order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub rate_minor: Option<i64>,
}

/// A time pack, block or pass, naming the groups it can be used on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupPlan {
    pub name: String,
    pub plan_kind: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub validity_days: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub window_end_min: Option<i64>,
    pub groups: Vec<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

/// A pricing rule of one group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupRule {
    pub name: String,
    /// The group's name.
    pub group: String,
    pub rule_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub days_mask: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub start_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub end_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_to: Option<String>,
    pub adjust_kind: String,
    #[ts(type = "number")]
    pub adjust_value: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub priority: i64,
    #[serde(default = "yes")]
    pub active: bool,
}

/// What a coupon is limited to: a `group`, `plan` or `product`, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCouponTarget {
    pub kind: String,
    pub name: String,
}

/// A coupon. How often it was used is not part of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCoupon {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub percent_bp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub amount_minor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub valid_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub usage_limit: Option<i64>,
    #[serde(default)]
    #[ts(type = "number")]
    pub min_bill_minor: i64,
    #[serde(default)]
    pub targets: Vec<SetupCouponTarget>,
    #[serde(default = "yes")]
    pub active: bool,
}

/// A product, without stock or image. Its category is just the label it carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupProduct {
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_rate_bp: Option<u16>,
    #[serde(default)]
    pub hsn: String,
    #[serde(default)]
    pub track_stock: bool,
    #[serde(default)]
    pub barcode: String,
    #[serde(default)]
    pub kitchen: bool,
    #[serde(default)]
    pub favourite: bool,
    #[serde(default = "yes")]
    pub active: bool,
}

/// One part of a bundle: a product by name, and how many.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupComponent {
    pub product: String,
    #[ts(type = "number")]
    pub qty: i64,
}

/// A bundle priced on its own, made of products by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupBundle {
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_rate_bp: Option<u16>,
    #[serde(default)]
    pub hsn: String,
    #[serde(default)]
    pub barcode: String,
    #[serde(default)]
    pub kitchen: bool,
    #[serde(default)]
    pub favourite: bool,
    #[serde(default = "yes")]
    pub active: bool,
    pub components: Vec<SetupComponent>,
}

/// The whole file. Amounts are minor units of `currency`, which must be the importing branch's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupFile {
    pub format: String,
    pub version: u32,
    pub currency: String,
    #[serde(default)]
    pub groups: Vec<SetupFileGroup>,
    #[serde(default)]
    pub products: Vec<SetupProduct>,
    #[serde(default)]
    pub bundles: Vec<SetupBundle>,
    #[serde(default)]
    pub plans: Vec<SetupPlan>,
    #[serde(default)]
    pub rules: Vec<SetupRule>,
    #[serde(default)]
    pub coupons: Vec<SetupCoupon>,
}

/// The payload of `import.setup`: the file, of which the Hub creates what it does not have by
/// name. All of it applies or none of it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportSetup {
    pub file: SetupFile,
}

/// The body of `POST /ui/import/setup/preview`: the text of a setup file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupPreviewRequest {
    pub json: String,
}

/// The body of `POST /ui/import/setup/commit`; `import_id` works as in
/// [`ProductImportCommitRequest`](crate::ProductImportCommitRequest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupCommitRequest {
    pub json: String,
    pub import_id: String,
}

/// What the dry-run decided for one entry of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupItem {
    /// `group`, `rate`, `product`, `bundle`, `plan`, `rule` or `coupon`.
    pub kind: String,
    /// The name (the code for a coupon, the group's name for a rate).
    pub name: String,
    /// Whether commit creates it.
    pub add: bool,
    /// Why it is skipped: `exists`, `invalid`, `no_group`, `no_product` or `no_plan`. Absent when
    /// added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// How many entries of one kind are added and skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupKindCount {
    pub kind: String,
    #[ts(type = "number")]
    pub added: i64,
    #[ts(type = "number")]
    pub skipped: i64,
}

/// What a setup-file dry-run found. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupPreview {
    pub counts: Vec<SetupKindCount>,
    pub items: Vec<SetupItem>,
    #[ts(type = "number")]
    pub added: i64,
    #[ts(type = "number")]
    pub skipped: i64,
    /// Whether commit has anything to create.
    pub can_commit: bool,
}

/// What a setup-file commit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetupFileResult {
    pub counts: Vec<SetupKindCount>,
    #[ts(type = "number")]
    pub added: i64,
    #[ts(type = "number")]
    pub skipped: i64,
}

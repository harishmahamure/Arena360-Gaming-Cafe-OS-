//! CSV import with a dry-run (Story 5.33): the `import.products` intent, and the request and
//! answers of `POST /ui/import/products/{preview,commit,template}`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How many rows one `import.products` intent carries. A bigger file is committed in chunks,
/// each with its own idempotency key.
pub const IMPORT_CHUNK_ROWS: usize = 500;
/// The most data rows one file may hold.
pub const IMPORT_MAX_ROWS: usize = 10_000;
/// The largest CSV the Hub reads, in bytes.
pub const IMPORT_MAX_BYTES: usize = 1_500_000;
/// The file's columns, in the template's order. `name` and `price` must be present; the others
/// may be left out.
pub const IMPORT_PRODUCT_COLUMNS: [&str; 10] = [
    "name",
    "category",
    "price",
    "tax_rate",
    "hsn",
    "barcode",
    "track_stock",
    "kitchen",
    "opening_qty",
    "unit_cost",
];
/// The most failures a preview lists (the counts are always complete).
pub const IMPORT_MAX_FAILURES: usize = 200;

/// One product row of an import, in minor units and basis points, already read from the CSV.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportProductRow {
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[ts(type = "number")]
    pub price_minor: i64,
    /// The tax rate in basis points; none takes the branch's product rate (a new product) or
    /// keeps the product's own (an update).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_rate_bp: Option<u16>,
    #[serde(default)]
    pub hsn: String,
    #[serde(default)]
    pub barcode: String,
    #[serde(default)]
    pub track_stock: bool,
    #[serde(default)]
    pub kitchen: bool,
    /// Units received as a stock-in lot (`stock.in` rules); 0 records none.
    #[serde(default)]
    #[ts(type = "number")]
    pub opening_qty: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub unit_cost_minor: i64,
}

/// The payload of `import.products`: up to [`IMPORT_CHUNK_ROWS`] rows, each creating a product or
/// updating the one it matches (by barcode, else exact name), and recording opening stock. All
/// rows apply or none do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportProducts {
    pub rows: Vec<ImportProductRow>,
}

/// The body of `POST /ui/import/products/preview` (a dry-run, which writes nothing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductImportPreviewRequest {
    pub csv: String,
}

/// The body of `POST /ui/import/products/commit`. `import_id` is a client ULID for this attempt:
/// each chunk's idempotency key is derived from it, so a retry with the same id never duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductImportCommitRequest {
    pub csv: String,
    pub import_id: String,
    /// Import the rows that passed and skip the failed ones.
    #[serde(default)]
    pub valid_only: bool,
}

/// One problem found in a file. `row` is the line of the file (the header is line 1); 0 when the
/// problem is the file as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportFailure {
    #[ts(type = "number")]
    pub row: i64,
    pub field: String,
    /// A stable code: `required`, `bad_number`, `bad_flag`, `out_of_range`, `too_long`,
    /// `bad_value`, `unknown_column`, `missing_column`, `duplicate_barcode`, `duplicate_product`,
    /// `barcode_taken`, `bundle`, `needs_hsn`, `needs_tracking`, `bad_file`; players add `bad_phone`
    /// and `duplicate_phone`; plans add `bad_time`, `unknown_group`, `bad_groups`, `group_kind`,
    /// `kind_change` and `duplicate_plan`.
    pub reason: String,
    pub message: String,
}

/// What a dry-run found. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductImportPreview {
    /// Data rows in the file.
    #[ts(type = "number")]
    pub rows: i64,
    /// Rows that create a product.
    #[ts(type = "number")]
    pub new: i64,
    /// Rows that update a product (matched by barcode, else exact name).
    #[ts(type = "number")]
    pub updated: i64,
    /// Rows that failed.
    #[ts(type = "number")]
    pub failed: i64,
    /// Rows that record opening stock.
    #[ts(type = "number")]
    pub with_stock: i64,
    /// Problems, the file's first, then by row; at most [`IMPORT_MAX_FAILURES`].
    pub failures: Vec<ImportFailure>,
    /// Whether the list above was cut.
    pub truncated: bool,
    /// Whether the file as a whole is unusable (no header, a missing column): nothing can be
    /// imported, not even the valid rows.
    pub unusable: bool,
    /// Whether commit is open without "valid rows only": no failure at all.
    pub can_commit: bool,
    /// Whether "valid rows only" would import something.
    pub can_commit_valid: bool,
}

/// What a commit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProductImportResult {
    #[ts(type = "number")]
    pub created: i64,
    #[ts(type = "number")]
    pub updated: i64,
    /// Failed rows left out ("valid rows only").
    #[ts(type = "number")]
    pub skipped: i64,
    /// Intents applied (one per [`IMPORT_CHUNK_ROWS`] rows).
    #[ts(type = "number")]
    pub chunks: i64,
}

/// The body of `POST /ui/import/products/template`: where to save the template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportTemplateRequest {
    pub folder: String,
}

/// What a template download wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportTemplateSaved {
    pub file_name: String,
    pub path: String,
}

// ----------------------------------------------------------------------------------------------
// Players and plans (Story 5.34)
// ----------------------------------------------------------------------------------------------

/// The player file's columns, in the template's order. `phone` and `name` must be present.
pub const IMPORT_PLAYER_COLUMNS: [&str; 4] = ["phone", "name", "balance", "credit_limit"];
/// The plan file's columns, in the template's order. `name`, `type` and `price` must be present.
pub const IMPORT_PLAN_COLUMNS: [&str; 8] = [
    "name",
    "type",
    "hours",
    "price",
    "validity_days",
    "window_start",
    "window_end",
    "groups",
];
/// The `password_hash` of an imported player: not a hash, so no password ever verifies until the
/// player sets one at the counter (Story 3.8).
pub const IMPORT_NO_PASSWORD: &str = "!";
/// The reminder days of credit an import enables (the same as the player screen's default).
pub const IMPORT_CREDIT_REMINDER_DAYS: [u16; 3] = [3, 7, 14];

/// One player of an import, already sealed by the Hub: personal data is ciphertext exactly as
/// `player.create` carries it, the amounts are minor units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportPlayerRow {
    pub player_id: String,
    pub username: String,
    pub phone_lookup: String,
    pub name_enc: String,
    pub phone_enc: String,
    pub wrapped_key: String,
    pub policy_version: String,
    /// Opening wallet balance; 0 adds no ledger row.
    #[ts(type = "number")]
    pub balance_minor: i64,
    /// Credit limit; above 0 enables credit for the player.
    #[ts(type = "number")]
    pub credit_limit_minor: i64,
}

/// The payload of `import.players`: up to [`IMPORT_CHUNK_ROWS`] new players. Each gets a profile
/// with consent recorded as not granted and no password, a wallet, an `import` ledger row for a
/// balance above 0 (no cash event, no invoice), and credit when a limit is set. All rows apply or
/// none do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportPlayers {
    pub rows: Vec<ImportPlayerRow>,
}

/// One plan of an import, in minor units and minutes, with its groups already matched by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportPlanRow {
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
    pub group_ids: Vec<String>,
}

/// The payload of `import.plans`: up to [`IMPORT_CHUNK_ROWS`] rows, each creating a plan or
/// updating the one with exactly its name, through the `plan.upsert` rules. All rows apply or
/// none do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportPlans {
    pub rows: Vec<ImportPlanRow>,
}

/// The body of `POST /ui/import/{players,plans}/preview` (a dry-run, which writes nothing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportPreviewRequest {
    pub csv: String,
}

/// The body of `POST /ui/import/{players,plans}/commit`; `import_id` works as in
/// [`ProductImportCommitRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ImportCommitRequest {
    pub csv: String,
    pub import_id: String,
    /// Import the rows that passed and skip the failed ones.
    #[serde(default)]
    pub valid_only: bool,
}

/// What a player dry-run found. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerImportPreview {
    /// Data rows in the file.
    #[ts(type = "number")]
    pub rows: i64,
    /// Rows that create a player.
    #[ts(type = "number")]
    pub new: i64,
    /// Rows whose phone already has a profile; they are left as they are.
    #[ts(type = "number")]
    pub matched: i64,
    /// Rows that failed.
    #[ts(type = "number")]
    pub failed: i64,
    /// New players that start with a wallet balance.
    #[ts(type = "number")]
    pub with_balance: i64,
    /// New players that get a credit limit.
    #[ts(type = "number")]
    pub with_credit: i64,
    /// Problems, the file's first, then by row; at most [`IMPORT_MAX_FAILURES`].
    pub failures: Vec<ImportFailure>,
    pub truncated: bool,
    pub unusable: bool,
    /// Whether commit is open without "valid rows only": no failure and something to add.
    pub can_commit: bool,
    /// Whether "valid rows only" would import something.
    pub can_commit_valid: bool,
}

/// What a player commit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerImportResult {
    #[ts(type = "number")]
    pub created: i64,
    /// Rows whose phone already had a profile.
    #[ts(type = "number")]
    pub matched: i64,
    /// Failed rows left out ("valid rows only").
    #[ts(type = "number")]
    pub skipped: i64,
    #[ts(type = "number")]
    pub chunks: i64,
}

/// What a plan dry-run found. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanImportPreview {
    #[ts(type = "number")]
    pub rows: i64,
    /// Rows that create a plan.
    #[ts(type = "number")]
    pub new: i64,
    /// Rows that update the plan with exactly their name.
    #[ts(type = "number")]
    pub updated: i64,
    #[ts(type = "number")]
    pub failed: i64,
    pub failures: Vec<ImportFailure>,
    pub truncated: bool,
    pub unusable: bool,
    pub can_commit: bool,
    pub can_commit_valid: bool,
}

/// What a plan commit did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlanImportResult {
    #[ts(type = "number")]
    pub created: i64,
    #[ts(type = "number")]
    pub updated: i64,
    #[ts(type = "number")]
    pub skipped: i64,
    #[ts(type = "number")]
    pub chunks: i64,
}

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
    /// `barcode_taken`, `bundle`, `needs_hsn`, `needs_tracking`, `bad_file`.
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

//! Exporting a Hub report to a file (Story 5.32): the request of `POST /ui/reports/export` and
//! what it wrote.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The report ids an export accepts, as the Hub's Reports screens name them. `stock-*` ones are
/// the tabs of Reports → Stock.
pub const EXPORT_REPORTS: [&str; 11] = [
    "daily-sales",
    "sessions",
    "shifts",
    "z-report",
    "players",
    "staff",
    "stock-on-hand",
    "stock-moves",
    "stock-waste",
    "stock-expiring",
    "gst",
];

/// A file format an export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ExportFormat {
    Csv,
    Pdf,
}

impl ExportFormat {
    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Pdf => "pdf",
        }
    }
}

/// The payload of `POST /ui/reports/export`. Which of `from`/`to`, `month`, `shift_id`, `product`
/// and `reason` a report reads is the same as its `GET` read; the rest are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReportExportRequest {
    /// One of [`EXPORT_REPORTS`].
    pub report: String,
    pub format: ExportFormat,
    /// An absolute path of a writable folder (a USB drive's folder works).
    pub folder: String,
    /// First business day (`YYYY-MM-DD`) of a ranged report.
    #[serde(default)]
    #[ts(optional)]
    pub from: Option<String>,
    /// Last business day (`YYYY-MM-DD`) of a ranged report.
    #[serde(default)]
    #[ts(optional)]
    pub to: Option<String>,
    /// `YYYY-MM` of the GST view.
    #[serde(default)]
    #[ts(optional)]
    pub month: Option<String>,
    /// The shift of a Z-report.
    #[serde(default)]
    #[ts(optional)]
    pub shift_id: Option<String>,
    /// The product filter of the stock movements.
    #[serde(default)]
    #[ts(optional)]
    pub product: Option<String>,
    /// The reason filter of the stock movements.
    #[serde(default)]
    #[ts(optional)]
    pub reason: Option<String>,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ReportExported {
    pub file_name: String,
    pub path: String,
    #[ts(type = "number")]
    pub bytes: i64,
    pub format: ExportFormat,
}

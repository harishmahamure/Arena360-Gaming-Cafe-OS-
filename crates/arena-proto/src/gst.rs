//! The GST return export (Story 5.29): `GET /ui/reports/gst?month=YYYY-MM` shows a month's totals
//! per GSTR-1 section and `POST /ui/reports/gst/export` writes the ZIP of CSVs to a folder
//! (permission `report.view`, GST branches only). All amounts are minor units.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GstSection.key`: invoices to registered buyers (`b2b.csv`).
pub const GST_B2B: &str = "b2b";
/// `GstSection.key`: invoices to consumers, by rate, net of their credit notes (`b2cs.csv`).
pub const GST_B2CS: &str = "b2cs";
/// `GstSection.key`: credit notes to registered buyers (`cdnr.csv`).
pub const GST_CDNR: &str = "cdnr";
/// `GstSection.key`: the HSN summary, invoices minus credit notes (`hsn.csv`).
pub const GST_HSN: &str = "hsn";

/// The file name of a month's export: `GSTR1_<branch code>_<YYYY-MM>.zip`.
pub fn gst_file_name(branch_code: &str, month: &str) -> String {
    let code: String = branch_code
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("GSTR1_{code}_{month}.zip")
}

/// One section of the return: how many documents it holds and what they add up to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstSection {
    pub key: String,
    #[ts(type = "number")]
    pub documents: i64,
    #[ts(type = "number")]
    pub taxable_minor: i64,
    #[ts(type = "number")]
    pub central_minor: i64,
    #[ts(type = "number")]
    pub state_minor: i64,
    #[ts(type = "number")]
    pub integrated_minor: i64,
}

/// A month of the return, as the GST screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstMonth {
    /// `YYYY-MM`.
    pub month: String,
    /// B2B, B2CS, CDNR and the HSN summary, in that order.
    pub sections: Vec<GstSection>,
    #[ts(type = "number")]
    pub invoices: i64,
    #[ts(type = "number")]
    pub credit_notes: i64,
    /// Invoices listed at 0% because the Hub stored no tax breakup for them.
    #[ts(type = "number")]
    pub untaxed: i64,
    /// Settled sales of the month that have no invoice number yet, so are not in the files.
    #[ts(type = "number")]
    pub unnumbered: i64,
    /// No invoice and no credit note in the month.
    pub empty: bool,
}

/// The payload of `POST /ui/reports/gst/export`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstExportRequest {
    /// `YYYY-MM`.
    pub month: String,
    /// An absolute path of a writable folder (a USB drive's folder works).
    pub folder: String,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstExported {
    pub file_name: String,
    pub path: String,
    #[ts(type = "number")]
    pub bytes: i64,
    pub month: GstMonth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_keeps_only_safe_characters() {
        assert_eq!(gst_file_name("KOR", "2026-10"), "GSTR1_KOR_2026-10.zip");
        assert_eq!(gst_file_name("A/B c", "2026-10"), "GSTR1_A-B-c_2026-10.zip");
    }
}

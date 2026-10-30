//! The yearly GST archive (Story 9.14, AD-36): `gst-archive/<fy>.zst`, a zstd JSON file of every
//! numbered invoice (with its lines), every credit note and the per-rate tax totals of one
//! financial year, kept for 8 years on paid tiers because a tax notice can ask that long after
//! the backup that held the same rows has been pruned.
//!
//! The file is built from the stored rows as they are: tax is never recomputed. All money is
//! integer minor units; a credit note's amounts and rates are negative as stored. It names a buyer
//! only where the invoice already does (a business invoice's GSTIN and legal name).
//!
//! `gst_archive.publish` is the system intent that records an upload in `gst_archive_log`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::TaxMode;

/// The `kind` of a presign request for a yearly GST archive.
pub const PRESIGN_KIND_GST_ARCHIVE: &str = "gst_archive";
/// The schema of `gst-archive/<fy>.zst` this release writes.
pub const GST_ARCHIVE_SCHEMA: u32 = 1;
/// The longest object key an archive record keeps.
pub const GST_ARCHIVE_KEY_MAX_CHARS: usize = 128;
/// The most orphaned invoice numbers one publish records.
pub const GST_ARCHIVE_ORPHANS_CAP: usize = 100_000;

/// The object key of the archive of `fy` under the branch's prefix of the data bucket; what
/// `gst_archive_log.key` records. `fy` must satisfy [`valid_fy`].
pub fn gst_archive_object_key(fy: &str) -> String {
    format!("gst-archive/{fy}.zst")
}

/// Whether `fy` is a financial-year label as the Hub stamps it: `FY2026-27` (a year that starts
/// in the year and ends in the next, two digits) or `FY2027` (a calendar year). Nothing else, so
/// a label can be part of an object key with no path characters in it.
pub fn valid_fy(fy: &str) -> bool {
    let Some(rest) = fy.strip_prefix("FY") else {
        return false;
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    match rest.split_once('-') {
        None => rest.len() == 4 && digits(rest),
        Some((year, end)) => {
            if year.len() != 4 || end.len() != 2 || !digits(year) || !digits(end) {
                return false;
            }
            let (Ok(y), Ok(e)) = (year.parse::<u32>(), end.parse::<u32>()) else {
                return false;
            };
            (y + 1) % 100 == e
        }
    }
}

/// How a financial-year label reads on screen: `FY2026-27` is `26-27`, `FY2027` is `2027`.
pub fn gst_archive_year_label(fy: &str) -> String {
    let rest = fy.strip_prefix("FY").unwrap_or(fy);
    match rest.split_once('-') {
        Some((year, end)) if year.len() == 4 => format!("{}-{end}", &year[2..]),
        _ => rest.to_owned(),
    }
}

/// The tax of one rate, as stored on an invoice or a credit note (negative on a credit note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveRate {
    #[ts(type = "number")]
    pub rate_bp: i64,
    #[ts(type = "number")]
    pub taxable_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// Central tax, when the Hub stored it (a GST branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub cgst_minor: Option<i64>,
    /// State tax, when the Hub stored it (a GST branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub sgst_minor: Option<i64>,
}

/// One line of an invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveLine {
    pub id: String,
    /// `time`, `product` or `plan`.
    pub kind: String,
    pub label: String,
    #[ts(type = "number")]
    pub qty: i64,
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub tax_rate_bp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hsn: Option<String>,
}

/// A numbered, settled sales invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveInvoice {
    pub invoice_no: String,
    pub order_id: String,
    /// `session`, `plan_sale` or `pos_sale`.
    pub kind: String,
    pub business_day: String,
    #[ts(type = "number")]
    pub settled_at: i64,
    #[ts(type = "number")]
    pub total_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// The stored per-rate tax; absent when the Hub stored none for the sale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_breakup: Option<Vec<GstArchiveRate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub buyer_gstin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub buyer_name: Option<String>,
    pub lines: Vec<GstArchiveLine>,
}

/// A credit note: a refund of part of an invoiced order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveCreditNote {
    pub credit_note_no: String,
    /// The invoice it refunds (empty when the order has no number).
    pub invoice_no: String,
    pub order_id: String,
    pub business_day: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub reason: String,
    /// The refund total, tax included (a positive magnitude, as stored).
    #[ts(type = "number")]
    pub amount_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// The stored per-rate tax of the refunded lines (negative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tax_breakup: Option<Vec<GstArchiveRate>>,
    /// The refunded lines, as stored on the credit note.
    #[ts(type = "Array<unknown>")]
    pub lines: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub buyer_gstin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub buyer_name: Option<String>,
}

/// The totals of one rate over a set of documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveRateTotal {
    #[ts(type = "number")]
    pub rate_bp: i64,
    #[ts(type = "number")]
    pub taxable_minor: i64,
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// Present when every contributing rate stored it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub cgst_minor: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub sgst_minor: Option<i64>,
}

/// `gst-archive/<fy>.zst`, schema 1 (the JSON inside the zstd frame).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveV1 {
    pub schema: u32,
    pub fy: String,
    pub branch_id: String,
    pub branch_code: String,
    pub currency: String,
    /// When it was built (Hub clock, ms).
    #[ts(type = "number")]
    pub built_at: i64,
    /// `gst_in` or `simple`: how the stored tax is to be read.
    pub tax_mode: TaxMode,
    /// The generic tax label of the branch (`Tax`, `VAT`, ...).
    pub tax_label: String,
    pub invoices: Vec<GstArchiveInvoice>,
    pub credit_notes: Vec<GstArchiveCreditNote>,
    /// Per-rate totals over the invoices.
    pub invoice_summary: Vec<GstArchiveRateTotal>,
    /// Per-rate totals over the credit notes (negative).
    pub credit_note_summary: Vec<GstArchiveRateTotal>,
    /// Invoice numbers of this year that a fork set aside, which the live database does not hold.
    pub orphaned_invoices: Vec<String>,
}

/// The payload of `gst_archive.publish` (system only): the archive of `fy` was PUT to the cloud.
/// Upserts the `gst_archive_log` row of that year and writes nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchivePublish {
    /// The financial year, `FY2026-27` or `FY2027`.
    pub fy: String,
    /// The object key it was PUT to (`gst-archive/<fy>.zst`).
    pub key: String,
    /// SHA-256 of the bytes PUT, lower-case hex.
    pub sha256: String,
    /// The size PUT.
    #[ts(type = "number")]
    pub bytes: i64,
    /// Invoices in the file.
    #[ts(type = "number")]
    pub invoices: i64,
    /// Credit notes in the file.
    #[ts(type = "number")]
    pub credit_notes: i64,
    /// The orphaned invoice numbers in the file.
    pub orphaned: Vec<String>,
    /// When it was built (Hub clock, ms).
    #[ts(type = "number")]
    pub built_at: i64,
}

/// A year archived, as System -> Backup lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveEntry {
    pub fy: String,
    /// `26-27` for `FY2026-27`.
    pub label: String,
    #[ts(type = "number")]
    pub invoices: i64,
    #[ts(type = "number")]
    pub credit_notes: i64,
    /// How many invoice numbers a fork set aside.
    #[ts(type = "number")]
    pub orphaned: i64,
    #[ts(type = "number")]
    pub bytes: i64,
    /// When the Hub built the file (ms).
    #[ts(type = "number")]
    pub built_at: i64,
}

/// The years archived, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GstArchiveList {
    pub archives: Vec<GstArchiveEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_financial_year_label_has_no_path_characters() {
        for ok in ["FY2026-27", "FY2027", "FY2099-00", "FY2025-26"] {
            assert!(valid_fy(ok), "{ok}");
        }
        for bad in [
            "",
            "FY",
            "FY26-27",
            "FY2026-28",
            "FY2026-2",
            "2026-27",
            "fy2026-27",
            "FY2026/27",
            "../FY2026-27",
            "FY2026-27/x",
            "FY2026-27 ",
            "FY2026-27.zst",
            "FY2026\\27",
            "FY20x6",
            "FY２０２６",
        ] {
            assert!(!valid_fy(bad), "{bad}");
        }
    }

    #[test]
    fn the_key_and_the_label_follow_the_year() {
        assert_eq!(
            gst_archive_object_key("FY2026-27"),
            "gst-archive/FY2026-27.zst"
        );
        assert_eq!(gst_archive_year_label("FY2026-27"), "26-27");
        assert_eq!(gst_archive_year_label("FY2027"), "2027");
        assert_eq!(gst_archive_year_label("FY2099-00"), "99-00");
    }
}

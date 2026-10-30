//! The yearly GST archive (Story 9.14, AD-36): builds `gst-archive/<fy>.zst` for a closed
//! financial year from the stored invoices, credit notes and their per-rate tax, on the snapshot
//! reader's own connection. It never runs inside `apply()`, never writes a row and makes no
//! network call; the upload task publishes it.
//!
//! Loading (`arena-store`) and folding (here) are separate: the fold is pure, so the same rows
//! give byte-identical JSON. No tax is recomputed: a rate's taxable value and tax are the stored
//! ones, and the per-rate summary only adds them up.

use std::collections::{BTreeMap, BTreeSet};

use arena_proto::{
    AppError, DEFAULT_TAX, ErrorCode, GST_ARCHIVE_SCHEMA, GstArchiveCreditNote, GstArchiveEntry,
    GstArchiveInvoice, GstArchiveLine, GstArchiveList, GstArchiveRate, GstArchiveRateTotal,
    GstArchiveV1, KEY_TAX, TaxMode, TaxSettings, gst_archive_year_label, valid_fy,
};
use arena_store::{
    ArchiveCreditRow, ArchiveInvoiceRow, GstArchiveRows, GstLineRow, Store, find_gst_archive_log,
    gst_fingerprints, last_rollup_date, list_gst_archive_logs, load_gst_archive_rows,
    scan_orphaned_invoices,
};
use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use crate::invoice::tax::RateTax;
use crate::locale::{effective_locale, fy_of, locale_rows};

/// The zstd level of an archive: it is written once a year and read rarely.
const ZSTD_LEVEL: i32 = 6;

/// What the file says about the branch it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveContext {
    pub fy: String,
    pub branch_id: String,
    pub branch_code: String,
    pub currency: String,
    pub built_at: i64,
    pub tax_mode: TaxMode,
    pub tax_label: String,
}

/// An archive built and compressed, ready to PUT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstArchiveFile {
    pub fy: String,
    pub bytes: Vec<u8>,
    /// SHA-256 of `bytes`, lower-case hex.
    pub sha256: String,
    pub invoices: i64,
    pub credit_notes: i64,
    /// The orphaned invoice numbers in the file (sorted).
    pub orphaned: Vec<String>,
    pub built_at: i64,
    /// Files under `orphaned/` that could not be read and were skipped.
    pub unreadable_orphans: usize,
}

fn rates_of(json: Option<&str>) -> Option<Vec<GstArchiveRate>> {
    let taxes = json
        .and_then(|j| serde_json::from_str::<Vec<RateTax>>(j).ok())
        .filter(|v| !v.is_empty())?;
    Some(
        taxes
            .into_iter()
            .map(|t| GstArchiveRate {
                rate_bp: i64::from(t.rate_bp),
                taxable_minor: t.taxable_minor,
                tax_minor: t.tax_minor,
                cgst_minor: t.cgst_minor,
                sgst_minor: t.sgst_minor,
            })
            .collect(),
    )
}

fn line_of(l: &GstLineRow) -> GstArchiveLine {
    GstArchiveLine {
        id: l.id.clone(),
        kind: l.kind.clone(),
        label: l.label.clone(),
        qty: l.qty,
        amount_minor: l.amount_minor,
        tax_rate_bp: l.tax_rate_bp,
        hsn: l.hsn.clone(),
    }
}

fn invoice_of(
    r: &ArchiveInvoiceRow,
    lines: &BTreeMap<&str, Vec<&GstLineRow>>,
) -> GstArchiveInvoice {
    GstArchiveInvoice {
        invoice_no: r.invoice_no.clone(),
        order_id: r.order_id.clone(),
        kind: r.kind.clone(),
        business_day: r.business_day.clone(),
        settled_at: r.settled_at,
        total_minor: r.total_minor,
        tax_minor: r.tax_minor,
        tax_breakup: rates_of(r.tax_breakup.as_deref()),
        buyer_gstin: r.buyer_gstin.clone().filter(|g| !g.trim().is_empty()),
        buyer_name: r.buyer_name.clone().filter(|n| !n.trim().is_empty()),
        lines: lines
            .get(r.order_id.as_str())
            .map(|ls| ls.iter().map(|l| line_of(l)).collect())
            .unwrap_or_default(),
    }
}

fn credit_of(r: &ArchiveCreditRow) -> GstArchiveCreditNote {
    let lines = match serde_json::from_str::<Vec<serde_json::Value>>(&r.lines) {
        Ok(v) => v,
        Err(_) => vec![serde_json::Value::String(r.lines.clone())],
    };
    GstArchiveCreditNote {
        credit_note_no: r.credit_note_no.clone(),
        invoice_no: r.invoice_no.clone(),
        order_id: r.order_id.clone(),
        business_day: r.business_day.clone(),
        created_at: r.created_at,
        reason: r.reason.clone(),
        amount_minor: r.amount_minor,
        tax_minor: r.tax_minor,
        tax_breakup: rates_of(r.tax_breakup.as_deref()),
        lines,
        buyer_gstin: r.buyer_gstin.clone().filter(|g| !g.trim().is_empty()),
        buyer_name: r.buyer_name.clone().filter(|n| !n.trim().is_empty()),
    }
}

/// One rate being added up. `central` and `state` stay `Some` only while every rate added carried
/// them.
#[derive(Default)]
struct Sum {
    taxable: i64,
    tax: i64,
    central: Option<i64>,
    state: Option<i64>,
    started: bool,
}

impl Sum {
    fn add(&mut self, taxable: i64, tax: i64, central: Option<i64>, state: Option<i64>) {
        self.taxable = self.taxable.saturating_add(taxable);
        self.tax = self.tax.saturating_add(tax);
        if self.started {
            self.central = self.central.zip(central).map(|(a, b)| a.saturating_add(b));
            self.state = self.state.zip(state).map(|(a, b)| a.saturating_add(b));
        } else {
            self.started = true;
            self.central = central;
            self.state = state;
        }
    }
}

/// The per-rate totals over `docs`, each `(stored breakup, document total)`. A document with no
/// stored breakup counts at 0% with its total as taxable value, as the GST return lists it. A
/// credit note's amounts are made negative (they are stored so).
fn summarise<'a>(
    docs: impl Iterator<Item = (Option<&'a [GstArchiveRate]>, i64)>,
    credit: bool,
) -> Vec<GstArchiveRateTotal> {
    let sign = |v: i64| if credit { -v.saturating_abs() } else { v };
    let mut by_rate = BTreeMap::<i64, Sum>::new();
    for (breakup, total) in docs {
        match breakup {
            Some(rates) => {
                for r in rates {
                    by_rate.entry(r.rate_bp).or_default().add(
                        sign(r.taxable_minor),
                        sign(r.tax_minor),
                        r.cgst_minor.map(sign),
                        r.sgst_minor.map(sign),
                    );
                }
            }
            None => by_rate
                .entry(0)
                .or_default()
                .add(sign(total), 0, None, None),
        }
    }
    by_rate
        .into_iter()
        .map(|(rate_bp, s)| GstArchiveRateTotal {
            rate_bp,
            taxable_minor: s.taxable,
            tax_minor: s.tax,
            cgst_minor: s.central,
            sgst_minor: s.state,
        })
        .collect()
}

/// Folds the stored rows of a year into the archive. `orphaned` are the invoice numbers a fork
/// set aside; they are sorted and de-duplicated.
pub fn build_archive(
    ctx: ArchiveContext,
    rows: &GstArchiveRows,
    orphaned: impl IntoIterator<Item = String>,
) -> GstArchiveV1 {
    let mut lines = BTreeMap::<&str, Vec<&GstLineRow>>::new();
    for l in &rows.lines {
        lines.entry(l.order_id.as_str()).or_default().push(l);
    }
    let invoices: Vec<GstArchiveInvoice> = rows
        .invoices
        .iter()
        .map(|r| invoice_of(r, &lines))
        .collect();
    let credit_notes: Vec<GstArchiveCreditNote> = rows.credits.iter().map(credit_of).collect();
    let invoice_summary = summarise(
        invoices
            .iter()
            .map(|i| (i.tax_breakup.as_deref(), i.total_minor)),
        false,
    );
    let credit_note_summary = summarise(
        credit_notes
            .iter()
            .map(|c| (c.tax_breakup.as_deref(), c.amount_minor)),
        true,
    );
    GstArchiveV1 {
        schema: GST_ARCHIVE_SCHEMA,
        fy: ctx.fy,
        branch_id: ctx.branch_id,
        branch_code: ctx.branch_code,
        currency: ctx.currency,
        built_at: ctx.built_at,
        tax_mode: ctx.tax_mode,
        tax_label: ctx.tax_label,
        invoices,
        credit_notes,
        invoice_summary,
        credit_note_summary,
        orphaned_invoices: orphaned
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

/// The archive as `zstd(JSON)` and its SHA-256.
pub fn encode_archive(archive: &GstArchiveV1) -> Result<(Vec<u8>, String), AppError> {
    let json = serde_json::to_vec(archive).map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            format!("could not encode the archive: {e}"),
        )
    })?;
    let bytes = zstd::bulk::compress(&json, ZSTD_LEVEL).map_err(|e| {
        AppError::new(
            ErrorCode::Internal,
            format!("could not compress the archive: {e}"),
        )
    })?;
    let sha256 = hex(&Sha256::digest(&bytes));
    Ok((bytes, sha256))
}

/// Reads an archive back (tests, and the day a restore wants to look inside one).
pub fn decode_archive(bytes: &[u8]) -> Result<GstArchiveV1, AppError> {
    let json = zstd::stream::decode_all(bytes)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("not a zstd archive: {e}")))?;
    serde_json::from_slice(&json)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("not a GST archive: {e}")))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn tax_settings(store: &Store) -> Result<TaxSettings, AppError> {
    Ok(store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_TAX)
        .and_then(|s| TaxSettings::parse(&s.value))
        .or_else(|| TaxSettings::parse(DEFAULT_TAX))
        .unwrap_or_else(|| TaxSettings {
            country: "IN".into(),
            gst: false,
            gstin: String::new(),
            inclusive: true,
            product_bp: 0,
            play_bp: 0,
            sac: String::new(),
            label: "Tax".into(),
            rate_bp: 0,
        }))
}

/// The archives due now, and the latest published rollup day they were worked out for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GstArchiveDue {
    /// The latest day with a published rollup; `None` before the branch exists or any rollup.
    pub day: Option<String>,
    /// The financial years to archive, oldest first. Left empty, without looking, when `day` is the
    /// day the caller already checked.
    pub years: Vec<String>,
}

/// The financial years whose archive is due (oldest first): those with a numbered invoice or a
/// credit note that are earlier than the year of the latest published rollup day, and have no
/// `gst_archive_log` row or whose rows changed since that row's file was built. A day equal to
/// `checked` is not looked at again. Blocking (SQLite); writes nothing.
pub fn due_years(store: &Store, checked: Option<&str>) -> Result<GstArchiveDue, AppError> {
    let none = GstArchiveDue::default();
    let Some(branch) = store.branch_locale()? else {
        return Ok(none);
    };
    let Some(day) = store.read_with(last_rollup_date)? else {
        return Ok(none);
    };
    if checked == Some(day.as_str()) {
        return Ok(GstArchiveDue {
            day: Some(day),
            years: Vec::new(),
        });
    }
    let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
        return Ok(none);
    };
    let rows = locale_rows(&branch, &store.settings()?);
    let current = fy_of(date, effective_locale(&rows, date).fy_start_month);
    let years = store.read_with(|conn| {
        let mut due = Vec::new();
        for (fy, now) in gst_fingerprints(conn)? {
            if fy >= current || !valid_fy(&fy) {
                continue;
            }
            let stale = find_gst_archive_log(conn, &fy)?.is_none_or(|log| log.is_stale(&now));
            if stale {
                due.push(fy);
            }
        }
        Ok(due)
    })?;
    Ok(GstArchiveDue {
        day: Some(day),
        years,
    })
}

/// Builds and compresses the archive of `fy` as of `built_at`; `None` before the branch exists.
/// The orphaned numbers are those found in databases under `orphaned/` that this database does
/// not hold, unioned with the ones already logged. Blocking (SQLite and disk); writes nothing.
pub fn build_file(
    store: &Store,
    fy: &str,
    built_at: i64,
) -> Result<Option<GstArchiveFile>, AppError> {
    if !valid_fy(fy) {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "fy",
            "fy must be a financial year such as FY2026-27",
        ));
    }
    let (Some(branch), Some(locale)) = (store.branch_summary()?, store.branch_locale()?) else {
        return Ok(None);
    };
    let tax = tax_settings(store)?;
    let db_path = store.path().to_path_buf();
    let (rows, logged, scan) = store.read_with(|conn| {
        let rows = load_gst_archive_rows(conn, fy)?;
        let logged = find_gst_archive_log(conn, fy)?.map(|l| l.orphaned);
        let scan = scan_orphaned_invoices(&db_path, fy, conn);
        Ok((rows, logged, scan))
    })?;
    let mut orphaned: BTreeSet<String> = logged.unwrap_or_default().into_iter().collect();
    orphaned.extend(scan.numbers);
    let tax_label = match tax.label.trim() {
        "" => "Tax".to_owned(),
        l => l.to_owned(),
    };
    let archive = build_archive(
        ArchiveContext {
            fy: fy.to_owned(),
            branch_id: branch.branch_id,
            branch_code: branch.branch_code,
            currency: locale.currency,
            built_at,
            tax_mode: tax.tax_mode(),
            tax_label,
        },
        &rows,
        orphaned,
    );
    let (bytes, sha256) = encode_archive(&archive)?;
    Ok(Some(GstArchiveFile {
        fy: fy.to_owned(),
        bytes,
        sha256,
        invoices: i64::try_from(archive.invoices.len()).unwrap_or(i64::MAX),
        credit_notes: i64::try_from(archive.credit_notes.len()).unwrap_or(i64::MAX),
        orphaned: archive.orphaned_invoices,
        built_at,
        unreadable_orphans: scan.unreadable,
    }))
}

/// The years archived, newest first (System -> Backup).
pub fn list(store: &Store) -> Result<GstArchiveList, AppError> {
    let rows = store.read_with(list_gst_archive_logs)?;
    Ok(GstArchiveList {
        archives: rows
            .into_iter()
            .map(|r| GstArchiveEntry {
                label: gst_archive_year_label(&r.fy),
                fy: r.fy,
                invoices: r.invoices,
                credit_notes: r.credit_notes,
                orphaned: i64::try_from(r.orphaned.len()).unwrap_or(i64::MAX),
                bytes: r.bytes,
                built_at: r.built_at,
            })
            .collect(),
    })
}

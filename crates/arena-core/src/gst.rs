//! The GSTR-1 export (Story 5.29): a month of stored invoices and credit notes folded into the
//! GST Offline Tool's CSV sections (B2B, B2CS, CDNR, HSN summary, Documents issued) and a stored
//! ZIP of them. Pure: it reads the rows `arena-store` loaded and never recomputes tax from today's
//! rates, so what the accountant files is what the Hub invoiced. The tool's B2B, B2CS and CDNR
//! templates carry rate and taxable value only; the CGST and SGST that must reconcile are in
//! `hsn.csv` and in `summary.csv`.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, ErrorCode, GST_B2B, GST_B2CS, GST_CDNR, GST_HSN, GstMonth, GstSection, TaxMode,
    TaxSettings,
};
use arena_store::{GstCreditRow, GstInvoiceRow, GstLineRow, GstRows};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

use crate::invoice::tax::RateTax;

/// The CSV files of a month, by section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstCsv {
    pub b2b: String,
    pub b2cs: String,
    pub cdnr: String,
    pub hsn: String,
    pub docs: String,
    pub summary: String,
}

/// A month's return: what the screen shows and the files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstReturn {
    pub month: GstMonth,
    pub csv: GstCsv,
}

const B2B_HEAD: [&str; 13] = [
    "GSTIN/UIN of Recipient",
    "Receiver Name",
    "Invoice Number",
    "Invoice date",
    "Invoice Value",
    "Place Of Supply",
    "Reverse Charge",
    "Applicable % of Tax Rate",
    "Invoice Type",
    "E-Commerce GSTIN",
    "Rate",
    "Taxable Value",
    "Cess Amount",
];
const B2CS_HEAD: [&str; 7] = [
    "Type",
    "Place Of Supply",
    "Applicable % of Tax Rate",
    "Rate",
    "Taxable Value",
    "Cess Amount",
    "E-Commerce GSTIN",
];
const CDNR_HEAD: [&str; 12] = [
    "GSTIN/UIN of Recipient",
    "Receiver Name",
    "Note Number",
    "Note Date",
    "Note Type",
    "Place Of Supply",
    "Reverse Charge",
    "Note Value",
    "Applicable % of Tax Rate",
    "Rate",
    "Taxable Value",
    "Cess Amount",
];
const HSN_HEAD: [&str; 11] = [
    "HSN",
    "Description",
    "UQC",
    "Total Quantity",
    "Total Value",
    "Rate",
    "Taxable Value",
    "Integrated Tax Amount",
    "Central Tax Amount",
    "State/UT Tax Amount",
    "Cess Amount",
];
const DOCS_HEAD: [&str; 5] = [
    "Nature of Document",
    "Sr. No. From",
    "Sr. No. To",
    "Total Number",
    "Cancelled",
];
const SUMMARY_HEAD: [&str; 6] = [
    "Section",
    "Documents",
    "Taxable Value",
    "Central Tax",
    "State/UT Tax",
    "Integrated Tax",
];

/// The GST state and union territory codes, as the return prints them (`29-Karnataka`).
const STATES: [(&str, &str); 38] = [
    ("01", "Jammu & Kashmir"),
    ("02", "Himachal Pradesh"),
    ("03", "Punjab"),
    ("04", "Chandigarh"),
    ("05", "Uttarakhand"),
    ("06", "Haryana"),
    ("07", "Delhi"),
    ("08", "Rajasthan"),
    ("09", "Uttar Pradesh"),
    ("10", "Bihar"),
    ("11", "Sikkim"),
    ("12", "Arunachal Pradesh"),
    ("13", "Nagaland"),
    ("14", "Manipur"),
    ("15", "Mizoram"),
    ("16", "Tripura"),
    ("17", "Meghalaya"),
    ("18", "Assam"),
    ("19", "West Bengal"),
    ("20", "Jharkhand"),
    ("21", "Odisha"),
    ("22", "Chhattisgarh"),
    ("23", "Madhya Pradesh"),
    ("24", "Gujarat"),
    ("26", "Dadra & Nagar Haveli and Daman & Diu"),
    ("27", "Maharashtra"),
    ("29", "Karnataka"),
    ("30", "Goa"),
    ("31", "Lakshadweep"),
    ("32", "Kerala"),
    ("33", "Tamil Nadu"),
    ("34", "Puducherry"),
    ("35", "Andaman & Nicobar Islands"),
    ("36", "Telangana"),
    ("37", "Andhra Pradesh"),
    ("38", "Ladakh"),
    ("97", "Other Territory"),
    ("99", "Centre Jurisdiction"),
];

/// The business days `YYYY-MM-01` and the month's last day for `month` (`YYYY-MM`).
pub fn month_range(month: &str) -> Result<(String, String), AppError> {
    let bad = || AppError::with_field(ErrorCode::ValidationFailed, "month", "Pick a valid month.");
    let (year, mon) = month.split_once('-').ok_or_else(bad)?;
    if year.len() != 4 || mon.len() != 2 {
        return Err(bad());
    }
    let first = NaiveDate::from_ymd_opt(
        year.parse().map_err(|_| bad())?,
        mon.parse().map_err(|_| bad())?,
        1,
    )
    .ok_or_else(bad)?;
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .ok_or_else(bad)?;
    let last = next.pred_opt().ok_or_else(bad)?;
    Ok((first.to_string(), last.to_string()))
}

/// `29-Karnataka` for a GSTIN (or bare state code); the code alone when it is not on the list.
pub fn place_of_supply(gstin: &str) -> String {
    let code = gstin.get(..2).unwrap_or("");
    match STATES.iter().find(|(c, _)| *c == code) {
        Some((c, name)) => format!("{c}-{name}"),
        None => code.to_owned(),
    }
}

fn money(minor: i64) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// A rate in basis points as the return prints it: `1800` is `18`, `250` is `2.5`.
fn rate_text(bp: i64) -> String {
    let text = format!("{}.{:02}", bp / 100, bp % 100);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `2026-10-05` as `05-Oct-2026`.
fn date_text(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d.format("%d-%b-%Y").to_string())
        .unwrap_or_else(|_| day.to_owned())
}

/// A text cell that a spreadsheet cannot take for a formula.
fn text(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_owned()
    }
}

fn cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn csv(head: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = head.iter().map(|h| cell(h)).collect::<Vec<_>>().join(",");
    out.push_str("\r\n");
    for row in rows {
        out.push_str(&row.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// One tax rate of a document: signed, so a credit note's values are negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rate {
    rate_bp: i64,
    taxable: i64,
    central: i64,
    state: i64,
}

/// An invoice or a credit note, ready to fold.
struct Doc<'a> {
    number: &'a str,
    date: &'a str,
    /// The invoice total, or the credit note's refund total (positive either way).
    value: i64,
    buyer: Option<(&'a str, &'a str)>,
    rates: Vec<Rate>,
    credit: bool,
    order_id: &'a str,
    /// The Hub stored no tax breakup: listed at 0% with the total as taxable value.
    untaxed: bool,
    /// The refunded lines of a credit note.
    credit_lines: &'a str,
}

impl Doc<'_> {
    fn sign(&self) -> i64 {
        if self.credit { -1 } else { 1 }
    }
}

fn rates_of(breakup: Option<&str>, total: i64, credit: bool) -> (Vec<Rate>, bool) {
    let parsed = breakup
        .and_then(|j| serde_json::from_str::<Vec<RateTax>>(j).ok())
        .filter(|v| !v.is_empty());
    match parsed {
        Some(taxes) => {
            // A credit note stores its breakup negative already; make that certain.
            let norm = |v: i64| if credit { -v.abs() } else { v };
            let rates = taxes
                .into_iter()
                .map(|t| {
                    let tax = norm(t.tax_minor);
                    let central = t.cgst_minor.map_or_else(|| half_up(tax), norm);
                    Rate {
                        rate_bp: i64::from(t.rate_bp),
                        taxable: norm(t.taxable_minor),
                        central,
                        state: t.sgst_minor.map_or(tax - central, norm),
                    }
                })
                .collect();
            (rates, false)
        }
        None => (
            vec![Rate {
                rate_bp: 0,
                taxable: if credit { -total } else { total },
                central: 0,
                state: 0,
            }],
            true,
        ),
    }
}

/// Half of `v` rounded away from zero (the CGST of an odd tax).
fn half_up(v: i64) -> i64 {
    v.signum() * ((v.abs() + 1) / 2)
}

fn state_code(gstin: &str) -> &str {
    gstin.get(..2).unwrap_or("")
}

/// Shares `total` over `weights` by largest remainder (ties to the earlier item), so the parts add
/// up to `total` exactly; equal shares when every weight is zero.
fn allocate(total: i64, weights: &[i64]) -> Vec<i64> {
    if weights.is_empty() {
        return Vec::new();
    }
    let sign = total.signum();
    let mag = i128::from(total.unsigned_abs());
    let mut w: Vec<i128> = weights
        .iter()
        .map(|w| i128::from(w.unsigned_abs()))
        .collect();
    let mut sum: i128 = w.iter().sum();
    if sum == 0 {
        w = vec![1; weights.len()];
        sum = i128::try_from(weights.len()).unwrap_or(1);
    }
    let mut parts: Vec<i128> = w.iter().map(|w| mag * w / sum).collect();
    let mut order: Vec<usize> = (0..w.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(mag * w[i] % sum), i));
    let mut left = mag - parts.iter().sum::<i128>();
    for i in order {
        if left == 0 {
            break;
        }
        parts[i] += 1;
        left -= 1;
    }
    parts
        .into_iter()
        .map(|p| i64::try_from(p).unwrap_or(0) * sign)
        .collect()
}

/// A line a document's tax is shared over.
struct Item {
    rate_bp: i64,
    hsn: String,
    desc: String,
    uqc: &'static str,
    qty: i64,
    weight: i64,
}

#[derive(Deserialize)]
struct CreditLine {
    line_id: String,
    label: String,
    qty: i64,
    amount_minor: i64,
    #[serde(default)]
    hsn: Option<String>,
}

const UQC_NOS: &str = "NOS-NUMBERS";
const UQC_OTHER: &str = "OTH-OTHERS";

fn line_item(l: &GstLineRow, sac: &str, untaxed: bool) -> Item {
    let product = l.kind == "product";
    Item {
        rate_bp: if untaxed {
            0
        } else {
            l.tax_rate_bp.unwrap_or(0)
        },
        hsn: if product {
            l.hsn.clone().unwrap_or_default()
        } else {
            sac.to_owned()
        },
        desc: match l.kind.as_str() {
            "product" => l.label.clone(),
            "plan" => "Time packs and passes".to_owned(),
            _ => "Play time".to_owned(),
        },
        uqc: if product { UQC_NOS } else { UQC_OTHER },
        qty: l.qty,
        weight: l.amount_minor,
    }
}

fn doc_items(doc: &Doc<'_>, lines: &BTreeMap<&str, Vec<&GstLineRow>>, sac: &str) -> Vec<Item> {
    let order_lines = lines.get(doc.order_id).map_or(&[][..], Vec::as_slice);
    if !doc.credit {
        return order_lines
            .iter()
            .map(|l| line_item(l, sac, doc.untaxed))
            .collect();
    }
    let refunded: Vec<CreditLine> = serde_json::from_str(doc.credit_lines).unwrap_or_default();
    refunded
        .into_iter()
        .map(|c| {
            let origin = order_lines.iter().find(|l| l.id == c.line_id);
            let mut item = origin.map_or_else(
                || Item {
                    rate_bp: 0,
                    hsn: c.hsn.clone().unwrap_or_default(),
                    desc: c.label.clone(),
                    uqc: UQC_OTHER,
                    qty: 0,
                    weight: 0,
                },
                |l| line_item(l, sac, doc.untaxed),
            );
            if let Some(hsn) = c.hsn.filter(|h| !h.is_empty()) {
                item.hsn = hsn;
            }
            item.qty = c.qty;
            item.weight = c.amount_minor;
            item
        })
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
struct Agg {
    documents: i64,
    taxable: i64,
    central: i64,
    state: i64,
    integrated: i64,
}

impl Agg {
    fn add(&mut self, taxable: i64, central: i64, state: i64, interstate: bool) {
        self.taxable += taxable;
        if interstate {
            self.integrated += central + state;
        } else {
            self.central += central;
            self.state += state;
        }
    }

    fn row(&self, label: &str) -> Vec<String> {
        vec![
            label.to_owned(),
            self.documents.to_string(),
            money(self.taxable),
            money(self.central),
            money(self.state),
            money(self.integrated),
        ]
    }

    fn section(&self, key: &str) -> GstSection {
        GstSection {
            key: key.to_owned(),
            documents: self.documents,
            taxable_minor: self.taxable,
            central_minor: self.central,
            state_minor: self.state,
            integrated_minor: self.integrated,
        }
    }
}

#[derive(Default)]
struct HsnRow {
    desc: String,
    qty: i64,
    value: i64,
    agg: Agg,
}

/// `(fy, epoch counter, segment, sequence)` of an invoice number
/// (`<code>/<FY>/E<counter><tag>/<seq>`).
fn parse_number(number: &str) -> Option<(String, i64, String, i64)> {
    let mut parts = number.rsplitn(4, '/');
    let seq = parts.next()?.parse().ok()?;
    let segment = parts.next()?;
    let fy = parts.next()?;
    parts.next()?;
    let counter = segment
        .trim_start_matches('E')
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    Some((fy.to_owned(), counter, segment.to_owned(), seq))
}

type SegmentKey = (String, i64, String);

/// The `(sequence, number)` pairs of one nature (invoices or credit notes) in a segment.
type Numbers<'a> = Vec<(i64, &'a str)>;

fn docs_issued(invoices: &[&Doc<'_>], credits: &[&Doc<'_>]) -> Vec<Vec<String>> {
    // (seq, number) of each nature, per segment.
    let mut by_segment: BTreeMap<SegmentKey, (Numbers<'_>, Numbers<'_>)> = BTreeMap::new();
    for (docs, credit) in [(invoices, false), (credits, true)] {
        for d in docs {
            if let Some((fy, counter, segment, seq)) = parse_number(d.number) {
                let entry = by_segment.entry((fy, counter, segment)).or_default();
                let list = if credit { &mut entry.1 } else { &mut entry.0 };
                list.push((seq, d.number));
            }
        }
    }
    let mut rows = Vec::new();
    for (_, (mut inv, mut cn)) in by_segment {
        inv.sort_unstable();
        cn.sort_unstable();
        let all: Vec<i64> = inv.iter().chain(cn.iter()).map(|(s, _)| *s).collect();
        let (lo, hi) = (
            all.iter().copied().min().unwrap_or(0),
            all.iter().copied().max().unwrap_or(0),
        );
        let span = hi - lo + 1;
        let cancelled = (span - i64::try_from(all.len()).unwrap_or(0)).max(0);
        let mut put = |nature: &str, list: &[(i64, &str)], cancelled: i64| {
            if let (Some(first), Some(last)) = (list.first(), list.last()) {
                rows.push(vec![
                    nature.to_owned(),
                    text(first.1),
                    text(last.1),
                    list.len().to_string(),
                    cancelled.to_string(),
                ]);
            }
        };
        // Numbers of the month's range that no document holds belong to neither nature; they are
        // counted once, on the invoices row (or the credit notes row of a segment with no invoice).
        put(
            "Invoices for outward supply",
            &inv,
            if inv.is_empty() { 0 } else { cancelled },
        );
        put(
            "Credit Note",
            &cn,
            if inv.is_empty() { cancelled } else { 0 },
        );
    }
    rows
}

/// Folds a month's stored rows into the return.
pub fn build(rows: &GstRows, tax: &TaxSettings, month: &str) -> GstReturn {
    let supplier = tax.gstin.as_str();
    let supplier_state = state_code(supplier);
    let sac = tax.sac.as_str();

    let mut lines: BTreeMap<&str, Vec<&GstLineRow>> = BTreeMap::new();
    for l in &rows.lines {
        lines.entry(l.order_id.as_str()).or_default().push(l);
    }

    let invoice_docs: Vec<Doc<'_>> = rows.invoices.iter().map(invoice_doc).collect();
    let credit_docs: Vec<Doc<'_>> = rows.credits.iter().map(credit_doc).collect();
    let interstate = |d: &Doc<'_>| {
        d.buyer
            .is_some_and(|(gstin, _)| state_code(gstin) != supplier_state)
    };

    let (mut b2b_rows, mut cdnr_rows) = (Vec::new(), Vec::new());
    let (mut b2b, mut cdnr, mut hsn_total, mut b2cs_total) = (
        Agg::default(),
        Agg::default(),
        Agg::default(),
        Agg::default(),
    );
    let mut b2cs: BTreeMap<i64, Agg> = BTreeMap::new();
    let mut hsn: BTreeMap<(String, i64, &'static str), HsnRow> = BTreeMap::new();

    for d in invoice_docs.iter().chain(credit_docs.iter()) {
        let inter = interstate(d);
        hsn_total.documents += 1;
        for r in &d.rates {
            hsn_total.add(r.taxable, r.central, r.state, inter);
        }
        match d.buyer {
            Some((gstin, name)) => {
                let place = place_of_supply(gstin);
                if d.credit {
                    cdnr.documents += 1;
                } else {
                    b2b.documents += 1;
                }
                for r in &d.rates {
                    if d.credit {
                        cdnr.add(-r.taxable, -r.central, -r.state, inter);
                        cdnr_rows.push((
                            d.date,
                            d.number,
                            vec![
                                gstin.to_owned(),
                                text(name),
                                text(d.number),
                                date_text(d.date),
                                "C".to_owned(),
                                place.clone(),
                                "N".to_owned(),
                                money(d.value),
                                String::new(),
                                rate_text(r.rate_bp),
                                money(-r.taxable),
                                money(0),
                            ],
                        ));
                    } else {
                        b2b.add(r.taxable, r.central, r.state, inter);
                        b2b_rows.push((
                            d.date,
                            d.number,
                            vec![
                                gstin.to_owned(),
                                text(name),
                                text(d.number),
                                date_text(d.date),
                                money(d.value),
                                place.clone(),
                                "N".to_owned(),
                                String::new(),
                                "Regular".to_owned(),
                                String::new(),
                                rate_text(r.rate_bp),
                                money(r.taxable),
                                money(0),
                            ],
                        ));
                    }
                }
            }
            None => {
                b2cs_total.documents += 1;
                for r in &d.rates {
                    b2cs.entry(r.rate_bp)
                        .or_default()
                        .add(r.taxable, r.central, r.state, false);
                    b2cs_total.add(r.taxable, r.central, r.state, false);
                }
            }
        }
        share_over_hsn(d, &lines, sac, inter, &mut hsn);
    }

    b2b_rows.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    cdnr_rows.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let b2b_rows: Vec<_> = b2b_rows.into_iter().map(|r| r.2).collect();
    let cdnr_rows: Vec<_> = cdnr_rows.into_iter().map(|r| r.2).collect();

    let place = place_of_supply(supplier);
    let b2cs_rows: Vec<Vec<String>> = b2cs
        .iter()
        .filter(|(_, a)| a.taxable != 0 || a.central != 0 || a.state != 0)
        .map(|(rate, a)| {
            vec![
                "OE".to_owned(),
                place.clone(),
                String::new(),
                rate_text(*rate),
                money(a.taxable),
                money(0),
                String::new(),
            ]
        })
        .collect();

    let hsn_rows: Vec<Vec<String>> = hsn
        .iter()
        .map(|((code, rate, uqc), h)| {
            vec![
                code.clone(),
                text(&h.desc),
                (*uqc).to_owned(),
                h.qty.to_string(),
                money(h.value),
                rate_text(*rate),
                money(h.agg.taxable),
                money(h.agg.integrated),
                money(h.agg.central),
                money(h.agg.state),
                money(0),
            ]
        })
        .collect();

    let invoice_refs: Vec<&Doc<'_>> = invoice_docs.iter().collect();
    let credit_refs: Vec<&Doc<'_>> = credit_docs.iter().collect();
    let docs_rows = docs_issued(&invoice_refs, &credit_refs);

    let mut net = Agg {
        documents: b2b.documents + b2cs_total.documents + cdnr.documents,
        ..Agg::default()
    };
    net.taxable = b2b.taxable + b2cs_total.taxable - cdnr.taxable;
    net.central = b2b.central + b2cs_total.central - cdnr.central;
    net.state = b2b.state + b2cs_total.state - cdnr.state;
    net.integrated = b2b.integrated + b2cs_total.integrated - cdnr.integrated;
    let summary_rows = vec![
        b2b.row("B2B invoices"),
        b2cs_total.row("B2C (B2CS), net of credit notes"),
        cdnr.row("Credit notes to registered buyers (CDNR)"),
        net.row("Net: B2B + B2CS - CDNR"),
        hsn_total.row("HSN summary, net"),
    ];

    let untaxed = invoice_docs.iter().filter(|d| d.untaxed).count();
    let month_view = GstMonth {
        month: month.to_owned(),
        sections: vec![
            b2b.section(GST_B2B),
            b2cs_total.section(GST_B2CS),
            cdnr.section(GST_CDNR),
            hsn_total.section(GST_HSN),
        ],
        invoices: i64::try_from(invoice_docs.len()).unwrap_or(i64::MAX),
        credit_notes: i64::try_from(credit_docs.len()).unwrap_or(i64::MAX),
        untaxed: i64::try_from(untaxed).unwrap_or(i64::MAX),
        unnumbered: rows.unnumbered,
        empty: invoice_docs.is_empty() && credit_docs.is_empty(),
    };
    GstReturn {
        month: month_view,
        csv: GstCsv {
            b2b: csv(&B2B_HEAD, &b2b_rows),
            b2cs: csv(&B2CS_HEAD, &b2cs_rows),
            cdnr: csv(&CDNR_HEAD, &cdnr_rows),
            hsn: csv(&HSN_HEAD, &hsn_rows),
            docs: csv(&DOCS_HEAD, &docs_rows),
            summary: csv(&SUMMARY_HEAD, &summary_rows),
        },
    }
}

fn invoice_doc(r: &GstInvoiceRow) -> Doc<'_> {
    let (rates, untaxed) = rates_of(r.tax_breakup.as_deref(), r.total_minor, false);
    Doc {
        number: &r.invoice_no,
        date: &r.business_day,
        value: r.total_minor,
        buyer: r
            .buyer_gstin
            .as_deref()
            .map(|g| (g, r.buyer_name.as_deref().unwrap_or(""))),
        rates,
        credit: false,
        order_id: &r.order_id,
        untaxed,
        credit_lines: "",
    }
}

fn credit_doc(r: &GstCreditRow) -> Doc<'_> {
    let (rates, untaxed) = rates_of(r.tax_breakup.as_deref(), r.amount_minor, true);
    Doc {
        number: &r.credit_note_no,
        date: &r.business_day,
        value: r.amount_minor,
        buyer: r
            .buyer_gstin
            .as_deref()
            .map(|g| (g, r.buyer_name.as_deref().unwrap_or(""))),
        rates,
        credit: true,
        order_id: &r.order_id,
        untaxed,
        credit_lines: &r.lines,
    }
}

/// Shares each rate of `d` over its lines' HSN codes (taxable value, CGST and SGST each by largest
/// remainder, so the HSN rows add up to the document's stored breakup exactly).
fn share_over_hsn(
    d: &Doc<'_>,
    lines: &BTreeMap<&str, Vec<&GstLineRow>>,
    sac: &str,
    interstate: bool,
    hsn: &mut BTreeMap<(String, i64, &'static str), HsnRow>,
) {
    let items = doc_items(d, lines, sac);
    for r in &d.rates {
        let mut of_rate: Vec<&Item> = items.iter().filter(|i| i.rate_bp == r.rate_bp).collect();
        let other = Item {
            rate_bp: r.rate_bp,
            hsn: String::new(),
            desc: "Other".to_owned(),
            uqc: UQC_OTHER,
            qty: 0,
            weight: 0,
        };
        if of_rate.is_empty() {
            of_rate.push(&other);
        }
        let weights: Vec<i64> = of_rate.iter().map(|i| i.weight).collect();
        let taxable = allocate(r.taxable, &weights);
        let central = allocate(r.central, &weights);
        let state = allocate(r.state, &weights);
        for (n, item) in of_rate.iter().enumerate() {
            let row = hsn
                .entry((item.hsn.clone(), r.rate_bp, item.uqc))
                .or_default();
            if row.desc.is_empty() {
                row.desc.clone_from(&item.desc);
            }
            row.qty += item.qty * d.sign();
            row.value += taxable[n] + central[n] + state[n];
            row.agg.add(taxable[n], central[n], state[n], interstate);
        }
    }
}

/// Whether `tax` is a GST registration the return applies to.
pub fn gst_applies(tax: &TaxSettings) -> bool {
    tax.tax_mode() == TaxMode::GstIn
}

fn crc32(data: &[u8]) -> u32 {
    const fn table() -> [u32; 256] {
        let mut t = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut c = n as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[n] = c;
            n += 1;
        }
        t
    }
    const TABLE: [u32; 256] = table();
    !data
        .iter()
        .fold(!0u32, |c, b| TABLE[usize::from((c as u8) ^ b)] ^ (c >> 8))
}

/// A ZIP of `files` with every entry stored (no compression), dated 1980-01-01 so the same month
/// always gives the same bytes.
pub fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    const DOS_DATE: u16 = (1 << 5) | 1;
    const UTF8: u16 = 0x0800;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let crc = crc32(data);
        let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&UTF8.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&UTF8.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0u8; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let cd_size = u32::try_from(central.len()).unwrap_or(u32::MAX);
    let count = u16::try_from(files.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

impl GstCsv {
    /// The month's ZIP: `b2b.csv`, `b2cs.csv`, `cdnr.csv`, `hsn.csv`, `docs.csv`, `summary.csv`.
    pub fn to_zip(&self) -> Vec<u8> {
        zip(&[
            ("b2b.csv", self.b2b.as_bytes()),
            ("b2cs.csv", self.b2cs.as_bytes()),
            ("cdnr.csv", self.cdnr.as_bytes()),
            ("hsn.csv", self.hsn.as_bytes()),
            ("docs.csv", self.docs.as_bytes()),
            ("summary.csv", self.summary.as_bytes()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::tax::{Bill, Line, compute};

    const SUPPLIER: &str = "29ABCDE1234F1Z5";
    const IN_STATE: &str = "29AABCT1332L1ZD";
    const OTHER_STATE: &str = "27AAPFU0939F1ZV";
    const BUYER_NAME: &str = "Acme, \"Traders\" Pvt Ltd";

    fn tax() -> TaxSettings {
        TaxSettings {
            country: "IN".to_owned(),
            gst: true,
            gstin: SUPPLIER.to_owned(),
            inclusive: true,
            product_bp: 500,
            play_bp: 1800,
            sac: "9996".to_owned(),
            label: "GST".to_owned(),
            rate_bp: 0,
        }
    }

    fn bill(lines: &[(u16, i64)]) -> Bill {
        let lines: Vec<Line> = lines
            .iter()
            .map(|(rate_bp, amount_minor)| Line {
                rate_bp: *rate_bp,
                amount_minor: *amount_minor,
            })
            .collect();
        compute(&lines, true, true).unwrap()
    }

    fn breakup(lines: &[(u16, i64)]) -> String {
        serde_json::to_string(&bill(lines).breakup).unwrap()
    }

    fn invoice(
        order: &str,
        no: &str,
        day: &str,
        total: i64,
        lines: &[(u16, i64)],
        buyer: Option<&str>,
    ) -> GstInvoiceRow {
        GstInvoiceRow {
            order_id: order.to_owned(),
            invoice_no: no.to_owned(),
            business_day: day.to_owned(),
            total_minor: total,
            tax_breakup: (!lines.is_empty()).then(|| breakup(lines)),
            buyer_gstin: buyer.map(str::to_owned),
            buyer_name: buyer.map(|_| BUYER_NAME.to_owned()),
        }
    }

    fn line(id: &str, order: &str, hsn: &str, label: &str, rate: i64, amount: i64) -> GstLineRow {
        GstLineRow {
            id: id.to_owned(),
            order_id: order.to_owned(),
            kind: "product".to_owned(),
            label: label.to_owned(),
            qty: 1,
            amount_minor: amount,
            tax_rate_bp: Some(rate),
            hsn: Some(hsn.to_owned()),
        }
    }

    /// Two epochs, B2B (in-state and inter-state) and B2C invoices at 5% and 18%, a session bill
    /// with no stored breakup, and a credit note to a consumer and one to a registered buyer.
    fn fixture() -> GstRows {
        GstRows {
            invoices: vec![
                invoice(
                    "o1",
                    "DEV/26-27/E1k/000001",
                    "2026-10-02",
                    42_800,
                    &[(500, 31_000), (1800, 11_800)],
                    None,
                ),
                invoice(
                    "o2",
                    "DEV/26-27/E1k/000002",
                    "2026-10-03",
                    10_500,
                    &[(500, 10_500)],
                    Some(OTHER_STATE),
                ),
                invoice("o3", "DEV/26-27/E1k/000003", "2026-10-04", 12_000, &[], None),
                invoice(
                    "o4",
                    "DEV/26-27/E2k/000001",
                    "2026-10-20",
                    23_600,
                    &[(1800, 23_600)],
                    Some(IN_STATE),
                ),
            ],
            credits: vec![
                GstCreditRow {
                    order_id: "o1".to_owned(),
                    credit_note_no: "DEV/26-27/E1k/000004".to_owned(),
                    invoice_no: "DEV/26-27/E1k/000001".to_owned(),
                    business_day: "2026-10-05".to_owned(),
                    amount_minor: 10_500,
                    tax_breakup: Some(breakup(&[(500, -10_500)])),
                    lines: r#"[{"line_id":"l1","label":"Snack","qty":1,"amount_minor":10500,"return_to_stock":true,"hsn":"2106"}]"#.to_owned(),
                    buyer_gstin: None,
                    buyer_name: None,
                },
                GstCreditRow {
                    order_id: "o4".to_owned(),
                    credit_note_no: "DEV/26-27/E2k/000002".to_owned(),
                    invoice_no: "DEV/26-27/E2k/000001".to_owned(),
                    business_day: "2026-10-21".to_owned(),
                    amount_minor: 11_800,
                    tax_breakup: Some(breakup(&[(1800, -11_800)])),
                    lines: r#"[{"line_id":"l5","label":"Cola","qty":1,"amount_minor":11800,"return_to_stock":false}]"#.to_owned(),
                    buyer_gstin: Some(IN_STATE.to_owned()),
                    buyer_name: Some(BUYER_NAME.to_owned()),
                },
            ],
            lines: vec![
                line("l1", "o1", "2106", "Snack", 500, 31_000),
                line("l2", "o1", "2202", "Cola", 1800, 11_800),
                line("l3", "o2", "2106", "Snack", 500, 10_500),
                GstLineRow {
                    id: "l4".to_owned(),
                    order_id: "o3".to_owned(),
                    kind: "time".to_owned(),
                    label: "PC 1".to_owned(),
                    qty: 1,
                    amount_minor: 12_000,
                    tax_rate_bp: None,
                    hsn: None,
                },
                line("l5", "o4", "2202", "Cola", 1800, 23_600),
            ],
            unnumbered: 2,
        }
    }

    fn section<'a>(m: &'a GstMonth, key: &str) -> &'a GstSection {
        m.sections.iter().find(|s| s.key == key).unwrap()
    }

    fn rows_of(csv: &str) -> Vec<&str> {
        csv.split("\r\n").filter(|l| !l.is_empty()).collect()
    }

    fn cgst(b: &Bill) -> i64 {
        b.breakup.iter().map(|r| r.cgst_minor.unwrap()).sum()
    }

    fn sgst(b: &Bill) -> i64 {
        b.breakup.iter().map(|r| r.sgst_minor.unwrap()).sum()
    }

    #[test]
    fn sections_reconcile_to_the_invoices_minus_the_credit_notes() {
        let built = build(&fixture(), &tax(), "2026-10");
        let m = &built.month;
        assert_eq!(
            (m.invoices, m.credit_notes, m.untaxed, m.unnumbered),
            (4, 2, 1, 2)
        );
        assert!(!m.empty);

        // B2B: the inter-state 5% invoice (integrated) and the in-state 18% one.
        let (inv2, inv4) = (bill(&[(500, 10_500)]), bill(&[(1800, 23_600)]));
        let b2b = section(m, GST_B2B);
        assert_eq!(b2b.documents, 2);
        assert_eq!(b2b.taxable_minor, inv2.subtotal_minor + inv4.subtotal_minor);
        assert_eq!(b2b.integrated_minor, inv2.tax_minor);
        assert_eq!(b2b.central_minor, cgst(&inv4));
        assert_eq!(b2b.state_minor, sgst(&inv4));

        // CDNR: the 18% credit note to the in-state buyer, positive.
        let note2 = bill(&[(1800, 11_800)]);
        let cdnr = section(m, GST_CDNR);
        assert_eq!(cdnr.documents, 1);
        assert_eq!(cdnr.taxable_minor, note2.subtotal_minor);
        assert_eq!(cdnr.central_minor, cgst(&note2));
        assert_eq!(cdnr.state_minor, sgst(&note2));

        // B2CS nets the consumer credit note; the session bill is listed at 0% as stored.
        let inv1 = bill(&[(500, 31_000), (1800, 11_800)]);
        let note1 = bill(&[(500, 10_500)]);
        let b2cs = section(m, GST_B2CS);
        assert_eq!(b2cs.documents, 3);
        assert_eq!(
            b2cs.taxable_minor,
            inv1.subtotal_minor - note1.subtotal_minor + 12_000
        );
        assert_eq!(b2cs.central_minor, cgst(&inv1) - cgst(&note1));
        assert_eq!(b2cs.state_minor, sgst(&inv1) - sgst(&note1));

        // B2B + B2CS - CDNR is the HSN summary, to the minor unit, in every column.
        let hsn = section(m, GST_HSN);
        assert_eq!(hsn.documents, 6);
        for (a, b, c, h) in [
            (
                b2b.taxable_minor,
                b2cs.taxable_minor,
                cdnr.taxable_minor,
                hsn.taxable_minor,
            ),
            (
                b2b.central_minor,
                b2cs.central_minor,
                cdnr.central_minor,
                hsn.central_minor,
            ),
            (
                b2b.state_minor,
                b2cs.state_minor,
                cdnr.state_minor,
                hsn.state_minor,
            ),
            (
                b2b.integrated_minor,
                b2cs.integrated_minor,
                cdnr.integrated_minor,
                hsn.integrated_minor,
            ),
        ] {
            assert_eq!(a + b - c, h);
        }
    }

    #[test]
    fn the_csv_files_follow_the_template() {
        let built = build(&fixture(), &tax(), "2026-10");
        let b2b = rows_of(&built.csv.b2b);
        assert_eq!(b2b[0], B2B_HEAD.join(","));
        assert_eq!(b2b.len(), 3);
        // Inter-state buyer: place of supply is the buyer's state; the name is quoted.
        assert_eq!(
            b2b[1],
            "27AAPFU0939F1ZV,\"Acme, \"\"Traders\"\" Pvt Ltd\",DEV/26-27/E1k/000002,03-Oct-2026,105.00,27-Maharashtra,N,,Regular,,5,100.00,0.00"
        );
        assert!(b2b[2].starts_with("29AABCT1332L1ZD,"));
        assert!(b2b[2].contains(",29-Karnataka,N,,Regular,,18,200.00,0.00"));

        let b2cs = rows_of(&built.csv.b2cs);
        assert_eq!(b2cs[0], B2CS_HEAD.join(","));
        assert_eq!(b2cs[1], "OE,29-Karnataka,,0,120.00,0.00,");
        assert!(b2cs.iter().any(|l| l.starts_with("OE,29-Karnataka,,5,")));
        assert!(b2cs.iter().any(|l| l.starts_with("OE,29-Karnataka,,18,")));

        let cdnr = rows_of(&built.csv.cdnr);
        assert_eq!(cdnr.len(), 2);
        assert!(
            cdnr[1].contains(
                ",DEV/26-27/E2k/000002,21-Oct-2026,C,29-Karnataka,N,118.00,,18,100.00,0.00"
            )
        );

        let hsn = rows_of(&built.csv.hsn);
        assert_eq!(hsn[0], HSN_HEAD.join(","));
        // Play time sits under the SAC; the snack under its HSN, net of its credit note.
        assert!(
            hsn.iter()
                .any(|l| l.starts_with("9996,Play time,OTH-OTHERS,1,120.00,0,120.00,"))
        );
        assert!(
            hsn.iter()
                .any(|l| l.starts_with("2106,Snack,NOS-NUMBERS,1,"))
        );
    }

    #[test]
    fn documents_issued_lists_each_segment_with_none_missing() {
        let built = build(&fixture(), &tax(), "2026-10");
        assert_eq!(
            rows_of(&built.csv.docs),
            vec![
                DOCS_HEAD.join(","),
                "Invoices for outward supply,DEV/26-27/E1k/000001,DEV/26-27/E1k/000003,3,0"
                    .to_owned(),
                "Credit Note,DEV/26-27/E1k/000004,DEV/26-27/E1k/000004,1,0".to_owned(),
                "Invoices for outward supply,DEV/26-27/E2k/000001,DEV/26-27/E2k/000001,1,0"
                    .to_owned(),
                "Credit Note,DEV/26-27/E2k/000002,DEV/26-27/E2k/000002,1,0".to_owned(),
            ]
        );
    }

    #[test]
    fn a_gap_in_a_segment_is_reported_as_cancelled() {
        let mut rows = fixture();
        rows.invoices.remove(2); // 000003 was never issued: 1, 2 and credit note 4 remain
        let docs = build(&rows, &tax(), "2026-10").csv.docs;
        assert_eq!(
            rows_of(&docs)[1],
            "Invoices for outward supply,DEV/26-27/E1k/000001,DEV/26-27/E1k/000002,2,1"
        );
    }

    #[test]
    fn an_empty_month_has_headers_only() {
        let built = build(&GstRows::default(), &tax(), "2026-11");
        assert!(built.month.empty);
        for (csv, head) in [
            (&built.csv.b2b, B2B_HEAD.join(",")),
            (&built.csv.b2cs, B2CS_HEAD.join(",")),
            (&built.csv.cdnr, CDNR_HEAD.join(",")),
            (&built.csv.hsn, HSN_HEAD.join(",")),
            (&built.csv.docs, DOCS_HEAD.join(",")),
        ] {
            assert_eq!(rows_of(csv), vec![head]);
        }
        assert!(
            built
                .month
                .sections
                .iter()
                .all(|s| s.documents == 0 && s.taxable_minor == 0)
        );
    }

    #[test]
    fn text_cells_cannot_run_as_formulas() {
        assert_eq!(text("=1+1"), "'=1+1");
        assert_eq!(text("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(text("-2"), "'-2");
        assert_eq!(text("Acme"), "Acme");
    }

    #[test]
    fn largest_remainder_shares_add_up() {
        assert_eq!(allocate(100, &[1, 1, 1]), vec![34, 33, 33]);
        assert_eq!(allocate(-100, &[1, 1, 1]), vec![-34, -33, -33]);
        assert_eq!(allocate(7, &[0, 0]), vec![4, 3]);
        assert_eq!(allocate(0, &[5, 5]), vec![0, 0]);
        assert!(allocate(5, &[]).is_empty());
        for total in [-1_001i64, 0, 1, 999, 123_457] {
            assert_eq!(allocate(total, &[3, 0, 7, 11]).iter().sum::<i64>(), total);
        }
    }

    #[test]
    fn numbers_and_dates_print_as_the_tool_reads_them() {
        assert_eq!(money(-1_050), "-10.50");
        assert_eq!(money(5), "0.05");
        assert_eq!(rate_text(1800), "18");
        assert_eq!(rate_text(250), "2.5");
        assert_eq!(rate_text(125), "1.25");
        assert_eq!(rate_text(0), "0");
        assert_eq!(date_text("2026-03-09"), "09-Mar-2026");
        assert_eq!(place_of_supply("29ABCDE1234F1Z5"), "29-Karnataka");
        assert_eq!(place_of_supply("98XXXX"), "98");
    }

    #[test]
    fn a_month_runs_from_its_first_to_its_last_day() {
        assert_eq!(
            month_range("2026-10").unwrap(),
            ("2026-10-01".to_owned(), "2026-10-31".to_owned())
        );
        assert_eq!(month_range("2028-02").unwrap().1, "2028-02-29");
        assert_eq!(month_range("2026-12").unwrap().1, "2026-12-31");
        for bad in [
            "2026-13",
            "2026-00",
            "26-10",
            "2026-1",
            "2026/10",
            "",
            "2026-10-01",
        ] {
            assert_eq!(
                month_range(bad).unwrap_err().code,
                ErrorCode::ValidationFailed,
                "{bad}"
            );
        }
    }

    /// Reads a stored ZIP back: `(name, bytes)` of every entry, checking each CRC.
    fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16_at = |i: usize| usize::from(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let eocd = bytes.len() - 22;
        assert_eq!(u32_at(eocd), 0x0605_4b50);
        let (count, mut at) = (u16_at(eocd + 10), u32_at(eocd + 16) as usize);
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(at), 0x0201_4b50);
            let (crc, size) = (u32_at(at + 16), u32_at(at + 20) as usize);
            let (name_len, local) = (u16_at(at + 28), u32_at(at + 42) as usize);
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(u32_at(local), 0x0403_4b50);
            let start = local + 30 + u16_at(local + 26);
            let data = bytes[start..start + size].to_vec();
            assert_eq!(crc32(&data), crc, "{name}");
            out.push((name, data));
            at += 46 + name_len;
        }
        out
    }

    #[test]
    fn the_zip_holds_every_section_and_reads_back() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let built = build(&fixture(), &tax(), "2026-10");
        let zip = built.csv.to_zip();
        let files = unzip(&zip);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "b2b.csv",
                "b2cs.csv",
                "cdnr.csv",
                "hsn.csv",
                "docs.csv",
                "summary.csv"
            ]
        );
        assert_eq!(files[0].1, built.csv.b2b.as_bytes());
        assert_eq!(files[5].1, built.csv.summary.as_bytes());
        assert_eq!(
            zip,
            built.csv.to_zip(),
            "the same month gives the same bytes"
        );
        assert!(rows_of(&built.csv.summary)[4].starts_with("Net: B2B + B2CS - CDNR,6,"));
    }

    #[test]
    fn only_a_gst_registration_gets_the_return() {
        assert!(gst_applies(&tax()));
        let mut simple = tax();
        simple.gst = false;
        assert!(!gst_applies(&simple));
    }
}

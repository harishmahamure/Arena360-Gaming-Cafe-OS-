//! The `receipt` renderer (Story 5.16): a pure function from a settled sale or a payment and the
//! receipt settings to ESC/POS bytes for 58 mm (32 columns) and 80 mm (48 columns) paper.
//!
//! Nothing here reads the database, the clock or the printer. Story 5.17 builds a [`SaleReceipt`]
//! or [`PaymentReceipt`] from the committed rows and sends the bytes through the side-effect
//! outbox. Text is Font A in code page PC437, so anything outside printable ASCII prints as `?`;
//! money therefore uses an ASCII symbol (`Rs` for the rupee, the ISO code where the symbol has no
//! ASCII form).

use arena_proto::currency::currency;
use arena_proto::{NumberFormat, PaymentMethod, PaymentRow, SymbolPosition};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use super::PaperWidth;
use super::escpos::pc437_text;
use crate::invoice::tax::RateTax;
use crate::locale::format_number;

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;
const LF: u8 = 0x0A;
/// Narrowest item-name column at which 80 mm paper still uses columns; below it the item stacks.
const MIN_NAME_COLUMN: usize = 12;

/// How a branch writes money on paper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptMoney {
    pub exponent: u8,
    pub symbol: String,
    pub symbol_position: SymbolPosition,
    pub number_format: NumberFormat,
}

impl ReceiptMoney {
    /// The money format of ISO `code` with the branch's position and grouping. An unknown code
    /// prints as itself with two decimals.
    pub fn new(code: &str, symbol_position: SymbolPosition, number_format: NumberFormat) -> Self {
        let (exponent, symbol) = currency(code).map_or((2, code.to_owned()), |c| {
            (c.exponent, ascii_symbol(c.symbol, c.code))
        });
        Self {
            exponent,
            symbol,
            symbol_position,
            number_format,
        }
    }

    /// `1,23,456.78` with a leading `-` for a negative amount.
    pub fn number(&self, minor: i64) -> String {
        let n = format_number(
            minor.unsigned_abs(),
            usize::from(self.exponent),
            self.number_format,
        );
        if minor < 0 { format!("-{n}") } else { n }
    }

    /// The amount with the symbol on the branch's side: `Rs1,180.00`, `1,180 JPY`.
    pub fn with_symbol(&self, minor: i64) -> String {
        let n = self.number(minor);
        let (sign, digits) = n.strip_prefix('-').map_or(("", n.as_str()), |d| ("-", d));
        match self.symbol_position {
            SymbolPosition::After => format!("{sign}{digits} {}", self.symbol),
            SymbolPosition::Before => format!("{sign}{}{digits}", self.symbol),
        }
    }
}

fn ascii_symbol(symbol: &str, code: &str) -> String {
    if symbol == "\u{20B9}" {
        "Rs".to_owned()
    } else if symbol.is_ascii() {
        symbol.to_owned()
    } else {
        code.to_owned()
    }
}

/// What every receipt takes from Settings → Receipts and the branch locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSettings {
    pub paper: PaperWidth,
    /// `branch.name`.
    pub name: String,
    /// `receipt.address`, lines separated by `\n`.
    pub address: String,
    /// `receipt.phone`.
    pub phone: String,
    /// `receipt.footer`, lines separated by `\n`.
    pub footer: String,
    pub money: ReceiptMoney,
    /// The branch's IANA time zone, for the printed date and time.
    pub tz: String,
}

/// A play-time line of a session bill; each rate segment is its own line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLine {
    pub device: String,
    pub minutes: i64,
    /// The hourly rate in minor units.
    pub rate_minor: i64,
    pub amount_minor: i64,
    /// The SAC code printed under the line on a GST receipt.
    pub sac: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLine {
    pub name: String,
    pub qty: i64,
    pub rate_minor: i64,
    pub amount_minor: i64,
    /// The HSN code printed under the name on a GST receipt.
    pub hsn: Option<String>,
}

/// The customer of a business (B2B) invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buyer {
    pub legal_name: String,
    pub gstin: String,
}

/// The coupon a sale took (Story 5.21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coupon {
    pub code: String,
    pub amount_minor: i64,
}

/// A settled sale: "Tax Invoice" on a GST branch, "Invoice" otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleReceipt {
    /// A GST-registered Indian branch (`TaxMode::GstIn`): the title, GSTIN, HSN/SAC codes and the
    /// CGST/SGST split appear only then.
    pub gst: bool,
    pub gstin: Option<String>,
    /// The simple-tax label (`VAT`); unused on a GST branch.
    pub tax_label: String,
    /// `None` while no number is issued yet: the line is left off.
    pub invoice_no: Option<String>,
    /// The business a GST invoice is made out to (Story 5.20): printed under the title.
    pub buyer: Option<Buyer>,
    pub at_ms: i64,
    pub staff: String,
    /// The player's username; `None` prints "Guest".
    pub player: Option<String>,
    pub sessions: Vec<SessionLine>,
    pub items: Vec<ItemLine>,
    /// Printed after the items and before the tax lines.
    pub coupon: Option<Coupon>,
    /// The manual discount (Story 5.22), printed after the coupon.
    pub discount_minor: Option<i64>,
    pub subtotal_minor: i64,
    pub tax: Vec<RateTax>,
    pub total_minor: i64,
    /// A [`PaymentMethod::Credit`] row is the remainder put on the player's credit.
    pub payments: Vec<PaymentRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentKind {
    WalletTopup,
    CreditCollection,
}

/// One bill a credit collection cleared, from the Story 3.19 `bill.settled` data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearedBill {
    /// The invoice number, or the bill's date where it has none.
    pub label: String,
    pub cleared_minor: i64,
}

/// A wallet top-up or a credit collection: money received, never a sale, so no invoice number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentReceipt {
    pub kind: PaymentKind,
    pub at_ms: i64,
    pub staff: String,
    pub player: String,
    pub payments: Vec<PaymentRow>,
    pub cleared: Vec<ClearedBill>,
    pub outstanding_after_minor: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Plain,
    Bold,
    /// Bold, double width and height: the branch name.
    Name,
    /// Bold, double height: the total.
    Total,
}

impl Style {
    /// Printed columns one character takes.
    fn factor(self) -> usize {
        if self == Self::Name { 2 } else { 1 }
    }

    fn size(self) -> u8 {
        match self {
            Self::Plain | Self::Bold => 0x00,
            Self::Name => 0x11,
            Self::Total => 0x01,
        }
    }

    fn bold(self) -> bool {
        self != Self::Plain
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    style: Style,
    text: String,
}

struct Layout {
    cols: usize,
    rows: Vec<Row>,
}

fn ascii(text: &str) -> String {
    pc437_text(text).into_iter().map(char::from).collect()
}

/// Wraps `text` at word boundaries to `width` characters, cutting a word longer than the line.
/// Newlines separate lines and blank lines are dropped. The Hub's receipt preview wraps the same
/// way (`apps/hub/src/lib/receipt.ts`).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in text.split('\n') {
        let mut current = String::new();
        for word in line.split_whitespace() {
            let mut rest: Vec<char> = word.chars().collect();
            while rest.len() > width {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                out.push(rest.drain(..width).collect());
            }
            let rest: String = rest.into_iter().collect();
            let rest_len = rest.chars().count();
            if current.is_empty() {
                current = rest;
            } else if current.chars().count() + 1 + rest_len <= width {
                current.push(' ');
                current.push_str(&rest);
            } else {
                out.push(std::mem::replace(&mut current, rest));
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

fn len(text: &str) -> usize {
    text.chars().count()
}

fn pad_left(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(len(text))))
}

fn pad_right(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(len(text))))
}

impl Layout {
    fn new(paper: PaperWidth) -> Self {
        Self {
            cols: paper.columns(),
            rows: Vec::new(),
        }
    }

    fn push(&mut self, style: Style, text: &str) {
        self.rows.push(Row {
            style,
            text: ascii(text).trim_end().to_owned(),
        });
    }

    fn rule(&mut self) {
        let rule = "-".repeat(self.cols);
        self.push(Style::Plain, &rule);
    }

    fn lines(&mut self, style: Style, text: &str) {
        for line in wrap(text, self.cols / style.factor()) {
            self.push(style, &line);
        }
    }

    fn centred(&mut self, style: Style, text: &str) {
        let width = self.cols / style.factor();
        for line in wrap(text, width) {
            let pad = (width - len(&line)) / 2;
            self.push(style, &format!("{}{line}", " ".repeat(pad)));
        }
    }

    /// `left` at the left margin and `right` at the right; when both do not fit on one line,
    /// `left` wraps and `right` goes right-aligned on the line below.
    fn pair(&mut self, style: Style, left: &str, right: &str) {
        let (left, right) = (ascii(left), ascii(right));
        if len(&left) + 1 + len(&right) <= self.cols {
            let gap = self.cols - len(&left) - len(&right);
            self.push(style, &format!("{left}{}{right}", " ".repeat(gap)));
            return;
        }
        self.lines(style, &left);
        for chunk in wrap(&right, self.cols) {
            let pad = self.cols - len(&chunk);
            self.push(style, &format!("{}{chunk}", " ".repeat(pad)));
        }
    }

    fn header(&mut self, s: &ReceiptSettings, gstin: Option<&str>) {
        self.centred(Style::Name, &s.name);
        self.centred(Style::Plain, &s.address);
        if let Some(gstin) = gstin.filter(|g| !g.is_empty()) {
            self.centred(Style::Plain, &format!("GSTIN {gstin}"));
        }
        self.centred(Style::Plain, &s.phone);
        self.rule();
    }

    fn meta(&mut self, s: &ReceiptSettings, at_ms: i64, staff: &str, customer: &str) {
        self.pair(Style::Plain, "Date", &receipt_time(at_ms, &s.tz));
        if !staff.trim().is_empty() {
            self.pair(Style::Plain, "Staff", staff.trim());
        }
        self.pair(Style::Plain, "Customer", customer);
        self.rule();
    }

    fn footer(&mut self, s: &ReceiptSettings) {
        if s.footer.trim().is_empty() {
            return;
        }
        self.rule();
        self.centred(Style::Plain, &s.footer);
    }

    fn sessions(&mut self, s: &ReceiptSettings, r: &SaleReceipt) {
        for line in &r.sessions {
            self.lines(Style::Plain, &line.device);
            if let Some(sac) = line.sac.as_deref().filter(|_| r.gst) {
                self.push(Style::Plain, &format!("SAC {sac}"));
            }
            self.pair(
                Style::Plain,
                &format!(
                    "{} min x {}/h",
                    line.minutes,
                    s.money.number(line.rate_minor)
                ),
                &s.money.number(line.amount_minor),
            );
        }
    }

    fn items(&mut self, s: &ReceiptSettings, r: &SaleReceipt) {
        if r.items.is_empty() {
            return;
        }
        let m = &s.money;
        let widths = |f: fn(&ItemLine) -> i64, min: usize| {
            r.items
                .iter()
                .map(|i| len(&m.number(f(i))))
                .fold(min, usize::max)
        };
        let qty_w = r
            .items
            .iter()
            .map(|i| len(&i.qty.to_string()))
            .fold(3, usize::max);
        let rate_w = widths(|i| i.rate_minor, 4);
        let amt_w = widths(|i| i.amount_minor, 6);
        let name_w = self.cols.saturating_sub(qty_w + rate_w + amt_w + 3);
        let columns = s.paper == PaperWidth::Mm80 && name_w >= MIN_NAME_COLUMN;
        if columns {
            self.push(
                Style::Plain,
                &format!(
                    "{} {} {} {}",
                    pad_right("Item", name_w),
                    pad_left("Qty", qty_w),
                    pad_left("Rate", rate_w),
                    pad_left("Amount", amt_w)
                ),
            );
        }
        for item in &r.items {
            let hsn = item.hsn.as_deref().filter(|_| r.gst);
            if columns {
                let mut name = wrap(&item.name, name_w);
                if name.is_empty() {
                    name.push(String::new());
                }
                for (n, text) in name.iter().enumerate() {
                    let tail = if n == 0 {
                        format!(
                            " {} {} {}",
                            pad_left(&item.qty.to_string(), qty_w),
                            pad_left(&m.number(item.rate_minor), rate_w),
                            pad_left(&m.number(item.amount_minor), amt_w)
                        )
                    } else {
                        String::new()
                    };
                    self.push(Style::Plain, &format!("{}{tail}", pad_right(text, name_w)));
                }
                if let Some(hsn) = hsn {
                    self.push(Style::Plain, &format!("HSN {hsn}"));
                }
            } else {
                self.lines(Style::Plain, &item.name);
                if let Some(hsn) = hsn {
                    self.push(Style::Plain, &format!("HSN {hsn}"));
                }
                self.pair(
                    Style::Plain,
                    &format!("{} x {}", item.qty, m.number(item.rate_minor)),
                    &m.number(item.amount_minor),
                );
            }
        }
    }

    fn tax(&mut self, s: &ReceiptSettings, r: &SaleReceipt) {
        if r.tax.is_empty() {
            return;
        }
        let m = &s.money;
        self.pair(Style::Plain, "Subtotal", &m.number(r.subtotal_minor));
        for t in &r.tax {
            let rate = percent(t.rate_bp);
            if !r.gst {
                self.pair(
                    Style::Plain,
                    &format!("{} {rate}%", r.tax_label),
                    &m.number(t.tax_minor),
                );
                continue;
            }
            self.pair(
                Style::Plain,
                &format!("Taxable {rate}%"),
                &m.number(t.taxable_minor),
            );
            if t.tax_minor == 0 && t.rate_bp == 0 {
                continue;
            }
            match (t.cgst_minor, t.sgst_minor) {
                (Some(cgst), Some(sgst)) => {
                    let half = half_percent(t.rate_bp);
                    self.pair(Style::Plain, &format!("CGST {half}%"), &m.number(cgst));
                    self.pair(Style::Plain, &format!("SGST {half}%"), &m.number(sgst));
                }
                _ => self.pair(
                    Style::Plain,
                    &format!("GST {rate}%"),
                    &m.number(t.tax_minor),
                ),
            }
        }
    }

    fn payments(&mut self, s: &ReceiptSettings, payments: &[PaymentRow]) {
        for p in payments {
            self.pair(
                Style::Plain,
                &payment_label(p),
                &s.money.number(p.amount_minor),
            );
        }
    }
}

/// `1800` as `18`, `250` as `2.5`, `25` as `0.25`.
fn percent(bp: u16) -> String {
    let (whole, frac) = (bp / 100, bp % 100);
    match (frac, frac % 10) {
        (0, _) => whole.to_string(),
        (_, 0) => format!("{whole}.{}", frac / 10),
        _ => format!("{whole}.{frac:02}"),
    }
}

/// Half of `bp` as a percent, exact for an odd rate: `1800` as `9`, `25` as `0.125`.
fn half_percent(bp: u16) -> String {
    let milli = u32::from(bp) * 5;
    let (whole, frac) = (milli / 1000, milli % 1000);
    if frac == 0 {
        return whole.to_string();
    }
    format!("{whole}.{frac:03}")
        .trim_end_matches('0')
        .to_owned()
}

fn payment_label(p: &PaymentRow) -> String {
    let reference = p
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    match (p.method, reference) {
        (PaymentMethod::Cash, _) => "Cash".to_owned(),
        (PaymentMethod::Upi, Some(r)) => {
            let last: Vec<char> = r.chars().rev().take(4).collect();
            format!("UPI ****{}", last.into_iter().rev().collect::<String>())
        }
        (PaymentMethod::Upi, None) => "UPI".to_owned(),
        (PaymentMethod::Card, Some(r)) => format!("Card ref {r}"),
        (PaymentMethod::Card, None) => "Card".to_owned(),
        (PaymentMethod::Wallet, _) => "Wallet".to_owned(),
        (PaymentMethod::Credit, _) => "On credit".to_owned(),
    }
}

/// The branch-local date and time as printed: `28 Oct 2026 14:05`.
fn receipt_time(at_ms: i64, tz: &str) -> String {
    let utc = DateTime::<Utc>::from_timestamp_millis(at_ms).unwrap_or_default();
    match tz.parse::<Tz>() {
        Ok(zone) => utc
            .with_timezone(&zone)
            .format("%-d %b %Y %H:%M")
            .to_string(),
        Err(_) => utc.format("%-d %b %Y %H:%M").to_string(),
    }
}

fn encode(rows: &[Row]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.len() * 40 + 16);
    out.extend_from_slice(&[ESC, b'@']);
    out.extend_from_slice(&[ESC, b't', 0]);
    // Font A.
    out.extend_from_slice(&[ESC, b'M', 0]);
    for row in rows {
        if row.style.bold() {
            out.extend_from_slice(&[ESC, b'E', 1]);
            out.extend_from_slice(&[GS, b'!', row.style.size()]);
        }
        out.extend_from_slice(row.text.as_bytes());
        out.push(LF);
        if row.style.bold() {
            out.extend_from_slice(&[ESC, b'E', 0]);
            out.extend_from_slice(&[GS, b'!', 0x00]);
        }
    }
    out.extend_from_slice(&[ESC, b'd', 4]);
    out.extend_from_slice(&[GS, b'V', 66, 0]);
    out
}

fn sale_rows_for(s: &ReceiptSettings, r: &SaleReceipt, reprint: bool) -> Vec<Row> {
    let mut l = Layout::new(s.paper);
    l.header(s, r.gstin.as_deref().filter(|_| r.gst));
    l.centred(Style::Bold, if r.gst { "Tax Invoice" } else { "Invoice" });
    if reprint {
        l.centred(Style::Bold, "Reprint");
    }
    if let Some(buyer) = r.buyer.as_ref().filter(|_| r.gst) {
        l.lines(Style::Plain, &buyer.legal_name);
        l.lines(Style::Plain, &format!("GSTIN {}", buyer.gstin));
    }
    if let Some(no) = &r.invoice_no {
        l.pair(Style::Plain, "Invoice no.", no);
    }
    l.meta(s, r.at_ms, &r.staff, r.player.as_deref().unwrap_or("Guest"));
    l.sessions(s, r);
    l.items(s, r);
    l.rule();
    if let Some(c) = &r.coupon {
        l.pair(
            Style::Plain,
            &format!("Coupon {}", c.code),
            &format!("-{}", s.money.number(c.amount_minor)),
        );
    }
    if let Some(d) = r.discount_minor {
        l.pair(Style::Plain, "Discount", &format!("-{}", s.money.number(d)));
    }
    l.tax(s, r);
    l.pair(Style::Total, "TOTAL", &s.money.with_symbol(r.total_minor));
    l.rule();
    l.payments(s, &r.payments);
    l.footer(s);
    l.rows
}

#[cfg(test)]
fn sale_rows(s: &ReceiptSettings, r: &SaleReceipt) -> Vec<Row> {
    sale_rows_for(s, r, false)
}

/// The numbers a credit note carries besides the refunded lines (Story 5.24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditHead {
    pub credit_note_no: String,
    /// The invoice the refund is against, and when it was issued.
    pub invoice_no: String,
    pub invoice_at_ms: i64,
}

/// A credit note: the Story 5.16 sale layout under the title "Credit Note", with its own number,
/// the original invoice's number and date, and every amount negative. `sale.items`, `tax`,
/// `total_minor` and `payments` hold the negative amounts; `invoice_no` is not printed.
fn credit_rows(s: &ReceiptSettings, r: &SaleReceipt, h: &CreditHead) -> Vec<Row> {
    let mut l = Layout::new(s.paper);
    l.header(s, r.gstin.as_deref().filter(|_| r.gst));
    l.centred(Style::Bold, "Credit Note");
    if let Some(buyer) = r.buyer.as_ref().filter(|_| r.gst) {
        l.lines(Style::Plain, &buyer.legal_name);
        l.lines(Style::Plain, &format!("GSTIN {}", buyer.gstin));
    }
    l.pair(Style::Plain, "Credit note no.", &h.credit_note_no);
    l.pair(Style::Plain, "Invoice no.", &h.invoice_no);
    l.pair(
        Style::Plain,
        "Invoice date",
        &receipt_time(h.invoice_at_ms, &s.tz),
    );
    l.meta(s, r.at_ms, &r.staff, r.player.as_deref().unwrap_or("Guest"));
    l.items(s, r);
    l.rule();
    l.tax(s, r);
    l.pair(Style::Total, "REFUND", &s.money.with_symbol(r.total_minor));
    l.rule();
    l.payments(s, &r.payments);
    l.footer(s);
    l.rows
}

/// A credit note as ESC/POS bytes.
pub fn render_credit_note(
    settings: &ReceiptSettings,
    sale: &SaleReceipt,
    head: &CreditHead,
) -> Vec<u8> {
    encode(&credit_rows(settings, sale, head))
}

fn payment_rows(s: &ReceiptSettings, r: &PaymentReceipt) -> Vec<Row> {
    let mut l = Layout::new(s.paper);
    l.header(s, None);
    l.centred(Style::Bold, "Payment Receipt");
    l.meta(s, r.at_ms, &r.staff, &r.player);
    l.lines(
        Style::Plain,
        match r.kind {
            PaymentKind::WalletTopup => "Wallet top-up",
            PaymentKind::CreditCollection => "Credit collection",
        },
    );
    let amount: i64 = r.payments.iter().map(|p| p.amount_minor).sum();
    l.pair(Style::Total, "AMOUNT", &s.money.with_symbol(amount));
    l.payments(s, &r.payments);
    if r.kind == PaymentKind::CreditCollection {
        if !r.cleared.is_empty() {
            l.rule();
            l.lines(Style::Plain, "Bills cleared");
            for b in &r.cleared {
                l.pair(Style::Plain, &b.label, &s.money.number(b.cleared_minor));
            }
        }
        if let Some(left) = r.outstanding_after_minor {
            l.rule();
            l.pair(Style::Bold, "Outstanding", &s.money.with_symbol(left));
        }
    }
    l.footer(s);
    l.rows
}

/// A sale as ESC/POS bytes: header, title, invoice number, date, staff, customer, play time,
/// items, tax per rate, total, payments and footer.
pub fn render_sale(settings: &ReceiptSettings, sale: &SaleReceipt) -> Vec<u8> {
    render_sale_copy(settings, sale, false)
}

/// [`render_sale`], with a centred "Reprint" line under the title when `reprint` is set (Story
/// 5.17: the same receipt printed again).
pub fn render_sale_copy(settings: &ReceiptSettings, sale: &SaleReceipt, reprint: bool) -> Vec<u8> {
    encode(&sale_rows_for(settings, sale, reprint))
}

/// A wallet top-up or credit collection as ESC/POS bytes. It never carries an invoice number.
pub fn render_payment(settings: &ReceiptSettings, payment: &PaymentReceipt) -> Vec<u8> {
    encode(&payment_rows(settings, payment))
}

/// The bytes with each control character spelled out, for the golden files: ESC and GS by name,
/// other controls in hex, a line feed as a real newline and `{` doubled.
#[cfg(test)]
fn dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            ESC => out.push_str("{ESC}"),
            GS => out.push_str("{GS}"),
            LF => out.push('\n'),
            b'{' => out.push_str("{{"),
            0..=0x1F | 0x7F.. => out.push_str(&format!("{{{b:02x}}}")),
            _ => out.push(char::from(b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const MS_2026_10_28_0835_UTC: i64 = 1_793_176_500_000;

    fn money(code: &str) -> ReceiptMoney {
        match code {
            "INR" => ReceiptMoney::new(code, SymbolPosition::Before, NumberFormat::Indian),
            "JPY" => ReceiptMoney::new(code, SymbolPosition::After, NumberFormat::International),
            _ => ReceiptMoney::new(code, SymbolPosition::Before, NumberFormat::International),
        }
    }

    fn settings(paper: PaperWidth, code: &str) -> ReceiptSettings {
        ReceiptSettings {
            paper,
            name: "Arena 360 Koramangala".into(),
            address: "12, 5th Cross, Koramangala 4th Block\nBengaluru 560034".into(),
            phone: "+91 98450 12345".into(),
            footer: "Thank you for playing!\nGoods once sold are not returnable.".into(),
            money: money(code),
            tz: "Asia/Kolkata".into(),
        }
    }

    fn pay(method: PaymentMethod, amount_minor: i64, reference: Option<&str>) -> PaymentRow {
        PaymentRow {
            method,
            amount_minor,
            reference: reference.map(str::to_owned),
        }
    }

    fn rate(bp: u16, taxable: i64, tax: i64, gst: bool) -> RateTax {
        RateTax {
            rate_bp: bp,
            taxable_minor: taxable,
            tax_minor: tax,
            cgst_minor: gst.then_some(tax / 2 + tax % 2),
            sgst_minor: gst.then_some(tax / 2),
        }
    }

    fn gst_sale() -> SaleReceipt {
        SaleReceipt {
            gst: true,
            gstin: Some("29ABCDE1234F1Z5".into()),
            tax_label: "Tax".into(),
            invoice_no: Some("KOR/26-27/E1k/000142".into()),
            buyer: None,
            coupon: None,
            discount_minor: None,
            at_ms: MS_2026_10_28_0835_UTC,
            staff: "Meera".into(),
            player: Some("shadowfox".into()),
            sessions: vec![
                SessionLine {
                    device: "PC-07".into(),
                    minutes: 90,
                    rate_minor: 12_000,
                    amount_minor: 18_000,
                    sac: Some("9996".into()),
                },
                SessionLine {
                    device: "PC-07".into(),
                    minutes: 30,
                    rate_minor: 15_000,
                    amount_minor: 7_500,
                    sac: Some("9996".into()),
                },
            ],
            items: vec![
                ItemLine {
                    name: "Cold Coffee".into(),
                    qty: 2,
                    rate_minor: 12_000,
                    amount_minor: 24_000,
                    hsn: Some("2101".into()),
                },
                ItemLine {
                    name: "Extra Large Peri Peri Loaded Fries With Cheese Dip".into(),
                    qty: 1,
                    rate_minor: 19_900,
                    amount_minor: 19_900,
                    hsn: Some("2005".into()),
                },
                ItemLine {
                    name: "Energy Drink".into(),
                    qty: 12,
                    rate_minor: 12_500_000,
                    amount_minor: 150_000_000,
                    hsn: Some("22029990".into()),
                },
            ],
            subtotal_minor: 175_019_067,
            tax: vec![
                rate(500, 150_024_000, 7_501_200, true),
                rate(1800, 25_495_000, 4_589_100, true),
            ],
            total_minor: 187_109_367,
            payments: vec![
                pay(PaymentMethod::Cash, 50_000, None),
                pay(PaymentMethod::Upi, 100_000_000, Some("UTR4455781234")),
                pay(PaymentMethod::Card, 87_059_367, Some("HDFC-ref 7781")),
                pay(PaymentMethod::Credit, 0, None),
            ],
        }
    }

    fn simple_sale() -> SaleReceipt {
        SaleReceipt {
            gst: false,
            gstin: None,
            tax_label: "VAT".into(),
            invoice_no: Some("DUB/26/E2a/000007".into()),
            buyer: None,
            coupon: None,
            discount_minor: None,
            at_ms: MS_2026_10_28_0835_UTC,
            staff: String::new(),
            player: None,
            sessions: vec![SessionLine {
                device: "Simulator 2".into(),
                minutes: 60,
                rate_minor: 2_500,
                amount_minor: 2_500,
                sac: None,
            }],
            items: vec![
                ItemLine {
                    name: "Water".into(),
                    qty: 3,
                    rate_minor: 150,
                    amount_minor: 450,
                    hsn: None,
                },
                ItemLine {
                    name: "Supercalifragilisticexpialidocious-Milkshake".into(),
                    qty: 1,
                    rate_minor: 900,
                    amount_minor: 900,
                    hsn: None,
                },
            ],
            subtotal_minor: 3_571,
            tax: vec![rate(500, 3_571, 179, false)],
            total_minor: 3_750,
            payments: vec![
                pay(PaymentMethod::Wallet, 1_000, None),
                pay(PaymentMethod::Credit, 2_750, None),
            ],
        }
    }

    fn jpy_sale() -> SaleReceipt {
        SaleReceipt {
            invoice_no: Some("TKY/26/E1b/000031".into()),
            buyer: None,
            coupon: None,
            discount_minor: None,
            sessions: vec![SessionLine {
                device: "VR Rig".into(),
                minutes: 45,
                rate_minor: 1_200,
                amount_minor: 900,
                sac: None,
            }],
            items: vec![ItemLine {
                name: "Ramune".into(),
                qty: 2,
                rate_minor: 350,
                amount_minor: 700,
                hsn: None,
            }],
            subtotal_minor: 1_455,
            tax: vec![rate(1000, 1_455, 145, false)],
            total_minor: 1_600,
            payments: vec![pay(PaymentMethod::Cash, 1_600, None)],
            tax_label: "Tax".into(),
            ..simple_sale()
        }
    }

    fn topup() -> PaymentReceipt {
        PaymentReceipt {
            kind: PaymentKind::WalletTopup,
            at_ms: MS_2026_10_28_0835_UTC,
            staff: "Meera".into(),
            player: "shadowfox".into(),
            payments: vec![
                pay(PaymentMethod::Cash, 20_000, None),
                pay(PaymentMethod::Upi, 30_000, Some("9981")),
            ],
            cleared: Vec::new(),
            outstanding_after_minor: None,
        }
    }

    fn collection() -> PaymentReceipt {
        PaymentReceipt {
            kind: PaymentKind::CreditCollection,
            payments: vec![pay(PaymentMethod::Upi, 90_000, Some("UTR1234567890"))],
            cleared: vec![
                ClearedBill {
                    label: "KOR/26-27/E1k/000120".into(),
                    cleared_minor: 60_000,
                },
                ClearedBill {
                    label: "12 Oct 2026".into(),
                    cleared_minor: 30_000,
                },
            ],
            outstanding_after_minor: Some(45_050),
            ..topup()
        }
    }

    fn golden_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/printer/golden")
            .join(format!("{name}.escpos.txt"))
    }

    /// Compares `bytes` with the stored golden file; `UPDATE_GOLDEN=1` rewrites it.
    fn check_golden(name: &str, bytes: &[u8]) {
        let path = golden_path(name);
        let got = dump(bytes);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &got).unwrap();
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_GOLDEN=1", path.display()));
        assert_eq!(
            got,
            want.replace("\r\n", "\n"),
            "{name} differs from its golden file"
        );
    }

    fn assert_fits(rows: &[Row], paper: PaperWidth) {
        for row in rows {
            assert!(
                len(&row.text) * row.style.factor() <= paper.columns(),
                "{:?} is wider than {} columns",
                row.text,
                paper.columns()
            );
            assert!(row.text.is_ascii(), "{:?} is not PC437-safe", row.text);
        }
    }

    #[test]
    fn every_fixture_matches_its_golden_file_and_fits_the_paper() {
        for (paper, tag) in [(PaperWidth::Mm80, "80"), (PaperWidth::Mm58, "58")] {
            let inr = settings(paper, "INR");
            let jpy = settings(paper, "JPY");
            let sales = [
                ("gst_multi_rate", &inr, gst_sale()),
                (
                    "gst_coupon",
                    &inr,
                    SaleReceipt {
                        coupon: Some(Coupon {
                            code: "DIWALI-10".into(),
                            amount_minor: 5_000,
                        }),
                        ..gst_sale()
                    },
                ),
                (
                    "gst_coupon_discount",
                    &inr,
                    SaleReceipt {
                        coupon: Some(Coupon {
                            code: "DIWALI-10".into(),
                            amount_minor: 5_000,
                        }),
                        discount_minor: Some(2_500),
                        ..gst_sale()
                    },
                ),
                ("simple_tax", &inr, simple_sale()),
                ("zero_exponent", &jpy, jpy_sale()),
            ];
            for (name, s, sale) in sales {
                assert_fits(&sale_rows(s, &sale), paper);
                check_golden(&format!("{name}_{tag}"), &render_sale(s, &sale));
            }
            for (name, payment) in [("topup", topup()), ("collection", collection())] {
                assert_fits(&payment_rows(&inr, &payment), paper);
                check_golden(&format!("{name}_{tag}"), &render_payment(&inr, &payment));
            }
        }
    }

    fn text(rows: Vec<Row>) -> Vec<String> {
        rows.into_iter().map(|r| r.text).collect()
    }

    #[test]
    fn a_gst_sale_reads_as_a_tax_invoice() {
        let s = settings(PaperWidth::Mm80, "INR");
        let rows = text(sale_rows(&s, &gst_sale()));
        assert!(rows.iter().any(|r| r.trim() == "Tax Invoice"));
        assert!(rows.iter().any(|r| r.trim() == "GSTIN 29ABCDE1234F1Z5"));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Invoice no.") && r.ends_with("KOR/26-27/E1k/000142"))
        );
        assert!(rows.iter().any(|r| r.starts_with("HSN 22029990")));
        assert!(rows.iter().any(|r| r.starts_with("SAC 9996")));
        assert!(rows.iter().any(|r| r.starts_with("CGST 2.5%")));
        assert!(rows.iter().any(|r| r.starts_with("SGST 9%")));
        assert!(rows.iter().any(|r| r.starts_with("UPI ****1234")));
        assert!(rows.iter().any(|r| r.starts_with("Card ref HDFC-ref 7781")));
        assert!(rows.iter().any(|r| r.starts_with("On credit")));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Customer") && r.ends_with("shadowfox"))
        );
    }

    #[test]
    fn a_simple_sale_is_an_invoice_with_no_gst_fields() {
        let s = settings(PaperWidth::Mm80, "INR");
        let rows = text(sale_rows(&s, &simple_sale()));
        assert!(rows.iter().any(|r| r.trim() == "Invoice"));
        assert!(rows.iter().all(|r| !r.contains("GSTIN")
            && !r.contains("HSN")
            && !r.contains("SAC")
            && !r.contains("CGST")));
        assert!(rows.iter().any(|r| r.starts_with("VAT 5%")));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Customer") && r.ends_with("Guest"))
        );
        assert!(rows.iter().all(|r| !r.starts_with("Staff")));
    }

    #[test]
    fn a_business_invoice_prints_the_buyer_under_the_title_on_gst_only() {
        let s = settings(PaperWidth::Mm80, "INR");
        let buyer = Buyer {
            legal_name: "Tata Consultancy Services".into(),
            gstin: "29AABCT1332L1ZA".into(),
        };
        let sale = SaleReceipt {
            buyer: Some(buyer.clone()),
            ..gst_sale()
        };
        let rows = text(sale_rows(&s, &sale));
        let title = rows.iter().position(|r| r.trim() == "Tax Invoice").unwrap();
        assert_eq!(rows[title + 1].trim(), "Tata Consultancy Services");
        assert_eq!(rows[title + 2].trim(), "GSTIN 29AABCT1332L1ZA");
        assert!(
            rows[title + 3].starts_with("Invoice no."),
            "the buyer sits directly under the title: {rows:?}"
        );

        let plain = text(sale_rows(&s, &gst_sale()));
        assert!(plain.iter().all(|r| !r.contains("29AABCT1332L1ZA")));

        let simple = SaleReceipt {
            buyer: Some(buyer),
            ..simple_sale()
        };
        assert!(
            text(sale_rows(&s, &simple))
                .iter()
                .all(|r| !r.contains("Tata") && !r.contains("GSTIN")),
            "a branch that is not on GST prints no buyer"
        );
    }

    #[test]
    fn a_coupon_prints_after_the_items_and_before_the_tax_lines() {
        let sale = SaleReceipt {
            coupon: Some(Coupon {
                code: "DIWALI-10".into(),
                amount_minor: 12_345,
            }),
            ..gst_sale()
        };
        for paper in [PaperWidth::Mm80, PaperWidth::Mm58] {
            let rows = text(sale_rows(&settings(paper, "INR"), &sale));
            let line = rows.iter().position(|r| r.starts_with("Coupon")).unwrap();
            assert!(rows[line].starts_with("Coupon DIWALI-10"), "{rows:?}");
            assert!(rows[line].ends_with("-123.45"), "{rows:?}");
            let subtotal = rows.iter().position(|r| r.starts_with("Subtotal")).unwrap();
            let last_item = rows.iter().rposition(|r| r.contains("HSN")).unwrap();
            assert!(last_item < line && line < subtotal, "{rows:?}");
        }
        let none = text(sale_rows(&settings(PaperWidth::Mm80, "INR"), &gst_sale()));
        assert!(none.iter().all(|r| !r.contains("Coupon")));
    }

    #[test]
    fn a_discount_prints_after_the_coupon_and_before_the_tax_lines() {
        let sale = SaleReceipt {
            coupon: Some(Coupon {
                code: "DIWALI-10".into(),
                amount_minor: 12_345,
            }),
            discount_minor: Some(6_789),
            ..gst_sale()
        };
        for paper in [PaperWidth::Mm80, PaperWidth::Mm58] {
            let rows = text(sale_rows(&settings(paper, "INR"), &sale));
            let coupon = rows.iter().position(|r| r.starts_with("Coupon")).unwrap();
            let line = rows.iter().position(|r| r.starts_with("Discount")).unwrap();
            assert!(rows[line].ends_with("-67.89"), "{rows:?}");
            let subtotal = rows.iter().position(|r| r.starts_with("Subtotal")).unwrap();
            assert!(coupon < line && line < subtotal, "{rows:?}");
        }
        let none = text(sale_rows(&settings(PaperWidth::Mm80, "INR"), &gst_sale()));
        assert!(none.iter().all(|r| !r.contains("Discount")));
    }

    #[test]
    fn a_long_legal_name_wraps_on_a_58_mm_receipt() {
        let s = settings(PaperWidth::Mm58, "INR");
        let sale = SaleReceipt {
            buyer: Some(Buyer {
                legal_name: "Bharat Heavy Electricals Limited Corporate Office".into(),
                gstin: "29AABCT1332L1ZA".into(),
            }),
            ..gst_sale()
        };
        let rows = text(sale_rows(&s, &sale));
        assert!(rows.iter().all(|r| r.chars().count() <= 32), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("Limited")));
        assert!(rows.iter().any(|r| r.trim() == "GSTIN 29AABCT1332L1ZA"));
    }

    #[test]
    fn a_gstin_is_not_printed_on_a_branch_that_is_not_on_gst() {
        let s = settings(PaperWidth::Mm80, "INR");
        let sale = SaleReceipt {
            gstin: Some("29ABCDE1234F1Z5".into()),
            ..simple_sale()
        };
        assert!(
            text(sale_rows(&s, &sale))
                .iter()
                .all(|r| !r.contains("GSTIN"))
        );
    }

    #[test]
    fn eighty_mm_items_sit_in_columns_and_fifty_eight_stacks_them() {
        let wide = text(sale_rows(
            &settings(PaperWidth::Mm80, "INR"),
            &simple_sale(),
        ));
        let header = wide.iter().find(|r| r.starts_with("Item")).unwrap();
        assert!(header.ends_with("Amount") && len(header) == 48);
        let water = wide.iter().find(|r| r.starts_with("Water")).unwrap();
        assert!(water.ends_with("1.50   4.50") && len(water) == 48);
        let narrow = text(sale_rows(
            &settings(PaperWidth::Mm58, "INR"),
            &simple_sale(),
        ));
        assert!(narrow.iter().all(|r| !r.starts_with("Item")));
        let at = narrow.iter().position(|r| r == "Water").unwrap();
        assert!(narrow[at + 1].starts_with("3 x 1.50") && narrow[at + 1].ends_with("4.50"));
        assert_eq!(len(&narrow[at + 1]), 32);
    }

    #[test]
    fn long_amounts_and_names_never_break_the_width() {
        let mut sale = gst_sale();
        sale.items[0].amount_minor = 99_999_999_999_999;
        sale.items[2].name = "A".repeat(200);
        for paper in [PaperWidth::Mm80, PaperWidth::Mm58] {
            assert_fits(&sale_rows(&settings(paper, "INR"), &sale), paper);
        }
    }

    #[test]
    fn a_payment_receipt_has_no_invoice_number() {
        let s = settings(PaperWidth::Mm80, "INR");
        for payment in [topup(), collection()] {
            let rows = text(payment_rows(&s, &payment));
            assert!(rows.iter().any(|r| r.trim() == "Payment Receipt"));
            assert!(
                rows.iter()
                    .all(|r| !r.contains("Invoice") && !r.contains("GSTIN"))
            );
        }
        let rows = text(payment_rows(&s, &collection()));
        assert!(rows.iter().any(|r| r.trim() == "Bills cleared"));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("KOR/26-27/E1k/000120") && r.ends_with("600.00"))
        );
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Outstanding") && r.ends_with("Rs450.50"))
        );
        assert!(rows.iter().any(|r| r.starts_with("UPI ****7890")));
    }

    #[test]
    fn the_branch_name_is_double_width_and_the_total_double_height() {
        let bytes = render_sale(&settings(PaperWidth::Mm58, "INR"), &simple_sale());
        assert!(bytes.starts_with(b"\x1b@\x1bt\x00\x1bM\x00\x1bE\x01\x1d!\x11"));
        let total = bytes.windows(6).position(|w| w == b"\x1d!\x01TOT").unwrap();
        assert_eq!(&bytes[total - 3..total], b"\x1bE\x01");
        assert!(bytes.ends_with(b"\x1bd\x04\x1dVB\x00"));
    }

    #[test]
    fn money_has_an_ascii_symbol_and_the_branch_grouping() {
        let inr = money("INR");
        assert_eq!(inr.with_symbol(11_800), "Rs118.00");
        assert_eq!(inr.with_symbol(-12_345_678), "-Rs1,23,456.78");
        assert_eq!(inr.number(0), "0.00");
        let jpy = ReceiptMoney::new("JPY", SymbolPosition::After, NumberFormat::International);
        assert_eq!(jpy.with_symbol(1_234_567), "1,234,567 JPY");
        assert_eq!(money("AED").with_symbol(500), "AED5.00");
        assert_eq!(money("XXX").with_symbol(500), "XXX5.00");
    }

    #[test]
    fn rates_print_without_trailing_zeros() {
        assert_eq!(percent(1800), "18");
        assert_eq!(percent(250), "2.5");
        assert_eq!(percent(25), "0.25");
        assert_eq!(percent(0), "0");
    }

    #[test]
    fn half_rates_are_exact_for_odd_basis_points() {
        assert_eq!(half_percent(1800), "9");
        assert_eq!(half_percent(500), "2.5");
        assert_eq!(half_percent(25), "0.125");
        assert_eq!(half_percent(121), "0.605");
        assert_eq!(half_percent(0), "0");
    }

    #[test]
    fn wrapping_cuts_at_words_and_splits_a_long_word() {
        assert_eq!(wrap("alpha beta gamma", 10), ["alpha beta", "gamma"]);
        assert_eq!(wrap("abcdefghijkl mno", 5), ["abcde", "fghij", "kl", "mno"]);
        assert_eq!(wrap("a\n\n  b  ", 5), ["a", "b"]);
        assert!(wrap("   ", 5).is_empty());
    }

    #[test]
    fn the_date_is_the_branch_local_time() {
        assert_eq!(
            receipt_time(MS_2026_10_28_0835_UTC, "Asia/Kolkata"),
            "28 Oct 2026 14:05"
        );
        assert_eq!(
            receipt_time(MS_2026_10_28_0835_UTC, "UTC"),
            "28 Oct 2026 08:35"
        );
    }

    #[test]
    fn an_empty_footer_leaves_no_trailing_rule_and_no_number_leaves_no_line() {
        let mut s = settings(PaperWidth::Mm58, "INR");
        s.footer.clear();
        let mut sale = simple_sale();
        sale.invoice_no = None;
        let rows = text(sale_rows(&s, &sale));
        assert!(rows.iter().all(|r| !r.starts_with("Invoice no.")));
        assert!(rows.last().unwrap().starts_with("On credit"));
    }
}

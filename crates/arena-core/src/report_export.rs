//! Exporting a Hub report to CSV or PDF (Story 5.32). Every report is first read through the
//! same `SnapshotReader` call its screen uses, then laid out once as an [`ExportDoc`] of typed
//! cells; the CSV and the PDF are both written from that one document, so a figure on the
//! screen, in the CSV and in the PDF is the same stored number. Nothing is recomputed and
//! nothing is written to the database.

use arena_proto::{
    AppError, ErrorCode, ExportFormat, NumberFormat, ReportExportRequest, SnapshotLocale,
};
use chrono::{DateTime, TimeZone};
use chrono_tz::Tz;

use crate::pdf::{self, PdfDoc, Table};
use crate::players::KEK_LEN;
use crate::snapshot::SnapshotReader;

/// One figure or text of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Text(String),
    Int(i64),
    /// Minor units of the branch currency.
    Money(i64),
    /// UTC milliseconds, written in the branch's local time.
    Time(i64),
    /// Basis points, written as a percentage.
    Bp(i64),
}

impl Cell {
    fn is_figure(&self) -> bool {
        !matches!(self, Cell::Text(_))
    }
}

fn t(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

/// One titled table of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSection {
    pub heading: String,
    pub header: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

impl ExportSection {
    fn new(heading: &str, header: &[&str]) -> Self {
        ExportSection {
            heading: heading.to_owned(),
            header: header.iter().map(|h| (*h).to_owned()).collect(),
            rows: Vec::new(),
        }
    }

    fn row(&mut self, cells: Vec<Cell>) {
        self.rows.push(cells);
    }

    fn has_money(&self) -> bool {
        self.rows
            .iter()
            .flatten()
            .any(|c| matches!(c, Cell::Money(_)))
    }

    /// A column of figures (right-aligned) when it has rows and every cell is one.
    fn is_figure_column(&self, c: usize) -> bool {
        !self.rows.is_empty()
            && self
                .rows
                .iter()
                .all(|r| r.get(c).is_some_and(Cell::is_figure))
    }
}

/// A report ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDoc {
    pub title: String,
    /// The business days or month it covers, or "As of" a time.
    pub range: String,
    /// A short name for the file.
    pub slug: String,
    pub sections: Vec<ExportSection>,
}

/// How a branch writes figures and times in an export.
#[derive(Debug, Clone)]
pub struct ExportFormatting {
    pub branch_name: String,
    pub branch_code: String,
    pub currency: String,
    pub exponent: u8,
    pub number_format: NumberFormat,
    pub tz: Tz,
}

impl ExportFormatting {
    fn from_branch(
        name: String,
        code: String,
        locale: Option<&SnapshotLocale>,
    ) -> ExportFormatting {
        ExportFormatting {
            branch_name: name,
            branch_code: code,
            currency: locale.map_or("INR", |l| l.currency.as_str()).to_owned(),
            exponent: locale.map_or(2, |l| l.exponent),
            number_format: locale.map_or(NumberFormat::Indian, |l| l.number_format),
            tz: locale
                .and_then(|l| l.tz.parse::<Tz>().ok())
                .unwrap_or(Tz::UTC),
        }
    }

    /// A UTC time as `YYYY-MM-DD HH:MM` on the branch clock.
    pub fn local_time(&self, ms: i64) -> String {
        match DateTime::from_timestamp_millis(ms) {
            Some(utc) => self
                .tz
                .from_utc_datetime(&utc.naive_utc())
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            None => String::new(),
        }
    }

    /// Minor units in major units with the currency's decimals and no grouping: `1234.50`.
    fn plain_money(&self, minor: i64) -> String {
        let exp = usize::from(self.exponent);
        let digits = format!("{:0>width$}", minor.unsigned_abs(), width = exp + 1);
        let (whole, fraction) = digits.split_at(digits.len() - exp);
        let sign = if minor < 0 { "-" } else { "" };
        if exp > 0 {
            format!("{sign}{whole}.{fraction}")
        } else {
            format!("{sign}{whole}")
        }
    }

    /// Minor units as the PDF shows them: grouped as the branch groups, with its decimals.
    fn grouped_money(&self, minor: i64) -> String {
        let number = crate::locale::format_number(
            minor.unsigned_abs(),
            usize::from(self.exponent),
            self.number_format,
        );
        if minor < 0 {
            format!("-{number}")
        } else {
            number
        }
    }

    /// Basis points as a percentage in tenths, as the screens show them (`62.5`, `62`).
    fn percent(bp: i64) -> String {
        let tenths = (bp as f64 / 10.0 + 0.5).floor() as i64;
        if tenths % 10 == 0 {
            (tenths / 10).to_string()
        } else {
            format!("{}.{}", tenths / 10, (tenths % 10).abs())
        }
    }

    fn csv_cell(&self, cell: &Cell) -> String {
        match cell {
            Cell::Text(s) => s.clone(),
            Cell::Int(n) => n.to_string(),
            Cell::Money(m) => self.plain_money(*m),
            Cell::Time(ms) => self.local_time(*ms),
            Cell::Bp(bp) => Self::percent(*bp),
        }
    }

    fn pdf_cell(&self, cell: &Cell) -> String {
        match cell {
            Cell::Text(s) => s.clone(),
            Cell::Int(n) => n.to_string(),
            Cell::Money(m) => self.grouped_money(*m),
            Cell::Time(ms) => self.local_time(*ms),
            Cell::Bp(bp) => format!("{}%", Self::percent(*bp)),
        }
    }
}

/// A CSV field: quoted when it holds a comma, quote or line break, and a text that a
/// spreadsheet would read as a formula is prefixed with `'`.
fn csv_field(text: &str, is_text: bool) -> String {
    let guarded = is_text && text.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let text = if guarded {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// The CSV of `doc`: UTF-8 with a byte-order mark (so Excel reads it as UTF-8), CRLF lines, one
/// header row per section, money in major units followed by a `Currency` column. A report of
/// several sections writes each as a table of its own, headed by its title and separated by a
/// blank line.
pub fn to_csv(doc: &ExportDoc, f: &ExportFormatting) -> Vec<u8> {
    let mut out = String::from('\u{FEFF}');
    let many = doc.sections.len() > 1;
    for (i, s) in doc.sections.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        if many {
            out.push_str(&csv_field(&s.heading, true));
            out.push_str("\r\n");
        }
        let money = s.has_money();
        let mut header: Vec<String> = s.header.iter().map(|h| csv_field(h, false)).collect();
        if money {
            header.push("Currency".to_owned());
        }
        out.push_str(&header.join(","));
        out.push_str("\r\n");
        for row in &s.rows {
            let mut line: Vec<String> = row
                .iter()
                .map(|c| csv_field(&f.csv_cell(c), matches!(c, Cell::Text(_))))
                .collect();
            if money {
                let row_has_money = row.iter().any(|c| matches!(c, Cell::Money(_)));
                line.push(if row_has_money {
                    f.currency.clone()
                } else {
                    String::new()
                });
            }
            out.push_str(&line.join(","));
            out.push_str("\r\n");
        }
    }
    out.into_bytes()
}

/// The A4 PDF of `doc` at `generated_ms`; `INTERNAL` only if the bundled font cannot be read.
pub fn to_pdf(
    doc: &ExportDoc,
    f: &ExportFormatting,
    generated_ms: i64,
) -> Result<Vec<u8>, AppError> {
    let tables = doc
        .sections
        .iter()
        .map(|s| {
            let mut header = s.header.clone();
            let mut right: Vec<bool> = (0..header.len()).map(|c| s.is_figure_column(c)).collect();
            let note = if s.has_money() {
                format!("{} (amounts in {})", s.heading, f.currency)
            } else {
                s.heading.clone()
            };
            if header.is_empty() {
                header.push(String::new());
                right.push(false);
            }
            let mut rows: Vec<Vec<String>> = s
                .rows
                .iter()
                .map(|r| r.iter().map(|c| f.pdf_cell(c)).collect())
                .collect();
            if rows.is_empty() {
                rows.push(vec!["None".to_owned()]);
            }
            Table {
                heading: note,
                header,
                right,
                rows,
            }
        })
        .collect();
    let pdf = PdfDoc {
        branch: f.branch_name.clone(),
        title: doc.title.clone(),
        range: doc.range.clone(),
        generated: format!("Generated {}", f.local_time(generated_ms)),
        tables,
    };
    pdf::render(&pdf)
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "the report font could not be read"))
}

/// `text` as the hex string the PDF shows it with (used by the API tests to find a figure).
#[doc(hidden)]
pub fn pdf_hex(text: &str) -> String {
    pdf::Font::sora()
        .map(|f| f.encode(text))
        .unwrap_or_default()
}

/// `snake_case` as a sentence: `cancelled_order` becomes `Cancelled order`.
fn label(key: &str) -> String {
    let mut text = key.replace('_', " ");
    if let Some(first) = text.get(..1) {
        let upper = first.to_uppercase();
        text.replace_range(..1, &upper);
    }
    text
}

fn sales_label(key: &str) -> String {
    match key {
        arena_proto::SALES_PLAY_TIME => "Play time".to_owned(),
        arena_proto::SALES_TAX_ADDED => "Tax added".to_owned(),
        arena_proto::RECEIVED_TOPUP => "Wallet top-ups".to_owned(),
        arena_proto::RECEIVED_CREDIT_COLLECT => "Credit collected".to_owned(),
        other => label(other),
    }
}

fn waste_label(key: &str) -> String {
    match key {
        arena_proto::WASTE_EXPIRED => "Expired write-off".to_owned(),
        other => label(other),
    }
}

fn credit_bucket_label(key: &str) -> String {
    match key {
        arena_proto::CREDIT_BUCKET_0_7 => "0 to 7 days".to_owned(),
        arena_proto::CREDIT_BUCKET_8_14 => "8 to 14 days".to_owned(),
        arena_proto::CREDIT_BUCKET_15_PLUS => "15 days or more".to_owned(),
        other => label(other),
    }
}

fn amounts(title: &str, kind: &str, rows: &[arena_proto::ReportAmount]) -> ExportSection {
    let mut s = ExportSection::new(title, &[kind, "Amount", "Count"]);
    for r in rows {
        s.row(vec![
            t(sales_label(&r.key)),
            Cell::Money(r.amount_minor),
            Cell::Int(r.count),
        ]);
    }
    s
}

fn named(title: &str, kind: &str, rows: &[arena_proto::NamedSales]) -> ExportSection {
    let mut s = ExportSection::new(title, &[kind, "Amount", "Count"]);
    for r in rows {
        s.row(vec![
            t(&r.name),
            Cell::Money(r.amount_minor),
            Cell::Int(r.count),
        ]);
    }
    s
}

fn metrics(title: &str, rows: Vec<(&str, Cell)>) -> ExportSection {
    let mut s = ExportSection::new(title, &["Figure", "Value"]);
    for (name, value) in rows {
        s.row(vec![t(name), value]);
    }
    s
}

fn span(from: &str, to: &str) -> String {
    if from == to {
        from.to_owned()
    } else {
        format!("{from} to {to}")
    }
}

fn daily_sales_doc(r: &arena_proto::DailySales) -> ExportDoc {
    let mut hours = ExportSection::new("Sales by hour", &["Hour", "Amount", "Count"]);
    for h in &r.by_hour {
        hours.row(vec![
            t(format!("{:02}:00", h.hour)),
            Cell::Money(h.amount_minor),
            Cell::Int(h.count),
        ]);
    }
    ExportDoc {
        title: "Daily sales".to_owned(),
        range: span(&r.from, &r.to),
        slug: format!("daily-sales_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![
            metrics(
                "Summary",
                vec![
                    ("Orders", Cell::Int(r.orders)),
                    ("Sales", Cell::Money(r.sales_minor)),
                    ("Refunds", Cell::Money(r.refunds_minor)),
                    ("Net sales", Cell::Money(r.net_minor)),
                    ("Tax", Cell::Money(r.tax_minor)),
                ],
            ),
            amounts("Sales by type", "Type", &r.by_type),
            hours,
            named("Sales by device group", "Group", &r.by_group),
            named("Sales by product category", "Category", &r.by_category),
            amounts("Payments received, not sales", "Kind", &r.received),
        ],
    }
}

fn sessions_doc(r: &arena_proto::SessionsReport) -> ExportDoc {
    let mut groups = ExportSection::new(
        "By device group",
        &["Group", "Devices", "Sessions", "Minutes", "Revenue"],
    );
    for g in &r.by_group {
        groups.row(vec![
            t(&g.name),
            Cell::Int(g.devices),
            Cell::Int(g.sessions),
            Cell::Int(g.minutes),
            Cell::Money(g.revenue_minor),
        ]);
    }
    let mut devices =
        ExportSection::new("By device", &["Device", "Sessions", "Minutes", "Revenue"]);
    for d in &r.by_device {
        devices.row(vec![
            t(&d.label),
            Cell::Int(d.sessions),
            Cell::Int(d.minutes),
            Cell::Money(d.revenue_minor),
        ]);
    }
    ExportDoc {
        title: "Sessions".to_owned(),
        range: span(&r.from, &r.to),
        slug: format!("sessions_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![
            metrics(
                "Summary",
                vec![
                    ("Days", Cell::Int(r.days)),
                    ("Sessions", Cell::Int(r.sessions)),
                    ("Minutes played", Cell::Int(r.minutes)),
                    ("Revenue", Cell::Money(r.revenue_minor)),
                ],
            ),
            groups,
            devices,
        ],
    }
}

fn shifts_doc(r: &arena_proto::ShiftList) -> ExportDoc {
    let mut s = ExportSection::new(
        "Closed shifts",
        &[
            "Opened",
            "Closed",
            "Opened by",
            "Closed by",
            "Counted",
            "Variance",
        ],
    );
    for x in &r.shifts {
        s.row(vec![
            Cell::Time(x.opened_at),
            Cell::Time(x.closed_at),
            t(&x.opened_by),
            t(&x.closed_by),
            Cell::Money(x.counted_minor),
            Cell::Money(x.variance_minor),
        ]);
    }
    ExportDoc {
        title: "Shifts".to_owned(),
        range: format!("The last {} closed shifts", arena_proto::SHIFT_LIST_LIMIT),
        slug: "shifts".to_owned(),
        sections: vec![s],
    }
}

fn z_report_doc(z: &arena_proto::ZReport, f: &ExportFormatting) -> ExportDoc {
    let cash_lines = |title: &str, lines: &[arena_proto::ZCashLine]| {
        let mut s = ExportSection::new(
            title,
            &["Category", "Reference", "Note", "Paid from", "Amount"],
        );
        for l in lines {
            s.row(vec![
                t(label(&l.category)),
                t(&l.reference),
                t(&l.note),
                t(label(&l.paid_from)),
                Cell::Money(l.amount_minor),
            ]);
        }
        s
    };
    let mut references = ExportSection::new(
        "Payment references",
        &["Method", "Time", "Invoice", "Reference", "Amount"],
    );
    for g in &z.references {
        for r in &g.rows {
            references.row(vec![
                t(label(&g.method)),
                Cell::Time(r.at),
                t(&r.invoice_no),
                t(&r.reference),
                Cell::Money(r.amount_minor),
            ]);
        }
    }
    let mut payments = ExportSection::new("Payments by method", &["Method", "Amount", "Count"]);
    for p in &z.payments {
        payments.row(vec![
            t(label(&p.key)),
            Cell::Money(p.amount_minor),
            Cell::Int(p.count),
        ]);
    }
    let mut refunds = ExportSection::new("Refunds", &["Credit note", "Method", "Amount"]);
    for r in &z.refunds {
        refunds.row(vec![
            t(&r.credit_note_no),
            t(label(&r.method)),
            Cell::Money(r.amount_minor),
        ]);
    }
    let mut waste = ExportSection::new("Waste", &["Reason", "Quantity", "Value at cost"]);
    for w in &z.waste {
        waste.row(vec![
            t(waste_label(&w.reason)),
            Cell::Int(w.qty),
            Cell::Money(w.value_minor),
        ]);
    }
    let mut voids = ExportSection::new("Voided orders", &["Reason", "Count", "Amount"]);
    for v in &z.voids {
        voids.row(vec![
            t(label(&v.reason)),
            Cell::Int(v.count),
            Cell::Money(v.amount_minor),
        ]);
    }
    let mut cash = vec![
        ("Opened", Cell::Time(z.opened_at)),
        ("Closed", Cell::Time(z.closed_at)),
        ("Opened by", t(&z.opened_by)),
        ("Closed by", t(&z.closed_by)),
        ("Float", Cell::Money(z.float_minor)),
        ("Expected in drawer", Cell::Money(z.expected_minor)),
        ("Counted", Cell::Money(z.counted_minor)),
        ("Variance", Cell::Money(z.variance_minor)),
        ("Drawer opens", Cell::Int(z.drawer_opens)),
    ];
    if let Some(reason) = &z.variance_reason {
        cash.push(("Variance reason", t(label(reason))));
    }
    if let Some(note) = &z.variance_note {
        cash.push(("Variance note", t(note)));
    }
    if let Some(approver) = &z.approver {
        cash.push(("Approved by", t(approver)));
    }
    let when = f.local_time(z.closed_at);
    ExportDoc {
        title: "Z-report".to_owned(),
        range: format!("Shift closed {when}"),
        slug: format!("z-report_{}", z.shift_id),
        sections: vec![
            metrics("Shift", cash),
            metrics(
                "Sales",
                vec![
                    ("Orders", Cell::Int(z.orders)),
                    ("Sales", Cell::Money(z.sales_minor)),
                    ("Tax", Cell::Money(z.tax_minor)),
                    ("Refunds", Cell::Money(z.refunds_minor)),
                ],
            ),
            amounts("Sales by type", "Type", &z.by_type),
            payments,
            references,
            amounts("Payments received, not sales", "Kind", &z.received),
            refunds,
            waste,
            cash_lines("Expenses", &z.expenses),
            cash_lines("Cash deposits", &z.deposits),
            voids,
        ],
    }
}

fn players_doc(r: &arena_proto::PlayersReport) -> ExportDoc {
    let mut top = ExportSection::new("Top players", &["Player", "Visits", "Spend"]);
    for p in &r.top {
        top.row(vec![
            t(&p.name),
            Cell::Int(p.visits),
            Cell::Money(p.spend_minor),
        ]);
    }
    let mut buckets = ExportSection::new("Credit by age", &["Age", "Outstanding"]);
    for b in &r.credit_buckets {
        buckets.row(vec![
            t(credit_bucket_label(&b.key)),
            Cell::Money(b.amount_minor),
        ]);
    }
    let mut debtors = ExportSection::new(
        "Credit outstanding",
        &["Player", "Outstanding", "Oldest (days)"],
    );
    for d in &r.debtors {
        debtors.row(vec![
            t(&d.name),
            Cell::Money(d.outstanding_minor),
            Cell::Int(d.oldest_age_days),
        ]);
    }
    ExportDoc {
        title: "Players".to_owned(),
        range: span(&r.from, &r.to),
        slug: format!("players_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![
            top,
            {
                let mut s =
                    ExportSection::new("Guests and new players", &["Group", "Count", "Spend"]);
                s.row(vec![
                    t("Guest bills"),
                    Cell::Int(r.guest.bills),
                    Cell::Money(r.guest.spend_minor),
                ]);
                s.row(vec![
                    t("New players"),
                    Cell::Int(r.new_players.players),
                    Cell::Money(r.new_players.spend_minor),
                ]);
                s.row(vec![
                    t("Returning players"),
                    Cell::Int(r.returning_players.players),
                    Cell::Money(r.returning_players.spend_minor),
                ]);
                s
            },
            buckets,
            debtors,
            metrics(
                &format!("Credit as of {}", r.credit_as_of),
                vec![("Total outstanding", Cell::Money(r.credit_outstanding_minor))],
            ),
        ],
    }
}

fn staff_doc(r: &arena_proto::StaffReport) -> ExportDoc {
    let mut s = ExportSection::new(
        "Staff",
        &[
            "Staff",
            "Sales",
            "Sales count",
            "Sessions started",
            "Discounts",
            "Discounts count",
            "Voids",
            "Voids count",
            "Refunds",
            "Refunds count",
            "Cash variance",
        ],
    );
    for x in &r.staff {
        s.row(vec![
            t(&x.name),
            Cell::Money(x.sales_minor),
            Cell::Int(x.sales_count),
            Cell::Int(x.sessions_started),
            Cell::Money(x.discounts_minor),
            Cell::Int(x.discounts_count),
            Cell::Money(x.voids_minor),
            Cell::Int(x.voids_count),
            Cell::Money(x.refunds_minor),
            Cell::Int(x.refunds_count),
            Cell::Money(x.variance_minor),
        ]);
    }
    ExportDoc {
        title: "Staff".to_owned(),
        range: span(&r.from, &r.to),
        slug: format!("staff_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![s],
    }
}

fn stock_on_hand_doc(r: &arena_proto::StockReport, as_of: &str) -> ExportDoc {
    let mut s = ExportSection::new(
        "Stock on hand",
        &["Product", "Quantity", "Average cost", "Value"],
    );
    for i in &r.items {
        s.row(vec![
            t(&i.name),
            Cell::Int(i.qty),
            Cell::Money(i.avg_cost_minor),
            Cell::Money(i.value_minor),
        ]);
    }
    s.row(vec![
        t("Total"),
        t(""),
        t(""),
        Cell::Money(r.total_value_minor),
    ]);
    ExportDoc {
        title: "Stock on hand".to_owned(),
        range: format!("As of {as_of}"),
        slug: "stock-on-hand".to_owned(),
        sections: vec![s],
    }
}

fn stock_moves_doc(r: &arena_proto::StockMoves) -> ExportDoc {
    let mut s = ExportSection::new(
        "Stock movements",
        &[
            "Time",
            "Product",
            "Reason",
            "Waste reason",
            "Quantity",
            "Value at cost",
            "Staff",
        ],
    );
    for m in &r.moves {
        s.row(vec![
            Cell::Time(m.at),
            t(&m.product),
            t(label(&m.reason)),
            t(m.waste_reason
                .as_deref()
                .map(waste_label)
                .unwrap_or_default()),
            Cell::Int(m.qty),
            Cell::Money(m.value_minor),
            t(&m.staff),
        ]);
    }
    let shown = if r.truncated {
        format!(
            "{} (the newest {} of {} movements)",
            span(&r.from, &r.to),
            r.moves.len(),
            r.total
        )
    } else {
        span(&r.from, &r.to)
    };
    ExportDoc {
        title: "Stock movements".to_owned(),
        range: shown,
        slug: format!("stock-moves_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![s],
    }
}

fn stock_waste_doc(r: &arena_proto::WasteReport) -> ExportDoc {
    let mut reasons =
        ExportSection::new("Waste by reason", &["Reason", "Quantity", "Value at cost"]);
    for x in &r.by_reason {
        reasons.row(vec![
            t(waste_label(&x.reason)),
            Cell::Int(x.qty),
            Cell::Money(x.value_minor),
        ]);
    }
    reasons.row(vec![
        t("Total"),
        Cell::Int(r.total_qty),
        Cell::Money(r.total_value_minor),
    ]);
    let mut products = ExportSection::new(
        "Waste by product",
        &["Product", "Reason", "Quantity", "Value at cost"],
    );
    for p in &r.by_product {
        products.row(vec![
            t(&p.product),
            t("All reasons"),
            Cell::Int(p.qty),
            Cell::Money(p.value_minor),
        ]);
        for x in p
            .reasons
            .iter()
            .filter(|x| x.qty != 0 || x.value_minor != 0)
        {
            products.row(vec![
                t(&p.product),
                t(waste_label(&x.reason)),
                Cell::Int(x.qty),
                Cell::Money(x.value_minor),
            ]);
        }
    }
    ExportDoc {
        title: "Waste".to_owned(),
        range: span(&r.from, &r.to),
        slug: format!("stock-waste_{}", span(&r.from, &r.to).replace(" to ", "_")),
        sections: vec![reasons, products],
    }
}

fn stock_expiring_doc(v: &arena_proto::ExpiryView) -> ExportDoc {
    let lots = |title: &str, rows: &[arena_proto::ExpiringLot]| {
        let mut s = ExportSection::new(title, &["Product", "Quantity", "Expiry", "Value at cost"]);
        for l in rows {
            s.row(vec![
                t(&l.product),
                Cell::Int(l.qty),
                t(&l.expiry),
                Cell::Money(l.value_minor),
            ]);
        }
        s
    };
    ExportDoc {
        title: "Expiring stock".to_owned(),
        range: format!("As of {} (warning window {} days)", v.today, v.warn_days),
        slug: "stock-expiring".to_owned(),
        sections: vec![
            lots("Expiring soon", &v.expiring),
            lots("Expired, not yet written off", &v.expired),
        ],
    }
}

fn gst_doc(m: &arena_proto::GstMonth) -> ExportDoc {
    let mut s = ExportSection::new(
        "GSTR-1 sections",
        &[
            "Section",
            "Documents",
            "Taxable value",
            "CGST",
            "SGST",
            "IGST",
        ],
    );
    for x in &m.sections {
        let name = match x.key.as_str() {
            arena_proto::GST_B2B => "B2B invoices",
            arena_proto::GST_B2CS => "B2C small",
            arena_proto::GST_CDNR => "Credit notes",
            arena_proto::GST_HSN => "HSN summary (net)",
            other => other,
        };
        s.row(vec![
            t(name),
            Cell::Int(x.documents),
            Cell::Money(x.taxable_minor),
            Cell::Money(x.central_minor),
            Cell::Money(x.state_minor),
            Cell::Money(x.integrated_minor),
        ]);
    }
    ExportDoc {
        title: "GST return (GSTR-1)".to_owned(),
        range: m.month.clone(),
        slug: format!("gst_{}", m.month),
        sections: vec![
            s,
            metrics(
                "Documents",
                vec![
                    ("Invoices", Cell::Int(m.invoices)),
                    ("Credit notes", Cell::Int(m.credit_notes)),
                    ("Invoices with no tax recorded", Cell::Int(m.untaxed)),
                    ("Sales not yet numbered", Cell::Int(m.unnumbered)),
                ],
            ),
        ],
    }
}

fn need<'a>(value: &'a Option<String>, field: &str) -> Result<&'a str, AppError> {
    value.as_deref().filter(|v| !v.is_empty()).ok_or_else(|| {
        AppError::with_field(ErrorCode::ValidationFailed, field, "This is required.")
    })
}

/// A file name part of letters, digits, `-`, `_` and `.` only.
fn safe(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

impl SnapshotReader {
    /// The report `req` names, as a document ready to be written, with the branch's formatting.
    /// `VALIDATION_FAILED` on `report`, on a missing or bad range, month or shift. Never writes.
    pub fn export_doc(
        &self,
        req: &ReportExportRequest,
        now_ms: i64,
        kek: Option<&[u8; KEK_LEN]>,
    ) -> Result<(ExportDoc, ExportFormatting), AppError> {
        let branch = self.branch()?;
        let locale = self.locale(now_ms)?;
        let f = ExportFormatting::from_branch(
            branch.as_ref().map(|b| b.name.clone()).unwrap_or_default(),
            branch
                .as_ref()
                .map(|b| b.branch_code.clone())
                .unwrap_or_default(),
            locale.as_ref(),
        );
        let range = || -> Result<(&str, &str), AppError> {
            Ok((need(&req.from, "from")?, need(&req.to, "to")?))
        };
        let doc = match req.report.as_str() {
            "daily-sales" => {
                let (from, to) = range()?;
                daily_sales_doc(&self.daily_sales(from, to, now_ms)?)
            }
            "sessions" => {
                let (from, to) = range()?;
                sessions_doc(&self.sessions_report(from, to, now_ms)?)
            }
            "shifts" => shifts_doc(&self.shift_list()?),
            "z-report" => z_report_doc(
                &self.z_report(need(&req.shift_id, "shift_id")?, now_ms)?,
                &f,
            ),
            "players" => {
                let (from, to) = range()?;
                let kek = kek.ok_or_else(|| {
                    AppError::new(ErrorCode::Unsupported, "this hub has no key store")
                })?;
                players_doc(&self.players_report(from, to, now_ms, kek)?)
            }
            "staff" => {
                let (from, to) = range()?;
                staff_doc(&self.staff_report(from, to)?)
            }
            "stock-on-hand" => stock_on_hand_doc(&self.stock_report()?, &f.local_time(now_ms)),
            "stock-moves" => {
                let (from, to) = range()?;
                let keep = |v: &Option<String>| v.clone().filter(|v| !v.is_empty());
                let moves = self.stock_moves(
                    from,
                    to,
                    keep(&req.product).as_deref(),
                    keep(&req.reason).as_deref(),
                )?;
                stock_moves_doc(&moves)
            }
            "stock-waste" => {
                let (from, to) = range()?;
                stock_waste_doc(&self.waste_report(from, to)?)
            }
            "stock-expiring" => stock_expiring_doc(&self.expiry(now_ms)?),
            "gst" => gst_doc(&self.gst_month(need(&req.month, "month")?)?),
            _ => {
                return Err(AppError::with_field(
                    ErrorCode::ValidationFailed,
                    "report",
                    "This report cannot be exported.",
                ));
            }
        };
        Ok((doc, f))
    }

    /// The file for `req`: its name and bytes. The name is `<branch code>_<report>_<range>.<ext>`.
    pub fn export_report(
        &self,
        req: &ReportExportRequest,
        now_ms: i64,
        kek: Option<&[u8; KEK_LEN]>,
    ) -> Result<(String, Vec<u8>), AppError> {
        let (doc, f) = self.export_doc(req, now_ms, kek)?;
        let bytes = match req.format {
            ExportFormat::Csv => to_csv(&doc, &f),
            ExportFormat::Pdf => to_pdf(&doc, &f, now_ms)?,
        };
        let code = if f.branch_code.is_empty() {
            "arena".to_owned()
        } else {
            safe(&f.branch_code)
        };
        let name = format!("{code}_{}.{}", safe(&doc.slug), req.format.extension());
        Ok((name, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> ExportFormatting {
        ExportFormatting {
            branch_name: "Arena".to_owned(),
            branch_code: "KOR".to_owned(),
            currency: "INR".to_owned(),
            exponent: 2,
            number_format: NumberFormat::Indian,
            tz: Tz::Asia__Kolkata,
        }
    }

    fn sample() -> ExportDoc {
        let mut sales = ExportSection::new("Sales by type", &["Type", "Amount", "Share"]);
        sales.row(vec![
            t("Play time"),
            Cell::Money(12_345_678),
            Cell::Bp(6_250),
        ]);
        sales.row(vec![t("=SUM(A1)"), Cell::Money(-5), Cell::Bp(10_000)]);
        sales.row(vec![t("Rock, \"paper\""), Cell::Money(0), Cell::Bp(33)]);
        let mut count = ExportSection::new("Counts", &["When", "Orders"]);
        count.row(vec![Cell::Time(1_793_000_000_000), Cell::Int(7)]);
        ExportDoc {
            title: "Sample".to_owned(),
            range: "2026-10-01 to 2026-10-31".to_owned(),
            slug: "sample".to_owned(),
            sections: vec![sales, count],
        }
    }

    #[test]
    fn money_is_written_in_major_units_with_the_currency_code() {
        let csv = String::from_utf8(to_csv(&sample(), &fmt())).unwrap();
        assert!(csv.starts_with('\u{FEFF}'), "UTF-8 byte-order mark");
        let lines: Vec<&str> = csv.trim_start_matches('\u{FEFF}').split("\r\n").collect();
        assert_eq!(lines[0], "Sales by type");
        assert_eq!(lines[1], "Type,Amount,Share,Currency");
        assert_eq!(lines[2], "Play time,123456.78,62.5,INR");
        assert_eq!(lines[3], "'=SUM(A1),-0.05,100,INR");
        assert_eq!(lines[4], "\"Rock, \"\"paper\"\"\",0.00,0.3,INR");
    }

    #[test]
    fn a_section_without_money_has_no_currency_column_and_times_are_local() {
        let csv = String::from_utf8(to_csv(&sample(), &fmt())).unwrap();
        let mut tail: Vec<&str> = csv.trim_end().split("\r\n").collect();
        tail.reverse();
        assert_eq!(
            tail[0],
            format!("{},7", fmt().local_time(1_793_000_000_000))
        );
        assert_eq!(tail[1], "When,Orders");
        assert_eq!(tail[2], "Counts");
        assert!(fmt().local_time(0).starts_with("1970-01-01 05:30"));
    }

    #[test]
    fn the_pdf_shows_the_same_figures_as_the_csv_in_the_branch_grouping() {
        let doc = sample();
        let pdf = to_pdf(&doc, &fmt(), 1_793_000_000_000).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        let font = pdf::Font::sora().unwrap();
        for shown in [
            "1,23,456.78",
            "62.5%",
            "-0.05",
            "Arena",
            "Sample",
            "2026-10-01 to 2026-10-31",
        ] {
            assert!(
                text.contains(&format!("<{}>", font.encode(shown))),
                "{shown}"
            );
        }
        assert!(text.contains(&format!(
            "<{}>",
            font.encode("Sales by type (amounts in INR)")
        )));
        // The same minor-unit figure is the CSV's 123456.78 and the PDF's 1,23,456.78.
        assert_eq!(fmt().plain_money(12_345_678), "123456.78");
        assert_eq!(fmt().grouped_money(12_345_678), "1,23,456.78");
    }

    #[test]
    fn a_zero_exponent_currency_has_no_decimals() {
        let mut f = fmt();
        f.exponent = 0;
        f.currency = "JPY".to_owned();
        assert_eq!(f.plain_money(1500), "1500");
        assert_eq!(f.plain_money(-7), "-7");
    }

    #[test]
    fn labels_read_as_words() {
        assert_eq!(label("cancelled_order"), "Cancelled order");
        assert_eq!(waste_label("expired"), "Expired write-off");
        assert_eq!(sales_label("tax_added"), "Tax added");
        assert_eq!(ExportFormatting::percent(6_250), "62.5");
        assert_eq!(ExportFormatting::percent(0), "0");
    }
}

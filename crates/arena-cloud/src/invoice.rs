//! GST tax invoices for subscription charges (Story 10.7). A `subscription.charged` event issues one
//! immutable invoice in the same transaction as the event ([`draft`] builds it, the webhook
//! writes it), numbered `A360/<FY>/<seq:06>` per Indian financial year (Apr-Mar, Asia/Kolkata) and
//! priced from the subscription's own locked price book. The `invoice-render` jobs task then
//! writes each invoice's PDF under `billing/invoices/` in the data bucket (never a branch
//! prefix) and tells the owner "Invoice ready" once, through the owner-alert path.
//!
//! GST is computed once, on the subtotal, as the quote does (18%, half-up to paise). On band IN it
//! is CGST + SGST (the tax split in two, the odd paisa to SGST) when the place of supply is
//! Arena 360's state, else IGST; a branch with no GSTIN has no state on record, so the place of
//! supply is the supplier's state (the IGST Act default). The MY, SG and US bands are zero-rated
//! exports in the band's currency with no GST lines.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::OnceLock;
use std::time::Duration;

use arena_proto::{AddOns, Band, BillingCycle, OwnerAlertKind, PriceItem, Tier};
use arena_store::control::{
    ControlDb, ControlError, Invoice, NewInvoice, PriceBookItem, Subscription,
};
use chrono::{Datelike, FixedOffset, Months};
use serde::{Deserialize, Serialize};

use crate::auth::AuthState;
use crate::backup::{AlertTarget, JobError, code, open, report_failure};
use crate::config::{Database, JobConfig, Task, get_env};
use crate::gstin;
use crate::owner_alerts::{self, Raise};
use crate::store::{ObjectStore, S3Store};
use crate::subscription::{QuoteError, quote_items, renews_at};

/// The bucket prefix every invoice PDF lives under.
pub const PDF_PREFIX: &str = "billing/invoices/";

/// The service accounting code of the subscription (online information and database access).
pub const SAC: &str = "998314";

/// What an export invoice says in place of GST lines.
pub const LUT_NOTE: &str = "Export of services - zero-rated under LUT";

/// The seller's legal name, address and GSTIN, set together in production.
pub const SELLER_NAME_ENV: &str = "ARENA_INVOICE_SELLER_NAME";
pub const SELLER_ADDRESS_ENV: &str = "ARENA_INVOICE_SELLER_ADDRESS";
pub const SELLER_GSTIN_ENV: &str = "ARENA_INVOICE_SELLER_GSTIN";

/// A run gives up (and alerts) after this long, inside the five-minute schedule.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(240);

/// How many invoices one run finishes.
const BATCH: u32 = 200;

/// India Standard Time, which has no daylight saving: UTC+05:30.
const IST_SECONDS: i32 = 5 * 3600 + 1800;

/// The financial year (Apr-Mar, Asia/Kolkata) that contains `at_ms`, as `2026-27`.
pub fn fy_of(at_ms: i64) -> String {
    let ist = FixedOffset::east_opt(IST_SECONDS).expect("UTC+05:30 is a valid offset");
    let at = chrono::DateTime::from_timestamp_millis(at_ms)
        .unwrap_or_default()
        .with_timezone(&ist);
    let start = if at.month() >= 4 {
        at.year()
    } else {
        at.year() - 1
    };
    format!("{start}-{:02}", (start + 1).rem_euclid(100))
}

/// Who issues the invoices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seller {
    pub name: String,
    pub address: String,
    pub gstin: String,
}

impl Seller {
    /// The development seller, used when `ARENA_INVOICE_SELLER_GSTIN` is not set (a Maharashtra
    /// GSTIN, state 27). Production sets all three variables.
    pub fn dev() -> Self {
        Self {
            name: "Arena 360 (development seller)".to_owned(),
            address: "Mumbai, Maharashtra, India".to_owned(),
            gstin: "27AAACR5055K1Z7".to_owned(),
        }
    }

    /// The seller's state code: the first two digits of its GSTIN.
    pub fn state_code(&self) -> &str {
        self.gstin.get(..2).unwrap_or("")
    }

    /// The seller from the environment; `None` when `ARENA_INVOICE_SELLER_GSTIN` is not set. A
    /// GSTIN that is set must be valid and come with a name and an address; errors name the
    /// variable.
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Option<Self>, String> {
        let Some(raw) = get_env(env, SELLER_GSTIN_ENV).filter(|v| !v.trim().is_empty()) else {
            return Ok(None);
        };
        let gstin = gstin::normalize(&raw);
        if !gstin::valid_gstin(&gstin) {
            return Err(format!("{SELLER_GSTIN_ENV} is not a valid GSTIN"));
        }
        let need = |name: &str| {
            get_env(env, name)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{name} is not set"))
        };
        Ok(Some(Self {
            name: need(SELLER_NAME_ENV)?,
            address: need(SELLER_ADDRESS_ENV)?,
            gstin,
        }))
    }
}

/// The process's seller, read once. Without `ARENA_INVOICE_SELLER_GSTIN` it is the development
/// seller and an error is logged; a wrong configuration is an error, so no invoice is issued and
/// the webhook is retried until it is fixed.
pub fn seller() -> Result<&'static Seller, String> {
    static SELLER: OnceLock<Result<Seller, String>> = OnceLock::new();
    SELLER
        .get_or_init(|| match Seller::from_env(&|k| std::env::var_os(k)) {
            Ok(Some(s)) => Ok(s),
            Ok(None) => {
                tracing::error!(
                    "{SELLER_GSTIN_ENV} is not set: invoices carry the development seller"
                );
                Ok(Seller::dev())
            }
            Err(e) => Err(e),
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// One invoice line, as `billing_invoices.lines_json` stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub description: String,
    pub quantity: u32,
    pub unit_minor: i64,
    pub amount_minor: i64,
}

/// What an invoice is made from.
#[derive(Debug, Clone)]
pub struct DraftInput<'a> {
    /// The new invoice's id.
    pub id: String,
    pub seller: &'a Seller,
    pub sub: &'a Subscription,
    /// The rows of the subscription's own price book.
    pub items: &'a [PriceBookItem],
    /// The buyer's name (the tenant).
    pub buyer_name: &'a str,
    /// The branch's GSTIN, when it is GST-registered.
    pub buyer_gstin: Option<&'a str>,
    /// The end of the period this charge pays for, when known.
    pub period_end: Option<i64>,
    /// Issue time, epoch ms.
    pub now: i64,
}

fn tier_item(tier: Tier) -> Result<PriceItem, QuoteError> {
    match tier {
        Tier::Basic => Ok(PriceItem::Basic),
        Tier::Premium => Ok(PriceItem::Premium),
        Tier::Free => Err(QuoteError::NoPrice("free".to_owned())),
    }
}

fn describe(item: PriceItem, cycle: BillingCycle) -> String {
    let cycle = match cycle {
        BillingCycle::Monthly => "monthly",
        BillingCycle::Yearly => "yearly",
    };
    match item {
        PriceItem::Basic => format!("Arena 360 Basic plan ({cycle})"),
        PriceItem::Premium => format!("Arena 360 Premium plan ({cycle})"),
        PriceItem::AddonDevices20 => format!("Additional devices, pack of 20 ({cycle})"),
        PriceItem::AddonTunnel => format!("Tunnel add-on ({cycle})"),
        PriceItem::AddonOwnerLogin => format!("Owner login add-on ({cycle})"),
    }
}

/// The period a charge pays for: ending at `end` (a cycle after `now` when unknown) and starting
/// one cycle before.
fn period(now: i64, end: Option<i64>, cycle: BillingCycle) -> (i64, i64) {
    let end = end.unwrap_or_else(|| renews_at(now, cycle));
    let months = match cycle {
        BillingCycle::Monthly => 1,
        BillingCycle::Yearly => 12,
    };
    let start = chrono::DateTime::from_timestamp_millis(end)
        .and_then(|e| e.checked_sub_months(Months::new(months)))
        .map_or(end, |s| s.timestamp_millis());
    (start.min(end), end)
}

/// The state code of a valid GSTIN.
fn state_of(gstin: &str) -> Option<String> {
    gstin::valid_gstin(gstin).then(|| gstin[..2].to_owned())
}

/// Builds the invoice of one charge of `input.sub`, priced from its locked price book. A missing
/// price is an error and nothing is issued.
pub fn draft(input: &DraftInput<'_>) -> Result<NewInvoice, QuoteError> {
    let sub = input.sub;
    let tier = tier_item(sub.tier)?;
    let add_ons = AddOns {
        devices_20: sub.addon_devices_20,
        tunnel: sub.addon_tunnel,
        owner_login: sub.addon_owner_login,
    };
    let quote = quote_items(input.items, sub.band, tier, sub.cycle, &add_ons)?;

    let mut lines = Vec::new();
    for (item, quantity) in [
        (tier, 1),
        (PriceItem::AddonDevices20, add_ons.devices_20),
        (PriceItem::AddonTunnel, add_ons.tunnel),
        (PriceItem::AddonOwnerLogin, add_ons.owner_login),
    ] {
        if quantity == 0 {
            continue;
        }
        let unit = input
            .items
            .iter()
            .find(|i| i.band == sub.band && i.item == item && i.cycle == sub.cycle)
            .ok_or_else(|| QuoteError::NoPrice(item.as_str().to_owned()))?
            .amount_minor;
        lines.push(Line {
            description: describe(item, sub.cycle),
            quantity,
            unit_minor: unit,
            amount_minor: unit * i64::from(quantity),
        });
    }
    let subtotal: i64 = lines.iter().map(|l| l.amount_minor).sum();
    if subtotal != quote.amount_minor {
        return Err(QuoteError::NoPrice("the lines do not add up".to_owned()));
    }

    let (mut cgst, mut sgst, mut igst) = (0, 0, 0);
    let (mut buyer_gstin, mut buyer_state, mut note) = (None, None, None);
    if sub.band.gst_applicable() {
        buyer_gstin = input
            .buyer_gstin
            .map(gstin::normalize)
            .filter(|g| gstin::valid_gstin(g));
        let state = buyer_gstin
            .as_deref()
            .and_then(state_of)
            .unwrap_or_else(|| input.seller.state_code().to_owned());
        if state == input.seller.state_code() {
            cgst = quote.tax_minor / 2;
            sgst = quote.tax_minor - cgst;
        } else {
            igst = quote.tax_minor;
        }
        buyer_state = Some(state);
    } else {
        note = Some(LUT_NOTE.to_owned());
    }

    let (period_start, period_end) = period(input.now, input.period_end, sub.cycle);
    Ok(NewInvoice {
        id: input.id.clone(),
        fy: fy_of(input.now),
        tenant_id: sub.tenant_id.clone(),
        branch_id: sub.branch_id.clone(),
        subscription_id: sub.id.clone(),
        issued_at: input.now,
        period_start,
        period_end,
        band: sub.band.as_str().to_owned(),
        currency: quote.currency,
        lines_json: serde_json::to_string(&lines).unwrap_or_default(),
        subtotal_minor: subtotal,
        cgst_minor: cgst,
        sgst_minor: sgst,
        igst_minor: igst,
        total_minor: subtotal + cgst + sgst + igst,
        sac: SAC.to_owned(),
        buyer_name: input.buyer_name.to_owned(),
        buyer_gstin,
        buyer_state_code: buyer_state,
        seller_name: input.seller.name.clone(),
        seller_address: input.seller.address.clone(),
        seller_gstin: input.seller.gstin.clone(),
        note,
    })
}

/// The invoice of one charge of `sub`, read from the data it is made of: the subscription's own
/// price book, its branch (GSTIN) and tenant (name). Errors leave nothing written.
pub async fn for_charge(
    db: &ControlDb,
    sub: &Subscription,
    period_end: Option<i64>,
    now: i64,
) -> Result<NewInvoice, ControlError> {
    let seller = seller().map_err(ControlError::Db)?;
    let items = db.price_book_items(&sub.price_book_id).await?;
    let branch = db
        .branch(&sub.branch_id)
        .await?
        .ok_or_else(|| ControlError::Db(format!("branch {} is gone", sub.branch_id)))?;
    let tenant = db
        .tenant(&sub.tenant_id)
        .await?
        .ok_or_else(|| ControlError::Db(format!("tenant {} is gone", sub.tenant_id)))?;
    let buyer_gstin = branch.gstin.as_deref().filter(|_| branch.gst_registered);
    draft(&DraftInput {
        id: ulid::Ulid::generate().to_string(),
        seller,
        sub,
        items: &items,
        buyer_name: &tenant.name,
        buyer_gstin,
        period_end,
        now,
    })
    .map_err(|e| match e {
        QuoteError::NoPrice(m) => ControlError::Db(format!(
            "no invoice for subscription {}: the price book has no price ({m})",
            sub.id
        )),
        QuoteError::Db(m) => ControlError::Db(m),
    })
}

/// `minor` units of `currency` as `1180.00`.
pub fn money(minor: i64) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// An amount with its currency, for an alert: `Rs 1180.00` or `MYR 99.00`.
pub fn money_with_currency(minor: i64, currency: &str) -> String {
    let unit = if currency == "INR" { "Rs" } else { currency };
    format!("{unit} {}", money(minor))
}

fn day(ms: i64) -> String {
    let ist = FixedOffset::east_opt(IST_SECONDS).expect("UTC+05:30 is a valid offset");
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&ist)
        .format("%d %b %Y")
        .to_string()
}

/// The key the PDF of `inv` is stored under: `billing/invoices/2026-27/A360-2026-27-000001.pdf`.
pub fn pdf_key(inv: &Invoice) -> String {
    format!(
        "{PDF_PREFIX}{}/{}.pdf",
        inv.fy,
        inv.number.replace('/', "-")
    )
}

const WIDTH: usize = 78;

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// The invoice as printable text lines in a monospaced layout.
fn layout(inv: &Invoice) -> Vec<String> {
    let lines: Vec<Line> = serde_json::from_str(&inv.lines_json).unwrap_or_default();
    let export = inv.band != Band::In.as_str();
    let mut out = vec![
        if export { "INVOICE" } else { "TAX INVOICE" }.to_owned(),
        "-".repeat(WIDTH),
        inv.seller_name.clone(),
    ];
    out.extend(wrap(&inv.seller_address, WIDTH));
    out.push(format!("GSTIN: {}", inv.seller_gstin));
    out.push(String::new());
    out.push(format!("Invoice no : {}", inv.number));
    out.push(format!("Date       : {}", day(inv.issued_at)));
    out.push(format!(
        "Period     : {} to {}",
        day(inv.period_start),
        day(inv.period_end)
    ));
    out.push(format!("SAC        : {}", inv.sac));
    out.push(String::new());
    out.push(format!("Bill to    : {}", inv.buyer_name));
    if !export {
        out.push(format!(
            "GSTIN      : {}",
            inv.buyer_gstin.as_deref().unwrap_or("Unregistered")
        ));
        if let Some(state) = &inv.buyer_state_code {
            out.push(format!("Place of supply (state code): {state}"));
        }
    }
    out.push(String::new());
    out.push(format!(
        "{:<42}{:>5}{:>15}{:>16}",
        "Description", "Qty", "Rate", "Amount"
    ));
    out.push("-".repeat(WIDTH));
    for l in &lines {
        let description: String = l.description.chars().take(41).collect();
        out.push(format!(
            "{:<42}{:>5}{:>15}{:>16}",
            description,
            l.quantity,
            money(l.unit_minor),
            money(l.amount_minor)
        ));
    }
    out.push("-".repeat(WIDTH));
    let total_line = |label: &str, minor: i64| format!("{label:>61}{:>17}", money(minor));
    out.push(total_line("Subtotal", inv.subtotal_minor));
    if !export {
        let half = crate::subscription::GST_PERCENT / 2;
        if inv.igst_minor > 0 {
            out.push(total_line(
                &format!("IGST @ {}%", crate::subscription::GST_PERCENT),
                inv.igst_minor,
            ));
        } else {
            out.push(total_line(&format!("CGST @ {half}%"), inv.cgst_minor));
            out.push(total_line(&format!("SGST @ {half}%"), inv.sgst_minor));
        }
    }
    out.push(total_line(
        &format!("Total ({})", inv.currency),
        inv.total_minor,
    ));
    if let Some(note) = &inv.note {
        out.push(String::new());
        out.extend(wrap(note, WIDTH));
    }
    out.push(String::new());
    out.push("This is a computer generated invoice.".to_owned());
    out
}

fn pdf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// Lines of text per PDF page.
const PAGE_LINES: usize = 60;

/// A minimal PDF 1.4 document: A4 pages of Courier 10 pt text, one line per entry.
pub fn pdf_of_lines(lines: &[String]) -> Vec<u8> {
    let pages: Vec<&[String]> = if lines.is_empty() {
        vec![&[]]
    } else {
        lines.chunks(PAGE_LINES).collect()
    };
    // Objects: 1 catalog, 2 pages, 3 font, then a page and its content stream per page.
    let mut objects: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + 2 * i))
        .collect();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned());
    for (i, page) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        let mut content = String::from("BT\n/F1 10 Tf\n12 TL\n50 800 Td\n");
        for line in *page {
            content.push_str(&format!("({}) Tj T*\n", pdf_text(line)));
        }
        content.push_str("ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The PDF of an issued invoice.
pub fn pdf_of(inv: &Invoice) -> Vec<u8> {
    pdf_of_lines(&layout(inv))
}

/// What one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderOutcome {
    /// PDFs written.
    pub rendered: u64,
    /// "Invoice ready" alerts raised.
    pub notified: u64,
    /// Invoices left for the next run (a write or an alert failed, or the branch is not entitled
    /// yet).
    pub failed: u64,
}

/// Finishes each invoice that has no PDF yet or whose owner has not been told: writes the PDF to
/// `store`, then raises one "Invoice ready" alert (its id is the invoice's, so a crash between the
/// alert and the mark never sends it twice). A failure leaves the invoice for the next run.
pub async fn render_once<S: ObjectStore>(
    st: &AuthState,
    store: &S,
) -> Result<RenderOutcome, JobError> {
    let failed = |e: arena_store::control::ControlError| {
        JobError::new(code::INVOICE_RENDER_FAILED, e.to_string())
    };
    let now = st.now();
    let mut out = RenderOutcome::default();
    for inv in st.db.invoices_to_render(BATCH).await.map_err(failed)? {
        let mut pdf_key_now = inv.pdf_key.clone();
        if pdf_key_now.is_none() {
            let key = pdf_key(&inv);
            if let Err(e) = store.put(&key, pdf_of(&inv)).await {
                tracing::error!(invoice_id = %inv.id, error = %e, "the invoice PDF was not written");
                out.failed += 1;
                continue;
            }
            st.db
                .set_invoice_pdf(&inv.id, &key, now)
                .await
                .map_err(failed)?;
            out.rendered += 1;
            pdf_key_now = Some(key);
        }
        if inv.notified_at.is_some() || pdf_key_now.is_none() {
            continue;
        }
        let Some(branch) = st.db.branch(&inv.branch_id).await.map_err(failed)? else {
            tracing::error!(invoice_id = %inv.id, "the invoice's branch is gone");
            out.failed += 1;
            continue;
        };
        let params = BTreeMap::from([
            ("number".to_owned(), inv.number.clone()),
            (
                "amount".to_owned(),
                money_with_currency(inv.total_minor, &inv.currency),
            ),
        ]);
        let id = format!("inv-{}", inv.id);
        match owner_alerts::raise(
            st,
            Raise {
                id: Some(&id),
                branch: &branch,
                kind: OwnerAlertKind::InvoiceReady,
                params: &params,
                whatsapp: true,
            },
        )
        .await
        {
            Ok(_) => {
                st.db
                    .mark_invoice_notified(&inv.id, now)
                    .await
                    .map_err(failed)?;
                out.notified += 1;
            }
            Err(e) => {
                tracing::warn!(
                    invoice_id = %inv.id,
                    code = ?e.body().code,
                    "the owner was not told about the invoice; retrying next run"
                );
                out.failed += 1;
            }
        }
    }
    tracing::info!(
        task = Task::InvoiceRender.as_str(),
        rendered = out.rendered,
        notified = out.notified,
        failed = out.failed,
        "invoices rendered"
    );
    Ok(out)
}

/// `--task invoice-render`: opens `gz_control` (no migration), builds the same [`AuthState`] as the
/// web role and the data bucket's store, runs once and reports a failure (including a timeout)
/// through the alert path. Returns whether it succeeded.
pub async fn run_job(
    job: &JobConfig,
    env: &impl Fn(&str) -> Option<OsString>,
    target: &AlertTarget,
) -> bool {
    let run = async {
        let dev = job.dev_data_dir.is_some();
        let database = match &job.dev_data_dir {
            Some(dir) => Database::Local {
                data_dir: dir.clone(),
            },
            None => crate::config::remote_database(env)
                .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?,
        };
        let cfg = crate::config::auth_from_env(dev, env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        let s3 = crate::presign::data_s3_config(dev, env)
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?
            .ok_or_else(|| {
                JobError::new(
                    code::CONFIG_INVALID,
                    format!("{}_ENDPOINT is not set", crate::presign::DATA_S3_PREFIX),
                )
            })?;
        let db = open(&database)
            .await
            .map_err(|e| JobError::new(code::INVOICE_RENDER_FAILED, e.to_string()))?;
        let st = crate::auth_state(std::sync::Arc::new(db), &cfg, crate::system_clock())
            .map_err(|e| JobError::new(code::CONFIG_INVALID, e))?;
        render_once(&st, &S3Store::new(&s3)).await
    };
    let res = match tokio::time::timeout(RUN_TIMEOUT, run).await {
        Ok(r) => r,
        Err(_) => Err(JobError::new(
            code::INVOICE_RENDER_TIMEOUT,
            format!(
                "the render did not finish within {} s",
                RUN_TIMEOUT.as_secs()
            ),
        )),
    };
    match res {
        Ok(_) => true,
        Err(e) => {
            report_failure(Task::InvoiceRender, &e, target).await;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::SubscriptionState;

    fn item(band: Band, item: PriceItem, cycle: BillingCycle, amount: i64) -> PriceBookItem {
        PriceBookItem {
            band,
            currency: match band {
                Band::In => "INR",
                Band::My => "MYR",
                Band::Sg => "SGD",
                Band::Us => "USD",
            }
            .to_owned(),
            item,
            cycle,
            amount_minor: amount,
            gst_applicable: band.gst_applicable(),
            razorpay_plan_id: None,
        }
    }

    fn book() -> Vec<PriceBookItem> {
        let mut v = Vec::new();
        for band in Band::ALL {
            for (it, m) in [
                (PriceItem::Basic, 100_000),
                (PriceItem::Premium, 300_000),
                (PriceItem::AddonDevices20, 20_005),
                (PriceItem::AddonTunnel, 50_000),
                (PriceItem::AddonOwnerLogin, 10_000),
            ] {
                v.push(item(band, it, BillingCycle::Monthly, m));
                v.push(item(band, it, BillingCycle::Yearly, m * 10));
            }
        }
        v
    }

    fn sub(band: Band) -> Subscription {
        Subscription {
            id: "sb_1".to_owned(),
            tenant_id: "t1".to_owned(),
            branch_id: "b1".to_owned(),
            state: SubscriptionState::Active,
            tier: Tier::Basic,
            cycle: BillingCycle::Monthly,
            band,
            currency: "INR".to_owned(),
            price_book_id: "pb_v1".to_owned(),
            addon_devices_20: 0,
            addon_tunnel: 0,
            addon_owner_login: 0,
            mandate_method: None,
            mandate_detail: None,
            razorpay_subscription_id: Some("sub_1".to_owned()),
            rev: 1,
            current_period_end: None,
            grace_until: None,
            first_paid_at: None,
            cancel_at_period_end: false,
            created_at: 0,
            updated_at: 0,
            payment_country: None,
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    fn make(sub: &Subscription, buyer_gstin: Option<&str>) -> Result<NewInvoice, QuoteError> {
        let seller = Seller::dev();
        let items = book();
        draft(&DraftInput {
            id: "inv1".to_owned(),
            seller: &seller,
            sub,
            items: &items,
            buyer_name: "Pixel Arena",
            buyer_gstin,
            period_end: Some(NOW + 30 * 86_400_000),
            now: NOW,
        })
    }

    #[test]
    fn the_financial_year_turns_over_at_midnight_ist_on_1_april() {
        // 31 Mar 2026 23:59:59.999 IST is 18:29:59.999 UTC; 1 Apr 00:00 IST is 18:30 UTC.
        let utc = |s: &str| {
            chrono::DateTime::parse_from_rfc3339(s)
                .unwrap()
                .timestamp_millis()
        };
        assert_eq!(fy_of(utc("2026-03-31T18:29:59.999Z")), "2025-26");
        assert_eq!(fy_of(utc("2026-03-31T18:30:00.000Z")), "2026-27");
        assert_eq!(fy_of(utc("2026-04-01T00:00:00Z")), "2026-27");
        assert_eq!(fy_of(utc("2027-03-31T18:29:59Z")), "2026-27");
        assert_eq!(fy_of(utc("2099-12-31T00:00:00Z")), "2099-00");
        assert_eq!(fy_of(utc("2026-01-01T00:00:00Z")), "2025-26");
    }

    #[test]
    fn the_same_state_splits_the_tax_into_cgst_and_sgst() {
        let inv = make(&sub(Band::In), Some("27AAPFU0939F1ZV")).unwrap();
        assert_eq!(inv.subtotal_minor, 100_000);
        assert_eq!(
            (inv.cgst_minor, inv.sgst_minor, inv.igst_minor),
            (9_000, 9_000, 0)
        );
        assert_eq!(inv.total_minor, 118_000);
        assert_eq!(inv.buyer_gstin.as_deref(), Some("27AAPFU0939F1ZV"));
        assert_eq!(inv.buyer_state_code.as_deref(), Some("27"));
        assert_eq!(inv.fy, "2026-27");
        assert_eq!(inv.sac, "998314");
        assert_eq!(inv.note, None);
        assert_eq!(inv.currency, "INR");
    }

    #[test]
    fn another_state_is_igst_and_no_gstin_falls_to_the_sellers_state() {
        let other = make(&sub(Band::In), Some("29AAGCB7383J1Z4")).unwrap();
        assert_eq!(
            (other.cgst_minor, other.sgst_minor, other.igst_minor),
            (0, 0, 18_000)
        );
        assert_eq!(other.buyer_state_code.as_deref(), Some("29"));
        assert_eq!(other.total_minor, 118_000);
        let none = make(&sub(Band::In), None).unwrap();
        assert_eq!(
            (none.cgst_minor, none.sgst_minor, none.igst_minor),
            (9_000, 9_000, 0)
        );
        assert_eq!(none.buyer_gstin, None);
        assert_eq!(none.buyer_state_code.as_deref(), Some("27"));
        // A malformed GSTIN is not a registration.
        let bad = make(&sub(Band::In), Some("27AAPFU0939F1ZX")).unwrap();
        assert_eq!(bad.buyer_gstin, None);
    }

    #[test]
    fn the_odd_paisa_goes_to_sgst_and_the_pair_equals_the_tax() {
        let s = sub(Band::In);
        let seller = Seller::dev();
        let mut odd = book();
        odd.retain(|i| i.item != PriceItem::Basic);
        odd.push(item(Band::In, PriceItem::Basic, BillingCycle::Monthly, 105));
        let inv = draft(&DraftInput {
            id: "i".to_owned(),
            seller: &seller,
            sub: &s,
            items: &odd,
            buyer_name: "x",
            buyer_gstin: None,
            period_end: None,
            now: NOW,
        })
        .unwrap();
        // 105 * 18% = 18.9 -> 19: CGST 9, SGST 10.
        assert_eq!((inv.cgst_minor, inv.sgst_minor), (9, 10));
        assert_eq!(inv.total_minor, 124);
    }

    #[test]
    fn export_bands_are_zero_rated_in_their_currency_with_the_lut_note() {
        for (band, cur) in [(Band::My, "MYR"), (Band::Sg, "SGD"), (Band::Us, "USD")] {
            let inv = make(&sub(band), Some("27AAPFU0939F1ZV")).unwrap();
            assert_eq!(inv.currency, cur);
            assert_eq!((inv.cgst_minor, inv.sgst_minor, inv.igst_minor), (0, 0, 0));
            assert_eq!(inv.total_minor, inv.subtotal_minor);
            assert_eq!(inv.note.as_deref(), Some(LUT_NOTE));
            assert_eq!((inv.buyer_gstin, inv.buyer_state_code), (None, None));
            assert_eq!(inv.band, band.as_str());
        }
    }

    #[test]
    fn add_ons_are_one_line_each_with_their_quantity() {
        let mut s = sub(Band::In);
        s.addon_devices_20 = 2;
        s.addon_tunnel = 1;
        let inv = make(&s, None).unwrap();
        let lines: Vec<Line> = serde_json::from_str(&inv.lines_json).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].quantity, lines[0].amount_minor), (1, 100_000));
        assert_eq!(
            (
                lines[1].quantity,
                lines[1].unit_minor,
                lines[1].amount_minor
            ),
            (2, 20_005, 40_010)
        );
        assert_eq!((lines[2].quantity, lines[2].amount_minor), (1, 50_000));
        assert_eq!(inv.subtotal_minor, 190_010);
        assert_eq!(inv.cgst_minor + inv.sgst_minor, (190_010 * 18 + 50) / 100);
    }

    #[test]
    fn a_missing_price_issues_nothing() {
        let s = sub(Band::In);
        let seller = Seller::dev();
        let mut items = book();
        items.retain(|i| !(i.band == Band::In && i.item == PriceItem::Basic));
        let res = draft(&DraftInput {
            id: "i".to_owned(),
            seller: &seller,
            sub: &s,
            items: &items,
            buyer_name: "x",
            buyer_gstin: None,
            period_end: None,
            now: NOW,
        });
        assert!(matches!(res, Err(QuoteError::NoPrice(_))));
        let mut free = sub(Band::In);
        free.tier = Tier::Free;
        assert!(make(&free, None).is_err());
    }

    #[test]
    fn the_period_runs_one_cycle_back_from_its_end() {
        let end = chrono::DateTime::parse_from_rfc3339("2026-12-15T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        let (start, e) = period(NOW, Some(end), BillingCycle::Monthly);
        assert_eq!(e, end);
        assert_eq!(
            start,
            chrono::DateTime::parse_from_rfc3339("2026-11-15T00:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
        let (start, _) = period(NOW, Some(end), BillingCycle::Yearly);
        assert_eq!(
            start,
            chrono::DateTime::parse_from_rfc3339("2025-12-15T00:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
        let (_, e) = period(NOW, None, BillingCycle::Monthly);
        assert_eq!(e, renews_at(NOW, BillingCycle::Monthly));
    }

    #[test]
    fn the_seller_comes_from_the_environment_or_is_the_dev_seller() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(Seller::from_env(&env(&[])).unwrap(), None);
        let ok = Seller::from_env(&env(&[
            (SELLER_GSTIN_ENV, " 29aagcb7383j1z4 "),
            (SELLER_NAME_ENV, "Arena 360 Pvt Ltd"),
            (SELLER_ADDRESS_ENV, "Bengaluru"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(ok.state_code(), "29");
        assert_eq!(ok.gstin, "29AAGCB7383J1Z4");
        let err = Seller::from_env(&env(&[(SELLER_GSTIN_ENV, "nope")])).unwrap_err();
        assert!(err.contains(SELLER_GSTIN_ENV), "{err}");
        let err = Seller::from_env(&env(&[(SELLER_GSTIN_ENV, "29AAGCB7383J1Z4")])).unwrap_err();
        assert!(err.contains(SELLER_NAME_ENV), "{err}");
        assert_eq!(Seller::dev().state_code(), "27");
    }

    #[test]
    fn money_has_two_decimals() {
        assert_eq!(money(118_000), "1180.00");
        assert_eq!(money(5), "0.05");
        assert_eq!(money_with_currency(2_359_000, "INR"), "Rs 23590.00");
        assert_eq!(money_with_currency(9_900, "MYR"), "MYR 99.00");
    }

    fn issued(new: NewInvoice) -> Invoice {
        Invoice {
            id: new.id,
            number: "A360/2026-27/000001".to_owned(),
            fy: "2026-27".to_owned(),
            kind: "invoice".to_owned(),
            seq: 1,
            tenant_id: new.tenant_id,
            branch_id: new.branch_id,
            subscription_id: new.subscription_id,
            billing_event_id: "e1".to_owned(),
            issued_at: new.issued_at,
            period_start: new.period_start,
            period_end: new.period_end,
            band: new.band,
            currency: new.currency,
            lines_json: new.lines_json,
            subtotal_minor: new.subtotal_minor,
            cgst_minor: new.cgst_minor,
            sgst_minor: new.sgst_minor,
            igst_minor: new.igst_minor,
            total_minor: new.total_minor,
            sac: new.sac,
            buyer_name: new.buyer_name,
            buyer_gstin: new.buyer_gstin,
            buyer_state_code: new.buyer_state_code,
            seller_name: new.seller_name,
            seller_address: new.seller_address,
            seller_gstin: new.seller_gstin,
            note: new.note,
            pdf_key: None,
            pdf_at: None,
            notified_at: None,
            original_number: None,
        }
    }

    #[test]
    fn the_pdf_is_well_formed_and_names_the_number_gst_lines_and_buyer() {
        let inv = issued(make(&sub(Band::In), Some("29AAGCB7383J1Z4")).unwrap());
        assert_eq!(
            pdf_key(&inv),
            "billing/invoices/2026-27/A360-2026-27-000001.pdf"
        );
        let pdf = pdf_of(&inv);
        let text = String::from_utf8_lossy(&pdf).into_owned();
        assert!(text.starts_with("%PDF-1.4\n") && text.trim_end().ends_with("%%EOF"));
        for needle in [
            "TAX INVOICE",
            "A360/2026-27/000001",
            "29AAGCB7383J1Z4",
            "Pixel Arena",
            "IGST @ 18%",
            "1180.00",
            "998314",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
        assert!(!text.contains("CGST @"));
        // Every xref entry points at its object.
        let xref = text.rfind("xref\n").unwrap();
        let entries: Vec<&str> = text[xref..].lines().skip(3).collect();
        for (i, entry) in entries
            .iter()
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let offset: usize = entry[..10].parse().unwrap();
            assert!(
                text[offset..].starts_with(&format!("{} 0 obj", i + 1)),
                "object {}",
                i + 1
            );
        }
        let intra = pdf_of(&issued(make(&sub(Band::In), None).unwrap()));
        let intra = String::from_utf8_lossy(&intra);
        assert!(intra.contains("CGST @ 9%") && intra.contains("SGST @ 9%"));
        assert!(intra.contains("Unregistered"));
        let export = pdf_of(&issued(make(&sub(Band::Us), None).unwrap()));
        let export = String::from_utf8_lossy(&export);
        assert!(export.contains(LUT_NOTE) && !export.contains("GST @"));
    }

    #[test]
    fn pdf_text_is_escaped_and_long_documents_get_more_pages() {
        assert_eq!(pdf_text("a(b)c\\d\u{20b9}"), "a\\(b\\)c\\\\d?");
        let lines: Vec<String> = (0..130).map(|i| format!("line {i}")).collect();
        let pdf = String::from_utf8(pdf_of_lines(&lines)).unwrap();
        assert!(pdf.contains("/Count 3"));
    }
}

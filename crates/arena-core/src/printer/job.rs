//! Turns a settled order into the bytes of its receipt (Story 5.17): the committed rows of
//! `arena_store::ReceiptSource` and the branch settings go in, the Story 5.16 renderer's ESC/POS
//! bytes come out. It reads nothing itself, so the print task and the tests share one path.

use arena_proto::{
    KEY_PRINTER_PAPER_MM, KEY_RECEIPT_ADDRESS, KEY_RECEIPT_FOOTER, KEY_RECEIPT_PHONE, KEY_TAX,
    NumberFormat, PaymentMethod, PaymentRow, SettingEntry, SnapshotLocale, SymbolPosition,
    TaxSettings,
};
use arena_store::{ReceiptLineRow, ReceiptSource};

use super::PaperWidth;
use super::receipt::{
    ItemLine, ReceiptMoney, ReceiptSettings, SaleReceipt, SessionLine, render_sale_copy,
};
use crate::invoice::tax::RateTax;

fn setting<'a>(settings: &'a [SettingEntry], key: &str) -> &'a str {
    settings
        .iter()
        .find(|s| s.key == key)
        .map_or("", |s| s.value.as_str())
}

fn method(name: &str) -> Option<PaymentMethod> {
    [
        PaymentMethod::Cash,
        PaymentMethod::Upi,
        PaymentMethod::Card,
        PaymentMethod::Wallet,
        PaymentMethod::Credit,
    ]
    .into_iter()
    .find(|m| m.as_str() == name)
}

/// The receipt settings the branch has chosen: paper, header, footer and how money is written.
pub fn receipt_settings(
    settings: &[SettingEntry],
    branch_name: &str,
    locale: Option<&SnapshotLocale>,
) -> ReceiptSettings {
    let paper = PaperWidth::from_setting(setting(settings, KEY_PRINTER_PAPER_MM));
    let (money, tz) = match locale {
        Some(l) => (
            ReceiptMoney::new(&l.currency, l.symbol_position, l.number_format),
            l.tz.clone(),
        ),
        None => (
            ReceiptMoney::new("INR", SymbolPosition::Before, NumberFormat::Indian),
            String::new(),
        ),
    };
    ReceiptSettings {
        paper,
        name: branch_name.to_owned(),
        address: setting(settings, KEY_RECEIPT_ADDRESS).to_owned(),
        phone: setting(settings, KEY_RECEIPT_PHONE).to_owned(),
        footer: setting(settings, KEY_RECEIPT_FOOTER).to_owned(),
        money,
        tz,
    }
}

/// A product line has a quantity; a plan line only a label; anything else is play time.
fn is_item(l: &ReceiptLineRow) -> bool {
    l.qty.is_some() || l.label.is_some()
}

fn item(l: &ReceiptLineRow) -> ItemLine {
    let qty = l.qty.unwrap_or(1).max(1);
    ItemLine {
        name: l.label.clone().unwrap_or_default(),
        qty,
        rate_minor: l.unit_price_minor.unwrap_or(l.amount_minor),
        amount_minor: l.amount_minor,
        hsn: l.hsn.clone().filter(|h| !h.is_empty()),
    }
}

/// The sale a [`ReceiptSource`] describes.
pub fn sale_receipt(
    settings: &[SettingEntry],
    source: &ReceiptSource,
    invoice_no: Option<String>,
) -> SaleReceipt {
    let tax = TaxSettings::parse(setting(settings, KEY_TAX));
    let gst = tax
        .as_ref()
        .is_some_and(|t| t.tax_mode() == arena_proto::TaxMode::GstIn);
    let sac = tax
        .as_ref()
        .map(|t| t.sac.clone())
        .filter(|s| gst && !s.is_empty());
    let head = &source.head;
    SaleReceipt {
        gst,
        gstin: tax
            .as_ref()
            .map(|t| t.gstin.clone())
            .filter(|g| gst && !g.is_empty()),
        tax_label: tax
            .as_ref()
            .map(|t| t.label.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "Tax".to_owned()),
        invoice_no,
        at_ms: head.settled_at,
        staff: head.staff.clone(),
        player: head.player.clone(),
        sessions: source
            .lines
            .iter()
            .filter(|l| !is_item(l))
            .map(|l| SessionLine {
                device: l.device.clone().unwrap_or_else(|| "Play time".to_owned()),
                minutes: l.minutes,
                rate_minor: l.rate_minor_per_hour,
                amount_minor: l.amount_minor,
                sac: sac.clone(),
            })
            .collect(),
        items: source
            .lines
            .iter()
            .filter(|l| is_item(l))
            .map(item)
            .collect(),
        subtotal_minor: head.subtotal_minor,
        tax: head
            .tax_breakup
            .as_deref()
            .and_then(|j| serde_json::from_str::<Vec<RateTax>>(j).ok())
            .unwrap_or_default(),
        total_minor: head.total_minor,
        payments: source
            .payments
            .iter()
            .filter_map(|p| {
                Some(PaymentRow {
                    method: method(&p.method)?,
                    amount_minor: p.amount_minor,
                    reference: p.reference.clone(),
                })
            })
            .collect(),
    }
}

/// The ESC/POS bytes of the receipt of `source`, marked "Reprint" when `reprint` is set.
pub fn render_order(
    settings: &[SettingEntry],
    branch_name: &str,
    locale: Option<&SnapshotLocale>,
    source: &ReceiptSource,
    reprint: bool,
) -> Vec<u8> {
    let sale = sale_receipt(settings, source, source.head.invoice_no.clone());
    render_sale_copy(
        &receipt_settings(settings, branch_name, locale),
        &sale,
        reprint,
    )
}

#[cfg(test)]
mod tests {
    use arena_store::{ReceiptHead, ReceiptPaymentRow};

    use super::*;

    fn entry(key: &str, value: &str) -> SettingEntry {
        SettingEntry {
            key: key.into(),
            value: value.into(),
            rev: 1,
        }
    }

    fn line(label: Option<&str>, qty: Option<i64>, device: Option<&str>) -> ReceiptLineRow {
        ReceiptLineRow {
            device: device.map(str::to_owned),
            label: label.map(str::to_owned),
            minutes: if device.is_some() { 90 } else { 0 },
            rate_minor_per_hour: if device.is_some() { 6000 } else { 0 },
            qty,
            unit_price_minor: qty.map(|_| 5000),
            hsn: qty.map(|_| "2106".to_owned()),
            amount_minor: if device.is_some() { 9000 } else { 10000 },
        }
    }

    fn source() -> ReceiptSource {
        ReceiptSource {
            head: ReceiptHead {
                kind: "session".into(),
                status: "settled".into(),
                subtotal_minor: 19000,
                total_minor: 19000,
                tax_breakup: Some(
                    r#"[{"rate_bp":1800,"taxable_minor":16102,"tax_minor":2898,"cgst_minor":1449,"sgst_minor":1449}]"#
                        .into(),
                ),
                settled_at: 1_793_176_500_000,
                staff: "Asha".into(),
                player: Some("rohan".into()),
                invoice_no: Some("DEV01/26-27/E1k/000007".into()),
            },
            lines: vec![
                line(None, None, Some("PC 4")),
                line(Some("Cold coffee"), Some(2), None),
            ],
            payments: vec![
                ReceiptPaymentRow {
                    method: "cash".into(),
                    amount_minor: 10000,
                    reference: None,
                },
                ReceiptPaymentRow {
                    method: "upi".into(),
                    amount_minor: 9000,
                    reference: Some("UPI12345678".into()),
                },
                ReceiptPaymentRow {
                    method: "gift".into(),
                    amount_minor: 1,
                    reference: None,
                },
            ],
        }
    }

    fn settings() -> Vec<SettingEntry> {
        vec![
            entry(KEY_PRINTER_PAPER_MM, "58"),
            entry(KEY_RECEIPT_FOOTER, "Thank you"),
            entry(
                KEY_TAX,
                r#"{"country":"IN","gst":true,"gstin":"29ABCDE1234F1Z5","inclusive":true,"product_bp":500,"play_bp":1800,"sac":"9996","label":"Tax","rate_bp":0}"#,
            ),
        ]
    }

    #[test]
    fn the_sale_is_read_from_the_committed_rows() {
        let sale = sale_receipt(&settings(), &source(), source().head.invoice_no);
        assert!(sale.gst);
        assert_eq!(sale.gstin.as_deref(), Some("29ABCDE1234F1Z5"));
        assert_eq!(sale.invoice_no.as_deref(), Some("DEV01/26-27/E1k/000007"));
        assert_eq!(sale.player.as_deref(), Some("rohan"));
        assert_eq!(sale.sessions.len(), 1);
        assert_eq!(sale.sessions[0].device, "PC 4");
        assert_eq!(sale.sessions[0].sac.as_deref(), Some("9996"));
        assert_eq!(sale.items.len(), 1);
        assert_eq!(sale.items[0].name, "Cold coffee");
        assert_eq!(sale.items[0].qty, 2);
        assert_eq!(sale.items[0].hsn.as_deref(), Some("2106"));
        assert_eq!(sale.tax.len(), 1);
        assert_eq!(sale.tax[0].cgst_minor, Some(1449));
        assert_eq!(
            sale.payments.len(),
            2,
            "a method this Hub does not know is left off"
        );
    }

    #[test]
    fn a_plan_line_prints_as_one_item() {
        let mut s = source();
        s.lines = vec![ReceiptLineRow {
            minutes: 120,
            rate_minor_per_hour: 0,
            qty: None,
            unit_price_minor: None,
            hsn: None,
            ..line(Some("2 hour pack"), None, None)
        }];
        let sale = sale_receipt(&settings(), &s, None);
        assert!(sale.sessions.is_empty());
        assert_eq!(sale.items[0].name, "2 hour pack");
        assert_eq!(sale.items[0].qty, 1);
        assert_eq!(sale.items[0].rate_minor, sale.items[0].amount_minor);
    }

    #[test]
    fn a_branch_without_gst_prints_no_gstin_or_sac() {
        let sale = sale_receipt(&[entry(KEY_TAX, "not json")], &source(), None);
        assert!(!sale.gst);
        assert_eq!(sale.gstin, None);
        assert_eq!(sale.sessions[0].sac, None);
        assert_eq!(sale.tax_label, "Tax");
    }

    #[test]
    fn a_reprint_carries_the_mark_and_the_same_receipt() {
        let first = render_order(&settings(), "Arena", None, &source(), false);
        let again = render_order(&settings(), "Arena", None, &source(), true);
        let text = |b: &[u8]| String::from_utf8_lossy(b).to_string();
        assert!(!text(&first).contains("Reprint"));
        assert!(text(&again).contains("Reprint"));
        assert!(text(&first).contains("DEV01/26-27/E1k/000007"));
        assert!(text(&again).contains("DEV01/26-27/E1k/000007"));
    }
}

//! Business (B2B) invoices (Story 5.20).
//!
//! A settle on a GST-registered Indian branch (`TaxMode::GstIn`) may carry the business the
//! invoice is made out to: its GSTIN (15 characters, valid state code and check digit) and its
//! legal name. [`record_on_settle`] checks both inside the settle transaction and stores them on
//! the order, which is what flags it B2B for GSTR-1 and what the receipt prints under its title.
//! A refused settle rolls back whole: no sale, no stock move, no payment row. A branch that is not
//! on GST never takes a business invoice. Nothing is registered as an irreversible effect, so
//! Undo stays open (the buyer goes with the order if the order is reverted and settled again).

use arena_proto::{
    AppError, BusinessBuyer, ErrorCode, Intent, IntentPayload, KEY_TAX, TaxMode, TaxSettings,
};
use arena_store::{StoreTx, find_setting, set_order_buyer};

use super::session::invalid;

/// The business a settle intent names, `None` for any other intent or a consumer sale.
fn buyer_of(intent: &Intent) -> Option<&BusinessBuyer> {
    match &intent.payload {
        IntentPayload::SessionSettle(s) => s.business.as_ref(),
        IntentPayload::PosSale(s) => s.business.as_ref(),
        IntentPayload::PlanSell(s) => s.business.as_ref(),
        _ => None,
    }
}

/// The business step of a settle: refuses a buyer on a branch that is not on GST, or one with an
/// invalid GSTIN (`GSTIN_INVALID`) or legal name, and otherwise stores it on the sale in
/// `order_id`. A bill with no total takes no invoice number, so it has no invoice to make out and
/// the buyer is accepted but not stored.
pub(super) fn record_on_settle(
    tx: &StoreTx<'_>,
    intent: &Intent,
    order_id: &str,
    total_minor: i64,
) -> Result<(), AppError> {
    let Some(buyer) = buyer_of(intent) else {
        return Ok(());
    };
    let gst = match find_setting(tx, KEY_TAX)? {
        Some(row) => TaxSettings::parse(&row.value)
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "the tax settings cannot be read"))?
            .tax_mode(),
        None => TaxMode::Simple,
    } == TaxMode::GstIn;
    if !gst {
        return Err(invalid(
            "business",
            "business invoices are for GST-registered branches",
        ));
    }
    buyer
        .check()
        .map_err(|(field, message)| invalid(field, message))?;
    if total_minor > 0 {
        set_order_buyer(tx, order_id, &buyer.gstin, &buyer.legal_name)?;
    }
    Ok(())
}

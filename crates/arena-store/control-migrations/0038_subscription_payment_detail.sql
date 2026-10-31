-- What the owner sees as the payment method of a subscription (Story 10.8): `mandate_method` says
-- how it is paid and `mandate_detail` names it, the last four digits of a card or the bank of an
-- eNACH mandate. A UPI handle is personal data and is never kept, so UPI has no detail. It is set
-- by the same webhook transition that sets `mandate_method`.
ALTER TABLE subscriptions ADD COLUMN mandate_detail TEXT;

-- A credit note (Story 10.15) names the invoice it reverses; an invoice has none.
ALTER TABLE billing_invoices ADD COLUMN original_number TEXT;

DROP TRIGGER billing_invoices_no_edit;

CREATE TRIGGER billing_invoices_no_edit BEFORE UPDATE OF id, number, fy, kind, seq, tenant_id, branch_id, subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, sac, buyer_name, buyer_gstin, buyer_state_code, seller_name, seller_address, seller_gstin, note, original_number ON billing_invoices
BEGIN
    SELECT RAISE(ABORT, 'billing_invoices are immutable');
END;

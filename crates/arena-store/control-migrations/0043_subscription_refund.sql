-- Full refund within 7 days of the first payment (Story 10.15). An owner who cancels inside the window
-- asks for the whole first payment back: `refund_requests` holds the ask (one per subscription, so a
-- second request is refused), which an Arena 360 operator approves or rejects with an audited
-- `arena-cloud ops refund` command. Approving refunds every payment of the subscription at Razorpay
-- (one `refund_payments` row each, so a retry skips what was already refunded) and cancels it at once;
-- Razorpay's `refund.processed` then marks each payment processed, issues a credit note for the invoice
-- of that payment and, when the last one lands, makes the subscription `refunded` and the branch Free.
-- `billing_invoices.payment_id` is the Razorpay payment that paid an invoice, so a refunded payment
-- finds the invoice it reverses; it is written once at issue.
ALTER TABLE billing_invoices ADD COLUMN payment_id TEXT;

CREATE INDEX billing_invoices_by_payment ON billing_invoices (payment_id) WHERE payment_id IS NOT NULL;

DROP TRIGGER billing_invoices_no_edit;

CREATE TRIGGER billing_invoices_no_edit BEFORE UPDATE OF id, number, fy, kind, seq, tenant_id, branch_id, subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, sac, buyer_name, buyer_gstin, buyer_state_code, seller_name, seller_address, seller_gstin, note, original_number, payment_id ON billing_invoices
BEGIN
    SELECT RAISE(ABORT, 'billing_invoices are immutable');
END;

CREATE TABLE refund_requests (
    id TEXT PRIMARY KEY NOT NULL,
    subscription_id TEXT NOT NULL UNIQUE REFERENCES subscriptions (id),
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    branch_id TEXT NOT NULL REFERENCES branches (id),
    -- The first payment, the one the 7 days run from.
    payment_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    status TEXT NOT NULL CHECK (status IN ('requested', 'issued', 'processed', 'rejected')),
    requested_at INTEGER NOT NULL,
    -- The operator's decision: who, why and when (`issued` and `rejected`).
    decided_by TEXT,
    decided_reason TEXT,
    decided_at INTEGER,
    -- The last provider error of an approval that did not complete; cleared when it does.
    last_error TEXT,
    processed_at INTEGER
);

CREATE INDEX refund_requests_by_status ON refund_requests (status, requested_at);

CREATE TABLE refund_payments (
    request_id TEXT NOT NULL REFERENCES refund_requests (id),
    payment_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    razorpay_refund_id TEXT,
    processed_at INTEGER,
    PRIMARY KEY (request_id, payment_id)
);

CREATE INDEX refund_payments_by_payment ON refund_payments (payment_id);

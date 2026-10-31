-- GST tax invoices for subscription charges (Story 10.7). One row per charge, written in the same
-- transaction as the billing event that caused it (`billing_event_id` is unique, so a repeated
-- delivery issues nothing). `number` is `A360/<FY>/<seq:06>` (credit notes, Story 10.15, are
-- `kind = 'credit_note'` with their own sequence); `seq` restarts at 1 each Indian financial year
-- (Apr-Mar, Asia/Kolkata) and is gap-free because it is `MAX(seq) + 1` inside the issuing
-- transaction, with `(fy, kind, seq)` unique as the backstop. Money is in minor units of
-- `currency`. Everything on an issued invoice is immutable; only the render and notify marks
-- (`pdf_key`, `pdf_at`, `notified_at`) are set afterwards.
CREATE TABLE billing_invoices (
    id TEXT PRIMARY KEY NOT NULL,
    number TEXT NOT NULL UNIQUE,
    fy TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'invoice' CHECK (kind IN ('invoice', 'credit_note')),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    branch_id TEXT NOT NULL REFERENCES branches (id),
    subscription_id TEXT NOT NULL REFERENCES subscriptions (id),
    billing_event_id TEXT NOT NULL UNIQUE REFERENCES billing_events (id),
    issued_at INTEGER NOT NULL,
    period_start INTEGER NOT NULL,
    period_end INTEGER NOT NULL,
    band TEXT NOT NULL CHECK (band IN ('IN', 'MY', 'SG', 'US')),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    lines_json TEXT NOT NULL,
    subtotal_minor INTEGER NOT NULL CHECK (subtotal_minor >= 0),
    cgst_minor INTEGER NOT NULL DEFAULT 0 CHECK (cgst_minor >= 0),
    sgst_minor INTEGER NOT NULL DEFAULT 0 CHECK (sgst_minor >= 0),
    igst_minor INTEGER NOT NULL DEFAULT 0 CHECK (igst_minor >= 0),
    total_minor INTEGER NOT NULL CHECK (total_minor >= 0),
    sac TEXT NOT NULL,
    buyer_name TEXT NOT NULL,
    buyer_gstin TEXT,
    buyer_state_code TEXT,
    seller_name TEXT NOT NULL,
    seller_address TEXT NOT NULL,
    seller_gstin TEXT NOT NULL,
    note TEXT,
    pdf_key TEXT,
    pdf_at INTEGER,
    notified_at INTEGER,
    UNIQUE (fy, kind, seq)
);

CREATE INDEX billing_invoices_by_branch ON billing_invoices (branch_id, issued_at);

CREATE INDEX billing_invoices_to_finish ON billing_invoices (issued_at) WHERE pdf_key IS NULL OR notified_at IS NULL;

CREATE TRIGGER billing_invoices_no_edit BEFORE UPDATE OF id, number, fy, kind, seq, tenant_id, branch_id, subscription_id, billing_event_id, issued_at, period_start, period_end, band, currency, lines_json, subtotal_minor, cgst_minor, sgst_minor, igst_minor, total_minor, sac, buyer_name, buyer_gstin, buyer_state_code, seller_name, seller_address, seller_gstin, note ON billing_invoices
BEGIN
    SELECT RAISE(ABORT, 'billing_invoices are immutable');
END;

CREATE TRIGGER billing_invoices_no_delete BEFORE DELETE ON billing_invoices
BEGIN
    SELECT RAISE(ABORT, 'billing_invoices are immutable');
END;

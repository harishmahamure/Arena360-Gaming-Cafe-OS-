-- Closing a shift (Story 5.26) keeps what was counted, what the ledger expected and the signed
-- difference (counted - expected; negative is short), with who closed it and who approved a
-- variance over the branch threshold. `next_float_minor` is what the next shift's float is
-- prefilled with: the counted cash less any deposit made at close.
ALTER TABLE shifts ADD COLUMN counted_minor INTEGER NULL CHECK (counted_minor IS NULL OR counted_minor >= 0);
ALTER TABLE shifts ADD COLUMN expected_minor INTEGER NULL;
ALTER TABLE shifts ADD COLUMN variance_minor INTEGER NULL;
ALTER TABLE shifts ADD COLUMN variance_reason TEXT NULL
    CHECK (variance_reason IS NULL OR variance_reason IN
           ('counting_error', 'unrecorded_expense', 'change_float_mismatch', 'other'));
ALTER TABLE shifts ADD COLUMN variance_note TEXT NULL;
ALTER TABLE shifts ADD COLUMN closed_by TEXT NULL;
ALTER TABLE shifts ADD COLUMN approver_id TEXT NULL;
ALTER TABLE shifts ADD COLUMN next_float_minor INTEGER NULL CHECK (next_float_minor IS NULL OR next_float_minor >= 0);

-- Deposits and expenses (Story 5.25) are cash events too. `amount_minor` stays the drawer's
-- effect (negative for a deposit or a drawer expense, 0 for an expense the owner paid), so the
-- expected cash is the sum of a shift's events. `spent_minor` is what an expense cost, whoever
-- paid it; `receipt_photo` is a JPEG data URI of at most 200 KB.
ALTER TABLE cash_events ADD COLUMN reference TEXT NULL;
ALTER TABLE cash_events ADD COLUMN category TEXT NULL
    CHECK (category IS NULL OR category IN ('supplies', 'utilities', 'repairs', 'food_stock', 'other'));
ALTER TABLE cash_events ADD COLUMN paid_from TEXT NULL
    CHECK (paid_from IS NULL OR paid_from IN ('drawer', 'owner'));
ALTER TABLE cash_events ADD COLUMN spent_minor INTEGER NULL CHECK (spent_minor IS NULL OR spent_minor > 0);
ALTER TABLE cash_events ADD COLUMN note TEXT NULL;
ALTER TABLE cash_events ADD COLUMN receipt_photo TEXT NULL;

DROP TRIGGER cash_events_no_update;
CREATE TRIGGER cash_events_no_update BEFORE UPDATE OF
    id, shift_id, kind, amount_minor, staff_id, order_id, reverses,
    reference, category, paid_from, spent_minor, note, receipt_photo
ON cash_events
BEGIN
    SELECT RAISE(ABORT, 'cash_events is append-only');
END;

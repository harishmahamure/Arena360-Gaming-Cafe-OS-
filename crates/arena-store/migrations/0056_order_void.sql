-- Void an unsettled order (Story 5.23). A voided order keeps its row and its lines (reports list
-- it, nothing counts it as a sale): `status = 'void'`, when it was voided, the approving staff
-- member (the actor when the actor holds `pos.void` themselves), the reason picked (`mistake`,
-- `changed_mind` or `other`), the free text of an `other` reason, and the session whose bill it
-- was. `session_id` itself is cleared so the session can open a fresh bill. All NULL otherwise.
ALTER TABLE orders ADD COLUMN voided_at INTEGER NULL;
ALTER TABLE orders ADD COLUMN voided_by TEXT NULL;
ALTER TABLE orders ADD COLUMN void_reason TEXT NULL
    CHECK (void_reason IS NULL OR void_reason IN ('mistake', 'changed_mind', 'other'));
ALTER TABLE orders ADD COLUMN void_note TEXT NULL;
ALTER TABLE orders ADD COLUMN void_session_id TEXT NULL;

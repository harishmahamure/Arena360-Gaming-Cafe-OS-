-- Manual discount at settle (Story 5.22). A discount staff gave by hand, approved with a manager
-- PIN: the amount taken off (the order total is already net of it; a session or plan order's
-- `subtotal_minor` too), the reason picked (`regular_customer`, `service_issue`, `staff_meal` or
-- `other`), the free text of an `other` reason, and the approving staff member (the actor when the
-- actor holds `discount.manual` themselves). All NULL when no discount was given.
ALTER TABLE orders ADD COLUMN discount_minor INTEGER NULL
    CHECK (discount_minor IS NULL OR discount_minor > 0);
ALTER TABLE orders ADD COLUMN discount_reason TEXT NULL
    CHECK (discount_reason IS NULL
           OR discount_reason IN ('regular_customer', 'service_issue', 'staff_meal', 'other'));
ALTER TABLE orders ADD COLUMN discount_note TEXT NULL;
ALTER TABLE orders ADD COLUMN discount_by TEXT NULL;

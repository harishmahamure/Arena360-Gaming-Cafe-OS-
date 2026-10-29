-- 0070: an order-ready alert for a walk-up kitchen ticket records the ticket (Story 7.4), so
-- several can be open at once at branch level (one per ticket). Device alerts leave it NULL.
ALTER TABLE alerts ADD COLUMN ticket_id TEXT NULL;

DROP INDEX alerts_open_per_branch;
CREATE UNIQUE INDEX alerts_open_per_branch
    ON alerts (kind, IFNULL(product_id, ''), IFNULL(ticket_id, ''))
    WHERE acked_at IS NULL AND device_id IS NULL;

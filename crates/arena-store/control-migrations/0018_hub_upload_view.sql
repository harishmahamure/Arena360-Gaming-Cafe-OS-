-- The cloud's cached view of one Hub's change-log upload (Story 8.3): what its last heartbeat
-- said, the range the cloud last asked it to send again and the range it said it no longer has.
-- A heartbeat that agrees with `last_epoch_seen` / `last_seq_seen` costs no bucket listing.
ALTER TABLE hubs ADD COLUMN last_epoch_seen TEXT;
ALTER TABLE hubs ADD COLUMN last_seq_seen INTEGER;
ALTER TABLE hubs ADD COLUMN last_upload_at INTEGER;
ALTER TABLE hubs ADD COLUMN last_summary_at INTEGER;
ALTER TABLE hubs ADD COLUMN last_rollup_date TEXT;
ALTER TABLE hubs ADD COLUMN resend_epoch TEXT;
ALTER TABLE hubs ADD COLUMN resend_from INTEGER;
ALTER TABLE hubs ADD COLUMN resend_to INTEGER;
ALTER TABLE hubs ADD COLUMN unavailable_epoch TEXT;
ALTER TABLE hubs ADD COLUMN unavailable_from INTEGER;
ALTER TABLE hubs ADD COLUMN unavailable_to INTEGER;
ALTER TABLE hubs ADD COLUMN gaps_found INTEGER NOT NULL DEFAULT 0;

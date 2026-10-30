-- 0089: why the last cloud upload failed (Story 9.16).
--
-- The header and System -> Backup say in plain words when a backup step is failing. The key
-- backup and the upgrade snapshot already keep their last error; this is the one for the batches,
-- summary, rollups, snapshot and GST archive. A short code, never player data, cleared by the
-- next cycle that ends well. Local bookkeeping outside the change log, like the rest of
-- `backup_state`.
ALTER TABLE backup_state ADD COLUMN upload_error TEXT NULL;
ALTER TABLE backup_state ADD COLUMN upload_error_at INTEGER NULL;

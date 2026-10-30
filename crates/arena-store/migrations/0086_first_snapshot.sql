-- 0086: the upgrade snapshot (Story 9.11, AD-36).
--
-- A Free branch uploads no change log. When its licence turns paid, the uploader first stores a
-- full snapshot (and the key backup), then starts batching from the snapshot's `seq + 1`. All of
-- this is local bookkeeping outside the change log, like the rest of `backup_state`.
--
-- * `base_epoch` / `base_seq`: the last change row the upgrade snapshot holds. The cloud has no
--   batches up to there, so the heartbeat reports `base_seq` and gap detection starts after it.
--   NULL on a Free branch and again after a lapse.
-- * `first_snapshot_pending`: the branch is paid but the upgrade snapshot is not stored yet, so no
--   batch is uploaded. `first_snapshot_error` / `first_snapshot_error_at` say why the last attempt
--   failed (a code, never player data).
ALTER TABLE backup_state ADD COLUMN base_epoch TEXT NULL;
ALTER TABLE backup_state ADD COLUMN base_seq INTEGER NULL CHECK (base_seq IS NULL OR base_seq >= 0);
ALTER TABLE backup_state ADD COLUMN first_snapshot_pending INTEGER NOT NULL DEFAULT 0 CHECK (first_snapshot_pending IN (0, 1));
ALTER TABLE backup_state ADD COLUMN first_snapshot_error TEXT NULL;
ALTER TABLE backup_state ADD COLUMN first_snapshot_error_at INTEGER NULL;

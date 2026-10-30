-- What the cloud remembers of a paid Hub's backup block (Story 9.17), next to the upload view
-- (0018). `last_snapshot_at` and `backup_failed_at` are Hub-clock ms as the heartbeat reported
-- them; `backup_pending_batches` is the waiting change rows in whole batches. It is NULL until a
-- paid Hub has sent the block, which is how the owner app tells "no backup information yet" from
-- "nothing pending". The heartbeat replaces all three each time it carries the block, so a failure
-- the Hub no longer reports leaves with it. Times only, never player data.
ALTER TABLE hubs ADD COLUMN last_snapshot_at INTEGER;
ALTER TABLE hubs ADD COLUMN backup_pending_batches INTEGER;
ALTER TABLE hubs ADD COLUMN backup_failed_at INTEGER;

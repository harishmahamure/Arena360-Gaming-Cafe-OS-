ALTER TABLE devices_meta ADD COLUMN tv_number INTEGER NULL;

CREATE UNIQUE INDEX devices_meta_tv_number ON devices_meta (tv_number)
    WHERE kind = 'console' AND tv_number IS NOT NULL AND deleted_at IS NULL;

UPDATE devices_meta
SET deleted_at = (SELECT d.revoked_at FROM devices d WHERE d.device_id = devices_meta.device_id)
WHERE deleted_at IS NULL
  AND device_id IN (SELECT device_id FROM devices WHERE revoked_at IS NOT NULL);

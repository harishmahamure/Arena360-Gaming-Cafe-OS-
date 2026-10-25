ALTER TABLE devices_meta ADD COLUMN state TEXT NOT NULL DEFAULT 'offline'
    CHECK (state IN ('online', 'offline', 'in_session', 'reserved', 'maintenance'));
ALTER TABLE devices_meta ADD COLUMN prior_state TEXT NULL
    CHECK (prior_state IS NULL OR prior_state IN ('online', 'offline', 'in_session', 'reserved'));
ALTER TABLE devices_meta ADD COLUMN maintenance_note TEXT NULL;

UPDATE devices_meta
SET state = 'online'
WHERE device_id IN (SELECT device_id FROM devices WHERE token_hash = '');

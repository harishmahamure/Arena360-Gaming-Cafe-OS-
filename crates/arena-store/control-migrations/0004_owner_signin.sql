ALTER TABLE owners ADD COLUMN sms_fallback INTEGER NOT NULL DEFAULT 0 CHECK (sms_fallback IN (0, 1));
ALTER TABLE owners ADD COLUMN failed_signins INTEGER NOT NULL DEFAULT 0;
ALTER TABLE owners ADD COLUMN locked_until INTEGER;

CREATE TABLE signin_challenges (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES owners (id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    consumed_at INTEGER,
    sms_code_hash TEXT,
    sms_sent_at INTEGER
);

CREATE INDEX signin_challenges_owner ON signin_challenges (owner_id);
CREATE INDEX signin_challenges_expires ON signin_challenges (expires_at);

CREATE TABLE owner_devices (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES owners (id),
    token_hash TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER
);

CREATE INDEX owner_devices_owner ON owner_devices (owner_id);

ALTER TABLE owner_sessions ADD COLUMN device_id TEXT;

CREATE INDEX owner_sessions_device ON owner_sessions (device_id);

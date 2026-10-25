CREATE TABLE devices_meta (
    device_id  TEXT PRIMARY KEY REFERENCES devices (device_id),
    kind       TEXT NOT NULL CHECK (kind IN ('pc', 'console', 'vr', 'sim')),
    label      TEXT NOT NULL,
    group_id   TEXT NULL,
    rev        INTEGER NOT NULL,
    deleted_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX devices_meta_group ON devices_meta (group_id);

INSERT INTO devices_meta (device_id, kind, label, group_id, rev, deleted_at, created_at, updated_at)
    SELECT device_id, 'pc', label, NULL, 1, NULL, created_at, updated_at
    FROM devices
    WHERE class = 'paired';

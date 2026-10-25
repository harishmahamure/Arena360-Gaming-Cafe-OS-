CREATE TABLE device_groups (
    group_id   TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('pc', 'console', 'vr', 'other')),
    colour     TEXT NOT NULL CHECK (colour IN ('teal', 'orange', 'lime', 'sand', 'coral', 'silver')),
    sort_order INTEGER NOT NULL,
    rev        INTEGER NOT NULL,
    deleted_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX device_groups_name ON device_groups (name COLLATE NOCASE) WHERE deleted_at IS NULL;

CREATE TABLE devices_meta_new (
    device_id  TEXT PRIMARY KEY REFERENCES devices (device_id),
    kind       TEXT NOT NULL CHECK (kind IN ('pc', 'console', 'vr', 'sim')),
    label      TEXT NOT NULL,
    group_id   TEXT NULL REFERENCES device_groups (group_id),
    rev        INTEGER NOT NULL,
    deleted_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO devices_meta_new SELECT device_id, kind, label, NULL, rev, deleted_at, created_at, updated_at FROM devices_meta;
DROP TABLE devices_meta;
ALTER TABLE devices_meta_new RENAME TO devices_meta;
CREATE INDEX devices_meta_group ON devices_meta (group_id);

CREATE TABLE alerts (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    device_id  TEXT NULL REFERENCES devices (device_id),
    raised_at  INTEGER NOT NULL,
    acked_at   INTEGER NULL,
    acked_by   TEXT NULL,
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX alerts_open_per_device ON alerts (kind, device_id)
    WHERE acked_at IS NULL AND device_id IS NOT NULL;
CREATE UNIQUE INDEX alerts_open_per_branch ON alerts (kind)
    WHERE acked_at IS NULL AND device_id IS NULL;
CREATE INDEX alerts_open ON alerts (raised_at, id) WHERE acked_at IS NULL;

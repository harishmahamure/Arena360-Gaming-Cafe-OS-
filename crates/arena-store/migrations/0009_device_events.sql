-- 0009: kiosk events (Story 1.16). Each `kiosk.log` intent a paired kiosk submits (queued while
-- the Hub was unreachable, replayed in creation order) writes one row. `at` is when the kiosk
-- recorded the event (the intent's `issued_at`), not when it reached the Hub.

CREATE TABLE device_events (
    event_id   TEXT PRIMARY KEY,
    branch_id  TEXT NOT NULL REFERENCES branch (branch_id),
    device_id  TEXT NOT NULL REFERENCES devices (device_id),
    at         INTEGER NOT NULL,
    level      TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
    code       TEXT NOT NULL,
    detail     TEXT NOT NULL,
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX device_events_device_at ON device_events (device_id, at);

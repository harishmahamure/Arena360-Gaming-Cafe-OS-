-- Ad requests and impressions (Story 4.14). Local bookkeeping outside the change log, like the
-- licence cache: one row per event, with nothing that identifies a device, creative, advertiser
-- or advertising id. `id` is the order the heartbeat's cursor walks; AUTOINCREMENT keeps an id
-- from ever being used twice.
CREATE TABLE ad_impressions (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    slot TEXT NOT NULL CHECK (slot IN ('banner', 'interstitial')),
    at   INTEGER NOT NULL
);

CREATE TABLE ad_requests (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    slot TEXT NOT NULL CHECK (slot IN ('banner', 'interstitial')),
    at   INTEGER NOT NULL
);

-- The last id the cloud has acknowledged (single row each).
CREATE TABLE ad_upload_cursor (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    through_id INTEGER NOT NULL
);
INSERT INTO ad_upload_cursor (id, through_id) VALUES (1, 0);

CREATE TABLE ad_request_cursor (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    through_id INTEGER NOT NULL
);
INSERT INTO ad_request_cursor (id, through_id) VALUES (1, 0);

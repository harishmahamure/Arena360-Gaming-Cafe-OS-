-- The latest installed-software scan of each kiosk (Story 6.4). One row per device, inserted on
-- its first `kiosk.scan_reported` and replaced by every newer one (`scanned_at` only grows).
-- `entries` is a JSON array of the executables found, `launchers` a JSON array of the launchers
-- installed with their paths, `skipped` the number of folders or files the scan could not read.
CREATE TABLE kiosk_scans (
    device_id  TEXT PRIMARY KEY REFERENCES devices (device_id),
    scanned_at INTEGER NOT NULL CHECK (scanned_at > 0),
    entries    TEXT NOT NULL DEFAULT '[]',
    launchers  TEXT NOT NULL DEFAULT '[]',
    skipped    INTEGER NOT NULL DEFAULT 0 CHECK (skipped >= 0),
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

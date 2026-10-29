-- 0076: the daily snapshot log (Story 8.8, AD-4).
--
-- One row per daily snapshot the Hub has stored in the cloud (`snapshots/<date>.db.zst`): the
-- business day it was taken for, the epoch and the last change-log `seq` the copy holds (so a
-- rebuild replays only what came after), its compressed size, the key it was PUT to and when
-- (Hub clock, ms). Local bookkeeping outside the change log, like `upload_cursor`: a row is
-- written only after the PUT answered 2xx.
CREATE TABLE snapshot_log (
    date        TEXT PRIMARY KEY,
    epoch       TEXT NOT NULL,
    last_seq    INTEGER NOT NULL CHECK (last_seq >= 0),
    bytes       INTEGER NOT NULL CHECK (bytes >= 0),
    key         TEXT NOT NULL,
    uploaded_at INTEGER NOT NULL
);

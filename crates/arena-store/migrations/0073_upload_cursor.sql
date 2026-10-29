-- 0073: the Hub's change-log upload (Story 8.2).
--
-- `lease.fork_parent_epoch` / `fork_parent_seq` are set when this Hub's epoch was forked from
-- another epoch at a known `seq` (Epic 9 sets them; both NULL until then). The first upload batch
-- of that epoch carries them and starts right after the parent's `seq`.
--
-- `upload_cursor` (single row, absent until the first batch is accepted) is the last change row
-- the cloud has stored: its epoch and `seq`, and when. It moves only after a presigned PUT
-- answered 2xx.
--
-- `changes_log_order` serves the log's order (numeric epoch counter, epoch, seq) without a scan:
-- the next epoch after the cursor's, and the count of rows still to upload.
ALTER TABLE lease ADD COLUMN fork_parent_epoch TEXT NULL;
ALTER TABLE lease ADD COLUMN fork_parent_seq INTEGER NULL;

CREATE TABLE upload_cursor (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    epoch             TEXT NOT NULL,
    last_seq_uploaded INTEGER NOT NULL CHECK (last_seq_uploaded >= 0),
    uploaded_at       INTEGER NOT NULL
);

CREATE INDEX changes_log_order ON changes (epoch_counter, epoch, seq);

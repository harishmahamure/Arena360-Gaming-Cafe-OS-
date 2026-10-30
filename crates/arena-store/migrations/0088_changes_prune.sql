-- 0088: pruning the uploaded part of the change log (Story 9.15, AD-36).
--
-- `changes` stays append-only. The delete trigger now lets a row go only while a one-row permit
-- (`changes_prune_permit`, written and removed by the housekeeping prune inside its own
-- transaction) is present and the row is: older than the permit's cutoff, passed by the upload
-- cursor, not the newest row of its epoch (so the next `seq` and the snapshot position survive),
-- and, when it is a money or tax row, older than the permit's longer money cutoff.
CREATE TABLE changes_prune_permit (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    cutoff_ms       INTEGER NOT NULL,
    money_cutoff_ms INTEGER NOT NULL
);

DROP TRIGGER changes_no_delete;

CREATE TRIGGER changes_no_delete BEFORE DELETE ON changes
WHEN NOT EXISTS (
    SELECT 1
    FROM changes_prune_permit p, upload_cursor c
    WHERE OLD.at < p.cutoff_ms
      AND (OLD.tbl NOT IN ('orders', 'order_lines', 'payments', 'refunds', 'invoice_sequences',
                           'gst_archive_log')
           OR OLD.at < p.money_cutoff_ms)
      AND ((OLD.epoch_counter, OLD.epoch)
               < (CAST(substr(c.epoch, 1, instr(c.epoch, ':') - 1) AS INTEGER), c.epoch)
           OR (OLD.epoch = c.epoch AND OLD.seq <= c.last_seq_uploaded))
      AND OLD.seq < (SELECT MAX(seq) FROM changes WHERE epoch = OLD.epoch)
)
BEGIN
    SELECT RAISE(ABORT, 'changes is append-only');
END;

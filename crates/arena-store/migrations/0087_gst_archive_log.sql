-- 0087: the yearly GST archive log (Story 9.14, AD-36).
--
-- One row per financial year the Hub has uploaded a `gst-archive/<fy>.zst` for, written by the
-- system intent `gst_archive.publish` and nothing else. It carries no business data: the file's
-- key, SHA-256, size and counts, when the Hub built it, and the invoice numbers a fork set aside
-- (a JSON array of text, kept so the 90-day sweep of `orphaned/` never loses one).
CREATE TABLE gst_archive_log (
    fy           TEXT PRIMARY KEY,
    key          TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    bytes        INTEGER NOT NULL CHECK (bytes >= 0),
    invoices     INTEGER NOT NULL CHECK (invoices >= 0),
    credit_notes INTEGER NOT NULL CHECK (credit_notes >= 0),
    orphaned     TEXT NOT NULL DEFAULT '[]',
    built_at     INTEGER NOT NULL,
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

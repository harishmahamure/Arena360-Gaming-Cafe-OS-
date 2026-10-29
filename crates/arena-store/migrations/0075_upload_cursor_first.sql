-- 0075: when this branch first uploaded (Story 8.6).
--
-- `upload_cursor.first_uploaded_at` is the Hub clock (ms) of the first batch the cloud accepted:
-- the moment the branch went paid as far as the Hub can tell. The daily rollup catch-up starts at
-- the business day of that instant, so a branch upgraded from Free is not asked to roll up its
-- whole Free history (the full snapshot of Story 9.11 covers that). Local bookkeeping outside the
-- change log, like the rest of `upload_cursor`.
ALTER TABLE upload_cursor ADD COLUMN first_uploaded_at INTEGER NULL;
UPDATE upload_cursor SET first_uploaded_at = uploaded_at;

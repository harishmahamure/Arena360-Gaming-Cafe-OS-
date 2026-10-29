-- 0074: the summary and rollup log (Story 8.4, AD-35).
--
-- One row per file the Hub has published to the cloud: `date = 'latest'` is `state/latest.json`
-- (written by the system intent `summary.publish`); a `yyyy-mm-dd` row is a daily rollup (Story
-- 8.5). `schema` is the file's schema version, `built_at` when the Hub built it (Hub clock, ms) and
-- `key` the object key it was PUT to. The table carries no business data: it is the one row a
-- publish intent writes, so the publish appears in the audit.
CREATE TABLE rollup_log (
    date       TEXT PRIMARY KEY,
    schema     INTEGER NOT NULL CHECK (schema >= 1),
    built_at   INTEGER NOT NULL,
    key        TEXT NOT NULL,
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

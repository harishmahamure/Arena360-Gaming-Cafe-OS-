-- The branch game and app library (Story 6.3). One row per game or app on the branch's PCs,
-- edited at the Hub under `games.manage`. `launcher` says which launcher installs it (`none` for
-- a plain executable); `executable_path` is a full Windows path and may be empty only when
-- `launch_uri` (a `steam://` style link) starts the entry; `tags` is a JSON array of text. A
-- linked entry carries the id of the global catalogue game it came from in `catalog_id` (no
-- foreign key: the catalogue lives in the reference cache, outside branch state). Rows are
-- soft-deleted; the partial unique index refuses a second live entry with the same kind,
-- launcher and executable path (case-insensitive, entries without a path excepted).
CREATE TABLE library_entries (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind IN ('game', 'app')),
    name            TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    launcher        TEXT NOT NULL CHECK (launcher IN ('steam', 'epic', 'riot', 'battlenet', 'none')),
    executable_path TEXT NOT NULL DEFAULT '',
    launch_uri      TEXT NOT NULL DEFAULT '',
    launch_args     TEXT NOT NULL DEFAULT '',
    category        TEXT NOT NULL DEFAULT '',
    tags            TEXT NOT NULL DEFAULT '[]',
    age_rating      TEXT NOT NULL DEFAULT '',
    enabled         INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    catalog_id      TEXT NULL,
    rev             INTEGER NOT NULL,
    deleted_at      INTEGER NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    CHECK (executable_path <> '' OR launch_uri <> '')
);

CREATE UNIQUE INDEX library_entries_exe
    ON library_entries (kind, launcher, executable_path COLLATE NOCASE)
    WHERE deleted_at IS NULL AND executable_path <> '';

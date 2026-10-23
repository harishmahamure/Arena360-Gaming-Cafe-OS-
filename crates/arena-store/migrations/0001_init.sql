-- 0001: schema bookkeeping, Hub metadata and the single-row writer lease.

CREATE TABLE schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE hub_meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);

CREATE TABLE lease (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    counter         INTEGER NOT NULL,
    hub_instance_id TEXT NOT NULL,
    provisional     INTEGER NOT NULL CHECK (provisional IN (0, 1)),
    renewed_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);

-- 0002: the branch row, branch settings and the append-only change log.

CREATE TABLE branch (
    branch_id      TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    branch_code    TEXT NOT NULL,
    currency       TEXT NOT NULL,
    tz             TEXT NOT NULL,
    day_start_min  INTEGER NOT NULL CHECK (day_start_min BETWEEN 0 AND 1439),
    fy_start_month INTEGER NOT NULL CHECK (fy_start_month BETWEEN 1 AND 12),
    rev            INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE settings (
    branch_id  TEXT NOT NULL REFERENCES branch (branch_id),
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    rev        INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (branch_id, key)
);

-- The audit log. `seq` is gap-free per epoch; rows are never updated or deleted.
CREATE TABLE changes (
    epoch                       TEXT NOT NULL,
    seq                         INTEGER NOT NULL,
    tbl                         TEXT NOT NULL,
    row_id                      TEXT NOT NULL,
    op                          TEXT NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
    row                         BLOB NOT NULL,
    intent_id                   TEXT NOT NULL,
    intent_kind                 TEXT NOT NULL,
    actor_kind                  TEXT NOT NULL
        CHECK (actor_kind IN ('staff', 'owner', 'admin', 'system', 'device')),
    actor_id                    TEXT NOT NULL,
    elevated_by_kind            TEXT NULL
        CHECK (elevated_by_kind IS NULL
               OR elevated_by_kind IN ('staff', 'owner', 'admin', 'system', 'device')),
    elevated_by_id              TEXT NULL,
    elevated_by_mode            TEXT NULL
        CHECK (elevated_by_mode IS NULL OR elevated_by_mode IN ('hub', 'cached_verifier')),
    elevated_by_verifier_version INTEGER NULL,
    at                          INTEGER NOT NULL,
    PRIMARY KEY (epoch, seq),
    -- elevated_by is all-or-nothing.
    CHECK ((elevated_by_kind IS NULL AND elevated_by_id IS NULL AND elevated_by_mode IS NULL
            AND elevated_by_verifier_version IS NULL)
        OR (elevated_by_kind IS NOT NULL AND elevated_by_id IS NOT NULL
            AND elevated_by_mode IS NOT NULL AND elevated_by_verifier_version IS NOT NULL))
);

CREATE TRIGGER changes_no_update BEFORE UPDATE ON changes
BEGIN
    SELECT RAISE(ABORT, 'changes is append-only');
END;

CREATE TRIGGER changes_no_delete BEFORE DELETE ON changes
BEGIN
    SELECT RAISE(ABORT, 'changes is append-only');
END;

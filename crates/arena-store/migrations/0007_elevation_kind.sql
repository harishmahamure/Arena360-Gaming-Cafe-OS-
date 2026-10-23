-- 0007: `changes.elevated_by_kind` records how an elevation was proven (`pin` or `mfa`), not an
-- actor kind, and the new `changes.reason` keeps the reason code of intents that require one.
-- SQLite cannot alter a CHECK, so the table is rebuilt with the same columns plus `reason`, its
-- rows copied in order and its append-only triggers recreated. No row could carry an elevation
-- before this migration (apply() refused every claim), so the mapping below is a formality.

CREATE TABLE changes_new (
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
        CHECK (elevated_by_kind IS NULL OR elevated_by_kind IN ('pin', 'mfa')),
    elevated_by_id              TEXT NULL,
    elevated_by_mode            TEXT NULL
        CHECK (elevated_by_mode IS NULL OR elevated_by_mode IN ('hub', 'cached_verifier')),
    elevated_by_verifier_version INTEGER NULL,
    at                          INTEGER NOT NULL,
    business_day                TEXT NULL,
    fy                          TEXT NULL,
    reason                      TEXT NULL,
    PRIMARY KEY (epoch, seq),
    -- elevated_by is all-or-nothing.
    CHECK ((elevated_by_kind IS NULL AND elevated_by_id IS NULL AND elevated_by_mode IS NULL
            AND elevated_by_verifier_version IS NULL)
        OR (elevated_by_kind IS NOT NULL AND elevated_by_id IS NOT NULL
            AND elevated_by_mode IS NOT NULL AND elevated_by_verifier_version IS NOT NULL))
);

INSERT INTO changes_new (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind, actor_kind,
                         actor_id, elevated_by_kind, elevated_by_id, elevated_by_mode,
                         elevated_by_verifier_version, at, business_day, fy)
SELECT epoch, seq, tbl, row_id, op, row, intent_id, intent_kind, actor_kind, actor_id,
       CASE WHEN elevated_by_kind IS NULL THEN NULL
            WHEN elevated_by_kind = 'owner' THEN 'mfa'
            ELSE 'pin' END,
       elevated_by_id, elevated_by_mode, elevated_by_verifier_version, at, business_day, fy
FROM changes
ORDER BY epoch, seq;

DROP TABLE changes;
ALTER TABLE changes_new RENAME TO changes;

CREATE TRIGGER changes_no_update BEFORE UPDATE ON changes
BEGIN
    SELECT RAISE(ABORT, 'changes is append-only');
END;

CREATE TRIGGER changes_no_delete BEFORE DELETE ON changes
BEGIN
    SELECT RAISE(ABORT, 'changes is append-only');
END;

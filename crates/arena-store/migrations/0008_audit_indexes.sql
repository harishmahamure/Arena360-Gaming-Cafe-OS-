-- 0008: the Hub's audit viewer (Story 1.15).
--
-- Two virtual generated columns on `changes` (computed on read, nothing stored, the
-- append-only triggers untouched): `epoch_counter`, the numeric part of `epoch` (log order is
-- epoch_counter, epoch, seq), and `module`, the domain of `intent_kind` (the text before its
-- first '.', or the whole kind when it has none). Indexes serve every audit page in its keyset
-- order (at, epoch, seq) with no sort, and the lookup of the previous change to one row.
--
-- `read_grants` records each read elevation a manager's PIN granted (`audit.read_granted`).

ALTER TABLE changes ADD COLUMN epoch_counter INTEGER
    GENERATED ALWAYS AS (CAST(substr(epoch, 1, instr(epoch, ':') - 1) AS INTEGER)) VIRTUAL;
ALTER TABLE changes ADD COLUMN module TEXT
    GENERATED ALWAYS AS (CASE WHEN instr(intent_kind, '.') > 0
                              THEN substr(intent_kind, 1, instr(intent_kind, '.') - 1)
                              ELSE intent_kind END) VIRTUAL;

CREATE INDEX changes_at ON changes (at, epoch, seq);
CREATE INDEX changes_actor_at ON changes (actor_kind, actor_id, at, epoch, seq);
CREATE INDEX changes_actor_kind_at ON changes (actor_kind, at, epoch, seq);
CREATE INDEX changes_module_at ON changes (module, at, epoch, seq);
CREATE INDEX changes_row ON changes (tbl, row_id, epoch_counter, epoch, seq);

CREATE TABLE read_grants (
    grant_id           TEXT PRIMARY KEY,
    branch_id          TEXT NOT NULL REFERENCES branch (branch_id),
    permission         TEXT NOT NULL,
    requester_staff_id TEXT NOT NULL,
    approver_staff_id  TEXT NOT NULL,
    verifier_version   INTEGER NOT NULL,
    rev                INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

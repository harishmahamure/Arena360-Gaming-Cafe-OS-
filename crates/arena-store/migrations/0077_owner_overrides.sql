-- 0077: owner overrides (Story 8.19, AD-5).
--
-- When an owner command lands on a row that changed after the owner's `base_rev`, the owner's
-- change wins and the change it overwrote is kept here, keyed by the owner change's
-- `(epoch, seq)`: the overwritten change's intent id, actor and time (Hub clock, ms), taken from
-- the newest `changes` row of that table and row before the command applied. Local bookkeeping
-- outside the change log, like `applied_commands`; written in the transaction of the command, and
-- nothing in `changes` is deleted, so the audit keeps both writes.
CREATE TABLE owner_overrides (
    epoch      TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    tbl        TEXT NOT NULL,
    row_id     TEXT NOT NULL,
    intent_id  TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_id   TEXT NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (epoch, seq)
);

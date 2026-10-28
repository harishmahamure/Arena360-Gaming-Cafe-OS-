-- The branch has one cash drawer, so one open shift at a time. `carried_minor` is the float the
-- screen offered (the last shift's opening float); `float_minor` is what staff confirmed.
CREATE TABLE shifts (
    id            TEXT PRIMARY KEY,
    staff_id      TEXT NOT NULL,
    opened_at     INTEGER NOT NULL,
    float_minor   INTEGER NOT NULL CHECK (float_minor >= 0),
    carried_minor INTEGER NOT NULL CHECK (carried_minor >= 0),
    state         TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    closed_at     INTEGER NULL,
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE UNIQUE INDEX shifts_one_open ON shifts (state) WHERE state = 'open';

-- Every rupee of cash that moves against a shift. Append-only: an Undo writes a compensating
-- row (`reverses` names the row it cancels) and never edits or removes one.
CREATE TABLE cash_events (
    id           TEXT PRIMARY KEY,
    shift_id     TEXT NOT NULL REFERENCES shifts (id),
    kind         TEXT NOT NULL,
    amount_minor INTEGER NOT NULL,
    staff_id     TEXT NOT NULL,
    order_id     TEXT NULL,
    reverses     TEXT NULL REFERENCES cash_events (id),
    rev          INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX cash_events_shift ON cash_events (shift_id);
CREATE INDEX cash_events_order ON cash_events (order_id) WHERE order_id IS NOT NULL;

CREATE TRIGGER cash_events_no_delete BEFORE DELETE ON cash_events
BEGIN
    SELECT RAISE(ABORT, 'cash_events is append-only');
END;

CREATE TRIGGER cash_events_no_update BEFORE UPDATE OF
    id, shift_id, kind, amount_minor, staff_id, order_id, reverses
ON cash_events
BEGIN
    SELECT RAISE(ABORT, 'cash_events is append-only');
END;

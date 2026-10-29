-- 0069: kitchen tickets (Story 7.3). A placement that holds kitchen lines opens one ticket holding
-- only those lines. The ticket's `stage` (new, preparing, ready, served, cancelled) is the single
-- source for the Hub sheet, the KOT, the display and the READY badge. `number` is the branch's
-- running K-<n> counter. `order_id` and `order_line_id` have no foreign keys: a cancelled order
-- or line leaves the ticket as a record. `seat` is "PC 07", "TV 3" or "Counter" (never a player).
-- `advanced_at` and `advanced_by` are the last stage change, for its Undo window.
CREATE TABLE kitchen_tickets (
    id            TEXT PRIMARY KEY,
    order_id      TEXT NOT NULL,
    device_id     TEXT NULL,
    seat          TEXT NOT NULL,
    number        INTEGER NOT NULL CHECK (number > 0),
    stage         TEXT NOT NULL CHECK (stage IN ('new', 'preparing', 'ready', 'served', 'cancelled')),
    placement_id  TEXT NULL,
    placed_at     INTEGER NOT NULL,
    advanced_at   INTEGER NULL,
    advanced_by   TEXT NULL,
    cancel_reason TEXT NULL,
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE UNIQUE INDEX kitchen_tickets_number ON kitchen_tickets (number);
CREATE INDEX kitchen_tickets_order ON kitchen_tickets (order_id);
CREATE INDEX kitchen_tickets_open ON kitchen_tickets (stage) WHERE stage IN ('new', 'preparing', 'ready');

CREATE TABLE kitchen_ticket_lines (
    id            TEXT PRIMARY KEY,
    ticket_id     TEXT NOT NULL REFERENCES kitchen_tickets (id),
    order_line_id TEXT NOT NULL,
    label         TEXT NOT NULL,
    qty           INTEGER NOT NULL CHECK (qty > 0),
    notes         TEXT NULL,
    rev           INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE INDEX kitchen_ticket_lines_ticket ON kitchen_ticket_lines (ticket_id);
CREATE INDEX kitchen_ticket_lines_line ON kitchen_ticket_lines (order_line_id);

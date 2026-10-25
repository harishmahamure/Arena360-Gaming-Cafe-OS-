CREATE TABLE applied_commands (
    command_id      INTEGER PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    kind            TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('applied', 'rejected')),
    applied_seq     INTEGER,
    error           TEXT,
    applied_at      INTEGER NOT NULL,
    reported_at     INTEGER
);

CREATE INDEX applied_commands_unreported ON applied_commands (command_id)
    WHERE reported_at IS NULL;

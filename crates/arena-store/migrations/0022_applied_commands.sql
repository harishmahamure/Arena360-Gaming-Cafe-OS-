-- 0022: owner commands the Hub has resolved. One row per cloud command id, written in the
-- same transaction as the command's changes (or alone for a rejection), so a command is never
-- applied twice. `reported_at` is empty until the cloud acknowledged the result; the poller
-- re-posts unreported rows. Not business state: no change rows, never synced or exported.

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

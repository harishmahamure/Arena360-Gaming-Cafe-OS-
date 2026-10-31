-- Moving a Free Android Hub to a Windows Hub (Story 10.16). `hub_migrations` is the move: one live row
-- per branch, from the Android Hub that is the branch's main Hub to the Windows Hub that pairs with a
-- `migrate` code. The Windows Hub is a `hubs` row held in the branch's standby slot (an Android main
-- Hub can have no standby, so the slot is free) and marked by `hubs.migration_id`, so it is not
-- mistaken for a standby: it can refresh its licence but cannot beat as the main Hub or promote.
-- The Android token stays valid; Stories 10.17-10.19 move the move on from `awaiting_handover`, so
-- their states are named here (a CHECK cannot be widened later).
CREATE TABLE hub_migrations (
    id TEXT PRIMARY KEY NOT NULL,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    from_hub_id TEXT NOT NULL REFERENCES hubs (id),
    to_hub_id TEXT REFERENCES hubs (id),
    -- The PC's name, shown to staff on the Android Hub ("Move this branch to <PC name>").
    target_hub_name TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'awaiting_windows', 'awaiting_handover', 'handing_over', 'handed_over', 'imported',
        'done', 'cancelled'
    )),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX hub_migrations_branch ON hub_migrations (branch_id);

CREATE UNIQUE INDEX hub_migrations_live ON hub_migrations (branch_id)
    WHERE state NOT IN ('done', 'cancelled');

ALTER TABLE hubs ADD COLUMN migration_id TEXT;

-- A migrate code is a pairing code with another purpose. A branch keeps one live code per role and
-- purpose, so a Hub code and a migrate code can wait side by side.
ALTER TABLE pairing_codes ADD COLUMN purpose TEXT NOT NULL DEFAULT 'pair'
    CHECK (purpose IN ('pair', 'migrate'));

DROP INDEX pairing_codes_live;

CREATE UNIQUE INDEX pairing_codes_live ON pairing_codes (branch_id, role, purpose)
    WHERE used_at IS NULL AND invalidated_at IS NULL;

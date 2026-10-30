-- A branch's standby PC (Story 9.18): a second Hub device with role `standby`, paired with its own
-- standby code. A branch keeps at most one live Hub of each role, so the "one live Hub" and "one
-- live code" indexes now count per role. Rows that exist are all `main`.
ALTER TABLE hubs ADD COLUMN role TEXT NOT NULL DEFAULT 'main' CHECK (role IN ('main', 'standby'));

DROP INDEX hubs_one_active;

CREATE UNIQUE INDEX hubs_one_active ON hubs (branch_id, role) WHERE revoked_at IS NULL;

ALTER TABLE pairing_codes ADD COLUMN role TEXT NOT NULL DEFAULT 'main'
    CHECK (role IN ('main', 'standby'));

DROP INDEX pairing_codes_live;

CREATE UNIQUE INDEX pairing_codes_live ON pairing_codes (branch_id, role)
    WHERE used_at IS NULL AND invalidated_at IS NULL;

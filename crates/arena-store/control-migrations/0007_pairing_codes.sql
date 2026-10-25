CREATE TABLE pairing_codes (
    id TEXT PRIMARY KEY NOT NULL,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    code_hash TEXT NOT NULL UNIQUE,
    tier_at_issue TEXT NOT NULL CHECK (tier_at_issue IN ('free', 'basic', 'premium')),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    invalidated_at INTEGER
);

CREATE INDEX pairing_codes_branch ON pairing_codes (branch_id);

CREATE UNIQUE INDEX pairing_codes_live ON pairing_codes (branch_id)
    WHERE used_at IS NULL AND invalidated_at IS NULL;

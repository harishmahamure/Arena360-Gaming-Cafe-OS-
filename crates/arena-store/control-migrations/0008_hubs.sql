CREATE TABLE hubs (
    id TEXT PRIMARY KEY NOT NULL,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    platform TEXT NOT NULL CHECK (platform IN ('windows', 'android')),
    hub_instance_id TEXT NOT NULL,
    version TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    paired_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER
);

CREATE INDEX hubs_branch ON hubs (branch_id);

CREATE UNIQUE INDEX hubs_one_active ON hubs (branch_id) WHERE revoked_at IS NULL;

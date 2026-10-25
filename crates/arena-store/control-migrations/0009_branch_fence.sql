CREATE TABLE branch_fence (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    epoch TEXT NOT NULL,
    hub_id TEXT NOT NULL REFERENCES hubs (id),
    created_at INTEGER NOT NULL
);

ALTER TABLE hubs ADD COLUMN schema_version INTEGER;

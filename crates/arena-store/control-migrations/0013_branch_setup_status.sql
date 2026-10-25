CREATE TABLE branch_setup_status (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    body TEXT NOT NULL,
    reported_at INTEGER NOT NULL
);

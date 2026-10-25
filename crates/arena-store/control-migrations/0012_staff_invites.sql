CREATE TABLE onboarding_progress_new (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    step TEXT NOT NULL CHECK (step IN ('cafe', 'tier', 'install_hub', 'devices', 'rates', 'products', 'staff', 'printer_payments', 'test_session', 'go_live')),
    state TEXT NOT NULL CHECK (state IN ('pending', 'done', 'skipped')),
    completed_at INTEGER,
    PRIMARY KEY (branch_id, step)
);

INSERT INTO onboarding_progress_new (branch_id, step, state, completed_at)
    SELECT branch_id, step, state, completed_at FROM onboarding_progress;

DROP TABLE onboarding_progress;

ALTER TABLE onboarding_progress_new RENAME TO onboarding_progress;

CREATE TABLE staff_invites (
    id TEXT PRIMARY KEY NOT NULL,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    name TEXT NOT NULL,
    phone TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('manager', 'counter', 'kitchen')),
    command_key TEXT NOT NULL,
    setup_expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX staff_invites_branch ON staff_invites (branch_id, created_at);

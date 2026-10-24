CREATE TABLE tenants (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL UNIQUE REFERENCES owners (id),
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE branches (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    name TEXT NOT NULL,
    city TEXT NOT NULL,
    country TEXT NOT NULL CHECK (length(country) = 2),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    timezone TEXT NOT NULL,
    gst_registered INTEGER NOT NULL DEFAULT 0 CHECK (gst_registered IN (0, 1)),
    gstin TEXT,
    branch_code TEXT NOT NULL CHECK (length(branch_code) BETWEEN 3 AND 6),
    code_locked_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (tenant_id, branch_code)
);

CREATE INDEX branches_tenant ON branches (tenant_id, created_at);

CREATE TABLE onboarding_progress (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    step TEXT NOT NULL CHECK (step IN ('cafe', 'tier', 'install_hub', 'devices', 'rates', 'products', 'staff', 'printer_payments', 'test_session', 'go_live')),
    state TEXT NOT NULL CHECK (state IN ('pending', 'done')),
    completed_at INTEGER,
    PRIMARY KEY (branch_id, step)
);

CREATE TABLE entitlements (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    tier TEXT NOT NULL CHECK (tier IN ('free', 'basic', 'premium')),
    source TEXT NOT NULL CHECK (source IN ('free', 'comp', 'paid')),
    addons_json TEXT NOT NULL DEFAULT '[]',
    issued_at INTEGER NOT NULL,
    until INTEGER,
    updated_at INTEGER NOT NULL,
    CHECK ((source = 'free') = (tier = 'free')),
    CHECK (source <> 'comp' OR until IS NOT NULL)
);

CREATE INDEX entitlements_tenant ON entitlements (tenant_id, tier);

CREATE UNIQUE INDEX entitlements_one_free ON entitlements (tenant_id)
    WHERE tier = 'free' AND source = 'free';

CREATE TABLE ops_audit (
    id TEXT PRIMARY KEY NOT NULL,
    at INTEGER NOT NULL,
    operator TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    reason TEXT NOT NULL,
    details_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX ops_audit_target ON ops_audit (target, at);

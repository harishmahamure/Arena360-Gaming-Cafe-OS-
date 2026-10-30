-- The on-demand audit builder (Story 8.36, AD-37). One row per branch and month: the build asked
-- for, where it is (`queued`, `building`, `ready`, `failed`), its progress (0 to 100) and, once
-- ready, the R2 key of `audit-cache/<yyyy-mm>.db.zst` and when it was built. A new request for a
-- failed, expired or abandoned build resets the row (`attempt` counts the tries). `requested_by`
-- and `reason` record who asked; the support path of Story 8.37 always carries a reason.
CREATE TABLE builder_jobs (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    month TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'building', 'ready', 'failed')),
    progress INTEGER NOT NULL DEFAULT 0,
    key TEXT,
    built_at INTEGER,
    error TEXT,
    attempt INTEGER NOT NULL DEFAULT 1,
    requested_by TEXT NOT NULL,
    reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (branch_id, month)
);

CREATE INDEX builder_jobs_queue ON builder_jobs (state, created_at);

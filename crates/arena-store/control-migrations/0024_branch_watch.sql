-- The offline watch (Story 8.35). `branch_opening` caches the opening hours (the branch's
-- `branch.opening_hours` JSON) and time zone that a paid Hub sends in its heartbeat, so the
-- cloud can tell a branch that is silent while it should be open from one that is closed.
-- `branch_watch` holds, per branch, the episode already alerted: `offline_key` is the Hub's
-- `last_seen_at` and `upload_key` the newest upload or summary time of the episode, so an alert
-- is raised once per episode and its id is deterministic.
CREATE TABLE branch_opening (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    opening_hours TEXT NOT NULL,
    timezone TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE branch_watch (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    offline_key INTEGER,
    upload_key INTEGER
);

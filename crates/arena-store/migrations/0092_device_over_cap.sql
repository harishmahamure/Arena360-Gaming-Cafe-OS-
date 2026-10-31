-- 0092: a device over the plan's device limit is read-only (Story 10.12). The Hub recomputes the
-- flag inside the transaction that applies a licence or revokes a device: the oldest devices
-- (by `created_at`, then id) up to the cap stay active and the rest are marked, newest first.
-- A marked device keeps its row, its tile and any running session; only new starts and
-- transfers onto it are refused.

ALTER TABLE devices_meta ADD COLUMN over_cap INTEGER NOT NULL DEFAULT 0 CHECK (over_cap IN (0, 1));

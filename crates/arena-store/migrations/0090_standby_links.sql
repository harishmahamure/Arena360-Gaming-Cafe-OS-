-- 0090: the paired devices the owner linked as this branch's standby Hub (Story 9.19). Only a
-- device listed here may take the replication snapshot or open the replication stream. A
-- Hub-local table: it is not in the change log, so it is never replicated to the standby.
CREATE TABLE standby_links (
    device_id TEXT PRIMARY KEY,
    linked_at INTEGER NOT NULL
);

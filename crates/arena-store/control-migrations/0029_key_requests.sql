-- A replacement Hub's request for the branch key (Story 9.9, AD-24). The Hub of a branch with
-- history opens in "Restore this branch", makes a one-time key pair and sends the public half
-- (`POST /v1/kek/request`). Story 9.10 shows the pending request to the owner, decides it, expires
-- it after 7 days and checks it for replay. Nothing here holds key material beyond the one-time
-- public key.
--
-- One row per Hub instance of a branch. Only a `pending` row is refreshed by the Hub asking again;
-- a decided or expired row stays as it is.
CREATE TABLE key_requests (
    id TEXT PRIMARY KEY,
    branch_id TEXT NOT NULL REFERENCES branches (id),
    hub_instance_id TEXT NOT NULL,
    public_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'denied', 'expired')),
    requested_at INTEGER NOT NULL,
    decided_at INTEGER NULL,
    decided_by TEXT NULL,
    UNIQUE (branch_id, hub_instance_id)
);

CREATE INDEX key_requests_pending ON key_requests (branch_id, state);

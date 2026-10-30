-- The Cloudflare Tunnel of a branch with the `tunnel` licence flag (Story 8.39). One row per
-- branch: the tunnel and its Access application as the provider made them, whether the tunnel
-- credential was handed to the Hub (`delivered_at`; the credential itself is never stored) and
-- where the row is: `ready`, or `deleting` after the flag went away (the delete is retried on
-- every heartbeat until the provider has removed the Access application and the tunnel).
CREATE TABLE branch_tunnels (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    hostname TEXT NOT NULL,
    tunnel_id TEXT NOT NULL,
    access_app_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('ready', 'deleting')),
    delivered_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

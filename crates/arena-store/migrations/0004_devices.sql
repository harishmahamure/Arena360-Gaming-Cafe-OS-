-- 0004: devices that authenticate on the LAN with a token (`paired` apps and `kds` screens;
-- phones are anonymous). `token_hash` is the argon2id PHC string of the token's secret, never
-- the token. A revoked device keeps its row with `revoked_at` set. Written only by apply()
-- (`device.register`, `device.revoke`, `device.version_reported`), so every write has change
-- rows.

CREATE TABLE devices (
    device_id        TEXT PRIMARY KEY,
    branch_id        TEXT NOT NULL REFERENCES branch (branch_id),
    class            TEXT NOT NULL CHECK (class IN ('paired', 'kds')),
    label            TEXT NOT NULL,
    token_hash       TEXT NOT NULL,
    reported_version TEXT NULL,
    revoked_at       INTEGER NULL,
    rev              INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

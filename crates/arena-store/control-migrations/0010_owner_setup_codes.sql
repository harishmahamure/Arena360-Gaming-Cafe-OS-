CREATE TABLE owner_setup_codes (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    code_hash TEXT NOT NULL,
    hub_expires_at INTEGER NOT NULL,
    ciphertext BLOB,
    nonce BLOB,
    created_at INTEGER NOT NULL,
    shown_at INTEGER
);

-- 0003: the owner's password, authenticator app (TOTP) and backup codes (Story 2.5). Times are
-- UTC milliseconds. Nothing here is stored in the clear: the password and each backup code are
-- argon2id PHC strings, and the TOTP secret is AES-256-GCM ciphertext (the owner id is the AAD).

ALTER TABLE owners ADD COLUMN password_hash TEXT;
ALTER TABLE owners ADD COLUMN password_set_at INTEGER;

-- One row per owner. `confirmed_at` is set when the first code verifies (the owner is then
-- `enrolled`); until then each start replaces the secret. `last_step` is the newest accepted
-- 30 s step: a code for that step or an earlier one is refused (no replay).
CREATE TABLE owner_mfa (
    owner_id TEXT PRIMARY KEY NOT NULL REFERENCES owners (id),
    secret_ct BLOB NOT NULL,
    secret_nonce BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    confirmed_at INTEGER,
    last_step INTEGER,
    backup_ack_at INTEGER
);

-- Single-use backup codes; regenerating replaces every row of the owner.
CREATE TABLE owner_backup_codes (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES owners (id),
    code_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    used_at INTEGER
);

CREATE INDEX owner_backup_codes_owner ON owner_backup_codes (owner_id);

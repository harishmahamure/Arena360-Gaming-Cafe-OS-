-- 0005: roles, their permission rows and staff. Written only by apply() (`branch.bootstrap`
-- seeds the built-in roles; `role.upsert` and `staff.upsert` do the rest), so every write has
-- change rows. Hashes are argon2id PHC strings, never the secret.

CREATE TABLE roles (
    role_id    TEXT PRIMARY KEY,
    branch_id  TEXT NOT NULL REFERENCES branch (branch_id),
    name       TEXT NOT NULL COLLATE NOCASE,
    builtin    INTEGER NOT NULL CHECK (builtin IN (0, 1)),
    rev        INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (branch_id, name)
);

CREATE TABLE role_perms (
    role_id    TEXT NOT NULL REFERENCES roles (role_id),
    permission TEXT NOT NULL,
    effect     TEXT NOT NULL CHECK (effect IN ('allow', 'deny')),
    PRIMARY KEY (role_id, permission)
);

CREATE TABLE staff (
    staff_id             TEXT PRIMARY KEY,
    branch_id            TEXT NOT NULL REFERENCES branch (branch_id),
    login                TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name         TEXT NOT NULL,
    role_id              TEXT NOT NULL REFERENCES roles (role_id),
    password_hash        TEXT NULL,
    pin_hash             TEXT NULL,
    pin_verifier_version INTEGER NOT NULL DEFAULT 1,
    active               INTEGER NOT NULL CHECK (active IN (0, 1)),
    rev                  INTEGER NOT NULL,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);

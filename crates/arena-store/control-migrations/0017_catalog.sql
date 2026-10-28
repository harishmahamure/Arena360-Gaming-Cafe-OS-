-- The global game catalogue (Story 6.1) and the append-only admin audit. A game is keyed by
-- name + launcher (case-insensitive). `changed_version` is the catalogue version at which the
-- game last changed in a way a Hub must hear about; the manifest serves rows beyond a Hub's
-- version. Media are content-addressed objects in the object store (`catalog/<sha256>.<ext>`).
CREATE TABLE catalog_games (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    launcher TEXT NOT NULL,
    category TEXT NOT NULL,
    age_rating TEXT NOT NULL,
    published INTEGER NOT NULL CHECK (published IN (0, 1)),
    rev INTEGER NOT NULL CHECK (rev >= 1),
    changed_version INTEGER NOT NULL CHECK (changed_version >= 0),
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);

CREATE UNIQUE INDEX catalog_games_key ON catalog_games (lower(name), lower(launcher));

CREATE INDEX catalog_games_changed ON catalog_games (changed_version);

CREATE TABLE catalog_exe_signatures (
    game_id TEXT NOT NULL REFERENCES catalog_games (id),
    exe_name TEXT NOT NULL,
    product_name TEXT NOT NULL,
    publisher TEXT NOT NULL,
    sha256 TEXT NOT NULL DEFAULT ''
);

CREATE UNIQUE INDEX catalog_exe_signatures_key
    ON catalog_exe_signatures (game_id, lower(exe_name), sha256);

CREATE TABLE catalog_media (
    game_id TEXT NOT NULL REFERENCES catalog_games (id),
    kind TEXT NOT NULL CHECK (kind IN ('cover', 'video')),
    sha256 TEXT NOT NULL,
    ext TEXT NOT NULL,
    bytes INTEGER NOT NULL CHECK (bytes > 0),
    flash_screened INTEGER NOT NULL DEFAULT 0 CHECK (flash_screened IN (0, 1)),
    PRIMARY KEY (game_id, kind)
);

-- The one `catalog_version` counter.
CREATE TABLE catalog_state (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    version INTEGER NOT NULL CHECK (version >= 0)
);

INSERT INTO catalog_state (id, version) VALUES (1, 0);

CREATE TABLE admin_audit (
    id TEXT PRIMARY KEY NOT NULL,
    at INTEGER NOT NULL,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('admin', 'system')),
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    reason TEXT,
    summary TEXT NOT NULL,
    request_id TEXT NOT NULL
);

CREATE INDEX admin_audit_at ON admin_audit (at);

CREATE TRIGGER admin_audit_no_update BEFORE UPDATE ON admin_audit
BEGIN
    SELECT RAISE(ABORT, 'admin_audit is append-only');
END;

CREATE TRIGGER admin_audit_no_delete BEFORE DELETE ON admin_audit
BEGIN
    SELECT RAISE(ABORT, 'admin_audit is append-only');
END;

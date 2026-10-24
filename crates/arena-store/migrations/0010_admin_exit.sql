-- 0010: kiosk admin exits (Story 1.20).
--
-- `device_admin_exits` holds one row per admin exit: a manager PIN at the kiosk checked by the
-- Hub (`hub`) or offline against the cached verifier (`cached_verifier`), or a remote unlock
-- from the Hub floor (`remote`). Written only by apply() (`device.admin_exit`,
-- `device.admin_unlock`, `device.admin_relock`), so every write has change rows. An offline
-- exit whose verifier was older than the one in effect at the time is kept, `flagged = 1`.
--
-- `verifier_history` records when each manager-verifier version came into effect, so apply()
-- can tell whether an offline exit used a stale verifier. It is derived from the staff and role
-- rows inside the same transaction (like `hub_meta`, not a change-log table);
-- `hub_meta.verifier_counter` is the monotonic part of the version.

CREATE TABLE device_admin_exits (
    exit_id     TEXT PRIMARY KEY,
    branch_id   TEXT NOT NULL REFERENCES branch (branch_id),
    device_id   TEXT NOT NULL REFERENCES devices (device_id),
    staff_id    TEXT NOT NULL,
    mode        TEXT NOT NULL CHECK (mode IN ('hub', 'cached_verifier', 'remote')),
    started_at  INTEGER NOT NULL,
    relock_at   INTEGER NOT NULL,
    relocked_at INTEGER NULL,
    flagged     INTEGER NOT NULL CHECK (flagged IN (0, 1)),
    flag_reason TEXT NULL,
    rev         INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX device_admin_exits_device ON device_admin_exits (device_id, started_at);

CREATE TABLE verifier_history (
    version    INTEGER PRIMARY KEY,
    valid_from INTEGER NOT NULL,
    digest     TEXT NOT NULL,
    -- The staff ids the verifier of this version listed, newline-separated (ids only, never a
    -- PIN or hash): an offline exit checked against it may only name one of them.
    staff_ids  TEXT NOT NULL
);

INSERT INTO hub_meta (key, value) VALUES ('verifier_counter', '0')
    ON CONFLICT (key) DO NOTHING;

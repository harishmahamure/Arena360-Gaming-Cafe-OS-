CREATE TABLE trusted_clock (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    cloud_ms      INTEGER NOT NULL,
    mono_ms       INTEGER NOT NULL,
    wall_delta_ms INTEGER NOT NULL
);

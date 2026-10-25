CREATE TABLE floor_zones (
    zone_id    TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    rev        INTEGER NOT NULL,
    deleted_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX floor_zones_name ON floor_zones (name COLLATE NOCASE) WHERE deleted_at IS NULL;

ALTER TABLE devices_meta ADD COLUMN zone_id TEXT NULL REFERENCES floor_zones (zone_id);
ALTER TABLE devices_meta ADD COLUMN floor_x INTEGER NULL CHECK (floor_x IS NULL OR floor_x >= 0);
ALTER TABLE devices_meta ADD COLUMN floor_y INTEGER NULL CHECK (floor_y IS NULL OR floor_y >= 0);

CREATE UNIQUE INDEX devices_meta_cell ON devices_meta (zone_id, floor_x, floor_y)
    WHERE zone_id IS NOT NULL AND deleted_at IS NULL;

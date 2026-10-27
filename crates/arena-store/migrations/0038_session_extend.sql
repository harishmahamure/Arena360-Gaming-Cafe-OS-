ALTER TABLE sessions ADD COLUMN extended_min INTEGER NOT NULL DEFAULT 0 CHECK (extended_min >= 0);
ALTER TABLE orders ADD COLUMN extends_session_id TEXT NULL REFERENCES sessions (id);

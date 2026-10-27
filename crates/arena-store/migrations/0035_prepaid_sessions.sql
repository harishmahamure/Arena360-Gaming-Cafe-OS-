ALTER TABLE sessions ADD COLUMN reserved_minor INTEGER NOT NULL DEFAULT 0 CHECK (reserved_minor >= 0);

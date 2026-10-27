ALTER TABLE sessions ADD COLUMN controller_check TEXT NULL;
ALTER TABLE sessions ADD COLUMN controller_checked_at INTEGER NULL;
ALTER TABLE sessions ADD COLUMN controller_checked_by TEXT NULL;
UPDATE sessions SET controller_check = 'na' WHERE ended_at IS NOT NULL;

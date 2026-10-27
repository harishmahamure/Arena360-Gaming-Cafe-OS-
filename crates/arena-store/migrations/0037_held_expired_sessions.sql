DROP INDEX sessions_one_live;
CREATE UNIQUE INDEX sessions_one_live ON sessions (device_id)
    WHERE state IN ('active', 'paused', 'grace')
       OR (state = 'expired' AND ended_at IS NULL);

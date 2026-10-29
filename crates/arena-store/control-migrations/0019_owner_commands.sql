-- Owner commands (Story 8.18): a fan-out to "all branches" shares one `group_id` (the owner's
-- idempotency key), and the Hub's heartbeat reports the newest command schema it understands.
ALTER TABLE commands ADD COLUMN group_id TEXT;
ALTER TABLE hubs ADD COLUMN command_schema_max INTEGER;

CREATE INDEX commands_group ON commands (group_id) WHERE group_id IS NOT NULL;

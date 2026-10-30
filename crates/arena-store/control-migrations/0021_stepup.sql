-- Step-up MFA for sensitive owner commands (Story 8.25). `stepup_at` is the session's proof that the
-- owner passed a second factor (valid for 5 minutes; a refresh carries it to the new row); the
-- `stepup_sms_*` pair holds the text-message code sent for a step-up. `commands.elevated_by` is the
-- owner id when the post was elevated; the cloud hands it to the Hub.
ALTER TABLE owner_sessions ADD COLUMN stepup_at INTEGER;
ALTER TABLE owner_sessions ADD COLUMN stepup_sms_hash TEXT;
ALTER TABLE owner_sessions ADD COLUMN stepup_sms_sent_at INTEGER;
ALTER TABLE commands ADD COLUMN elevated_by TEXT;

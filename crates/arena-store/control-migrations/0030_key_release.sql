-- The owner releases the branch key to a replacement Hub (Story 9.10, AD-24).
--
-- `wrapped_kek` is the branch key re-wrapped for the request's one-time public key (an age file,
-- base64): only the waiting Hub's private half can open it. It is written when the owner approves
-- and cleared when the Hub fetches it (`fetched_at`), so the blob is handed out once and does not
-- sit in the table afterwards. `pc_name` is the Hub PC's sanitised host name, shown to the owner in
-- "Release branch key to <PC name>".
--
-- A pending request older than 7 days is flipped to `expired` by the sweep that runs before every
-- list, decision and fetch; a denied or expired row stays decided.
ALTER TABLE key_requests ADD COLUMN wrapped_kek TEXT NULL;
ALTER TABLE key_requests ADD COLUMN fetched_at INTEGER NULL;
ALTER TABLE key_requests ADD COLUMN pc_name TEXT NULL;

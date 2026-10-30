-- 0085: a replacement Hub waits to be restored (Story 9.9, AD-11).
--
-- A Hub paired with the code of a branch that already has history keeps no branch of its own: it
-- opens in "Restore this branch", writes nothing and serves no device until a restore completes.
-- The flag lives on the identity row, outside the change log, so a restart or a lost WAN finds the
-- Hub still waiting. `restore_branch_name` is the name the screen shows (the Hub has no branch
-- row yet). The one-time key pair for the branch key release is kept in the OS keystore, never
-- here.
ALTER TABLE hub_identity ADD COLUMN restore_pending INTEGER NOT NULL DEFAULT 0 CHECK (restore_pending IN (0, 1));
ALTER TABLE hub_identity ADD COLUMN restore_branch_name TEXT NULL;

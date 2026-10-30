-- 0028: the fork point of the branch fence (Story 9.3, AD-24).
--
-- `fork_seq` is the `seq` of the previous epoch the fence's current epoch was forked from, as the
-- claim (or the heartbeat that advanced the fence) declared it. It is the fork point of the
-- command re-queue: commands applied past it at the old epoch return to `pending`. NULL for a
-- fence that was never claimed or advanced.
ALTER TABLE branch_fence ADD COLUMN fork_seq INTEGER;

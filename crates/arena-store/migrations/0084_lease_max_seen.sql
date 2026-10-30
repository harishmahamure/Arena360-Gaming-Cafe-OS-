-- 0084: the highest epoch counter this Hub has seen (Story 9.3, AD-24).
--
-- Raised (never lowered) from heartbeat answers, claim replies and, once LAN epochs exist, the
-- LAN. A Hub that takes a lease offline mints `max(held counter, max_seen_counter) + 1`, so a
-- provisional epoch never repeats a counter the cloud or a peer already issued. It is a column
-- of the lease row, read and raised by its own functions; `write_lease` leaves it alone.
ALTER TABLE lease ADD COLUMN max_seen_counter INTEGER NOT NULL DEFAULT 0 CHECK (max_seen_counter >= 0);

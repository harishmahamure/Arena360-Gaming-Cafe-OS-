-- 0006: deferred locale settings and the business day / financial year of every change.
--
-- A `branch.tz` or `branch.day_start_min` change saved during business day D keeps the old
-- value in `prior_value` and applies from `effective_from` (D+1, a `YYYY-MM-DD` business day).
-- Every other setting takes effect at once (both NULL). `apply()` stamps `business_day` and
-- `fy` on each new change row; rows written before this migration keep NULL.

ALTER TABLE settings ADD COLUMN prior_value TEXT NULL;
ALTER TABLE settings ADD COLUMN effective_from TEXT NULL;

ALTER TABLE changes ADD COLUMN business_day TEXT NULL;
ALTER TABLE changes ADD COLUMN fy TEXT NULL;

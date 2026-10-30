-- The cloud backup retention sweep (Story 9.15, AD-36). `branch_retention` remembers, per branch,
-- the retention (days) of the last paid tier the daily sweep saw (`paid_days`), the retention it
-- had before the last change (`prev_days`) and when the change was seen (`changed_at`). After a
-- move to a shorter plan the sweep keeps the longer `prev_days` for 30 days; a lapse to Free
-- keeps `paid_days` as it is.
CREATE TABLE branch_retention (
    branch_id TEXT PRIMARY KEY NOT NULL REFERENCES branches (id),
    paid_days INTEGER NOT NULL CHECK (paid_days > 0),
    prev_days INTEGER NOT NULL CHECK (prev_days > 0),
    changed_at INTEGER NOT NULL
);

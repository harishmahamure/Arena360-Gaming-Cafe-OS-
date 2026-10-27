-- Ad metrics of Free branches (Story 4.14): non-personal daily totals per branch, business day
-- and slot, and the Hub's acknowledged cursor for each of the two kinds. No tenant name, device,
-- creative or advertiser id is stored.
CREATE TABLE ad_impression_daily (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    day TEXT NOT NULL,
    slot TEXT NOT NULL CHECK (slot IN ('banner', 'interstitial')),
    count INTEGER NOT NULL CHECK (count >= 0),
    PRIMARY KEY (branch_id, day, slot)
);

CREATE TABLE ad_request_daily (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    day TEXT NOT NULL,
    slot TEXT NOT NULL CHECK (slot IN ('banner', 'interstitial')),
    count INTEGER NOT NULL CHECK (count >= 0),
    PRIMARY KEY (branch_id, day, slot)
);

-- The last Hub row id added to the totals, per branch and kind. It moves in the same
-- transaction as the totals.
CREATE TABLE ad_cursor (
    branch_id TEXT NOT NULL REFERENCES branches (id),
    kind TEXT NOT NULL CHECK (kind IN ('impression', 'request')),
    through_id INTEGER NOT NULL CHECK (through_id >= 0),
    PRIMARY KEY (branch_id, kind)
);

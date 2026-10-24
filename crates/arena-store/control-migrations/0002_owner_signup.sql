-- 0002: owner sign-up with a phone one-time code (Story 2.4). Ids are ULIDs, times are UTC
-- milliseconds, enums are TEXT with a CHECK.

CREATE TABLE owners (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    phone_e164 TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    cafe_name TEXT NOT NULL,
    city TEXT NOT NULL,
    mfa_state TEXT NOT NULL CHECK (mfa_state IN ('pending', 'enrolled')),
    created_at INTEGER NOT NULL
);

-- `code_hash` is HMAC-SHA256(pepper, "otp\0" || id || code); `payload_json` is the submitted
-- form, used to create the owner once the code is verified, and set to NULL as soon as the
-- challenge is consumed or voided. Challenges expired for over 24 h are deleted.
CREATE TABLE otp_challenges (
    id TEXT PRIMARY KEY NOT NULL,
    purpose TEXT NOT NULL CHECK (purpose IN ('signup')),
    phone_e164 TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    payload_json TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    consumed_at INTEGER
);

CREATE INDEX otp_challenges_phone ON otp_challenges (phone_e164, created_at);
CREATE INDEX otp_challenges_expires ON otp_challenges (expires_at);

-- Tokens are stored only as SHA-256 hashes. A family shares one sign-in; presenting a rotated
-- refresh token revokes the whole family.
CREATE TABLE owner_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES owners (id),
    family_id TEXT NOT NULL,
    access_hash TEXT NOT NULL UNIQUE,
    access_expires_at INTEGER NOT NULL,
    refresh_hash TEXT NOT NULL UNIQUE,
    refresh_expires_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX owner_sessions_family ON owner_sessions (family_id);
CREATE INDEX owner_sessions_owner ON owner_sessions (owner_id);
CREATE INDEX owner_sessions_refresh_expires ON owner_sessions (refresh_expires_at);

-- Counts per key per window start (a minute bucket for the OTP send limit).
CREATE TABLE rate_limits (
    key TEXT NOT NULL,
    window_start INTEGER NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (key, window_start)
);

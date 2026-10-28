-- The notification outbox (Story 5.19). A settle whose receipt choice is WhatsApp writes one
-- `receipt` row in the same transaction as the sale: purpose `transactional`, the consent policy
-- version the player agreed to, the order and `not_before` = the end of the Undo window. The row
-- names the player, never the phone number: the drain (Epic 8) decrypts the recipient when it
-- sends. Unlike `side_effect_outbox` this table is in the change log, so a standby and the audit
-- see it. `order_id` has no foreign key: an Undo deletes a counter sale or plan sale, and its
-- `skipped` row stays behind to show the receipt was never sent.
CREATE TABLE notify_outbox (
    id                     TEXT PRIMARY KEY,
    purpose                TEXT NOT NULL CHECK (purpose IN ('transactional')),
    kind                   TEXT NOT NULL CHECK (kind IN ('receipt')),
    order_id               TEXT NOT NULL,
    player_id              TEXT NOT NULL REFERENCES players (player_id),
    consent_policy_version TEXT NOT NULL,
    not_before             INTEGER NOT NULL,
    state                  TEXT NOT NULL CHECK (state IN ('queued', 'sent', 'failed', 'skipped')),
    rev                    INTEGER NOT NULL,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);

CREATE INDEX notify_outbox_due ON notify_outbox (not_before, id) WHERE state = 'queued';
CREATE INDEX notify_outbox_order ON notify_outbox (order_id, kind);

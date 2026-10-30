-- Owner alerts (Story 8.33). `owner_alerts` keeps what was raised for a café's branch: the kind and
-- its display parameters only, never a phone, name or email (the text is rendered from the kind
-- when read). Rows older than 90 days are deleted on each raise. `owner_alert_deliveries` is the
-- once-only guard of web push: one row per alert and push subscription, claimed before the push
-- is sent and removed with the alert. WhatsApp is guarded by the `send_ledger` row
-- `<alert id>.wa`. `push_subscriptions` are the browsers an owner turned push on in (the endpoint
-- is a secret and never logged); `owner_alert_settings` holds the owner's channel switches.
CREATE TABLE push_subscriptions (
    id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL REFERENCES owners (id),
    endpoint TEXT NOT NULL UNIQUE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    user_agent TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    last_success_at INTEGER
);

CREATE INDEX push_subscriptions_owner ON push_subscriptions (owner_user_id, created_at);

CREATE TABLE owner_alerts (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    branch_id TEXT NOT NULL REFERENCES branches (id),
    kind TEXT NOT NULL,
    params TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE INDEX owner_alerts_tenant ON owner_alerts (tenant_id, created_at DESC, id DESC);

CREATE TABLE owner_alert_deliveries (
    alert_id TEXT NOT NULL REFERENCES owner_alerts (id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    PRIMARY KEY (alert_id, target)
);

CREATE TABLE owner_alert_settings (
    owner_id TEXT PRIMARY KEY NOT NULL REFERENCES owners (id),
    whatsapp_enabled INTEGER NOT NULL DEFAULT 1 CHECK (whatsapp_enabled IN (0, 1))
);

-- Version 1 of the owner alert message: the branch name and the alert's plain-text line. The
-- WhatsApp and DLT template ids are the names it is submitted under.
INSERT INTO message_templates
    (kind, version, whatsapp_template_id, sms_dlt_template_id, sms_text, param_names, approved_at)
VALUES
    ('owner_alert', 1, 'arena360_owner_alert_v1', 'DLT-ARENA360-OWNERALERT-1',
     'Arena 360 alert, {branch}: {text}',
     '["branch","text"]', 1790000000000);

-- Player and owner messages sent by the cloud (Story 8.28). `message_templates` holds each
-- kind's approved versions (the WhatsApp template id, the DLT template id and text of the SMS
-- fallback, and the parameter names in WhatsApp order); a change is a new version, so rows of
-- `send_ledger` keep the version they were sent with. `send_ledger` has one row per Hub outbox
-- row (`outbox_id` unique): the recipient is stored only as a salted hash, never the number or
-- the text. `in_flight` is 1 while a send runs or ended with an unknown outcome; such a row is
-- never sent again.
CREATE TABLE message_templates (
    kind TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 1),
    whatsapp_template_id TEXT NOT NULL,
    sms_dlt_template_id TEXT NOT NULL,
    sms_text TEXT NOT NULL,
    param_names TEXT NOT NULL,
    approved_at INTEGER,
    PRIMARY KEY (kind, version)
);

CREATE TABLE send_ledger (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants (id),
    branch_id TEXT NOT NULL REFERENCES branches (id),
    outbox_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    purpose TEXT NOT NULL,
    consent_policy_version TEXT NOT NULL,
    recipient_hash TEXT NOT NULL,
    template_version INTEGER NOT NULL,
    channel TEXT NOT NULL CHECK (channel IN ('whatsapp', 'sms')),
    state TEXT NOT NULL CHECK (state IN ('queued', 'sent', 'failed')),
    in_flight INTEGER NOT NULL DEFAULT 0 CHECK (in_flight IN (0, 1)),
    attempts INTEGER NOT NULL DEFAULT 0,
    attempt_log TEXT NOT NULL DEFAULT '[]',
    error_code TEXT,
    provider_message_id TEXT,
    created_at INTEGER NOT NULL,
    sent_at INTEGER
);

CREATE INDEX send_ledger_branch ON send_ledger (branch_id, created_at);

-- Version 1 of the five kinds. The WhatsApp and DLT template ids are the names the templates are
-- submitted under; the operator adds a new version when the provider approves other ids.
INSERT INTO message_templates
    (kind, version, whatsapp_template_id, sms_dlt_template_id, sms_text, param_names, approved_at)
VALUES
    ('credit_reminder', 1, 'arena360_credit_reminder_v1', 'DLT-ARENA360-CREDIT-1',
     'Hi {name}, you owe {amount} at {cafe}. Please pay at the counter. - Arena 360',
     '["name","amount","cafe"]', 1790000000000),
    ('low_balance', 1, 'arena360_low_balance_v1', 'DLT-ARENA360-LOWBAL-1',
     'Hi {name}, your wallet at {cafe} is down to {balance}. Top up at the counter. - Arena 360',
     '["name","balance","cafe"]', 1790000000000),
    ('session_ending', 1, 'arena360_session_ending_v1', 'DLT-ARENA360-ENDING-1',
     'Hi {name}, your session at {cafe} ends in {minutes} min. Extend at the counter. - Arena 360',
     '["name","minutes","cafe"]', 1790000000000),
    ('console_5min', 1, 'arena360_console_5min_v1', 'DLT-ARENA360-CONSOLE-1',
     'Hi {name}, 5 minutes left on {console} at {cafe}. - Arena 360',
     '["name","console","cafe"]', 1790000000000),
    ('receipt', 1, 'arena360_receipt_v1', 'DLT-ARENA360-RECEIPT-1',
     'Hi {name}, receipt {receipt_no} from {cafe}: {amount}. Thank you! - Arena 360',
     '["name","receipt_no","amount","cafe"]', 1790000000000);

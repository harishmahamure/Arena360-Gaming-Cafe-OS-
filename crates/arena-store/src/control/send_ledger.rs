//! Message templates and the send ledger (Story 8.28). The ledger is the cloud's exactly-once
//! guard: one row per Hub outbox row, claimed before anything is sent.

use super::{
    ControlDb, ControlError, SqlValue, col_int, col_opt_int, col_opt_text, col_text, text,
};

/// One approved version of a message kind's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTemplate {
    pub kind: String,
    pub version: u32,
    pub whatsapp_template_id: String,
    pub sms_dlt_template_id: String,
    /// The SMS fallback text, with `{name}` placeholders.
    pub sms_text: String,
    /// The parameter names, in the order WhatsApp takes them.
    pub param_names: Vec<String>,
    pub approved_at: Option<i64>,
}

/// A template version to add; its `version` is the next one for the kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTemplate<'a> {
    pub kind: &'a str,
    pub whatsapp_template_id: &'a str,
    pub sms_dlt_template_id: &'a str,
    pub sms_text: &'a str,
    pub param_names: &'a [String],
    pub approved_at: Option<i64>,
}

/// A ledger row about to be inserted (state `queued`, channel WhatsApp, no attempt yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLedgerRow<'a> {
    pub id: &'a str,
    pub tenant_id: &'a str,
    pub branch_id: &'a str,
    pub outbox_id: &'a str,
    pub kind: &'a str,
    pub purpose: &'a str,
    pub consent_policy_version: &'a str,
    pub recipient_hash: &'a str,
    pub template_version: u32,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    pub id: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub outbox_id: String,
    pub kind: String,
    pub purpose: String,
    pub consent_policy_version: String,
    pub recipient_hash: String,
    pub template_version: u32,
    pub channel: String,
    pub state: String,
    pub in_flight: bool,
    pub attempts: u32,
    /// A JSON array with one entry per provider attempt.
    pub attempt_log: String,
    pub error_code: Option<String>,
    pub provider_message_id: Option<String>,
    pub created_at: i64,
    pub sent_at: Option<i64>,
}

/// What a claimed row becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement<'a> {
    /// `queued`, `sent` or `failed`.
    pub state: &'a str,
    /// The channel of the last attempt, or the one to try next.
    pub channel: &'a str,
    /// Keep the row claimed: the outcome is unknown, so it is never sent again.
    pub in_flight: bool,
    pub attempts: u32,
    pub attempt_log: &'a str,
    pub error_code: Option<&'a str>,
    pub provider_message_id: Option<&'a str>,
    pub sent_at: Option<i64>,
}

const TEMPLATE_COLUMNS: &str = "kind, version, whatsapp_template_id, sms_dlt_template_id, \
     sms_text, param_names, approved_at";

fn count(r: &[SqlValue], i: usize) -> Result<u32, ControlError> {
    u32::try_from(col_int(r, i)?).map_err(|_| ControlError::Db("count out of range".into()))
}

fn template_from(r: &[SqlValue]) -> Result<MessageTemplate, ControlError> {
    let names = col_text(r, 5)?;
    Ok(MessageTemplate {
        kind: col_text(r, 0)?,
        version: count(r, 1)?,
        whatsapp_template_id: col_text(r, 2)?,
        sms_dlt_template_id: col_text(r, 3)?,
        sms_text: col_text(r, 4)?,
        param_names: serde_json::from_str(&names)
            .map_err(|_| ControlError::Db("template param_names is not a list".into()))?,
        approved_at: col_opt_int(r, 6)?,
    })
}

const LEDGER_COLUMNS: &str = "id, tenant_id, branch_id, outbox_id, kind, purpose, \
     consent_policy_version, recipient_hash, template_version, channel, state, in_flight, \
     attempts, attempt_log, error_code, provider_message_id, created_at, sent_at";

fn ledger_from(r: &[SqlValue]) -> Result<LedgerRow, ControlError> {
    Ok(LedgerRow {
        id: col_text(r, 0)?,
        tenant_id: col_text(r, 1)?,
        branch_id: col_text(r, 2)?,
        outbox_id: col_text(r, 3)?,
        kind: col_text(r, 4)?,
        purpose: col_text(r, 5)?,
        consent_policy_version: col_text(r, 6)?,
        recipient_hash: col_text(r, 7)?,
        template_version: count(r, 8)?,
        channel: col_text(r, 9)?,
        state: col_text(r, 10)?,
        in_flight: col_int(r, 11)? != 0,
        attempts: count(r, 12)?,
        attempt_log: col_text(r, 13)?,
        error_code: col_opt_text(r, 14)?,
        provider_message_id: col_opt_text(r, 15)?,
        created_at: col_int(r, 16)?,
        sent_at: col_opt_int(r, 17)?,
    })
}

impl ControlDb {
    /// The newest approved version of `kind`'s template.
    pub async fn current_template(
        &self,
        kind: &str,
    ) -> Result<Option<MessageTemplate>, ControlError> {
        let rows = self
            .query(
                &format!(
                    "SELECT {TEMPLATE_COLUMNS} FROM message_templates \
                     WHERE kind = ? AND approved_at IS NOT NULL ORDER BY version DESC LIMIT 1"
                ),
                vec![text(kind)],
            )
            .await?;
        rows.first().map(|r| template_from(r)).transpose()
    }

    /// Stores `new` as the next version of its kind and answers that version.
    pub async fn add_template_version(&self, new: &NewTemplate<'_>) -> Result<u32, ControlError> {
        let names = serde_json::to_string(new.param_names)
            .map_err(|e| ControlError::Db(format!("param_names: {e}")))?;
        self.execute(
            "INSERT INTO message_templates (kind, version, whatsapp_template_id, \
             sms_dlt_template_id, sms_text, param_names, approved_at) \
             SELECT ?1, COALESCE(MAX(version), 0) + 1, ?2, ?3, ?4, ?5, ?6 \
             FROM message_templates WHERE kind = ?1",
            vec![
                text(new.kind),
                text(new.whatsapp_template_id),
                text(new.sms_dlt_template_id),
                text(new.sms_text),
                text(&names),
                new.approved_at.map_or(SqlValue::Null, SqlValue::Integer),
            ],
        )
        .await?;
        let rows = self
            .query(
                "SELECT MAX(version) FROM message_templates WHERE kind = ?",
                vec![text(new.kind)],
            )
            .await?;
        rows.first().map_or(Ok(0), |r| count(r, 0))
    }

    /// Inserts `new` unless its `outbox_id` already has a row. Answers whether it was inserted.
    pub async fn ledger_insert(&self, new: &NewLedgerRow<'_>) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "INSERT INTO send_ledger (id, tenant_id, branch_id, outbox_id, kind, purpose, \
                 consent_policy_version, recipient_hash, template_version, channel, state, \
                 in_flight, attempts, attempt_log, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'whatsapp', 'queued', 0, 0, '[]', ?) \
                 ON CONFLICT (outbox_id) DO NOTHING",
                vec![
                    text(new.id),
                    text(new.tenant_id),
                    text(new.branch_id),
                    text(new.outbox_id),
                    text(new.kind),
                    text(new.purpose),
                    text(new.consent_policy_version),
                    text(new.recipient_hash),
                    SqlValue::Integer(i64::from(new.template_version)),
                    SqlValue::Integer(new.now),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn ledger_by_outbox(
        &self,
        outbox_id: &str,
    ) -> Result<Option<LedgerRow>, ControlError> {
        let rows = self
            .query(
                &format!("SELECT {LEDGER_COLUMNS} FROM send_ledger WHERE outbox_id = ?"),
                vec![text(outbox_id)],
            )
            .await?;
        rows.first().map(|r| ledger_from(r)).transpose()
    }

    /// Claims a `queued` row that no send holds. Only the caller that gets `true` may send.
    pub async fn ledger_claim(&self, outbox_id: &str) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE send_ledger SET in_flight = 1 \
                 WHERE outbox_id = ? AND state = 'queued' AND in_flight = 0",
                vec![text(outbox_id)],
            )
            .await?;
        Ok(n == 1)
    }

    /// Records the outcome of a claimed row.
    pub async fn ledger_settle(
        &self,
        outbox_id: &str,
        s: &Settlement<'_>,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE send_ledger SET state = ?, channel = ?, in_flight = ?, attempts = ?, \
             attempt_log = ?, error_code = ?, provider_message_id = ?, sent_at = ? \
             WHERE outbox_id = ?",
            vec![
                text(s.state),
                text(s.channel),
                SqlValue::Integer(i64::from(s.in_flight)),
                SqlValue::Integer(i64::from(s.attempts)),
                text(s.attempt_log),
                s.error_code.map_or(SqlValue::Null, text),
                s.provider_message_id.map_or(SqlValue::Null, text),
                s.sent_at.map_or(SqlValue::Null, SqlValue::Integer),
                text(outbox_id),
            ],
        )
        .await?;
        Ok(())
    }
}

impl ControlDb {
    /// The template of `kind` at exactly `version`, approved or not.
    pub async fn template_version(
        &self,
        kind: &str,
        version: u32,
    ) -> Result<Option<MessageTemplate>, ControlError> {
        let rows = self
            .query(
                &format!(
                    "SELECT {TEMPLATE_COLUMNS} FROM message_templates WHERE kind = ? AND version = ?"
                ),
                vec![text(kind), SqlValue::Integer(i64::from(version))],
            )
            .await?;
        rows.first().map(|r| template_from(r)).transpose()
    }
}

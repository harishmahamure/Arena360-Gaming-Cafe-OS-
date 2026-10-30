//! `POST /v1/notify` (Story 8.28): a paid Hub with a device token asks the cloud to send one
//! player message from Arena 360's own account, once per `outbox_id`.
//!
//! The `send_ledger` row is the guard. It is inserted and then claimed before anything leaves, so
//! a repeat post, a lost reply or two Hubs racing never send twice: a repeat answers the stored
//! result. WhatsApp goes first; a non-retryable WhatsApp rejection is followed by the same
//! message as an SMS in the same call, and the ledger keeps both attempts. A failure that proves
//! nothing was sent leaves the row `queued` so the Hub can post it again; an unknown outcome
//! (a timeout after the request went out) leaves it `queued` and claimed, and it is not sent
//! again. Logs and traces carry the outbox id and the branch id, never a number or a text.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, MESSAGE_KINDS, NOTIFY_PARAM_MAX_CHARS, NotifyChannel, NotifyRequest,
    NotifyResponse, NotifyState, Tier, valid_e164,
};
use arena_store::control::{LedgerRow, MessageTemplate, NewLedgerRow, Settlement};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use crate::auth::AuthState;
use crate::messaging::{Channel, Outgoing, SendFailure};

const OUTBOX_ID_MAX: usize = 64;
const PURPOSE_MAX: usize = 40;
const CONSENT_VERSION_MAX: usize = 40;

fn token_ok(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

/// Checks the shape of `body`, and arranges its parameters in the template's order.
fn check(body: &NotifyRequest, template: &MessageTemplate) -> Result<Vec<String>, ApiError> {
    if !token_ok(&body.outbox_id, OUTBOX_ID_MAX) {
        return Err(ApiError::validation(
            "outbox_id",
            "Send the outbox row's id.",
        ));
    }
    if !token_ok(&body.purpose, PURPOSE_MAX) {
        return Err(ApiError::validation(
            "purpose",
            "Send the message's purpose.",
        ));
    }
    if !token_ok(&body.consent_policy_version, CONSENT_VERSION_MAX) {
        return Err(ApiError::validation(
            "consent_policy_version",
            "Send the consent policy version the player agreed to.",
        ));
    }
    if !valid_e164(&body.recipient) {
        return Err(ApiError::validation(
            "recipient",
            "Send a phone number like +919876543210.",
        ));
    }
    if let Some(extra) = body
        .params
        .keys()
        .find(|k| !template.param_names.contains(k))
    {
        return Err(ApiError::validation(
            "params",
            format!("This message has no parameter {extra}."),
        ));
    }
    template
        .param_names
        .iter()
        .map(|name| {
            let v = body.params.get(name).ok_or_else(|| {
                ApiError::validation("params", format!("Send the parameter {name}."))
            })?;
            if v.is_empty()
                || v.chars().count() > NOTIFY_PARAM_MAX_CHARS
                || v.chars().any(char::is_control)
            {
                return Err(ApiError::validation(
                    "params",
                    format!("The parameter {name} must be 1 to {NOTIFY_PARAM_MAX_CHARS} characters of plain text."),
                ));
            }
            Ok(v.clone())
        })
        .collect()
}

/// `text` with each `{name}` replaced by its value.
fn render(text: &str, names: &[String], values: &[String]) -> String {
    let mut out = text.to_owned();
    for (n, v) in names.iter().zip(values) {
        out = out.replace(&format!("{{{n}}}"), v);
    }
    out
}

fn channel_of(s: &str) -> NotifyChannel {
    if s == "sms" {
        NotifyChannel::Sms
    } else {
        NotifyChannel::Whatsapp
    }
}

fn state_of(s: &str) -> NotifyState {
    match s {
        "sent" => NotifyState::Sent,
        "failed" => NotifyState::Failed,
        _ => NotifyState::Queued,
    }
}

fn view(row: &LedgerRow) -> NotifyResponse {
    NotifyResponse {
        outbox_id: row.outbox_id.clone(),
        state: state_of(&row.state),
        channel: channel_of(&row.channel),
        attempts: row.attempts,
        provider_message_id: row.provider_message_id.clone(),
        error_code: row.error_code.clone(),
    }
}

async fn notify(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<NotifyRequest>,
) -> Result<Json<NotifyResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    let branch = st
        .db
        .branch(&hub.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("hub without its branch"))?;
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let claims =
        crate::licence::claims_for(&branch.tenant_id, &branch.id, entitlement.as_ref(), now);
    if claims.tier == Tier::Free {
        return Err(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "Sending messages needs a paid plan.",
        )
        .into());
    }
    if !MESSAGE_KINDS.contains(&body.kind.as_str()) {
        return Err(ApiError::validation(
            "kind",
            "This kind of message isn't supported.",
        ));
    }
    let Some(notifier) = st.notifier.get() else {
        return Err(ApiError::unavailable("no messaging adapter configured"));
    };
    // A repeat gets the stored result, whatever it says; one still to send uses the template
    // version it was created with, even if a newer one exists now.
    let existing = st
        .db
        .ledger_by_outbox(&body.outbox_id)
        .await
        .map_err(ApiError::internal)?;
    let (template, mut row) = match existing {
        Some(row) if row.branch_id != branch.id => {
            return Err(AppError::new(
                ErrorCode::IdempotencyConflict,
                "That outbox id belongs to another branch.",
            )
            .into());
        }
        Some(row) if row.state != "queued" || row.in_flight => {
            return Ok(Json(view(&row)));
        }
        Some(row) => {
            let template = st
                .db
                .template_version(&row.kind, row.template_version)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("ledger row without its template"))?;
            (template, row)
        }
        None => {
            let template = st
                .db
                .current_template(&body.kind)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("no approved template for a known kind"))?;
            let id = ulid::Ulid::generate().to_string();
            let hash = st.recipient_hash(&body.recipient);
            check(&body, &template)?;
            let inserted = st
                .db
                .ledger_insert(&NewLedgerRow {
                    id: &id,
                    tenant_id: &branch.tenant_id,
                    branch_id: &branch.id,
                    outbox_id: &body.outbox_id,
                    kind: &body.kind,
                    purpose: &body.purpose,
                    consent_policy_version: &body.consent_policy_version,
                    recipient_hash: &hash,
                    template_version: template.version,
                    now,
                })
                .await
                .map_err(ApiError::internal)?;
            tracing::info!(outbox_id = %body.outbox_id, branch_id = %branch.id, inserted, "notify: ledger row");
            let row = st
                .db
                .ledger_by_outbox(&body.outbox_id)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("ledger row missing after insert"))?;
            if row.state != "queued" || row.in_flight {
                return Ok(Json(view(&row)));
            }
            (template, row)
        }
    };
    let params = check(&body, &template)?;
    if !st
        .db
        .ledger_claim(&body.outbox_id)
        .await
        .map_err(ApiError::internal)?
    {
        // Another post claimed it first.
        let row = st
            .db
            .ledger_by_outbox(&body.outbox_id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::internal("ledger row vanished"))?;
        return Ok(Json(view(&row)));
    }

    let sms_text = render(&template.sms_text, &template.param_names, &params);
    let outgoing = Outgoing {
        outbox_id: &row.outbox_id,
        to: &body.recipient,
        whatsapp_template_id: &template.whatsapp_template_id,
        dlt_template_id: &template.sms_dlt_template_id,
        params: &params,
        sms_text: &sms_text,
    };
    let mut channel = if row.channel == "sms" {
        Channel::Sms
    } else {
        Channel::Whatsapp
    };
    let mut log: Vec<serde_json::Value> =
        serde_json::from_str(&row.attempt_log).unwrap_or_default();
    loop {
        row.attempts += 1;
        let result = notifier.send(channel, &outgoing).await;
        let outcome = match &result {
            Ok(_) => "sent",
            Err(SendFailure::Rejected { .. }) => "rejected",
            Err(SendFailure::Transient { .. }) => "transient",
            Err(SendFailure::Uncertain { .. }) => "uncertain",
        };
        log.push(json!({
            "channel": channel.as_str(),
            "at": now,
            "outcome": outcome,
            "code": result.as_ref().err().map(SendFailure::code),
        }));
        let attempt_log = serde_json::Value::Array(log.clone()).to_string();
        let settle = |state, channel: Channel, in_flight, error_code, message_id| Settlement {
            state,
            channel: channel.as_str(),
            in_flight,
            attempts: row.attempts,
            attempt_log: &attempt_log,
            error_code,
            provider_message_id: message_id,
            sent_at: (state == "sent").then_some(now),
        };
        let settlement = match &result {
            Ok(id) => settle("sent", channel, false, None, Some(id.as_str())),
            Err(SendFailure::Rejected { .. }) if channel == Channel::Whatsapp => {
                channel = Channel::Sms;
                continue;
            }
            Err(SendFailure::Rejected { code }) => {
                settle("failed", channel, false, Some(code.as_str()), None)
            }
            Err(SendFailure::Transient { code }) => {
                settle("queued", channel, false, Some(code.as_str()), None)
            }
            Err(SendFailure::Uncertain { code }) => {
                settle("queued", channel, true, Some(code.as_str()), None)
            }
        };
        st.db
            .ledger_settle(&row.outbox_id, &settlement)
            .await
            .map_err(ApiError::internal)?;
        tracing::info!(outbox_id = %row.outbox_id, branch_id = %branch.id, state = settlement.state, channel = channel.as_str(), attempts = row.attempts, "notify: settled");
        return Ok(Json(NotifyResponse {
            outbox_id: row.outbox_id.clone(),
            state: state_of(settlement.state),
            channel: channel_of(settlement.channel),
            attempts: row.attempts,
            provider_message_id: settlement.provider_message_id.map(str::to_owned),
            error_code: settlement.error_code.map(str::to_owned),
        }));
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new().route("/notify", post(notify)).with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_fills_every_placeholder() {
        let names = vec!["name".to_owned(), "cafe".to_owned()];
        let values = vec!["Asha".to_owned(), "Arena".to_owned()];
        assert_eq!(
            render("Hi {name} at {cafe}, {name}", &names, &values),
            "Hi Asha at Arena, Asha"
        );
    }

    #[test]
    fn outbox_ids_are_plain_tokens() {
        assert!(token_ok("01J9ZXAMPLE", 64));
        assert!(!token_ok("", 64));
        assert!(!token_ok("a b", 64));
        assert!(!token_ok("a/../b", 64));
    }
}

//! Raising an owner alert (Story 8.33): the alert is stored, pushed to each of the owner's
//! browsers and sent on WhatsApp, each once per alert id. Stories 8.34 and 8.35 call [`raise`]
//! from the Hub's thresholds and the offline detector; this story's source is the dev fixture.
//!
//! "Once" is the durable guard, claimed before anything is sent: `owner_alerts` is insert-if-
//! absent by id, a push claims `(alert_id, subscription_id)` in `owner_alert_deliveries`, and the
//! WhatsApp message is the send-ledger row `<alert id>.wa`. A repeat therefore sends only what is
//! not yet claimed, and a failed push is logged and never retried. Logs carry alert and
//! subscription ids, never an endpoint, a key, a phone or the text.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, ErrorCode, NotifyRequest, OwnerAlertKind, OwnerAlertPush, OwnerAlertView, Tier,
};
use arena_store::control::{Branch, NewOwnerAlert, OWNER_ALERT_RETENTION_MS, OwnerAlertRow};

use crate::api::ApiError;
use crate::api::notify::deliver;
use crate::auth::AuthState;
use crate::push::{PushOutcome, PushTarget};

/// The message kind and purpose of an owner's WhatsApp alert; its consent version is `owner`.
const WA_KIND: &str = "owner_alert";
const WA_CONSENT: &str = "owner";
/// The longest alert id (the ledger id `<id>.wa` must stay a 64-character token).
const ID_MAX: usize = 60;
/// The longest push title, in characters.
const TITLE_MAX: usize = 80;

/// An alert to raise.
#[derive(Debug, Clone)]
pub struct Raise<'a> {
    /// The alert's id; a repeat with the same id sends nothing twice. A ULID when `None`.
    pub id: Option<&'a str>,
    pub branch: &'a Branch,
    pub kind: OwnerAlertKind,
    pub params: &'a BTreeMap<String, String>,
    /// Also send the alert on WhatsApp (when the owner has not turned that off). Push and the
    /// list always get it.
    pub whatsapp: bool,
}

/// What a raise did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    pub id: String,
    /// The alert row was new.
    pub created: bool,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The list row of a stored alert, or `None` when its kind is not one this build knows.
pub fn view_of(row: &OwnerAlertRow) -> Option<OwnerAlertView> {
    let kind = OwnerAlertKind::parse(&row.kind)?;
    let params: BTreeMap<String, String> = serde_json::from_str(&row.params).ok()?;
    Some(OwnerAlertView {
        id: row.id.clone(),
        branch_id: row.branch_id.clone(),
        branch_name: row.branch_name.clone(),
        kind,
        text: kind.text(&params),
        url: kind.url(&row.branch_id),
        created_at: row.created_at,
    })
}

/// Raises an alert for `r.branch`: stores it, pushes it to the owner's browsers and sends it on
/// WhatsApp when the owner has not turned that off. A channel that fails does not fail the raise.
pub async fn raise(st: &AuthState, r: Raise<'_>) -> Result<Raised, ApiError> {
    r.kind
        .check_params(r.params)
        .map_err(|e| ApiError::validation(&e.field, e.message))?;
    let now = st.now();
    let tier = st
        .db
        .entitlement(&r.branch.id)
        .await
        .map_err(ApiError::internal)?
        .map_or(Tier::Free, |e| e.tier_at(now));
    // A refund leaves the branch on Free, and the owner is told all the same.
    if tier == Tier::Free && r.kind != OwnerAlertKind::RefundProcessed {
        return Err(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "Owner alerts need a paid plan.",
        )
        .into());
    }
    let id = match r.id {
        Some(id) if valid_id(id) => id.to_owned(),
        Some(_) => {
            return Err(ApiError::validation(
                "id",
                "Send an id of letters and digits.",
            ));
        }
        None => ulid::Ulid::generate().to_string(),
    };
    let params = serde_json::to_string(r.params).map_err(ApiError::internal)?;

    st.db
        .owner_alerts_prune(now - OWNER_ALERT_RETENTION_MS)
        .await
        .map_err(ApiError::internal)?;
    let created = st
        .db
        .owner_alert_insert(&NewOwnerAlert {
            id: &id,
            tenant_id: &r.branch.tenant_id,
            branch_id: &r.branch.id,
            kind: r.kind.as_str(),
            params: &params,
            now,
        })
        .await
        .map_err(ApiError::internal)?;
    // What goes out is what is stored, so a repeat says the same thing.
    let row = st
        .db
        .owner_alert(&id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("alert missing after insert"))?;
    if row.tenant_id != r.branch.tenant_id || row.branch_id != r.branch.id {
        return Err(AppError::new(
            ErrorCode::IdempotencyConflict,
            "That alert id belongs to another branch.",
        )
        .into());
    }
    let view = view_of(&row).ok_or_else(|| ApiError::internal("stored alert of unknown kind"))?;
    tracing::info!(alert_id = %id, branch_id = %row.branch_id, kind = %row.kind, created, "owner alert raised");

    let owner = st
        .db
        .owner_of_branch(&row.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("branch without an owner"))?;

    if let Err(e) = push_all(st, &owner.id, &view, now).await {
        tracing::error!(alert_id = %id, error = %e, "owner alert: push failed");
    }
    match st
        .db
        .owner_alert_whatsapp(&owner.id)
        .await
        .map(|on| on && r.whatsapp)
    {
        Ok(true) => {
            let request = NotifyRequest {
                outbox_id: format!("{id}.wa"),
                kind: WA_KIND.to_owned(),
                purpose: WA_KIND.to_owned(),
                consent_policy_version: WA_CONSENT.to_owned(),
                recipient: owner.phone_e164.clone(),
                params: BTreeMap::from([
                    ("branch".to_owned(), view.branch_name.clone()),
                    ("text".to_owned(), view.text.clone()),
                ]),
            };
            match deliver(st, r.branch, &request).await {
                Ok(sent) => {
                    tracing::info!(alert_id = %id, state = ?sent.state, "owner alert: whatsapp");
                }
                Err(e) => {
                    tracing::error!(alert_id = %id, code = ?e.body().code, "owner alert: whatsapp not sent");
                }
            }
        }
        Ok(false) => {}
        Err(e) => tracing::error!(alert_id = %id, error = %e, "owner alert: settings unreadable"),
    }
    Ok(Raised { id, created })
}

/// Pushes `view` to each of the owner's subscriptions that has not been claimed for it.
async fn push_all(
    st: &AuthState,
    owner_id: &str,
    view: &OwnerAlertView,
    now: i64,
) -> Result<(), arena_store::control::ControlError> {
    let Some(push) = st.push.get() else {
        return Ok(());
    };
    let payload = serde_json::to_vec(&OwnerAlertPush {
        // A long branch name must not push the payload past one record.
        title: view.branch_name.chars().take(TITLE_MAX).collect(),
        body: view.text.clone(),
        url: view.url.clone(),
        tag: view.id.clone(),
    })
    .unwrap_or_default();
    let mut claimed = Vec::new();
    for sub in st.db.push_subscriptions(owner_id).await? {
        // A claim that fails leaves that one unsent; the others still go out.
        match st.db.owner_alert_claim(&view.id, &sub.id).await {
            Ok(true) => claimed.push(sub),
            Ok(false) => {}
            Err(e) => {
                tracing::error!(alert_id = %view.id, subscription_id = %sub.id, error = %e, "owner alert: push claim failed");
            }
        }
    }
    let sends = claimed.iter().map(|sub| {
        let payload = &payload;
        async move {
            let outcome = push
                .send(
                    &PushTarget {
                        endpoint: &sub.endpoint,
                        p256dh: &sub.p256dh,
                        auth: &sub.auth,
                    },
                    payload,
                )
                .await;
            (sub.id.as_str(), outcome)
        }
    });
    for (sub_id, outcome) in futures_util::future::join_all(sends).await {
        match outcome {
            PushOutcome::Delivered => {
                if let Err(e) = st.db.push_subscription_succeeded(sub_id, now).await {
                    tracing::error!(alert_id = %view.id, subscription_id = %sub_id, error = %e, "owner alert: push outcome not recorded");
                }
            }
            PushOutcome::Gone => {
                tracing::info!(alert_id = %view.id, subscription_id = %sub_id, "owner alert: subscription gone");
                if let Err(e) = st.db.push_subscription_delete(sub_id).await {
                    tracing::error!(alert_id = %view.id, subscription_id = %sub_id, error = %e, "owner alert: gone subscription not deleted");
                }
            }
            PushOutcome::Failed(code) => {
                tracing::warn!(alert_id = %view.id, subscription_id = %sub_id, code = %code, "owner alert: push failed, not retried");
            }
        }
    }
    Ok(())
}

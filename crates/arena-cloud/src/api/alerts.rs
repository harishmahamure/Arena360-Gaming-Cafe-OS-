//! The owner's alert routes (Story 8.33), for a signed-in owner whose account setup is complete:
//! `GET /v1/alerts` (the café's alerts, newest first), `GET|PATCH /v1/alerts/settings` (the
//! WhatsApp switch and what push needs), `POST /v1/alerts/push` (subscribe a browser) and
//! `POST /v1/alerts/push/remove`. Without a VAPID key the two push routes answer 503. An endpoint,
//! a key or an alert's text is never logged.

use std::sync::Arc;

use arena_proto::{
    OWNER_ALERTS_PAGE, OWNER_ALERTS_PAGE_MAX, OwnerAlertList, OwnerAlertSettings,
    OwnerAlertSettingsPatch, PUSH_SUBSCRIPTIONS_MAX, PushRemove, PushSubscribe,
};
use arena_store::control::{OWNER_ALERT_RETENTION_MS, UpsertSubscription, UpsertedSubscription};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use crate::auth::AuthState;
use crate::owner_alerts::view_of;

/// The longest push endpoint stored.
const ENDPOINT_MAX: usize = 2048;
/// The longest `User-Agent` stored.
const USER_AGENT_MAX: usize = 256;

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    before: Option<String>,
    limit: Option<usize>,
}

/// `before` is `<created_at>.<id>`, as `next_before` gave it.
fn parse_before(s: &str) -> Option<(i64, &str)> {
    let (at, id) = s.split_once('.')?;
    let at = at.parse::<i64>().ok().filter(|n| *n >= 0)?;
    (!id.is_empty()).then_some((at, id))
}

async fn list(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<OwnerAlertList>, ApiError> {
    let Query(q) = query.map_err(|_| ApiError::validation("limit", "Send a valid limit."))?;
    let before = match q.before.as_deref().filter(|b| !b.is_empty()) {
        None => None,
        Some(b) => Some(parse_before(b).ok_or_else(|| {
            ApiError::validation("before", "Send the next_before the last page gave.")
        })?),
    };
    let tenant = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let limit = q
        .limit
        .unwrap_or(OWNER_ALERTS_PAGE)
        .clamp(1, OWNER_ALERTS_PAGE_MAX);
    let since = st.now() - OWNER_ALERT_RETENTION_MS;
    let mut rows = st
        .db
        .owner_alerts(&tenant.id, since, before, limit + 1)
        .await
        .map_err(ApiError::internal)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next_before = rows
        .last()
        .filter(|_| more)
        .map(|r| format!("{}.{}", r.created_at, r.id));
    Ok(Json(OwnerAlertList {
        alerts: rows.iter().filter_map(view_of).collect(),
        next_before,
    }))
}

async fn settings_of(st: &AuthState, owner_id: &str) -> Result<OwnerAlertSettings, ApiError> {
    Ok(OwnerAlertSettings {
        vapid_public_key: st.push.get().map(|p| p.public_key().to_owned()),
        whatsapp: st
            .db
            .owner_alert_whatsapp(owner_id)
            .await
            .map_err(ApiError::internal)?,
        subscriptions: st
            .db
            .push_subscription_count(owner_id)
            .await
            .map_err(ApiError::internal)?,
    })
}

async fn get_settings(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
) -> Result<Json<OwnerAlertSettings>, ApiError> {
    Ok(Json(settings_of(&st, &signed.owner.id).await?))
}

async fn patch_settings(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<OwnerAlertSettingsPatch>,
) -> Result<Json<OwnerAlertSettings>, ApiError> {
    st.db
        .set_owner_alert_whatsapp(&signed.owner.id, body.whatsapp)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(settings_of(&st, &signed.owner.id).await?))
}

fn push_unavailable(st: &AuthState) -> Result<(), ApiError> {
    if st.push.get().is_none() {
        return Err(ApiError::unavailable("no VAPID key configured"));
    }
    Ok(())
}

/// An `https` endpoint with a named host: not a literal address and not `localhost`, so a push
/// post never goes to the cloud's own network.
fn endpoint_ok(endpoint: &str) -> bool {
    if endpoint.len() > ENDPOINT_MAX || endpoint.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .domain()
            .is_some_and(|d| d != "localhost" && !d.ends_with(".localhost"))
}

fn key_ok(value: &str, len: usize) -> bool {
    URL_SAFE_NO_PAD.decode(value).is_ok_and(|b| b.len() == len)
}

async fn subscribe(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PushSubscribe>,
) -> Result<StatusCode, ApiError> {
    push_unavailable(&st)?;
    if !endpoint_ok(&body.endpoint) {
        return Err(ApiError::validation(
            "endpoint",
            "Send the https endpoint of the browser's push subscription.",
        ));
    }
    // An uncompressed P-256 point, and a 16-byte auth secret.
    if !key_ok(&body.keys.p256dh, 65) {
        return Err(ApiError::validation(
            "keys.p256dh",
            "Send the subscription's p256dh key.",
        ));
    }
    if !key_ok(&body.keys.auth, 16) {
        return Err(ApiError::validation(
            "keys.auth",
            "Send the subscription's auth key.",
        ));
    }
    let user_agent: String = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(USER_AGENT_MAX)
        .collect();
    let id = ulid::Ulid::generate().to_string();
    let saved = st
        .db
        .push_subscription_upsert(&UpsertSubscription {
            id: &id,
            owner_user_id: &signed.owner.id,
            endpoint: &body.endpoint,
            p256dh: &body.keys.p256dh,
            auth: &body.keys.auth,
            user_agent: &user_agent,
            now: st.now(),
            max_per_owner: PUSH_SUBSCRIPTIONS_MAX,
        })
        .await
        .map_err(ApiError::internal)?;
    match saved {
        UpsertedSubscription::Saved => Ok(StatusCode::NO_CONTENT),
        UpsertedSubscription::TooMany => Err(ApiError::validation(
            "endpoint",
            format!("You can turn alerts on in at most {PUSH_SUBSCRIPTIONS_MAX} browsers."),
        )),
    }
}

async fn unsubscribe(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<PushRemove>,
) -> Result<StatusCode, ApiError> {
    push_unavailable(&st)?;
    st.db
        .push_subscription_remove(&signed.owner.id, &body.endpoint)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/alerts", get(list))
        .route("/alerts/settings", get(get_settings).patch(patch_settings))
        .route("/alerts/push", post(subscribe))
        .route("/alerts/push/remove", post(unsubscribe))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_named_https_hosts_are_endpoints() {
        assert!(endpoint_ok("https://fcm.googleapis.com/fcm/send/abc"));
        assert!(endpoint_ok(
            "https://updates.push.services.mozilla.com:443/wpush/v2/x"
        ));
        for bad in [
            "http://fcm.googleapis.com/x",
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://[::1]/x",
            "https://localhost/x",
            "https://user:pw@push.example/x",
            "ftp://push.example/x",
            "not a url",
            "",
        ] {
            assert!(!endpoint_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_page_cursor_is_a_time_and_an_id() {
        assert_eq!(
            parse_before("1800000000000.01J9X"),
            Some((1_800_000_000_000, "01J9X"))
        );
        assert_eq!(parse_before("abc.01J9X"), None);
        assert_eq!(parse_before("123."), None);
        assert_eq!(parse_before("123"), None);
        assert_eq!(parse_before("-5.x"), None);
    }
}

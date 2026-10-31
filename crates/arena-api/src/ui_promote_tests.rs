//! Story 9.21: `POST /ui/standby/promote` over the loopback API.

use std::sync::{Arc, Mutex};

use arena_proto::{AppError, ErrorCode, LEASE_FRESH_MESSAGE, Promoted};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::PromoteHook;
use crate::ui_auth::tests::{Hub, call, hub, token_for};

const PROMOTE: &str = "/ui/standby/promote";

/// A hook that records who asked and answers `result`.
fn hook(result: Result<Promoted, AppError>, asked: &Arc<Mutex<Vec<String>>>) -> PromoteHook {
    let asked = Arc::clone(asked);
    Arc::new(move |manager| {
        asked.lock().unwrap().push(manager);
        let result = result.clone();
        Box::pin(async move { result })
    })
}

fn done() -> Promoted {
    Promoted {
        epoch: "4:STBY".into(),
        provisional: true,
        kitchen_displays_to_repair: 2,
    }
}

async fn post(hub: &Hub, token: &str, body: Value) -> (StatusCode, Value) {
    call(hub, "POST", PROMOTE, Some(token), Some(body)).await
}

#[tokio::test]
async fn a_hub_that_is_not_a_standby_has_nothing_to_promote() {
    let hub = hub().await;
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, body) = post(&hub, &token, json!({"pin": "2222"})).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("UNSUPPORTED"))
    );
}

#[tokio::test]
async fn a_manager_promotes_with_their_own_pin() {
    let hub = hub().await;
    let asked = Arc::new(Mutex::new(Vec::new()));
    hub.auth.set_promote(hook(Ok(done()), &asked));
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, body) = post(&hub, &token, json!({"pin": "2222"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"epoch": "4:STBY", "provisional": true, "kitchen_displays_to_repair": 2})
    );
    assert_eq!(*asked.lock().unwrap(), vec![hub.manager_id.clone()]);
}

#[tokio::test]
async fn a_counter_promotes_with_a_managers_pin_and_the_manager_is_the_one_recorded() {
    let hub = hub().await;
    let asked = Arc::new(Mutex::new(Vec::new()));
    hub.auth.set_promote(hook(Ok(done()), &asked));
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = post(&hub, &token, json!({"pin": "2222"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(*asked.lock().unwrap(), vec![hub.manager_id.clone()]);
}

#[tokio::test]
async fn without_a_pin_a_wrong_pin_or_their_own_pin_a_counter_cannot_promote() {
    let hub = hub().await;
    let asked = Arc::new(Mutex::new(Vec::new()));
    hub.auth.set_promote(hook(Ok(done()), &asked));
    let token = token_for(&hub, "counter", "c-pass").await;
    let (status, body) = post(&hub, &token, json!({})).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let (status, body) = post(&hub, &token, json!({"pin": "3333"})).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
    );
    let (status, body) = post(&hub, &token, json!({"pin": "9999"})).await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(asked.lock().unwrap().is_empty(), "nothing started");
}

#[tokio::test]
async fn what_the_steps_refuse_comes_back_as_it_is() {
    let hub = hub().await;
    let asked = Arc::new(Mutex::new(Vec::new()));
    hub.auth.set_promote(hook(
        Err(AppError::new(ErrorCode::LeaseFresh, LEASE_FRESH_MESSAGE)),
        &asked,
    ));
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, body) = post(&hub, &token, json!({"pin": "2222"})).await;
    assert_eq!(
        (status, body["code"].as_str(), body["message"].as_str()),
        (
            StatusCode::CONFLICT,
            Some("LEASE_FRESH"),
            Some(LEASE_FRESH_MESSAGE)
        )
    );
}

#[tokio::test]
async fn the_route_needs_a_session_and_refuses_unknown_fields() {
    let hub = hub().await;
    let (status, _) = call(&hub, "POST", PROMOTE, None, Some(json!({"pin": "2222"}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let token = token_for(&hub, "manager", "m-pass").await;
    let (status, _) = post(&hub, &token, json!({"pin": "2222", "force": true})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

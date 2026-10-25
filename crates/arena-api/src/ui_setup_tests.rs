use arena_core::staff::hash_secret;
use arena_core::{Clock, CloudTime};
use arena_store::Store;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::ui_auth::tests::{Hub, call, hub};

const CODE: &str = "ABCD2345";
const PASSWORD: &str = "a-long-new-password";
const PIN: &str = "4821";
const DAY: i64 = 24 * 3_600_000;

fn pend(hub: &Hub, login: &str, expires_in_ms: i64) {
    let hash = hash_secret(CODE).unwrap();
    let at = hub.clock.now_ms() + expires_in_ms;
    Store::open(&hub.path, 0)
        .unwrap()
        .write_tx(|tx| {
            tx.execute_batch(&format!(
                "UPDATE staff SET password_hash = NULL, pin_hash = NULL,
                                  setup_code_hash = '{hash}', setup_expires_at = {at}
                 WHERE login = '{login}'"
            ))
            .map_err(arena_store::db_error)?;
            Ok(())
        })
        .unwrap();
}

fn body(login: &str, code: &str, password: &str, pin: &str) -> Value {
    json!({"login": login, "code": code, "password": password, "pin": pin})
}

async fn submit(hub: &Hub, login: &str, code: &str) -> (StatusCode, Value) {
    call(
        hub,
        "POST",
        "/ui/auth/setup-code",
        None,
        Some(body(login, code, PASSWORD, PIN)),
    )
    .await
}

#[tokio::test]
async fn a_valid_code_sets_the_credentials_signs_in_and_works_once() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    let (status, res) = submit(&hub, "Manager", "abcd 2345").await;
    assert_eq!(status, StatusCode::OK, "{res}");
    let token = res["token"].as_str().unwrap();
    assert_eq!(res["staff"]["role_name"], "Manager");
    let (status, me) = call(&hub, "GET", "/ui/me", Some(token), None).await;
    assert_eq!(
        (status, me["has_pin"].as_bool()),
        (StatusCode::OK, Some(true))
    );
    let (status, _) = call(
        &hub,
        "POST",
        "/ui/auth/sign-in",
        None,
        Some(json!({"login": "manager", "password": PASSWORD})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, res) = submit(&hub, "manager", CODE).await;
    assert_eq!(
        (status, res["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("SETUP_CODE_INVALID"))
    );
    let row = Store::open(&hub.path, 0)
        .unwrap()
        .staff_with_role("manager")
        .unwrap()
        .unwrap()
        .0;
    assert_eq!((row.setup_code_hash, row.setup_expires_at), (None, None));
    assert!(row.pin_verifier_version > 1);
}

#[tokio::test]
async fn every_refusal_is_the_same_line() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    pend(&hub, "counter", -1);
    let wrong = submit(&hub, "manager", "ZZZZ2345").await;
    let expired = submit(&hub, "counter", CODE).await;
    let unknown = submit(&hub, "nobody", CODE).await;
    let malformed = submit(&hub, "manager", "short").await;
    let has_credentials = submit(&hub, "kitchen", CODE).await;
    for r in [&expired, &unknown, &malformed, &has_credentials] {
        assert_eq!(r, &wrong);
    }
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.1["code"], "SETUP_CODE_INVALID");
    assert_eq!(wrong.1["message"], "that setup code can't be used");
}

#[tokio::test]
async fn five_failures_pause_the_login_for_sixty_seconds() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    for _ in 0..5 {
        assert_eq!(
            submit(&hub, "manager", "ZZZZ2345").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, res) = submit(&hub, "Manager", CODE).await;
    assert_eq!(
        (status, res["code"].as_str(), res["retry_after_s"].as_u64()),
        (StatusCode::TOO_MANY_REQUESTS, Some("AUTH_LOCKED"), Some(60))
    );
    assert_eq!(
        submit(&hub, "counter", "ZZZZ2345").await.0,
        StatusCode::UNAUTHORIZED
    );
    hub.clock.advance(59_000);
    assert_eq!(
        submit(&hub, "manager", CODE).await.1["retry_after_s"].as_u64(),
        Some(1)
    );
    hub.clock.advance(1_000);
    assert_eq!(submit(&hub, "manager", CODE).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_weak_password_or_pin_does_not_spend_the_code() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    let send = |password: &'static str, pin: &'static str| {
        let hub = &hub;
        async move {
            call(
                hub,
                "POST",
                "/ui/auth/setup-code",
                None,
                Some(body("manager", CODE, password, pin)),
            )
            .await
        }
    };
    let (status, res) = send("short", PIN).await;
    assert_eq!(
        (status, res["code"].as_str(), res["field"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("PASSWORD_TOO_SHORT"),
            Some("password")
        )
    );
    for pin in ["123", "1234567", "12a4"] {
        let (status, res) = send(PASSWORD, pin).await;
        assert_eq!(
            (status, res["field"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some("pin")),
            "{pin}"
        );
    }
    let long = "x".repeat(129);
    let (status, _) = call(
        &hub,
        "POST",
        "/ui/auth/setup-code",
        None,
        Some(body("manager", CODE, &long, PIN)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for _ in 0..6 {
        let (status, _) = send("short", PIN).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(send(PASSWORD, PIN).await.0, StatusCode::OK);
}

#[tokio::test]
async fn expiry_is_judged_on_trusted_time_not_the_wall_clock() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    let wall = hub.clock.now_ms();
    hub._status.send_modify(|s| {
        s.cloud_time = Some(CloudTime {
            cloud_ms: wall + 2 * DAY,
            at: std::time::Instant::now(),
        })
    });
    hub.clock.advance(-1000);
    assert_eq!(
        submit(&hub, "manager", CODE).await.0,
        StatusCode::UNAUTHORIZED
    );
    hub._status.send_modify(|s| {
        s.cloud_time = Some(CloudTime {
            cloud_ms: wall,
            at: std::time::Instant::now(),
        })
    });
    hub.clock.advance(5 * DAY);
    assert_eq!(submit(&hub, "manager", CODE).await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_ui_intent_route_refuses_the_kind_and_no_secret_reaches_a_change_row() {
    let hub = hub().await;
    pend(&hub, "manager", DAY);
    let (status, res) = submit(&hub, "manager", CODE).await;
    assert_eq!(status, StatusCode::OK);
    let token = res["token"].as_str().unwrap().to_owned();
    let intent = crate::ui_auth::tests::intent_as(
        arena_proto::Actor {
            kind: arena_proto::ActorKind::Staff,
            id: "x".into(),
        },
        arena_proto::IntentPayload::StaffSetCredentials(arena_proto::StaffSetCredentials {
            staff_id: hub.manager_id.clone(),
            setup_code_hash: "h".into(),
            trusted_now_ms: 0,
            password_hash: "p".into(),
            pin_hash: "q".into(),
        }),
        None,
    );
    let (status, res) = call(
        &hub,
        "POST",
        "/ui/intent",
        Some(&token),
        Some(json!({"intent": intent})),
    )
    .await;
    assert_eq!(
        (status, res["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("UNSUPPORTED"))
    );
    let changes = Store::open(&hub.path, 0).unwrap().changes().unwrap();
    let change = changes
        .iter()
        .find(|c| c.intent_kind == "staff.set_credentials")
        .unwrap();
    assert_eq!(
        (change.actor_kind.as_str(), change.actor_id.as_str()),
        ("staff", hub.manager_id.as_str())
    );
    for secret in [PASSWORD, PIN, CODE] {
        assert!(
            !change
                .row
                .windows(secret.len())
                .any(|w| w == secret.as_bytes()),
            "{secret}"
        );
    }
}

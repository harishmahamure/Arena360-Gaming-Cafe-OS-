use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, Permission};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, ui_error};

fn denied() -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "you don't have permission to do this",
    )
}

async fn manager_session(auth: &UiAuth, headers: &HeaderMap) -> Result<String, Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    if !me.permissions.contains(&Permission::DeviceManage) {
        return Err(fail(ui_error(denied())));
    }
    Ok(token)
}

pub(crate) async fn issue(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let token = match manager_session(&auth, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let Some(pairing) = auth.pairing() else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotFound,
            "kiosk pairing is not available on this hub",
        )));
    };
    let primary = auth.writer().is_some_and(|w| !w.is_closed())
        && auth
            .status()
            .is_none_or(|s| s.borrow().role == arena_core::Role::Primary);
    if !primary {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    match pairing.issue(&token) {
        Ok(code) => Json(code).into_response(),
        Err(e) => fail(ui_error(e)),
    }
}

pub(crate) async fn discard(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.session_for(&token).await {
        return fail(e);
    }
    if let Some(pairing) = auth.pairing() {
        pairing.discard(&token);
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// `http://<hub-lan-ip>:7401/kitchen`, from the first LAN address the Hub knows (Story 7.6).
fn kitchen_address(auth: &UiAuth) -> Option<String> {
    let status = auth.status()?.borrow().clone();
    let addr = status.lan_addresses.first()?;
    let (_, web_port) = status.lan_ports?;
    Some(format!(
        "http://{addr}:{web_port}{}",
        arena_proto::KDS_PAGE_PATH
    ))
}

/// `POST /ui/kds-pairing-code` (Story 7.6): a Manager or Owner (`device.manage`) on a Basic-and-above
/// branch gets a 6-digit single-use code for a kitchen display, good for 10 minutes, and the address
/// the tablet opens. Issuing writes nothing and replaces the branch's previous kitchen code. Free is
/// `TIER_LOCKED`; a follower is `NOT_PRIMARY`.
pub(crate) async fn issue_kds(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let token = match manager_session(&auth, &headers).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Some(pairing) = auth.kds_pairing() else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotFound,
            "kitchen display pairing is not available on this hub",
        )));
    };
    let primary = auth.writer().is_some_and(|w| !w.is_closed())
        && auth
            .status()
            .is_none_or(|s| s.borrow().role == arena_core::Role::Primary);
    if !primary {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    // The same flag test as the `kds.pair` gate.
    let reader = Arc::clone(auth.reader());
    let flags = tokio::task::spawn_blocking(move || reader.settings()).await;
    let has_kitchen = match flags {
        Ok(Ok(settings)) => settings.iter().any(|s| {
            s.key == arena_proto::KEY_LICENCE_FLAGS
                && arena_proto::has_flag(&s.value, arena_proto::FLAG_KITCHEN)
        }),
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => {
            return fail(ui_error(AppError::new(
                ErrorCode::Internal,
                "settings read failed",
            )));
        }
    };
    if !has_kitchen {
        return fail(ui_error(AppError::new(
            ErrorCode::TierLocked,
            "available on Basic and above",
        )));
    }
    match pairing.issue(&me.staff_id) {
        Ok(code) => Json(arena_proto::KdsPairingCode {
            code: code.code,
            expires_at: code.expires_at,
            address: kitchen_address(&auth),
        })
        .into_response(),
        Err(e) => fail(ui_error(e)),
    }
}

/// `POST /ui/kds-pairing-code/discard`: the dialog closed, so its code is dropped.
pub(crate) async fn discard_kds(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.session_for(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if let Some(pairing) = auth.kds_pairing() {
        pairing.discard(&me.staff_id);
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use crate::ui_auth::UiAuth;
    use crate::ui_auth::tests::{call, call_app, hub, token_for};
    use arena_core::Clock;
    use axum::http::StatusCode;
    use std::sync::Arc;

    #[tokio::test]
    async fn a_manager_gets_a_six_digit_code_and_a_counter_is_refused() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, body) = call(&hub, "POST", "/ui/pairing-code", Some(&counter), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "PERMISSION_DENIED");
        assert!(!hub.pairing.has_live_code());
        let (status, body) = call(&hub, "POST", "/ui/pairing-code", None, None).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("AUTH_FAILED"))
        );

        let (status, body) = call(&hub, "POST", "/ui/pairing-code", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let code = body["code"].as_str().unwrap().to_owned();
        assert!(arena_proto::is_pair_code(&code));
        assert_eq!(
            body["expires_at"].as_i64().unwrap(),
            hub.clock.now_ms() + arena_proto::PAIR_CODE_TTL_MS
        );
        let (status, _) = call(&hub, "POST", "/ui/pairing-code", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/pairing-code/discard",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(hub.pairing.has_live_code());
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/pairing-code/discard",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!hub.pairing.has_live_code());
    }

    #[tokio::test]
    async fn a_follower_refuses_to_issue_a_code() {
        let hub = hub().await;
        let follower = crate::ui_auth::ui_auth_router(Arc::new(
            UiAuth::new(
                Arc::new(arena_core::StaffDirectory::open(&hub.path).unwrap()),
                Arc::new(arena_core::SnapshotReader::open(&hub.path).unwrap()),
                None,
                hub.clock.clone(),
            )
            .with_pairing(Arc::clone(&hub.pairing)),
        ));
        let (_, body) = call_app(
            &follower,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(serde_json::json!({"login": "manager", "password": "m-pass"})),
        )
        .await;
        let token = body["token"].as_str().unwrap().to_owned();
        let (status, body) =
            call_app(&follower, "POST", "/ui/pairing-code", Some(&token), None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "NOT_PRIMARY");
        assert!(!hub.pairing.has_live_code());
    }
    #[tokio::test]
    async fn a_manager_issues_a_kitchen_code_on_basic_only_and_a_counter_is_refused() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;

        // Free: the same flag test as the `kds.pair` gate.
        let (status, body) = call(&hub, "POST", "/ui/kds-pairing-code", Some(&manager), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["code"], "TIER_LOCKED");
        assert!(!hub.kds_pairing.has_live_code());

        arena_store::Store::open(&hub.path, 0)
            .unwrap()
            .write_tx(|tx| {
                tx.execute_batch(
                    "UPDATE settings SET value = 'kitchen' WHERE key = 'licence.flags'",
                )
                .map_err(arena_store::db_error)
            })
            .unwrap();
        let (status, body) = call(&hub, "POST", "/ui/kds-pairing-code", Some(&counter), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["code"], "PERMISSION_DENIED");
        let (status, body) = call(&hub, "POST", "/ui/kds-pairing-code", None, None).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("AUTH_FAILED"))
        );
        assert!(!hub.kds_pairing.has_live_code());

        let (status, body) = call(&hub, "POST", "/ui/kds-pairing-code", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(arena_proto::is_pair_code(body["code"].as_str().unwrap()));
        assert_eq!(
            body["expires_at"].as_i64().unwrap(),
            hub.clock.now_ms() + arena_proto::PAIR_CODE_TTL_MS
        );
        assert!(hub.kds_pairing.has_live_code());
        // It is the kitchen's code, not the kiosk's.
        assert!(!hub.pairing.has_live_code());

        // Issuing again replaces the code; discarding by another session leaves it.
        let (_, second) = call(&hub, "POST", "/ui/kds-pairing-code", Some(&manager), None).await;
        assert_ne!(second["code"], body["code"]);
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/kds-pairing-code/discard",
            Some(&counter),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(hub.kds_pairing.has_live_code());
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/kds-pairing-code/discard",
            Some(&manager),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!hub.kds_pairing.has_live_code());
    }

    #[tokio::test]
    async fn a_follower_refuses_to_issue_a_kitchen_code() {
        let hub = hub().await;
        let follower = crate::ui_auth::ui_auth_router(Arc::new(
            UiAuth::new(
                Arc::new(arena_core::StaffDirectory::open(&hub.path).unwrap()),
                Arc::new(arena_core::SnapshotReader::open(&hub.path).unwrap()),
                None,
                hub.clock.clone(),
            )
            .with_kds_pairing(Arc::clone(&hub.kds_pairing)),
        ));
        let (_, body) = call_app(
            &follower,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(serde_json::json!({"login": "manager", "password": "m-pass"})),
        )
        .await;
        let token = body["token"].as_str().unwrap().to_owned();
        let (status, body) = call_app(
            &follower,
            "POST",
            "/ui/kds-pairing-code",
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "NOT_PRIMARY");
        assert!(!hub.kds_pairing.has_live_code());
    }
}

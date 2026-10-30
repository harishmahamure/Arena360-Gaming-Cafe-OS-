//! The loopback routes behind "Restore from cloud backup" on a replacement Hub (Story 9.12).
//!
//! A Hub waiting in "Restore this branch" has no staff and no session, so these routes, like
//! `/ui/pair-hub`, sit behind the loopback guard only. The Owner already authorised the restore
//! with step-up MFA when releasing the branch key (Story 9.10); `start` is refused unless that key
//! is in the Hub's keystore and the tier is paid.
//!
//! * `GET  /ui/restore/cloud`          the download's progress, or `null`
//! * `POST /ui/restore/cloud`          start, or resume from the files already staged
//! * `POST /ui/restore/cloud/cancel`   stop and remove the staging folder
//! * `POST /ui/restore/cloud/partial`  after `RESTORE_GAP`, keep what is before the gap

use std::sync::Arc;

use arena_core::RestoreFetchControl;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::ui_auth::fail;

type Control = Arc<dyn RestoreFetchControl>;

async fn view(State(c): State<Control>) -> Response {
    Json(c.view()).into_response()
}

async fn start(State(c): State<Control>) -> Response {
    match c.start() {
        Ok(()) => Json(c.view()).into_response(),
        Err(e) => fail(e),
    }
}

async fn cancel(State(c): State<Control>) -> Response {
    c.cancel();
    Json(c.view()).into_response()
}

async fn partial(State(c): State<Control>) -> Response {
    match c.restore_partial() {
        Ok(()) => Json(c.view()).into_response(),
        Err(e) => fail(e),
    }
}

pub fn ui_restore_fetch_router(control: Control) -> Router {
    Router::new()
        .route("/ui/restore/cloud", get(view).post(start))
        .route("/ui/restore/cloud/cancel", post(cancel))
        .route("/ui/restore/cloud/partial", post(partial))
        .with_state(control)
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use arena_proto::{ErrorCode, RestoreFetchPhase, RestoreFetchView, UiError};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use tower::ServiceExt;

    struct Scripted {
        view: Mutex<Option<RestoreFetchView>>,
        allowed: bool,
        calls: Mutex<Vec<&'static str>>,
    }

    fn running() -> RestoreFetchView {
        RestoreFetchView {
            phase: RestoreFetchPhase::Downloading,
            done: 3,
            total: 41,
            bytes: 1024,
            percent: 7,
            retrying: None,
            error: None,
            gap: None,
            ready: None,
        }
    }

    impl RestoreFetchControl for Scripted {
        fn view(&self) -> Option<RestoreFetchView> {
            self.view.lock().unwrap().clone()
        }
        fn start(&self) -> Result<(), UiError> {
            self.calls.lock().unwrap().push("start");
            if !self.allowed {
                return Err(UiError {
                    code: ErrorCode::KeyNotReleased,
                    message: "wait".into(),
                    field: None,
                    retry_after_s: None,
                });
            }
            *self.view.lock().unwrap() = Some(running());
            Ok(())
        }
        fn cancel(&self) {
            self.calls.lock().unwrap().push("cancel");
            *self.view.lock().unwrap() = None;
        }
        fn restore_partial(&self) -> Result<(), UiError> {
            self.calls.lock().unwrap().push("partial");
            Err(UiError {
                code: ErrorCode::NotFound,
                message: "no gap".into(),
                field: None,
                retry_after_s: None,
            })
        }
    }

    async fn call(app: &Router, method: &str, path: &str, host: &str) -> (u16, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = to_bytes(res.into_body(), 1 << 16).await.unwrap();
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, body)
    }

    #[tokio::test]
    async fn the_routes_start_show_cancel_and_map_refusals() {
        let control = Arc::new(Scripted {
            view: Mutex::new(None),
            allowed: true,
            calls: Mutex::default(),
        });
        let app = ui_restore_fetch_router(Arc::clone(&control) as Control);
        let host = "127.0.0.1:7409";
        assert_eq!(
            call(&app, "GET", "/ui/restore/cloud", host).await,
            (200, serde_json::Value::Null)
        );
        let (s, b) = call(&app, "POST", "/ui/restore/cloud", host).await;
        assert_eq!(
            (s, b["phase"].as_str(), b["done"].as_u64()),
            (200, Some("downloading"), Some(3))
        );
        let (s, b) = call(&app, "GET", "/ui/restore/cloud", host).await;
        assert_eq!(
            (s, b["total"].as_u64(), b["percent"].as_u64()),
            (200, Some(41), Some(7))
        );
        let (s, b) = call(&app, "POST", "/ui/restore/cloud/partial", host).await;
        assert_eq!((s, b["code"].as_str()), (404, Some("NOT_FOUND")));
        let (s, b) = call(&app, "POST", "/ui/restore/cloud/cancel", host).await;
        assert_eq!((s, b), (200, serde_json::Value::Null));
        assert_eq!(
            *control.calls.lock().unwrap(),
            ["start", "partial", "cancel"]
        );
    }

    #[tokio::test]
    async fn a_refused_start_answers_the_code_and_a_foreign_host_is_forbidden() {
        let control = Arc::new(Scripted {
            view: Mutex::new(None),
            allowed: false,
            calls: Mutex::default(),
        });
        let app = ui_restore_fetch_router(Arc::clone(&control) as Control);
        let (s, b) = call(&app, "POST", "/ui/restore/cloud", "127.0.0.1:7409").await;
        assert_eq!((s, b["code"].as_str()), (403, Some("KEY_NOT_RELEASED")));
        let (s, _) = call(&app, "POST", "/ui/restore/cloud", "192.168.1.20:7409").await;
        assert_eq!(s, 403);
        assert_eq!(
            control.calls.lock().unwrap().len(),
            1,
            "the foreign call never ran"
        );
    }
}

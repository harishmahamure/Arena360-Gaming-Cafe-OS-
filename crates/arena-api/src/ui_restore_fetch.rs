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
//! * `GET  /ui/restore/cloud/rebuild`  the rebuild's progress, or `null` (Story 9.13)
//! * `POST /ui/restore/cloud/rebuild`  rebuild the branch from the downloaded backup, then restart
//! * `POST /ui/restore/cloud/rebuild/cancel`  stop the rebuild before the restart

use std::sync::Arc;

use arena_core::{RestoreFetchControl, RestoreRebuildControl};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::ui_auth::fail;

type Control = Arc<dyn RestoreFetchControl>;

#[derive(Clone)]
struct Controls {
    fetch: Control,
    rebuild: Arc<dyn RestoreRebuildControl>,
}

async fn view(State(c): State<Controls>) -> Response {
    Json(c.fetch.view()).into_response()
}

async fn start(State(c): State<Controls>) -> Response {
    match c.fetch.start() {
        Ok(()) => Json(c.fetch.view()).into_response(),
        Err(e) => fail(e),
    }
}

async fn cancel(State(c): State<Controls>) -> Response {
    c.fetch.cancel();
    Json(c.fetch.view()).into_response()
}

async fn partial(State(c): State<Controls>) -> Response {
    match c.fetch.restore_partial() {
        Ok(()) => Json(c.fetch.view()).into_response(),
        Err(e) => fail(e),
    }
}

async fn rebuild_view(State(c): State<Controls>) -> Response {
    Json(c.rebuild.view()).into_response()
}

async fn rebuild_start(State(c): State<Controls>) -> Response {
    match c.rebuild.start() {
        Ok(()) => Json(c.rebuild.view()).into_response(),
        Err(e) => fail(e),
    }
}

async fn rebuild_cancel(State(c): State<Controls>) -> Response {
    c.rebuild.cancel();
    Json(c.rebuild.view()).into_response()
}

pub fn ui_restore_fetch_router(fetch: Control, rebuild: Arc<dyn RestoreRebuildControl>) -> Router {
    Router::new()
        .route("/ui/restore/cloud", get(view).post(start))
        .route("/ui/restore/cloud/cancel", post(cancel))
        .route("/ui/restore/cloud/partial", post(partial))
        .route(
            "/ui/restore/cloud/rebuild",
            get(rebuild_view).post(rebuild_start),
        )
        .route("/ui/restore/cloud/rebuild/cancel", post(rebuild_cancel))
        .with_state(Controls { fetch, rebuild })
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use arena_proto::{
        ErrorCode, RestoreFetchPhase, RestoreFetchView, RestoreRebuildPhase, RestoreRebuildView,
        UiError,
    };
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

    struct ScriptedRebuild {
        view: Mutex<Option<RestoreRebuildView>>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl RestoreRebuildControl for ScriptedRebuild {
        fn view(&self) -> Option<RestoreRebuildView> {
            self.view.lock().unwrap().clone()
        }
        fn start(&self) -> Result<(), UiError> {
            self.calls.lock().unwrap().push("rebuild");
            *self.view.lock().unwrap() = Some(RestoreRebuildView {
                phase: RestoreRebuildPhase::Replaying,
                done: 2,
                total: 9,
                percent: 22,
                error: None,
            });
            Ok(())
        }
        fn cancel(&self) {
            self.calls.lock().unwrap().push("rebuild-cancel");
            *self.view.lock().unwrap() = None;
        }
    }

    fn rebuild_control() -> Arc<ScriptedRebuild> {
        Arc::new(ScriptedRebuild {
            view: Mutex::new(None),
            calls: Mutex::default(),
        })
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
        let app = ui_restore_fetch_router(Arc::clone(&control) as Control, rebuild_control());
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
        let app = ui_restore_fetch_router(Arc::clone(&control) as Control, rebuild_control());
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

#[cfg(test)]
mod rebuild_tests {
    use super::*;
    use std::sync::Mutex;

    use arena_proto::{RestoreFetchView, RestoreRebuildPhase, RestoreRebuildView, UiError};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use tower::ServiceExt;

    struct NoFetch;

    impl RestoreFetchControl for NoFetch {
        fn view(&self) -> Option<RestoreFetchView> {
            None
        }
        fn start(&self) -> Result<(), UiError> {
            Ok(())
        }
        fn cancel(&self) {}
        fn restore_partial(&self) -> Result<(), UiError> {
            Ok(())
        }
    }

    struct Rebuild(Mutex<Option<RestoreRebuildView>>);

    impl RestoreRebuildControl for Rebuild {
        fn view(&self) -> Option<RestoreRebuildView> {
            self.0.lock().unwrap().clone()
        }
        fn start(&self) -> Result<(), UiError> {
            let mut view = self.0.lock().unwrap();
            if view.is_some() {
                return Ok(());
            }
            *view = Some(RestoreRebuildView {
                phase: RestoreRebuildPhase::Replaying,
                done: 1,
                total: 4,
                percent: 25,
                error: None,
            });
            Ok(())
        }
        fn cancel(&self) {
            *self.0.lock().unwrap() = None;
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn the_rebuild_routes_start_show_and_cancel_on_loopback_only() {
        let app = ui_restore_fetch_router(Arc::new(NoFetch), Arc::new(Rebuild(Mutex::new(None))));
        let host = "127.0.0.1:7409";
        let path = "/ui/restore/cloud/rebuild";
        assert_eq!(
            call(&app, "GET", path, host).await,
            (200, serde_json::Value::Null)
        );
        let (s, b) = call(&app, "POST", path, host).await;
        assert_eq!(
            (s, b["phase"].as_str(), b["percent"].as_u64()),
            (200, Some("replaying"), Some(25))
        );
        let (s, b) = call(&app, "GET", path, host).await;
        assert_eq!(
            (s, b["done"].as_u64(), b["total"].as_u64()),
            (200, Some(1), Some(4))
        );
        let (s, b) = call(&app, "POST", &format!("{path}/cancel"), host).await;
        assert_eq!((s, b), (200, serde_json::Value::Null));
        // Another PC on the LAN cannot rebuild this Hub.
        let (s, _) = call(&app, "POST", path, "192.168.1.20:7409").await;
        assert_eq!(s, 403);
        assert!(call(&app, "GET", path, host).await.1.is_null());
    }
}

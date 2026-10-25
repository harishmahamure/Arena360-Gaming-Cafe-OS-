use std::sync::Arc;

use arena_core::{HubPairer, PairGate};
use arena_proto::{ErrorCode, HubSnapshot, PairHubOk, PairHubRequest, UiError};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::ui_auth::{bad_frame, fail};

pub const PAIR_EXEMPT_PATHS: [&str; 2] = ["/ui/pair-hub", "/ui/snapshot"];

pub fn hub_not_paired() -> UiError {
    UiError {
        code: ErrorCode::HubNotPaired,
        message: "pair this hub with its branch first".into(),
        field: None,
        retry_after_s: None,
    }
}

fn already_paired() -> UiError {
    UiError {
        code: ErrorCode::AlreadySet,
        message: "this hub is already paired".into(),
        field: None,
        retry_after_s: None,
    }
}

pub fn unpaired_snapshot(full: HubSnapshot) -> HubSnapshot {
    HubSnapshot {
        branch: None,
        groups: Vec::new(),
        zones: Vec::new(),
        devices: Vec::new(),
        locale: None,
        ui: full.ui,
        connectivity: full.connectivity,
        setup: None,
        lan: None,
        paired: false,
        pairing_revoked: None,
        licence_unverified: None,
        clock_wrong: None,
    }
}

pub fn failure(e: UiError) -> Response {
    let retry = e.retry_after_s;
    let mut res = fail(e);
    if let Some(s) = retry {
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(s.max(1)));
    }
    res
}

pub(crate) async fn pair_gate(
    State(gate): State<Option<PairGate>>,
    req: Request,
    next: Next,
) -> Response {
    let blocked = gate.as_ref().is_some_and(|g| !g.is_paired())
        && req.uri().path().starts_with("/ui/")
        && !PAIR_EXEMPT_PATHS.contains(&req.uri().path());
    if blocked {
        return fail(hub_not_paired());
    }
    next.run(req).await
}

#[derive(Clone)]
struct PairState {
    pairer: Arc<dyn HubPairer>,
    gate: PairGate,
}

async fn pair_hub(State(st): State<PairState>, body: axum::body::Bytes) -> Response {
    if st.gate.is_paired() {
        return fail(already_paired());
    }
    let Ok(req) = serde_json::from_slice::<PairHubRequest>(&body) else {
        return fail(bad_frame("the body must be {\"code\"}"));
    };
    match st.pairer.pair(&req.code).await {
        Ok(()) => Json(PairHubOk { paired: true }).into_response(),
        Err(e) if e.code == ErrorCode::AlreadySet => fail(e),
        Err(e) => failure(e),
    }
}

pub fn ui_pair_router(pairer: Arc<dyn HubPairer>, gate: PairGate) -> Router {
    Router::new()
        .route("/ui/pair-hub", post(pair_hub))
        .with_state(PairState { pairer, gate })
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use arena_core::PairFuture;
    use axum::body::{Body, to_bytes};
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    struct Scripted {
        answers: Mutex<Vec<Result<(), UiError>>>,
        seen: Mutex<Vec<String>>,
        gate: PairGate,
    }

    impl HubPairer for Scripted {
        fn pair<'a>(&'a self, code: &'a str) -> PairFuture<'a> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(code.to_owned());
                let answer = self.answers.lock().unwrap().remove(0);
                if answer.is_ok() {
                    self.gate.mark_paired();
                }
                answer
            })
        }
    }

    fn err(code: ErrorCode, retry: Option<u32>) -> UiError {
        UiError {
            code,
            message: "m".into(),
            field: None,
            retry_after_s: retry,
        }
    }

    async fn post(app: &Router, body: &str) -> (u16, Option<String>, serde_json::Value) {
        let req = HttpRequest::post("/ui/pair-hub")
            .header(header::HOST, "127.0.0.1:7409")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let retry = res
            .headers()
            .get(header::RETRY_AFTER)
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = to_bytes(res.into_body(), 1 << 16).await.unwrap();
        (status, retry, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn pair_hub_maps_each_outcome_and_refuses_once_paired() {
        let gate = PairGate::new(false);
        let pairer = Arc::new(Scripted {
            answers: Mutex::new(vec![
                Err(err(ErrorCode::PairCodeInvalid, None)),
                Err(err(ErrorCode::FeatureNotEntitled, None)),
                Err(err(ErrorCode::RateLimited, Some(90))),
                Err(err(ErrorCode::CloudUnreachable, None)),
                Ok(()),
            ]),
            seen: Mutex::new(Vec::new()),
            gate: gate.clone(),
        });
        let app = ui_pair_router(Arc::clone(&pairer) as Arc<dyn HubPairer>, gate.clone());
        let body = r#"{"code":"7k2m 9pqr"}"#;
        let (s, _, b) = post(&app, body).await;
        assert_eq!((s, b["code"].as_str()), (401, Some("PAIR_CODE_INVALID")));
        let (s, _, b) = post(&app, body).await;
        assert_eq!((s, b["code"].as_str()), (403, Some("FEATURE_NOT_ENTITLED")));
        let (s, retry, b) = post(&app, body).await;
        assert_eq!((s, b["code"].as_str()), (429, Some("RATE_LIMITED")));
        assert_eq!(retry.as_deref(), Some("90"));
        assert_eq!(b["retry_after_s"], 90);
        let (s, _, b) = post(&app, body).await;
        assert_eq!((s, b["code"].as_str()), (503, Some("CLOUD_UNREACHABLE")));
        let (s, _, b) = post(&app, body).await;
        assert_eq!((s, b), (200, serde_json::json!({"paired": true})));
        let (s, _, b) = post(&app, body).await;
        assert_eq!((s, b["code"].as_str()), (409, Some("ALREADY_SET")));
        assert_eq!(pairer.seen.lock().unwrap().len(), 5);
        let (s, _, b) = post(&ui_pair_router(pairer, PairGate::new(false)), "nope").await;
        assert_eq!((s, b["code"].as_str()), (400, Some("BAD_FRAME")));
    }

    #[tokio::test]
    async fn the_gate_blocks_other_ui_routes_until_paired() {
        let gate = PairGate::new(false);
        let app = Router::new()
            .route("/ui/me", axum::routing::get(|| async { "me" }))
            .route("/ui/snapshot", axum::routing::get(|| async { "snap" }))
            .route("/healthz", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Some(gate.clone()),
                pair_gate,
            ));
        let get = |path: &'static str| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(HttpRequest::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = res.status().as_u16();
                let bytes = to_bytes(res.into_body(), 1 << 16).await.unwrap();
                (status, String::from_utf8(bytes.to_vec()).unwrap())
            }
        };
        let (s, b) = get("/ui/me").await;
        assert_eq!(s, 409);
        assert!(b.contains("HUB_NOT_PAIRED"), "{b}");
        assert_eq!(get("/ui/snapshot").await, (200, "snap".to_owned()));
        assert_eq!(get("/healthz").await, (200, "ok".to_owned()));
        gate.mark_paired();
        assert_eq!(get("/ui/me").await, (200, "me".to_owned()));
    }

    #[test]
    fn the_unpaired_snapshot_keeps_only_the_minimal_fields() {
        let full = HubSnapshot {
            branch: Some(arena_proto::SnapshotBranch {
                name: "B".into(),
                branch_code: "B1".into(),
            }),
            groups: vec![],
            zones: vec![],
            devices: vec![],
            locale: None,
            ui: arena_proto::SnapshotUi::default(),
            connectivity: arena_proto::Connectivity::default(),
            setup: None,
            lan: None,
            paired: true,
            pairing_revoked: None,
            licence_unverified: None,
            clock_wrong: None,
        };
        let min = unpaired_snapshot(full);
        assert!(!min.paired);
        assert_eq!(min.branch, None);
    }
}

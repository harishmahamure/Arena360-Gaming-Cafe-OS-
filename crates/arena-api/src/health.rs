//! `GET /healthz` on `127.0.0.1` only.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use arena_core::{Clock, HubStatus};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// The default health port.
pub const DEFAULT_HEALTH_PORT: u16 = 7409;

#[derive(Debug, Serialize, PartialEq, Eq)]
struct LeaseBody {
    counter: i64,
    hub_instance_id: String,
    provisional: bool,
    expires_at: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct HealthBody {
    db_open: bool,
    role: &'static str,
    lease: Option<LeaseBody>,
    epoch: Option<String>,
    schema_version: u32,
    self_instance_id: String,
    tls_fingerprint: Option<String>,
    now: i64,
}

impl HealthBody {
    fn new(s: &HubStatus, now: i64) -> Self {
        Self {
            db_open: s.db_open,
            role: s.role.as_str(),
            lease: s.lease.as_ref().map(|l| LeaseBody {
                counter: l.counter,
                hub_instance_id: l.hub_instance_id.clone(),
                provisional: l.provisional,
                expires_at: l.expires_at,
            }),
            epoch: s.lease.as_ref().map(|l| l.epoch().to_string()),
            schema_version: s.schema_version,
            self_instance_id: s.self_instance_id.clone(),
            tls_fingerprint: s.tls_fingerprint.clone(),
            now,
        }
    }
}

#[derive(Clone)]
struct HealthState {
    status: watch::Receiver<HubStatus>,
    clock: Arc<dyn Clock>,
}

async fn healthz(State(state): State<HealthState>) -> Json<HealthBody> {
    let now = state.clock.now_ms();
    let body = HealthBody::new(&state.status.borrow(), now);
    Json(body)
}

/// The health router. `now` in the body comes from `clock`.
pub fn router(status: watch::Receiver<HubStatus>, clock: Arc<dyn Clock>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(HealthState { status, clock })
}

/// Binds the health listener on loopback only.
pub async fn bind_health(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// Serves `router` on `listener` until `shutdown` resolves.
pub async fn serve_router(
    listener: TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Dev-only loopback endpoints (feature `dev-fixtures`, never in release builds):
/// `POST /dev/revoke-device` with `{"device_id": "…"}` submits `device.revoke` as
/// `actor{system, "dev-revoke"}` through the running Hub's writer. It answers 200 with
/// `{"applied": …}`, or an error status with `{"error": AppError}`.
#[cfg(feature = "dev-fixtures")]
pub fn dev_router(writer: Option<arena_core::WriterHandle>, clock: Arc<dyn Clock>) -> Router {
    use arena_proto::{
        Actor, ActorKind, AppError, DeviceRevoke, ErrorCode, Intent, IntentKind, IntentPayload,
    };
    use axum::http::StatusCode;
    use axum::routing::post;

    #[derive(Clone)]
    struct DevState {
        writer: Option<arena_core::WriterHandle>,
        clock: Arc<dyn Clock>,
    }

    fn status_of(code: ErrorCode) -> StatusCode {
        match code {
            ErrorCode::BadFrame => StatusCode::BAD_REQUEST,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotPrimary => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::ValidationFailed | ErrorCode::Stale => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    async fn revoke(
        State(state): State<DevState>,
        body: String,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let fail = |e: AppError| (status_of(e.code), Json(serde_json::json!({ "error": e })));
        let device_id = match serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("device_id")?.as_str().map(str::to_owned))
        {
            Some(id) => id,
            None => {
                return fail(AppError::new(
                    ErrorCode::BadFrame,
                    "the body must be {\"device_id\": \"…\"}",
                ));
            }
        };
        let Some(writer) = state.writer else {
            return fail(AppError::new(
                ErrorCode::NotPrimary,
                "this hub is not the primary writer",
            ));
        };
        let intent = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::DeviceRevoke,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: state.clock.now_ms(),
            base_rev: None,
            actor: Actor {
                kind: ActorKind::System,
                id: "dev-revoke".into(),
            },
            elevated_by: None,
            payload: IntentPayload::DeviceRevoke(DeviceRevoke { device_id }),
        };
        match writer.submit_intent(intent).await {
            Ok(applied) => (
                StatusCode::OK,
                Json(serde_json::json!({ "applied": crate::lan::registry::summary(&applied) })),
            ),
            Err(e) => fail(e),
        }
    }

    Router::new()
        .route("/dev/revoke-device", post(revoke))
        .with_state(DevState { writer, clock })
}

/// Serves `/healthz` on `listener` until `shutdown` resolves.
pub async fn serve_health(
    listener: TcpListener,
    status: watch::Receiver<HubStatus>,
    clock: Arc<dyn Clock>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router(status, clock))
        .with_graceful_shutdown(shutdown)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{LeaseInfo, ManualClock, Role, status_channel};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn get_json(rx: watch::Receiver<HubStatus>) -> Value {
        let res = router(rx, Arc::new(ManualClock::new(1_000)))
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn reports_primary_with_lease_and_epoch() {
        let (tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        tx.send_modify(|s| {
            s.role = Role::Primary;
            s.lease = Some(LeaseInfo {
                counter: 1,
                hub_instance_id: "01JX".into(),
                provisional: true,
                expires_at: 42,
            });
            s.tls_fingerprint = Some("ab".repeat(32));
        });
        assert_eq!(
            get_json(rx).await,
            json!({
                "db_open": true,
                "role": "primary",
                "lease": {"counter": 1, "hub_instance_id": "01JX", "provisional": true, "expires_at": 42},
                "epoch": "1:01JX",
                "schema_version": 1,
                "self_instance_id": "01JSELF",
                "tls_fingerprint": "ab".repeat(32),
                "now": 1000
            })
        );
    }

    #[tokio::test]
    async fn reports_follower_without_lease_as_null() {
        let (_tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        assert_eq!(
            get_json(rx).await,
            json!({"db_open": true, "role": "follower", "lease": null, "epoch": null, "schema_version": 1, "self_instance_id": "01JSELF", "tls_fingerprint": null, "now": 1000})
        );
    }

    #[cfg(feature = "dev-fixtures")]
    #[tokio::test]
    async fn dev_revoke_needs_a_body_and_a_writer() {
        let app = dev_router(None, Arc::new(ManualClock::new(0)));
        for (body, status) in [
            ("nope", StatusCode::BAD_REQUEST),
            (r#"{"device_id":"D"}"#, StatusCode::SERVICE_UNAVAILABLE),
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::post("/dev/revoke-device")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), status, "{body}");
        }
    }

    #[cfg(not(feature = "dev-fixtures"))]
    #[tokio::test]
    async fn no_dev_routes_without_the_feature() {
        let res = router(
            status_channel(HubStatus::opened(1, "H")).1,
            Arc::new(ManualClock::new(0)),
        )
        .oneshot(
            Request::post("/dev/revoke-device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn binds_loopback_only() {
        let listener = bind_health(0).await.unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }
}

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
                "now": 1000
            })
        );
    }

    #[tokio::test]
    async fn reports_follower_without_lease_as_null() {
        let (_tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        assert_eq!(
            get_json(rx).await,
            json!({"db_open": true, "role": "follower", "lease": null, "epoch": null, "schema_version": 1, "self_instance_id": "01JSELF", "now": 1000})
        );
    }

    #[tokio::test]
    async fn binds_loopback_only() {
        let listener = bind_health(0).await.unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }
}

//! `GET /ui/tunnel` (Story 8.39): the System screen's tunnel line, read from the Hub status the
//! tunnel task and the heartbeat keep. Loopback only (the caller adds the origin guard); it
//! carries the state and the hostname, never the credential.

use arena_core::HubStatus;
use arena_proto::TunnelLine;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use tokio::sync::watch;

type Status = Option<watch::Receiver<HubStatus>>;

async fn tunnel(State(status): State<Status>) -> Json<TunnelLine> {
    Json(
        status
            .as_ref()
            .map(|s| s.borrow().tunnel.clone())
            .unwrap_or_default(),
    )
}

/// The tunnel route. Without a status channel it answers `off`.
pub fn router(status: Status) -> Router {
    Router::new()
        .route("/ui/tunnel", get(tunnel))
        .with_state(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::status_channel;
    use arena_proto::TunnelState;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn get_line(app: Router, host: &str, origin: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::get("/ui/tunnel").header(header::HOST, host);
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 4096).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn guarded(app: Router) -> Router {
        app.layer(axum::middleware::from_fn(crate::health::ui_guard))
    }

    #[tokio::test]
    async fn it_reports_the_line_the_tunnel_task_published() {
        let (tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        let app = || guarded(router(Some(rx.clone())));
        assert_eq!(
            get_line(app(), "127.0.0.1:7409", None).await,
            (StatusCode::OK, json!({"state": "off"}))
        );
        tx.send_modify(|s| {
            s.tunnel = TunnelLine::new(TunnelState::Reconnecting, Some("B.tunnel.x".into()));
        });
        assert_eq!(
            get_line(app(), "127.0.0.1:7409", Some("tauri://localhost")).await,
            (
                StatusCode::OK,
                json!({"state": "reconnecting", "hostname": "B.tunnel.x"})
            )
        );
        tx.send_modify(|s| {
            s.tunnel = TunnelLine::new(TunnelState::UnsupportedPlatform, None);
        });
        assert_eq!(
            get_line(app(), "127.0.0.1:7409", None).await.1,
            json!({"state": "unsupported_platform"})
        );
    }

    #[tokio::test]
    async fn only_the_hub_ui_may_read_it() {
        let (_tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        let app = guarded(router(Some(rx)));
        assert_eq!(
            get_line(app.clone(), "example.com", None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get_line(app, "127.0.0.1:7409", Some("https://evil.example"))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn without_a_status_channel_it_answers_off() {
        assert_eq!(
            get_line(guarded(router(None)), "localhost", None).await,
            (StatusCode::OK, json!({"state": "off"}))
        );
    }
}

//! `GET /ui/standby` (Story 9.20): the standby screen on a standby Hub and Settings -> System ->
//! Standby on the main Hub. Loopback only (the caller adds the origin guard). Read-only: it shows
//! lag, last contact and version; unlinking needs a manager PIN and is not served here.

use std::path::PathBuf;
use std::sync::Arc;

use arena_core::{Clock, HubStatus};
use arena_proto::{StandbyScreen, StandbyView};
use arena_store::Store;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::sync::watch;

use crate::StandbyObserver;

#[derive(Clone)]
struct UiStandby {
    status: watch::Receiver<HubStatus>,
    clock: Arc<dyn Clock>,
    /// What the main Hub's stream saw of the standby; absent on a Hub that serves none.
    observer: Option<StandbyObserver>,
    db_path: PathBuf,
}

async fn standby(State(s): State<UiStandby>) -> Response {
    let (following, licence, needs_plan) = {
        let status = s.status.borrow();
        (
            status.following.clone(),
            status.licence.map(|l| l.as_str().to_owned()),
            status.standby_needs_plan,
        )
    };
    let now = s.clock.now_ms();
    let db = s.db_path.clone();
    let observer = s.observer.clone();
    let read =
        tokio::task::spawn_blocking(move || -> Result<StandbyView, arena_proto::AppError> {
            let store = Store::open_reader(&db)?;
            if let Some(report) = following {
                return Ok(StandbyView {
                    screen: Some(StandbyScreen {
                        following: store.branch_name()?,
                        report,
                        licence,
                        needs_plan,
                    }),
                    panel: None,
                });
            }
            let panel = match (observer, store.linked_standby()?) {
                (Some(o), Some(linked)) => Some(o.panel(&linked, now)),
                _ => None,
            };
            Ok(StandbyView {
                screen: None,
                panel,
            })
        })
        .await;
    match read {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), "ui standby failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "code": e.code })),
            )
                .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The standby route. `observer` is the main Hub's handle from its replication source
/// (`LanHub::replication`), `db_path` the Hub's own database.
pub fn ui_standby_router(
    status: watch::Receiver<HubStatus>,
    clock: Arc<dyn Clock>,
    observer: Option<StandbyObserver>,
    db_path: PathBuf,
) -> Router {
    Router::new()
        .route("/ui/standby", get(standby))
        .with_state(UiStandby {
            status,
            clock,
            observer,
            db_path,
        })
        .layer(axum::middleware::from_fn(crate::health::ui_guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::{ManualClock, status_channel};
    use arena_proto::{StandbyBehindCause, StandbyReport};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use serde_json::Value;
    use tower::ServiceExt;

    const T0: i64 = 1_800_000_000_000;

    async fn get_view(app: &Router) -> Value {
        let req = Request::get("/ui/standby")
            .header(header::HOST, "127.0.0.1:7409")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1 << 16).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn report(lag_rows: i64, lag_seconds: i64) -> StandbyReport {
        StandbyReport {
            applied_epoch: Some("1:a".into()),
            applied_seq: Some(9),
            lag_rows,
            lag_seconds,
            last_row_at: Some(T0 - 1_000),
            schema_version: 90,
            behind: (lag_seconds > 30).then_some(StandbyBehindCause::Lagging),
        }
    }

    #[tokio::test]
    async fn a_hub_with_no_standby_and_no_follow_answers_an_empty_view() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.db");
        Store::open(&path, T0).unwrap();
        let (_tx, rx) = status_channel(HubStatus::opened(90, "01JSELF"));
        let app = ui_standby_router(
            rx,
            Arc::new(ManualClock::new(T0)),
            Some(StandbyObserver::default()),
            path,
        );
        assert_eq!(get_view(&app).await, serde_json::json!({}));
    }

    #[tokio::test]
    async fn a_standby_shows_who_it_follows_its_lag_and_licence_and_the_main_hub_shows_the_standby()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.db");
        let mut store = Store::open(&path, T0).unwrap();
        store
            .write_tx(|tx| {
                tx.execute(
                    "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min, \
                     fy_start_month, rev, created_at, updated_at) \
                     VALUES ('B', 'Arena One', 'AR1', 'INR', 'Asia/Kolkata', 360, 4, 1, 1, 1)",
                    [],
                )
                .map_err(arena_store::db_error)?;
                arena_store::link_standby(tx, "dev-s", T0 - 90_000)
            })
            .unwrap();
        drop(store);

        // The standby's own screen.
        let (tx, rx) = status_channel(HubStatus::opened(90, "01JSELF"));
        tx.send_modify(|s| {
            s.following = Some(report(12, 44));
            s.licence = Some(arena_proto::LicenceStanding::Verified);
        });
        let app = ui_standby_router(rx, Arc::new(ManualClock::new(T0)), None, path.clone());
        let v = get_view(&app).await;
        assert_eq!(v["screen"]["following"], "Arena One");
        assert_eq!(v["screen"]["licence"], "verified");
        assert_eq!(v["screen"]["report"]["lag_rows"], 12);
        assert_eq!(v["screen"]["report"]["behind"], "lagging");
        assert!(v.get("panel").is_none());

        // The main Hub's panel for the linked standby: connected a moment ago, no report yet.
        let observer = StandbyObserver::default();
        observer.connected("dev-s", 90, T0 - 1_000);
        observer.reported("dev-s", report(0, 0), T0 - 500);
        let (_tx2, rx2) = status_channel(HubStatus::opened(90, "01JMAIN"));
        let app = ui_standby_router(rx2, Arc::new(ManualClock::new(T0)), Some(observer), path);
        let v = get_view(&app).await;
        assert!(v.get("screen").is_none());
        assert_eq!(v["panel"]["device_id"], "dev-s");
        assert_eq!(v["panel"]["connected"], true);
        assert_eq!(v["panel"]["last_contact_at"], T0 - 500);
        assert_eq!(v["panel"]["lag_rows"], 0);
    }
}

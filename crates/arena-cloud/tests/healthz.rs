//! `GET /healthz`: the whole body, from the cached reachability.

use arena_cloud::config::Role;
use arena_cloud::health::{Reachability, router};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

async fn get(rx: watch::Receiver<Reachability>, migration_version: u32) -> (StatusCode, Value) {
    let res = router(Role::Control, rx, migration_version)
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn reachable() {
    let (_tx, rx) = watch::channel(Reachability {
        reachable: true,
        checked_at: Some(1_000),
    });
    assert_eq!(
        get(rx, 1).await,
        (
            StatusCode::OK,
            json!({
                "role": "control",
                "version": env!("CARGO_PKG_VERSION"),
                "gz_control": {"reachable": true, "checked_at": 1000},
                "migration_version": 1
            })
        )
    );
}

#[tokio::test]
async fn unreachable_is_still_200_and_follows_the_cache() {
    let (tx, rx) = watch::channel(Reachability {
        reachable: false,
        checked_at: None,
    });
    assert_eq!(
        get(rx.clone(), 1).await,
        (
            StatusCode::OK,
            json!({
                "role": "control",
                "version": env!("CARGO_PKG_VERSION"),
                "gz_control": {"reachable": false, "checked_at": null},
                "migration_version": 1
            })
        )
    );
    tx.send(Reachability {
        reachable: true,
        checked_at: Some(5),
    })
    .unwrap();
    let (status, body) = get(rx, 1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["gz_control"],
        json!({"reachable": true, "checked_at": 5})
    );
}

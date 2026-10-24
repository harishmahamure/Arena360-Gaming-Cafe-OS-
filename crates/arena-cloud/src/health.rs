//! `GET /healthz` and the background `gz_control` reachability check.
//!
//! The handler only reads the cached [`Reachability`]: kubelet probes never become Turso load,
//! and it always answers 200, so a Turso blip never restarts a pod (liveness and readiness use
//! the same path).

use std::future::Future;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::watch;

use crate::config::Role;

/// How often the background check runs.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(15);
/// How long one `SELECT 1` may take.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(3);

/// The last `gz_control` check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Reachability {
    pub reachable: bool,
    /// Unix ms of the check, or `None` before the first one.
    pub checked_at: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct HealthBody {
    role: &'static str,
    version: &'static str,
    gz_control: Reachability,
    migration_version: u32,
}

#[derive(Clone)]
struct HealthState {
    role: Role,
    reach: watch::Receiver<Reachability>,
    migration_version: u32,
}

async fn healthz(State(s): State<HealthState>) -> Json<HealthBody> {
    let gz_control = *s.reach.borrow();
    Json(HealthBody {
        role: s.role.as_str(),
        version: env!("CARGO_PKG_VERSION"),
        gz_control,
        migration_version: s.migration_version,
    })
}

/// The health router: `GET /healthz` → 200 `{role, version, gz_control:{reachable,
/// checked_at}, migration_version}`.
pub fn router(role: Role, reach: watch::Receiver<Reachability>, migration_version: u32) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(HealthState {
            role,
            reach,
            migration_version,
        })
}

/// Runs `probe` every `interval` (the first at once), each bounded by `timeout`, and publishes
/// the result stamped with `now_ms()` to `tx`. Returns when every receiver is gone.
pub async fn reachability_loop<P, F>(
    tx: watch::Sender<Reachability>,
    interval: Duration,
    timeout: Duration,
    now_ms: impl Fn() -> i64,
    mut probe: P,
) where
    P: FnMut() -> F,
    F: Future<Output = Result<(), String>>,
{
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last = None;
    loop {
        tick.tick().await;
        let ok = match tokio::time::timeout(timeout, probe()).await {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                tracing::warn!(detail = %e, "gz_control check failed");
                false
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = timeout.as_millis() as u64,
                    "gz_control check timed out"
                );
                false
            }
        };
        if last != Some(ok) {
            tracing::info!(reachable = ok, "gz_control reachability");
            last = Some(ok);
        }
        let r = Reachability {
            reachable: ok,
            checked_at: Some(now_ms()),
        };
        if tx.send(r).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test(start_paused = true)]
    async fn a_failed_check_reports_unreachable_and_the_next_tick_recovers() {
        let (tx, mut rx) = watch::channel(Reachability {
            reachable: true,
            checked_at: Some(0),
        });
        let calls = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&calls);
        let task = tokio::spawn(reachability_loop(
            tx,
            Duration::from_secs(15),
            Duration::from_secs(3),
            || 42,
            move || {
                let n = c.fetch_add(1, Ordering::SeqCst);
                async move {
                    match n {
                        0 => Err("blip".to_owned()),
                        1 => {
                            // Hangs past the timeout.
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            Ok(())
                        }
                        _ => Ok(()),
                    }
                }
            },
        ));
        rx.changed().await.unwrap();
        assert_eq!(
            *rx.borrow_and_update(),
            Reachability {
                reachable: false,
                checked_at: Some(42)
            }
        );
        rx.changed().await.unwrap();
        assert!(!rx.borrow_and_update().reachable, "timed out");
        rx.changed().await.unwrap();
        assert!(rx.borrow_and_update().reachable, "recovered");
        drop(rx);
        task.await.unwrap();
    }
}

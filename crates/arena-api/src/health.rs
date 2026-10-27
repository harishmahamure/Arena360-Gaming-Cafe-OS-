//! The loopback-only HTTP server on `127.0.0.1`: `GET /healthz`, and `GET /ui/snapshot` for the
//! local Hub UI ([`ui_router`]). The staff auth routes live in [`crate::ui_auth`] behind the same
//! origin guard.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use arena_core::{Clock, HubStatus, SnapshotReader};
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
    connectivity: arena_proto::Connectivity,
    setup: Option<arena_proto::SetupStatus>,
    tier: Option<&'static str>,
    platform: &'static str,
    now: i64,
}

impl HealthBody {
    fn new(s: &HubStatus, now: i64, setup: Option<arena_proto::SetupStatus>) -> Self {
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
            connectivity: s.connectivity,
            setup,
            tier: s.tier.map(arena_proto::Tier::as_str),
            platform: s.platform.as_str(),
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
    let status = state.status.borrow();
    let body = HealthBody::new(&status, now, status.setup);
    Json(body)
}

/// The health router. `now` in the body comes from `clock`.
pub fn router(status: watch::Receiver<HubStatus>, clock: Arc<dyn Clock>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(HealthState { status, clock })
}

/// The origins allowed to read `/ui/*`: the Tauri Hub UI (`tauri://localhost` on macOS and
/// Linux, `http(s)://tauri.localhost` on Windows) and the vite dev/preview server.
pub const UI_ALLOWED_ORIGINS: [&str; 4] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:5173",
];

/// Host names a `/ui/*` request may be addressed to (blocks DNS-rebinding pages).
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

fn host_is_loopback(host: &str) -> bool {
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !name.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    LOOPBACK_HOSTS.iter().any(|h| h.eq_ignore_ascii_case(name))
}

/// CORS and origin guard for `/ui/*`: a request from another origin, without a `Host` header
/// or addressed to a non-loopback host name, is refused with 403. An allowed origin is echoed back in
/// `Access-Control-Allow-Origin`; a preflight is answered here with 204.
pub(crate) async fn ui_guard(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{HeaderValue, Method, StatusCode, header};
    use axum::response::IntoResponse;

    let headers = req.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(host_is_loopback);
    if !host_ok {
        return StatusCode::FORBIDDEN.into_response();
    }
    let origin = match headers.get(header::ORIGIN) {
        None => None,
        Some(o) => match o.to_str() {
            Ok(o) if UI_ALLOWED_ORIGINS.contains(&o) => Some(HeaderValue::from_str(o)),
            _ => return StatusCode::FORBIDDEN.into_response(),
        },
    };
    let origin = origin.and_then(Result::ok);
    let mut res = if req.method() == Method::OPTIONS {
        let mut r = StatusCode::NO_CONTENT.into_response();
        let h = r.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("authorization, content-type, x-arena-elevation"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        r
    } else {
        next.run(req).await
    };
    let h = res.headers_mut();
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(origin) = origin {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    res
}

#[derive(Clone)]
struct SnapshotState {
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    presence: crate::lan::Presence,
    status: Option<watch::Receiver<HubStatus>>,
    pairing: Option<arena_core::PairGate>,
}

pub fn snapshot_lan(status: &HubStatus) -> Option<arena_proto::SnapshotLan> {
    let fingerprint = status.tls_fingerprint.clone()?;
    let (lan_port, web_port) = status.lan_ports?;
    Some(arena_proto::SnapshotLan {
        addresses: status.lan_addresses.iter().map(|a| a.to_string()).collect(),
        lan_port,
        web_port,
        fingerprint,
        network: status.lan_network.clone(),
        no_lan: status.platform == arena_proto::HubPlatform::Android
            && status.lan_addresses.is_empty(),
    })
}

async fn ui_snapshot(State(state): State<SnapshotState>) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let now = state.clock.now_ms();
    let reader = state.reader;
    let read = tokio::task::spawn_blocking(move || reader.snapshot(now)).await;
    match read {
        Ok(Ok(mut snapshot)) => {
            if let Some(gate) = state.pairing.as_ref().filter(|g| !g.is_paired()) {
                snapshot = crate::ui_pair::unpaired_snapshot(snapshot);
                snapshot.needs_repair = gate.needs_repair().then_some(true);
            }
            for d in &mut snapshot.devices {
                d.online = state.presence.online(&d.device_id);
                let (shown, seen) = state.presence.status(&d.device_id);
                d.apply_presence(shown, seen);
            }
            if let Some(status) = &state.status {
                let status = status.borrow();
                snapshot.connectivity = status.connectivity;
                if snapshot.paired {
                    if status.setup.is_some() {
                        snapshot.setup = status.setup;
                    }
                    snapshot.lan = snapshot_lan(&status);
                    snapshot.pairing_revoked =
                        (status.pairing == arena_core::PairingState::Revoked).then_some(true);
                    snapshot.licence_unverified = (status.licence
                        == Some(arena_proto::LicenceStanding::Unverified))
                    .then_some(true);
                    snapshot.clock_wrong = status.clock_wrong.then_some(true);
                    snapshot.tier = status.tier;
                }
            }
            Json(snapshot).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), detail = %e.message, "ui snapshot failed");
            // Only the code: the message may carry database details.
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "code": e.code })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(detail = %e, "ui snapshot task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The local Hub UI's read API (merged into the loopback health server):
/// `GET /ui/snapshot` returns a [`arena_proto::HubSnapshot`], its locale resolved at `clock`'s
/// now, each device's `online` read from the LAN registry through `presence`, and
/// `connectivity` from `status` (the default `online` without one). Only the
/// [`UI_ALLOWED_ORIGINS`] may read it from a browser.
pub fn ui_router(
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    presence: crate::lan::Presence,
    status: Option<watch::Receiver<HubStatus>>,
    pairing: Option<arena_core::PairGate>,
) -> Router {
    Router::new()
        .route("/ui/snapshot", get(ui_snapshot))
        .with_state(SnapshotState {
            reader,
            clock,
            presence,
            status,
            pairing,
        })
        .layer(axum::middleware::from_fn(ui_guard))
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
/// `POST /dev/revoke-device` with `{"device_id": "…", "reason"?: "lost"}` (the reason defaults
/// to `other`) submits `device.revoke` as
/// `actor{owner, "dev-revoke"}` (`device.remote` is a permission, which the owner holds) through the running Hub's writer. It answers 200 with
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
        let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
        // A revoke records why; the dev route defaults to `other`, and refuses a reason it does
        // not know.
        let reason = match parsed.as_ref().and_then(|v| v.get("reason")) {
            None => arena_proto::ReasonCode::Other,
            Some(raw) => match serde_json::from_value(raw.clone()) {
                Ok(r) => r,
                Err(_) => {
                    return fail(AppError::with_field(
                        ErrorCode::ValidationFailed,
                        "reason",
                        "reason must be lost, stolen, replaced or other",
                    ));
                }
            },
        };
        let device_id = match parsed
            .as_ref()
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
                kind: ActorKind::Owner,
                id: "dev-revoke".into(),
            },
            elevated_by: None,
            reason: Some(reason),
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

#[cfg(feature = "dev-fixtures")]
pub type PrintedJobs = Arc<dyn Fn() -> Vec<(String, Vec<u8>)> + Send + Sync>;

#[cfg(feature = "dev-fixtures")]
pub fn dev_printer_router(jobs: PrintedJobs) -> Router {
    async fn list(State(jobs): State<PrintedJobs>) -> Json<serde_json::Value> {
        let jobs: Vec<serde_json::Value> = jobs()
            .into_iter()
            .map(|(target, bytes)| {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                serde_json::json!({ "target": target, "bytes_hex": hex })
            })
            .collect();
        Json(serde_json::json!({ "jobs": jobs }))
    }

    Router::new()
        .route("/dev/printer/jobs", get(list))
        .with_state(jobs)
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
            s.connectivity = arena_proto::Connectivity {
                state: arena_proto::ConnectivityState::InternetOffline,
                checked_at: Some(900),
            };
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
                "connectivity": {"state": "internet_offline", "checked_at": 900},
                "setup": null,
                "tier": null,
                "platform": "windows",
                "now": 1000
            })
        );
    }

    #[tokio::test]
    async fn reports_the_android_platform() {
        let (tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        tx.send_modify(|s| s.platform = arena_proto::HubPlatform::Android);
        assert_eq!(get_json(rx).await["platform"], "android");
    }

    #[test]
    fn the_snapshot_flags_an_android_hub_with_no_lan_and_names_its_network() {
        let mut s = HubStatus::opened(1, "01JSELF");
        s.tls_fingerprint = Some("ab".repeat(32));
        s.lan_ports = Some((7400, 7401));
        s.platform = arena_proto::HubPlatform::Android;
        let lan = snapshot_lan(&s).unwrap();
        assert!(lan.no_lan && lan.addresses.is_empty() && lan.network.is_none());

        s.lan_addresses = vec![std::net::Ipv4Addr::new(192, 168, 1, 20)];
        s.lan_network = Some("Wi-Fi".into());
        let lan = snapshot_lan(&s).unwrap();
        assert!(!lan.no_lan);
        assert_eq!(lan.network.as_deref(), Some("Wi-Fi"));

        // A Windows Hub with no address is not flagged.
        s.platform = arena_proto::HubPlatform::Windows;
        s.lan_addresses.clear();
        assert!(!snapshot_lan(&s).unwrap().no_lan);
    }

    #[tokio::test]
    async fn reports_the_licence_tier() {
        let (tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        tx.send_modify(|s| s.tier = Some(arena_proto::Tier::Basic));
        assert_eq!(get_json(rx).await["tier"], "basic");
    }

    #[tokio::test]
    async fn reports_follower_without_lease_as_null() {
        let (_tx, rx) = status_channel(HubStatus::opened(1, "01JSELF"));
        assert_eq!(
            get_json(rx).await,
            json!({"db_open": true, "role": "follower", "lease": null, "epoch": null, "schema_version": 1, "self_instance_id": "01JSELF", "tls_fingerprint": null, "connectivity": {"state": "online", "checked_at": null}, "setup": null, "tier": null, "platform": "windows", "now": 1000})
        );
    }

    #[cfg(feature = "dev-fixtures")]
    #[tokio::test]
    async fn dev_revoke_needs_a_body_and_a_writer() {
        let app = dev_router(None, Arc::new(ManualClock::new(0)));
        for (body, status) in [
            ("nope", StatusCode::BAD_REQUEST),
            (r#"{"device_id":"D"}"#, StatusCode::SERVICE_UNAVAILABLE),
            (
                r#"{"device_id":"D","reason":"stolen"}"#,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            // An unknown reason is refused on `reason`, never taken as `other`.
            (
                r#"{"device_id":"D","reason":"bored"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
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

    mod ui {
        use super::*;
        use arena_store::{
            NewBranch, NewDevice, Store, insert_branch, insert_device, set_device_revoked,
        };
        use axum::http::header;

        /// A branch database: bootstrapped with "Dev Kiosk" and a revoked "Old Kiosk", or empty.
        fn reader(seeded: bool) -> (tempfile::TempDir, Arc<SnapshotReader>) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("branch.db");
            let mut store = Store::open(&path, 0).unwrap();
            if seeded {
                store
                    .write_tx(|tx| {
                        insert_branch(
                            tx,
                            &NewBranch {
                                branch_id: "B",
                                name: "Dev Branch",
                                branch_code: "DEV01",
                                currency: "INR",
                                tz: "Asia/Kolkata",
                                day_start_min: 360,
                                fy_start_month: 4,
                            },
                        )?;
                        for (id, label) in [("D1", "Dev Kiosk"), ("D2", "Old Kiosk")] {
                            insert_device(
                                tx,
                                &NewDevice {
                                    device_id: id,
                                    branch_id: "B",
                                    class: "paired",
                                    label,
                                    token_hash: "$argon2id$x",
                                },
                            )?;
                        }
                        set_device_revoked(tx, "D2", 7)
                    })
                    .unwrap();
            }
            let reader = Arc::new(SnapshotReader::open(&path).unwrap());
            (dir, reader)
        }

        async fn send(
            reader: Arc<SnapshotReader>,
            req: Request<Body>,
        ) -> (StatusCode, axum::http::HeaderMap, Value) {
            send_with(reader, req, |_| {}).await
        }

        async fn send_with(
            reader: Arc<SnapshotReader>,
            req: Request<Body>,
            tweak: fn(&mut HubStatus),
        ) -> (StatusCode, axum::http::HeaderMap, Value) {
            let clock = Arc::new(ManualClock::new(1_800_000_000_000));
            let (tx, rx) = status_channel(HubStatus::opened(1, "H"));
            tx.send_modify(|s| {
                s.connectivity = arena_proto::Connectivity {
                    state: arena_proto::ConnectivityState::CloudUnreachable,
                    checked_at: Some(7),
                };
                s.tls_fingerprint = Some("ab".repeat(32));
                s.lan_ports = Some((7400, 7401));
                s.lan_addresses = vec![std::net::Ipv4Addr::new(192, 168, 1, 20)];
                tweak(s);
            });
            let res = ui_router(reader, clock, crate::lan::Presence::none(), Some(rx), None)
                .oneshot(req)
                .await
                .unwrap();
            let status = res.status();
            let headers = res.headers().clone();
            let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
            let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            (status, headers, body)
        }

        fn get(origin: Option<&str>) -> Request<Body> {
            let mut b = Request::get("/ui/snapshot").header(header::HOST, "127.0.0.1:7409");
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            b.body(Body::empty()).unwrap()
        }

        #[tokio::test]
        async fn snapshot_has_the_branch_and_excludes_revoked_devices() {
            let (_dir, r) = reader(true);
            let (status, _, body) = send(r, get(None)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body,
                json!({
                    "branch": {"name": "Dev Branch", "branch_code": "DEV01"},
                    "groups": [], "zones": [], "sessions": [], "bills": [], "group_bills": [], "alerts": [],
                    "devices": [{"device_id": "D1", "label": "Dev Kiosk", "class": "paired", "zone": null, "online": false, "kind": null, "group_id": null, "meta_rev": null, "tv_number": null, "floor_x": null, "floor_y": null, "status": "offline", "last_seen_at": null, "maintenance_note": null}],
                    "locale": {
                        "currency": "INR",
                        "exponent": 2,
                        "symbol": "₹",
                        "symbol_position": "before",
                        "number_format": "indian",
                        "tz": "Asia/Kolkata",
                        "day_start_min": 360,
                        "fy_start_month": 4,
                        "pending": null
                    },
                    "ui": {"idle_lock_minutes": 5, "hc": false},
                    "connectivity": {"state": "cloud_unreachable", "checked_at": 7},
                    "setup": {"opening_hours_set": false, "backup_location_set": false, "ad_consent": "missing", "printer": "unset", "products_count": 0, "groups_without_rate": 0, "manager_exists": false, "test_session": "unset", "live": false},
                    "lan": {
                        "addresses": ["192.168.1.20"],
                        "lan_port": 7400,
                        "web_port": 7401,
                        "fingerprint": "ab".repeat(32),
                        "no_lan": false
                    },
                    "paired": true,
                    "device_cap": 20
                })
            );
        }

        #[tokio::test]
        async fn snapshot_without_a_branch_is_null() {
            let (_dir, r) = reader(false);
            let (status, _, body) = send(r, get(None)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body,
                json!({"branch": null, "groups": [], "zones": [], "sessions": [], "bills": [], "group_bills": [], "alerts": [], "devices": [], "locale": null, "ui": {"idle_lock_minutes": 5, "hc": false}, "connectivity": {"state": "cloud_unreachable", "checked_at": 7}, "setup": null, "lan": {"addresses": ["192.168.1.20"], "lan_port": 7400, "web_port": 7401, "fingerprint": "ab".repeat(32), "no_lan": false}, "paired": true, "device_cap": 20})
            );
        }

        #[tokio::test]
        async fn a_revoked_pairing_is_flagged_in_the_snapshot() {
            let (_dir, r) = reader(false);
            let (_, _, body) = send_with(r, get(None), |s| {
                s.pairing = arena_core::PairingState::Revoked;
            })
            .await;
            assert_eq!(body["pairing_revoked"], true);
            let (_dir, r) = reader(false);
            let (_, _, body) = send(r, get(None)).await;
            assert!(body.get("pairing_revoked").is_none());
        }

        #[tokio::test]
        async fn licence_and_clock_warnings_are_flagged_in_the_snapshot() {
            let (_dir, r) = reader(false);
            let (_, _, body) = send_with(r, get(None), |s| {
                s.licence = Some(arena_proto::LicenceStanding::Unverified);
                s.clock_wrong = true;
            })
            .await;
            assert_eq!(body["licence_unverified"], true);
            assert_eq!(body["clock_wrong"], true);
            let tweaks: [fn(&mut HubStatus); 3] = [
                |s| s.licence = Some(arena_proto::LicenceStanding::Verified),
                |s| s.licence = Some(arena_proto::LicenceStanding::Expired),
                |s| s.licence = None,
            ];
            for tweak in tweaks {
                let (_dir, r) = reader(false);
                let (_, _, body) = send_with(r, get(None), tweak).await;
                assert!(body.get("licence_unverified").is_none());
                assert!(body.get("clock_wrong").is_none());
            }
        }

        #[tokio::test]
        async fn the_snapshot_carries_the_effective_tier_for_the_ads_gate() {
            let (_dir, r) = reader(false);
            let (_, _, body) = send_with(r, get(None), |s| {
                s.tier = Some(arena_proto::Tier::Free);
            })
            .await;
            assert_eq!(body["tier"], "free");
            let (_dir, r) = reader(false);
            let (_, _, body) = send_with(r, get(None), |s| s.tier = None).await;
            assert!(body.get("tier").is_none());
        }

        #[tokio::test]
        async fn cors_allows_only_the_tauri_and_vite_origins() {
            let (_dir, r) = reader(true);
            for origin in UI_ALLOWED_ORIGINS {
                let (status, headers, _) = send(Arc::clone(&r), get(Some(origin))).await;
                assert_eq!(status, StatusCode::OK, "{origin}");
                assert_eq!(
                    headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                    origin,
                    "{origin}"
                );
                assert_eq!(headers[header::VARY], "Origin");
            }
            for origin in [
                "http://evil.example",
                "http://localhost:4173",
                "null",
                "tauri://localhost.evil",
            ] {
                let (status, headers, _) = send(Arc::clone(&r), get(Some(origin))).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
                assert!(
                    headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none(),
                    "{origin}"
                );
            }
            // No Origin (curl): served, without CORS headers.
            let (status, headers, _) = send(Arc::clone(&r), get(None)).await;
            assert_eq!(status, StatusCode::OK);
            assert!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        }

        #[tokio::test]
        async fn preflight_is_answered_for_allowed_origins() {
            let (_dir, r) = reader(true);
            let req = Request::options("/ui/snapshot")
                .header(header::HOST, "127.0.0.1:7409")
                .header(header::ORIGIN, "http://tauri.localhost")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap();
            let (status, headers, _) = send(r, req).await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            assert_eq!(
                headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                "http://tauri.localhost"
            );
            assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_METHODS], "GET, POST");
        }

        #[tokio::test]
        async fn a_missing_host_header_is_refused() {
            let (_dir, r) = reader(true);
            let req = Request::get("/ui/snapshot").body(Body::empty()).unwrap();
            let (status, _, _) = send(r, req).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }

        #[tokio::test]
        async fn a_failed_read_returns_only_the_code() {
            let (dir, r) = reader(true);
            // Drop the devices table under the reader: the next read fails.
            Store::open(dir.path().join("branch.db"), 0)
                .unwrap()
                .write_tx(|tx| {
                    tx.execute_batch("DROP TABLE devices")
                        .map_err(arena_store::db_error)
                })
                .unwrap();
            let (status, _, body) = send(r, get(None)).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
            assert_eq!(body, json!({"code": "INTERNAL"}));
        }

        #[tokio::test]
        async fn non_loopback_host_names_are_refused() {
            let (_dir, r) = reader(true);
            for (host, ok) in [
                ("127.0.0.1:7409", true),
                ("localhost:7409", true),
                ("[::1]:7409", true),
                ("127.0.0.1", true),
                ("evil.example:7409", false),
                ("evil.example", false),
                ("192.168.1.5:7409", false),
            ] {
                let req = Request::get("/ui/snapshot")
                    .header(header::HOST, host)
                    .body(Body::empty())
                    .unwrap();
                let (status, _, _) = send(Arc::clone(&r), req).await;
                let want = if ok {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                };
                assert_eq!(status, want, "{host}");
            }
        }
    }

    #[tokio::test]
    async fn binds_loopback_only() {
        let listener = bind_health(0).await.unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }
}

//! The audit viewer's read-only loopback routes over the `changes` log (Story 1.15), behind the
//! same origin guard as the other `/ui/*` routes:
//!
//! - `GET /ui/audit?actor=<kind[:id]>&module=<domain>&from=<ms>&to=<ms>&cursor=&limit=` answers
//!   an [`AuditPage`](arena_proto::AuditPage): newest first by `(at, epoch, seq)`, keyset-paged
//!   by the opaque `cursor` (never `OFFSET`), 1–100 items (50 by default), `next_cursor` `null`
//!   on the last page. Filters combine with AND; `from` is inclusive, `to` exclusive.
//! - `GET /ui/audit/item?epoch=&seq=` answers an [`AuditDetail`](arena_proto::AuditDetail):
//!   the change with its row image, the image of the previous change to the same row (`null`
//!   when it created the row) and the changed columns. Secret columns read `"[redacted]"`.
//! - `GET /ui/audit/facets` answers the filter options (staff names, known modules).
//!
//! Every route needs a bearer session that may act (`SESSION_LOCKED` while it is idle-locked)
//! and either holds `audit.view` or sends a read-elevation ticket for `audit.view`
//! (`POST /ui/elevate {items: [{permission: "audit.view"}], pin}`) in `X-Arena-Elevation`.
//! Anything else is `PERMISSION_DENIED` (403), and no audit data is read before the grant is
//! checked. Everything is read on the Hub's own read-only connection: nothing leaves the box.

use std::collections::HashMap;
use std::sync::Arc;

use arena_core::AuditQuery;
use arena_proto::{AppError, ELEVATION_HEADER, ErrorCode, Permission, UiError};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, internal, ui_error};

/// The refusal for a session without `audit.view` and without a valid read elevation. It never
/// says whether a ticket was sent.
pub const AUDIT_DENIED_MESSAGE: &str = "viewing the audit log needs a manager's approval";

fn denied() -> UiError {
    UiError {
        code: ErrorCode::PermissionDenied,
        message: AUDIT_DENIED_MESSAGE.into(),
        field: None,
        retry_after_s: None,
    }
}

/// Checks the read gate: a session that may act, holding `audit.view` or presenting a read
/// elevation for it.
async fn gate(auth: &UiAuth, headers: &HeaderMap) -> Result<(), UiError> {
    let token = bearer(headers).ok_or_else(auth_failed)?.to_owned();
    let me = auth.acting_session(&token).await?;
    if me.permissions.contains(&Permission::AuditView) {
        return Ok(());
    }
    let ticket = headers
        .get(ELEVATION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|t| !t.is_empty());
    match ticket {
        Some(t)
            if auth
                .read_granted(t, &token, &me.staff_id, Permission::AuditView)
                .await? =>
        {
            Ok(())
        }
        _ => {
            tracing::debug!(staff_id = %me.staff_id, "audit read refused");
            Err(denied())
        }
    }
}

fn params(
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<HashMap<String, String>, UiError> {
    query
        .map(|Query(q)| q)
        .map_err(|_| bad_frame("the query string is not valid"))
}

/// Runs a blocking read on the audit reader.
async fn read<R: Send + 'static>(
    auth: &UiAuth,
    f: impl FnOnce(&arena_core::AuditReader) -> Result<R, AppError> + Send + 'static,
) -> Result<R, UiError> {
    let reader = auth.audit_reader()?;
    tokio::task::spawn_blocking(move || f(&reader))
        .await
        .map_err(|_| internal())?
        .map_err(ui_error)
}

/// `GET /ui/audit`.
pub(crate) async fn list(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Response {
    if let Err(e) = gate(&auth, &headers).await {
        return fail(e);
    }
    let q = match params(query).and_then(|p| AuditQuery::parse(&p).map_err(ui_error)) {
        Ok(q) => q,
        Err(e) => return fail(e),
    };
    match read(&auth, move |r| r.page(&q)).await {
        Ok(page) => Json(page).into_response(),
        Err(e) => fail(e),
    }
}

/// `GET /ui/audit/item`.
pub(crate) async fn item(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Response {
    if let Err(e) = gate(&auth, &headers).await {
        return fail(e);
    }
    let p = match params(query) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    let invalid = |field: &str, message: &str| {
        fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            field,
            message,
        )))
    };
    let Some(epoch) = p
        .get("epoch")
        .map(|e| e.trim().to_owned())
        .filter(|e| !e.is_empty())
    else {
        return invalid("epoch", "name the change's epoch");
    };
    let Some(seq) = p.get("seq").and_then(|s| s.trim().parse::<i64>().ok()) else {
        return invalid("seq", "name the change's seq");
    };
    match read(&auth, move |r| r.detail(&epoch, seq)).await {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => invalid("seq", "no change has this epoch and seq"),
        Err(e) => fail(e),
    }
}

/// `GET /ui/audit/facets`.
pub(crate) async fn facets(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = gate(&auth, &headers).await {
        return fail(e);
    }
    match read(&auth, |r| r.facets()).await {
        Ok(f) => Json(f).into_response(),
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use arena_core::Clock;
    use arena_proto::{Actor, ActorKind, IntentPayload, SettingsSet};
    use arena_store::{ChangeOp, ChangeRecord, Store, insert_change};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::ui_auth::tests::{Hub, call, hub, intent_as, token_for};

    /// `GET uri` with a bearer token and, optionally, a read-elevation ticket.
    async fn get(hub: &Hub, uri: &str, token: &str, ticket: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::HOST, "127.0.0.1:7409")
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(t) = ticket {
            req = req.header("X-Arena-Elevation", t);
        }
        let res = hub
            .app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), 4 * 1024 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn rev_of(hub: &Hub, key: &str) -> i64 {
        Store::open_reader(&hub.path)
            .unwrap()
            .settings()
            .unwrap()
            .into_iter()
            .find(|s| s.key == key)
            .unwrap()
            .rev
    }

    async fn elevate(hub: &Hub, token: &str, items: Value, pin: &str) -> (StatusCode, Value) {
        call(
            hub,
            "POST",
            "/ui/elevate",
            Some(token),
            Some(json!({"items": items, "pin": pin})),
        )
        .await
    }

    /// A read elevation of `audit.view` approved with the manager's PIN.
    async fn read_ticket(hub: &Hub, token: &str) -> String {
        let (status, body) =
            elevate(hub, token, json!([{"permission": "audit.view"}]), "2222").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["expires_in_s"], 600);
        body["ticket"].as_str().unwrap().to_owned()
    }

    /// Sets `key` to `value` as the session's member, approved by the manager's PIN when
    /// `elevated`.
    async fn set(hub: &Hub, token: &str, key: &str, value: &str, elevated: bool) {
        let mut body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::SettingsSet(SettingsSet { key: key.into(), value: value.into() }),
            Some(rev_of(hub, key)),
        ) });
        if elevated {
            let (status, ok) = elevate(
                hub,
                token,
                json!([{"intent_kind": "settings.set", "key": key}]),
                "2222",
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{ok}");
            body["elevation_ticket"] = ok["ticket"].clone();
        }
        let (status, reply) = call(hub, "POST", "/ui/intent", Some(token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
    }

    fn items(page: &Value) -> &Vec<Value> {
        page["items"].as_array().unwrap()
    }

    fn order_key(item: &Value) -> (i64, String, i64) {
        (
            item["at"].as_i64().unwrap(),
            item["epoch"].as_str().unwrap().to_owned(),
            item["seq"].as_i64().unwrap(),
        )
    }

    #[tokio::test]
    async fn a_stale_offline_admin_exit_is_flagged_in_the_list_and_summarised_in_the_detail() {
        let hub = hub().await;
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let kiosk = hub
            .writer
            .submit_intent(intent_as(
                owner.clone(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister {
                    device_id: None,
                    device_kind: Default::default(),
                    class: arena_proto::ClientClass::Paired,
                    label: "Kiosk 3".into(),
                    token_hash: arena_core::devices::hash_token_secret("k").unwrap(),
                    tv_number: None,
                    group_id: None,
                }),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let dir = arena_core::DeviceDirectory::open(&hub.path).unwrap();
        let old = dir.manager_verifier().unwrap().version;
        // The Hub moves to a newer verifier; the kiosk still used the old one.
        hub.clock.advance(60_000);
        let mut add = intent_as(
            owner,
            IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                staff_id: None,
                login: "manager2".into(),
                display_name: "Manager 2".into(),
                role_id: hub.manager_role.clone(),
                password_hash: Some(arena_core::staff::hash_secret("pw").unwrap()),
                pin_hash: Some(arena_core::staff::hash_secret("6666").unwrap()),
                active: true,
            }),
            None,
        );
        add.issued_at = hub.clock.now_ms();
        hub.writer.submit_intent(add).await.unwrap();
        assert!(dir.manager_verifier().unwrap().version > old);
        let exit_id = ulid::Ulid::generate().to_string();
        let mut exit = intent_as(
            Actor {
                kind: ActorKind::Device,
                id: kiosk.clone(),
            },
            IntentPayload::DeviceAdminExit(arena_proto::DeviceAdminExit {
                exit_id: exit_id.clone(),
                mode: arena_proto::AdminExitMode::CachedVerifier,
            }),
            None,
        );
        exit.issued_at = hub.clock.now_ms();
        exit.elevated_by = Some(arena_proto::ElevatedBy {
            kind: arena_proto::ElevationKind::Pin,
            id: hub.manager_id.clone(),
            mode: arena_proto::ElevationMode::CachedVerifier,
            verifier_version: old,
        });
        hub.writer.submit_intent(exit).await.unwrap();

        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, page) = get(&hub, "/ui/audit?module=device", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let item = items(&page)
            .iter()
            .find(|i| i["tbl"] == "device_admin_exits")
            .unwrap()
            .clone();
        assert_eq!(item["flagged"], true);
        assert_eq!(item["row_id"], exit_id.as_str());
        let others = items(&page)
            .iter()
            .filter(|i| i["tbl"] != "device_admin_exits");
        for other in others {
            assert!(other.get("flagged").is_none(), "{other}");
        }
        let uri = format!(
            "/ui/audit/item?epoch={}&seq={}",
            item["epoch"].as_str().unwrap(),
            item["seq"]
        );
        let (status, detail) = get(&hub, &uri, &token, None).await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        let e = &detail["admin_exit"];
        assert_eq!(e["device_label"], "Kiosk 3");
        assert_eq!(e["staff_name"], "Manager");
        assert_eq!(e["mode"], "cached_verifier");
        assert_eq!(e["flagged"], true);
        assert_eq!(e["flag_reason"], "stale_verifier");
        assert_eq!(detail["item"]["flagged"], true);
    }

    #[tokio::test]
    async fn a_manager_pages_the_whole_log_newest_first_without_overlap_or_gap() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, first) = get(&hub, "/ui/audit", &token, None).await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert_eq!(items(&first).len(), 50);
        assert!(first["next_cursor"].is_string());
        let mut all: Vec<Value> = Vec::new();
        let mut uri = "/ui/audit".to_owned();
        loop {
            let (status, page) = get(&hub, &uri, &token, None).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            all.extend(items(&page).iter().cloned());
            match page["next_cursor"].as_str() {
                Some(c) => uri = format!("/ui/audit?cursor={c}"),
                None => break,
            }
        }
        let keys: Vec<_> = all.iter().map(order_key).collect();
        let mut sorted = keys.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        sorted.dedup();
        assert_eq!(keys, sorted, "newest first, each once");
        let total = Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .len();
        assert_eq!(all.len(), total, "no gap");
        // The bootstrap is the system's, staff were added by the owner: fixed catalogue keys.
        let actor_names: std::collections::HashSet<(&str, &str)> = all
            .iter()
            .map(|i| {
                (
                    i["actor"]["kind"].as_str().unwrap(),
                    i["actor"]["name"].as_str().unwrap(),
                )
            })
            .collect();
        assert!(actor_names.contains(&("system", "hub.audit.actor.system")));
        assert!(actor_names.contains(&("owner", "hub.audit.actor.owner")));
        let branch = all.iter().find(|i| i["tbl"] == "branch").unwrap();
        assert_eq!(branch["module"], "branch");
        assert_eq!(branch["op"], "insert");
        assert!(branch["elevated_by"].is_null());
        // A limit within 1–100 is honoured; outside it is refused.
        let (_, small) = get(&hub, "/ui/audit?limit=3", &token, None).await;
        assert_eq!(items(&small).len(), 3);
        let (status, bad) = get(&hub, "/ui/audit?limit=101", &token, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(bad["field"], "limit");
    }

    #[tokio::test]
    async fn filters_combine_and_an_elevated_change_names_its_approver() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        hub.clock.advance(60_000);
        let before = hub.clock.now_ms();
        set(&hub, &counter, "branch.currency", "USD", true).await;
        hub.clock.advance(60_000);
        set(&hub, &manager, "receipt.footer", "Thanks", false).await;
        hub.clock.advance(60_000);
        set(&hub, &counter, "receipt.footer", "Later", true).await;
        let uri = format!(
            "/ui/audit?actor=staff:{}&module=settings&from={before}&to={}",
            hub.counter_id,
            before + 60_000
        );
        let (status, page) = get(&hub, &uri, &manager, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let found = items(&page);
        assert_eq!(found.len(), 1, "{page}");
        let item = &found[0];
        assert_eq!(item["intent_kind"], "settings.set");
        assert!(
            item["row_id"]
                .as_str()
                .unwrap()
                .ends_with("/branch.currency")
        );
        assert_eq!(item["actor"]["name"], "Counter");
        assert_eq!(item["actor"]["id"], hub.counter_id.as_str());
        assert_eq!(
            item["elevated_by"],
            json!({"kind": "pin", "id": hub.manager_id, "name": "Manager", "mode": "hub"})
        );
        assert!(page["next_cursor"].is_null());
        // Without the date range both of the counter's settings changes show.
        let uri = format!("/ui/audit?actor=staff:{}&module=settings", hub.counter_id);
        assert_eq!(items(&get(&hub, &uri, &manager, None).await.1).len(), 2);
        // A module nobody touched: an empty page.
        let (status, empty) = get(&hub, "/ui/audit?module=console", &manager, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(empty, json!({"items": [], "next_cursor": null}));
        let (status, bad) = get(&hub, "/ui/audit?actor=robot", &manager, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(bad["field"], "actor");
    }

    #[tokio::test]
    async fn stopping_the_hub_is_audited_with_the_counter_and_the_approving_manager() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let body = json!({ "intent": intent_as(
            Actor { kind: ActorKind::Staff, id: "x".into() },
            IntentPayload::SettingsSet(SettingsSet {
                key: "hub.stopped_at".into(),
                value: "1730000000000".into(),
            }),
            Some(rev_of(&hub, "hub.stopped_at")),
        ) });
        let (status, refused) = call(&hub, "POST", "/ui/intent", Some(&counter), Some(body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
        set(&hub, &counter, "hub.stopped_at", "1730000000000", true).await;
        let (status, page) = get(&hub, "/ui/audit?module=settings", &manager, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let stop = items(&page)
            .iter()
            .find(|i| i["row_id"].as_str().unwrap().ends_with("/hub.stopped_at"))
            .unwrap();
        assert_eq!(stop["actor"]["name"], "Counter");
        assert_eq!(
            stop["elevated_by"],
            json!({"kind": "pin", "id": hub.manager_id, "name": "Manager", "mode": "hub"})
        );
    }

    #[tokio::test]
    async fn detail_shows_before_and_after_and_redacts_secrets() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        hub.clock.advance(1_000);
        set(&hub, &manager, "branch.currency", "USD", false).await;
        let changes = Store::open_reader(&hub.path).unwrap().changes().unwrap();
        let detail = |c: &ChangeRecord| format!("/ui/audit/item?epoch={}&seq={}", c.epoch, c.seq);

        // An update: the previous image and the changed columns.
        let update = changes
            .iter()
            .rfind(|c| c.row_id.ends_with("/branch.currency"))
            .unwrap();
        let (status, d) = get(&hub, &detail(update), &manager, None).await;
        assert_eq!(status, StatusCode::OK, "{d}");
        assert_eq!(d["before"]["value"], "INR");
        assert_eq!(d["after"]["value"], "USD");
        let mut changed: Vec<&str> = d["changed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        changed.sort_unstable();
        assert_eq!(changed, vec!["rev", "updated_at", "value"]);
        assert_eq!(d["item"]["op"], "update");

        // A create: no before image.
        let branch = changes.iter().find(|c| c.tbl == "branch").unwrap();
        let (_, d) = get(&hub, &detail(branch), &manager, None).await;
        assert!(d["before"].is_null(), "{d}");
        assert_eq!(d["after"]["name"], "Dev Branch");
        assert_eq!(d["changed"], json!([]));

        // A staff row: hashes never leave the hub.
        let staff = changes.iter().find(|c| c.tbl == "staff").unwrap();
        let (status, d) = get(&hub, &detail(staff), &manager, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(d["after"]["password_hash"], "[redacted]");
        assert_eq!(d["after"]["pin_hash"], "[redacted]");
        assert!(!d.to_string().contains("$argon2"), "{d}");

        let (status, missing) = get(&hub, "/ui/audit/item?epoch=9:x&seq=1", &manager, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(missing["field"], "seq");
        let (_, missing) = get(&hub, "/ui/audit/item?seq=1", &manager, None).await;
        assert_eq!(missing["field"], "epoch");
    }

    fn staff_rev(hub: &Hub, id: &str) -> i64 {
        Store::open_reader(&hub.path)
            .unwrap()
            .rows(arena_store::Table::Staff)
            .unwrap()
            .into_iter()
            .find(|r| r.get("staff_id") == Some(&arena_store::RowValue::Text(id.to_owned())))
            .and_then(|r| match r.get("rev") {
                Some(arena_store::RowValue::Integer(n)) => Some(*n),
                _ => None,
            })
            .unwrap()
    }

    /// The owner edits the manager: `role_id`, `active` and a new PIN as given.
    async fn edit_manager(hub: &Hub, role_id: &str, active: bool, pin: Option<&str>) {
        let rev = staff_rev(hub, &hub.manager_id);
        hub.writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: Some(hub.manager_id.clone()),
                    login: "manager".into(),
                    display_name: "Manager".into(),
                    role_id: role_id.into(),
                    password_hash: None,
                    pin_hash: pin.map(|p| arena_core::staff::hash_secret(p).unwrap()),
                    active,
                }),
                Some(rev),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_read_approval_ends_when_its_approver_no_longer_holds_it() {
        let manager_role = |hub: &Hub| hub.manager_role.clone();
        for case in ["deactivated", "pin rotated", "role changed"] {
            let hub = hub().await;
            let counter = token_for(&hub, "counter", "c-pass").await;
            let ticket = read_ticket(&hub, &counter).await;
            assert_eq!(
                get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
                StatusCode::OK
            );
            hub.clock.advance(60_000);
            match case {
                "deactivated" => edit_manager(&hub, &manager_role(&hub), false, None).await,
                "pin rotated" => edit_manager(&hub, &manager_role(&hub), true, Some("2468")).await,
                _ => edit_manager(&hub, &hub.counter_role.clone(), true, None).await,
            }
            let (status, body) = get(&hub, "/ui/audit", &counter, Some(&ticket)).await;
            assert_eq!(
                (status, body["code"].as_str()),
                (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED")),
                "{case}"
            );
            // The ticket is gone, even if the approver is restored.
            if case == "role changed" {
                edit_manager(&hub, &manager_role(&hub), true, None).await;
                assert_eq!(
                    get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
                    StatusCode::FORBIDDEN
                );
            }
        }
    }

    #[tokio::test]
    async fn a_read_grant_is_recorded_in_the_change_log() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let ticket = read_ticket(&hub, &counter).await;
        let (_, page) = get(&hub, "/ui/audit?module=audit", &counter, Some(&ticket)).await;
        let found = items(&page);
        assert_eq!(found.len(), 1, "{page}");
        assert_eq!(found[0]["intent_kind"], "audit.read_granted");
        assert_eq!(found[0]["tbl"], "read_grants");
        assert_eq!(found[0]["actor"]["kind"], "system");
        let uri = format!(
            "/ui/audit/item?epoch={}&seq={}",
            found[0]["epoch"].as_str().unwrap(),
            found[0]["seq"]
        );
        let (status, d) = get(&hub, &uri, &counter, Some(&ticket)).await;
        assert_eq!(status, StatusCode::OK, "{d}");
        assert_eq!(d["after"]["permission"], "audit.view");
        assert_eq!(d["after"]["requester_staff_id"], hub.counter_id.as_str());
        assert_eq!(d["after"]["approver_staff_id"], hub.manager_id.as_str());
        assert_eq!(d["after"]["verifier_version"], 1);
        assert!(d["before"].is_null());
        // A refused PIN records nothing.
        let (status, _) = elevate(
            &hub,
            &counter,
            json!([{"permission": "audit.view"}]),
            "9999",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let grants = Store::open_reader(&hub.path)
            .unwrap()
            .rows(arena_store::Table::ReadGrants)
            .unwrap();
        assert_eq!(grants.len(), 1);
    }

    #[tokio::test]
    async fn a_redacted_column_that_changed_is_listed_but_never_shown() {
        let hub = hub().await;
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let rev = Store::open_reader(&hub.path)
            .unwrap()
            .rows(arena_store::Table::Staff)
            .unwrap()
            .into_iter()
            .find(|r| {
                r.get("staff_id") == Some(&arena_store::RowValue::Text(hub.counter_id.clone()))
            })
            .and_then(|r| match r.get("rev") {
                Some(arena_store::RowValue::Integer(n)) => Some(*n),
                _ => None,
            })
            .unwrap();
        hub.writer
            .submit_intent(intent_as(
                owner,
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: Some(hub.counter_id.clone()),
                    login: "counter".into(),
                    display_name: "Counter".into(),
                    role_id: hub.counter_role.clone(),
                    password_hash: None,
                    pin_hash: Some(arena_core::staff::hash_secret("5555").unwrap()),
                    active: true,
                }),
                Some(rev),
            ))
            .await
            .unwrap();
        let manager = token_for(&hub, "manager", "m-pass").await;
        let last = Store::open_reader(&hub.path)
            .unwrap()
            .changes()
            .unwrap()
            .into_iter()
            .rfind(|c| c.tbl == "staff")
            .unwrap();
        let uri = format!("/ui/audit/item?epoch={}&seq={}", last.epoch, last.seq);
        let (status, d) = get(&hub, &uri, &manager, None).await;
        assert_eq!(status, StatusCode::OK, "{d}");
        assert!(
            d["changed"]
                .as_array()
                .unwrap()
                .contains(&json!("pin_hash")),
            "{d}"
        );
        assert_eq!(d["before"]["pin_hash"], "[redacted]");
        assert_eq!(d["after"]["pin_hash"], "[redacted]");
        assert!(!d.to_string().contains("$argon2"));
    }

    #[tokio::test]
    async fn a_counter_without_audit_view_sees_nothing_until_a_manager_approves() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        for uri in [
            "/ui/audit",
            "/ui/audit/item?epoch=1:x&seq=1",
            "/ui/audit/facets",
        ] {
            let (status, body) = get(&hub, uri, &counter, None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
            assert_eq!(body["code"], "PERMISSION_DENIED");
            assert!(body.get("items").is_none());
            let (status, _) = get(&hub, uri, &counter, Some("made-up")).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: a made-up ticket");
        }
        // Wrong PIN, a PIN without audit.view, and the counter's own: one generic refusal.
        for pin in ["9999", "4444", "3333"] {
            let (status, body) =
                elevate(&hub, &counter, json!([{"permission": "audit.view"}]), pin).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{pin}");
            assert_eq!(body["code"], "PIN_REJECTED");
        }
        let ticket = read_ticket(&hub, &counter).await;
        let (status, page) = get(&hub, "/ui/audit", &counter, Some(&ticket)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(items(&page).len(), 50);
        // Unlimited reads within 10 minutes.
        for _ in 0..3 {
            assert_eq!(
                get(&hub, "/ui/audit/facets", &counter, Some(&ticket))
                    .await
                    .0,
                StatusCode::OK
            );
        }
        let (_, facets) = get(&hub, "/ui/audit/facets", &counter, Some(&ticket)).await;
        assert!(
            facets["staff"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["name"] == "Manager")
        );
        assert!(
            facets["modules"]
                .as_array()
                .unwrap()
                .contains(&json!("settings"))
        );
        hub.clock.advance(9 * 60_000 + 59_000);
        assert_eq!(
            get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
            StatusCode::OK
        );
        hub.clock.advance(1_000);
        let (status, body) = get(&hub, "/ui/audit", &counter, Some(&ticket)).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
    }

    #[tokio::test]
    async fn a_read_ticket_ends_with_a_lock_or_a_sign_out_and_never_travels() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let ticket = read_ticket(&hub, &counter).await;
        assert_eq!(
            get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
            StatusCode::OK
        );
        // Another session of the same member cannot use it.
        let other = token_for(&hub, "counter", "c-pass").await;
        assert_eq!(
            get(&hub, "/ui/audit", &other, Some(&ticket)).await.0,
            StatusCode::FORBIDDEN
        );
        // It never redeems an intent.
        let body = json!({
            "intent": intent_as(
                Actor { kind: ActorKind::Staff, id: "x".into() },
                IntentPayload::SettingsSet(SettingsSet { key: "receipt.footer".into(), value: "x".into() }),
                Some(rev_of(&hub, "receipt.footer")),
            ),
            "elevation_ticket": ticket,
        });
        let (_, reply) = call(&hub, "POST", "/ui/intent", Some(&counter), Some(body)).await;
        assert_eq!(reply["code"], "PIN_REQUIRED");
        // Locked: the screen is covered and the audit refuses; unlocked: the approval is gone.
        let (status, _) = call(&hub, "POST", "/ui/lock", Some(&counter), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = get(&hub, "/ui/audit", &counter, Some(&ticket)).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::LOCKED, Some("SESSION_LOCKED"))
        );
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/unlock",
            Some(&counter),
            Some(json!({"pin": "3333"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
            StatusCode::FORBIDDEN
        );
        // Signing out drops a fresh one; a new session cannot use it either.
        let ticket = read_ticket(&hub, &counter).await;
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some(&counter), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            get(&hub, "/ui/audit", &counter, Some(&ticket)).await.0,
            StatusCode::UNAUTHORIZED
        );
        let again = token_for(&hub, "counter", "c-pass").await;
        assert_eq!(
            get(&hub, "/ui/audit", &again, Some(&ticket)).await.0,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn read_elevation_takes_one_read_permission_on_its_own() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        for items in [
            json!([{"permission": "settings.edit"}]),
            json!([{"permission": "audit.view"}, {"permission": "report.view"}]),
            json!([{"permission": "audit.view"}, {"intent_kind": "settings.set", "key": "k"}]),
            json!([{"permission": "audit.view", "intent_kind": "settings.set"}]),
            json!([{"permission": "audit.view", "key": "k"}]),
            json!([{}]),
        ] {
            let (status, body) = elevate(&hub, &counter, items.clone(), "2222").await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{items}: {body}");
            assert_eq!(body["field"], "items");
        }
        // A manager already holds audit.view.
        let (status, body) = elevate(
            &hub,
            &manager,
            json!([{"permission": "audit.view"}]),
            "3333",
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        // report.view is a read permission too (the counter lacks it).
        let (status, body) = elevate(
            &hub,
            &counter,
            json!([{"permission": "report.view"}]),
            "2222",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // A report.view ticket does not open the audit.
        let t = body["ticket"].as_str().unwrap();
        assert_eq!(
            get(&hub, "/ui/audit", &counter, Some(t)).await.0,
            StatusCode::FORBIDDEN
        );
    }

    /// 100,000 change rows across 5 actors and 3 modules: the filtered and unfiltered first
    /// pages each answer within 250 ms in a debug build (a proxy for < 1 s on the reference
    /// hardware, checked by the release benchmark).
    #[tokio::test]
    async fn the_first_page_of_100k_changes_is_fast() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let base = hub.clock.now_ms();
        let actors: [(&str, &str); 5] = [
            ("staff", hub.manager_id.as_str()),
            ("staff", hub.counter_id.as_str()),
            ("staff", "01JOTHER00000000000000000"),
            ("system", "hub"),
            ("device", "01JDEVICE0000000000000000"),
        ];
        let kinds = ["settings.set", "staff.upsert", "device.register"];
        {
            let mut store = Store::open(&hub.path, base).unwrap();
            let started = Instant::now();
            store
                .write_tx(|tx| {
                    for i in 0..100_000i64 {
                        let (kind, id) = actors[(i % 5) as usize];
                        insert_change(
                            tx,
                            &ChangeRecord {
                                epoch: "2:PERF".into(),
                                seq: i + 1,
                                tbl: "settings".into(),
                                row_id: format!("B/k{}", i % 97),
                                op: ChangeOp::Update,
                                row: vec![0x80],
                                intent_id: format!("I{i}"),
                                intent_kind: kinds[(i % 3) as usize].into(),
                                actor_kind: kind.into(),
                                actor_id: id.into(),
                                elevated_by_kind: None,
                                elevated_by_id: None,
                                elevated_by_mode: None,
                                elevated_by_verifier_version: None,
                                // About 28 hours of changes, one a second.
                                at: base + i * 1_000,
                                business_day: None,
                                fy: None,
                                reason: None,
                            },
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            eprintln!("inserted 100k changes in {:?}", started.elapsed());
        }
        // Open the reader and warm the page cache once (not part of a timed page).
        assert_eq!(
            get(&hub, "/ui/audit/facets", &manager, None).await.0,
            StatusCode::OK
        );
        assert_eq!(
            get(&hub, "/ui/audit?limit=1", &manager, None).await.0,
            StatusCode::OK
        );
        let budget_ms: u128 = std::env::var("AUDIT_PERF_BUDGET_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(250);
        let from = base + 20_000_000;
        let to = base + 60_000_000;
        for uri in [
            "/ui/audit".to_owned(),
            format!(
                "/ui/audit?actor=staff:{}&from={from}&to={to}",
                hub.counter_id
            ),
            format!(
                "/ui/audit?actor=staff:{}&module=settings&from={from}&to={to}",
                hub.counter_id
            ),
            "/ui/audit?module=device".to_owned(),
            "/ui/audit?actor=system".to_owned(),
        ] {
            let started = Instant::now();
            let (status, page) = get(&hub, &uri, &manager, None).await;
            let took = started.elapsed();
            assert_eq!(status, StatusCode::OK, "{page}");
            assert_eq!(items(&page).len(), 50, "{uri}");
            assert!(page["next_cursor"].is_string());
            eprintln!("{uri}: {took:?}");
            assert!(
                took.as_millis() < budget_ms,
                "{uri} took {took:?} (budget {budget_ms} ms)"
            );
        }
    }
}

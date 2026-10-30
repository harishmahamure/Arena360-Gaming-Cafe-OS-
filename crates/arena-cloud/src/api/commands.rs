use std::sync::Arc;

use arena_proto::{
    AppError, BRANCH_ONLINE_WINDOW_MS, COMMAND_PAYLOAD_MAX, COMMAND_SCHEMA_VERSION, COMMANDS_PAGE,
    CommandResult, CommandResultAck, CommandStatus, CommandWire, CommandsPage, ErrorCode,
    OWNER_ALL_BRANCHES, OWNER_COMMAND_POSTS_PER_MIN, OWNER_COMMANDS_PAGE, OWNER_COMMANDS_PAGE_MAX,
    OwnerCommandKind, OwnerCommandList, OwnerCommandPost, OwnerCommandPosted, OwnerCommandPreview,
    OwnerCommandPreviewRequest, OwnerCommandSkip, OwnerCommandTarget, OwnerCommandView,
    OwnerStaffUpsert, PRODUCT_COMMAND_PAYLOAD_MAX, SETTINGS_COMMAND_KEYS, SETUP_CODE_HUB_TTL_MS,
    STEPUP_TTL_MS, StaffSetupCode, Tier, command_payload_max, is_setup_command, owner_command_kind,
    validate_owner_payload,
};
use arena_store::control::{
    Branch, CommandRow, NewCommand, OwnerCommandFilter, OwnerCommandRow, RateRule,
};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::handler::Handler;
use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::account::blocking;
use super::branches::tier_of;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::pair::check_field;
use super::signup::ceil_s;

/// The body of an owner command post: a product image rides in the payload (Story 8.24).
const OWNER_POST_BODY_LIMIT: usize = PRODUCT_COMMAND_PAYLOAD_MAX + 16 * 1024;
use crate::auth::{AuthState, valid_phone, valid_text};
use crate::pairing_code::new_pairing_code;
use crate::passwords::hash_secret;
use crate::presign::{Epoch, is_fenced};

const POST_RATE_PREFIX: &str = "cmdpost:";
const POST_WINDOW_MS: i64 = 60 * 1000;
const POST_BUCKET_MS: i64 = 1000;

pub async fn submit_command(
    st: &AuthState,
    branch_id: &str,
    idempotency_key: &str,
    kind: &str,
    payload: &serde_json::Value,
    issued_by: &str,
) -> Result<CommandRow, ApiError> {
    if !is_setup_command(kind) {
        return Err(AppError::with_field(
            ErrorCode::Unsupported,
            "kind",
            "This change can't be sent to the branch yet.",
        )
        .into());
    }
    if ulid::Ulid::from_string(idempotency_key).is_err() {
        return Err(ApiError::validation(
            "idempotency_key",
            "The key must be a ULID.",
        ));
    }
    let Some(fields) = payload.as_object() else {
        return Err(ApiError::validation(
            "payload",
            "Send the payload as an object.",
        ));
    };
    if kind == "settings.set"
        && !fields
            .get("key")
            .and_then(|k| k.as_str())
            .is_some_and(|k| SETTINGS_COMMAND_KEYS.contains(&k))
    {
        return Err(ApiError::validation(
            "key",
            "Only the branch name and timezone can be set from here.",
        ));
    }
    let payload = serde_json::to_string(payload).map_err(ApiError::internal)?;
    if payload.len() > COMMAND_PAYLOAD_MAX {
        return Err(ApiError::validation("payload", "The payload is too large."));
    }
    st.db
        .submit_command(&NewCommand {
            branch_id,
            idempotency_key,
            kind,
            schema_version: COMMAND_SCHEMA_VERSION,
            payload: &payload,
            issued_by,
            group_id: None,
            elevated_by: None,
            now: st.now(),
        })
        .await
        .map_err(ApiError::internal)
}

#[derive(Debug, Deserialize)]
struct FetchQuery {
    #[serde(default)]
    after: i64,
    epoch: String,
}

fn wire(row: CommandRow) -> Result<CommandWire, ApiError> {
    Ok(CommandWire {
        id: row.id,
        idempotency_key: row.idempotency_key,
        kind: row.kind,
        schema_version: row.schema_version,
        payload: serde_json::from_str(&row.payload).map_err(ApiError::internal)?,
        issued_by: row.issued_by,
        elevated_by: row.elevated_by,
    })
}

/// `FENCED` (naming the current fence) when `epoch` is below the branch fence: a newer Hub has
/// taken over, so this one neither fetches nor answers commands (Story 8.20).
async fn refuse_fenced(st: &AuthState, branch_id: &str, epoch: &str) -> Result<(), ApiError> {
    let Ok(parsed) = Epoch::parse(epoch) else {
        return Err(ApiError::validation(
            "epoch",
            "Send an epoch like 3:HUBINSTANCE.",
        ));
    };
    let fence = st
        .db
        .branch_fence_epoch(branch_id)
        .await
        .map_err(ApiError::internal)?;
    match fence {
        Some(fence) if is_fenced(&parsed, Some(&fence)) => {
            tracing::warn!(branch_id, epoch, fence = %fence, "commands: fenced Hub refused");
            Err(AppError::new(
                ErrorCode::Fenced,
                format!("A newer Hub has taken over this branch (epoch {fence})."),
            )
            .into())
        }
        _ => Ok(()),
    }
}

async fn fetch(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Query(q): Query<FetchQuery>,
) -> Result<Json<CommandsPage>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &q.epoch)?;
    refuse_fenced(&st, &hub.branch_id, &q.epoch).await?;
    let rows = st
        .db
        .fetch_commands(&hub.branch_id, q.after.max(0), COMMANDS_PAGE, now)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(CommandsPage {
        commands: rows.into_iter().map(wire).collect::<Result<_, _>>()?,
    }))
}

async fn result(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    ApiJson(body): ApiJson<CommandResult>,
) -> Result<Json<CommandResultAck>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    check_field("epoch", &body.epoch)?;
    if let Err(fenced) = refuse_fenced(&st, &hub.branch_id, &body.epoch).await {
        st.db
            .requeue_fetched_command(&hub.branch_id, id)
            .await
            .map_err(ApiError::internal)?;
        return Err(fenced);
    }
    let error = body
        .error
        .as_ref()
        .map(|e| serde_json::to_string(e).map_err(ApiError::internal))
        .transpose()?;
    let status = st
        .db
        .resolve_command(
            &hub.branch_id,
            id,
            body.applied_seq,
            error.as_deref(),
            &body.epoch,
            now,
        )
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if status == CommandStatus::Applied {
        st.db
            .finish_staff_step(&hub.branch_id, id, now)
            .await
            .map_err(ApiError::internal)?;
    }
    let _ = st.wizard_tx.send(hub.branch_id.clone());
    Ok(Json(CommandResultAck { status }))
}

fn rank(t: Tier) -> u8 {
    match t {
        Tier::Free => 0,
        Tier::Basic => 1,
        Tier::Premium => 2,
    }
}

/// Why a branch cannot be sent a kind, or `None` when it can: the tier and flag it needs and the
/// command schema its Hub last reported (a Hub that never reported one runs version 1).
async fn refusal(
    st: &AuthState,
    b: &Branch,
    kind: &OwnerCommandKind,
) -> Result<Option<AppError>, ApiError> {
    let now = st.now();
    let tier = tier_of(st, &b.id).await?;
    let entitled = rank(tier) >= rank(kind.min_tier);
    let flagged = match kind.flag {
        None => true,
        Some(flag) => {
            let entitlement = st.db.entitlement(&b.id).await.map_err(ApiError::internal)?;
            crate::licence::claims_for(&b.tenant_id, &b.id, entitlement.as_ref(), now)
                .flags
                .iter()
                .any(|f| f == flag)
        }
    };
    if !(entitled && flagged) {
        return Ok(Some(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "This change is available on Basic.",
        )));
    }
    let reported = st
        .db
        .branch_command_schema(&b.id)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or(1);
    if reported < kind.schema_version {
        return Ok(Some(AppError::new(
            ErrorCode::BranchUpdateRequired,
            "Update the branch's Hub to send this change.",
        )));
    }
    Ok(None)
}

/// Whether `stepup_at` is a proof still good at `now`.
pub fn stepup_fresh(stepup_at: Option<i64>, now: i64) -> bool {
    stepup_at.is_some_and(|at| (0..STEPUP_TTL_MS).contains(&(now - at)))
}

/// The step-up gate (Story 8.25): a sensitive kind needs a proof younger than [`STEPUP_TTL_MS`] on the
/// session, else `MFA_REQUIRED` and nothing is queued. `true` when the post is elevated.
fn step_up_check(
    kind: &OwnerCommandKind,
    stepup_at: Option<i64>,
    now: i64,
) -> Result<bool, ApiError> {
    if !kind.sensitive {
        return Ok(false);
    }
    if stepup_fresh(stepup_at, now) {
        return Ok(true);
    }
    Err(AppError::new(
        ErrorCode::MfaRequired,
        "Confirm it's you with a code to send this change.",
    )
    .into())
}

/// The row key of one branch in an "all branches" post: a ULID taken from the owner's key and the
/// branch, so the same post finds the same rows and a ULID is what the Hub accepts.
fn branch_key(group_key: &str, branch_id: &str) -> String {
    let digest = Sha256::new()
        .chain_update(group_key.as_bytes())
        .chain_update([0])
        .chain_update(branch_id.as_bytes())
        .finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);
    ulid::Ulid::from(u128::from_be_bytes(b) >> 3).to_string()
}

/// What the owner sees of a stored payload: a staff member's setup code hash stays in the cloud, and
/// `setup_code: true` says a code was issued, so Retry asks for a new one (Story 8.26).
fn shown_payload(kind: &str, stored: &str) -> serde_json::Value {
    let mut payload = serde_json::from_str(stored).unwrap_or(serde_json::Value::Null);
    if kind == "staff.upsert"
        && let Some(fields) = payload.as_object_mut()
        && fields.remove("setup_code_hash").is_some()
    {
        fields.remove("setup_expires_at");
        fields.insert("setup_code".into(), true.into());
    }
    payload
}

fn view(row: OwnerCommandRow) -> OwnerCommandView {
    OwnerCommandView {
        id: row.id,
        branch_id: row.branch_id,
        group_id: row.group_id,
        payload: shown_payload(&row.kind, &row.payload),
        kind: row.kind,
        status: row.status,
        error: row.error.and_then(|e| serde_json::from_str(&e).ok()),
        queued_at: row.created_at,
        done_at: row.resolved_at,
    }
}

fn view_of(row: CommandRow, group_id: Option<String>) -> OwnerCommandView {
    view(OwnerCommandRow {
        id: row.id,
        branch_id: row.branch_id,
        group_id,
        idempotency_key: row.idempotency_key,
        kind: row.kind,
        payload: row.payload,
        status: row.status,
        error: row.error,
        issued_by: row.issued_by,
        created_at: row.created_at,
        resolved_at: row.resolved_at,
    })
}

/// The staff id of a member added by a post: taken from the owner's key and the branch, so the same
/// post names the same member whenever it is sent again.
fn new_staff_id(key: &str, branch_id: &str) -> String {
    branch_key(&format!("staff:{key}"), branch_id)
}

/// The `staff.upsert` a branch is sent (Story 8.26): the owner's fields, and for a new member (or
/// when a new code is asked for) the id, a fresh setup code's hash and its expiry. The code itself
/// comes back beside the payload; only its hash is stored and sent to the Hub.
async fn staff_part(
    fields: &OwnerStaffUpsert,
    base_rev: Option<i64>,
    key: &str,
    branch_id: &str,
    now: i64,
) -> Result<(serde_json::Value, Option<StaffSetupCode>), ApiError> {
    let issue = fields.staff_id.is_none() || fields.setup_code == Some(true);
    let staff_id = fields
        .staff_id
        .clone()
        .unwrap_or_else(|| new_staff_id(key, branch_id));
    let mut payload = serde_json::json!({
        "login": fields.login.trim(),
        "display_name": fields.display_name.trim(),
        "role_id": fields.role_id,
        "active": fields.active,
    });
    let Some(out) = payload.as_object_mut() else {
        return Err(ApiError::internal("staff payload"));
    };
    if fields.staff_id.is_some() || issue {
        out.insert("staff_id".into(), staff_id.clone().into());
    }
    if let Some(rev) = base_rev {
        out.insert("base_rev".into(), rev.into());
    }
    if !issue {
        return Ok((payload, None));
    }
    let code = new_pairing_code().map_err(ApiError::internal)?;
    let hash = {
        let code = code.clone();
        blocking(move || hash_secret(&code)).await?
    };
    if let Some(out) = payload.as_object_mut() {
        out.insert("setup_code_hash".into(), hash.into());
        out.insert(
            "setup_expires_at".into(),
            (now + SETUP_CODE_HUB_TTL_MS).into(),
        );
    }
    Ok((
        payload,
        Some(StaffSetupCode {
            branch_id: branch_id.to_owned(),
            staff_id,
            code,
        }),
    ))
}

/// `POST /v1/commands {branch_id | "all", idempotency_key, kind, payload, base_rev?}` from a
/// signed-in owner (Story 8.18).
async fn post_command(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<OwnerCommandPost>,
) -> Result<Response, ApiError> {
    let now = st.now();
    let tenant = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let key = format!("{POST_RATE_PREFIX}{}", tenant.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: POST_RATE_PREFIX,
        bucket_ms: POST_BUCKET_MS,
        window_ms: POST_WINDOW_MS,
        limit: OWNER_COMMAND_POSTS_PER_MIN,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        tracing::warn!(tenant_id = %tenant.id, "owner command refused: rate limited");
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let kind = st.owner_kind(&body.kind)?;
    if ulid::Ulid::from_string(&body.idempotency_key).is_err() {
        return Err(ApiError::validation(
            "idempotency_key",
            "The key must be a ULID.",
        ));
    }
    if owner_command_kind(kind.kind).is_some() {
        validate_owner_payload(kind.kind, &body.payload)?;
    } else if !body.payload.is_object() {
        // A kind of `AuthState::extra_kinds` (tests only): any object will do.
        return Err(ApiError::validation(
            "payload",
            "Send the payload as an object.",
        ));
    }
    let mut payload = body.payload.clone();
    if let (Some(rev), Some(fields)) = (body.base_rev, payload.as_object_mut()) {
        fields.insert("base_rev".into(), rev.into());
    }
    let payload = serde_json::to_string(&payload).map_err(ApiError::internal)?;
    if payload.len() > command_payload_max(kind.kind) {
        return Err(ApiError::validation("payload", "The payload is too large."));
    }
    let staff = if kind.kind == "staff.upsert" {
        Some(checked_staff(&body)?)
    } else {
        None
    };
    let elevated = step_up_check(&kind, signed.session.stepup_at, now)?;
    let branches = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?;
    let all = body.branch_id == OWNER_ALL_BRANCHES;
    let targets = if all {
        branches
    } else {
        let b = branches
            .into_iter()
            .find(|b| b.id == body.branch_id)
            .ok_or_else(|| {
                tracing::warn!(owner_id = %signed.owner.id, "owner command: branch outside the tenant");
                ApiError::not_found()
            })?;
        vec![b]
    };
    let mut commands = Vec::new();
    let mut skipped = Vec::new();
    let mut setup_codes = Vec::new();
    for b in &targets {
        if let Some(e) = refusal(&st, b, &kind).await? {
            if !all {
                return Err(e.into());
            }
            skipped.push(OwnerCommandSkip {
                branch_id: b.id.clone(),
                code: e.code,
                message: e.message,
            });
            continue;
        }
        let (row_key, group) = if all {
            (
                branch_key(&body.idempotency_key, &b.id),
                Some(body.idempotency_key.as_str()),
            )
        } else {
            (body.idempotency_key.clone(), None)
        };
        let (payload, code) = match &staff {
            Some(fields) => {
                let (p, code) = staff_part(fields, body.base_rev, &row_key, &b.id, now).await?;
                (serde_json::to_string(&p).map_err(ApiError::internal)?, code)
            }
            None => (payload.clone(), None),
        };
        let row = st
            .db
            .submit_command(&NewCommand {
                branch_id: &b.id,
                idempotency_key: &row_key,
                kind: kind.kind,
                schema_version: kind.schema_version,
                payload: &payload,
                issued_by: &signed.owner.id,
                group_id: group,
                elevated_by: elevated.then_some(signed.owner.id.as_str()),
                now,
            })
            .await
            .map_err(ApiError::internal)?;
        // A post sent again finds its first row, which holds the first code's hash: this code
        // would not work, so it is not shown.
        if let Some(code) = code
            && row.payload == payload
        {
            setup_codes.push(code);
        }
        commands.push(view_of(row, group.map(str::to_owned)));
    }
    if commands.is_empty()
        && let Some(first) = skipped.first()
    {
        return Err(AppError::new(first.code, first.message.clone()).into());
    }
    let posted = OwnerCommandPosted {
        commands,
        skipped,
        setup_codes: (!setup_codes.is_empty()).then_some(setup_codes),
    };
    // A setup code is shown once: nothing may keep the answer.
    Ok(([(CACHE_CONTROL, "no-store")], Json(posted)).into_response())
}

/// The owner's fields of a `staff.upsert`, checked the way the wizard checks an invitation. A
/// member who is not new keeps the login the Hub holds, so only a new one must be a phone number.
fn checked_staff(body: &OwnerCommandPost) -> Result<OwnerStaffUpsert, ApiError> {
    let mut fields = body.payload.clone();
    if let Some(o) = fields.as_object_mut() {
        o.remove("base_rev");
    }
    let staff: OwnerStaffUpsert = serde_json::from_value(fields)
        .map_err(|_| ApiError::validation("payload", "Send the staff member's details."))?;
    if !valid_text(&staff.display_name) {
        return Err(ApiError::validation(
            "display_name",
            "Enter their name (up to 80 characters).",
        ));
    }
    if staff.staff_id.is_none() && !valid_phone(staff.login.trim()) {
        return Err(ApiError::validation(
            "login",
            "Enter their mobile number with the country code, like +91 98765 43210.",
        ));
    }
    if staff.staff_id.is_some() && staff.login.trim().is_empty() {
        return Err(ApiError::validation("login", "Send the member's login."));
    }
    if staff.role_id.trim().is_empty() {
        return Err(ApiError::validation("role_id", "Choose a role."));
    }
    Ok(staff)
}

/// `POST /v1/commands/preview {branch_id | "all", kind}` from a signed-in owner (Story 8.21): the
/// branches a post of this kind would reach, each with its online state from the Hub's last
/// heartbeat and, where the branch cannot take the kind, the reason a post would give. Writes
/// nothing and does not count against the post limit.
async fn preview_command(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<OwnerCommandPreviewRequest>,
) -> Result<Json<OwnerCommandPreview>, ApiError> {
    let now = st.now();
    let tenant = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let kind = st.owner_kind(&body.kind)?;
    let branches = st
        .db
        .branches_of_tenant(&tenant.id)
        .await
        .map_err(ApiError::internal)?;
    let targets: Vec<Branch> = if body.branch_id == OWNER_ALL_BRANCHES {
        branches
    } else {
        let b = branches
            .into_iter()
            .find(|b| b.id == body.branch_id)
            .ok_or_else(|| {
                tracing::warn!(owner_id = %signed.owner.id, "owner command preview: branch outside the tenant");
                ApiError::not_found()
            })?;
        vec![b]
    };
    let mut out = Vec::with_capacity(targets.len());
    for b in &targets {
        let last_heartbeat_at = st
            .db
            .active_hub(&b.id)
            .await
            .map_err(ApiError::internal)?
            .and_then(|h| h.last_seen_at);
        let error = refusal(&st, b, &kind).await?;
        out.push(OwnerCommandTarget {
            branch_id: b.id.clone(),
            name: b.name.clone(),
            online: last_heartbeat_at.is_some_and(|t| now - t <= BRANCH_ONLINE_WINDOW_MS),
            last_heartbeat_at,
            offered: error.is_none(),
            error,
        });
    }
    Ok(Json(OwnerCommandPreview { targets: out }))
}

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    status: Option<String>,
    branch: Option<String>,
    group: Option<String>,
    before: Option<i64>,
    limit: Option<usize>,
}

/// `GET /v1/commands?status=&branch=&group=&before=&limit=` for a signed-in owner: the café's
/// commands, newest first, paged by `before`.
async fn list_commands(
    st: &Arc<AuthState>,
    parts: &mut Parts,
) -> Result<Json<OwnerCommandList>, ApiError> {
    let RequireSetupComplete(signed) = RequireSetupComplete::from_request_parts(parts, st).await?;
    let Query(q) = Query::<ListQuery>::try_from_uri(&parts.uri)
        .map_err(|_| ApiError::validation("status", "Send a valid filter."))?;
    let status = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => Some(CommandStatus::parse(s).ok_or_else(|| {
            ApiError::validation("status", "Choose pending, fetched, applied or rejected.")
        })?),
    };
    let tenant = st
        .db
        .tenant_of_owner(&signed.owner.id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let limit = q
        .limit
        .unwrap_or(OWNER_COMMANDS_PAGE)
        .clamp(1, OWNER_COMMANDS_PAGE_MAX);
    let mut rows = st
        .db
        .owner_commands(
            &tenant.id,
            &OwnerCommandFilter {
                status,
                branch_id: q.branch.as_deref().filter(|s| !s.is_empty()),
                group_id: q.group.as_deref().filter(|s| !s.is_empty()),
                before: q.before,
                limit: limit + 1,
            },
        )
        .await
        .map_err(ApiError::internal)?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next_before = rows.last().map(|r| r.id).filter(|_| more);
    Ok(Json(OwnerCommandList {
        commands: rows.into_iter().map(view).collect(),
        next_before,
    }))
}

/// `GET /v1/commands`: a Hub (a bearer token, or the `epoch` only it sends) fetches its mailbox, an
/// owner's session lists the café's commands.
async fn get_commands(
    State(st): State<Arc<AuthState>>,
    mut parts: Parts,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let epoch = parts
        .uri
        .query()
        .is_some_and(|q| q.split('&').any(|p| p.starts_with("epoch=")));
    if epoch || parts.headers.contains_key(AUTHORIZATION) {
        let Query(q) = Query::<FetchQuery>::try_from_uri(&parts.uri)
            .map_err(|_| ApiError::bad_request("Send the epoch.", Some("epoch".into())))?;
        return Ok(fetch(State(st), parts.headers, Query(q))
            .await?
            .into_response());
    }
    Ok(list_commands(&st, &mut parts).await?.into_response())
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/commands",
            get(get_commands)
                .post(post_command.layer(DefaultBodyLimit::max(OWNER_POST_BODY_LIMIT))),
        )
        .route("/commands/preview", post(preview_command))
        .route("/commands/{id}/result", post(result))
        .with_state(st)
}

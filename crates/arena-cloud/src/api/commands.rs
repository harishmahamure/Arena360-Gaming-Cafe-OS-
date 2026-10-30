use std::sync::Arc;

use arena_proto::{
    AppError, COMMAND_PAYLOAD_MAX, COMMAND_SCHEMA_VERSION, COMMANDS_PAGE, CommandResult,
    CommandResultAck, CommandStatus, CommandWire, CommandsPage, ErrorCode, OWNER_ALL_BRANCHES,
    OWNER_COMMAND_POSTS_PER_MIN, OWNER_COMMANDS_PAGE, OWNER_COMMANDS_PAGE_MAX, OwnerCommandKind,
    OwnerCommandList, OwnerCommandPost, OwnerCommandPosted, OwnerCommandSkip, OwnerCommandView,
    SETTINGS_COMMAND_KEYS, Tier, accepted_owner_kind, is_setup_command, validate_owner_payload,
};
use arena_store::control::{
    Branch, CommandRow, NewCommand, OwnerCommandFilter, OwnerCommandRow, RateRule,
};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::branches::tier_of;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::pair::check_field;
use super::signup::ceil_s;
use crate::auth::AuthState;
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

fn view(row: OwnerCommandRow) -> OwnerCommandView {
    OwnerCommandView {
        id: row.id,
        branch_id: row.branch_id,
        group_id: row.group_id,
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
        status: row.status,
        error: row.error,
        issued_by: row.issued_by,
        created_at: row.created_at,
        resolved_at: row.resolved_at,
    })
}

/// `POST /v1/commands {branch_id | "all", idempotency_key, kind, payload, base_rev?}` from a
/// signed-in owner (Story 8.18).
async fn post_command(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<OwnerCommandPost>,
) -> Result<Json<OwnerCommandPosted>, ApiError> {
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
    let kind = accepted_owner_kind(&body.kind)?;
    if ulid::Ulid::from_string(&body.idempotency_key).is_err() {
        return Err(ApiError::validation(
            "idempotency_key",
            "The key must be a ULID.",
        ));
    }
    validate_owner_payload(kind.kind, &body.payload)?;
    let mut payload = body.payload.clone();
    if let (Some(rev), Some(fields)) = (body.base_rev, payload.as_object_mut()) {
        fields.insert("base_rev".into(), rev.into());
    }
    let payload = serde_json::to_string(&payload).map_err(ApiError::internal)?;
    if payload.len() > COMMAND_PAYLOAD_MAX {
        return Err(ApiError::validation("payload", "The payload is too large."));
    }
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
    for b in &targets {
        if let Some(e) = refusal(&st, b, kind).await? {
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
                now,
            })
            .await
            .map_err(ApiError::internal)?;
        commands.push(view_of(row, group.map(str::to_owned)));
    }
    if commands.is_empty()
        && let Some(first) = skipped.first()
    {
        return Err(AppError::new(first.code, first.message.clone()).into());
    }
    Ok(Json(OwnerCommandPosted { commands, skipped }))
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
        .route("/commands", get(get_commands).post(post_command))
        .route("/commands/{id}/result", post(result))
        .with_state(st)
}

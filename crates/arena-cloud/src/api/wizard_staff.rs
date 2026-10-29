use std::sync::Arc;

use arena_proto::{
    AppError, COMMAND_SCHEMA_VERSION, CommandStatus, ErrorCode, InviteRole, InviteStatus,
    IssuePairingCode, SETUP_CODE_HUB_TTL_MS, StaffInvite, StaffInviteView, StaffInvited,
    StaffInvites, WizardState, WizardStep,
};
use arena_store::control::{Branch, NewCommand, NewStaffInvite, RateRule, StaffInviteRow};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::account::blocking;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::wizard::wizard_state;
use super::wizard_tier::target_branch;
use crate::auth::{AuthState, valid_phone, valid_text};
use crate::pairing_code::new_pairing_code;
use crate::passwords::hash_secret;

const INVITE_PREFIX: &str = "staffinvite:";
const INVITES_PER_HOUR: u64 = 30;
const HOUR_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Deserialize)]
struct StaffQuery {
    branch_id: Option<String>,
}

pub(super) async fn ready_branch(
    st: &AuthState,
    owner_id: &str,
    branch_id: Option<&str>,
) -> Result<Branch, ApiError> {
    let branch = target_branch(st, owner_id, branch_id).await?;
    let seen = st
        .db
        .active_hub(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|h| h.last_seen_at.is_some());
    if !seen {
        return Err(AppError::new(ErrorCode::StepNotReady, "Pair your Hub first.").into());
    }
    Ok(branch)
}

fn view(r: StaffInviteRow) -> Result<StaffInviteView, ApiError> {
    let (status, error) = match r.status {
        Some(CommandStatus::Applied) => (InviteStatus::Applied, None),
        Some(CommandStatus::Rejected) => (
            InviteStatus::Failed,
            r.error
                .as_deref()
                .map(serde_json::from_str::<AppError>)
                .transpose()
                .map_err(ApiError::internal)?,
        ),
        _ => (InviteStatus::Waiting, None),
    };
    Ok(StaffInviteView {
        id: r.id,
        name: r.name,
        phone: r.phone,
        role: InviteRole::parse(&r.role).ok_or_else(|| ApiError::internal("invite role"))?,
        status,
        error,
        setup_expires_at: r.setup_expires_at,
    })
}

async fn reserve(st: &AuthState, branch_id: &str, now: i64) -> Result<(), ApiError> {
    let key = format!("{INVITE_PREFIX}{branch_id}");
    let rule = RateRule {
        key: &key,
        prune_prefix: INVITE_PREFIX,
        bucket_ms: 60_000,
        window_ms: HOUR_MS,
        limit: INVITES_PER_HOUR,
    };
    st.db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
        .map(|_| ())
        .map_err(|wait_ms| {
            ApiError::rate_limited(u64::try_from((wait_ms + 999) / 1000).unwrap_or(1))
        })
}

struct Issued {
    code: String,
    key: String,
    payload: String,
    expires_at: i64,
}

async fn issue(
    id: &str,
    name: &str,
    phone: &str,
    role: InviteRole,
    now: i64,
) -> Result<Issued, ApiError> {
    let code = new_pairing_code().map_err(ApiError::internal)?;
    let hash = {
        let code = code.clone();
        blocking(move || hash_secret(&code)).await?
    };
    let expires_at = now + SETUP_CODE_HUB_TTL_MS;
    let payload = serde_json::json!({
        "staff_id": id,
        "login": phone,
        "display_name": name,
        "role_id": role.hub_role(),
        "active": true,
        "setup_code_hash": hash,
        "setup_expires_at": expires_at,
    });
    Ok(Issued {
        code,
        key: ulid::Ulid::generate().to_string(),
        payload: serde_json::to_string(&payload).map_err(ApiError::internal)?,
        expires_at,
    })
}

fn command<'a>(branch_id: &'a str, i: &'a Issued, owner_id: &'a str, now: i64) -> NewCommand<'a> {
    NewCommand {
        branch_id,
        idempotency_key: &i.key,
        kind: "staff.upsert",
        schema_version: COMMAND_SCHEMA_VERSION,
        payload: &i.payload,
        issued_by: owner_id,
        group_id: None,
        now,
    }
}

async fn get_staff(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<StaffQuery>,
) -> Result<Json<StaffInvites>, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    let rows = st
        .db
        .staff_invites(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(StaffInvites {
        invites: rows.into_iter().map(view).collect::<Result<_, _>>()?,
    }))
}

async fn invited(
    st: &AuthState,
    branch_id: &str,
    id: &str,
    code: String,
) -> Result<Response, ApiError> {
    let row = st
        .db
        .staff_invite(branch_id, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("invite missing after write"))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(StaffInvited {
            invite: view(row)?,
            code,
        }),
    )
        .into_response())
}

async fn post_invite(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<StaffInvite>,
) -> Result<Response, ApiError> {
    let name = body.name.trim();
    if !valid_text(name) {
        return Err(ApiError::validation(
            "name",
            "Enter their name (up to 80 characters).",
        ));
    }
    let phone = body.phone.trim();
    if !valid_phone(phone) {
        return Err(ApiError::validation(
            "phone",
            "Enter their mobile number with the country code, like +91 98765 43210.",
        ));
    }
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let now = st.now();
    reserve(&st, &branch.id, now).await?;
    let id = ulid::Ulid::generate().to_string();
    let issued = issue(&id, name, phone, body.role, now).await?;
    let created = st
        .db
        .create_staff_invite(
            &NewStaffInvite {
                id: &id,
                branch_id: &branch.id,
                name,
                phone,
                role: body.role.as_str(),
                setup_expires_at: issued.expires_at,
                now,
            },
            &command(&branch.id, &issued, &signed.owner.id, now),
        )
        .await
        .map_err(ApiError::internal)?;
    if !created {
        return Err(ApiError::internal("staff invite not written"));
    }
    tracing::info!(owner_id = %signed.owner.id, branch_id = %branch.id, staff_invite_id = %id, "staff invited");
    invited(&st, &branch.id, &id, issued.code).await
}

async fn post_code(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<Response, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, body.branch_id.as_deref()).await?;
    let row = st
        .db
        .staff_invite(&branch.id, &id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let role = InviteRole::parse(&row.role).ok_or_else(|| ApiError::internal("invite role"))?;
    let now = st.now();
    reserve(&st, &branch.id, now).await?;
    let issued = issue(&id, &row.name, &row.phone, role, now).await?;
    let changed = st
        .db
        .reissue_staff_invite(
            &branch.id,
            &id,
            issued.expires_at,
            &command(&branch.id, &issued, &signed.owner.id, now),
        )
        .await
        .map_err(ApiError::internal)?;
    if !changed {
        return Err(ApiError::internal("staff invite not reissued"));
    }
    tracing::info!(owner_id = %signed.owner.id, branch_id = %branch.id, staff_invite_id = %id, "staff setup code reissued");
    invited(&st, &branch.id, &id, issued.code).await
}

pub(super) async fn skip(
    st: &AuthState,
    owner_id: &str,
    body: IssuePairingCode,
    step: WizardStep,
) -> Result<Json<WizardState>, ApiError> {
    let branch = ready_branch(st, owner_id, body.branch_id.as_deref()).await?;
    st.db
        .skip_step(&branch.id, step.as_str())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(
        wizard_state(st, owner_id, body.branch_id.as_deref()).await?,
    ))
}

async fn post_skip(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<Json<WizardState>, ApiError> {
    skip(&st, &signed.owner.id, body, WizardStep::Staff).await
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/staff", get(get_staff).post(post_invite))
        .route("/wizard/staff/skip", post(post_skip))
        .route("/wizard/staff/{id}/code", post(post_code))
        .with_state(st)
}

use std::sync::Arc;

use arena_proto::{
    AdConsentState, BranchSetup, GoLiveBlocker, GoLiveItem, GoLiveItemView, GoLiveView,
    IssuePairingCode, StepState, Tier, WizardState, WizardStep,
};
use arena_store::control::Branch;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::commands::submit_command;
use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::json::ApiJson;
use super::wizard::wizard_state;
use super::wizard_staff::ready_branch;
use crate::auth::AuthState;

const STEPS_DONE: [WizardStep; 6] = [
    WizardStep::Cafe,
    WizardStep::Tier,
    WizardStep::InstallHub,
    WizardStep::Devices,
    WizardStep::Rates,
    WizardStep::TestSession,
];
const STEPS_SETTLED: [WizardStep; 3] = [
    WizardStep::Products,
    WizardStep::Staff,
    WizardStep::PrinterPayments,
];

#[derive(Debug, Deserialize)]
struct GoLiveQuery {
    branch_id: Option<String>,
}

async fn view(st: &AuthState, branch: &Branch) -> Result<GoLiveView, ApiError> {
    let free = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .is_none_or(|e| e.tier_at(st.now()) == Tier::Free);
    let status = st
        .db
        .branch_setup(&branch.id)
        .await
        .map_err(ApiError::internal)?
        .map(|(body, _)| serde_json::from_str::<BranchSetup>(&body))
        .transpose()
        .map_err(ApiError::internal)?
        .and_then(|s| s.status);
    let items = vec![
        GoLiveItemView {
            key: GoLiveItem::OpeningHours,
            done: status.is_some_and(|s| s.opening_hours_set),
        },
        GoLiveItemView {
            key: GoLiveItem::BackupLocation,
            done: status.is_some_and(|s| s.backup_location_set),
        },
        GoLiveItemView {
            key: GoLiveItem::Standby,
            done: false,
        },
    ]
    .into_iter()
    .chain(free.then(|| GoLiveItemView {
        key: GoLiveItem::AdConsent,
        done: status.is_some_and(|s| s.ad_consent == AdConsentState::Given),
    }))
    .collect::<Vec<_>>();
    let progress = st
        .db
        .onboarding_progress(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let state = |step| {
        progress
            .iter()
            .find(|p| p.step == step)
            .map_or(StepState::Pending, |p| p.state)
    };
    let blocker = STEPS_DONE
        .into_iter()
        .find(|s| state(*s) != StepState::Done)
        .or_else(|| {
            STEPS_SETTLED
                .into_iter()
                .find(|s| state(*s) == StepState::Pending)
        })
        .map(GoLiveBlocker::Step)
        .or_else(|| {
            items
                .iter()
                .find(|i| i.key != GoLiveItem::Standby && !i.done)
                .map(|i| GoLiveBlocker::Item(i.key))
        });
    Ok(GoLiveView {
        items,
        blocker,
        free,
        live: branch.live_at.is_some(),
    })
}

async fn get_go_live(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<GoLiveQuery>,
) -> Result<Json<GoLiveView>, ApiError> {
    let branch = ready_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    Ok(Json(view(&st, &branch).await?))
}

async fn post_go_live(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    ApiJson(body): ApiJson<IssuePairingCode>,
) -> Result<Json<WizardState>, ApiError> {
    let owner_id = &signed.owner.id;
    let branch = ready_branch(&st, owner_id, body.branch_id.as_deref()).await?;
    let go = view(&st, &branch).await?;
    if go.blocker.is_some() {
        return Err(ApiError::validation(
            "go_live",
            "Finish the items listed above before you go live.",
        ));
    }
    if !go.live {
        submit_command(
            &st,
            &branch.id,
            &ulid::Ulid::generate().to_string(),
            "branch.go_live",
            &serde_json::json!({}),
            owner_id,
        )
        .await?;
        st.db
            .go_live(&branch.id, st.now())
            .await
            .map_err(ApiError::internal)?;
        tracing::info!(owner_id = %owner_id, branch_id = %branch.id, "branch went live");
    }
    Ok(Json(
        wizard_state(&st, owner_id, body.branch_id.as_deref()).await?,
    ))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/go-live", get(get_go_live).post(post_go_live))
        .with_state(st)
}

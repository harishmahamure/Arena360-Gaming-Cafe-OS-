use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use super::error::ApiError;
use super::gate::RequireSetupComplete;
use super::wizard_tier::target_branch;
use crate::auth::AuthState;

const KEEP_ALIVE: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct EventsQuery {
    branch_id: Option<String>,
}

async fn events(
    State(st): State<Arc<AuthState>>,
    RequireSetupComplete(signed): RequireSetupComplete,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let branch = target_branch(&st, &signed.owner.id, q.branch_id.as_deref()).await?;
    let rx = st.wizard_tx.subscribe();
    let stream = futures_util::stream::unfold((rx, branch.id), |(mut rx, id)| async move {
        loop {
            match rx.recv().await {
                Ok(b) if b != id => {}
                Ok(_) | Err(RecvError::Lagged(_)) => {
                    let event = Event::default().event("wizard.changed").data("{}");
                    return Some((Ok(event), (rx, id)));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/wizard/events", get(events))
        .with_state(st)
}

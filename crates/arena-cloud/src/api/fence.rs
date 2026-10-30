//! `POST /v1/fence/claim` (Story 9.3, AD-24): the one place, with the heartbeat, where an epoch
//! counter is issued.
//!
//! The branch comes from the device token. The claim moves the branch fence from exactly
//! `expected` to `(expected counter + 1):<instance_id>` by compare-and-set
//! ([`arena_store::control::ControlDb::claim_branch_fence`]); when the fence is anything else the
//! answer is `FENCED` naming the fence and nothing is written. The claim's `fork.parent_seq` is
//! stored on the fence row and is the fork point of the command re-queue, done in the same
//! transaction as the compare-and-set.

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, FenceClaimRequest, FenceClaimResponse, FenceFork};
use arena_store::control::{FenceMove, RateRule};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::presign::{Epoch, MAX_SEQ};

const RATE_PREFIX: &str = "fence:";
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Claims an hour per branch: promote and restore are rare and manual.
const HOURLY_LIMIT: u64 = 60;

/// The text of the `FENCED` message that names `fence`; a Hub reads it back with
/// [`arena_proto::fenced_epoch`].
pub(super) fn fenced_message(fence: &str) -> String {
    format!("A newer Hub has taken over this branch (epoch {fence}).")
}

/// What a claim asks for once its body is checked: the epoch to move from, the epoch to make and
/// the fork point.
#[derive(Debug, PartialEq, Eq)]
struct Checked {
    expected: Epoch,
    next: String,
    fork_seq: i64,
}

fn bad(field: &str, message: &str) -> ApiError {
    ApiError::validation(field, message)
}

fn check(body: &FenceClaimRequest) -> Result<Checked, ApiError> {
    let expected = Epoch::parse(&body.expected)
        .map_err(|_| bad("expected", "Send the fence epoch like 3:HUBINSTANCE."))?;
    // The instance is checked as the instance part of an epoch.
    let instance = Epoch::parse(&format!("1:{}", body.instance_id))
        .ok()
        .filter(|e| e.instance() == body.instance_id)
        .ok_or_else(|| bad("instance_id", "Send the Hub's instance id."))?;
    let FenceFork {
        parent_epoch,
        parent_seq,
    } = &body.fork;
    if Epoch::parse(parent_epoch).is_err() {
        return Err(bad(
            "fork.parent_epoch",
            "Send the parent epoch like 3:HUBINSTANCE.",
        ));
    }
    if !(0..=MAX_SEQ).contains(parent_seq) {
        return Err(bad("fork.parent_seq", "The parent seq cannot be negative."));
    }
    let counter = expected
        .counter()
        .checked_add(1)
        .filter(|c| *c <= i64::MAX as u64)
        .ok_or_else(|| bad("expected", "That epoch counter cannot be advanced."))?;
    Ok(Checked {
        next: format!("{counter}:{}", instance.instance()),
        expected,
        fork_seq: *parent_seq,
    })
}

async fn claim(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<FenceClaimRequest>,
) -> Result<Json<FenceClaimResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    let checked = check(&body)?;
    let key = format!("{RATE_PREFIX}{}", hub.branch_id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: HOURLY_LIMIT,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }
    let moved = st
        .db
        .claim_branch_fence(
            &hub.branch_id,
            &hub.id,
            checked.expected.as_str(),
            &checked.next,
            Some(checked.fork_seq),
            now,
        )
        .await
        .map_err(ApiError::internal)?;
    match moved {
        FenceMove::Moved { requeued } => {
            tracing::info!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                reason = ?body.reason,
                epoch = %checked.next,
                requeued,
                "fence claimed"
            );
            Ok(Json(FenceClaimResponse {
                epoch: checked.next,
            }))
        }
        FenceMove::Stale { current } => {
            tracing::warn!(
                hub_id = %hub.id,
                branch_id = %hub.branch_id,
                expected = %checked.expected.as_str(),
                "fence claim refused"
            );
            Err(AppError::new(
                ErrorCode::Fenced,
                match current {
                    Some(fence) => fenced_message(&fence),
                    None => "No Hub has reported to this branch yet.".to_owned(),
                },
            )
            .into())
        }
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/fence/claim", post(claim))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::FenceClaimReason;

    fn req(expected: &str, instance: &str, parent: &str, seq: i64) -> FenceClaimRequest {
        FenceClaimRequest {
            expected: expected.into(),
            instance_id: instance.into(),
            reason: FenceClaimReason::Promote,
            fork: FenceFork {
                parent_epoch: parent.into(),
                parent_seq: seq,
            },
        }
    }

    #[test]
    fn a_claim_makes_the_next_counter_for_the_claimers_instance() {
        let ok = check(&req("3:A", "B", "3:A", 40)).unwrap();
        assert_eq!((ok.next.as_str(), ok.fork_seq), ("4:B", 40));
        assert_eq!(check(&req("3:A", "B", "3:A", 0)).unwrap().fork_seq, 0);
    }

    #[test]
    fn a_body_that_does_not_parse_is_refused() {
        for bad in [
            req("legacy", "B", "3:A", 1),
            req("3:A", "", "3:A", 1),
            req("3:A", "B/C", "3:A", 1),
            req("3:A", "9:B", "3:A", 1),
            req("3:A", "B", "nope", 1),
            req("3:A", "B", "3:A", -1),
            req("3:A", "B", "3:A", MAX_SEQ + 1),
            req("9223372036854775807:A", "B", "3:A", 1),
        ] {
            assert!(check(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_fenced_message_names_the_fence_a_hub_can_read_back() {
        assert_eq!(
            arena_proto::fenced_epoch(&fenced_message("4:B")),
            Some("4:B")
        );
    }
}

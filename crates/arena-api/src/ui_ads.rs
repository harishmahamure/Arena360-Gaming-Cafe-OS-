//! `POST /ui/ads/event` (Story 4.14): the page forwards the ad plugin's `ad.requested` and
//! `ad.shown` events here, one post per event. The Hub records each as a system intent
//! (`ad.request_recorded` / `ad.impression_recorded`) through `apply()`, with the event's id as
//! the idempotency key, so a post that is repeated writes nothing more.
//!
//! No staff session is needed: ads also run on the sign-in and lock screens, and the body holds
//! only a kind, a slot and an id (no staff, device or creative information). The route sits
//! behind the same loopback origin guard as the rest of `/ui`. The Hub knows nothing about ads
//! beyond the two slot names.

use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AdEventKind, AdImpressionRecorded, AdRequestRecorded, AppError, ErrorCode,
    Intent, IntentPayload, UiAdEvent,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, bad_frame, fail, ui_error};

const SHAPE: &str = "the body must be {\"kind\": \"request\"|\"impression\", \"slot\": \"banner\"|\"interstitial\", \"id\": ULID}";

/// The system actor the two ad intents run as.
const ACTOR: &str = "hub-ads";

pub(crate) async fn event(State(auth): State<Arc<UiAuth>>, body: axum::body::Bytes) -> Response {
    let Ok(event) = serde_json::from_slice::<UiAdEvent>(&body) else {
        return fail(bad_frame(SHAPE));
    };
    if !arena_core::is_ulid(&event.id) {
        return fail(bad_frame("id must be a ULID"));
    }
    let Some(writer) = auth.writer() else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    };
    let payload = match event.kind {
        AdEventKind::Request => {
            IntentPayload::AdRequestRecorded(AdRequestRecorded { slot: event.slot })
        }
        AdEventKind::Impression => {
            IntentPayload::AdImpressionRecorded(AdImpressionRecorded { slot: event.slot })
        }
    };
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: event.id,
        issued_at: auth.clock().now_ms(),
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: ACTOR.into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    };
    match writer.submit_intent(intent).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => fail(ui_error(e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use crate::ui_auth::tests::{call, hub};

    const A: &str = "01JADEVENT0000000000000001";
    const B: &str = "01JADEVENT0000000000000002";

    fn post(kind: &str, slot: &str, id: &str) -> serde_json::Value {
        json!({"kind": kind, "slot": slot, "id": id})
    }

    fn total(counts: &Option<arena_proto::AdCounts>) -> i64 {
        counts
            .as_ref()
            .map_or(0, |c| c.counts.iter().map(|d| d.n).sum())
    }

    #[tokio::test]
    async fn an_event_is_one_row_and_a_repeat_adds_none() {
        let hub = hub().await;
        for _ in 0..2 {
            let (status, _) = call(
                &hub,
                "POST",
                "/ui/ads/event",
                None,
                Some(post("impression", "banner", A)),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/ads/event",
            None,
            Some(post("request", "interstitial", B)),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let pending = hub.writer.ad_pending().await.unwrap();
        assert_eq!(total(&pending.impressions), 1);
        assert_eq!(total(&pending.requests), 1);
        assert_eq!(
            pending.impressions.unwrap().counts[0].slot,
            arena_proto::AdSlot::Banner
        );
    }

    #[tokio::test]
    async fn a_malformed_event_is_refused_and_writes_nothing() {
        let hub = hub().await;
        for body in [
            post("impression", "rewarded", A),
            post("click", "banner", A),
            post("impression", "banner", "not-a-ulid"),
            json!({"kind": "impression", "slot": "banner"}),
            json!({"kind": "impression", "slot": "banner", "id": A, "device": "pc-1"}),
        ] {
            let (status, res) = call(&hub, "POST", "/ui/ads/event", None, Some(body.clone())).await;
            // An unknown extra field is ignored; the rest are refused as a bad frame.
            if body.get("device").is_some() {
                assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
            } else {
                assert_eq!(
                    (status, res["code"].as_str()),
                    (StatusCode::BAD_REQUEST, Some("BAD_FRAME")),
                    "{body}"
                );
            }
        }
        let pending = hub.writer.ad_pending().await.unwrap();
        assert_eq!(total(&pending.impressions), 1, "only the last was valid");
        assert_eq!(pending.requests, None);
    }

    #[tokio::test]
    async fn one_event_id_cannot_count_for_two_slots() {
        let hub = hub().await;
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/ads/event",
            None,
            Some(post("request", "banner", A)),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, res) = call(
            &hub,
            "POST",
            "/ui/ads/event",
            None,
            Some(post("request", "interstitial", A)),
        )
        .await;
        assert_eq!(
            (status, res["code"].as_str()),
            (StatusCode::CONFLICT, Some("IDEMPOTENCY_CONFLICT"))
        );
        assert_eq!(total(&hub.writer.ad_pending().await.unwrap().requests), 1);
    }
}

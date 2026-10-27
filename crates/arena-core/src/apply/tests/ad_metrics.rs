use super::stock::{intent, owner, staff};
use super::*;
use crate::rbac::ROLE_COUNTER;
use arena_proto::{
    AD_BATCH_ROWS, AdDayCount, AdImpressionRecorded, AdRequestRecorded, AdSlot, ErrorCode,
};
use arena_store::{AdKind, ad_rows_after, insert_ad};

const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;
// T0 is 2027-01-15 13:30 in Asia/Kolkata; the business day starts at 06:00.
const D1: &str = "2027-01-15";
const D2: &str = "2027-01-16";
const D3: &str = "2027-01-17";

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub-ads".into(),
    }
}

fn impression(slot: AdSlot) -> Intent {
    intent(
        system(),
        IntentPayload::AdImpressionRecorded(AdImpressionRecorded { slot }),
    )
}

fn request(slot: AdSlot) -> Intent {
    intent(
        system(),
        IntentPayload::AdRequestRecorded(AdRequestRecorded { slot }),
    )
}

fn count(day: &str, slot: AdSlot, n: i64) -> AdDayCount {
    AdDayCount {
        business_day: day.into(),
        slot,
        n,
    }
}

async fn rows(f: &Fixture, table: &str) -> Vec<(i64, String, i64)> {
    let kind = if table == "ad_requests" {
        AdKind::Request
    } else {
        AdKind::Impression
    };
    f.writer
        .submit(move |tx| ad_rows_after(tx, kind, 0, 100_000))
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.id, r.slot, r.at))
        .collect()
}

fn log_len(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

#[tokio::test]
async fn an_impression_and_a_request_each_write_one_row_and_no_change_record() {
    let f = bootstrapped().await;
    let before = log_len(&f);
    f.clock.set(T0 + 5_000);
    let a = f
        .writer
        .submit_intent(impression(AdSlot::Banner))
        .await
        .unwrap();
    let b = f
        .writer
        .submit_intent(request(AdSlot::Interstitial))
        .await
        .unwrap();
    assert!(a.changes.is_empty() && b.changes.is_empty());
    assert_eq!(
        rows(&f, "ad_impressions").await,
        [(1, "banner".into(), T0 + 5_000)]
    );
    assert_eq!(
        rows(&f, "ad_requests").await,
        [(1, "interstitial".into(), T0 + 5_000)]
    );
    assert_eq!(log_len(&f), before, "not in the change log");
}

#[tokio::test]
async fn the_same_event_again_writes_no_second_row() {
    let mut f = bootstrapped().await;
    let first = impression(AdSlot::Banner);
    f.writer.submit_intent(first.clone()).await.unwrap();
    f.events.try_recv().unwrap();
    // A retry (a new intent id, the same event id) gets the first outcome and writes nothing.
    let retry = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        ..first.clone()
    };
    f.writer.submit_intent(retry).await.unwrap();
    f.writer.submit_intent(first.clone()).await.unwrap();
    assert_eq!(rows(&f, "ad_impressions").await.len(), 1);
    assert!(f.events.try_recv().is_err(), "a replay emits nothing");
    // The same event id for another slot is a different body, never a second count.
    let clash = Intent {
        payload: IntentPayload::AdImpressionRecorded(AdImpressionRecorded {
            slot: AdSlot::Interstitial,
        }),
        ..first
    };
    let err = f.writer.submit_intent(clash).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::IdempotencyConflict);
    assert_eq!(rows(&f, "ad_impressions").await.len(), 1);
}

#[tokio::test]
async fn only_the_system_records_an_ad() {
    let f = bootstrapped().await;
    let counter = staff(&f, "ahmed", ROLE_COUNTER).await;
    for actor in [
        owner(),
        counter,
        Actor {
            kind: ActorKind::Device,
            id: "pc".into(),
        },
    ] {
        for payload in [
            IntentPayload::AdImpressionRecorded(AdImpressionRecorded {
                slot: AdSlot::Banner,
            }),
            IntentPayload::AdRequestRecorded(AdRequestRecorded {
                slot: AdSlot::Banner,
            }),
        ] {
            let err = f
                .writer
                .submit_intent(intent(actor.clone(), payload))
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::PermissionDenied, "{actor:?}");
        }
    }
    assert!(
        rows(&f, "ad_impressions").await.is_empty() && rows(&f, "ad_requests").await.is_empty()
    );
}

#[tokio::test]
async fn a_branch_that_does_not_exist_records_nothing() {
    let f = primary_long_lease();
    let err = f
        .writer
        .submit_intent(request(AdSlot::Banner))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationFailed);
}

#[tokio::test]
async fn an_ad_intent_takes_no_base_rev() {
    let f = bootstrapped().await;
    let mut i = request(AdSlot::Banner);
    i.base_rev = Some(1);
    let err = f.writer.submit_intent(i).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    assert!(rows(&f, "ad_requests").await.is_empty());
}

#[tokio::test]
async fn nothing_is_pending_before_anything_is_recorded() {
    let f = bootstrapped().await;
    assert_eq!(
        f.writer.ad_pending().await.unwrap(),
        crate::AdPending::default()
    );
}

#[tokio::test]
async fn a_three_day_outage_sends_each_day_once_and_loses_or_doubles_nothing() {
    let f = bootstrapped().await;
    let events = [
        (T0, AdSlot::Banner),
        (T0 + 2 * HOUR, AdSlot::Banner),
        // 00:30 on the 16th is before the 06:00 day start: still business day the 15th.
        (T0 + 11 * HOUR, AdSlot::Interstitial),
        (T0 + DAY, AdSlot::Banner),
        (T0 + DAY + HOUR, AdSlot::Banner),
        (T0 + 2 * DAY, AdSlot::Interstitial),
    ];
    for (at, slot) in events {
        f.clock.set(at);
        f.writer.submit_intent(request(slot)).await.unwrap();
    }
    for (at, slot) in &events[..3] {
        f.clock.set(*at);
        f.writer.submit_intent(impression(*slot)).await.unwrap();
    }
    f.clock.set(T0 + 2 * DAY + HOUR);

    let pending = f.writer.ad_pending().await.unwrap();
    let requests = pending.requests.clone().unwrap();
    assert_eq!((requests.after_id, requests.through_id), (Some(0), 6));
    assert_eq!(
        requests.counts,
        [
            count(D1, AdSlot::Banner, 2),
            count(D1, AdSlot::Interstitial, 1),
            count(D2, AdSlot::Banner, 2),
            count(D3, AdSlot::Interstitial, 1),
        ]
    );
    let impressions = pending.impressions.clone().unwrap();
    assert_eq!(impressions.through_id, 3);
    assert_eq!(
        impressions.counts,
        [
            count(D1, AdSlot::Banner, 2),
            count(D1, AdSlot::Interstitial, 1)
        ]
    );

    // Until the cloud acknowledges, the same totals are built again, never more.
    assert_eq!(f.writer.ad_pending().await.unwrap(), pending);

    // The acknowledgement moves the cursors; the next build starts after them.
    f.writer.ad_ack(Some(3), Some(6)).await.unwrap();
    assert_eq!(
        f.writer.ad_pending().await.unwrap(),
        crate::AdPending::default()
    );
    f.clock.set(T0 + 2 * DAY + 2 * HOUR);
    f.writer
        .submit_intent(request(AdSlot::Banner))
        .await
        .unwrap();
    let next = f.writer.ad_pending().await.unwrap();
    assert_eq!(next.impressions, None);
    let requests = next.requests.unwrap();
    assert_eq!((requests.after_id, requests.through_id), (Some(6), 7));
    assert_eq!(requests.counts, [count(D3, AdSlot::Banner, 1)]);
}

#[tokio::test]
async fn a_cursor_moves_only_on_an_acknowledgement_and_never_past_what_exists() {
    let f = bootstrapped().await;
    for _ in 0..3 {
        f.writer
            .submit_intent(request(AdSlot::Banner))
            .await
            .unwrap();
    }
    f.writer.ad_ack(None, Some(2)).await.unwrap();
    let after_two = f.writer.ad_pending().await.unwrap().requests.unwrap();
    assert_eq!((after_two.after_id, after_two.through_id), (Some(2), 3));
    f.writer.ad_ack(None, Some(900)).await.unwrap();
    assert_eq!(f.writer.ad_pending().await.unwrap().requests, None);
    f.writer
        .submit_intent(request(AdSlot::Banner))
        .await
        .unwrap();
    let next = f.writer.ad_pending().await.unwrap().requests.unwrap();
    assert_eq!((next.after_id, next.through_id), (Some(3), 4));
}

#[tokio::test]
async fn one_heartbeat_carries_at_most_a_batch_and_the_rest_follow() {
    let f = bootstrapped().await;
    let total = AD_BATCH_ROWS + 3;
    f.writer
        .submit(move |tx| {
            for i in 0..total {
                insert_ad(tx, AdKind::Request, "banner", T0 + i as i64)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let first = f.writer.ad_pending().await.unwrap().requests.unwrap();
    assert_eq!(first.through_id, AD_BATCH_ROWS as i64);
    assert_eq!(
        first.counts.iter().map(|c| c.n).sum::<i64>(),
        AD_BATCH_ROWS as i64
    );
    assert!(first.valid());
    f.writer.ad_ack(None, Some(first.through_id)).await.unwrap();
    let rest = f.writer.ad_pending().await.unwrap().requests.unwrap();
    assert_eq!(rest.counts.iter().map(|c| c.n).sum::<i64>(), 3);
    assert_eq!(
        (rest.after_id, rest.through_id),
        (Some(first.through_id), first.through_id + 3)
    );
}

#[tokio::test]
async fn a_follower_neither_reports_nor_moves_a_cursor() {
    let f = bootstrapped().await;
    f.writer
        .submit_intent(request(AdSlot::Banner))
        .await
        .unwrap();
    f.status.send_modify(|s| s.role = Role::Follower);
    assert_eq!(
        f.writer.ad_pending().await.unwrap_err().code,
        ErrorCode::NotPrimary
    );
    assert_eq!(
        f.writer.ad_ack(None, Some(1)).await.unwrap_err().code,
        ErrorCode::NotPrimary
    );
    assert_eq!(rows(&f, "ad_requests").await.len(), 1);
}

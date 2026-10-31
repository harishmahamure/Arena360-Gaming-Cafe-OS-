//! Story 10.12: a lower licence marks the newest devices over the cap, touches no session, and
//! refuses new starts and moves onto them; revoking a device or a higher licence frees them.

use super::prepaid::{begin, group_of, ist, system};
use super::session::{end, end_intent, int, pc, refusal, rev_of, row_of, submit, table, text};
use super::stock::owner;
use super::transfer::{online, postpaid, spare};
use super::*;
use arena_proto::{
    DeviceKind, DeviceRevoke, LicenceApply, LicenceStanding, SessionMode, SessionTransfer, Tier,
};

async fn licence(f: &Fixture, devices: u32) -> Result<Applied, AppError> {
    submit(
        f,
        system(),
        IntentPayload::LicenceApply(LicenceApply {
            standing: LicenceStanding::Verified,
            clock_wrong: false,
            tier: Tier::Free,
            flags: String::new(),
            device_cap: devices,
            user_cap: 10,
        }),
    )
    .await
}

fn over(f: &Fixture, device: &str) -> bool {
    table(f, Table::DevicesMeta)
        .iter()
        .find(|r| text(r, "device_id") == device)
        .is_some_and(|r| int(r, "over_cap") == 1)
}

/// Three PCs of one group, oldest pairing first, all online.
async fn three(f: &Fixture) -> [String; 3] {
    let a = pc(f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let g = group_of(f, &a);
    let b = spare(f, Some(&g), DeviceKind::Pc).await;
    let c = spare(f, Some(&g), DeviceKind::Pc).await;
    online(f, &[&a, &b, &c]).await;
    [a, b, c]
}

#[tokio::test]
async fn a_lower_cap_marks_the_newest_pairings_and_audits_each() {
    let f = bootstrapped().await;
    let [a, b, c] = three(&f).await;
    let applied = licence(&f, 1).await.unwrap();
    assert!(!over(&f, &a) && over(&f, &b) && over(&f, &c));
    let marked: Vec<_> = applied
        .changes
        .iter()
        .filter(|ch| ch.tbl == "devices_meta")
        .map(|ch| ch.row_id.clone())
        .collect();
    assert_eq!(marked.len(), 2);
    assert!(marked.contains(&b) && marked.contains(&c));
    assert!(
        applied
            .changes
            .iter()
            .all(|ch| ch.tbl == "settings" || ch.tbl == "devices_meta")
    );
}

#[tokio::test]
async fn a_running_session_on_a_marked_device_is_untouched_and_the_lock_waits_for_its_end() {
    let f = bootstrapped().await;
    let [_a, _b, c] = three(&f).await;
    let id = postpaid(&f, &c, "2027-01-15 15:30").await;
    let before = row_of(&f, Table::Sessions, &id);
    let applied = licence(&f, 1).await.unwrap();
    assert!(applied.changes.iter().all(|ch| ch.tbl != "sessions"));
    assert!(over(&f, &c));
    assert_eq!(row_of(&f, Table::Sessions, &id), before);
    f.clock.set(ist("2027-01-15 16:00"));
    let rev = rev_of(&f, Table::Sessions, &id);
    end(&f, end_intent(&id, Some(rev))).await.unwrap();
    assert_eq!(
        refusal(begin(&f, &c, None, SessionMode::Postpaid, None).await),
        (ErrorCode::DeviceOverCap, Some("device_id".into()))
    );
}

#[tokio::test]
async fn starts_and_transfers_onto_a_marked_device_are_refused() {
    let f = bootstrapped().await;
    let [a, b, _c] = three(&f).await;
    let id = postpaid(&f, &a, "2027-01-15 15:30").await;
    licence(&f, 1).await.unwrap();
    assert_eq!(
        refusal(begin(&f, &b, None, SessionMode::Postpaid, None).await),
        (ErrorCode::DeviceOverCap, Some("device_id".into()))
    );
    let mut i = super::stock::intent(
        owner(),
        IntentPayload::SessionTransfer(SessionTransfer {
            session_id: id.clone(),
            device_id: b.clone(),
        }),
    );
    i.base_rev = Some(rev_of(&f, Table::Sessions, &id));
    assert_eq!(
        refusal(end(&f, i).await),
        (ErrorCode::DeviceOverCap, Some("device_id".into()))
    );
    assert_eq!(text(&row_of(&f, Table::Sessions, &id), "device_id"), a);
}

#[tokio::test]
async fn a_revoke_or_a_higher_cap_frees_the_oldest_marked_device_first() {
    let f = bootstrapped().await;
    let [a, b, c] = three(&f).await;
    licence(&f, 2).await.unwrap();
    assert!(!over(&f, &a) && !over(&f, &b) && over(&f, &c));
    let mut revoke = super::stock::intent(
        owner(),
        IntentPayload::DeviceRevoke(DeviceRevoke {
            device_id: a.clone(),
        }),
    );
    revoke.reason = IntentKind::DeviceRevoke.reasons().map(|r| r[0]);
    revoke.base_rev = Some(1);
    end(&f, revoke).await.unwrap();
    assert!(!over(&f, &b) && !over(&f, &c));
    licence(&f, 1).await.unwrap();
    assert!(!over(&f, &b) && over(&f, &c));
    licence(&f, 20).await.unwrap();
    assert!(!over(&f, &c));
}

#[tokio::test]
async fn the_expired_standing_recomputes_against_free_limits() {
    let f = bootstrapped().await;
    let [a, ..] = three(&f).await;
    licence(&f, 200).await.unwrap();
    let applied = submit(
        &f,
        system(),
        IntentPayload::LicenceApply(LicenceApply {
            standing: LicenceStanding::Expired,
            clock_wrong: false,
            tier: Tier::Free,
            flags: String::new(),
            device_cap: 2,
            user_cap: 10,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        applied
            .changes
            .iter()
            .filter(|ch| ch.tbl == "devices_meta")
            .count(),
        1
    );
    assert!(!over(&f, &a));
    assert_eq!(
        table(&f, Table::DevicesMeta)
            .iter()
            .filter(|r| int(r, "over_cap") == 1)
            .count(),
        1
    );
}

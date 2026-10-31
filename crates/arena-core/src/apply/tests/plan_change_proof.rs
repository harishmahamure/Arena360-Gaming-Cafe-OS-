//! Story 10.20: the 10-session test café goes Premium -> Basic -> Free and nothing running ends.
//!
//! The licence is signed and evaluated the way the heartbeat does it (`Entitlement::evaluate` on
//! `trusted_now`), then handed to the writer as the system `licence.apply` intent. Ads belong to
//! the Free tier on Android, so "ads are back" is the effective tier being Free.

use super::kitchen_ticket::{advance, tickets};
use super::order_line::{burger, place, place_of};
use super::prepaid::{begin, group_of, system};
use super::session::{end, end_intent, int, pc, refusal, rev_of, sessions, submit, table, text};
use super::stock::owner;
use super::transfer::{online, postpaid, spare};
use super::*;
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LicenceClaims, dev_sign};
use arena_proto::{DeviceKind, Entitlement, LicenceStanding, SessionMode, Tier};

const TENANT: &str = "TEN1";
const BRANCH: &str = "BR1";
const DEVICES: usize = 24;
const SESSIONS: usize = 10;
const WEEK_S: i64 = 7 * 24 * 60 * 60;

/// The licence the cloud signs for `tier` at `iat_s`, valid for a week.
fn signed(tier: Tier, iat_s: i64) -> String {
    let claims = LicenceClaims::for_tier(TENANT, BRANCH, tier, iat_s);
    dev_sign(&claims).unwrap()
}

fn evaluate(jwt: &str, trusted_now_ms: i64) -> Entitlement {
    Entitlement::evaluate(
        Some(jwt),
        DEV_LICENCE_VERIFY_KEYS,
        TENANT,
        BRANCH,
        trusted_now_ms,
    )
}

async fn apply(f: &Fixture, e: &Entitlement) -> Applied {
    submit(
        f,
        system(),
        IntentPayload::LicenceApply(e.apply_payload(false)),
    )
    .await
    .unwrap()
}

/// Writes a proof's figures to `$ARENA_PROOF_OUT/<name>.json` when the release bench asks for it.
fn record_proof(name: &str, value: &serde_json::Value) {
    if let Ok(dir) = std::env::var("ARENA_PROOF_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/{name}.json"), value.to_string()).unwrap();
    }
}

/// Ads show only on a Hub whose effective tier is Free (the shared UI's `adsEnabled`).
fn ads_on(e: &Entitlement) -> bool {
    e.tier == Tier::Free
}

fn over_cap(f: &Fixture) -> Vec<String> {
    let mut ids: Vec<String> = table(f, Table::DevicesMeta)
        .iter()
        .filter(|r| int(r, "over_cap") == 1)
        .map(|r| text(r, "device_id"))
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_premium_cafe_downgraded_to_basic_and_lapsed_to_free_ends_nothing_it_was_running() {
    let f = bootstrapped().await;
    let now_s = f.clock.now_ms().div_euclid(1000);

    // Premium: 24 PCs, oldest pairing first, all online.
    let premium = signed(Tier::Premium, now_s - 60);
    let e = evaluate(&premium, f.clock.now_ms());
    assert_eq!(
        (e.tier, e.standing),
        (Tier::Premium, LicenceStanding::Verified)
    );
    assert!(!ads_on(&e));
    apply(&f, &e).await;

    let first = pc(&f, DeviceKind::Pc, "pc", Some(6_000)).await;
    let group = group_of(&f, &first);
    let mut devices = vec![first];
    for _ in 1..DEVICES {
        devices.push(spare(&f, Some(&group), DeviceKind::Pc).await);
    }
    let refs: Vec<&str> = devices.iter().map(String::as_str).collect();
    online(&f, &refs).await;
    assert!(over_cap(&f).is_empty());

    // Ten live sessions on the ten newest pairings, and two open kitchen tickets.
    let live = &devices[DEVICES - SESSIONS..];
    let mut ids = Vec::new();
    for d in live {
        ids.push(postpaid(&f, d, "2027-01-15 13:30").await);
    }
    assert_eq!(sessions(&f).len(), SESSIONS);
    let b = burger(&f, 10).await;
    for s in &ids[..2] {
        place(&f, owner(), place_of(s, &[(&b, 1)])).await.unwrap();
    }
    assert_eq!(tickets(&f).len(), 2);
    let running = (
        table(&f, Table::Sessions),
        table(&f, Table::Orders),
        tickets(&f),
    );

    // Premium -> Basic: the cap is 60, nothing is over it, nothing running changes.
    let basic = signed(Tier::Basic, now_s - 30);
    let e = evaluate(&basic, f.clock.now_ms());
    assert_eq!(e.tier, Tier::Basic);
    assert!(!ads_on(&e));
    apply(&f, &e).await;
    assert!(over_cap(&f).is_empty());
    assert_eq!(
        (
            table(&f, Table::Sessions),
            table(&f, Table::Orders),
            tickets(&f)
        ),
        running
    );

    // The paid licence is still in force a second before it ends: still no ads, same limits.
    let exp_ms = (now_s - 30 + WEEK_S) * 1000;
    f.clock.set(exp_ms - 1_000);
    let e = evaluate(&basic, exp_ms - 1_000);
    assert_eq!(
        (e.tier, e.standing),
        (Tier::Basic, LicenceStanding::Verified)
    );
    assert!(!ads_on(&e));

    // Basic lapses to Free at `exp` on trusted_now: ads are back, the cap is 20.
    f.clock.set(exp_ms);
    let e = evaluate(&basic, exp_ms);
    assert_eq!((e.tier, e.standing), (Tier::Free, LicenceStanding::Expired));
    assert!(ads_on(&e));
    let applied = apply(&f, &e).await;
    assert!(applied.changes.iter().all(|c| c.tbl != "sessions"));

    // The four newest pairings are over the cap; every session is exactly as it was.
    let expect: Vec<String> = {
        let mut v = devices[20..].to_vec();
        v.sort();
        v
    };
    assert_eq!(over_cap(&f), expect);
    let sessions_after = table(&f, Table::Sessions);
    assert_eq!(sessions_after.len(), SESSIONS);
    assert!(
        sessions_after
            .iter()
            .all(|s| text(s, "state") == "running" || text(s, "state") == "active"),
        "no session ended"
    );
    assert_eq!(table(&f, Table::Orders), running.1);

    // The two open tickets still finish; a new kitchen order opens no new ticket on Free.
    let open: Vec<String> = tickets(&f).iter().map(|t| text(t, "id")).collect();
    for t in &open {
        for to in ["preparing", "ready", "served"] {
            advance(&f, t, to).await.unwrap();
        }
    }
    place(&f, owner(), place_of(&ids[2], &[(&b, 1)]))
        .await
        .unwrap();
    assert_eq!(tickets(&f).len(), 2);

    // An over-cap PC locks only after its session ends.
    let over_device = devices[DEVICES - 1].clone();
    let over_session = ids.last().unwrap().clone();
    let rev = rev_of(&f, Table::Sessions, &over_session);
    end(&f, end_intent(&over_session, Some(rev))).await.unwrap();
    assert_eq!(
        refusal(begin(&f, &over_device, None, SessionMode::Postpaid, None).await),
        (ErrorCode::DeviceOverCap, Some("device_id".into()))
    );
    // A PC inside the cap starts normally; the other three marked PCs keep running.
    let inside = &devices[0];
    begin(&f, inside, None, SessionMode::Postpaid, None)
        .await
        .unwrap();
    assert_eq!(over_cap(&f), expect);
    record_proof(
        "downgrade",
        &serde_json::json!({
            "sessions": SESSIONS,
            "sessions_ended": 0,
            "tickets_open": 2,
            "devices": DEVICES,
            "over_cap": expect.len(),
            "ads_before_exp": false,
            "ads_at_exp": true,
        }),
    );
}

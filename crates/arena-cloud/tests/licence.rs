use std::path::Path;
use std::process::{Command, Output, Stdio};

use arena_cloud::licence::{self, DEV_LICENCE_KID, LicenceSigner, claims_for};
use arena_proto::licence::{DEV_LICENCE_VERIFY_KEYS, LICENCE_TTL_S, LICENCE_VERIFY_KEYS, verify};
use arena_proto::{EntitlementSource, LicenceClaims, LicenceError, Tier, VerifyKey};
use arena_store::control::{ControlDb, Entitlement};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

const NOW: i64 = 1_800_000_000_000;

fn entitlement(tier: Tier, source: EntitlementSource, until: Option<i64>) -> Entitlement {
    Entitlement {
        branch_id: "b1".into(),
        tier,
        source,
        addons: vec![],
        issued_at: NOW,
        until,
        updated_at: NOW,
    }
}

fn key_of<'a>(kid: &'a str, public_key: &'a str) -> VerifyKey<'a> {
    VerifyKey { kid, public_key }
}

#[test]
fn the_dev_signer_matches_the_dev_verify_key() {
    let dev = LicenceSigner::dev();
    assert_eq!(DEV_LICENCE_KID, arena_proto::licence::DEV_LICENCE_KID);
    assert_eq!(dev.kid(), DEV_LICENCE_VERIFY_KEYS[0].kid);
    assert_eq!(dev.public_key(), DEV_LICENCE_VERIFY_KEYS[0].public_key);
}

#[test]
fn claims_follow_the_tier_table_for_each_tier_and_source() {
    let cases = [
        (None, Tier::Free),
        (
            Some(entitlement(Tier::Free, EntitlementSource::Free, None)),
            Tier::Free,
        ),
        (
            Some(entitlement(
                Tier::Basic,
                EntitlementSource::Comp,
                Some(NOW + 30 * 86_400_000),
            )),
            Tier::Basic,
        ),
        (
            Some(entitlement(
                Tier::Premium,
                EntitlementSource::Comp,
                Some(NOW + 30 * 86_400_000),
            )),
            Tier::Premium,
        ),
        (
            Some(entitlement(
                Tier::Premium,
                EntitlementSource::Comp,
                Some(NOW - 1),
            )),
            Tier::Free,
        ),
        (
            Some(entitlement(Tier::Basic, EntitlementSource::Paid, None)),
            Tier::Basic,
        ),
    ];
    for (e, want) in cases {
        let c = claims_for("t1", "b1", e.as_ref(), NOW);
        let d = want.defaults();
        assert_eq!(c.tier, want, "{e:?}");
        assert_eq!((c.tenant_id.as_str(), c.branch_id.as_str()), ("t1", "b1"));
        assert_eq!(
            c.flags,
            d.flags.iter().map(|f| f.to_string()).collect::<Vec<_>>()
        );
        assert_eq!(c.features.len(), d.features.len());
        assert_eq!(c.limits, d.limits);
        assert_eq!(
            (
                c.upload_interval_s,
                c.summary_interval_s,
                c.rollup_schema,
                c.retention_days
            ),
            (
                d.upload_interval_s,
                d.summary_interval_s,
                d.rollup_schema,
                d.retention_days
            )
        );
        assert_eq!(c.iat, NOW / 1000);
        assert_eq!(c.exp, c.iat + 7 * 24 * 3600);
    }
    let free = claims_for("t1", "b1", None, NOW);
    assert_eq!(
        (free.limits.devices, free.limits.users, free.limits.branches),
        (20, 3, 1)
    );
    assert_eq!((free.upload_interval_s, free.summary_interval_s), (0, 0));
    assert!(free.flags.is_empty());
}

#[test]
fn a_paid_entitlements_add_ons_raise_the_device_limit_and_add_the_tunnel() {
    let paid = |tier: Tier, addons: &[&str]| {
        let mut e = entitlement(tier, EntitlementSource::Paid, None);
        e.addons = addons.iter().map(|a| (*a).to_owned()).collect();
        claims_for("t1", "b1", Some(&e), NOW)
    };
    let plain = paid(Tier::Basic, &[]);
    assert_eq!(plain.limits.devices, 60);
    assert!(!plain.flags.iter().any(|f| f == "tunnel"));
    let with = paid(Tier::Basic, &["devices_20:3", "tunnel"]);
    assert_eq!(with.limits.devices, 120);
    assert!(with.flags.iter().any(|f| f == "tunnel"));
    assert_eq!(with.flags.iter().filter(|f| *f == "tunnel").count(), 1);
    // Premium already has the tunnel; unknown, malformed and oversized add-ons change nothing
    // beyond the ten-pack ceiling.
    let premium = paid(
        Tier::Premium,
        &["tunnel", "owner_login:1", "devices_20:x", "mystery"],
    );
    assert_eq!(premium.limits.devices, 200);
    assert_eq!(premium.flags.iter().filter(|f| *f == "tunnel").count(), 1);
    assert_eq!(
        paid(Tier::Basic, &["devices_20:99"]).limits.devices,
        60 + 200
    );
    // Free never takes add-ons.
    let mut free = entitlement(Tier::Free, EntitlementSource::Free, None);
    free.addons = vec!["devices_20:5".to_owned()];
    assert_eq!(claims_for("t1", "b1", Some(&free), NOW).limits.devices, 20);
}

#[test]
fn a_dev_licence_verifies_and_its_header_names_the_kid() {
    let claims = claims_for("t1", "b1", None, NOW);
    let token = LicenceSigner::dev().sign(&claims).unwrap();
    let header = jsonwebtoken::decode_header(&token).unwrap();
    assert_eq!(header.alg, jsonwebtoken::Algorithm::EdDSA);
    assert_eq!(header.kid.as_deref(), Some(DEV_LICENCE_KID));
    assert_eq!(
        verify(&token, DEV_LICENCE_VERIFY_KEYS, NOW).unwrap(),
        claims
    );
}

#[test]
fn both_slots_verify_and_anything_else_is_refused() {
    let current = licence::generate(Some("current")).unwrap();
    let next = licence::generate(Some("next")).unwrap();
    let other = licence::generate(Some("other")).unwrap();
    let set = [
        key_of(&current.kid, &current.public_key),
        key_of(&next.kid, &next.public_key),
    ];
    let claims = claims_for(
        "t1",
        "b1",
        Some(&entitlement(Tier::Premium, EntitlementSource::Comp, None)),
        NOW,
    );
    for g in [&current, &next] {
        let token = LicenceSigner::from_pem(&g.kid, &g.pem)
            .unwrap()
            .sign(&claims)
            .unwrap();
        assert_eq!(verify(&token, &set, NOW).unwrap(), claims, "{}", g.kid);
    }

    let foreign = LicenceSigner::from_pem(&other.kid, &other.pem)
        .unwrap()
        .sign(&claims)
        .unwrap();
    assert_eq!(
        verify(&foreign, &set, NOW).unwrap_err(),
        LicenceError::UnknownKid
    );
    let impostor = LicenceSigner::from_pem("current", &other.pem)
        .unwrap()
        .sign(&claims)
        .unwrap();
    assert_eq!(
        verify(&impostor, &set, NOW).unwrap_err(),
        LicenceError::BadSignature
    );

    let good = LicenceSigner::from_pem(&current.kid, &current.pem)
        .unwrap()
        .sign(&claims)
        .unwrap();
    let parts: Vec<&str> = good.split('.').collect();
    let mut payload: LicenceClaims =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    payload.limits.devices = 9_999;
    let edited = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()),
        parts[2]
    );
    assert_eq!(
        verify(&edited, &set, NOW).unwrap_err(),
        LicenceError::BadSignature
    );
    let mut downgraded = payload.clone();
    downgraded.limits = claims.limits;
    downgraded.tier = Tier::Free;
    let edited = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&downgraded).unwrap()),
        parts[2]
    );
    assert_eq!(
        verify(&edited, &set, NOW).unwrap_err(),
        LicenceError::BadSignature
    );

    let exp_ms = claims.exp * 1000;
    assert!(verify(&good, &set, exp_ms - 1).is_ok());
    assert_eq!(
        verify(&good, &set, exp_ms).unwrap_err(),
        LicenceError::Expired
    );
    assert_eq!(
        verify(&good, &set, NOW - 3_600_000).unwrap_err(),
        LicenceError::NotYetValid
    );

    let e = verify(&good, LICENCE_VERIFY_KEYS, NOW).unwrap_err();
    assert_eq!(e, LicenceError::NoKeys);
    assert!(e.to_string().contains("no licence verify keys"));
    assert_eq!(verify(&good, &[], NOW).unwrap_err(), LicenceError::NoKeys);
}

#[test]
fn a_token_living_longer_than_seven_days_is_refused() {
    let mut claims = claims_for("t1", "b1", None, NOW);
    claims.exp = claims.iat + LICENCE_TTL_S + 1;
    let token = LicenceSigner::dev().sign(&claims).unwrap();
    assert_eq!(
        verify(&token, DEV_LICENCE_VERIFY_KEYS, NOW).unwrap_err(),
        LicenceError::Malformed
    );
}

fn cloud() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arena-cloud"));
    c.env_remove("ARENA_CONTROL_DB_URL")
        .env_remove("ARENA_CONTROL_DB_TOKEN")
        .stdin(Stdio::null());
    c
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

async fn seeded(dir: &Path) -> ControlDb {
    let db = ControlDb::open_local(&dir.join("gz_control.db"))
        .await
        .unwrap();
    db.migrate(NOW).await.unwrap();
    let c = rusqlite::Connection::open(dir.join("gz_control.db")).unwrap();
    c.execute_batch(
        "PRAGMA foreign_keys = OFF;
         INSERT INTO tenants (id, owner_id, name, created_at) VALUES ('t1', 'o1', 'Pixel', 1);
         INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone,
           gst_registered, gstin, branch_code, code_locked_at, created_at, updated_at)
           VALUES ('b1', 't1', 'Pixel Arena', 'Pune', 'IN', 'INR', 'Asia/Kolkata', 0, NULL,
           'PAG', NULL, 1, 1);
         INSERT INTO onboarding_progress (branch_id, step, state, completed_at) VALUES
           ('b1', 'cafe', 'done', 1), ('b1', 'tier', 'pending', NULL);",
    )
    .unwrap();
    db
}

fn grant(dir: &Path, branch: &str, tier: &str, until: &str) -> Output {
    cloud()
        .args(["ops", "grant-comp", "--branch", branch, "--tier", tier])
        .args([
            "--until",
            until,
            "--operator",
            "harish",
            "--reason",
            "beta café",
        ])
        .arg("--dev")
        .arg("--data-dir")
        .arg(dir)
        .output()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_comp_writes_the_entitlement_the_audit_and_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded(dir.path()).await;
    let o = grant(dir.path(), "b1", "premium", "2099-12-31");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("granted comp premium to branch b1"), "{out}");
    assert!(out.contains("devices 200, users 50, branches 1"), "{out}");
    assert!(out.contains("wizard step tier: done"), "{out}");

    let e = db.entitlement("b1").await.unwrap().unwrap();
    assert_eq!((e.tier, e.source), (Tier::Premium, EntitlementSource::Comp));
    assert_eq!(e.until, arena_cloud::config::end_of_ist_day("2099-12-31"));
    let audit = db.ops_audit_for("b1").await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(
        (
            audit[0].operator.as_str(),
            audit[0].action.as_str(),
            audit[0].reason.as_str()
        ),
        ("harish", "grant_comp", "beta café")
    );
    let details: serde_json::Value = serde_json::from_str(&audit[0].details_json).unwrap();
    assert_eq!(details["tier"], "premium");
    assert_eq!(details["until"], "2099-12-31");
    assert_eq!(details["previous"], serde_json::Value::Null);
    assert!(out.contains(&audit[0].id), "{out}");
    let progress = db.onboarding_progress("b1").await.unwrap();
    let tier = progress
        .iter()
        .find(|p| p.step == arena_proto::WizardStep::Tier)
        .unwrap();
    assert_eq!(tier.state, arena_proto::StepState::Done);

    let branch = db.branch("b1").await.unwrap().unwrap();
    let token = licence::sign_branch(&LicenceSigner::dev(), &branch, Some(&e), NOW).unwrap();
    let claims = verify(&token, DEV_LICENCE_VERIFY_KEYS, NOW).unwrap();
    assert_eq!((claims.tier, claims.limits.devices), (Tier::Premium, 200));
    assert_eq!(claims.tenant_id, "t1");

    let o = grant(dir.path(), "b1", "basic", "2099-06-30");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let audit = db.ops_audit_for("b1").await.unwrap();
    assert_eq!(audit.len(), 2);
    let details: serde_json::Value = serde_json::from_str(&audit[1].details_json).unwrap();
    assert_eq!(details["previous"]["tier"], "premium");
    let e = db.entitlement("b1").await.unwrap().unwrap();
    let claims = verify(
        &licence::sign_branch(&LicenceSigner::dev(), &branch, Some(&e), NOW).unwrap(),
        DEV_LICENCE_VERIFY_KEYS,
        NOW,
    )
    .unwrap();
    assert_eq!(claims.limits.devices, 60);
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_comp_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded(dir.path()).await;

    let o = grant(dir.path(), "nope", "premium", "2099-12-31");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("UNKNOWN_BRANCH"), "{}", text(&o));

    let o = grant(dir.path(), "b1", "premium", "2020-01-01");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("PAST_DATE"), "{}", text(&o));

    for (tier, until) in [("free", "2099-12-31"), ("premium", "2099-13-01")] {
        let o = grant(dir.path(), "b1", tier, until);
        assert_eq!(o.status.code(), Some(64), "{}", text(&o));
    }
    let o = cloud()
        .args(["ops", "grant-comp", "--branch", "b1", "--tier", "basic"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(64), "{}", text(&o));
    assert!(text(&o).contains("--until"), "{}", text(&o));

    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, "x").unwrap();
    let o = grant(&file, "b1", "premium", "2099-12-31");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("CONTROL_DB_FAILED"), "{}", text(&o));

    let o = cloud()
        .args(["ops", "grant-comp", "--branch", "b1", "--tier", "basic"])
        .args(["--until", "2099-12-31", "--operator", "h", "--reason", "r"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(64), "{}", text(&o));
    assert!(text(&o).contains("ARENA_CONTROL_DB_URL"), "{}", text(&o));

    assert_eq!(db.entitlement("b1").await.unwrap(), None);
    assert!(db.ops_audit_for("b1").await.unwrap().is_empty());
}

#[test]
fn licence_keygen_prints_a_usable_key() {
    let o = cloud()
        .args(["ops", "licence-keygen", "--kid", "prod-2026-10"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let pem = String::from_utf8(o.stdout.clone()).unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr);
    let signer = LicenceSigner::from_pem("prod-2026-10", &pem).unwrap();
    assert!(!signer.is_dev_key());
    assert!(
        stderr.contains("ARENA_LICENCE_KID=prod-2026-10"),
        "{stderr}"
    );
    let line = format!(
        "VerifyKey {{ kid: \"prod-2026-10\", public_key: \"{}\" }},",
        signer.public_key()
    );
    assert!(stderr.contains(&line), "{stderr}");
    assert!(!stderr.contains("PRIVATE"), "{stderr}");
    let token = signer.sign(&claims_for("t", "b", None, NOW)).unwrap();
    let pk = signer.public_key();
    assert!(verify(&token, &[key_of("prod-2026-10", &pk)], NOW).is_ok());

    let o = cloud()
        .args(["ops", "licence-keygen", "--kid", "bad kid"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(64), "{}", text(&o));
}

#[test]
fn a_comp_licence_never_outlives_its_until() {
    let until = NOW + 3_600_000;
    let c = claims_for(
        "t1",
        "b1",
        Some(&entitlement(
            Tier::Premium,
            EntitlementSource::Comp,
            Some(until),
        )),
        NOW,
    );
    assert_eq!(c.tier, Tier::Premium);
    assert_eq!(c.exp, until / 1000);
    let far = NOW + 30 * 86_400_000;
    let c = claims_for(
        "t1",
        "b1",
        Some(&entitlement(
            Tier::Basic,
            EntitlementSource::Comp,
            Some(far),
        )),
        NOW,
    );
    assert_eq!(c.exp, c.iat + LICENCE_TTL_S);
    let token = LicenceSigner::dev()
        .sign(&claims_for(
            "t1",
            "b1",
            Some(&entitlement(
                Tier::Premium,
                EntitlementSource::Comp,
                Some(until),
            )),
            NOW,
        ))
        .unwrap();
    assert!(verify(&token, DEV_LICENCE_VERIFY_KEYS, until - 1000).is_ok());
    assert_eq!(
        verify(&token, DEV_LICENCE_VERIFY_KEYS, until + 1000).unwrap_err(),
        LicenceError::Expired
    );
}

#[test]
fn iat_may_lead_the_clock_by_up_to_five_minutes() {
    let now_s = NOW / 1000;
    let signer = LicenceSigner::dev();
    let at = |lead: i64| {
        let token = signer
            .sign(&LicenceClaims::for_tier(
                "t1",
                "b1",
                Tier::Free,
                now_s + lead,
            ))
            .unwrap();
        verify(&token, DEV_LICENCE_VERIFY_KEYS, NOW)
    };
    assert!(at(299).is_ok());
    assert!(at(300).is_ok());
    assert_eq!(at(301).unwrap_err(), LicenceError::NotYetValid);
}

#[test]
fn hostile_claims_do_not_overflow() {
    let mut claims = claims_for("t1", "b1", None, NOW);
    claims.iat = i64::MIN;
    claims.exp = i64::MAX;
    let token = LicenceSigner::dev().sign(&claims).unwrap();
    assert_eq!(
        verify(&token, DEV_LICENCE_VERIFY_KEYS, NOW).unwrap_err(),
        LicenceError::Malformed
    );
    let mut claims = claims_for("t1", "b1", None, NOW);
    claims.iat = i64::MAX - 10;
    claims.exp = i64::MAX;
    let token = LicenceSigner::dev().sign(&claims).unwrap();
    assert_eq!(
        verify(&token, DEV_LICENCE_VERIFY_KEYS, i64::MAX).unwrap_err(),
        LicenceError::NotYetValid
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn grant_comp_refuses_to_replace_a_paid_plan() {
    let dir = tempfile::tempdir().unwrap();
    let db = seeded(dir.path()).await;
    rusqlite::Connection::open(dir.path().join("gz_control.db"))
        .unwrap()
        .execute(
            "INSERT INTO entitlements (branch_id, tenant_id, tier, source, issued_at, until, \
             updated_at) VALUES ('b1', 't1', 'basic', 'paid', 1, NULL, 1)",
            [],
        )
        .unwrap();
    let o = grant(dir.path(), "b1", "premium", "2099-12-31");
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("PAID_PLAN"), "{}", text(&o));
    assert!(text(&o).contains("paid basic plan"), "{}", text(&o));
    let e = db.entitlement("b1").await.unwrap().unwrap();
    assert_eq!((e.tier, e.source), (Tier::Basic, EntitlementSource::Paid));
    assert!(db.ops_audit_for("b1").await.unwrap().is_empty());
}

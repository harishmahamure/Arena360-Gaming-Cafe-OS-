//! Story 6.4: `kiosk.scan_reported` stores a kiosk's latest installed-software scan.

use super::session::{int, refusal, table, text};
use super::*;
use arena_proto::{KioskScanReported, ScanEntry, ScanLauncher};

fn entry(path: &str) -> ScanEntry {
    ScanEntry {
        path: path.into(),
        file_name: "cs2.exe".into(),
        product_name: "Counter-Strike 2".into(),
        publisher: "Valve".into(),
        launcher: "steam".into(),
    }
}

fn report(at: i64, paths: &[&str]) -> KioskScanReported {
    KioskScanReported {
        scanned_at: at,
        entries: paths.iter().map(|p| entry(p)).collect(),
        launchers: vec![ScanLauncher {
            launcher: "steam".into(),
            path: r"C:\Program Files (x86)\Steam".into(),
        }],
        skipped: 2,
    }
}

fn scan_intent(device_id: &str, r: KioskScanReported) -> Intent {
    let mut i = device_intent(ActorKind::Device, IntentPayload::KioskScanReported(r));
    i.actor.id = device_id.into();
    i
}

fn scans(f: &Fixture) -> Vec<Row> {
    table(f, Table::KioskScans)
}

#[tokio::test]
async fn the_first_report_inserts_a_row_and_a_newer_one_replaces_it() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;

    let first = scan_intent(
        &id,
        report(T0 - 5000, &[r"C:\Games\a.exe", r"C:\Games\b.exe"]),
    );
    let applied = f.writer.submit_intent(first.clone()).await.unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(
        (applied.changes[0].tbl.as_str(), applied.changes[0].op),
        ("kiosk_scans", ChangeOp::Insert)
    );
    assert_eq!(applied.changes[0].row_id, id);
    let rows = scans(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "device_id"), id);
    assert_eq!(int(&rows[0], "scanned_at"), T0 - 5000);
    assert_eq!(int(&rows[0], "skipped"), 2);
    assert_eq!(int(&rows[0], "rev"), 1);
    let entries: Vec<ScanEntry> = serde_json::from_str(&text(&rows[0], "entries")).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], entry(r"C:\Games\a.exe"));
    let launchers: Vec<ScanLauncher> = serde_json::from_str(&text(&rows[0], "launchers")).unwrap();
    assert_eq!(launchers[0].launcher, "steam");

    // A replay (same key) applies once: the stored outcome, no new change.
    let changes = reopen(&f).changes().unwrap().len();
    assert_eq!(f.writer.submit_intent(first).await.unwrap(), applied);
    assert_eq!(reopen(&f).changes().unwrap().len(), changes);
    assert_eq!(int(&scans(&f)[0], "rev"), 1);

    // A newer one replaces it: rev 2, the old entries gone.
    let second = scan_intent(&id, report(T0 - 1000, &[r"C:\Games\c.exe"]));
    let applied = f.writer.submit_intent(second).await.unwrap();
    assert_eq!(applied.changes[0].op, ChangeOp::Update);
    let rows = scans(&f);
    assert_eq!(rows.len(), 1, "one row per device");
    assert_eq!(int(&rows[0], "rev"), 2);
    assert_eq!(int(&rows[0], "scanned_at"), T0 - 1000);
    let entries: Vec<ScanEntry> = serde_json::from_str(&text(&rows[0], "entries")).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        [r"C:\Games\c.exe"]
    );
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(
        (change.intent_kind.as_str(), change.actor_kind.as_str()),
        ("kiosk.scan_reported", "device")
    );
}

#[tokio::test]
async fn a_report_not_newer_than_the_stored_one_is_refused_and_the_stored_scan_stays() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    f.writer
        .submit_intent(scan_intent(&id, report(T0 - 2000, &[r"C:\Games\a.exe"])))
        .await
        .unwrap();
    for at in [T0 - 2000, T0 - 3000] {
        let r = f
            .writer
            .submit_intent(scan_intent(&id, report(at, &[r"C:\Games\z.exe"])))
            .await;
        assert_eq!(
            refusal(r),
            (ErrorCode::ValidationFailed, Some("scanned_at".into())),
            "{at}"
        );
    }
    let rows = scans(&f);
    assert_eq!(int(&rows[0], "scanned_at"), T0 - 2000);
    assert_eq!(int(&rows[0], "rev"), 1);
    assert!(text(&rows[0], "entries").contains("a.exe"));
}

#[tokio::test]
async fn each_device_has_its_own_row() {
    let mut f = primary();
    let (a, _) = with_device(&mut f).await;
    let secret = crate::devices::new_token_secret().unwrap();
    let hash = crate::devices::hash_token_secret(&secret).unwrap();
    let applied = f
        .writer
        .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
        .await
        .unwrap();
    let b = applied.changes[0].row_id.clone();
    f.writer
        .submit_intent(scan_intent(&a, report(T0 - 10, &[r"C:\Games\a.exe"])))
        .await
        .unwrap();
    // The other PC's older time is not stale against this PC's scan.
    f.writer
        .submit_intent(scan_intent(&b, report(T0 - 20, &[r"C:\Games\b.exe"])))
        .await
        .unwrap();
    assert_eq!(scans(&f).len(), 2);
}

#[tokio::test]
async fn a_bad_payload_writes_nothing_and_names_the_field() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    let many: Vec<String> = (0..501).map(|i| format!(r"C:\Games\{i}.exe")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let mut control = report(T0 - 1, &[r"C:\Games\a.exe"]);
    control.entries[0].product_name = "bell\u{7}".into();
    let mut empty_path = report(T0 - 1, &[r"C:\Games\a.exe"]);
    empty_path.entries[0].path = String::new();
    let mut launcher = report(T0 - 1, &[r"C:\Games\a.exe"]);
    launcher.launchers[0].launcher = "gog".into();
    for (r, field) in [
        (report(T0 - 1, &many), "entries"),
        (control, "entries[0].product_name"),
        (empty_path, "entries[0].path"),
        (launcher, "launchers[0].launcher"),
        (report(0, &[]), "scanned_at"),
        // After the intent's own time.
        (report(T0 + 1, &[]), "scanned_at"),
    ] {
        let err = expect_err(&f, scan_intent(&id, r)).await;
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert_eq!(err.field.as_deref(), Some(field));
    }
    let mut edit = scan_intent(&id, report(T0 - 1, &[]));
    edit.base_rev = Some(1);
    assert_eq!(
        expect_err(&f, edit).await.field.as_deref(),
        Some("base_rev")
    );
    assert!(scans(&f).is_empty());
}

#[tokio::test]
async fn only_a_live_device_may_report() {
    let mut f = primary();
    let (id, _) = with_device(&mut f).await;
    for actor in [ActorKind::Owner, ActorKind::System, ActorKind::Staff] {
        let mut i = scan_intent(&id, report(T0 - 1, &[]));
        i.actor.kind = actor;
        assert_eq!(
            expect_err(&f, i).await.code,
            ErrorCode::PermissionDenied,
            "{actor:?}"
        );
    }
    let err = expect_err(
        &f,
        scan_intent("01JNOPEAAAAAAAAAAAAAAAAAAA", report(T0 - 1, &[])),
    )
    .await;
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    // A reason is not accepted.
    let mut with_reason = scan_intent(&id, report(T0 - 1, &[]));
    with_reason.reason = Some(arena_proto::ReasonCode::Other);
    assert!(f.writer.submit_intent(with_reason).await.is_err());
    assert!(scans(&f).is_empty());
}

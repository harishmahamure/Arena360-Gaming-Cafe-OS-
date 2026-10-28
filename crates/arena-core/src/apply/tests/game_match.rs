//! Story 6.5: `game.matched`, per-device installs on `library.upsert`, and the dismissed
//! proposals.

use super::session::{int, refusal, table, text};
use super::stock::{intent, owner};
use super::*;
use arena_proto::{
    GameMatch, GameMatched, KioskLibraryEntry, KioskScanReported, LibraryDismiss,
    LibraryDismissClear, LibraryInstall, LibraryUpsert, ProcessRule, ScanEntry, proposal_key,
};

const SCAN_AT: i64 = T0 - 5000;

fn system() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "hub-lan".into(),
    }
}

fn scan_entry(path: &str, product: &str) -> ScanEntry {
    ScanEntry {
        path: path.into(),
        file_name: path.rsplit('\\').next().unwrap().into(),
        product_name: product.into(),
        publisher: "Maker".into(),
        launcher: "none".into(),
    }
}

/// A second paired device (the first comes from `with_device`).
async fn another_device(f: &Fixture) -> String {
    let hash = crate::devices::hash_token_secret("s2").unwrap();
    f.writer
        .submit_intent(device_intent(ActorKind::Owner, register(&hash)))
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone()
}

async fn scan(f: &Fixture, device: &str, at: i64, entries: Vec<ScanEntry>) {
    let mut i = device_intent(
        ActorKind::Device,
        IntentPayload::KioskScanReported(KioskScanReported {
            scanned_at: at,
            entries,
            launchers: Vec::new(),
            skipped: 0,
        }),
    );
    i.actor.id = device.into();
    f.writer.submit_intent(i).await.unwrap();
}

fn matched(device: &str, at: i64, pairs: &[(&str, &str)]) -> GameMatched {
    GameMatched {
        device_id: device.into(),
        scanned_at: at,
        matches: pairs
            .iter()
            .map(|(c, p)| GameMatch {
                catalog_id: (*c).into(),
                path: (*p).into(),
            })
            .collect(),
    }
}

async fn record(f: &Fixture, m: GameMatched) -> Result<Applied, AppError> {
    f.writer
        .submit_intent(intent(system(), IntentPayload::GameMatched(m)))
        .await
}

fn upsert(name: &str, path: &str) -> LibraryUpsert {
    LibraryUpsert {
        id: None,
        entry_kind: "game".into(),
        name: name.into(),
        launcher: "none".into(),
        executable_path: path.into(),
        launch_uri: String::new(),
        launch_args: String::new(),
        category: String::new(),
        tags: Vec::new(),
        age_rating: String::new(),
        enabled: true,
        catalog_id: None,
        installs: Vec::new(),
    }
}

fn install(device: &str, path: &str, key: &str) -> LibraryInstall {
    LibraryInstall {
        device_id: device.into(),
        path: path.into(),
        match_key: key.into(),
    }
}

async fn save(f: &Fixture, u: LibraryUpsert, base_rev: Option<i64>) -> Result<Applied, AppError> {
    let mut i = intent(owner(), IntentPayload::LibraryUpsert(u));
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

fn entry_id(a: &Applied) -> String {
    a.changes
        .iter()
        .find(|c| c.tbl == "library_entries")
        .unwrap()
        .row_id
        .clone()
}

fn installs(f: &Fixture) -> Vec<Row> {
    table(f, Table::LibraryInstalls)
}

fn library(f: &Fixture) -> Vec<KioskLibraryEntry> {
    let row = reopen(f)
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text("kiosk.library".into()))
        .unwrap();
    serde_json::from_str(&text(&row, "value")).unwrap()
}

fn allow(f: &Fixture) -> Vec<String> {
    let row = reopen(f)
        .rows(Table::Settings)
        .unwrap()
        .into_iter()
        .find(|r| r["key"] == RowValue::Text("kiosk.process_allow".into()))
        .unwrap();
    serde_json::from_str::<Vec<ProcessRule>>(&text(&row, "value"))
        .unwrap()
        .into_iter()
        .map(|r| r.path_glob)
        .collect()
}

fn tables_of(a: &Applied) -> Vec<&str> {
    a.changes.iter().map(|c| c.tbl.as_str()).collect()
}

#[tokio::test]
async fn matches_replace_the_devices_row_and_need_a_current_scan() {
    let mut f = primary();
    let (d, _) = with_device(&mut f).await;
    let a = r"C:\Games\a.exe";
    let b = r"C:\Games\b.exe";
    // No scan yet.
    assert_eq!(
        refusal(record(&f, matched(&d, SCAN_AT, &[])).await),
        (ErrorCode::ValidationFailed, Some("device_id".into()))
    );
    scan(
        &f,
        &d,
        SCAN_AT,
        vec![scan_entry(a, "A"), scan_entry(b, "B")],
    )
    .await;

    let first = record(&f, matched(&d, SCAN_AT, &[("G1", a)]))
        .await
        .unwrap();
    assert_eq!(
        (first.changes[0].tbl.as_str(), first.changes[0].op),
        ("game_matches", ChangeOp::Insert)
    );
    assert_eq!(first.changes.len(), 1, "no library entry to update");
    let rows = table(&f, Table::GameMatches);
    assert_eq!((rows.len(), int(&rows[0], "rev")), (1, 1));
    assert_eq!(text(&rows[0], "device_id"), d);
    let stored: Vec<GameMatch> = serde_json::from_str(&text(&rows[0], "matches")).unwrap();
    assert_eq!(stored[0].catalog_id, "G1");

    // The same matches for the same scan are refused; different ones replace the row.
    assert_eq!(
        refusal(record(&f, matched(&d, SCAN_AT, &[("G1", a)])).await),
        (ErrorCode::ValidationFailed, Some("matches".into()))
    );
    let second = record(&f, matched(&d, SCAN_AT, &[("G2", b)]))
        .await
        .unwrap();
    assert_eq!(second.changes[0].op, ChangeOp::Update);
    let rows = table(&f, Table::GameMatches);
    assert_eq!(int(&rows[0], "rev"), 2);
    assert!(text(&rows[0], "matches").contains("G2") && !text(&rows[0], "matches").contains("G1"));
    let change = reopen(&f).changes().unwrap().pop().unwrap();
    assert_eq!(
        (change.intent_kind.as_str(), change.actor_kind.as_str()),
        ("game.matched", "system")
    );
}

#[tokio::test]
async fn bad_matches_are_refused_and_write_nothing() {
    let mut f = primary();
    let (d, _) = with_device(&mut f).await;
    scan(&f, &d, SCAN_AT, vec![scan_entry(r"C:\Games\a.exe", "A")]).await;
    let before = reopen(&f).changes().unwrap().len();
    for (m, field) in [
        // A scan other than the stored one.
        (matched(&d, SCAN_AT + 1, &[]), "scanned_at"),
        // A path the scan did not find.
        (
            matched(&d, SCAN_AT, &[("G1", r"C:\Games\zzz.exe")]),
            "matches[0].path",
        ),
        // An unknown device.
        (matched("NOPE", SCAN_AT, &[]), "device_id"),
        // Not a path.
        (matched(&d, SCAN_AT, &[("G1", "a.exe")]), "matches[0].path"),
    ] {
        assert_eq!(
            refusal(record(&f, m).await),
            (ErrorCode::ValidationFailed, Some(field.into())),
            "{field}"
        );
    }
    // Only the system records matches: not the owner, not a device.
    for actor in [
        owner(),
        Actor {
            kind: ActorKind::Device,
            id: d.clone(),
        },
    ] {
        let i = intent(
            actor,
            IntentPayload::GameMatched(matched(&d, SCAN_AT, &[("G1", r"C:\Games\a.exe")])),
        );
        assert_eq!(
            refusal(f.writer.submit_intent(i).await).0,
            ErrorCode::PermissionDenied
        );
    }
    assert_eq!(reopen(&f).changes().unwrap().len(), before);
    assert!(table(&f, Table::GameMatches).is_empty());
}

#[tokio::test]
async fn an_approved_catalogue_game_follows_each_pcs_path_and_the_allow_list() {
    let mut f = primary();
    let (d1, _) = with_device(&mut f).await;
    let d2 = another_device(&f).await;
    let p1 = r"C:\Games\Great\great.exe";
    let p2 = r"D:\Games\great.exe";
    scan(&f, &d1, SCAN_AT, vec![scan_entry(p1, "Great")]).await;
    scan(&f, &d2, SCAN_AT, vec![scan_entry(p2, "Great")]).await;

    // Approve: one upsert, linked to the catalogue game, with the install on PC 1.
    let mut u = upsert("Great Game", p1);
    u.catalog_id = Some("G1".into());
    u.installs = vec![install(&d1, p1, "game:G1")];
    let saved = save(&f, u, None).await.unwrap();
    assert_eq!(
        tables_of(&saved),
        [
            "library_entries",
            "library_installs",
            "settings",
            "settings"
        ]
    );
    let id = entry_id(&saved);
    let rows = installs(&f);
    assert_eq!((rows.len(), text(&rows[0], "path")), (1, p1.to_owned()));
    assert_eq!(int(&rows[0], "rev"), 1);
    assert_eq!(library(&f)[0].installs.len(), 1);
    assert!(allow(&f).contains(&p1.to_owned()));
    assert!(!allow(&f).contains(&p2.to_owned()));

    // PC 2's scan finds the same game: its path is recorded without a new proposal.
    let r = record(&f, matched(&d2, SCAN_AT, &[("G1", p2)]))
        .await
        .unwrap();
    assert_eq!(
        tables_of(&r),
        ["game_matches", "library_installs", "settings", "settings"]
    );
    assert_eq!(installs(&f).len(), 2);
    let lib = library(&f);
    assert_eq!(
        lib[0]
            .installs
            .iter()
            .map(|i| (i.device_id.as_str(), i.path.as_str()))
            .collect::<Vec<_>>(),
        [(d1.as_str(), p1), (d2.as_str(), p2)]
    );
    assert!(allow(&f).contains(&p2.to_owned()));
    let two = installs(&f)
        .into_iter()
        .find(|r| text(r, "device_id") == d2)
        .unwrap();
    assert_eq!(text(&two, "entry_id"), id);
    assert_eq!(text(&two, "match_key"), "game:G1");

    // A later scan on PC 2 finds it at a new path: the path is updated (rev 2) and the old
    // allow rule leaves.
    let p3 = r"E:\Moved\great.exe";
    scan(&f, &d2, SCAN_AT + 1000, vec![scan_entry(p3, "Great")]).await;
    let r = record(&f, matched(&d2, SCAN_AT + 1000, &[("G1", p3)]))
        .await
        .unwrap();
    assert_eq!(
        tables_of(&r),
        ["game_matches", "library_installs", "settings", "settings"]
    );
    let two = installs(&f)
        .into_iter()
        .find(|r| text(r, "device_id") == d2)
        .unwrap();
    assert_eq!((text(&two, "path"), int(&two, "rev")), (p3.to_owned(), 2));
    assert!(allow(&f).contains(&p3.to_owned()));
    assert!(!allow(&f).contains(&p2.to_owned()));
    assert_eq!(installs(&f).len(), 2, "no new entry or install");

    // A scan that still finds the recorded path (and a second copy) changes nothing.
    let extra = r"F:\Copy\great.exe";
    scan(
        &f,
        &d2,
        SCAN_AT + 2000,
        vec![scan_entry(extra, "Great"), scan_entry(p3, "Great")],
    )
    .await;
    let r = record(
        &f,
        matched(&d2, SCAN_AT + 2000, &[("G1", extra), ("G1", p3)]),
    )
    .await
    .unwrap();
    assert_eq!(tables_of(&r), ["game_matches"]);
    let two = installs(&f)
        .into_iter()
        .find(|r| text(r, "device_id") == d2)
        .unwrap();
    assert_eq!(text(&two, "path"), p3);
}

#[tokio::test]
async fn an_approved_unmatched_executable_is_found_again_by_its_key() {
    let mut f = primary();
    let (d1, _) = with_device(&mut f).await;
    let d2 = another_device(&f).await;
    let p1 = r"C:\Tools\Cool\cool.exe";
    let key = proposal_key(None, "cool.exe", "Cool Tool", "Maker");
    scan(&f, &d1, SCAN_AT, vec![scan_entry(p1, "Cool Tool")]).await;
    let mut u = upsert("Cool Tool", p1);
    u.installs = vec![install(&d1, p1, &key)];
    save(&f, u, None).await.unwrap();

    // PC 2 has it in another folder: no catalogue match, found by key.
    let p2 = r"D:\Apps\cool.exe";
    scan(&f, &d2, SCAN_AT, vec![scan_entry(p2, "COOL TOOL")]).await;
    let r = record(&f, matched(&d2, SCAN_AT, &[])).await.unwrap();
    assert_eq!(
        tables_of(&r),
        ["game_matches", "library_installs", "settings", "settings"]
    );
    let two = installs(&f)
        .into_iter()
        .find(|r| text(r, "device_id") == d2)
        .unwrap();
    assert_eq!(
        (text(&two, "path"), text(&two, "match_key")),
        (p2.to_owned(), key)
    );
}

#[tokio::test]
async fn an_unsafe_path_is_not_made_an_install_but_the_matches_are_kept() {
    let mut f = primary();
    let (d1, _) = with_device(&mut f).await;
    let d2 = another_device(&f).await;
    let ok = r"C:\Games\g.exe";
    scan(&f, &d1, SCAN_AT, vec![scan_entry(ok, "G")]).await;
    let mut u = upsert("G", ok);
    u.catalog_id = Some("G1".into());
    u.installs = vec![install(&d1, ok, "game:G1")];
    save(&f, u, None).await.unwrap();
    // A user-writable root is refused as an unsigned allow rule.
    let unsafe_path = r"C:\Users\p\g.exe";
    scan(&f, &d2, SCAN_AT, vec![scan_entry(unsafe_path, "G")]).await;
    let r = record(&f, matched(&d2, SCAN_AT, &[("G1", unsafe_path)]))
        .await
        .unwrap();
    assert_eq!(tables_of(&r), ["game_matches"]);
    assert_eq!(installs(&f).len(), 1);
    assert!(!allow(&f).contains(&unsafe_path.to_owned()));
}

#[tokio::test]
async fn upsert_installs_are_checked_and_listed_devices_only_are_changed() {
    let mut f = primary();
    let (d1, _) = with_device(&mut f).await;
    let d2 = another_device(&f).await;
    let p = r"C:\Games\g.exe";
    let bad = |installs: Vec<LibraryInstall>| {
        let mut u = upsert("G", p);
        u.installs = installs;
        u
    };
    for (u, field) in [
        (bad(vec![install("NOPE", p, "")]), "installs[0].device_id"),
        (bad(vec![install(&d1, "g.exe", "")]), "installs[0].path"),
        (
            bad(vec![install(&d1, r"C:\Games\*.exe", "")]),
            "installs[0].path",
        ),
        (bad(vec![install(&d1, p, "junk")]), "installs[0].match_key"),
        (
            bad(vec![install(&d1, p, ""), install(&d1, p, "")]),
            "installs[1].device_id",
        ),
        (
            bad(vec![install(&d1, r"C:\Users\p\g.exe", "")]),
            "executable_path",
        ),
    ] {
        assert_eq!(
            refusal(save(&f, u, None).await),
            (ErrorCode::ValidationFailed, Some(field.into())),
            "{field}"
        );
    }
    assert!(table(&f, Table::LibraryEntries).is_empty());
    assert!(installs(&f).is_empty());

    let id = entry_id(
        &save(
            &f,
            bad(vec![install(&d1, p, ""), install(&d2, r"D:\g.exe", "")]),
            None,
        )
        .await
        .unwrap(),
    );
    assert_eq!(installs(&f).len(), 2);
    // An edit without installs leaves them; one listing PC 1 rewrites only PC 1.
    let mut edit = upsert("G", p);
    edit.id = Some(id.clone());
    edit.category = "Action".into();
    save(&f, edit.clone(), Some(1)).await.unwrap();
    assert_eq!(installs(&f).len(), 2);
    assert!(installs(&f).iter().all(|r| int(r, "rev") == 1));
    edit.installs = vec![install(&d1, r"E:\g.exe", "")];
    let r = save(&f, edit, Some(2)).await.unwrap();
    assert!(tables_of(&r).contains(&"library_installs"));
    let rows = installs(&f);
    let one = rows.iter().find(|r| text(r, "device_id") == d1).unwrap();
    let two = rows.iter().find(|r| text(r, "device_id") == d2).unwrap();
    assert_eq!(
        (text(one, "path"), int(one, "rev")),
        (r"E:\g.exe".to_owned(), 2)
    );
    assert_eq!(
        (text(two, "path"), int(two, "rev")),
        (r"D:\g.exe".to_owned(), 1)
    );
    // The new path is allowed.
    assert!(allow(&f).contains(&r"E:\g.exe".to_owned()));
    assert!(!allow(&f).contains(&p.to_owned()) || p == r"C:\Games\g.exe");
}

#[tokio::test]
async fn a_deleted_entrys_installs_leave_the_kiosk_library_and_the_allow_list() {
    let mut f = primary();
    let (d1, _) = with_device(&mut f).await;
    let mut u = upsert("G", r"C:\Games\g.exe");
    u.installs = vec![install(&d1, r"D:\Elsewhere\g.exe", "")];
    let id = entry_id(&save(&f, u, None).await.unwrap());
    assert!(allow(&f).contains(&r"D:\Elsewhere\g.exe".to_owned()));
    let mut del = intent(
        owner(),
        IntentPayload::LibraryDelete(arena_proto::LibraryDelete { id }),
    );
    del.base_rev = Some(1);
    f.writer.submit_intent(del).await.unwrap();
    assert!(library(&f).is_empty());
    assert!(!allow(&f).contains(&r"D:\Elsewhere\g.exe".to_owned()));
    assert!(reopen(&f).library_installs().unwrap().is_empty());
}

fn dismiss(keys: &[&str]) -> IntentPayload {
    IntentPayload::LibraryDismiss(LibraryDismiss {
        keys: keys.iter().map(|k| (*k).to_owned()).collect(),
    })
}

#[tokio::test]
async fn dismissed_proposals_are_kept_until_cleared() {
    let mut f = primary();
    with_device(&mut f).await;
    // Nothing to clear yet.
    assert_eq!(
        refusal(
            f.writer
                .submit_intent(intent(
                    owner(),
                    IntentPayload::LibraryDismissClear(LibraryDismissClear {})
                ))
                .await
        ),
        (ErrorCode::ValidationFailed, Some("keys".into()))
    );
    let r = f
        .writer
        .submit_intent(intent(
            owner(),
            dismiss(&["game:G1", "exe:a.exe||", "game:G1"]),
        ))
        .await
        .unwrap();
    assert_eq!(r.changes.len(), 2, "a key listed twice is stored once");
    assert!(
        r.changes
            .iter()
            .all(|c| c.tbl == "library_dismissals" && c.op == ChangeOp::Insert)
    );
    assert_eq!(table(&f, Table::LibraryDismissals).len(), 2);
    // The same keys again, and malformed ones, are refused.
    assert_eq!(
        refusal(
            f.writer
                .submit_intent(intent(owner(), dismiss(&["game:G1"])))
                .await
        ),
        (ErrorCode::ValidationFailed, Some("keys".into()))
    );
    assert_eq!(
        refusal(
            f.writer
                .submit_intent(intent(owner(), dismiss(&["nope"])))
                .await
        ),
        (ErrorCode::ValidationFailed, Some("keys[0]".into()))
    );
    assert_eq!(
        refusal(f.writer.submit_intent(intent(owner(), dismiss(&[]))).await),
        (ErrorCode::ValidationFailed, Some("keys".into()))
    );
    // Clearing deletes every row.
    let r = f
        .writer
        .submit_intent(intent(
            owner(),
            IntentPayload::LibraryDismissClear(LibraryDismissClear {}),
        ))
        .await
        .unwrap();
    assert_eq!(r.changes.len(), 2);
    assert!(r.changes.iter().all(|c| c.op == ChangeOp::Delete));
    assert!(table(&f, Table::LibraryDismissals).is_empty());
    // And the key can be dismissed again.
    f.writer
        .submit_intent(intent(owner(), dismiss(&["game:G1"])))
        .await
        .unwrap();
}

#[tokio::test]
async fn dismissing_needs_games_manage() {
    let mut f = primary();
    with_device(&mut f).await;
    let counter = super::stock::staff(&f, "till", crate::rbac::ROLE_COUNTER).await;
    assert_eq!(
        refusal(
            f.writer
                .submit_intent(intent(counter, dismiss(&["game:G1"])))
                .await
        )
        .0,
        ErrorCode::PermissionDenied
    );
    let manager = super::stock::staff(&f, "boss", crate::rbac::ROLE_MANAGER).await;
    f.writer
        .submit_intent(intent(manager, dismiss(&["game:G1"])))
        .await
        .unwrap();
}

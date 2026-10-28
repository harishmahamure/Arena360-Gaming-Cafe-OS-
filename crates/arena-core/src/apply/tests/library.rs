//! Story 6.3: the branch game and app library and the settings derived from it.

use super::session::{int, refusal, table, text};
use super::stock::{intent, manager_with_pin, owner, staff};
use super::*;
use crate::rbac::{ROLE_COUNTER, ROLE_MANAGER};
use arena_proto::{
    ElevatedBy, ElevationKind, ElevationMode, KioskLibraryEntry, LibraryDelete, LibraryUpsert,
    ProcessRule,
};

const STEAM_ROOT: &str = r"C:\Program Files (x86)\Steam\**.exe";
const RIOT_ROOT: &str = r"C:\Riot Games\**.exe";

fn entry(name: &str, path: &str) -> LibraryUpsert {
    LibraryUpsert {
        id: None,
        entry_kind: "game".into(),
        name: name.into(),
        launcher: "steam".into(),
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

async fn save(
    f: &Fixture,
    actor: Actor,
    u: LibraryUpsert,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(actor, IntentPayload::LibraryUpsert(u));
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

async fn delete(
    f: &Fixture,
    actor: Actor,
    id: &str,
    base_rev: Option<i64>,
) -> Result<Applied, AppError> {
    let mut i = intent(
        actor,
        IntentPayload::LibraryDelete(LibraryDelete { id: id.into() }),
    );
    i.base_rev = base_rev;
    f.writer.submit_intent(i).await
}

/// Adds `u` as the owner and returns its id.
async fn made(f: &Fixture, u: LibraryUpsert) -> String {
    save(f, owner(), u, None)
        .await
        .unwrap()
        .changes
        .iter()
        .find(|c| c.tbl == "library_entries")
        .unwrap()
        .row_id
        .clone()
}

fn entry_row(f: &Fixture, id: &str) -> Row {
    table(f, Table::LibraryEntries)
        .into_iter()
        .find(|r| text(r, "id") == id)
        .unwrap()
}

fn value_of(f: &Fixture, key: &str) -> String {
    text(&setting(&reopen(f), key), "value")
}

fn library(f: &Fixture) -> Vec<KioskLibraryEntry> {
    serde_json::from_str(&value_of(f, "kiosk.library")).unwrap()
}

fn allow(f: &Fixture) -> Vec<String> {
    serde_json::from_str::<Vec<ProcessRule>>(&value_of(f, "kiosk.process_allow"))
        .unwrap()
        .into_iter()
        .map(|r| r.path_glob)
        .collect()
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

fn tables_of(a: &Applied) -> Vec<&str> {
    a.changes.iter().map(|c| c.tbl.as_str()).collect()
}

fn operator_rule(f: &Fixture) -> Vec<ProcessRule> {
    serde_json::from_str(&value_of(f, "kiosk.process_allow")).unwrap()
}

#[tokio::test]
async fn adding_a_game_stores_it_and_derives_the_kiosk_library_and_the_allow_list() {
    let f = bootstrapped().await;
    let defaults = allow(&f);
    let mut u = entry("Counter-Strike 2", r"C:\Games\CS2\cs2.exe");
    u.category = "Shooter".into();
    u.tags = vec!["fps".into(), "esports".into()];
    u.age_rating = "18+".into();
    u.launch_args = "-novid".into();
    let id = made(&f, u).await;
    let row = entry_row(&f, &id);
    assert_eq!(int(&row, "rev"), 1);
    assert_eq!(text(&row, "kind"), "game");
    assert_eq!(text(&row, "tags"), r#"["fps","esports"]"#);
    assert_eq!(text(&row, "executable_path"), r"C:\Games\CS2\cs2.exe");
    assert_eq!(row["catalog_id"], RowValue::Null);
    assert_eq!(row["deleted_at"], RowValue::Null);

    let lib = library(&f);
    assert_eq!(lib.len(), 1);
    assert_eq!(lib[0].id, id);
    assert_eq!(lib[0].name, "Counter-Strike 2");
    assert_eq!(lib[0].launch_args, "-novid");
    assert_eq!(lib[0].tags, ["fps", "esports"]);
    assert_eq!(lib[0].catalog_id, None);

    let rules = allow(&f);
    assert_eq!(
        rules[..defaults.len()],
        defaults[..],
        "operator rules stay in place"
    );
    assert_eq!(
        rules[defaults.len()..],
        [r"C:\Games\CS2\cs2.exe".to_owned(), STEAM_ROOT.to_owned()]
    );
}

#[tokio::test]
async fn the_library_settings_are_written_in_the_same_transaction_and_only_when_they_change() {
    let f = bootstrapped().await;
    let a = save(&f, owner(), entry("A", r"C:\Games\a.exe"), None)
        .await
        .unwrap();
    assert_eq!(tables_of(&a), ["library_entries", "settings", "settings"]);
    let keys: Vec<_> = a
        .changes
        .iter()
        .filter(|c| c.tbl == "settings")
        .map(|c| c.row_id.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(keys, ["kiosk.library", "kiosk.process_allow"]);
    let id = a.changes[0].row_id.clone();
    // A category edit changes the library but not the allow-list.
    let mut edit = entry("A", r"C:\Games\a.exe");
    edit.id = Some(id.clone());
    edit.category = "Action".into();
    let b = save(&f, owner(), edit, Some(1)).await.unwrap();
    assert_eq!(tables_of(&b), ["library_entries", "settings"]);
    assert_eq!(library(&f)[0].category, "Action");
    // Saving the same thing again touches only the entry row.
    let mut same = entry("A", r"C:\Games\a.exe");
    same.id = Some(id);
    same.category = "Action".into();
    let c = save(&f, owner(), same, Some(2)).await.unwrap();
    assert_eq!(tables_of(&c), ["library_entries"]);
}

#[tokio::test]
async fn a_disabled_entry_is_not_in_the_kiosk_library_and_its_rules_leave() {
    let f = bootstrapped().await;
    let before = allow(&f);
    let id = made(&f, entry("A", r"C:\Games\a.exe")).await;
    assert_eq!(library(&f).len(), 1);
    let mut off = entry("A", r"C:\Games\a.exe");
    off.id = Some(id.clone());
    off.enabled = false;
    save(&f, owner(), off.clone(), Some(1)).await.unwrap();
    assert!(library(&f).is_empty());
    assert_eq!(allow(&f), before);
    assert_eq!(int(&entry_row(&f, &id), "enabled"), 0);
    // Enabling it again brings both back.
    off.enabled = true;
    save(&f, owner(), off, Some(2)).await.unwrap();
    assert_eq!(library(&f).len(), 1);
    assert!(allow(&f).contains(&r"C:\Games\a.exe".to_owned()));
}

#[tokio::test]
async fn a_rule_another_enabled_entry_needs_stays_and_operator_rules_are_never_removed() {
    let f = bootstrapped().await;
    // The operator already allowed a tool by hand.
    let current = operator_rule(&f);
    let mut custom = current.clone();
    custom.push(ProcessRule {
        path_glob: r"D:\Tools\timer.exe".into(),
        signer: Some("Tools Inc".into()),
    });
    let rev = int(&setting(&reopen(&f), "kiosk.process_allow"), "rev");
    f.writer
        .submit_intent(set(
            "kiosk.process_allow",
            &serde_json::to_string(&custom).unwrap(),
            Some(rev),
        ))
        .await
        .unwrap();
    let a = made(&f, entry("A", r"C:\Games\a.exe")).await;
    let b = made(&f, entry("B", r"C:\Games\b.exe")).await;
    assert_eq!(
        allow(&f)
            .iter()
            .filter(|g| g.as_str() == STEAM_ROOT)
            .count(),
        1,
        "two entries of one launcher share one rule"
    );
    delete(&f, owner(), &a, Some(1)).await.unwrap();
    let rules = allow(&f);
    assert!(!rules.contains(&r"C:\Games\a.exe".to_owned()));
    assert!(rules.contains(&r"C:\Games\b.exe".to_owned()));
    assert!(
        rules.contains(&STEAM_ROOT.to_owned()),
        "B still needs Steam"
    );
    assert!(rules.contains(&r"D:\Tools\timer.exe".to_owned()));
    delete(&f, owner(), &b, Some(1)).await.unwrap();
    assert_eq!(
        allow(&f),
        custom
            .iter()
            .map(|r| r.path_glob.clone())
            .collect::<Vec<_>>()
    );
    assert!(library(&f).is_empty());
}

#[tokio::test]
async fn an_entry_that_only_launches_by_uri_adds_only_its_launchers_rules() {
    let f = bootstrapped().await;
    let before = allow(&f);
    let mut u = entry("Valorant", "");
    u.launcher = "riot".into();
    u.launch_uri = "riot://launch".into();
    made(&f, u).await;
    assert_eq!(allow(&f)[before.len()..], [RIOT_ROOT.to_owned()]);
    assert_eq!(library(&f)[0].launch_uri, "riot://launch");
    let mut app = entry("Notepad++", r"C:\Apps\npp.exe");
    app.entry_kind = "app".into();
    app.launcher = "none".into();
    made(&f, app).await;
    assert_eq!(
        allow(&f)[before.len()..],
        [RIOT_ROOT.to_owned(), r"C:\Apps\npp.exe".to_owned()],
        "a launcher of none adds no install rule"
    );
    let names: Vec<_> = library(&f).into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["Notepad++", "Valorant"], "ordered by name");
}

#[tokio::test]
async fn bad_fields_are_refused_by_name_and_nothing_is_written() {
    let f = bootstrapped().await;
    made(&f, entry("Taken", r"C:\Games\taken.exe")).await;
    let before = changes(&f);
    let long_tag = "t".repeat(33);
    let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
    let cases: Vec<(&str, LibraryUpsert)> = vec![
        ("name", entry("", r"C:\Games\x.exe")),
        ("name", entry("   ", r"C:\Games\x.exe")),
        ("name", entry(&"n".repeat(81), r"C:\Games\x.exe")),
        ("name", entry("bad\nname", r"C:\Games\x.exe")),
        ("executable_path", entry("X", "")),
        ("executable_path", entry("X", "x.exe")),
        ("executable_path", entry("X", r"C:\Games\*.exe")),
        ("executable_path", entry("X", r"C:\Games\x?.exe")),
        ("executable_path", entry("X", r"C:\{install}\x.exe")),
        ("executable_path", entry("X", r"{install}\x.exe")),
        ("executable_path", entry("X", r"C:\Users\p\g.exe")),
        ("executable_path", entry("X", r"c:\GAMES\TAKEN.EXE")),
        (
            "entry_kind",
            LibraryUpsert {
                entry_kind: "movie".into(),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "launcher",
            LibraryUpsert {
                launcher: "gog".into(),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "launch_uri",
            LibraryUpsert {
                launch_uri: "steam".into(),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "launch_uri",
            LibraryUpsert {
                launch_uri: format!("steam://{}", "x".repeat(510)),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "launch_args",
            LibraryUpsert {
                launch_args: "a".repeat(513),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "category",
            LibraryUpsert {
                category: "c".repeat(41),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "tags",
            LibraryUpsert {
                tags: many,
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "tags",
            LibraryUpsert {
                tags: vec![long_tag],
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "age_rating",
            LibraryUpsert {
                age_rating: "a".repeat(17),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
        (
            "id",
            LibraryUpsert {
                id: Some("NOPE".into()),
                ..entry("X", r"C:\Games\x.exe")
            },
        ),
    ];
    for (field, u) in cases {
        let (code, got) = refusal(save(&f, owner(), u.clone(), Some(1)).await);
        assert_eq!(code, ErrorCode::ValidationFailed, "{field} {u:?}");
        assert_eq!(got.as_deref(), Some(field), "{u:?}");
    }
    assert_eq!(changes(&f), before, "nothing was written");
    assert_eq!(table(&f, Table::LibraryEntries).len(), 1);
}

#[tokio::test]
async fn a_duplicate_is_the_same_kind_launcher_and_path_without_case() {
    let f = bootstrapped().await;
    let id = made(&f, entry("A", r"C:\Games\a.exe")).await;
    let (code, field) = refusal(save(&f, owner(), entry("Again", r"c:\games\A.EXE"), None).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("executable_path"))
    );
    // Another launcher or another kind is not a duplicate.
    let mut epic = entry("A (Epic)", r"C:\Games\a.exe");
    epic.launcher = "epic".into();
    made(&f, epic).await;
    let mut app = entry("A (app)", r"C:\Games\a.exe");
    app.entry_kind = "app".into();
    made(&f, app).await;
    // An entry is not a duplicate of itself, and a deleted one frees its path.
    let mut itself = entry("A renamed", r"C:\Games\a.exe");
    itself.id = Some(id.clone());
    save(&f, owner(), itself, Some(1)).await.unwrap();
    delete(&f, owner(), &id, Some(2)).await.unwrap();
    made(&f, entry("A again", r"C:\Games\a.exe")).await;
    // Entries without a path never collide.
    let mut uri = entry("U", "");
    uri.launch_uri = "steam://rungameid/1".into();
    made(&f, uri.clone()).await;
    made(&f, uri).await;
}

#[tokio::test]
async fn an_unsafe_path_is_refused_even_for_a_disabled_entry() {
    let f = bootstrapped().await;
    let before = changes(&f);
    for path in [
        r"C:\Users\p\g.exe",
        r"C:\ProgramData\x\g.exe",
        r"C:\Temp\g.exe",
    ] {
        for enabled in [true, false] {
            let mut u = entry("G", path);
            u.enabled = enabled;
            let (code, field) = refusal(save(&f, owner(), u, None).await);
            assert_eq!(
                (code, field.as_deref()),
                (ErrorCode::ValidationFailed, Some("executable_path")),
                "{path} {enabled}"
            );
        }
    }
    assert_eq!(changes(&f), before);
}

#[tokio::test]
async fn edits_carry_base_rev_and_a_stale_one_fails() {
    let f = bootstrapped().await;
    let manager = staff(&f, "mgr", ROLE_MANAGER).await;
    let id = save(&f, manager.clone(), entry("A", r"C:\Games\a.exe"), None)
        .await
        .unwrap()
        .changes[0]
        .row_id
        .clone();
    let edit = |category: &str| LibraryUpsert {
        id: Some(id.clone()),
        category: category.into(),
        ..entry("A", r"C:\Games\a.exe")
    };
    let (code, field) = refusal(save(&f, manager.clone(), edit("x"), None).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    save(&f, manager.clone(), edit("one"), Some(1))
        .await
        .unwrap();
    let before = changes(&f);
    let (code, field) = refusal(save(&f, manager.clone(), edit("two"), Some(1)).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::Stale, Some("base_rev"))
    );
    let (code, _) = refusal(delete(&f, manager.clone(), &id, Some(1)).await);
    assert_eq!(code, ErrorCode::Stale);
    let (code, field) = refusal(delete(&f, manager.clone(), &id, None).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("base_rev"))
    );
    assert_eq!(changes(&f), before);
    assert_eq!(library(&f)[0].category, "one");
    // The owner is exempt.
    save(&f, owner(), edit("owner"), None).await.unwrap();
    save(&f, owner(), edit("owner again"), Some(1))
        .await
        .unwrap();
    assert_eq!(library(&f)[0].category, "owner again");
}

#[tokio::test]
async fn deleting_is_soft_and_an_unknown_id_is_refused() {
    let f = bootstrapped().await;
    let id = made(&f, entry("A", r"C:\Games\a.exe")).await;
    let a = delete(&f, owner(), &id, Some(1)).await.unwrap();
    assert_eq!(tables_of(&a), ["library_entries", "settings", "settings"]);
    let row = entry_row(&f, &id);
    assert_ne!(row["deleted_at"], RowValue::Null, "the row stays");
    assert_eq!(int(&row, "rev"), 2);
    assert!(library(&f).is_empty());
    for gone in [id.as_str(), "NOPE"] {
        let (code, field) = refusal(delete(&f, owner(), gone, None).await);
        assert_eq!(
            (code, field.as_deref()),
            (ErrorCode::ValidationFailed, Some("id"))
        );
    }
    // A deleted entry cannot be edited.
    let (code, field) = refusal(
        save(
            &f,
            owner(),
            LibraryUpsert {
                id: Some(id),
                ..entry("A", r"C:\Games\a.exe")
            },
            Some(2),
        )
        .await,
    );
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("id"))
    );
}

#[tokio::test]
async fn a_linked_entry_keeps_its_catalogue_fields_but_category_and_tags_are_editable() {
    let f = bootstrapped().await;
    let mut linked = entry("Dota 2", r"C:\Games\dota.exe");
    linked.age_rating = "16+".into();
    linked.catalog_id = Some("01JCATALOGGAME".into());
    let id = made(&f, linked.clone()).await;
    assert_eq!(text(&entry_row(&f, &id), "catalog_id"), "01JCATALOGGAME");
    assert_eq!(library(&f)[0].catalog_id.as_deref(), Some("01JCATALOGGAME"));
    let before = changes(&f);
    let edit = |u: LibraryUpsert| LibraryUpsert {
        id: Some(id.clone()),
        ..u
    };
    for (field, u) in [
        (
            "name",
            LibraryUpsert {
                name: "Dota 3".into(),
                ..linked.clone()
            },
        ),
        (
            "launcher",
            LibraryUpsert {
                launcher: "epic".into(),
                ..linked.clone()
            },
        ),
        (
            "age_rating",
            LibraryUpsert {
                age_rating: "18+".into(),
                ..linked.clone()
            },
        ),
        (
            "catalog_id",
            LibraryUpsert {
                catalog_id: Some("OTHER".into()),
                ..linked.clone()
            },
        ),
        (
            "catalog_id",
            LibraryUpsert {
                catalog_id: None,
                ..linked.clone()
            },
        ),
    ] {
        let (code, got) = refusal(save(&f, owner(), edit(u), Some(1)).await);
        assert_eq!(code, ErrorCode::ValidationFailed, "{field}");
        assert_eq!(got.as_deref(), Some(field));
    }
    assert_eq!(changes(&f), before);
    let mut ok = linked.clone();
    ok.category = "MOBA".into();
    ok.tags = vec!["team".into()];
    save(&f, owner(), edit(ok), Some(1)).await.unwrap();
    let lib = library(&f);
    assert_eq!(
        (lib[0].category.as_str(), lib[0].tags.clone()),
        ("MOBA", vec!["team".to_owned()])
    );
    // An unlinked entry may be linked later, once.
    let plain = made(&f, entry("Other", r"C:\Games\other.exe")).await;
    let mut link = entry("Catalogue name", r"C:\Games\other.exe");
    link.id = Some(plain);
    link.catalog_id = Some("01JOTHER".into());
    save(&f, owner(), link, Some(1)).await.unwrap();
}

#[tokio::test]
async fn a_counter_is_refused_until_a_manager_pin_elevates_and_both_actors_are_audited() {
    let f = bootstrapped().await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    let before = changes(&f);
    let (code, _) = refusal(save(&f, counter.clone(), entry("A", r"C:\Games\a.exe"), None).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert_eq!(changes(&f), before);
    let elevated = Intent {
        elevated_by: Some(ElevatedBy {
            kind: ElevationKind::Pin,
            id: manager.clone(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        }),
        ..intent(
            counter.clone(),
            IntentPayload::LibraryUpsert(entry("A", r"C:\Games\a.exe")),
        )
    };
    let applied = f.writer.submit_intent(elevated).await.unwrap();
    assert_eq!(library(&f).len(), 1);
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + applied.changes.len());
    for change in &rows[before..] {
        assert_eq!(change.actor_id, counter.id);
        assert_eq!(change.elevated_by_id.as_deref(), Some(manager.as_str()));
    }
    // A manager holds the permission outright.
    let mgr = staff(&f, "mgr2", ROLE_MANAGER).await;
    save(&f, mgr, entry("B", r"C:\Games\b.exe"), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn games_manage_goes_to_managers_and_the_owner_only() {
    use arena_proto::Permission;
    for (role, holds) in [
        (crate::rbac::ROLE_OWNER, true),
        (ROLE_MANAGER, true),
        (ROLE_COUNTER, false),
        (crate::rbac::ROLE_KITCHEN, false),
        (crate::rbac::ROLE_KIOSK_DEVICE, false),
    ] {
        assert_eq!(
            crate::rbac::builtin_perms(role).contains(&Permission::GamesManage),
            holds,
            "{role}"
        );
    }
}

#[tokio::test]
async fn kiosk_library_cannot_be_set_by_settings_set() {
    let f = bootstrapped().await;
    let rev = int(&setting(&reopen(&f), "kiosk.library"), "rev");
    let before = changes(&f);
    let (code, field) = refusal(
        f.writer
            .submit_intent(set("kiosk.library", "[]", Some(rev)))
            .await,
    );
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("key"))
    );
    assert_eq!(changes(&f), before);
    assert_eq!(value_of(&f, "kiosk.library"), "[]", "seeded empty");
}

#[tokio::test]
async fn a_branch_without_the_library_setting_gets_the_row_on_the_first_write() {
    let f = bootstrapped().await;
    f.writer
        .submit(|tx| {
            tx.execute_batch("DELETE FROM settings WHERE key = 'kiosk.library'")
                .map_err(arena_store::db_error)
        })
        .await
        .unwrap();
    let a = save(&f, owner(), entry("A", r"C:\Games\a.exe"), None)
        .await
        .unwrap();
    assert!(
        a.changes
            .iter()
            .any(|c| c.tbl == "settings" && c.row_id.ends_with("/kiosk.library"))
    );
    assert_eq!(library(&f).len(), 1);
}

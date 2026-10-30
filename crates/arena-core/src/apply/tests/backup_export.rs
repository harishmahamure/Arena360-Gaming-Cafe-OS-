//! Story 9.4: the `backup.export` record, `backup.last_export`, and the reader that exports.

use super::session::{refusal, text};
use super::stock::{intent, manager_with_pin, owner, staff};
use super::*;
use crate::SnapshotReader;
use crate::rbac::ROLE_COUNTER;
use arena_proto::{BackupExport, ElevatedBy, ElevationKind, ElevationMode, ExportHeader};

const KEY: &str = "backup.last_export";

fn facts() -> BackupExport {
    BackupExport {
        file_name: "DEV01-2027-01-15-1330.a360".into(),
        bytes: 4096,
        epoch: "1:abc".into(),
        last_seq: 7,
        exported_at: T0,
        ..Default::default()
    }
}

fn record(actor: Actor, facts: BackupExport) -> Intent {
    intent(actor, IntentPayload::BackupExport(facts))
}

fn stored(f: &Fixture) -> String {
    text(&setting(&reopen(f), KEY), "value")
}

fn changes(f: &Fixture) -> usize {
    reopen(f).changes().unwrap().len()
}

async fn manager(f: &Fixture) -> Actor {
    staff(f, "mgr-direct", crate::rbac::ROLE_MANAGER).await
}

#[tokio::test]
async fn the_setting_starts_empty() {
    let f = bootstrapped().await;
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn a_manager_records_an_export_and_the_setting_holds_its_facts() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    let before = changes(&f);
    let applied = f
        .writer
        .submit_intent(record(mgr.clone(), facts()))
        .await
        .unwrap();
    assert_eq!(applied.changes.len(), 1);
    assert_eq!(applied.changes[0].tbl, "settings");
    assert!(applied.changes[0].row_id.ends_with("/backup.last_export"));
    assert_eq!(
        BackupExport::parse(&stored(&f)),
        Some(BackupExport {
            by: Some("mgr-direct".into()),
            ..facts()
        })
    );
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].actor_id, mgr.id);
    assert_eq!(rows[before].elevated_by_id, None);
}

#[tokio::test]
async fn a_second_export_replaces_the_first_and_needs_no_base_rev() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    f.writer
        .submit_intent(record(mgr.clone(), facts()))
        .await
        .unwrap();
    let later = BackupExport {
        file_name: "DEV01-2027-01-16-0900.a360".into(),
        last_seq: 20,
        ..facts()
    };
    let mut again = record(mgr, later.clone());
    again.base_rev = Some(99);
    f.writer.submit_intent(again).await.unwrap();
    let kept = BackupExport::parse(&stored(&f)).unwrap();
    assert_eq!(kept.file_name, later.file_name);
    assert_eq!(kept.last_seq, 20);
    let names: Vec<String> = kept.entries().into_iter().map(|e| e.file_name).collect();
    assert_eq!(
        names,
        vec![
            "DEV01-2027-01-16-0900.a360".to_owned(),
            "DEV01-2027-01-15-1330.a360".to_owned()
        ]
    );
}

#[tokio::test]
async fn a_counter_is_refused_until_a_managers_pin_elevates_and_both_actors_are_audited() {
    let f = bootstrapped().await;
    let counter = staff(&f, "ctr", ROLE_COUNTER).await;
    let manager = manager_with_pin(&f).await;
    let before = changes(&f);
    let (code, _) = refusal(
        f.writer
            .submit_intent(record(counter.clone(), facts()))
            .await,
    );
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");

    let elevated = Intent {
        elevated_by: Some(ElevatedBy {
            kind: ElevationKind::Pin,
            id: manager.clone(),
            mode: ElevationMode::Hub,
            verifier_version: 1,
        }),
        ..record(counter.clone(), facts())
    };
    f.writer.submit_intent(elevated).await.unwrap();
    let kept = BackupExport::parse(&stored(&f)).unwrap();
    assert_eq!(
        (kept.file_name.as_str(), kept.by.as_deref()),
        (facts().file_name.as_str(), Some("ctr"))
    );
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].actor_id, counter.id);
    assert_eq!(
        rows[before].elevated_by_id.as_deref(),
        Some(manager.as_str())
    );
}

fn auto_facts(n: i64) -> BackupExport {
    BackupExport {
        file_name: format!("DEV01-auto-2027-01-{:02}-0130.a360", n),
        bytes: 100 + n,
        epoch: "1:abc".into(),
        last_seq: n,
        exported_at: T0 + n,
        ..Default::default()
    }
}

fn hub_record(facts: BackupExport) -> Intent {
    Intent {
        actor: Actor {
            kind: ActorKind::System,
            id: arena_proto::AUTO_EXPORT_ACTOR.into(),
        },
        ..record(owner(), facts)
    }
}

#[tokio::test]
async fn the_hub_records_its_auto_export_without_a_person_and_the_list_keeps_seven() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    f.writer.submit_intent(record(mgr, facts())).await.unwrap();
    for n in 1..=8 {
        f.writer
            .submit_intent(hub_record(auto_facts(n)))
            .await
            .unwrap();
    }
    let kept = BackupExport::parse(&stored(&f)).unwrap();
    assert_eq!(kept.by, None, "an auto export names no person");
    let entries = kept.entries();
    assert_eq!(entries.len(), arena_proto::EXPORT_HISTORY);
    let names: Vec<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();
    assert_eq!(names[0], "DEV01-auto-2027-01-08-0130.a360");
    assert_eq!(names[6], "DEV01-auto-2027-01-02-0130.a360");
    assert!(
        entries.iter().all(|e| e.by.is_none()),
        "the manual export has aged out"
    );
}

#[tokio::test]
async fn staff_cannot_pass_for_the_hub_and_the_hub_cannot_pass_for_staff() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    let before = changes(&f);
    // A staff member sending an auto name.
    let (code, field) = refusal(f.writer.submit_intent(record(mgr, auto_facts(1))).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("actor"))
    );
    // The Hub sending a manual name, or another system actor sending an auto one.
    let (code, _) = refusal(f.writer.submit_intent(hub_record(facts())).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    let mut other = hub_record(auto_facts(1));
    other.actor.id = "notify".into();
    let (code, _) = refusal(f.writer.submit_intent(other).await);
    assert_eq!(code, ErrorCode::PermissionDenied);
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn only_a_staff_member_records_and_the_facts_must_be_plausible() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    let before = changes(&f);
    // The owner's cloud actor is not a staff session.
    let (code, field) = refusal(f.writer.submit_intent(record(owner(), facts())).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("actor"))
    );
    for bad in [
        BackupExport {
            file_name: "../escape.a360".into(),
            ..facts()
        },
        BackupExport {
            file_name: "dump.db".into(),
            ..facts()
        },
        BackupExport {
            bytes: 0,
            ..facts()
        },
        BackupExport {
            exported_at: 0,
            ..facts()
        },
    ] {
        let (code, field) = refusal(f.writer.submit_intent(record(mgr.clone(), bad)).await);
        assert_eq!(
            (code, field.as_deref()),
            (ErrorCode::ValidationFailed, Some("file_name"))
        );
    }
    assert_eq!(changes(&f), before);
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn settings_set_cannot_write_the_record() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    let rev = match &setting(&reopen(&f), KEY)["rev"] {
        RowValue::Integer(i) => *i,
        other => panic!("{other:?}"),
    };
    let mut set = intent(
        mgr,
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: KEY.into(),
            value: facts().to_json(),
        }),
    );
    set.base_rev = Some(rev);
    let (code, field) = refusal(f.writer.submit_intent(set).await);
    assert_eq!(
        (code, field.as_deref()),
        (ErrorCode::ValidationFailed, Some("key"))
    );
    assert_eq!(stored(&f), "");
}

#[tokio::test]
async fn the_reader_exports_into_a_folder_named_by_the_branch_code_and_zone() {
    let f = bootstrapped().await;
    let mgr = manager(&f).await;
    f.writer.submit_intent(record(mgr, facts())).await.unwrap();
    let last = reopen(&f).changes().unwrap().last().cloned().unwrap();
    let out = tempfile::tempdir().unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    let (done, path) = reader
        .export_to_folder(&out.path().to_string_lossy(), T0)
        .unwrap();
    // 2027-01-15 08:00 UTC is 13:30 in Asia/Kolkata.
    assert_eq!(done.file_name, "DEV01-2027-01-15-1330.a360");
    assert_eq!(path, out.path().join(&done.file_name));
    assert_eq!(done.bytes, std::fs::metadata(&path).unwrap().len() as i64);
    assert_eq!(
        (done.epoch.as_str(), done.last_seq),
        (last.epoch.as_str(), last.seq)
    );
    assert_eq!(done.exported_at, T0);
    assert!(done.is_consistent());

    let bytes = std::fs::read(&path).unwrap();
    let (header, _) = ExportHeader::decode(&bytes).unwrap();
    assert_eq!(header.branch_code, "DEV01");
    assert_eq!(header.epoch, done.epoch);
    assert_eq!(header.last_seq, done.last_seq);
    assert_eq!(header.schema_version, arena_store::LATEST_SCHEMA_VERSION);
}

#[tokio::test]
async fn the_reader_names_why_a_location_cannot_take_an_export() {
    let f = bootstrapped().await;
    let reader = SnapshotReader::open(&f.path).unwrap();
    let out = tempfile::tempdir().unwrap();
    let why = |folder: &str| {
        let e = reader.export_to_folder(folder, T0).unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed, "{folder}");
        assert_eq!(e.field.as_deref(), Some("folder"), "{folder}");
        e.message
    };
    assert_eq!(why(""), "not_set");
    assert_eq!(why("   "), "not_set");
    assert_eq!(why("content://tree/primary%3ABackups"), "unsupported");
    assert_eq!(why(&out.path().join("gone").to_string_lossy()), "not_found");
    let file = out.path().join("file.txt");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(why(&file.to_string_lossy()), "not_dir");
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "nothing written"
    );
}

// Story 9.6: "Export everything".

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut all: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    all.sort();
    all
}

#[tokio::test]
async fn export_everything_writes_the_database_the_report_csvs_and_a_readme_into_one_folder() {
    let f = bootstrapped().await;
    let out = tempfile::tempdir().unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    let kek = [7u8; crate::players::KEK_LEN];
    let (done, dir) = reader
        .export_everything(
            &out.path().to_string_lossy(),
            ("2027-01-01", "2027-01-31"),
            T0,
            Some(&kek),
        )
        .unwrap();
    // 2027-01-15 08:00 UTC is 13:30 in Asia/Kolkata.
    assert_eq!(
        done.folder_name,
        "DEV01-everything-2027-01-01_2027-01-31-2027-01-15-1330"
    );
    assert_eq!(dir, out.path().join(&done.folder_name));
    assert_eq!(names(out.path()), vec![done.folder_name.clone()]);
    assert!(done.skipped.is_empty(), "{:?}", done.skipped);
    assert_eq!(done.export.file_name, "DEV01-2027-01-15-1330.a360");
    assert!(done.export.is_consistent());

    let files = names(&dir);
    assert!(files.contains(&done.export.file_name));
    assert!(files.contains(&"README.txt".to_owned()));
    for report in [
        "DEV01_daily-sales_2027-01-01_2027-01-31.csv",
        "DEV01_sessions_2027-01-01_2027-01-31.csv",
    ] {
        assert!(files.contains(&report.to_owned()), "{report} in {files:?}");
    }
    // The fixture branch is not GST-registered, so it has no GST return and that is not an error.
    assert!(!files.iter().any(|n| n.contains("gst")));
    assert!(files.iter().any(|n| n.contains("stock-on-hand")));
    // Every report named is a file of the folder, and the folder holds nothing else.
    let mut expected = done.reports.clone();
    expected.push(done.export.file_name.clone());
    expected.push("README.txt".to_owned());
    expected.sort();
    assert_eq!(files, expected);

    // A report's CSV is byte for byte the single-report export.
    let req = arena_proto::ReportExportRequest {
        report: "daily-sales".into(),
        format: arena_proto::ExportFormat::Csv,
        folder: String::new(),
        from: Some("2027-01-01".into()),
        to: Some("2027-01-31".into()),
        month: None,
        shift_id: None,
        product: None,
        reason: None,
    };
    let (name, bytes) = reader.export_report(&req, T0, None).unwrap();
    assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes);

    let readme = std::fs::read_to_string(dir.join("README.txt")).unwrap();
    assert!(readme.contains(arena_proto::EXPORT_FORMAT_REFERENCE_URL));
    assert!(readme.contains("2027-01-01 to 2027-01-31"));
    assert!(readme.contains(&done.export.file_name));
}

#[tokio::test]
async fn a_report_that_cannot_be_built_is_listed_in_the_readme_and_does_not_fail_the_export() {
    let f = bootstrapped().await;
    let out = tempfile::tempdir().unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    // No key store: the Players report cannot be built.
    let (done, dir) = reader
        .export_everything(
            &out.path().to_string_lossy(),
            ("2027-01-15", "2027-01-15"),
            T0,
            None,
        )
        .unwrap();
    assert_eq!(done.skipped, vec!["players".to_owned()]);
    assert!(done.reports.iter().all(|r| !r.contains("players")));
    let readme = std::fs::read_to_string(dir.join("README.txt")).unwrap();
    assert!(readme.contains("Not exported:\n  players"));
}

#[tokio::test]
async fn export_everything_refuses_a_bad_range_or_location_and_leaves_nothing() {
    let f = bootstrapped().await;
    let out = tempfile::tempdir().unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    let folder = out.path().to_string_lossy().into_owned();
    let field = |from: &str, to: &str, folder: &str| {
        let e = reader
            .export_everything(folder, (from, to), T0, None)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        (e.field.unwrap_or_default(), e.message)
    };
    assert_eq!(field("2027-01-31", "2027-01-01", &folder).0, "to");
    assert_eq!(field("2027-01-01", "2027-06-30", &folder).0, "to");
    assert_eq!(field("nope", "2027-01-01", &folder).0, "from");
    assert_eq!(
        field("2027-01-01", "2027-01-02", ""),
        ("folder".into(), "not_set".into())
    );
    assert_eq!(
        field(
            "2027-01-01",
            "2027-01-02",
            &out.path().join("gone").to_string_lossy()
        ),
        ("folder".into(), "not_found".into())
    );
    assert!(names(out.path()).is_empty(), "nothing written");
}

#[tokio::test]
async fn a_second_export_in_the_same_minute_gets_its_own_folder() {
    let f = bootstrapped().await;
    let out = tempfile::tempdir().unwrap();
    let reader = SnapshotReader::open(&f.path).unwrap();
    let folder = out.path().to_string_lossy().into_owned();
    let first = reader
        .export_everything(&folder, ("2027-01-15", "2027-01-15"), T0, None)
        .unwrap()
        .0;
    let second = reader
        .export_everything(&folder, ("2027-01-15", "2027-01-15"), T0, None)
        .unwrap()
        .0;
    assert_eq!(second.folder_name, format!("{}-2", first.folder_name));
    assert_eq!(names(out.path()).len(), 2);
}

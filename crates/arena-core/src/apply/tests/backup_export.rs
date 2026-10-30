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

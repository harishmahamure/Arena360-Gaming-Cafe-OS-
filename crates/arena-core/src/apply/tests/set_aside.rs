//! Story 9.23: the set-aside list built from the `lease.promote` and `ops.restore` records and
//! the databases in `orphaned/`, its CSV, and the sweep that lists them into the audit first.

use super::session::text;
use super::stock::{intent, staff};
use super::*;
use crate::rbac::ROLE_MANAGER;
use crate::set_aside;
use arena_proto::{
    ExportHeader, LeasePromote, ORPHAN_SWEEP_SYSTEM_ACTOR, OpsRestore, SetAsideKind,
};

const DAY: i64 = 24 * 60 * 60 * 1000;
const ORPHAN: &str = "4-abc-20270115T133000.db";

fn promote() -> LeasePromote {
    LeasePromote {
        old_epoch: "2:01JMAIN".into(),
        fork_seq: 50,
        new_epoch: "3:01JSTBY".into(),
        provisional: false,
        lag_rows: 7,
        force: true,
        force_reason: Some("LEASE_FRESH".into()),
        erasures_reapplied: 0,
        kitchen_displays_to_repair: 0,
        promoted_at: T0 - 5_000,
        by: None,
    }
}

fn restore(f: &Fixture) -> OpsRestore {
    OpsRestore {
        file_name: "DEV01-2027-01-15-1330.a360".into(),
        header: ExportHeader {
            format_version: arena_proto::EXPORT_FORMAT_VERSION,
            branch_id: text(&setting(&reopen(f), "backup.last_restore"), "branch_id"),
            branch_code: "DEV01".into(),
            epoch: "4:abc".into(),
            last_seq: 120,
            schema_version: 84,
            exported_at: T0 - 60_000,
            sha256: "ab".repeat(32),
            purpose: None,
        },
        old_epoch: "4:abc".into(),
        new_epoch: "5:abc".into(),
        rows_set_aside: 5,
        erasures_reapplied: 0,
        orphaned_file: ORPHAN.into(),
        restored_at: T0,
        by: None,
        cloud: None,
    }
}

fn order(conn: &arena_store::DbConn, id: &str, no: &str, total: i64) {
    conn.execute(
        "INSERT INTO orders (id, status, subtotal_minor, total_minor, business_day, fy, rev,
                             created_at, updated_at, settled_at, kind, invoice_no, tax_minor,
                             tax_breakup)
         VALUES (?1, 'settled', ?3, ?3, '2027-01-10', 'FY2026-27', 0, 5, 5, 77, 'pos_sale', ?2, 0, '[]')",
        (id, no, total),
    )
    .unwrap();
}

/// A copy of the live database in `orphaned/` with two invoices the live one lacks and five
/// changes of the old epoch above the restore's fork.
fn orphan_copy(f: &Fixture) -> std::path::PathBuf {
    let dir = f.path.parent().unwrap().join("orphaned");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join(ORPHAN);
    arena_store::DbConn::open(&f.path)
        .unwrap()
        .execute("VACUUM INTO ?1", [target.to_str().unwrap()])
        .unwrap();
    let conn = arena_store::DbConn::open(&target).unwrap();
    order(&conn, "O1", "INV/26-27/0042", 11_800);
    order(&conn, "O2", "INV/26-27/0043", 500);
    for seq in 118..=125 {
        conn.execute(
            "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                                  actor_kind, actor_id, at)
             VALUES ('4:abc', ?1, 't', 'r', 'insert', x'00', 'i', 'k', 'system', 'a', 0)",
            [seq],
        )
        .unwrap();
    }
    target
}

async fn with_records() -> Fixture {
    let f = bootstrapped().await;
    let manager = staff(&f, "manager-asha", ROLE_MANAGER).await;
    f.writer
        .submit_intent(intent(manager, IntentPayload::LeasePromote(promote())))
        .await
        .unwrap();
    let owner_staff = staff(&f, "owner-direct", crate::rbac::ROLE_OWNER).await;
    f.writer
        .submit_intent(intent(owner_staff, IntentPayload::OpsRestore(restore(&f))))
        .await
        .unwrap();
    f
}

#[tokio::test]
async fn the_list_names_each_fork_newest_first_and_what_each_orphaned_database_holds() {
    let f = with_records().await;
    orphan_copy(&f);
    let found = set_aside::build(&reopen(&f)).unwrap();
    let list = &found.list;

    assert_eq!(list.forks.len(), 2);
    let (restored, promoted) = (&list.forks[0], &list.forks[1]);
    assert_eq!(restored.kind, SetAsideKind::Restore);
    assert_eq!(
        (restored.old_epoch.as_str(), restored.new_epoch.as_str()),
        ("4:abc", "5:abc")
    );
    assert_eq!(restored.fork_seq, 120);
    assert_eq!(restored.set_aside_changes, 5);
    assert!(!restored.forced);
    assert_eq!(restored.orphaned_file.as_deref(), Some(ORPHAN));
    assert_eq!(promoted.kind, SetAsideKind::Promote);
    assert!(promoted.forced);
    assert_eq!(promoted.set_aside_changes, 7);
    assert_eq!(promoted.by.as_deref(), Some("manager-asha"));

    assert_eq!(list.databases.len(), 1);
    assert_eq!(list.databases[0].file, ORPHAN);
    assert_eq!(
        list.databases[0].set_aside_changes,
        Some(5),
        "seq 121..=125"
    );
    assert_eq!(list.databases[0].invoices, 2);
    assert!(!list.databases[0].unreadable);
    let numbers: Vec<_> = list
        .invoices
        .iter()
        .map(|i| (i.invoice_no.as_str(), i.amount_minor))
        .collect();
    assert_eq!(
        numbers,
        vec![("INV/26-27/0042", 11_800), ("INV/26-27/0043", 500)]
    );
    assert_eq!(list.invoices[0].at, Some(77));
    assert!(!list.invoices_truncated);
    assert_eq!(list.invoice_total, 2);
    assert_eq!(found.all_invoices.len(), 2);
}

#[tokio::test]
async fn a_branch_with_nothing_set_aside_lists_nothing() {
    let f = bootstrapped().await;
    let found = set_aside::build(&reopen(&f)).unwrap();
    assert_eq!(found, set_aside::SetAside::default());
}

#[tokio::test]
async fn export_writes_a_csv_to_the_backup_location() {
    let f = with_records().await;
    orphan_copy(&f);
    let found = set_aside::build(&reopen(&f)).unwrap();
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().to_str().unwrap();
    let (done, path) =
        set_aside::export_csv(&found, folder, "DEV01", "Asia/Kolkata", "INR", 2, T0).unwrap();
    assert!(done.file_name.starts_with("DEV01-set-aside-") && done.file_name.ends_with(".csv"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("INV/26-27/0042,118.00,INR,"), "{text}");
    assert!(text.contains("INV/26-27/0043,5.00,INR,"));
    assert!(text.contains("restore,4:abc,5:abc,120"));
    assert!(text.contains("promote,2:01JMAIN,3:01JSTBY,50,manager-asha,yes,7"));
    assert_eq!(done.bytes, std::fs::metadata(&path).unwrap().len() as i64);

    for (bad, reason) in [
        ("", "not_set"),
        ("content://tree/x", "unsupported"),
        (out.path().join("nope").to_str().unwrap(), "not_found"),
        (path.to_str().unwrap(), "not_dir"),
    ] {
        let e = set_aside::export_csv(&found, bad, "DEV01", "UTC", "INR", 2, T0).unwrap_err();
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some("folder"));
        assert_eq!(e.message, reason);
    }
}

#[tokio::test]
async fn a_database_is_due_after_90_days_and_its_list_goes_into_the_audit_before_it_is_deleted() {
    let f = with_records().await;
    let file = orphan_copy(&f);
    let store = reopen(&f);

    // 89 days after the stamp (2027-01-15): not due. 91 days: due.
    let stamp = 1_800_000_000_000_i64;
    assert!(
        set_aside::sweep_due(&store, stamp + 89 * DAY)
            .unwrap()
            .is_none()
    );
    let sweep = set_aside::sweep_due(&store, stamp + 91 * DAY)
        .unwrap()
        .unwrap();
    assert_eq!(sweep.files.len(), 1);
    assert_eq!(sweep.files[0].file, ORPHAN);
    assert_eq!(sweep.files[0].set_aside_changes, Some(5));
    let numbers: Vec<_> = sweep.files[0]
        .invoices
        .iter()
        .map(|i| i.invoice_no.as_str())
        .collect();
    assert_eq!(numbers, vec!["INV/26-27/0042", "INV/26-27/0043"]);
    assert!(sweep.is_consistent());
    assert!(file.exists(), "listing deletes nothing");

    // The Hub writes the record, then deletes the file.
    let before = reopen(&f).changes().unwrap().len();
    f.writer.record_orphan_sweep(sweep.clone()).await.unwrap();
    let rows = reopen(&f).changes().unwrap();
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].intent_kind, "ops.orphan_sweep");
    assert_eq!(rows[before].actor_id, ORPHAN_SWEEP_SYSTEM_ACTOR);
    let kept = text(&setting(&reopen(&f), "ops.last_orphan_sweep"), "value");
    assert!(kept.contains("INV/26-27/0042") && kept.contains("11800"));
    assert!(
        !kept.contains("phone") && !kept.contains("name"),
        "no personal data: {kept}"
    );
    assert_eq!(
        arena_store::delete_orphans(&f.path, &[ORPHAN.to_owned()]),
        1
    );
    assert!(!file.exists());
    assert!(
        set_aside::build(&reopen(&f))
            .unwrap()
            .list
            .databases
            .is_empty()
    );
}

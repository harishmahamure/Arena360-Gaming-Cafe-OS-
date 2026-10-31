//! What a promote or restore set aside, for System -> Backup (Story 9.23): the forks this Hub's
//! epoch came from (read from the `lease.promote` and `ops.restore` records in the change log),
//! the databases in `orphaned/` and the invoice numbers only they hold, the CSV of all that for
//! the accountant, and the sweep of databases older than 90 days (listed into the audit before
//! they are deleted).
//!
//! Everything here only reads: the Hub's one writer records the sweep, and the files go afterwards.

use std::collections::HashMap;
use std::path::Path;

use arena_proto::{
    AppError, ErrorCode, LeasePromote, OpsRestore, OrphanSweep, OrphanSweepFile,
    OrphanSweepInvoice, SET_ASIDE_LIST_INVOICES_CAP, SetAsideDatabase, SetAsideExported,
    SetAsideFork, SetAsideInvoice, SetAsideKind, SetAsideList, set_aside_csv, set_aside_file_name,
};
use arena_store::{
    AuditFilter, ChangeOp, RowValue, Store, count_changes_above, scan_orphan_databases,
};

use crate::audit::decode_image;

/// The most records of one kind read from the change log.
const FORKS_PER_KIND: u32 = 200;

/// The list for the screen and every orphaned invoice for the CSV.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetAside {
    pub list: SetAsideList,
    pub all_invoices: Vec<SetAsideInvoice>,
}

fn internal(what: &str) -> AppError {
    AppError::new(ErrorCode::Internal, what.to_owned())
}

/// The JSON value of the settings row a record's change row holds.
fn record_value(store: &Store, epoch: &str, seq: i64) -> Result<Option<String>, AppError> {
    let Some(change) = store.change(epoch, seq)? else {
        return Ok(None);
    };
    if change.op == ChangeOp::Delete {
        return Ok(None);
    }
    let image = decode_image(&change.row)?;
    Ok(match image.get("value") {
        Some(RowValue::Text(t)) => Some(t.clone()),
        _ => None,
    })
}

/// Every promote and restore on record, newest first. A change row whose record does not parse is
/// skipped.
fn read_forks(store: &Store) -> Result<Vec<Fork>, AppError> {
    let mut out: Vec<Fork> = Vec::new();
    for kind in ["lease.promote", "ops.restore"] {
        let filter = AuditFilter {
            intent_kind: Some(kind.to_owned()),
            ..Default::default()
        };
        for row in store.audit_page(&filter, FORKS_PER_KIND)? {
            let Some(value) = record_value(store, &row.epoch, row.seq)? else {
                continue;
            };
            if kind == "lease.promote" {
                if let Some(p) = LeasePromote::parse(&value) {
                    out.push((
                        SetAsideFork {
                            kind: SetAsideKind::Promote,
                            at: p.promoted_at,
                            old_epoch: p.old_epoch,
                            new_epoch: p.new_epoch,
                            fork_seq: p.fork_seq,
                            by: p.by,
                            forced: p.force,
                            set_aside_changes: p.lag_rows,
                            orphaned_file: None,
                        },
                        None,
                    ));
                }
            } else if let Some(r) = OpsRestore::parse(&value) {
                out.push((
                    SetAsideFork {
                        kind: SetAsideKind::Restore,
                        at: r.restored_at,
                        old_epoch: r.old_epoch.clone(),
                        new_epoch: r.new_epoch.clone(),
                        fork_seq: r.header.last_seq,
                        by: r.by.clone(),
                        forced: false,
                        set_aside_changes: r.rows_set_aside,
                        orphaned_file: Some(r.orphaned_file.clone()),
                    },
                    Some(r),
                ));
            }
        }
    }
    out.sort_by(|a, b| {
        b.0.at
            .cmp(&a.0.at)
            .then_with(|| a.0.new_epoch.cmp(&b.0.new_epoch))
    });
    Ok(out)
}

/// A fork with the restore record it came from, when it came from one.
type Fork = (SetAsideFork, Option<OpsRestore>);

/// The databases in `orphaned/` beside the store's file with what each holds, and the forks.
fn scan(store: &Store) -> Result<(Vec<Fork>, Vec<ScannedDb>), AppError> {
    let forks = read_forks(store)?;
    let db_path = store.path().to_path_buf();
    let found = store.read_with(|conn| Ok(scan_orphan_databases(&db_path, conn)))?;
    let by_file: HashMap<&str, &OpsRestore> = forks
        .iter()
        .filter_map(|(_, r)| r.as_ref())
        .map(|r| (r.orphaned_file.as_str(), r))
        .collect();
    let dbs = found
        .into_iter()
        .map(|db| {
            let set_aside_changes = by_file
                .get(db.file.name.as_str())
                .and_then(|r| count_changes_above(&db.file.path, &r.old_epoch, r.header.last_seq));
            ScannedDb {
                db,
                set_aside_changes,
            }
        })
        .collect();
    Ok((forks, dbs))
}

struct ScannedDb {
    db: arena_store::OrphanDb,
    set_aside_changes: Option<i64>,
}

/// The set-aside list of this Hub. Blocking (SQLite and disk).
pub fn build(store: &Store) -> Result<SetAside, AppError> {
    let (forks, dbs) = scan(store)?;
    let mut databases = Vec::new();
    let mut all_invoices = Vec::new();
    for d in &dbs {
        databases.push(SetAsideDatabase {
            file: d.db.file.name.clone(),
            set_aside_changes: d.set_aside_changes,
            invoices: i64::try_from(d.db.invoices.len()).unwrap_or(i64::MAX),
            unreadable: d.db.unreadable,
        });
        all_invoices.extend(d.db.invoices.iter().map(|i| SetAsideInvoice {
            invoice_no: i.invoice_no.clone(),
            amount_minor: i.amount_minor,
            at: i.at,
            database: d.db.file.name.clone(),
        }));
    }
    all_invoices.sort_by(|a, b| {
        a.invoice_no
            .cmp(&b.invoice_no)
            .then_with(|| a.database.cmp(&b.database))
    });
    let invoice_total = i64::try_from(all_invoices.len()).unwrap_or(i64::MAX);
    let shown: Vec<SetAsideInvoice> = all_invoices
        .iter()
        .take(SET_ASIDE_LIST_INVOICES_CAP)
        .cloned()
        .collect();
    Ok(SetAside {
        list: SetAsideList {
            forks: forks.into_iter().map(|(f, _)| f).collect(),
            databases,
            invoices_truncated: all_invoices.len() > shown.len(),
            invoices: shown,
            invoice_total,
        },
        all_invoices,
    })
}

/// The databases in `orphaned/` older than 90 days at `now_ms`, as the record to write into the
/// audit before they are deleted; `None` when none is due. Blocking (SQLite and disk).
pub fn sweep_due(store: &Store, now_ms: i64) -> Result<Option<OrphanSweep>, AppError> {
    let (_, dbs) = scan(store)?;
    let files: Vec<OrphanSweepFile> = dbs
        .into_iter()
        .filter(|d| d.db.file.is_due(now_ms))
        .map(|d| OrphanSweepFile {
            file: d.db.file.name,
            set_aside_changes: d.set_aside_changes,
            invoices: d
                .db
                .invoices
                .into_iter()
                .map(|i| OrphanSweepInvoice {
                    invoice_no: i.invoice_no,
                    amount_minor: i.amount_minor,
                })
                .collect(),
        })
        .collect();
    Ok((!files.is_empty()).then_some(OrphanSweep {
        files,
        swept_at: now_ms,
    }))
}

/// Writes the CSV of the list into `folder` as `<branch_code>-set-aside-<yyyy-mm-dd-hhmm>.csv`,
/// the time in the branch zone. `VALIDATION_FAILED` on `folder` says why the location cannot take
/// it (`not_set`, `unsupported`, `not_found`, `not_dir`, `not_writable`). Blocking (disk).
pub fn export_csv(
    set_aside: &SetAside,
    folder: &str,
    branch_code: &str,
    tz: &str,
    currency: &str,
    exponent: u8,
    now_ms: i64,
) -> Result<(SetAsideExported, std::path::PathBuf), AppError> {
    let bad = |reason: &str| AppError::with_field(ErrorCode::ValidationFailed, "folder", reason);
    if folder.trim().is_empty() {
        return Err(bad("not_set"));
    }
    if arena_proto::is_backup_tree_uri(folder) {
        return Err(bad("unsupported"));
    }
    let dir = Path::new(folder);
    if !dir.exists() {
        return Err(bad("not_found"));
    }
    if !dir.is_dir() {
        return Err(bad("not_dir"));
    }
    let file_name = set_aside_file_name(branch_code, now_ms, tz);
    let bytes = set_aside_csv(
        &set_aside.list,
        &set_aside.all_invoices,
        tz,
        currency,
        exponent,
    );
    let part = dir.join(format!(".{file_name}.part"));
    let target = dir.join(&file_name);
    let written = std::fs::write(&part, &bytes).and_then(|()| std::fs::rename(&part, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
        return Err(bad("not_writable"));
    }
    let size = i64::try_from(bytes.len()).map_err(|_| internal("the list is too large"))?;
    Ok((
        SetAsideExported {
            file_name,
            bytes: size,
        },
        target,
    ))
}

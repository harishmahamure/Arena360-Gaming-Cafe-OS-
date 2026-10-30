//! "Export everything" (Story 9.6): one folder holding the Story 9.4 database file, every Hub
//! report of Story 5.32 for a range as CSV, and a `README.txt` that says what is what and links
//! the format reference.
//!
//! The database file is made first (it is the part that matters and the slow one); each report
//! is then written on its own, so a report that cannot be built (a Hub without a key store cannot
//! build the Players report) is listed under "Not exported" in the README rather than failing the
//! export. A folder is only left behind when the database file was written.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use arena_proto::{
    AppError, EXPORT_FORMAT_REFERENCE_URL, ErrorCode, EverythingExported, ExportFormat,
    ReportExportRequest, everything_folder_name,
};
use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;

use crate::locale::business_day_of;
use crate::players::KEK_LEN;
use crate::reports::parse_range;
use crate::snapshot::SnapshotReader;

/// The reports that cover a range, in the order they are written.
const RANGED: [&str; 6] = [
    "daily-sales",
    "sessions",
    "players",
    "staff",
    "stock-moves",
    "stock-waste",
];

/// The `YYYY-MM` of every month the range touches, first to last.
fn months(from: NaiveDate, to: NaiveDate) -> Vec<String> {
    let mut out = Vec::new();
    let (mut year, mut month) = (from.year(), from.month());
    while (year, month) <= (to.year(), to.month()) {
        out.push(format!("{year:04}-{month:02}"));
        if month == 12 {
            (year, month) = (year + 1, 1);
        } else {
            month += 1;
        }
    }
    out
}

fn folder_problem(reason: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "folder", reason)
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), AppError> {
    std::fs::write(dir.join(name), bytes).map_err(|e| {
        tracing::warn!(error = %e, file = name, "could not write an export file");
        folder_problem("not_writable")
    })
}

/// The text of `README.txt`.
fn readme(
    branch: &str,
    (from, to): (&str, &str),
    db_file: &str,
    reports: &[String],
    skipped: &[String],
) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Arena 360 export of {branch}");
    let _ = writeln!(text, "Business days {from} to {to}");
    let _ = writeln!(text);
    let _ = writeln!(
        text,
        "This folder holds your data in formats you can open without Arena 360."
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "{db_file}");
    let _ = writeln!(
        text,
        "  The whole branch database (every day, not only the range). It is a zstd-compressed\n  \
         SQLite database inside a small header. The format reference explains every table and\n  \
         column and how to open the file with the free zstd and sqlite3 tools:\n  \
         {EXPORT_FORMAT_REFERENCE_URL}"
    );
    let _ = writeln!(text);
    let _ = writeln!(
        text,
        "Reports for {from} to {to} (CSV, open in any spreadsheet):"
    );
    for name in reports {
        let _ = writeln!(text, "  {name}");
    }
    if !skipped.is_empty() {
        let _ = writeln!(text);
        let _ = writeln!(text, "Not exported:");
        for id in skipped {
            let _ = writeln!(text, "  {id}");
        }
    }
    text
}

impl SnapshotReader {
    /// Writes the "Export everything" folder for `from` to `to` inside `folder` (the backup
    /// location), and answers what it holds and the folder's path. `VALIDATION_FAILED` on `from`
    /// or `to` (a bad or too long range) or on `folder` (as [`Self::export_to_folder`]). The
    /// folder is removed again when the database file could not be written. Blocking (disk).
    pub fn export_everything(
        &self,
        folder: &str,
        (from, to): (&str, &str),
        now_ms: i64,
        kek: Option<&[u8; KEK_LEN]>,
    ) -> Result<(EverythingExported, PathBuf), AppError> {
        parse_range(from, to)?;
        let range = (
            NaiveDate::parse_from_str(from, "%Y-%m-%d").expect("parsed above"),
            NaiveDate::parse_from_str(to, "%Y-%m-%d").expect("parsed above"),
        );
        let branch = self
            .branch()?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "the branch is not set up"))?;
        let locale = self.locale(now_ms)?;
        let tz_name = locale.as_ref().map_or("UTC", |l| l.tz.as_str());
        let base = everything_folder_name(&branch.branch_code, from, to, now_ms, tz_name);
        if folder.trim().is_empty() {
            return Err(folder_problem("not_set"));
        }
        if arena_proto::is_backup_tree_uri(folder) {
            return Err(folder_problem("unsupported"));
        }

        // A second export in the same minute gets a folder of its own.
        let root = Path::new(folder);
        let mut name = base.clone();
        let mut dir = root.join(&name);
        for n in 2..100 {
            if !dir.exists() {
                break;
            }
            name = format!("{base}-{n}");
            dir = root.join(&name);
        }
        std::fs::create_dir(&dir).map_err(|_| match std::fs::metadata(root) {
            Err(_) => folder_problem("not_found"),
            Ok(m) if !m.is_dir() => folder_problem("not_dir"),
            Ok(_) => folder_problem("not_writable"),
        })?;
        let written = self.fill_everything(&dir, &name, (from, to), range, now_ms, kek);
        if written.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        written.map(|done| (done, dir))
    }

    fn fill_everything(
        &self,
        dir: &Path,
        folder_name: &str,
        (from, to): (&str, &str),
        (first, last): (NaiveDate, NaiveDate),
        now_ms: i64,
        kek: Option<&[u8; KEK_LEN]>,
    ) -> Result<EverythingExported, AppError> {
        let (export, _) = self.export_to_folder(&dir.to_string_lossy(), now_ms)?;

        let locale = self.locale(now_ms)?;
        let tz: Tz = locale
            .as_ref()
            .and_then(|l| l.tz.parse().ok())
            .unwrap_or(Tz::UTC);
        let day_start = locale.as_ref().map_or(0, |l| l.day_start_min);
        let in_range = |closed_at: i64| {
            let day = business_day_of(closed_at, tz, day_start);
            day >= first && day <= last
        };

        let ask = |report: &str| ReportExportRequest {
            report: report.to_owned(),
            format: ExportFormat::Csv,
            folder: String::new(),
            from: Some(from.to_owned()),
            to: Some(to.to_owned()),
            month: None,
            shift_id: None,
            product: None,
            reason: None,
        };
        // (report id, request) in the order the files are written.
        let mut wanted: Vec<(String, ReportExportRequest)> = Vec::new();
        for id in RANGED {
            wanted.push((id.to_owned(), ask(id)));
        }
        wanted.push(("shifts".to_owned(), ask("shifts")));
        match self.shift_list() {
            Ok(list) => {
                for shift in list.shifts.iter().filter(|s| in_range(s.closed_at)) {
                    let mut req = ask("z-report");
                    req.shift_id = Some(shift.shift_id.clone());
                    wanted.push(("z-report".to_owned(), req));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "export everything: the shift list could not be read");
                wanted.push(("z-report".to_owned(), ask("z-report")));
            }
        }
        for month in months(first, last) {
            let mut req = ask("gst");
            req.month = Some(month);
            wanted.push(("gst".to_owned(), req));
        }
        wanted.push(("stock-on-hand".to_owned(), ask("stock-on-hand")));
        wanted.push(("stock-expiring".to_owned(), ask("stock-expiring")));

        let mut reports = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for (id, req) in wanted {
            let made = self
                .export_report(&req, now_ms, kek)
                .and_then(|(file, bytes)| write_file(dir, &file, &bytes).map(|()| file));
            match made {
                Ok(file) => reports.push(file),
                // A branch that is not GST-registered has no GST return to export.
                Err(e) if id == "gst" && e.field.as_deref() == Some("tax") => {}
                Err(e) => {
                    tracing::warn!(report = %id, error = %e, "export everything: a report was left out");
                    if !skipped.contains(&id) {
                        skipped.push(id);
                    }
                }
            }
        }

        let branch_name = self.branch()?.map(|b| b.name).unwrap_or_default();
        write_file(
            dir,
            "README.txt",
            readme(
                &branch_name,
                (from, to),
                &export.file_name,
                &reports,
                &skipped,
            )
            .as_bytes(),
        )?;
        Ok(EverythingExported {
            folder_name: folder_name.to_owned(),
            export,
            reports,
            skipped,
        })
    }
}

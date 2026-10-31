//! What a promote or a restore set aside (Story 9.23, AD-4, AD-26).
//!
//! A Hub that opens a new epoch leaves the rows of the old one behind: a restore moves the replaced
//! database into `orphaned/`, a promote leaves them on the old main PC. System -> Backup -> "Set
//! aside after promote or restore" lists each fork and, from every database in `orphaned/`, the
//! changes set aside and the invoice numbers that only that database holds, so the owner's
//! accountant can account for them; "Export list" writes the same as CSV to the backup location.
//!
//! `ops.orphan_sweep` ([`OrphanSweep`]) is the system intent the daily housekeeping submits before
//! it deletes an `orphaned/` database older than 90 days: invoice numbers and amounts only, no
//! personal data, and it sets `ops.last_orphan_sweep`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{GST_ARCHIVE_ORPHANS_CAP, restore_time_text};

/// The settings key `ops.orphan_sweep` writes: the last sweep that deleted something.
pub const KEY_OPS_LAST_ORPHAN_SWEEP: &str = "ops.last_orphan_sweep";
/// Before any sweep.
pub const DEFAULT_OPS_LAST_ORPHAN_SWEEP: &str = "";
/// The actor id of the system intent `ops.orphan_sweep`.
pub const ORPHAN_SWEEP_SYSTEM_ACTOR: &str = "hub-orphan-sweep";
/// The most orphaned invoices [`SetAsideList::invoices`] holds; the CSV and the audit carry all.
pub const SET_ASIDE_LIST_INVOICES_CAP: usize = 2_000;
/// The longest text (a file name, an epoch, an invoice number) a record keeps.
const MAX_TEXT_CHARS: usize = 128;

/// How the fork came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SetAsideKind {
    /// A standby was promoted (`lease.promote`).
    Promote,
    /// The branch was restored from a file or from the cloud backup (`ops.restore`).
    Restore,
}

impl SetAsideKind {
    /// The word the list and the CSV use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Restore => "restore",
        }
    }
}

/// One fork this Hub's epoch came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetAsideFork {
    pub kind: SetAsideKind,
    /// When the new epoch opened, UTC ms.
    #[ts(type = "number")]
    pub at: i64,
    pub old_epoch: String,
    pub new_epoch: String,
    /// The last `seq` of the old epoch this Hub kept.
    #[ts(type = "number")]
    pub fork_seq: i64,
    /// The Manager or Owner who confirmed it (their display name).
    #[serde(default)]
    pub by: Option<String>,
    /// A promote forced past a fresh lease or a refused claim (Story 9.22).
    pub forced: bool,
    /// Old-epoch changes this Hub did not keep: a promote's lag at the time, a restore's rows
    /// above the file.
    #[ts(type = "number")]
    pub set_aside_changes: i64,
    /// The file inside `orphaned/` a restore moved the replaced database to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub orphaned_file: Option<String>,
}

/// An invoice number that only an `orphaned/` database holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetAsideInvoice {
    pub invoice_no: String,
    /// The order total in minor units.
    #[ts(type = "number")]
    pub amount_minor: i64,
    /// When it was settled (else created), UTC ms.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub at: Option<i64>,
    /// The database in `orphaned/` that holds it.
    pub database: String,
}

/// A database in `orphaned/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetAsideDatabase {
    pub file: String,
    /// Changes of the old epoch above the fork; `None` when no restore on record names the file.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub set_aside_changes: Option<i64>,
    /// How many invoice numbers only this database holds.
    #[ts(type = "number")]
    pub invoices: i64,
    /// The file could not be opened; its invoices are not listed.
    pub unreadable: bool,
}

/// The answer of `GET /ui/backup/set-aside`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetAsideList {
    /// Newest first.
    pub forks: Vec<SetAsideFork>,
    pub databases: Vec<SetAsideDatabase>,
    /// Orphaned invoices, in invoice order, at most [`SET_ASIDE_LIST_INVOICES_CAP`].
    pub invoices: Vec<SetAsideInvoice>,
    /// More orphaned invoices exist than `invoices` holds (the CSV has them all).
    pub invoices_truncated: bool,
    /// How many orphaned invoices there are in all.
    #[ts(type = "number")]
    pub invoice_total: i64,
}

/// The answer of `POST /ui/backup/set-aside/export`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetAsideExported {
    pub file_name: String,
    #[ts(type = "number")]
    pub bytes: i64,
}

/// `<branch_code>-set-aside-<yyyy-mm-dd-hhmm>.csv`, the time in the branch zone.
pub fn set_aside_file_name(branch_code: &str, now_ms: i64, tz: &str) -> String {
    let stamp = restore_time_text(now_ms, tz)
        .replace(' ', "-")
        .replace(':', "");
    let code: String = branch_code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    let code = if code.is_empty() { "branch" } else { &code };
    format!("{code}-set-aside-{stamp}.csv")
}

/// An invoice of a swept database, as the audit keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrphanSweepInvoice {
    pub invoice_no: String,
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// A database the sweep is about to delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrphanSweepFile {
    pub file: String,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub set_aside_changes: Option<i64>,
    pub invoices: Vec<OrphanSweepInvoice>,
}

/// What `ops.orphan_sweep` records, and (as JSON) the value of `ops.last_orphan_sweep`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OrphanSweep {
    pub files: Vec<OrphanSweepFile>,
    /// When the sweep ran, UTC ms.
    #[ts(type = "number")]
    pub swept_at: i64,
}

fn text_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

impl OrphanSweep {
    /// How many invoice numbers the record holds.
    pub fn invoice_count(&self) -> usize {
        self.files.iter().map(|f| f.invoices.len()).sum()
    }

    /// Whether the facts could describe a sweep this build performs.
    pub fn is_consistent(&self) -> bool {
        !self.files.is_empty()
            && self.swept_at > 0
            && self.invoice_count() <= GST_ARCHIVE_ORPHANS_CAP
            && self.files.iter().all(|f| {
                text_ok(&f.file)
                    && !f.file.contains(['/', '\\'])
                    && f.set_aside_changes.is_none_or(|n| n >= 0)
                    && f.invoices.iter().all(|i| text_ok(&i.invoice_no))
            })
    }

    /// The value of `ops.last_orphan_sweep`, or `None` when it is empty or malformed.
    pub fn parse(value: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(value).ok()?;
        parsed.is_consistent().then_some(parsed)
    }

    /// The JSON stored in `ops.last_orphan_sweep`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// A CSV field: quoted when it holds a comma, quote or line break, and a text that a spreadsheet
/// would read as a formula is prefixed with `'`.
fn csv_field(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// Minor units as a plain number in major units (`1180` with exponent 2 is `11.80`).
fn plain_money(minor: i64, exponent: u8) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    if exponent == 0 {
        return format!("{sign}{abs}");
    }
    let scale = 10_u64.pow(u32::from(exponent));
    format!(
        "{sign}{}.{:0width$}",
        abs / scale,
        abs % scale,
        width = usize::from(exponent)
    )
}

/// The CSV of a set-aside list for the accountant: UTF-8 with a byte-order mark, CRLF lines, one
/// table each for the forks, the databases and the orphaned invoices (all of them, not only the
/// ones the screen shows), headed by their titles and separated by a blank line. Money is in major
/// units followed by the currency.
pub fn set_aside_csv(
    list: &SetAsideList,
    all_invoices: &[SetAsideInvoice],
    tz: &str,
    currency: &str,
    exponent: u8,
) -> Vec<u8> {
    let mut out = String::from('\u{FEFF}');
    let mut line = |cells: &[String]| {
        out.push_str(
            &cells
                .iter()
                .map(|c| csv_field(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    };
    let s = |t: &str| t.to_owned();
    line(&[s("Forks")]);
    line(&[
        s("Time"),
        s("Kind"),
        s("Old epoch"),
        s("New epoch"),
        s("Fork seq"),
        s("By"),
        s("Forced"),
        s("Changes set aside"),
    ]);
    for f in &list.forks {
        line(&[
            restore_time_text(f.at, tz),
            s(f.kind.as_str()),
            f.old_epoch.clone(),
            f.new_epoch.clone(),
            f.fork_seq.to_string(),
            f.by.clone().unwrap_or_default(),
            s(if f.forced { "yes" } else { "no" }),
            f.set_aside_changes.to_string(),
        ]);
    }
    line(&[]);
    line(&[s("Set-aside databases")]);
    line(&[
        s("File"),
        s("Changes set aside"),
        s("Orphaned invoices"),
        s("Readable"),
    ]);
    for d in &list.databases {
        line(&[
            d.file.clone(),
            d.set_aside_changes
                .map(|n| n.to_string())
                .unwrap_or_default(),
            d.invoices.to_string(),
            s(if d.unreadable { "no" } else { "yes" }),
        ]);
    }
    line(&[]);
    line(&[s("Orphaned invoices")]);
    line(&[
        s("Invoice number"),
        s("Amount"),
        s("Currency"),
        s("Time"),
        s("Database"),
    ]);
    for i in all_invoices {
        line(&[
            i.invoice_no.clone(),
            plain_money(i.amount_minor, exponent),
            s(currency),
            i.at.map(|at| restore_time_text(at, tz)).unwrap_or_default(),
            i.database.clone(),
        ]);
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_800_000_000_000;

    fn fork(kind: SetAsideKind) -> SetAsideFork {
        SetAsideFork {
            kind,
            at: T,
            old_epoch: "3:OLD".into(),
            new_epoch: "4:NEW".into(),
            fork_seq: 120,
            by: Some("Asha".into()),
            forced: true,
            set_aside_changes: 12,
            orphaned_file: None,
        }
    }

    fn invoice(no: &str, minor: i64) -> SetAsideInvoice {
        SetAsideInvoice {
            invoice_no: no.into(),
            amount_minor: minor,
            at: Some(T),
            database: "3-OLD-20270115T133000.db".into(),
        }
    }

    #[test]
    fn money_is_written_in_major_units() {
        assert_eq!(plain_money(1180, 2), "11.80");
        assert_eq!(plain_money(5, 2), "0.05");
        assert_eq!(plain_money(-250, 2), "-2.50");
        assert_eq!(plain_money(700, 0), "700");
        assert_eq!(plain_money(1234, 3), "1.234");
    }

    #[test]
    fn the_csv_has_three_tables_with_a_bom_and_crlf() {
        let list = SetAsideList {
            forks: vec![fork(SetAsideKind::Promote)],
            databases: vec![SetAsideDatabase {
                file: "3-OLD-20270115T133000.db".into(),
                set_aside_changes: Some(12),
                invoices: 1,
                unreadable: false,
            }],
            ..Default::default()
        };
        let csv = set_aside_csv(
            &list,
            &[invoice("INV/26-27/0042", 11_800), invoice("=BAD", 5)],
            "Asia/Kolkata",
            "INR",
            2,
        );
        let text = String::from_utf8(csv).unwrap();
        assert!(text.starts_with('\u{FEFF}'));
        assert!(text.contains("\r\nForks\r\n") || text.contains("Forks\r\n"));
        assert!(text.contains("promote,3:OLD,4:NEW,120,Asha,yes,12"));
        assert!(text.contains("3-OLD-20270115T133000.db,12,1,yes"));
        assert!(text.contains("INV/26-27/0042,118.00,INR,"));
        assert!(text.contains("'=BAD,0.05,INR"), "formulas are defused");
        assert_eq!(text.matches("\r\n\r\n").count(), 2);
    }

    #[test]
    fn a_file_name_carries_the_branch_code_and_the_local_minute() {
        // 1_800_000_000_000 ms is 2027-01-15 08:00 UTC, 13:30 in Kolkata.
        assert_eq!(
            set_aside_file_name("LVL01", T, "Asia/Kolkata"),
            "LVL01-set-aside-2027-01-15-1330.csv"
        );
        assert_eq!(
            set_aside_file_name("../x", T, "UTC"),
            "x-set-aside-2027-01-15-0800.csv"
        );
        assert!(set_aside_file_name("", T, "UTC").starts_with("branch-set-aside-"));
    }

    fn sweep() -> OrphanSweep {
        OrphanSweep {
            files: vec![OrphanSweepFile {
                file: "3-OLD-20270115T133000.db".into(),
                set_aside_changes: Some(4),
                invoices: vec![OrphanSweepInvoice {
                    invoice_no: "INV/26-27/0042".into(),
                    amount_minor: 11_800,
                }],
            }],
            swept_at: T,
        }
    }

    #[test]
    fn a_sweep_record_round_trips_and_checks_itself() {
        let ok = sweep();
        assert!(ok.is_consistent());
        assert_eq!(OrphanSweep::parse(&ok.to_json()), Some(ok.clone()));
        assert_eq!(ok.invoice_count(), 1);

        assert_eq!(OrphanSweep::parse(""), None);
        let mut empty = ok.clone();
        empty.files.clear();
        assert!(!empty.is_consistent(), "a sweep deletes something");
        let mut path = ok.clone();
        path.files[0].file = "orphaned/x.db".into();
        assert!(!path.is_consistent());
        let mut number = ok.clone();
        number.files[0].invoices[0].invoice_no = "bad\nno".into();
        assert!(!number.is_consistent());
        let mut negative = ok;
        negative.files[0].set_aside_changes = Some(-1);
        assert!(!negative.is_consistent());
    }
}

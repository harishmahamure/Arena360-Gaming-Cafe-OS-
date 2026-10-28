//! CSV import of players with a dry-run (Story 5.34).
//!
//! A row is a phone, a name, an opening wallet balance and a credit limit. The dry-run checks each
//! row (a valid phone, a name, amounts that are not negative), finds rows of one file that share a
//! phone, and sets aside rows whose phone already has a profile: those are *matched* and left as
//! they are, which also makes a retry of a half-finished import safe. Nothing here writes; the
//! Hub seals the rows that passed ([`crate::players::seal_import`]) and commits them as
//! `import.players` intents.

use std::collections::{HashMap, HashSet};

use arena_proto::{
    AppError, CREDIT_LIMIT_MAX_MINOR, IMPORT_PLAYER_COLUMNS, ImportFailure, ImportPlayerRow,
    PLAYER_NAME_MAX_CHARS, PlayerImportPreview, WALLET_TOPUP_MAX_MINOR, normalize_phone,
    valid_phone, valid_username,
};

use crate::import_csv::CsvRecord;
use crate::import_file::{File, Header, Read, line_of, problems, read_file};
use crate::import_products::{Problem, number, problem, take};
use crate::players::{KEK_LEN, phone_lookup};

/// The file name of the template.
pub const TEMPLATE_FILE: &str = "players-template.csv";

/// The template: the exact header and two sample rows. It passes its own dry-run.
pub fn template() -> String {
    let mut out = IMPORT_PLAYER_COLUMNS.join(",");
    out.push_str("\r\n");
    out.push_str("+919876500001,Asha Rao,500,0\r\n");
    out.push_str("+919876500002,Ravi Kumar,0,2000\r\n");
    out
}

/// What the Hub holds that a player import is checked against.
pub(crate) struct PlayerContext {
    /// Phone lookups of the live players.
    pub lookups: HashSet<String>,
    /// Usernames in use.
    pub usernames: HashSet<String>,
}

/// The context read from the store (outside a write transaction).
pub(crate) fn context(store: &arena_store::Store) -> Result<PlayerContext, AppError> {
    let (lookups, usernames) = store.player_import_keys()?;
    Ok(PlayerContext { lookups, usernames })
}

/// A row that passed: what the Hub seals and commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLine {
    /// The phone without spaces or dashes, with its `+`.
    pub phone: String,
    pub name: String,
    pub balance_minor: i64,
    pub credit_limit_minor: i64,
}

/// A file and the new players in it.
pub(crate) struct Analysis {
    pub preview: PlayerImportPreview,
    /// The rows that create a player, in file order.
    pub rows: Vec<PlayerLine>,
}

fn unusable(failures: Vec<ImportFailure>) -> Analysis {
    Analysis {
        preview: PlayerImportPreview {
            rows: 0,
            new: 0,
            matched: 0,
            failed: 0,
            with_balance: 0,
            with_credit: 0,
            failures,
            truncated: false,
            unusable: true,
            can_commit: false,
            can_commit_valid: false,
        },
        rows: Vec::new(),
    }
}

/// The checks a single row passes or fails on its own.
fn parse_row(rec: &CsvRecord, header: &Header, exp: u32) -> Result<PlayerLine, Vec<Problem>> {
    let mut found = Vec::new();
    if header.overflows(rec) {
        found.push(problem(
            "",
            "bad_value",
            "the row has more values than the header has columns",
        ));
    }
    let raw_phone = header.cell(rec, "phone");
    let phone = normalize_phone(raw_phone);
    if raw_phone.is_empty() {
        found.push(problem("phone", "required", "phone is required"));
    } else if !valid_phone(&phone) {
        found.push(problem(
            "phone",
            "bad_phone",
            "phone must start with + and the country code, with 8 to 15 digits",
        ));
    }
    let name = header.cell(rec, "name");
    if name.is_empty() {
        found.push(problem("name", "required", "name is required"));
    } else if name.chars().count() > PLAYER_NAME_MAX_CHARS {
        found.push(problem("name", "too_long", "name must be 1-80 characters"));
    } else if name.chars().any(char::is_control) {
        found.push(problem(
            "name",
            "bad_value",
            "name has characters that are not allowed",
        ));
    }
    let amount = |found: &mut Vec<Problem>, column: &str, what: &str, max: i64| {
        let text = header.cell(rec, column);
        if text.is_empty() {
            return 0;
        }
        let v = take(found, number(column, text, exp, what), 0);
        if v > max {
            found.push(problem(
                column,
                "out_of_range",
                format!("{what} is too large"),
            ));
            return 0;
        }
        v
    };
    let balance_minor = amount(&mut found, "balance", "balance", WALLET_TOPUP_MAX_MINOR);
    let credit_limit_minor = amount(
        &mut found,
        "credit_limit",
        "credit limit",
        CREDIT_LIMIT_MAX_MINOR,
    );
    if !found.is_empty() {
        return Err(found);
    }
    Ok(PlayerLine {
        phone,
        name: name.to_owned(),
        balance_minor,
        credit_limit_minor,
    })
}

/// Reads `csv` against `ctx` (a dry-run): the counts, every problem with its line, and the rows
/// that create a player. `exponent` is the currency's decimal places; `kek` turns a phone into
/// its lookup so it can be compared with the profiles the Hub holds. `Err` only for a file over
/// the size or row limits.
pub(crate) fn analyse(
    csv: &str,
    ctx: &PlayerContext,
    exponent: u8,
    kek: &[u8; KEK_LEN],
) -> Result<Analysis, AppError> {
    let File {
        header,
        records,
        unknown,
    } = match read_file(csv, &IMPORT_PLAYER_COLUMNS, &["phone", "name"])? {
        Read::Unusable(failures) => return Ok(unusable(failures)),
        Read::File(f) => f,
    };
    let exp = u32::from(exponent);
    let mut bad: Vec<(i64, Problem)> = Vec::new();
    let mut seen: HashMap<String, i64> = HashMap::new();
    let mut rows = Vec::new();
    let (mut matched, mut with_balance, mut with_credit) = (0i64, 0i64, 0i64);
    for rec in &records {
        let line = line_of(rec);
        let row = match parse_row(rec, &header, exp) {
            Ok(row) => row,
            Err(found) => {
                bad.extend(found.into_iter().map(|p| (line, p)));
                continue;
            }
        };
        let lookup = phone_lookup(kek, &row.phone);
        if let Some(first) = seen.get(&lookup) {
            bad.push((
                line,
                problem(
                    "phone",
                    "duplicate_phone",
                    format!("the phone is already used by row {first}"),
                ),
            ));
            continue;
        }
        seen.insert(lookup.clone(), line);
        if ctx.lookups.contains(&lookup) {
            matched += 1;
            continue;
        }
        with_balance += i64::from(row.balance_minor > 0);
        with_credit += i64::from(row.credit_limit_minor > 0);
        rows.push(row);
    }
    let found = problems(unknown.clone(), bad);
    let new = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let failed = found.failed;
    Ok(Analysis {
        preview: PlayerImportPreview {
            rows: i64::try_from(records.len()).unwrap_or(i64::MAX),
            new,
            matched,
            failed,
            with_balance,
            with_credit,
            failures: found.list,
            truncated: found.truncated,
            unusable: false,
            can_commit: failed == 0 && unknown.is_empty() && new > 0,
            can_commit_valid: new > 0,
        },
        rows,
    })
}

/// A username for a new player: the name's letters and digits joined by dots (at most 20
/// characters), then the phone's last four digits; "-1", "-2" and so on when that is taken. The
/// result is added to `taken`.
pub fn username_for(name: &str, phone: &str, taken: &mut HashSet<String>) -> String {
    let mut base = String::new();
    for word in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !base.is_empty() {
            base.push('.');
        }
        base.push_str(&word.to_ascii_lowercase());
    }
    base.truncate(20);
    let base = base.trim_end_matches('.').to_owned();
    let digits: Vec<char> = phone.chars().filter(char::is_ascii_digit).collect();
    let tail: String = digits[digits.len().saturating_sub(4)..].iter().collect();
    let stem = if base.is_empty() {
        format!("player{tail}")
    } else {
        format!("{base}{tail}")
    };
    let mut candidate = stem.clone();
    let mut n = 0u32;
    while !valid_username(&candidate) || taken.contains(&candidate) {
        n += 1;
        candidate = format!("{stem}-{n}");
    }
    taken.insert(candidate.clone());
    candidate
}

/// Seals the rows that passed ([`crate::players::seal_import`]): a fresh id and a generated
/// username each. `taken` holds the usernames already in use and gains the new ones.
pub fn seal_rows(
    kek: &[u8; KEK_LEN],
    lines: &[PlayerLine],
    taken: &mut HashSet<String>,
) -> Result<Vec<ImportPlayerRow>, AppError> {
    lines
        .iter()
        .map(|l| {
            let username = username_for(&l.name, &l.phone, taken);
            crate::players::seal_import(
                kek,
                &ulid::Ulid::generate().to_string(),
                &username,
                &l.phone,
                &l.name,
                l.balance_minor,
                l.credit_limit_minor,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEK: [u8; KEK_LEN] = [7; KEK_LEN];

    fn run(csv: &str, existing: &[&str]) -> Analysis {
        let ctx = PlayerContext {
            lookups: existing.iter().map(|p| phone_lookup(&KEK, p)).collect(),
            usernames: HashSet::new(),
        };
        analyse(csv, &ctx, 2, &KEK).unwrap()
    }

    #[test]
    fn the_template_passes_its_own_dry_run() {
        let a = run(&template(), &[]);
        assert_eq!(
            (
                a.preview.new,
                a.preview.failed,
                a.preview.with_balance,
                a.preview.with_credit
            ),
            (2, 0, 1, 1)
        );
        assert!(a.preview.can_commit);
    }

    #[test]
    fn a_clean_file_counts_new_and_matched_rows() {
        let a = run(
            "phone,name,balance,credit_limit\n+919800000001,A,100.50,\n+919800000002,B,,500\n+919800000003,C,0,0\n",
            &["+919800000003"],
        );
        assert_eq!(
            (a.preview.new, a.preview.matched, a.preview.failed),
            (2, 1, 0)
        );
        assert_eq!(a.rows[0].balance_minor, 10_050);
        assert_eq!(a.rows[1].credit_limit_minor, 50_000);
        assert!(a.preview.can_commit);
    }

    #[test]
    fn every_bad_row_is_listed_with_its_line_and_reason() {
        let a = run(
            "phone,name,balance,credit_limit\n\
             12345,A,0,0\n\
             +919800000001,,0,0\n\
             +919800000002,B,abc,0\n\
             +919800000003,C,-5,0\n\
             +919800000004,D,0,99999999999\n\
             +919800000005,E,1.234,0\n",
            &[],
        );
        let got: Vec<(i64, &str, &str)> = a
            .preview
            .failures
            .iter()
            .map(|f| (f.row, f.field.as_str(), f.reason.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (2, "phone", "bad_phone"),
                (3, "name", "required"),
                (4, "balance", "bad_number"),
                (5, "balance", "out_of_range"),
                (6, "credit_limit", "out_of_range"),
                (7, "balance", "bad_number"),
            ]
        );
        assert_eq!((a.preview.new, a.preview.failed), (0, 6));
        assert!(!a.preview.can_commit && !a.preview.can_commit_valid);
    }

    #[test]
    fn a_phone_repeated_in_the_file_fails_the_later_row_even_when_written_differently() {
        let a = run(
            "phone,name\n+91 98000-00001,A\n+919800000001,B\n+919800000002,C\n",
            &[],
        );
        assert_eq!(a.preview.failures.len(), 1);
        assert_eq!(a.preview.failures[0].row, 3);
        assert_eq!(a.preview.failures[0].reason, "duplicate_phone");
        assert_eq!(a.preview.new, 2);
        assert!(!a.preview.can_commit && a.preview.can_commit_valid);
        assert_eq!(a.rows[0].phone, "+919800000001");
    }

    #[test]
    fn an_existing_phone_is_matched_not_failed() {
        let a = run("phone,name\n+919800000001,A\n", &["+919800000001"]);
        assert_eq!(
            (a.preview.new, a.preview.matched, a.preview.failed),
            (0, 1, 0)
        );
        assert!(!a.preview.can_commit, "nothing to add");
    }

    #[test]
    fn an_unknown_column_blocks_commit_but_not_valid_rows() {
        let a = run("phone,name,colour\n+919800000001,A,red\n", &[]);
        assert_eq!(a.preview.failures[0].reason, "unknown_column");
        assert!(!a.preview.can_commit && a.preview.can_commit_valid);
    }

    #[test]
    fn a_missing_phone_or_name_column_makes_the_file_unusable() {
        let a = run("name,balance\nA,1\n", &[]);
        assert!(a.preview.unusable);
        assert_eq!(a.preview.failures[0].reason, "missing_column");
        assert!(run("", &[]).preview.unusable);
    }

    #[test]
    fn usernames_are_made_from_the_name_and_the_phone_and_never_repeat() {
        let mut taken = HashSet::from(["asha.rao0001".to_owned()]);
        assert_eq!(
            username_for("Asha Rao", "+919800000001", &mut taken),
            "asha.rao0001-1"
        );
        assert_eq!(
            username_for("Asha Rao", "+919800000001", &mut taken),
            "asha.rao0001-2"
        );
        assert_eq!(
            username_for("आशा", "+919800000042", &mut taken),
            "player0042"
        );
        let long = username_for(
            "Maximilian Alexander Montgomery",
            "+919800000042",
            &mut taken,
        );
        assert!(valid_username(&long) && long.len() <= 32, "{long}");
    }
}

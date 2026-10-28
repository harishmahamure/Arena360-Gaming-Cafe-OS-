//! What the player and plan imports share once a CSV is split into records (Story 5.34): the
//! header (known columns in any order, unknown ones listed, required ones checked) and the list
//! of problems with its limits. Nothing here touches the database.

use arena_proto::{
    AppError, ErrorCode, IMPORT_MAX_BYTES, IMPORT_MAX_FAILURES, IMPORT_MAX_ROWS, ImportFailure,
};

use crate::import_csv::{CsvRecord, parse_csv};
use crate::import_products::Problem;

/// Where each known column sits in the file.
pub(crate) struct Header {
    columns: &'static [&'static str],
    positions: Vec<Option<usize>>,
    /// How many columns the header row has.
    pub width: usize,
}

impl Header {
    /// The trimmed cell of `column` in `rec`; empty when the file has no such column or the row
    /// is short.
    pub fn cell<'a>(&self, rec: &'a CsvRecord, column: &str) -> &'a str {
        self.columns
            .iter()
            .position(|c| *c == column)
            .and_then(|k| self.positions[k])
            .and_then(|i| rec.cells.get(i))
            .map_or("", String::as_str)
    }

    /// Whether `rec` has values beyond the header's columns.
    pub fn overflows(&self, rec: &CsvRecord) -> bool {
        rec.cells.len() > self.width && rec.cells[self.width..].iter().any(|c| !c.is_empty())
    }
}

/// A readable file: its header, its data records and the unknown columns it names.
pub(crate) struct File {
    pub header: Header,
    pub records: Vec<CsvRecord>,
    /// One `unknown_column` failure per column the template does not have.
    pub unknown: Vec<ImportFailure>,
}

/// A file that was read, or why none of it can be imported.
pub(crate) enum Read {
    Unusable(Vec<ImportFailure>),
    File(File),
}

/// A problem with the file as a whole (row 0).
pub(crate) fn whole_file(reason: &str, field: &str, message: impl Into<String>) -> ImportFailure {
    ImportFailure {
        row: 0,
        field: field.to_owned(),
        reason: reason.to_owned(),
        message: message.into(),
    }
}

/// Reads `csv`: splits it, finds `columns` in the header (any order, any case) and checks that
/// every `required` one is there. `Err` only for a file over the size or row limits.
pub(crate) fn read_file(
    csv: &str,
    columns: &'static [&'static str],
    required: &[&str],
) -> Result<Read, AppError> {
    let too_big = |m: &str| AppError::with_field(ErrorCode::ValidationFailed, "csv", m);
    if csv.len() > IMPORT_MAX_BYTES {
        return Err(too_big("the file is larger than 1.5 MB"));
    }
    let records = match parse_csv(csv) {
        Ok(r) => r,
        Err(m) => return Ok(Read::Unusable(vec![whole_file("bad_file", "", m)])),
    };
    let mut records = records.into_iter();
    let Some(header) = records.next() else {
        return Ok(Read::Unusable(vec![whole_file(
            "bad_file",
            "",
            "the file is empty; it needs a header row",
        )]));
    };
    let data: Vec<CsvRecord> = records.collect();
    if data.len() > IMPORT_MAX_ROWS {
        return Err(too_big("the file has more than 10,000 rows"));
    }
    let line = i64::try_from(header.line).unwrap_or(1);
    let mut positions = vec![None; columns.len()];
    let mut unknown = Vec::new();
    for (i, cell) in header.cells.iter().enumerate() {
        let key = cell.to_ascii_lowercase();
        match columns.iter().position(|c| *c == key) {
            Some(k) if positions[k].is_some() => {
                return Ok(Read::Unusable(vec![ImportFailure {
                    row: line,
                    field: key,
                    reason: "bad_file".to_owned(),
                    message: "the column appears twice in the header".to_owned(),
                }]));
            }
            Some(k) => positions[k] = Some(i),
            None => unknown.push(ImportFailure {
                row: line,
                field: cell.clone(),
                reason: "unknown_column".to_owned(),
                message: "this column is not part of the template".to_owned(),
            }),
        }
    }
    let missing: Vec<ImportFailure> = required
        .iter()
        .filter(|c| {
            columns
                .iter()
                .position(|k| k == *c)
                .is_some_and(|k| positions[k].is_none())
        })
        .map(|c| {
            whole_file(
                "missing_column",
                c,
                format!("the header has no \"{c}\" column"),
            )
        })
        .collect();
    if !missing.is_empty() {
        return Ok(Read::Unusable(missing));
    }
    Ok(Read::File(File {
        header: Header {
            columns,
            positions,
            width: header.cells.len(),
        },
        records: data,
        unknown,
    }))
}

/// The problems of a dry-run, ready to show.
pub(crate) struct Problems {
    /// The unknown columns first, then every problem by row; at most [`IMPORT_MAX_FAILURES`].
    pub list: Vec<ImportFailure>,
    pub truncated: bool,
    /// Rows with at least one problem.
    pub failed: i64,
}

/// Orders `bad` (`(line, problem)`) by line behind the file's own `unknown` failures, counts the
/// rows that failed and cuts the list to the limit.
pub(crate) fn problems(unknown: Vec<ImportFailure>, mut bad: Vec<(i64, Problem)>) -> Problems {
    bad.sort_by_key(|(line, _)| *line);
    let mut lines: Vec<i64> = bad.iter().map(|(l, _)| *l).collect();
    lines.dedup();
    let failed = i64::try_from(lines.len()).unwrap_or(i64::MAX);
    let mut list = unknown;
    list.extend(bad.into_iter().map(|(row, p)| ImportFailure {
        row,
        field: p.field,
        reason: p.reason.to_owned(),
        message: p.message,
    }));
    let truncated = list.len() > IMPORT_MAX_FAILURES;
    list.truncate(IMPORT_MAX_FAILURES);
    Problems {
        list,
        truncated,
        failed,
    }
}

/// The line number of a record as the file's own numbering.
pub(crate) fn line_of(rec: &CsvRecord) -> i64 {
    i64::try_from(rec.line).unwrap_or(i64::MAX)
}

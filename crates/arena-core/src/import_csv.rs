//! The CSV reading shared by every import (Story 5.33): records and their line numbers, and
//! exact decimal numbers. Nothing here touches the database.

/// One record of a file: the line it starts on (the first line is 1) and its cells, trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRecord {
    pub line: usize,
    pub cells: Vec<String>,
}

/// Splits `text` into records (RFC 4180: quoted cells may hold commas, quotes written twice and
/// line breaks; CRLF, LF and CR end a record). A leading byte-order mark is dropped, and blank
/// lines, or lines of only empty cells, are skipped. Cells are trimmed.
/// `Err` names the line of a quote that never closes.
pub fn parse_csv(text: &str) -> Result<Vec<CsvRecord>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut line = 1usize;
    let mut start = 1usize;
    let mut chars = text.chars().peekable();
    let finish = |cells: &mut Vec<String>, cell: &mut String, start: usize, out: &mut Vec<_>| {
        cells.push(cell.trim().to_owned());
        cell.clear();
        let record = std::mem::take(cells);
        if record.iter().any(|c| !c.is_empty()) {
            out.push(CsvRecord {
                line: start,
                cells: record,
            });
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    cell.push('\n');
                }
                other => cell.push(other),
            }
            continue;
        }
        match c {
            '"' if cell.trim().is_empty() => {
                cell.clear();
                quoted = true;
                was_quoted = true;
            }
            ',' => {
                cells.push(cell.trim().to_owned());
                cell.clear();
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                finish(&mut cells, &mut cell, start, &mut records);
                line += 1;
                start = line;
                was_quoted = false;
            }
            other => cell.push(other),
        }
    }
    if quoted {
        return Err(format!(
            "a quoted value starting near line {start} is never closed"
        ));
    }
    if !cells.is_empty() || !cell.is_empty() || was_quoted {
        finish(&mut cells, &mut cell, start, &mut records);
    }
    Ok(records)
}

/// Why a decimal could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalError {
    /// Not a plain number (`12`, `12.5`): letters, signs, grouping, a bare point.
    Syntax,
    /// Below zero.
    Negative,
    /// More decimal places than the unit has.
    Places,
    /// Too large to hold.
    Big,
}

/// A non-negative decimal as a whole number of `10^-places` units: `parse_decimal("12.5", 2)`
/// is `1250`. Extra places are accepted only when they are zeros.
pub fn parse_decimal(text: &str, places: u32) -> Result<i64, DecimalError> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('-') {
        return Err(if rest.chars().any(|c| c.is_ascii_digit()) {
            DecimalError::Negative
        } else {
            DecimalError::Syntax
        });
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty())
        || !digits(whole)
        || !digits(fraction)
        || (text.contains('.') && fraction.is_empty())
    {
        return Err(DecimalError::Syntax);
    }
    let places = places as usize;
    let (kept, extra) = if fraction.len() > places {
        fraction.split_at(places)
    } else {
        (fraction, "")
    };
    if extra.bytes().any(|b| b != b'0') {
        return Err(DecimalError::Places);
    }
    let mut value: i64 = 0;
    for b in whole
        .bytes()
        .chain(kept.bytes())
        .chain(std::iter::repeat_n(b'0', places - kept.len()))
    {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(i64::from(b - b'0')))
            .ok_or(DecimalError::Big)?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Vec<String>> {
        parse_csv(text)
            .unwrap()
            .into_iter()
            .map(|r| r.cells)
            .collect()
    }

    #[test]
    fn records_split_on_every_line_ending_and_drop_the_bom() {
        assert_eq!(
            cells("\u{feff}a,b\r\n1,2\n3,4\r5,6"),
            vec![
                vec!["a", "b"],
                vec!["1", "2"],
                vec!["3", "4"],
                vec!["5", "6"]
            ]
        );
    }

    #[test]
    fn quoted_cells_hold_commas_quotes_and_line_breaks() {
        let records =
            parse_csv("name,note\n\"Rock, \"\"paper\"\"\",\"two\nlines\"\nnext,x\n").unwrap();
        assert_eq!(records[1].cells, vec!["Rock, \"paper\"", "two\nlines"]);
        assert_eq!(records[1].line, 2);
        assert_eq!(records[2].line, 4, "a line break inside quotes counts");
    }

    #[test]
    fn blank_lines_and_empty_rows_are_skipped_but_keep_the_line_numbers() {
        let records = parse_csv("a,b\n\n,\nx,y\n").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].line, 4);
        assert!(parse_csv("").unwrap().is_empty());
    }

    #[test]
    fn a_quote_that_never_closes_is_reported() {
        assert!(parse_csv("a,b\n\"oops,1\n").is_err());
    }

    #[test]
    fn a_quoted_empty_cell_is_a_cell() {
        assert_eq!(cells("a,\"\"\n1,2"), vec![vec!["a", ""], vec!["1", "2"]]);
        assert_eq!(cells("x,\"\""), vec![vec!["x", ""]]);
    }

    #[test]
    fn decimals_are_exact() {
        assert_eq!(parse_decimal("12", 2), Ok(1200));
        assert_eq!(parse_decimal("12.5", 2), Ok(1250));
        assert_eq!(parse_decimal("0.05", 2), Ok(5));
        assert_eq!(parse_decimal(".5", 2), Ok(50));
        assert_eq!(parse_decimal("12.500", 2), Ok(1250));
        assert_eq!(parse_decimal("7", 0), Ok(7));
        assert_eq!(parse_decimal("12.505", 2), Err(DecimalError::Places));
        assert_eq!(parse_decimal("1.5", 0), Err(DecimalError::Places));
        assert_eq!(parse_decimal("-1", 2), Err(DecimalError::Negative));
        assert_eq!(parse_decimal("1,000", 2), Err(DecimalError::Syntax));
        assert_eq!(parse_decimal("abc", 2), Err(DecimalError::Syntax));
        assert_eq!(parse_decimal("12.", 2), Err(DecimalError::Syntax));
        assert_eq!(parse_decimal("", 2), Err(DecimalError::Syntax));
        assert_eq!(parse_decimal("-", 2), Err(DecimalError::Syntax));
        assert_eq!(
            parse_decimal("99999999999999999999", 2),
            Err(DecimalError::Big)
        );
    }
}

//! The `kot` renderer (Story 7.5): a pure function from a kitchen ticket and the paper width to
//! ESC/POS bytes for 58 mm (32 columns) and 80 mm (48 columns) paper.
//!
//! The slip is for the cook: the title, the seat in double width and height, the ticket number,
//! the short order id, the branch-local time, then each line as `qty x name` with its notes
//! indented below. It carries no prices and no player name. Text is Font A in PC437, so the
//! multiplication sign prints as a plain `x`. It reuses the receipt layout, so a slip wraps and
//! encodes exactly as a receipt does.

use super::{Layout, PaperWidth, Row, Style, encode, len, receipt_clock, receipt_time, wrap};

/// The notes of a line print this many columns in.
const NOTE_INDENT: usize = 2;
/// How many characters of the order id the slip shows.
const SHORT_ORDER_CHARS: usize = 6;

/// One line of a ticket as the slip prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotLine {
    pub qty: i64,
    pub name: String,
    pub notes: Vec<String>,
}

/// A kitchen ticket as the slip prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotTicket {
    /// The seat label: "PC 07", "TV 3" or "Counter".
    pub seat: String,
    pub number: i64,
    pub order_id: String,
    pub placed_at_ms: i64,
    pub lines: Vec<KotLine>,
}

/// The short form of an order id: the last 6 characters of its ULID, upper-case, as the Hub shows
/// it on the order.
pub fn short_order_id(order_id: &str) -> String {
    let chars: Vec<char> = order_id.chars().collect();
    let from = chars.len().saturating_sub(SHORT_ORDER_CHARS);
    chars[from..].iter().collect::<String>().to_uppercase()
}

/// `qty x name`, wrapped under its own first character.
fn push_line(l: &mut Layout, line: &KotLine) {
    let prefix = format!("{} x ", line.qty);
    let hang = " ".repeat(len(&prefix));
    let width = l.cols.saturating_sub(len(&prefix)).max(1);
    for (i, chunk) in wrap(&line.name, width).iter().enumerate() {
        let lead = if i == 0 {
            prefix.as_str()
        } else {
            hang.as_str()
        };
        l.push(Style::Plain, &format!("{lead}{chunk}"));
    }
    let indent = " ".repeat(NOTE_INDENT);
    for note in &line.notes {
        for chunk in wrap(note, l.cols.saturating_sub(NOTE_INDENT).max(1)) {
            l.push(Style::Plain, &format!("{indent}{chunk}"));
        }
    }
}

fn kot_rows(paper: PaperWidth, tz: &str, ticket: &KotTicket, reprint_at: Option<i64>) -> Vec<Row> {
    let mut l = Layout::new(paper);
    l.centred(Style::Bold, "KOT");
    if let Some(at) = reprint_at {
        l.centred(Style::Bold, &format!("REPRINT {}", receipt_clock(at, tz)));
    }
    l.centred(Style::Name, &ticket.seat);
    l.centred(Style::Bold, &format!("K-{}", ticket.number));
    l.push(
        Style::Plain,
        &format!("Order {}", short_order_id(&ticket.order_id)),
    );
    l.push(Style::Plain, &receipt_time(ticket.placed_at_ms, tz));
    l.rule();
    for line in &ticket.lines {
        push_line(&mut l, line);
    }
    l.rule();
    l.rows
}

/// A ticket as ESC/POS bytes. `reprint_at` is the time of a reprint, which marks the slip
/// "REPRINT" with that time; the ticket's own time stays the time it was placed.
pub fn render_kot(
    paper: PaperWidth,
    tz: &str,
    ticket: &KotTicket,
    reprint_at: Option<i64>,
) -> Vec<u8> {
    encode(&kot_rows(paper, tz, ticket, reprint_at))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::dump;
    use super::*;

    const PLACED: i64 = 1_793_176_500_000;
    const REPRINTED: i64 = PLACED + 17 * 60_000;
    const ORDER: &str = "01K8Z3Y7QXW2R5T9V1B4N9MZD4Q";
    const TZ: &str = "Asia/Kolkata";

    fn line(qty: i64, name: &str, notes: &[&str]) -> KotLine {
        KotLine {
            qty,
            name: name.into(),
            notes: notes.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    fn ticket(seat: &str, number: i64, lines: Vec<KotLine>) -> KotTicket {
        KotTicket {
            seat: seat.into(),
            number,
            order_id: ORDER.into(),
            placed_at_ms: PLACED,
            lines,
        }
    }

    fn fixtures() -> Vec<(&'static str, KotTicket)> {
        vec![
            (
                "one_line",
                ticket("PC 07", 2043, vec![line(1, "Masala chai", &[])]),
            ),
            (
                "many_lines",
                ticket(
                    "PC 12",
                    17,
                    (1..=9)
                        .map(|i| line(i, &format!("Item number {i}"), &[]))
                        .collect(),
                ),
            ),
            (
                "long_names",
                ticket(
                    "PC 03",
                    88,
                    vec![
                        line(
                            2,
                            "Paneer butter masala with garlic naan and a side of raita",
                            &[],
                        ),
                        line(
                            12,
                            "Supercalifragilisticexpialidocious-biryani-special",
                            &[],
                        ),
                    ],
                ),
            ),
            (
                "several_notes",
                ticket(
                    "PC 21",
                    5,
                    vec![
                        line(
                            1,
                            "Cold coffee",
                            &[
                                "No sugar",
                                "Extra ice",
                                "Served in a tall glass with a straw",
                            ],
                        ),
                        line(3, "Veg sandwich", &["Toasted", "Cut in half"]),
                    ],
                ),
            ),
            (
                "counter",
                ticket("Counter", 2044, vec![line(2, "Samosa", &["Extra chutney"])]),
            ),
            (
                "tv",
                ticket("TV 3", 9, vec![line(1, "Popcorn", &["Salted"])]),
            ),
        ]
    }

    fn golden_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/printer/golden")
            .join(format!("{name}.escpos.txt"))
    }

    /// Compares `bytes` with the stored golden file; `UPDATE_GOLDEN=1` rewrites it.
    fn check_golden(name: &str, bytes: &[u8]) {
        let path = golden_path(name);
        let got = dump(bytes);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &got).unwrap();
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_GOLDEN=1", path.display()));
        assert_eq!(
            got,
            want.replace("\r\n", "\n"),
            "{name} differs from its golden file"
        );
    }

    #[test]
    fn every_fixture_matches_its_golden_file_and_fits_the_paper() {
        for (paper, tag) in [(PaperWidth::Mm80, "80"), (PaperWidth::Mm58, "58")] {
            for (name, t) in fixtures() {
                let rows = kot_rows(paper, TZ, &t, None);
                for row in &rows {
                    assert!(
                        len(&row.text) * row.style.factor() <= paper.columns(),
                        "{name}/{tag}: {:?} is wider than {} columns",
                        row.text,
                        paper.columns()
                    );
                    assert!(row.text.is_ascii(), "{:?} is not PC437-safe", row.text);
                }
                let has = |want: &str| rows.iter().any(|r| r.text.trim() == want);
                assert!(has(&t.seat), "{name}/{tag}: seat");
                assert!(has(&format!("K-{}", t.number)), "{name}/{tag}: number");
                assert!(has("Order 9MZD4Q"), "{name}/{tag}: order id");
                check_golden(
                    &format!("kot_{name}_{tag}"),
                    &render_kot(paper, TZ, &t, None),
                );
            }
        }
    }

    #[test]
    fn a_reprint_is_marked_with_the_reprint_time_and_keeps_the_placed_time() {
        let t = ticket("PC 07", 2043, vec![line(1, "Masala chai", &[])]);
        check_golden(
            "kot_reprint_80",
            &render_kot(PaperWidth::Mm80, TZ, &t, Some(REPRINTED)),
        );
        let rows = kot_rows(PaperWidth::Mm58, TZ, &t, Some(REPRINTED));
        assert!(rows.iter().any(|r| r.text.trim() == "REPRINT 14:22"));
        assert!(rows.iter().any(|r| r.text == "28 Oct 2026 14:05"));
    }

    #[test]
    fn a_slip_has_no_price_and_no_player() {
        let t = ticket("PC 07", 1, vec![line(1, "Tea", &["Hot"])]);
        let bytes = render_kot(PaperWidth::Mm80, TZ, &t, None);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("Rs") && !text.contains("Total") && !text.contains("Customer"));
    }

    #[test]
    fn the_short_order_id_is_the_last_six_characters() {
        assert_eq!(short_order_id(ORDER), "9MZD4Q");
        assert_eq!(short_order_id("abc"), "ABC");
    }
}

//! An export opens with the stock tools (Story 9.6): the body of a real `.a360` file goes through
//! `zstd -d` and `sqlite3`, every table the reference lists is there, and its row count matches
//! the source database.
//!
//! The tools are not on every developer PC, so without them the test is skipped, unless
//! `ARENA_REQUIRE_STOCK_TOOLS` is set (CI sets it on Linux, where both are installed).

use std::path::Path;
use std::process::Command;

use arena_proto::ExportHeader;
use arena_store::{Store, write_export};
use rusqlite::Connection;

const REFERENCE: &str = include_str!("../../../docs/export-format/reference.md");

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The tables the reference documents: its `### \`name\`` headings under "Tables".
fn documented_tables() -> Vec<String> {
    let (_, tables) = REFERENCE
        .split_once("\n## Tables\n")
        .expect("Tables section");
    tables
        .lines()
        .filter_map(|l| l.strip_prefix("### `")?.strip_suffix('`'))
        .map(str::to_owned)
        .collect()
}

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().expect("run a stock tool");
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn count_query(tables: &[String]) -> String {
    tables
        .iter()
        .map(|t| format!("SELECT '{t}', count(*) FROM \"{t}\""))
        .collect::<Vec<_>>()
        .join(" UNION ALL ")
}

fn counts_with_stock_sqlite(db: &Path, tables: &[String]) -> Vec<(String, i64)> {
    let out = run(Command::new("sqlite3")
        .arg("-separator")
        .arg("|")
        .arg(db)
        .arg(count_query(tables)));
    out.lines()
        .map(|l| {
            let (t, n) = l.split_once('|').expect("table|count");
            (t.to_owned(), n.parse().expect("a count"))
        })
        .collect()
}

#[test]
fn an_export_opens_with_stock_zstd_and_sqlite3() {
    if !(has_tool("zstd") && has_tool("sqlite3")) {
        assert!(
            std::env::var("ARENA_REQUIRE_STOCK_TOOLS").is_ok_and(|v| v.is_empty())
                || std::env::var_os("ARENA_REQUIRE_STOCK_TOOLS").is_none(),
            "ARENA_REQUIRE_STOCK_TOOLS is set but zstd or sqlite3 is not installed"
        );
        eprintln!("skipped: stock zstd and sqlite3 are not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("hub.db");
    drop(Store::open(&source, 0).unwrap());
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let file = write_export(&source, &out_dir, "KOR-test.a360", 1_793_000_000_000).unwrap();

    // The documented layout: magic, header length, header, then the zstd body.
    let bytes = std::fs::read(&file.path).unwrap();
    let (header, body_at) = ExportHeader::decode(&bytes).unwrap();
    assert_eq!(&bytes[..8], b"A360EXP1");
    let header_len = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    assert_eq!(body_at, 12 + header_len, "the body follows the header");
    let body = dir.path().join("body.zst");
    std::fs::write(&body, &bytes[body_at..]).unwrap();
    assert_eq!(header.schema_version, arena_store::LATEST_SCHEMA_VERSION);

    let opened = dir.path().join("branch.db");
    run(Command::new("zstd")
        .arg("-d")
        .arg("-q")
        .arg(&body)
        .arg("-o")
        .arg(&opened));

    let documented = documented_tables();
    assert!(documented.len() > 50, "the reference lists the tables");
    let in_export: Vec<String> = run(Command::new("sqlite3").arg(&opened).arg(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND substr(name, 1, 7) != 'sqlite_' ORDER BY name",
    ))
    .lines()
    .map(str::to_owned)
    .collect();
    for table in &documented {
        assert!(
            in_export.contains(table),
            "{table} is documented but not in the export"
        );
    }
    for table in &in_export {
        assert!(
            documented.contains(table),
            "{table} is in the export but not documented"
        );
    }

    // Row counts: the export's, read by the stock tool, against the source's.
    let source_conn = Connection::open(&source).unwrap();
    let exported = counts_with_stock_sqlite(&opened, &documented);
    let mut any_rows = false;
    for (table, count) in exported {
        let expected: i64 = if table == "licence_cache" {
            0 // cleared by the export
        } else {
            source_conn
                .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        assert_eq!(count, expected, "row count of {table}");
        any_rows |= count > 0;
    }
    assert!(any_rows, "the sample database holds rows");
}

/// The same checks with the zstd library and rusqlite, so they run on every PC: the documented
/// tables are exactly the exported ones and the counts match the source.
#[test]
fn the_documented_tables_are_the_exported_ones_with_matching_counts() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("hub.db");
    drop(Store::open(&source, 0).unwrap());
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let file = write_export(&source, &out_dir, "KOR-test.a360", 1_793_000_000_000).unwrap();
    let bytes = std::fs::read(&file.path).unwrap();
    let (_, body_at) = ExportHeader::decode(&bytes).unwrap();
    let opened = dir.path().join("branch.db");
    std::fs::write(&opened, zstd::decode_all(&bytes[body_at..]).unwrap()).unwrap();

    let exported = Connection::open(&opened).unwrap();
    let source = Connection::open(&source).unwrap();
    let documented = documented_tables();
    let mut listed: Vec<String> = exported
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND substr(name, 1, 7) != 'sqlite_'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    listed.sort();
    let mut documented_sorted = documented.clone();
    documented_sorted.sort();
    assert_eq!(listed, documented_sorted);
    let mut any_rows = false;
    for table in &documented {
        let count = |c: &Connection| -> i64 {
            c.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let expected = if table == "licence_cache" {
            0
        } else {
            count(&source)
        };
        assert_eq!(count(&exported), expected, "row count of {table}");
        any_rows |= expected > 0;
    }
    assert!(any_rows);
}

//! The export format reference (Story 9.6) is rendered from the migrations and must match the
//! committed `docs/export-format/reference.md`; every table and column needs a meaning.
//!
//! `ARENA_UPDATE_DOCS=1 cargo test -p arena-store --test export_reference` rewrites the file.

use arena_store::{
    LATEST_SCHEMA_VERSION, Meanings, Store, reference_gaps, render_export_reference,
};
use rusqlite::Connection;

const MEANINGS: &str = include_str!("../export-reference/meanings.txt");
const REFERENCE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/export-format/reference.md"
);

fn migrated() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub.db");
    drop(Store::open(&path, 0).unwrap());
    let conn = Connection::open(&path).unwrap();
    (dir, conn)
}

#[test]
fn every_table_and_column_has_a_meaning() {
    let (_dir, conn) = migrated();
    let gaps = reference_gaps(&conn, &Meanings::parse(MEANINGS)).unwrap();
    assert!(
        gaps.is_empty(),
        "add a meaning to crates/arena-store/export-reference/meanings.txt for:\n{}",
        gaps.join("\n")
    );
}

#[test]
fn the_committed_reference_matches_the_migrations() {
    let (_dir, conn) = migrated();
    let rendered =
        render_export_reference(&conn, &Meanings::parse(MEANINGS), LATEST_SCHEMA_VERSION).unwrap();
    if std::env::var_os("ARENA_UPDATE_DOCS").is_some() {
        std::fs::write(REFERENCE_PATH, &rendered).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(REFERENCE_PATH)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == rendered,
        "docs/export-format/reference.md is stale against the migrations. Run \
         `ARENA_UPDATE_DOCS=1 cargo test -p arena-store --test export_reference` and commit it."
    );
}

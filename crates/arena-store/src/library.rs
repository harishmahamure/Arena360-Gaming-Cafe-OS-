//! The branch game and app library (Story 6.3): the `library_entries` change-log table. Only
//! `apply()` writes it, through these helpers.

use arena_proto::{AppError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// One `library_entries` row. `tags` is decoded from its JSON column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub launcher: String,
    pub executable_path: String,
    pub launch_uri: String,
    pub launch_args: String,
    pub category: String,
    pub tags: Vec<String>,
    pub age_rating: String,
    pub enabled: bool,
    pub catalog_id: Option<String>,
    pub rev: i64,
    pub deleted_at: Option<i64>,
}

/// The fields `insert_library_entry` and `update_library_entry` write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryWrite<'a> {
    pub kind: &'a str,
    pub name: &'a str,
    pub launcher: &'a str,
    pub executable_path: &'a str,
    pub launch_uri: &'a str,
    pub launch_args: &'a str,
    pub category: &'a str,
    pub tags: &'a [String],
    pub age_rating: &'a str,
    pub enabled: bool,
    pub catalog_id: Option<&'a str>,
}

const COLUMNS: &str = "id, kind, name, launcher, executable_path, launch_uri, launch_args, \
                       category, tags, age_rating, enabled, catalog_id, rev, deleted_at";

fn from(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryRow> {
    let tags: String = r.get(8)?;
    Ok(LibraryRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        launcher: r.get(3)?,
        executable_path: r.get(4)?,
        launch_uri: r.get(5)?,
        launch_args: r.get(6)?,
        category: r.get(7)?,
        tags: arena_proto::tags_from_json(&tags),
        age_rating: r.get(9)?,
        enabled: r.get(10)?,
        catalog_id: r.get(11)?,
        rev: r.get(12)?,
        deleted_at: r.get(13)?,
    })
}

fn tags_json(tags: &[String]) -> String {
    arena_proto::tags_to_json(tags)
}

/// The entry with this id, deleted or not.
pub fn find_library_entry(conn: &Connection, id: &str) -> Result<Option<LibraryRow>, AppError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM library_entries WHERE id = ?1"),
        [id],
        from,
    )
    .optional()
    .map_err(db_error)
}

/// Every live (not deleted) entry, enabled or not, ordered by name then id.
pub fn list_library_entries(conn: &Connection) -> Result<Vec<LibraryRow>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM library_entries WHERE deleted_at IS NULL
             ORDER BY name COLLATE NOCASE, id"
        ))
        .map_err(db_error)?;
    let rows = stmt.query_map([], from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Adds an entry (`rev` and the timestamps are set by the stamp step).
pub fn insert_library_entry(
    conn: &Connection,
    id: &str,
    w: &LibraryWrite<'_>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO library_entries (id, kind, name, launcher, executable_path, launch_uri,
             launch_args, category, tags, age_rating, enabled, catalog_id, rev, deleted_at,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, NULL, 0, 0)",
        params![
            id,
            w.kind,
            w.name,
            w.launcher,
            w.executable_path,
            w.launch_uri,
            w.launch_args,
            w.category,
            tags_json(w.tags),
            w.age_rating,
            w.enabled,
            w.catalog_id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

fn expect_one(n: usize, id: &str) -> Result<(), AppError> {
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no library_entries row {id}"),
        ))
    }
}

/// Rewrites an entry's fields (the stamp step then bumps `rev`).
pub fn update_library_entry(
    conn: &Connection,
    id: &str,
    w: &LibraryWrite<'_>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE library_entries SET kind = ?2, name = ?3, launcher = ?4, executable_path = ?5,
                 launch_uri = ?6, launch_args = ?7, category = ?8, tags = ?9, age_rating = ?10,
                 enabled = ?11, catalog_id = ?12
             WHERE id = ?1",
            params![
                id,
                w.kind,
                w.name,
                w.launcher,
                w.executable_path,
                w.launch_uri,
                w.launch_args,
                w.category,
                tags_json(w.tags),
                w.age_rating,
                w.enabled,
                w.catalog_id
            ],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

/// Soft-deletes an entry.
pub fn set_library_entry_deleted(conn: &Connection, id: &str, at_ms: i64) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE library_entries SET deleted_at = ?2 WHERE id = ?1",
            params![id, at_ms],
        )
        .map_err(db_error)?;
    expect_one(n, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/0062_library_entries.sql"))
            .unwrap();
        conn
    }

    fn write<'a>(name: &'a str, path: &'a str, tags: &'a [String]) -> LibraryWrite<'a> {
        LibraryWrite {
            kind: "game",
            name,
            launcher: "steam",
            executable_path: path,
            launch_uri: "",
            launch_args: "",
            category: "",
            tags,
            age_rating: "",
            enabled: true,
            catalog_id: None,
        }
    }

    #[test]
    fn rows_round_trip_with_their_tags_and_list_by_name() {
        let c = conn();
        let tags = vec!["fps".to_owned(), "co-op".to_owned()];
        insert_library_entry(&c, "B", &write("beta", r"C:\b.exe", &tags)).unwrap();
        insert_library_entry(&c, "A", &write("Alpha", r"C:\a.exe", &[])).unwrap();
        let rows = list_library_entries(&c).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["A", "B"],
            "ordered by name, case-insensitively"
        );
        assert_eq!(rows[1].tags, tags);
        update_library_entry(&c, "B", &write("beta", r"C:\b2.exe", &[])).unwrap();
        assert_eq!(
            find_library_entry(&c, "B")
                .unwrap()
                .unwrap()
                .executable_path,
            r"C:\b2.exe"
        );
        set_library_entry_deleted(&c, "B", 5).unwrap();
        assert_eq!(list_library_entries(&c).unwrap().len(), 1);
        assert_eq!(
            find_library_entry(&c, "B").unwrap().unwrap().deleted_at,
            Some(5)
        );
        assert!(update_library_entry(&c, "Z", &write("z", r"C:\z.exe", &[])).is_err());
    }

    #[test]
    fn the_index_refuses_a_live_duplicate_but_not_a_deleted_one_or_an_empty_path() {
        let c = conn();
        insert_library_entry(&c, "A", &write("a", r"C:\Games\X.exe", &[])).unwrap();
        assert!(insert_library_entry(&c, "B", &write("b", r"c:\games\x.EXE", &[])).is_err());
        let mut other = write("b", r"C:\Games\X.exe", &[]);
        other.launcher = "epic";
        insert_library_entry(&c, "C", &other).unwrap();
        set_library_entry_deleted(&c, "A", 1).unwrap();
        insert_library_entry(&c, "D", &write("d", r"C:\Games\X.exe", &[])).unwrap();
        let mut uri = write("u1", "", &[]);
        uri.launch_uri = "steam://rungameid/1";
        insert_library_entry(&c, "U1", &uri).unwrap();
        insert_library_entry(&c, "U2", &uri).unwrap();
        assert!(
            insert_library_entry(&c, "N", &write("n", "", &[])).is_err(),
            "an entry needs a path or a URI"
        );
    }
}

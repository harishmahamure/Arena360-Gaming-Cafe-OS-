//! The public reference for the `.a360` export file (Story 9.6): its layout, how to open it
//! without Arena 360, and every table and column with its type and meaning.
//!
//! The reference is rendered from a database the shipped migrations built, so it cannot drift
//! from them: a test compares the committed `docs/export-format/reference.md` with
//! [`render_export_reference`] and fails when they differ, and [`reference_gaps`] names every
//! table or column that has no meaning, so a migration cannot ship undocumented.
//!
//! Meanings come from a small text file, one entry per line:
//!
//! ```text
//! table: what the table holds
//! table.column: what the column means
//! *.column: what a column of this name means in every table
//! ```
//!
//! A text that starts with `[ciphertext]` marks the column as ciphertext. A column with no entry
//! falls back to what its name or a foreign key says (`*_minor`, `*_at`, `*_bp`, `*_enc`,
//! `REFERENCES`), and has none otherwise.

use std::collections::BTreeMap;

use arena_proto::{AppError, EXPORT_EXTENSION, EXPORT_FORMAT_VERSION, EXPORT_MAGIC};
use rusqlite::Connection;

use crate::db_error;

/// What a ciphertext column is marked with in the reference.
const CIPHERTEXT_TAG: &str = "[ciphertext]";

/// The parsed meanings file.
#[derive(Debug, Default)]
pub struct Meanings {
    tables: BTreeMap<String, String>,
    columns: BTreeMap<(String, String), String>,
    any: BTreeMap<String, String>,
}

impl Meanings {
    /// Parses the meanings file. Blank lines and lines starting with `#` are skipped; a line
    /// without `: ` is ignored.
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, meaning)) = line.split_once(": ") else {
                continue;
            };
            let meaning = meaning.trim().to_owned();
            match key.split_once('.') {
                Some(("*", column)) => {
                    out.any.insert(column.to_owned(), meaning);
                }
                Some((table, column)) => {
                    out.columns
                        .insert((table.to_owned(), column.to_owned()), meaning);
                }
                None => {
                    out.tables.insert(key.to_owned(), meaning);
                }
            }
        }
        out
    }
}

/// One column as SQLite describes it.
struct Column {
    name: String,
    sql_type: String,
    not_null: bool,
    primary_key: bool,
    references: Option<String>,
}

struct Table {
    name: String,
    columns: Vec<Column>,
}

fn load_tables(conn: &Connection) -> Result<Vec<Table>, AppError> {
    let names: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
                 ORDER BY name",
            )
            .map_err(db_error)?;
        stmt.query_map([], |r| r.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?
    };
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let mut refs: BTreeMap<String, String> = BTreeMap::new();
        {
            let mut stmt = conn
                .prepare(&format!("PRAGMA foreign_key_list(\"{name}\")"))
                .map_err(db_error)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>("from")?,
                        r.get::<_, String>("table")?,
                        r.get::<_, Option<String>>("to")?,
                    ))
                })
                .map_err(db_error)?;
            for row in rows {
                let (from, table, to) = row.map_err(db_error)?;
                refs.insert(
                    from,
                    match to {
                        Some(to) => format!("{table}.{to}"),
                        None => table,
                    },
                );
            }
        }
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{name}\")"))
            .map_err(db_error)?;
        let columns = stmt
            .query_map([], |r| {
                let column: String = r.get("name")?;
                Ok(Column {
                    references: refs.get(&column).cloned(),
                    name: column,
                    sql_type: r.get::<_, String>("type")?.to_lowercase(),
                    not_null: r.get::<_, i64>("notnull")? != 0,
                    primary_key: r.get::<_, i64>("pk")? != 0,
                })
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        tables.push(Table { name, columns });
    }
    Ok(tables)
}

/// What the text for `column` of `table` says, or `None` when nothing does.
fn meaning_of(m: &Meanings, table: &str, column: &Column) -> Option<String> {
    let name = column.name.as_str();
    if let Some(text) = m.columns.get(&(table.to_owned(), name.to_owned())) {
        return Some(text.clone());
    }
    if let Some(text) = m.any.get(name) {
        return Some(text.clone());
    }
    if name.ends_with("_enc") {
        return Some(format!(
            "{CIPHERTEXT_TAG} The player's data, encrypted with their own key; unreadable once \
             the player is erased."
        ));
    }
    if name.ends_with("_minor") {
        return Some("An amount in minor currency units (paise for INR).".to_owned());
    }
    if name.ends_with("_bp") {
        return Some("A rate in basis points (100 = 1%).".to_owned());
    }
    if name.ends_with("_at") || name.ends_with("_ms") {
        return Some("A time as milliseconds since 1970-01-01 UTC.".to_owned());
    }
    column
        .references
        .as_ref()
        .map(|target| format!("The row of `{target}` this row belongs to."))
}

/// A table or column of the schema that has no meaning, as `table` or `table.column`.
pub fn reference_gaps(conn: &Connection, meanings: &Meanings) -> Result<Vec<String>, AppError> {
    let mut gaps = Vec::new();
    for table in load_tables(conn)? {
        if !meanings.tables.contains_key(&table.name) {
            gaps.push(table.name.clone());
        }
        for column in &table.columns {
            if meaning_of(meanings, &table.name, column).is_none() {
                gaps.push(format!("{}.{}", table.name, column.name));
            }
        }
    }
    Ok(gaps)
}

fn type_label(column: &Column) -> String {
    if column.sql_type.is_empty() {
        "any".to_owned()
    } else {
        column.sql_type.clone()
    }
}

/// Escapes `|` and line breaks for a Markdown table cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

/// The reference as Markdown, rendered from the migrated database `conn`. A table or column with
/// no meaning is rendered as `—` (see [`reference_gaps`]).
pub fn render_export_reference(
    conn: &Connection,
    meanings: &Meanings,
    schema_version: u32,
) -> Result<String, AppError> {
    let tables = load_tables(conn)?;
    let magic = String::from_utf8_lossy(EXPORT_MAGIC);
    let mut out = String::new();
    out.push_str(&format!(
        "# Arena 360 export file format (version {EXPORT_FORMAT_VERSION})\n\n\
         <!-- Generated by `render_export_reference` in arena-store from the database migrations. \
         Do not edit: run `ARENA_UPDATE_DOCS=1 cargo test -p arena-store --test export_reference`. -->\n\n\
         An Arena 360 export (`.{EXPORT_EXTENSION}`) is a copy of one branch's whole database. You can open it with \
         free tools, without Arena 360. This page is the reference for the file and every table in it. It \
         describes schema version {schema_version}, with {} tables.\n\n",
        tables.len()
    ));

    out.push_str(&format!(
        "## The file\n\n\
         | Part | Size | Meaning |\n|---|---|---|\n\
         | Magic | 8 bytes | The text `{magic}`. Every export starts with it. |\n\
         | Header length | 4 bytes | A big-endian unsigned number: how many bytes the header takes. |\n\
         | Header | that many bytes | JSON, described below. |\n\
         | Body | the rest | The database, compressed with zstd. |\n\n\
         ### Header fields\n\n\
         | Field | Type | Meaning |\n|---|---|---|\n\
         | `format_version` | number | The layout of this file. It is {EXPORT_FORMAT_VERSION} for everything described here. |\n\
         | `branch_id` | text | The branch the export belongs to. |\n\
         | `branch_code` | text | The branch's short code, the start of the file name. |\n\
         | `epoch` | text | The epoch of the last change in the copy; empty when the copy holds none. |\n\
         | `last_seq` | number | The sequence number of that last change. Together with `epoch` it says what point in time the file stands for. |\n\
         | `schema_version` | number | The newest database migration the copy has. |\n\
         | `exported_at` | number | When the export was made, in milliseconds since 1970-01-01 UTC. |\n\
         | `sha256` | text | The SHA-256 of the body exactly as stored, in lower-case hex. It lets you check the file without decompressing it. |\n\n"
    ));

    out.push_str(
        "## Opening an export without Arena 360\n\n\
         1. Read the first 12 bytes. Bytes 8 to 11 are the header length `N`.\n\
         2. The header is the next `N` bytes. The body starts right after it.\n\
         3. Save the body to a file and decompress it with the stock `zstd` tool: `zstd -d body.zst -o branch.db`.\n\
         4. Open `branch.db` with any SQLite tool, for example `sqlite3 branch.db .tables`.\n\n\
         On Linux or macOS, `tail -c +<N+13> file.a360 > body.zst` cuts the body out. The reports exported \
         beside a database in an \"Export everything\" folder are plain CSV files that open in any spreadsheet.\n\n",
    );

    out.push_str(
        "## Conventions\n\n\
         - **Money** is stored in minor units: whole paise for INR, never a decimal. A column ending in `_minor` \
         holds one. Divide by 100 for rupees (the branch's currency exponent says how many places). The CSV reports \
         show major units with the currency code.\n\
         - **Times** are whole milliseconds since 1970-01-01 UTC. Business days (`YYYY-MM-DD`) are in the branch's \
         time zone, which the `branch` and settings tables name.\n\
         - **Rates** (tax, shares) are basis points: 100 is 1%.\n\
         - **Yes/no** columns hold 0 or 1.\n\
         - **Ids** are text (ULIDs) unless a column says otherwise.\n\
         - **Ciphertext**: a player's personal data (name, phone, email, date of birth, notes) is stored encrypted, \
         with a key of its own per player. Those columns are marked *ciphertext* below. The keys are wrapped by a \
         branch key that is not in the file, so the file alone cannot read them, and an erased player's data can \
         never be read again.\n\
         - `changes` is the gap-free log of every change. `rev` columns count how often a row changed.\n\n\
         ## What an export leaves out or clears\n\n\
         - The licence cache is emptied.\n\
         - Device tokens (`devices.token_hash`) and kitchen display tokens (`kitchen_displays.token_hash`) are emptied.\n\
         - The reference cache (game catalogue and media) and the keystore secrets are not in the database, so they are not in the file.\n\n",
    );

    out.push_str("## Tables\n\n");
    out.push_str(&format!("{} tables, in name order.\n\n", tables.len()));
    for table in &tables {
        out.push_str(&format!("### `{}`\n\n", table.name));
        let about = meanings.tables.get(&table.name).map_or("—", String::as_str);
        out.push_str(&format!("{about}\n\n"));
        out.push_str("| Column | Type | Key | Meaning |\n|---|---|---|---|\n");
        for column in &table.columns {
            let meaning = meaning_of(meanings, &table.name, column);
            let (tag, text) = match &meaning {
                Some(text) => match text.strip_prefix(CIPHERTEXT_TAG) {
                    Some(rest) => ("*ciphertext* ", rest.trim().to_owned()),
                    None => ("", text.clone()),
                },
                None => ("", "—".to_owned()),
            };
            let mut key = String::new();
            if column.primary_key {
                key.push_str("primary");
            }
            if let Some(target) = &column.references {
                if !key.is_empty() {
                    key.push_str(", ");
                }
                key.push_str(&format!("→ `{target}`"));
            }
            let null = if column.not_null || column.primary_key {
                ""
            } else {
                ", may be empty"
            };
            out.push_str(&format!(
                "| `{}` | {}{null} | {} | {}{} |\n",
                column.name,
                type_label(column),
                cell(&key),
                tag,
                cell(&text)
            ));
        }
        out.push('\n');
    }
    Ok(out)
}

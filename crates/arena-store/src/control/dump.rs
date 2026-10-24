//! `gz_control` dump and restore: a self-contained SQL text that both backends read and write.
//!
//! ```text
//! -- arena-control-dump v1
//! -- manifest: {"schema_version":1,"taken_at":<unix ms>,"tables":{"<name>":<rows>,…}}
//! -- section: schema
//! CREATE TABLE …;                 (tables, in creation order)
//! -- section: data
//! INSERT INTO "t" ("a","b") VALUES (…);   (one statement per line)
//! -- section: sequences
//! DELETE FROM sqlite_sequence WHERE name = 't'; INSERT INTO sqlite_sequence …;   (per line)
//! -- section: post-data
//! CREATE INDEX …; CREATE VIEW …; CREATE TRIGGER …;   (creation order)
//! -- end
//! ```
//!
//! Indexes, views and triggers come after the data, so a trigger never fires on restored rows.
//! Objects named `sqlite_%`, `libsql_%` or `_litestream%` (and anything on such a table) are
//! internal and skipped. `schema_migrations` is an ordinary table here, so a restored database
//! is already at the dump's version and needs no migration.
//!
//! Every INSERT fits on one line: a TEXT value containing a line break or NUL is written as
//! `CAST(X'…' AS TEXT)`, which keeps its exact UTF-8 bytes. Rows are read 1,000 at a time with
//! keyset paging by `rowid` (by the primary key, in key order, for `WITHOUT ROWID` tables), so
//! no table is loaded at once. The `sequences` section carries `sqlite_sequence` (AUTOINCREMENT
//! high-water marks) and is applied after the data.
//!
//! On the local backend the whole dump is one read transaction, a snapshot. On the remote
//! backend (Turso) it is not: Turso times out an interactive transaction after a few seconds,
//! so each page is its own read. A write during a remote dump may therefore land in one table's
//! rows and not another's (cross-table drift, e.g. a child row without its parent). The
//! manifest always counts exactly the rows the dump holds, and a restore runs
//! `foreign_key_check` after the data, so such drift fails the restore loudly rather than
//! silently. The nightly run is at 02:30 IST, when writes are rare.
//!
//! A restore turns `foreign_keys` off on the target for the duration (rows arrive table by
//! table), then checks every reference once with `PRAGMA foreign_key_check`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{
    Backend, ControlDb, ControlError, HAS_MIGRATIONS_TABLE, LATEST_CONTROL_VERSION, MAX_VERSION,
    Reader, SqlValue, plan,
};

/// The first line of every dump.
pub const DUMP_HEADER: &str = "-- arena-control-dump v1";
/// Rows read per query while dumping a table.
pub const DUMP_PAGE_ROWS: i64 = 1000;

const MANIFEST_PREFIX: &str = "-- manifest: ";
const SECTION_SCHEMA: &str = "-- section: schema";
const SECTION_DATA: &str = "-- section: data";
const SECTION_SEQUENCES: &str = "-- section: sequences";
const SECTION_POST: &str = "-- section: post-data";
/// Every line of the sequences section starts with this (then the quoted table name).
const SEQUENCE_LINE_PREFIX: &str = "DELETE FROM sqlite_sequence WHERE name = ";
const END: &str = "-- end";
/// A restore applies the data section in transactions of at most this many statements …
const BATCH_STATEMENTS: usize = 500;
/// … or about this many bytes.
const BATCH_BYTES: usize = 512 * 1024;

/// The JSON manifest on a dump's second line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DumpManifest {
    /// The database's `schema_migrations` version when it was dumped.
    pub schema_version: u32,
    /// When the dump was taken (Unix ms).
    pub taken_at: i64,
    /// Every dumped table and the number of rows the dump holds for it.
    pub tables: BTreeMap<String, u64>,
}

/// What a successful [`ControlDb::restore`] verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    /// The restored database's schema version (equal to the manifest's).
    pub schema_version: u32,
    /// Every table and its row count, each equal to the manifest's.
    pub tables: BTreeMap<String, u64>,
}

/// `<col> NOT LIKE …` for every internal prefix.
fn not_internal(col: &str) -> String {
    ["sqlite\\_%", "libsql\\_%", "\\_litestream%"]
        .iter()
        .map(|p| format!("{col} NOT LIKE '{p}' ESCAPE '\\'"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `"name"`, with embedded quotes doubled.
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn hex(bytes: &[u8], out: &mut String) {
    for b in bytes {
        let _ = write!(out, "{b:02X}");
    }
}

/// Appends `v` as an SQL literal that reads back as the same value and storage class.
fn literal(v: &SqlValue, out: &mut String) {
    match v {
        SqlValue::Null => out.push_str("NULL"),
        // `-9223372036854775808` would parse as a REAL in some contexts.
        SqlValue::Integer(i64::MIN) => out.push_str("(-9223372036854775807-1)"),
        SqlValue::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        SqlValue::Real(r) if r.is_nan() => out.push_str("NULL"),
        SqlValue::Real(r) if r.is_infinite() => {
            out.push_str(if *r > 0.0 { "9e999" } else { "-9e999" });
        }
        // `{:?}` is the shortest text that round-trips, and always has a `.` or an exponent,
        // so SQLite reads it back as REAL.
        SqlValue::Real(r) => {
            let _ = write!(out, "{r:?}");
        }
        SqlValue::Text(t) if t.contains(['\n', '\r', '\0']) => {
            out.push_str("CAST(X'");
            hex(t.as_bytes(), out);
            out.push_str("' AS TEXT)");
        }
        SqlValue::Text(t) => {
            out.push('\'');
            out.push_str(&t.replace('\'', "''"));
            out.push('\'');
        }
        SqlValue::Blob(b) => {
            out.push_str("X'");
            hex(b, out);
            out.push('\'');
        }
    }
}

fn text(v: &SqlValue, what: &str) -> Result<String, ControlError> {
    match v {
        SqlValue::Text(t) => Ok(t.clone()),
        other => Err(ControlError::Db(format!(
            "{what}: expected text, got {other:?}"
        ))),
    }
}

fn integer(v: &SqlValue, what: &str) -> Result<i64, ControlError> {
    match v {
        SqlValue::Integer(i) => Ok(*i),
        other => Err(ControlError::Db(format!(
            "{what}: expected an integer, got {other:?}"
        ))),
    }
}

/// Is `sql` (a `CREATE TABLE`) a `WITHOUT ROWID` table? The options follow the last `)`.
fn without_rowid(sql: &str) -> bool {
    let tail = sql
        .rsplit_once(')')
        .map_or("", |(_, t)| t)
        .to_ascii_uppercase();
    let words: Vec<&str> = tail
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    words.windows(2).any(|w| w == ["WITHOUT", "ROWID"])
}

/// A parsed dump.
struct Parsed<'a> {
    manifest: DumpManifest,
    schema: String,
    data: Vec<&'a str>,
    sequences: String,
    post: String,
}

/// The manifest of `dump`, without checking the rest.
pub fn read_manifest(dump: &str) -> Result<DumpManifest, ControlError> {
    let mut lines = dump.lines();
    if lines.next() != Some(DUMP_HEADER) {
        return Err(invalid(format!("the first line is not {DUMP_HEADER:?}")));
    }
    let manifest = lines
        .next()
        .and_then(|l| l.strip_prefix(MANIFEST_PREFIX))
        .ok_or_else(|| invalid("no manifest on the second line"))?;
    serde_json::from_str(manifest).map_err(|e| invalid(format!("the manifest is not valid: {e}")))
}

fn invalid(m: impl Into<String>) -> ControlError {
    ControlError::DumpInvalid(m.into())
}

fn parse(dump: &str) -> Result<Parsed<'_>, ControlError> {
    let manifest = read_manifest(dump)?;
    let mut lines = dump.lines().skip(2);
    if lines.next() != Some(SECTION_SCHEMA) {
        return Err(invalid("no schema section"));
    }
    let mut schema = String::new();
    let mut found_data = false;
    for l in lines.by_ref() {
        if l == SECTION_DATA {
            found_data = true;
            break;
        }
        schema.push_str(l);
        schema.push('\n');
    }
    if !found_data {
        return Err(invalid("no data section"));
    }
    let mut data = Vec::new();
    let mut found_sequences = false;
    for l in lines.by_ref() {
        if l == SECTION_SEQUENCES {
            found_sequences = true;
            break;
        }
        if l.trim().is_empty() || l.starts_with("--") {
            continue;
        }
        if !l.starts_with("INSERT INTO ") {
            return Err(invalid("a data line is not an INSERT"));
        }
        data.push(l);
    }
    if !found_sequences {
        return Err(invalid("no sequences section"));
    }
    let mut sequences = String::new();
    let mut found_post = false;
    for l in lines.by_ref() {
        if l == SECTION_POST {
            found_post = true;
            break;
        }
        if l.trim().is_empty() || l.starts_with("--") {
            continue;
        }
        if !l.starts_with(SEQUENCE_LINE_PREFIX) {
            return Err(invalid(
                "a sequences line does not set a sqlite_sequence row",
            ));
        }
        sequences.push_str(l);
        sequences.push('\n');
    }
    if !found_post {
        return Err(invalid("no post-data section"));
    }
    let mut post = String::new();
    let mut found_end = false;
    for l in lines.by_ref() {
        if l == END {
            found_end = true;
            break;
        }
        post.push_str(l);
        post.push('\n');
    }
    if !found_end {
        return Err(invalid("truncated: no end marker"));
    }
    if lines.any(|l| !l.trim().is_empty()) {
        return Err(invalid("text after the end marker"));
    }
    Ok(Parsed {
        manifest,
        schema,
        data,
        sequences,
        post,
    })
}

/// A dump held in parts, so it can be written out (e.g. straight into an encryptor) without
/// first being joined into one more copy.
#[derive(Debug, Clone)]
pub struct Dump {
    pub manifest: DumpManifest,
    manifest_json: String,
    schema: String,
    data: String,
    sequences: String,
    post: String,
}

impl Dump {
    /// Writes the dump text (see the module docs) to `w`.
    pub fn write_to(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        for part in [
            DUMP_HEADER,
            "\n",
            MANIFEST_PREFIX,
            &self.manifest_json,
            "\n",
            SECTION_SCHEMA,
            "\n",
            &self.schema,
            SECTION_DATA,
            "\n",
            &self.data,
            SECTION_SEQUENCES,
            "\n",
            &self.sequences,
            SECTION_POST,
            "\n",
            &self.post,
            END,
            "\n",
        ] {
            w.write_all(part.as_bytes())?;
        }
        Ok(())
    }

    /// The length of the dump text in bytes.
    pub fn len(&self) -> usize {
        DUMP_HEADER.len()
            + MANIFEST_PREFIX.len()
            + self.manifest_json.len()
            + SECTION_SCHEMA.len()
            + self.schema.len()
            + SECTION_DATA.len()
            + self.data.len()
            + SECTION_SEQUENCES.len()
            + self.sequences.len()
            + SECTION_POST.len()
            + self.post.len()
            + END.len()
            + 7
    }

    /// Never true: a dump always has its header.
    pub fn is_empty(&self) -> bool {
        false
    }
}

fn first_integer(rows: &[Vec<SqlValue>], what: &str) -> Result<Option<i64>, ControlError> {
    match rows.first().and_then(|r| r.first()) {
        None | Some(SqlValue::Null) => Ok(None),
        Some(v) => integer(v, what).map(Some),
    }
}

impl ControlDb {
    /// Dumps the whole database as text (see [`ControlDb::dump_parts`]).
    pub async fn dump(&self, taken_at_ms: i64) -> Result<String, ControlError> {
        let d = self.dump_parts(taken_at_ms).await?;
        let mut out = Vec::with_capacity(d.len());
        d.write_to(&mut out)
            .map_err(|e| ControlError::Dump(e.to_string()))?;
        String::from_utf8(out).map_err(|e| ControlError::Dump(e.to_string()))
    }

    /// Dumps the whole database (see the module docs for the format), stamping the manifest
    /// with `taken_at_ms`. Read-only. On the local backend the whole dump is one read
    /// transaction (a snapshot); on the remote backend it is not (see the module docs).
    pub async fn dump_parts(&self, taken_at_ms: i64) -> Result<Dump, ControlError> {
        let mut r = self.reader().await;
        let local = matches!(r, Reader::Local(_));
        if local {
            r.query("BEGIN", vec![]).await?;
        }
        let res = dump_with(&mut r, taken_at_ms).await;
        if local {
            let end = r
                .query(if res.is_ok() { "COMMIT" } else { "ROLLBACK" }, vec![])
                .await;
            if res.is_ok() {
                end?;
            }
        }
        res
    }

    /// Every non-internal object in the database (`type name`).
    async fn user_objects(&self) -> Result<Vec<String>, ControlError> {
        let rows = self
            .query(
                &format!(
                    "SELECT type, name FROM sqlite_master WHERE {} AND {} ORDER BY rowid",
                    not_internal("name"),
                    not_internal("tbl_name")
                ),
                vec![],
            )
            .await?;
        rows.iter()
            .map(|r| match r.as_slice() {
                [k, n] => Ok(format!("{} {}", text(k, "type")?, text(n, "name")?)),
                _ => Err(ControlError::Db("sqlite_master: unexpected columns".into())),
            })
            .collect()
    }

    /// Applies `batch` in one transaction with foreign keys off. A remote connection may use
    /// a new stream per request, so there the pragma goes in the same request, ahead of the
    /// transaction (it has no effect inside one); the local connection keeps the pragma set by
    /// [`ControlDb::restore`].
    async fn restore_batch(&self, batch: String) -> Result<(), ControlError> {
        if batch.trim().is_empty() {
            return Ok(());
        }
        match &self.backend {
            Backend::Remote(r) => r
                .conn
                .execute_batch(&format!(
                    "PRAGMA foreign_keys = OFF;\nBEGIN;\n{batch}COMMIT;\n"
                ))
                .await
                .map(|_| ())
                .map_err(|e| r.err(e)),
            Backend::Local(_) => self.apply_batch(batch).await.map_err(|f| f.error),
        }
    }

    /// Restores `dump` (from [`ControlDb::dump`]) into this database, which must be empty.
    ///
    /// Refuses, before writing anything, a malformed or truncated dump
    /// ([`ControlError::DumpInvalid`]), a dump newer than this binary
    /// ([`ControlError::SchemaTooNew`]) and a target that already has user objects
    /// ([`ControlError::TargetNotEmpty`]). Then applies the schema, the data (in transactions of
    /// up to 500 statements) and the indexes, views and triggers, and recounts every table
    /// against the manifest ([`ControlError::CountMismatch`] names the first table that
    /// differs). A failure part-way leaves a partly restored database: start again on a fresh
    /// one.
    pub async fn restore(&self, dump: &str) -> Result<RestoreReport, ControlError> {
        let parsed = parse(dump)?;
        let manifest = &parsed.manifest;
        plan(manifest.schema_version, LATEST_CONTROL_VERSION)
            .map_err(ControlError::SchemaTooNew)?;
        let existing = self.user_objects().await?;
        if !existing.is_empty() {
            return Err(ControlError::TargetNotEmpty { objects: existing });
        }

        // Rows go in table by table in batches, so a child row may arrive before its parent;
        // integrity is checked once, after all the data (`foreign_key_check` below).
        if let Backend::Local(_) = &self.backend {
            self.query("PRAGMA foreign_keys = OFF", vec![]).await?;
        }
        self.restore_batch(parsed.schema).await?;
        let mut batch = String::new();
        let mut in_batch = 0;
        for stmt in &parsed.data {
            batch.push_str(stmt);
            batch.push('\n');
            in_batch += 1;
            if in_batch >= BATCH_STATEMENTS || batch.len() >= BATCH_BYTES {
                self.restore_batch(std::mem::take(&mut batch)).await?;
                in_batch = 0;
            }
        }
        self.restore_batch(batch).await?;
        self.restore_batch(parsed.sequences).await?;
        if let Some(row) = self
            .query("PRAGMA foreign_key_check", vec![])
            .await?
            .first()
        {
            let field = |i: usize| match row.get(i) {
                Some(SqlValue::Text(t)) => t.clone(),
                _ => "?".to_owned(),
            };
            return Err(ControlError::ForeignKeyViolation {
                table: field(0),
                parent: field(2),
            });
        }
        self.restore_batch(parsed.post).await?;

        let tables = self
            .query(
                &format!(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND {} ORDER BY name",
                    not_internal("name")
                ),
                vec![],
            )
            .await?;
        let mut counts = BTreeMap::new();
        for row in &tables {
            let name = text(row.first().unwrap_or(&SqlValue::Null), "name")?;
            let n = self
                .query(&format!("SELECT COUNT(*) FROM {}", ident(&name)), vec![])
                .await?;
            let n = integer(
                n.first().and_then(|r| r.first()).unwrap_or(&SqlValue::Null),
                "count",
            )?;
            counts.insert(name, u64::try_from(n).unwrap_or(0));
        }
        for (table, expected) in &manifest.tables {
            let found = counts.get(table).copied();
            if found != Some(*expected) {
                return Err(ControlError::CountMismatch {
                    table: table.clone(),
                    expected: Some(*expected),
                    found,
                });
            }
        }
        if let Some((table, n)) = counts
            .iter()
            .find(|(t, _)| !manifest.tables.contains_key(*t))
        {
            return Err(ControlError::CountMismatch {
                table: table.clone(),
                expected: None,
                found: Some(*n),
            });
        }
        let schema_version = self.schema_version().await?;
        if schema_version != manifest.schema_version {
            return Err(invalid(format!(
                "the restored schema version is {schema_version}, the manifest says {}",
                manifest.schema_version
            )));
        }
        Ok(RestoreReport {
            schema_version,
            tables: counts,
        })
    }
}

async fn dump_with(r: &mut Reader<'_>, taken_at_ms: i64) -> Result<Dump, ControlError> {
    let has_migrations =
        first_integer(&r.query(HAS_MIGRATIONS_TABLE, vec![]).await?, "count")?.unwrap_or(0) > 0;
    let schema_version = if has_migrations {
        first_integer(&r.query(MAX_VERSION, vec![]).await?, "version")?.unwrap_or(0)
    } else {
        0
    };
    let schema_version = u32::try_from(schema_version)
        .map_err(|_| ControlError::Db(format!("invalid schema version {schema_version}")))?;
    let objects = r
        .query(
            &format!(
                "SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL AND {} AND {} ORDER BY rowid",
                not_internal("name"),
                not_internal("tbl_name")
            ),
            vec![],
        )
        .await?;
    let mut schema = String::new();
    let mut post = String::new();
    let mut tables = Vec::new();
    for row in &objects {
        let [kind, name, sql] = row.as_slice() else {
            return Err(ControlError::Db("sqlite_master: unexpected columns".into()));
        };
        let (kind, name, sql) = (text(kind, "type")?, text(name, "name")?, text(sql, "sql")?);
        if kind == "table" {
            if sql
                .trim_start()
                .get(..14)
                .is_some_and(|p| p.eq_ignore_ascii_case("CREATE VIRTUAL"))
            {
                return Err(ControlError::Dump(format!(
                    "virtual table {name} cannot be dumped"
                )));
            }
            schema.push_str(&sql);
            schema.push_str(";\n");
            tables.push((name, sql));
        } else {
            post.push_str(&sql);
            post.push_str(";\n");
        }
    }

    let mut data = String::new();
    let mut counts = BTreeMap::new();
    for (name, sql) in &tables {
        let n = dump_table(r, name, sql, &mut data).await?;
        counts.insert(name.clone(), n);
    }

    // AUTOINCREMENT high-water marks, so a restored table never reuses a deleted id.
    let mut sequences = String::new();
    let has_sequences = first_integer(
        &r.query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            vec![],
        )
        .await?,
        "count",
    )?
    .unwrap_or(0)
        > 0;
    if has_sequences {
        for row in r
            .query(
                "SELECT name, seq FROM sqlite_sequence ORDER BY name",
                vec![],
            )
            .await?
        {
            let [name, seq] = row.as_slice() else {
                return Err(ControlError::Db(
                    "sqlite_sequence: unexpected columns".into(),
                ));
            };
            let name = text(name, "sequence name")?;
            if !counts.contains_key(&name) {
                continue;
            }
            let quoted = {
                let mut q = String::new();
                literal(&SqlValue::Text(name), &mut q);
                q
            };
            let mut seq_lit = String::new();
            literal(seq, &mut seq_lit);
            let _ = writeln!(
                sequences,
                "{SEQUENCE_LINE_PREFIX}{quoted}; INSERT INTO sqlite_sequence (name, seq) VALUES ({quoted}, {seq_lit});"
            );
        }
    }

    let manifest = DumpManifest {
        schema_version,
        taken_at: taken_at_ms,
        tables: counts,
    };
    let manifest_json = serde_json::to_string(&manifest)
        .map_err(|e| ControlError::Dump(format!("manifest: {e}")))?;
    Ok(Dump {
        manifest,
        manifest_json,
        schema,
        data,
        sequences,
        post,
    })
}

/// Appends one INSERT line per row of `table` to `out`, a page at a time (keyset paging: by
/// `rowid`, or by the primary key for a `WITHOUT ROWID` table), and returns the number of rows.
async fn dump_table(
    r: &mut Reader<'_>,
    table: &str,
    create_sql: &str,
    out: &mut String,
) -> Result<u64, ControlError> {
    // Generated columns (hidden 2 and 3) cannot be inserted; hidden 1 never appears here.
    let cols = r
        .query(
            "SELECT name, pk FROM pragma_table_xinfo(?1) WHERE hidden = 0 ORDER BY cid",
            vec![SqlValue::Text(table.to_owned())],
        )
        .await?;
    let mut names = Vec::new();
    // (position in the primary key, column index)
    let mut pk: Vec<(i64, usize)> = Vec::new();
    for (i, c) in cols.iter().enumerate() {
        let [name, key] = c.as_slice() else {
            return Err(ControlError::Db("table_xinfo: unexpected columns".into()));
        };
        let key = integer(key, "pk")?;
        if key > 0 {
            pk.push((key, i));
        }
        names.push(text(name, "column name")?);
    }
    if names.is_empty() {
        return Err(ControlError::Dump(format!("table {table} has no columns")));
    }
    pk.sort();
    let col_list = names
        .iter()
        .map(|n| ident(n))
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = format!("INSERT INTO {} ({col_list}) VALUES (", ident(table));
    let t = ident(table);

    let mut count = 0u64;
    if without_rowid(create_sql) {
        if pk.is_empty() {
            return Err(ControlError::Dump(format!(
                "table {table} has no primary key"
            )));
        }
        let key_cols = pk
            .iter()
            .map(|(_, i)| ident(&names[*i]))
            .collect::<Vec<_>>()
            .join(", ");
        let marks = (1..=pk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let first =
            format!("SELECT {col_list} FROM {t} ORDER BY {key_cols} LIMIT {DUMP_PAGE_ROWS}");
        let next = format!(
            "SELECT {col_list} FROM {t} WHERE ({key_cols}) > ({marks}) ORDER BY {key_cols} LIMIT {DUMP_PAGE_ROWS}"
        );
        let mut after: Option<Vec<SqlValue>> = None;
        loop {
            let rows = match &after {
                None => r.query(&first, vec![]).await?,
                Some(k) => r.query(&next, k.clone()).await?,
            };
            for row in &rows {
                push_insert(&prefix, row, out);
            }
            count += rows.len() as u64;
            match rows.last() {
                Some(last) if (rows.len() as i64) == DUMP_PAGE_ROWS => {
                    after = Some(pk.iter().map(|(_, i)| last[*i].clone()).collect());
                }
                _ => return Ok(count),
            }
        }
    }
    let first = format!("SELECT rowid, {col_list} FROM {t} ORDER BY rowid LIMIT {DUMP_PAGE_ROWS}");
    let next = format!(
        "SELECT rowid, {col_list} FROM {t} WHERE rowid > ?1 ORDER BY rowid LIMIT {DUMP_PAGE_ROWS}"
    );
    let mut after: Option<i64> = None;
    loop {
        let rows = match after {
            None => r.query(&first, vec![]).await?,
            Some(a) => r.query(&next, vec![SqlValue::Integer(a)]).await?,
        };
        for row in &rows {
            let (rowid, values) = row
                .split_first()
                .ok_or_else(|| ControlError::Db(format!("{table}: empty row")))?;
            after = Some(integer(rowid, "rowid")?);
            push_insert(&prefix, values, out);
        }
        count += rows.len() as u64;
        if (rows.len() as i64) < DUMP_PAGE_ROWS {
            return Ok(count);
        }
    }
}

fn push_insert(prefix: &str, values: &[SqlValue], out: &mut String) {
    out.push_str(prefix);
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        literal(v, out);
    }
    out.push_str(");\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(v: SqlValue) -> String {
        let mut s = String::new();
        literal(&v, &mut s);
        s
    }

    #[test]
    fn literals_quote_escape_and_keep_their_class() {
        assert_eq!(lit(SqlValue::Null), "NULL");
        assert_eq!(lit(SqlValue::Integer(-5)), "-5");
        assert_eq!(lit(SqlValue::Integer(i64::MIN)), "(-9223372036854775807-1)");
        assert_eq!(lit(SqlValue::Real(3.0)), "3.0");
        assert_eq!(lit(SqlValue::Real(1e300)), "1e300");
        assert_eq!(lit(SqlValue::Real(f64::INFINITY)), "9e999");
        assert_eq!(lit(SqlValue::Text("O'Brien".into())), "'O''Brien'");
        assert_eq!(
            lit(SqlValue::Text("a\nb".into())),
            "CAST(X'610A62' AS TEXT)"
        );
        assert_eq!(lit(SqlValue::Blob(vec![0, 255])), "X'00FF'");
        assert_eq!(lit(SqlValue::Blob(vec![])), "X''");
    }

    #[test]
    fn without_rowid_is_read_from_the_table_options() {
        assert!(without_rowid(
            "CREATE TABLE t (a PRIMARY KEY) WITHOUT ROWID"
        ));
        assert!(without_rowid(
            "CREATE TABLE t (a PRIMARY KEY) strict,  without\n rowid"
        ));
        assert!(!without_rowid(
            "CREATE TABLE t (a TEXT DEFAULT 'WITHOUT ROWID')"
        ));
        assert!(!without_rowid("CREATE TABLE t (a)"));
    }

    #[test]
    fn identifiers_double_their_quotes() {
        assert_eq!(ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn a_dump_without_every_marker_is_invalid() {
        let ok = format!(
            "{DUMP_HEADER}\n{MANIFEST_PREFIX}{{\"schema_version\":1,\"taken_at\":0,\"tables\":{{}}}}\n{SECTION_SCHEMA}\n{SECTION_DATA}\n{SECTION_SEQUENCES}\n{SECTION_POST}\n{END}\n"
        );
        assert!(parse(&ok).is_ok());
        for broken in [
            ok.replace(DUMP_HEADER, "-- something else"),
            ok.replace(MANIFEST_PREFIX, "-- nope: "),
            ok.replace(END, ""),
            ok.replace(SECTION_POST, ""),
            format!("{ok}DROP TABLE x;\n"),
            ok.replace(
                &format!("{SECTION_DATA}\n"),
                &format!("{SECTION_DATA}\nDROP TABLE x;\n"),
            ),
        ] {
            assert!(
                matches!(parse(&broken), Err(ControlError::DumpInvalid(_))),
                "{broken}"
            );
        }
    }
}

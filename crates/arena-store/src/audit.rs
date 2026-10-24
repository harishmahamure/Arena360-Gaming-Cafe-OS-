//! Read-only audit queries over the append-only `changes` log (Story 1.15): a filtered,
//! keyset-paged list (newest first by `(at, epoch, seq)`, never `OFFSET`), one change with the
//! row image of the previous change to the same row, and the names the viewer shows.

use arena_proto::AppError;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::branch::{CHANGE_COLUMNS, ChangeOp, ChangeRecord, change_from};
use crate::db_error;

/// A position in the audit order: the last item of a page. The next page starts strictly after
/// it (older).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditCursor {
    pub at: i64,
    pub epoch: String,
    pub seq: i64,
}

/// Which changes an audit page holds. Every set filter must match (AND).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    /// `actor_kind` must equal this.
    pub actor_kind: Option<String>,
    /// `actor_id` must equal this (only with `actor_kind`).
    pub actor_id: Option<String>,
    /// The intent domain: the generated `module` column (the text of `intent_kind` before its
    /// first '.', or the whole kind when it has none) must equal this.
    pub module: Option<String>,
    /// `at >= from_ms`.
    pub from_ms: Option<i64>,
    /// `at < to_ms`.
    pub to_ms: Option<i64>,
    /// Only changes after (older than) this position.
    pub after: Option<AuditCursor>,
}

/// One change as the audit list shows it (no row image).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub epoch: String,
    pub seq: i64,
    pub at: i64,
    pub business_day: Option<String>,
    pub actor_kind: String,
    pub actor_id: String,
    pub elevated_by_kind: Option<String>,
    pub elevated_by_id: Option<String>,
    pub elevated_by_mode: Option<String>,
    pub intent_kind: String,
    pub intent_id: String,
    pub tbl: String,
    pub row_id: String,
    pub op: ChangeOp,
    /// For a `device_admin_exits` change: whether that exit is flagged for review now.
    pub flagged: Option<bool>,
}

impl From<&ChangeRecord> for AuditRow {
    fn from(c: &ChangeRecord) -> Self {
        Self {
            epoch: c.epoch.clone(),
            seq: c.seq,
            at: c.at,
            business_day: c.business_day.clone(),
            actor_kind: c.actor_kind.clone(),
            actor_id: c.actor_id.clone(),
            elevated_by_kind: c.elevated_by_kind.clone(),
            elevated_by_id: c.elevated_by_id.clone(),
            elevated_by_mode: c.elevated_by_mode.clone(),
            intent_kind: c.intent_kind.clone(),
            intent_id: c.intent_id.clone(),
            tbl: c.tbl.clone(),
            row_id: c.row_id.clone(),
            op: c.op,
            flagged: None,
        }
    }
}

/// Up to `limit` changes matching `filter`, newest first by `(at DESC, epoch DESC, seq DESC)`.
/// Pages by keyset (the row-value comparison against [`AuditFilter::after`]), so a page costs
/// the same however deep it is.
pub fn list_audit(
    conn: &Connection,
    filter: &AuditFilter,
    limit: u32,
) -> Result<Vec<AuditRow>, AppError> {
    let (sql, values) = audit_sql(filter, limit);
    let mut stmt = conn.prepare_cached(&sql).map_err(db_error)?;
    let rows = stmt
        .query_map(params_from_iter(values), |r| {
            let op: String = r.get(13)?;
            Ok(AuditRow {
                epoch: r.get(0)?,
                seq: r.get(1)?,
                at: r.get(2)?,
                business_day: r.get(3)?,
                actor_kind: r.get(4)?,
                actor_id: r.get(5)?,
                elevated_by_kind: r.get(6)?,
                elevated_by_id: r.get(7)?,
                elevated_by_mode: r.get(8)?,
                intent_kind: r.get(9)?,
                intent_id: r.get(10)?,
                tbl: r.get(11)?,
                row_id: r.get(12)?,
                op: ChangeOp::parse(&op).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Text,
                        format!("unknown op {op}").into(),
                    )
                })?,
                flagged: r.get::<_, Option<i64>>(14)?.map(|f| f != 0),
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The SQL and parameters of one audit page.
fn audit_sql(filter: &AuditFilter, limit: u32) -> (String, Vec<Value>) {
    let mut clauses: Vec<&str> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    if let Some(kind) = &filter.actor_kind {
        clauses.push("actor_kind = ?");
        values.push(Value::Text(kind.clone()));
        if let Some(id) = &filter.actor_id {
            clauses.push("actor_id = ?");
            values.push(Value::Text(id.clone()));
        }
    }
    if let Some(module) = &filter.module {
        // Equality on the generated column, so `changes_module_at` serves it in keyset order.
        clauses.push("module = ?");
        values.push(Value::Text(module.clone()));
    }
    if let Some(from) = filter.from_ms {
        clauses.push("at >= ?");
        values.push(Value::Integer(from));
    }
    if let Some(to) = filter.to_ms {
        clauses.push("at < ?");
        values.push(Value::Integer(to));
    }
    if let Some(c) = &filter.after {
        clauses.push("(at, epoch, seq) < (?, ?, ?)");
        values.push(Value::Integer(c.at));
        values.push(Value::Text(c.epoch.clone()));
        values.push(Value::Integer(c.seq));
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    values.push(Value::Integer(i64::from(limit)));
    let sql = format!(
        "SELECT epoch, seq, at, business_day, actor_kind, actor_id, elevated_by_kind,
                elevated_by_id, elevated_by_mode, intent_kind, intent_id, tbl, row_id, op,
                CASE WHEN tbl = 'device_admin_exits'
                     THEN (SELECT e.flagged FROM device_admin_exits e WHERE e.exit_id = changes.row_id)
                END
         FROM changes {where_sql}
         ORDER BY at DESC, epoch DESC, seq DESC
         LIMIT ?"
    );
    (sql, values)
}

/// The change `(epoch, seq)`, if it exists.
pub fn find_change(
    conn: &Connection,
    epoch: &str,
    seq: i64,
) -> Result<Option<ChangeRecord>, AppError> {
    let sql = format!("SELECT {CHANGE_COLUMNS} FROM changes WHERE epoch = ?1 AND seq = ?2");
    conn.query_row(&sql, params![epoch, seq], change_from)
        .optional()
        .map_err(db_error)
}

/// The row image (MessagePack) of the change to `(tbl, row_id)` that comes right before
/// `(epoch, seq)` in log order (`epoch_counter`, `epoch`, `seq`), or `None` when there is none
/// or it was a delete: either way this change (re)created the row. One indexed lookup
/// (`changes_row`).
pub fn previous_row_image(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
    epoch: &str,
    seq: i64,
) -> Result<Option<Vec<u8>>, AppError> {
    let found: Option<(String, Vec<u8>)> = conn
        .prepare_cached(PREVIOUS_SQL)
        .map_err(db_error)?
        .query_row(params![tbl, row_id, epoch, seq], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(db_error)?;
    Ok(found.and_then(|(op, image)| (op != ChangeOp::Delete.as_str()).then_some(image)))
}

const PREVIOUS_SQL: &str = "SELECT op, row FROM changes
     WHERE tbl = ?1 AND row_id = ?2
       AND (epoch_counter, epoch, seq)
           < (CAST(substr(?3, 1, instr(?3, ':') - 1) AS INTEGER), ?3, ?4)
     ORDER BY epoch_counter DESC, epoch DESC, seq DESC
     LIMIT 1";

/// Every staff member's `(staff_id, display_name)`, ordered by name then id.
pub fn list_staff_names(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT staff_id, display_name FROM staff ORDER BY display_name COLLATE NOCASE, staff_id",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Every device's `(device_id, label)`.
pub fn list_device_labels(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn
        .prepare_cached("SELECT device_id, label FROM devices ORDER BY device_id")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, insert_change};

    fn change(epoch: &str, seq: i64, at: i64, actor: (&str, &str), kind: &str) -> ChangeRecord {
        ChangeRecord {
            epoch: epoch.into(),
            seq,
            tbl: "settings".into(),
            row_id: "B/k".into(),
            op: ChangeOp::Update,
            row: vec![0x80],
            intent_id: format!("I{seq}"),
            intent_kind: kind.into(),
            actor_kind: actor.0.into(),
            actor_id: actor.1.into(),
            elevated_by_kind: None,
            elevated_by_id: None,
            elevated_by_mode: None,
            elevated_by_verifier_version: None,
            at,
            business_day: None,
            fy: None,
            reason: None,
        }
    }

    fn store_with(changes: &[ChangeRecord]) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                for c in changes {
                    insert_change(tx, c)?;
                }
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    #[test]
    fn pages_newest_first_without_overlap_or_gap() {
        let mut all = Vec::new();
        for seq in 1..=7 {
            // Two changes share each timestamp: epoch then seq break the tie.
            all.push(change(
                "1:H",
                seq,
                1000 + seq / 2,
                ("staff", "S"),
                "settings.set",
            ));
        }
        let (_d, store) = store_with(&all);
        let mut filter = AuditFilter::default();
        let mut seen = Vec::new();
        loop {
            let page = list_audit(&store.conn, &filter, 3).unwrap();
            if page.is_empty() {
                break;
            }
            let last = page.last().unwrap();
            filter.after = Some(AuditCursor {
                at: last.at,
                epoch: last.epoch.clone(),
                seq: last.seq,
            });
            seen.extend(page.into_iter().map(|r| r.seq));
        }
        assert_eq!(seen, vec![7, 6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn filters_combine_with_and() {
        let (_d, store) = store_with(&[
            change("1:H", 1, 100, ("staff", "A"), "settings.set"),
            change("1:H", 2, 200, ("staff", "B"), "settings.set"),
            change("1:H", 3, 300, ("staff", "A"), "staff.upsert"),
            change("1:H", 4, 400, ("staff", "A"), "settings.set"),
            change("1:H", 5, 500, ("system", "hub"), "settingsx.y"),
        ]);
        let filter = AuditFilter {
            actor_kind: Some("staff".into()),
            actor_id: Some("A".into()),
            module: Some("settings".into()),
            from_ms: Some(100),
            to_ms: Some(400),
            after: None,
        };
        let seqs: Vec<i64> = list_audit(&store.conn, &filter, 50)
            .unwrap()
            .into_iter()
            .map(|r| r.seq)
            .collect();
        assert_eq!(seqs, vec![1]);
        let module_only = AuditFilter {
            module: Some("settings".into()),
            ..AuditFilter::default()
        };
        let seqs: Vec<i64> = list_audit(&store.conn, &module_only, 50)
            .unwrap()
            .into_iter()
            .map(|r| r.seq)
            .collect();
        assert_eq!(seqs, vec![4, 2, 1], "settingsx.* is another module");
        // A kind without a dot is its own module.
        let (_d2, dotless) = store_with(&[change("1:H", 1, 1, ("system", "hub"), "odd")]);
        let odd = AuditFilter {
            module: Some("odd".into()),
            ..AuditFilter::default()
        };
        assert_eq!(list_audit(&dotless.conn, &odd, 50).unwrap().len(), 1);
        let kind_only = AuditFilter {
            actor_kind: Some("system".into()),
            ..AuditFilter::default()
        };
        assert_eq!(list_audit(&store.conn, &kind_only, 50).unwrap().len(), 1);
    }

    #[test]
    fn the_previous_image_follows_log_order_across_epochs() {
        let mut a = change("2:H", 1, 50, ("staff", "S"), "settings.set");
        a.row = vec![1];
        let mut b = change("10:H", 1, 40, ("staff", "S"), "settings.set");
        b.row = vec![2];
        let mut c = change("10:H", 2, 60, ("staff", "S"), "settings.set");
        c.row = vec![3];
        let (_d, store) = store_with(&[a, b, c]);
        let prev = |e: &str, s: i64| previous_row_image(&store.conn, "settings", "B/k", e, s);
        // Epoch 10 comes after epoch 2 (numerically), whatever the timestamps say.
        assert_eq!(prev("10:H", 2).unwrap(), Some(vec![2]));
        assert_eq!(prev("10:H", 1).unwrap(), Some(vec![1]));
        assert_eq!(prev("2:H", 1).unwrap(), None);
        assert!(find_change(&store.conn, "10:H", 2).unwrap().is_some());
        assert_eq!(epoch_counter_of(&store, "10:H", 2), 10);
        assert!(find_change(&store.conn, "10:H", 9).unwrap().is_none());
    }

    fn epoch_counter_of(store: &Store, epoch: &str, seq: i64) -> i64 {
        store
            .conn
            .query_row(
                "SELECT epoch_counter FROM changes WHERE epoch = ?1 AND seq = ?2",
                params![epoch, seq],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_change_after_a_delete_has_no_before_image() {
        let mut a = change("1:H", 1, 1, ("staff", "S"), "settings.set");
        a.row = vec![1];
        let mut b = change("1:H", 2, 2, ("staff", "S"), "settings.set");
        b.op = ChangeOp::Delete;
        b.row = vec![1];
        let mut c = change("1:H", 3, 3, ("staff", "S"), "settings.set");
        c.op = ChangeOp::Insert;
        c.row = vec![3];
        let (_d, store) = store_with(&[a, b, c]);
        let prev = |s: i64| previous_row_image(&store.conn, "settings", "B/k", "1:H", s).unwrap();
        assert_eq!(prev(3), None, "re-created after a delete");
        assert_eq!(prev(2), Some(vec![1]));
    }

    fn plan(store: &Store, sql: &str, values: Vec<Value>) -> String {
        let mut stmt = store
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        stmt.query_map(params_from_iter(values), |r| r.get::<_, String>(3))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn the_audit_indexes_serve_the_page_queries() {
        let (_d, store) = store_with(&[]);
        let cursor = Some(AuditCursor {
            at: 5,
            epoch: "1:H".into(),
            seq: 3,
        });
        let actor = |id: Option<&str>| AuditFilter {
            actor_kind: Some("staff".into()),
            actor_id: id.map(str::to_owned),
            ..AuditFilter::default()
        };
        let module = AuditFilter {
            module: Some("settings".into()),
            ..AuditFilter::default()
        };
        let cases: Vec<(&str, AuditFilter, &str)> = vec![
            ("unfiltered", AuditFilter::default(), "changes_at"),
            ("actor", actor(Some("x")), "changes_actor_at"),
            ("kind only", actor(None), "changes_actor_kind_at"),
            ("module only", module.clone(), "changes_module_at"),
            (
                "actor + dates",
                AuditFilter {
                    from_ms: Some(1),
                    to_ms: Some(9),
                    ..actor(Some("x"))
                },
                "changes_actor_at",
            ),
            (
                "module + dates",
                AuditFilter {
                    from_ms: Some(1),
                    to_ms: Some(9),
                    ..module.clone()
                },
                "changes_module_at",
            ),
            (
                "actor + module",
                AuditFilter {
                    module: Some("settings".into()),
                    ..actor(Some("x"))
                },
                "changes_",
            ),
            (
                "kind + module + dates",
                AuditFilter {
                    module: Some("settings".into()),
                    from_ms: Some(1),
                    ..actor(None)
                },
                "changes_",
            ),
        ];
        for (name, filter, index) in cases {
            for after in [None, cursor.clone()] {
                let f = AuditFilter {
                    after,
                    ..filter.clone()
                };
                let (sql, values) = audit_sql(&f, 50);
                let p = plan(&store, &sql, values);
                assert!(p.contains(index), "{name}: {p}");
                assert!(!p.contains("TEMP B-TREE"), "{name} sorts: {p}");
            }
        }
        let p = plan(
            &store,
            PREVIOUS_SQL,
            vec![
                Value::Text("settings".into()),
                Value::Text("x".into()),
                Value::Text("1:H".into()),
                Value::Integer(1),
            ],
        );
        assert!(
            p.contains("changes_row") && !p.contains("TEMP B-TREE"),
            "{p}"
        );
    }
}

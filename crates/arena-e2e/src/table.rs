//! The Hub's change log as the driver checks it: gap-free `seq`, applied intents per kind
//! (against the driver's ledger) and a normalised table two runs can be compared by (ids,
//! timestamps, hashes and epochs masked).

use std::collections::BTreeMap;
use std::path::Path;

use arena_store::{ChangeRecord, RowValue, Store};
use serde_json::{Map, Value, json};

/// `kiosk.log` codes the kiosk's app watchdog writes on its own schedule (an app that is slow
/// to connect): timing, not a flow, so they are left out of the comparison.
pub const TIMING_LOG_CODES: [&str; 2] = ["app_restarted", "no_session"];

/// Every change row of the branch database at `db`.
pub fn read(db: &Path) -> Result<Vec<ChangeRecord>, String> {
    Store::open_reader(db)
        .and_then(|s| s.changes())
        .map_err(|e| format!("read {}: {}", db.display(), e.message))
}

/// `seq` must run 1, 2, 3… with no gap or repeat within each epoch.
pub fn check_gap_free(changes: &[ChangeRecord]) -> Result<(), String> {
    let mut by_epoch: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for c in changes {
        by_epoch.entry(c.epoch.as_str()).or_default().push(c.seq);
    }
    for (epoch, mut seqs) in by_epoch {
        seqs.sort_unstable();
        for (i, seq) in seqs.iter().enumerate() {
            let want = i as i64 + 1;
            if *seq != want {
                return Err(format!(
                    "epoch {epoch}: seq {seq} where {want} was expected (gap or duplicate)"
                ));
            }
        }
    }
    Ok(())
}

/// How many distinct intents of each kind applied.
pub fn intents_by_kind(changes: &[ChangeRecord]) -> BTreeMap<String, u64> {
    let mut seen: BTreeMap<String, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for c in changes {
        seen.entry(c.intent_kind.clone())
            .or_default()
            .insert(c.intent_id.as_str());
    }
    seen.into_iter()
        .map(|(k, ids)| (k, ids.len() as u64))
        .collect()
}

/// `after - before` must equal `ledger` for every kind the ledger names: nothing the driver
/// caused was lost or applied twice.
pub fn check_ledger(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
    ledger: &BTreeMap<String, u64>,
) -> Result<(), String> {
    let mut bad = Vec::new();
    for (kind, want) in ledger {
        let (a, b) = (
            after.get(kind).copied().unwrap_or(0),
            before.get(kind).copied().unwrap_or(0),
        );
        match a.checked_sub(b) {
            Some(got) if got == *want => {}
            Some(got) => bad.push(format!("{kind}: {got} applied, the driver caused {want}")),
            None => bad.push(format!(
                "{kind}: {a} applied now but {b} before the run, the driver caused {want}"
            )),
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("; "))
    }
}

/// No intent may insert the same row twice: an intent applied a second time (with fresh
/// `seq`s) passes the gap check and the distinct-intent ledger, but not this.
pub fn check_no_duplicates(changes: &[ChangeRecord]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for c in changes {
        if c.op == arena_store::ChangeOp::Insert
            && !seen.insert((c.intent_id.as_str(), c.tbl.as_str(), c.row_id.as_str()))
        {
            return Err(format!(
                "intent {} ({}) inserted {} {} twice (at seq {} of epoch {})",
                c.intent_id, c.intent_kind, c.tbl, c.row_id, c.seq, c.epoch
            ));
        }
    }
    Ok(())
}

fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes().all(|b| {
            b.is_ascii_digit()
                || (b.is_ascii_uppercase() && !matches!(b, b'I' | b'L' | b'O' | b'U'))
        })
}

/// Masks the run-specific parts of a text: ULIDs, argon2 hashes and long hex strings.
pub fn mask_text(s: &str) -> String {
    if s.contains("$argon2") {
        return "<hash>".into();
    }
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if is_ulid(word) {
            out.push_str("<id>");
        } else if word.len() >= 32 && word.bytes().all(|b| b.is_ascii_hexdigit()) {
            out.push_str("<hex>");
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Masks a value: timestamps (integers from 10^12 on) and texts as [`mask_text`]. A text that
/// is JSON is masked inside.
fn mask_value(v: &RowValue) -> Value {
    match v {
        RowValue::Null => Value::Null,
        RowValue::Integer(n) if *n >= 1_000_000_000_000 => json!("<ts>"),
        RowValue::Integer(n) => json!(n),
        RowValue::Real(f) => json!(f),
        RowValue::Text(t) => match serde_json::from_str::<Value>(t) {
            Ok(j @ (Value::Object(_) | Value::Array(_))) => mask_json(&j),
            _ => json!(mask_text(t)),
        },
    }
}

fn mask_json(v: &Value) -> Value {
    match v {
        Value::String(s) => json!(mask_text(s)),
        Value::Number(n) if n.as_i64().is_some_and(|n| n >= 1_000_000_000_000) => json!("<ts>"),
        Value::Array(a) => Value::Array(a.iter().map(mask_json).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (mask_text(k), mask_json(v)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

/// The `kiosk.log` code of a change, if it is one.
fn log_code(c: &ChangeRecord, row: &BTreeMap<String, RowValue>) -> Option<String> {
    (c.intent_kind == "kiosk.log")
        .then(|| match row.get("code") {
            Some(RowValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .flatten()
}

/// One change row with everything run-specific masked: no epoch, seq, ids or times. Rows the
/// watchdog writes on its own schedule ([`TIMING_LOG_CODES`]) are `None`.
pub fn normalise(c: &ChangeRecord) -> Result<Option<Value>, String> {
    let row: BTreeMap<String, RowValue> = rmp_serde::from_slice(&c.row)
        .map_err(|e| format!("change {}:{}: row: {e}", c.epoch, c.seq))?;
    if log_code(c, &row).is_some_and(|code| TIMING_LOG_CODES.contains(&code.as_str())) {
        return Ok(None);
    }
    let masked: Map<String, Value> = row
        .iter()
        .map(|(k, v)| (k.clone(), mask_value(v)))
        .collect();
    Ok(Some(json!({
        "tbl": c.tbl,
        "op": format!("{:?}", c.op),
        "row_id": mask_text(&c.row_id),
        "intent_kind": c.intent_kind,
        "actor_kind": c.actor_kind,
        "actor_id": mask_text(&c.actor_id),
        "elevated_by_kind": c.elevated_by_kind,
        "elevated_by_mode": c.elevated_by_mode,
        "elevated_by_verifier_version": c.elevated_by_verifier_version,
        "reason": c.reason,
        "row": Value::Object(masked),
    })))
}

/// The normalised table: one line per change row, sorted (kiosk events and Hub UI intents
/// interleave differently from run to run; their order is not part of the result).
pub fn normalised_table(changes: &[ChangeRecord]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for c in changes {
        if let Some(v) = normalise(c)? {
            lines.push(v.to_string());
        }
    }
    lines.sort();
    Ok(lines)
}

/// Compares two normalised tables; `Err` lists what only one side has.
pub fn compare(a: &[String], b: &[String]) -> Result<(), String> {
    let count = |t: &[String]| {
        let mut m: BTreeMap<String, i64> = BTreeMap::new();
        for l in t {
            *m.entry(l.clone()).or_default() += 1;
        }
        m
    };
    let (ca, cb) = (count(a), count(b));
    let mut diff = Vec::new();
    for (line, n) in &ca {
        let m = cb.get(line).copied().unwrap_or(0);
        if *n != m {
            diff.push(format!("{n} vs {m}: {line}"));
        }
    }
    for (line, m) in &cb {
        if !ca.contains_key(line) {
            diff.push(format!("0 vs {m}: {line}"));
        }
    }
    if diff.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} differing rows (first run vs second):\n{}",
            diff.len(),
            diff.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_store::ChangeOp;

    fn rec(
        epoch: &str,
        seq: i64,
        kind: &str,
        intent: &str,
        row: &[(&str, RowValue)],
    ) -> ChangeRecord {
        let row: BTreeMap<String, RowValue> = row
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        ChangeRecord {
            epoch: epoch.into(),
            seq,
            tbl: "settings".into(),
            row_id: "01JBRANCHAAAAAAAAAAAAAAAAA/display.hc.01JDEVXCEAAAAAAAAAAAAAAAAA".into(),
            op: ChangeOp::Update,
            row: rmp_serde::to_vec_named(&row).unwrap(),
            intent_id: intent.into(),
            intent_kind: kind.into(),
            actor_kind: "staff".into(),
            actor_id: "01JSTAFFAAAAAAAAAAAAAAAAAA".into(),
            elevated_by_kind: None,
            elevated_by_id: None,
            elevated_by_mode: None,
            elevated_by_verifier_version: None,
            at: 1_800_000_000_000 + seq,
            business_day: Some("2026-10-24".into()),
            fy: Some("FY2026-27".into()),
            reason: None,
        }
    }

    #[test]
    fn masking_removes_ids_times_and_hashes_only() {
        assert_eq!(
            mask_text("B/display.hc.01JDEVXCEAAAAAAAAAAAAAAAAA"),
            "B/display.hc.<id>"
        );
        assert_eq!(mask_text("$argon2id$v=19$m=1"), "<hash>");
        assert_eq!(mask_text(&"ab".repeat(32)), "<hex>");
        assert_eq!(mask_text("e2e footer 3"), "e2e footer 3");
        assert_eq!(
            mask_value(&RowValue::Text(
                r#"[{"path_glob":"x","at":1800000000000}]"#.into()
            )),
            json!([{"path_glob": "x", "at": "<ts>"}])
        );
        assert_eq!(mask_value(&RowValue::Integer(5)), json!(5));
    }

    #[test]
    fn two_runs_with_other_ids_and_times_normalise_equal() {
        let a = rec(
            "1:01JHUBAAAAAAAAAAAAAAAAAAAA",
            1,
            "settings.set",
            "01JINTENTAAAAAAAAAAAAAAAAA",
            &[
                ("value", RowValue::Text("true".into())),
                ("updated_at", RowValue::Integer(1_800_000_000_001)),
                ("rev", RowValue::Integer(2)),
            ],
        );
        let mut b = a.clone();
        b.epoch = "1:01JHUBBBBBBBBBBBBBBBBBBBBB".into();
        b.seq = 9;
        b.intent_id = "01JINTENTBBBBBBBBBBBBBBBBB".into();
        b.at += 55;
        b.row = rmp_serde::to_vec_named(&BTreeMap::from([
            ("value".to_owned(), RowValue::Text("true".into())),
            (
                "updated_at".to_owned(),
                RowValue::Integer(1_800_000_099_999),
            ),
            ("rev".to_owned(), RowValue::Integer(2)),
        ]))
        .unwrap();
        let (ta, tb) = (
            normalised_table(std::slice::from_ref(&a)).unwrap(),
            normalised_table(&[b]).unwrap(),
        );
        assert_eq!(compare(&ta, &tb), Ok(()));
        let mut c = a;
        c.row = rmp_serde::to_vec_named(&BTreeMap::from([(
            "value".to_owned(),
            RowValue::Text("false".into()),
        )]))
        .unwrap();
        let err = compare(&ta, &normalised_table(&[c]).unwrap()).unwrap_err();
        assert!(err.contains("2 differing rows"), "{err}");
    }

    #[test]
    fn watchdog_logs_are_left_out() {
        let mut c = rec(
            "E",
            1,
            "kiosk.log",
            "I",
            &[("code", RowValue::Text("app_restarted".into()))],
        );
        c.tbl = "device_events".into();
        assert_eq!(normalise(&c).unwrap(), None);
        let mut d = rec(
            "E",
            1,
            "kiosk.log",
            "I",
            &[("code", RowValue::Text("process_blocked".into()))],
        );
        d.tbl = "device_events".into();
        assert!(normalise(&d).unwrap().is_some());
    }

    #[test]
    fn gaps_and_duplicates_are_caught_per_epoch() {
        let r = |e: &str, s| rec(e, s, "k", "I", &[]);
        assert_eq!(check_gap_free(&[r("A", 1), r("A", 2), r("B", 1)]), Ok(()));
        assert!(check_gap_free(&[r("A", 1), r("A", 3)]).is_err());
        assert!(check_gap_free(&[r("A", 1), r("A", 1)]).is_err());
        assert!(check_gap_free(&[r("A", 2)]).is_err());
    }

    #[test]
    fn a_doubled_insert_is_caught() {
        let mut first = rec("A", 1, "device.admin_exit", "I1", &[]);
        first.op = ChangeOp::Insert;
        let mut again = first.clone();
        again.seq = 2;
        assert_eq!(check_no_duplicates(std::slice::from_ref(&first)), Ok(()));
        // Gap-free and one distinct intent, yet the row was inserted twice.
        assert_eq!(check_gap_free(&[first.clone(), again.clone()]), Ok(()));
        assert!(check_no_duplicates(&[first.clone(), again]).is_err());
        // An update of the same row by the same intent is not a duplicate insert.
        let mut update = first.clone();
        update.seq = 2;
        update.op = ChangeOp::Update;
        assert_eq!(check_no_duplicates(&[first, update]), Ok(()));
    }

    #[test]
    fn the_ledger_never_underflows() {
        let before = BTreeMap::from([("settings.set".to_owned(), 5)]);
        let after = BTreeMap::from([("settings.set".to_owned(), 2)]);
        let ledger = BTreeMap::from([("settings.set".to_owned(), 1)]);
        assert!(check_ledger(&before, &after, &ledger).is_err());
    }

    #[test]
    fn the_ledger_counts_distinct_intents() {
        let changes = [
            rec("A", 1, "settings.set", "I1", &[]),
            rec("A", 2, "settings.set", "I1", &[]),
            rec("A", 3, "settings.set", "I2", &[]),
            rec("A", 4, "device.admin_exit", "I3", &[]),
        ];
        let after = intents_by_kind(&changes);
        assert_eq!(after["settings.set"], 2);
        let before = BTreeMap::from([("settings.set".to_owned(), 1)]);
        let ok = BTreeMap::from([
            ("settings.set".to_owned(), 1),
            ("device.admin_exit".to_owned(), 1),
        ]);
        assert_eq!(check_ledger(&before, &after, &ok), Ok(()));
        let lost = BTreeMap::from([("device.admin_exit".to_owned(), 2)]);
        assert!(check_ledger(&before, &after, &lost).is_err());
    }
}

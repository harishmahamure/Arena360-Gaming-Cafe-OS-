//! The audit builder's fixed query catalogue (Story 8.37, AD-37): the only questions
//! `GET /v1/branches/{id}/audit?q=` answers over a built month. A query names one catalogue entry
//! (`q`) and the parameters that entry takes; an unknown entry or a parameter outside its list is
//! `UNSUPPORTED`, so no caller can shape its own query over branch data. Pages are
//! [`AUDIT_QUERY_PAGE`] rows, newest first.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audit::AuditItem;
use crate::{AppError, ErrorCode};

/// Rows in one page of a search.
pub const AUDIT_QUERY_PAGE: u32 = 50;
/// The most the builder keeps on its local disk for audit months before it evicts the least
/// recently used, in bytes.
pub const AUDIT_LOCAL_CACHE_BYTES: u64 = 50 * 1024 * 1024 * 1024;

/// An entry of the query catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AuditQueryKind {
    /// Every change, newest first.
    Recent,
    /// Changes by one actor (`actor_kind`, optionally `actor_id`).
    Actor,
    /// Changes in one module, the domain of the intent (`module`).
    Module,
    /// Changes made by one intent kind (`intent`).
    Intent,
    /// Changes to one table, or to one row of it (`tbl`, optionally `row_id`).
    Entity,
    /// Changes inside a time range (`from` and `to`, UTC ms, `to` exclusive).
    Time,
}

impl AuditQueryKind {
    pub const ALL: [Self; 6] = [
        Self::Recent,
        Self::Actor,
        Self::Module,
        Self::Intent,
        Self::Entity,
        Self::Time,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Actor => "actor",
            Self::Module => "module",
            Self::Intent => "intent",
            Self::Entity => "entity",
            Self::Time => "time",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// The parameters this entry takes besides `month`, `q` and `cursor`.
    fn params(self) -> &'static [&'static str] {
        match self {
            Self::Recent => &["from", "to"],
            Self::Actor => &["actor_kind", "actor_id", "from", "to"],
            Self::Module => &["module", "from", "to"],
            Self::Intent => &["intent", "from", "to"],
            Self::Entity => &["tbl", "row_id", "from", "to"],
            Self::Time => &["from", "to"],
        }
    }
}

/// Parameters every search takes.
const COMMON_PARAMS: [&str; 3] = ["month", "q", "cursor"];
/// The actor kinds a change records.
const ACTOR_KINDS: [&str; 5] = ["staff", "owner", "admin", "system", "device"];
const WORD_MAX: usize = 64;
const ID_MAX: usize = 128;

/// A search that passed the catalogue: which entry, and its filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSearch {
    pub kind: AuditQueryKind,
    pub actor_kind: Option<String>,
    pub actor_id: Option<String>,
    pub module: Option<String>,
    pub intent_kind: Option<String>,
    pub tbl: Option<String>,
    pub row_id: Option<String>,
    /// `at >= from_ms`.
    pub from_ms: Option<i64>,
    /// `at < to_ms`.
    pub to_ms: Option<i64>,
    /// The opaque cursor of the page before; the caller decodes it.
    pub cursor: Option<String>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

fn is_word(s: &str, extra: &[u8]) -> bool {
    !s.is_empty()
        && s.len() <= WORD_MAX
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || extra.contains(&b)
        })
}

impl AuditSearch {
    /// Checks `params` (the whole query string, `month` included) against the catalogue.
    /// A missing or unknown `q`, or a parameter its entry does not take, is `UNSUPPORTED`; a
    /// value that is malformed, or a required one that is missing, is `VALIDATION_FAILED`
    /// naming the parameter. An empty value counts as absent.
    pub fn parse(params: &HashMap<String, String>) -> Result<Self, AppError> {
        let unsupported = |what: String| AppError::new(ErrorCode::Unsupported, what);
        let name = params.get("q").map_or("", |q| q.trim());
        let kind = AuditQueryKind::parse(name)
            .ok_or_else(|| unsupported("That search is not one the audit offers.".to_owned()))?;
        let mut names: Vec<&String> = params.keys().collect();
        names.sort();
        if let Some(extra) = names
            .into_iter()
            .find(|k| !COMMON_PARAMS.contains(&k.as_str()) && !kind.params().contains(&k.as_str()))
        {
            return Err(unsupported(format!(
                "The {} search does not take {extra}.",
                kind.as_str()
            )));
        }
        let get = |k: &str| params.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let required = |k: &str| get(k).ok_or_else(|| invalid(k, "This search needs it."));
        let ms = |k: &str| -> Result<Option<i64>, AppError> {
            get(k)
                .map(|v| {
                    v.parse::<i64>()
                        .ok()
                        .filter(|n| *n >= 0)
                        .ok_or_else(|| invalid(k, "A time is UTC milliseconds."))
                })
                .transpose()
        };
        let mut search = Self {
            kind,
            actor_kind: None,
            actor_id: None,
            module: None,
            intent_kind: None,
            tbl: None,
            row_id: None,
            from_ms: ms("from")?,
            to_ms: ms("to")?,
            cursor: get("cursor").map(str::to_owned),
        };
        if let (Some(f), Some(t)) = (search.from_ms, search.to_ms)
            && f > t
        {
            return Err(invalid("to", "To must not be before from."));
        }
        match kind {
            AuditQueryKind::Recent => {}
            AuditQueryKind::Actor => {
                let k = required("actor_kind")?;
                if !ACTOR_KINDS.contains(&k) {
                    return Err(invalid(
                        "actor_kind",
                        "Use staff, owner, admin, system or device.",
                    ));
                }
                search.actor_kind = Some(k.to_owned());
                if let Some(id) = get("actor_id") {
                    if id.len() > ID_MAX || id.chars().any(char::is_control) {
                        return Err(invalid("actor_id", "That is not an id."));
                    }
                    search.actor_id = Some(id.to_owned());
                }
            }
            AuditQueryKind::Module => {
                let m = required("module")?;
                if !is_word(m, &[]) {
                    return Err(invalid(
                        "module",
                        "A module is an intent domain, e.g. settings.",
                    ));
                }
                search.module = Some(m.to_owned());
            }
            AuditQueryKind::Intent => {
                let i = required("intent")?;
                if !is_word(i, b".") {
                    return Err(invalid("intent", "An intent is like settings.set."));
                }
                search.intent_kind = Some(i.to_owned());
            }
            AuditQueryKind::Entity => {
                let t = required("tbl")?;
                if !is_word(t, &[]) {
                    return Err(invalid("tbl", "A table is a name like settings."));
                }
                search.tbl = Some(t.to_owned());
                if let Some(id) = get("row_id") {
                    if id.len() > ID_MAX || id.chars().any(char::is_control) {
                        return Err(invalid("row_id", "That is not a row id."));
                    }
                    search.row_id = Some(id.to_owned());
                }
            }
            AuditQueryKind::Time => {
                required("from")?;
                required("to")?;
            }
        }
        Ok(search)
    }
}

/// One page of a search over a built month. `orphaned` lists, on the first page only, the
/// changes a fork point kept out of the branch's state; they match the same filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditSearchPage {
    pub branch_id: String,
    pub month: String,
    /// When the month was built (UTC ms); it is good for [`crate::AUDIT_CACHE_TTL_MS`] after.
    #[ts(type = "number")]
    pub built_at: i64,
    pub items: Vec<AuditItem>,
    pub orphaned: Vec<AuditItem>,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn every_entry_has_a_name_that_parses_back() {
        for k in AuditQueryKind::ALL {
            assert_eq!(AuditQueryKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn an_unknown_entry_or_parameter_is_unsupported() {
        for params in [
            q(&[("month", "2026-10")]),
            q(&[("month", "2026-10"), ("q", "sql")]),
            q(&[("month", "2026-10"), ("q", "recent"), ("where", "1=1")]),
            q(&[
                ("month", "2026-10"),
                ("q", "recent"),
                ("module", "settings"),
            ]),
            q(&[("month", "2026-10"), ("q", "recent"), ("limit", "500")]),
        ] {
            let e = AuditSearch::parse(&params).unwrap_err();
            assert_eq!(e.code, ErrorCode::Unsupported, "{params:?}");
        }
    }

    #[test]
    fn a_search_carries_its_filters() {
        let s = AuditSearch::parse(&q(&[
            ("month", "2026-10"),
            ("q", "actor"),
            ("actor_kind", "staff"),
            ("actor_id", "S1"),
            ("from", "10"),
            ("to", "20"),
            ("cursor", "abc"),
        ]))
        .unwrap();
        assert_eq!(s.kind, AuditQueryKind::Actor);
        assert_eq!(s.actor_kind.as_deref(), Some("staff"));
        assert_eq!(s.actor_id.as_deref(), Some("S1"));
        assert_eq!((s.from_ms, s.to_ms), (Some(10), Some(20)));
        assert_eq!(s.cursor.as_deref(), Some("abc"));
    }

    #[test]
    fn a_missing_or_malformed_value_is_a_validation_error() {
        for (params, field) in [
            (q(&[("q", "actor")]), "actor_kind"),
            (q(&[("q", "actor"), ("actor_kind", "player")]), "actor_kind"),
            (q(&[("q", "module")]), "module"),
            (q(&[("q", "module"), ("module", "Money; DROP")]), "module"),
            (q(&[("q", "intent"), ("intent", "a b")]), "intent"),
            (q(&[("q", "entity")]), "tbl"),
            (q(&[("q", "time"), ("from", "1")]), "to"),
            (q(&[("q", "recent"), ("from", "soon")]), "from"),
            (q(&[("q", "recent"), ("from", "5"), ("to", "4")]), "to"),
        ] {
            let e = AuditSearch::parse(&params).unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{params:?}");
            assert_eq!(e.field.as_deref(), Some(field), "{params:?}");
        }
    }
}

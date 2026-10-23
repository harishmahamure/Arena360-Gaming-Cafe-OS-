//! The Hub UI's read-only audit API over the `changes` log (`GET /ui/audit`,
//! `GET /ui/audit/item`, `GET /ui/audit/facets`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::actor::{ActorKind, ElevationKind, ElevationMode};
use crate::frame::ChangeOpKind;

/// The request header that carries a read-elevation ticket (`POST /ui/elevate` with a
/// `{permission}` item) to the audit endpoints.
pub const ELEVATION_HEADER: &str = "x-arena-elevation";

/// Most items one audit page holds.
pub const AUDIT_MAX_LIMIT: u32 = 100;
/// Items per audit page when the request names no `limit`.
pub const AUDIT_DEFAULT_LIMIT: u32 = 50;

/// An actor kind as the audit shows it: a stored kind this build does not know reads
/// `unknown` (the page still loads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AuditActorKind {
    Staff,
    Owner,
    Admin,
    System,
    Device,
    Unknown,
}

impl From<ActorKind> for AuditActorKind {
    fn from(k: ActorKind) -> Self {
        match k {
            ActorKind::Staff => Self::Staff,
            ActorKind::Owner => Self::Owner,
            ActorKind::Admin => Self::Admin,
            ActorKind::System => Self::System,
            ActorKind::Device => Self::Device,
        }
    }
}

/// How an elevation was proven, as the audit shows it (`unknown` for a stored value this build
/// does not know).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AuditElevationKind {
    Pin,
    Mfa,
    Unknown,
}

impl From<ElevationKind> for AuditElevationKind {
    fn from(k: ElevationKind) -> Self {
        match k {
            ElevationKind::Pin => Self::Pin,
            ElevationKind::Mfa => Self::Mfa,
        }
    }
}

/// Where an elevation was verified, as the audit shows it (`unknown` for a stored value this
/// build does not know).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AuditElevationMode {
    Hub,
    CachedVerifier,
    Unknown,
}

impl From<ElevationMode> for AuditElevationMode {
    fn from(m: ElevationMode) -> Self {
        match m {
            ElevationMode::Hub => Self::Hub,
            ElevationMode::CachedVerifier => Self::CachedVerifier,
        }
    }
}

/// Who made a change. `name` is the staff member's display name or the device's label; for
/// `system`, `owner` and `admin` it is a message-catalogue key (e.g. `hub.audit.actor.system`)
/// the UI translates. An id the branch no longer knows shows the id itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditActor {
    pub kind: AuditActorKind,
    pub id: String,
    pub name: String,
}

/// Who approved an elevated change, and where the PIN was checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditElevatedBy {
    pub kind: AuditElevationKind,
    pub id: String,
    pub name: String,
    pub mode: AuditElevationMode,
}

/// One change row, as the audit list shows it. `module` is the domain part of `intent_kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditItem {
    pub epoch: String,
    #[ts(type = "number")]
    pub seq: i64,
    /// UTC ms.
    #[ts(type = "number")]
    pub at: i64,
    pub business_day: Option<String>,
    pub actor: AuditActor,
    pub elevated_by: Option<AuditElevatedBy>,
    pub module: String,
    pub intent_kind: String,
    pub intent_id: String,
    pub tbl: String,
    pub row_id: String,
    pub op: ChangeOpKind,
}

/// One page of `GET /ui/audit`, newest first. `next_cursor` is `null` on the last page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditPage {
    pub items: Vec<AuditItem>,
    pub next_cursor: Option<String>,
}

/// One column value of a row image (MessagePack decoded to JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuditValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

/// A row image: column name to value. Secret columns read `"[redacted]"`.
pub type AuditImage = BTreeMap<String, AuditValue>;

/// `GET /ui/audit/item`: one change with its row images. `before` is the image of the previous
/// change to the same row in log order (`null`: this change created it). For a delete, `after`
/// is the image the row had when it was deleted. `changed` lists the columns that differ
/// between the two (empty for a created row), redacted ones included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditDetail {
    pub item: AuditItem,
    #[ts(type = "Record<string, string | number | null>")]
    pub after: AuditImage,
    #[ts(type = "Record<string, string | number | null> | null")]
    pub before: Option<AuditImage>,
    pub changed: Vec<String>,
}

/// A staff member the audit actor filter offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditStaffOption {
    pub id: String,
    pub name: String,
}

/// `GET /ui/audit/facets`: what the audit filters offer: every staff member (by name) and the
/// known modules (intent domains).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditFacets {
    pub staff: Vec<AuditStaffOption>,
    pub modules: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_serialize_as_plain_json() {
        let image: AuditImage = [
            ("a".to_owned(), AuditValue::Null),
            ("b".to_owned(), AuditValue::Integer(3)),
            ("c".to_owned(), AuditValue::Text("x".into())),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            serde_json::to_string(&image).unwrap(),
            r#"{"a":null,"b":3,"c":"x"}"#
        );
    }

    #[test]
    fn a_page_always_carries_its_cursor() {
        let page = AuditPage {
            items: vec![],
            next_cursor: None,
        };
        assert_eq!(
            serde_json::to_string(&page).unwrap(),
            r#"{"items":[],"next_cursor":null}"#
        );
    }
}

//! The on-demand audit builder's wire types (Story 8.36, AD-37): a request to rebuild one month of
//! a branch's history in the cloud, and the state of that build. The cloud keeps no standing copy
//! of branch data; a month exists only as `audit-cache/<yyyy-mm>.db.zst` for 24 hours after a
//! build.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ErrorCode;

/// Builds one tenant may ask for in an hour before `RATE_LIMITED`.
pub const AUDIT_BUILDS_PER_HOUR: u64 = 6;
/// How long a built month stays usable (the R2 lifecycle rule on `audit-cache/`), in ms.
pub const AUDIT_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
/// A `building` job whose last progress write is older than this was abandoned by a builder that
/// died; the next builder picks it up again, in ms.
pub const AUDIT_BUILD_STALE_MS: i64 = 15 * 60 * 1000;

/// `POST /v1/branches/{id}/audit/build`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditBuildRequest {
    /// The month to rebuild, `yyyy-mm`.
    pub month: String,
}

/// Where a build job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AuditBuildState {
    Queued,
    Building,
    Ready,
    Failed,
}

impl AuditBuildState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "building" => Self::Building,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// A build job as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AuditBuildStatus {
    pub branch_id: String,
    pub month: String,
    pub state: AuditBuildState,
    /// 0 to 100.
    pub progress: u8,
    /// When a `ready` month was built (ms); the cache is good for [`AUDIT_CACHE_TTL_MS`] after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub built_at: Option<i64>,
    /// `AUDIT_BUILDING` while queued or building, `AUDIT_BUILD_FAILED` when failed, none when
    /// ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<ErrorCode>,
}

impl AuditBuildStatus {
    /// The code a state answers with.
    pub fn code_of(state: AuditBuildState) -> Option<ErrorCode> {
        match state {
            AuditBuildState::Queued | AuditBuildState::Building => Some(ErrorCode::AuditBuilding),
            AuditBuildState::Failed => Some(ErrorCode::AuditBuildFailed),
            AuditBuildState::Ready => None,
        }
    }
}

/// Whether `s` is a month key `yyyy-mm` (2000 to 2999, month 01 to 12).
pub fn is_audit_month(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..].iter().all(u8::is_ascii_digit)
        && matches!(b[0], b'2')
        && matches!(
            &s[5..],
            "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12"
        )
}

/// The first day (`yyyy-mm-dd`) of `month`, the day a snapshot "on or before the month start" is
/// compared with.
pub fn audit_month_start(month: &str) -> Option<String> {
    is_audit_month(month).then(|| format!("{month}-01"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_key_is_year_dash_month() {
        for ok in ["2026-10", "2000-01", "2999-12"] {
            assert!(is_audit_month(ok), "{ok}");
        }
        for bad in [
            "2026-13",
            "2026-00",
            "2026-1",
            "26-10",
            "2026/10",
            "1999-12",
            "2026-10-01",
            "",
            "2026-1a",
            "../2026-1",
        ] {
            assert!(!is_audit_month(bad), "{bad}");
        }
        assert_eq!(audit_month_start("2026-10").as_deref(), Some("2026-10-01"));
        assert_eq!(audit_month_start("nope"), None);
    }

    #[test]
    fn states_round_trip_and_name_their_code() {
        for s in [
            AuditBuildState::Queued,
            AuditBuildState::Building,
            AuditBuildState::Ready,
            AuditBuildState::Failed,
        ] {
            assert_eq!(AuditBuildState::parse(s.as_str()), Some(s));
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!("\"{}\"", s.as_str())
            );
        }
        assert_eq!(AuditBuildState::parse("gone"), None);
        assert_eq!(
            AuditBuildStatus::code_of(AuditBuildState::Building),
            Some(ErrorCode::AuditBuilding)
        );
        assert_eq!(
            AuditBuildStatus::code_of(AuditBuildState::Failed),
            Some(ErrorCode::AuditBuildFailed)
        );
        assert_eq!(AuditBuildStatus::code_of(AuditBuildState::Ready), None);
    }
}

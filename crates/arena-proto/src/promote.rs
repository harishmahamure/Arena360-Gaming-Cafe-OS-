//! Promoting the standby to main (Story 9.21, AD-11, AD-24).
//!
//! A Manager taps "Promote to main" on the standby screen and enters their PIN in place
//! (`ops.promote`, `POST /ui/standby/promote`). The standby checks that the main Hub's lease has
//! run out, that its schema is not behind the main Hub's, claims a new epoch from the cloud with
//! the fork `{old epoch, last replicated seq}` (provisional when the cloud does not answer in
//! 3 s), re-applies every erasure, opens the writer and announces the epoch. The Hub then records
//! `lease.promote` ([`LeasePromote`]), which also sets `ops.last_promote`.
//!
//! Force promote (Story 9.22): when the lease is still fresh, or the cloud's claim was refused, the
//! refusal carries a [`ForceOffer`] ("Data since the last sync may be lost. Promote anyway?" and
//! the lag). A Manager who types their PIN and acknowledges the data loss sends the request again
//! with `force`; the same steps run without the freshness check and without a confirmed claim, the
//! record says `force: true` and why, and the Hub raises the owner alert `standby_forced`. A
//! standby that is behind the main Hub's schema is never offered force.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The settings key `lease.promote` writes: the last promote that finished.
pub const KEY_OPS_LAST_PROMOTE: &str = "ops.last_promote";
/// Before any promote.
pub const DEFAULT_OPS_LAST_PROMOTE: &str = "";
/// How long the main Hub may go without a renewal before "Promote to main" is allowed, in ms. The
/// standby reads it from the line: the main Hub pings every 10 s while it holds
/// its lease.
pub const MAIN_LEASE_STALE_MS: i64 = 15_000;
/// How long the epoch claim waits for the cloud before the standby promotes on a provisional
/// epoch, in ms.
pub const PROMOTE_CLAIM_WAIT_MS: u64 = 3_000;
/// The promote budget, PIN confirmation to the first accepted intent, in ms (NFR8).
pub const PROMOTE_BUDGET_MS: u64 = 10_000;
/// What `LEASE_FRESH` tells the manager.
pub const LEASE_FRESH_MESSAGE: &str = "The main Hub is still running. Check it before promoting.";

/// What the standby screen asks before a forced promote (Story 9.22).
pub const FORCE_PROMOTE_MESSAGE: &str = "Data since the last sync may be lost. Promote anyway?";
/// The reason a forced promote records when no check stood in the way.
pub const FORCE_REASON_PLAIN: &str = "FORCED";

const MAX_TEXT_CHARS: usize = 100;

/// "12 changes not yet copied": the lag shown beside the force-promote warning.
pub fn lag_text(lag_rows: i64) -> String {
    match lag_rows {
        ..=0 => "All changes copied".to_owned(),
        1 => "1 change not yet copied".to_owned(),
        n => format!("{n} changes not yet copied"),
    }
}

/// Whether the main Hub's lease still stands, given how long ago the standby last heard from it
/// (`None`: it never has, which counts as stale).
pub fn main_lease_fresh(silent_ms: Option<i64>) -> bool {
    silent_ms.is_some_and(|s| s < MAIN_LEASE_STALE_MS)
}

/// The payload of `POST /ui/standby/promote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PromoteRequest {
    /// The PIN that confirms the promote: the signed-in Manager's (or Owner's) own, or another
    /// holder's when a Counter is signed in. The route refuses a request without one.
    #[serde(default)]
    #[ts(optional)]
    pub pin: Option<String>,
    /// Promote although the lease is fresh or the cloud did not confirm the claim (Story 9.22).
    /// Needs `data_loss_acknowledged`.
    #[serde(default)]
    pub force: bool,
    /// The manager has read [`FORCE_PROMOTE_MESSAGE`] and the lag.
    #[serde(default)]
    pub data_loss_acknowledged: bool,
}

/// What a refused promote offers (Story 9.22): the warning and how far behind the standby is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ForceOffer {
    /// [`FORCE_PROMOTE_MESSAGE`].
    pub message: String,
    /// Change rows of the main Hub the standby had not copied.
    #[ts(type = "number")]
    pub lag_rows: i64,
    /// [`lag_text`] of `lag_rows`.
    pub lag_text: String,
}

impl ForceOffer {
    pub fn new(lag_rows: i64) -> Self {
        Self {
            message: FORCE_PROMOTE_MESSAGE.to_owned(),
            lag_rows,
            lag_text: lag_text(lag_rows),
        }
    }
}

/// The body of a refused `POST /ui/standby/promote`: the usual error and, when a forced promote
/// is possible, the offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PromoteRefused {
    #[serde(flatten)]
    pub error: crate::UiError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub force_offer: Option<ForceOffer>,
}

/// Why a promote did not happen, with the force offer when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteFailure {
    pub error: crate::AppError,
    pub force_offer: Option<ForceOffer>,
}

impl From<crate::AppError> for PromoteFailure {
    fn from(error: crate::AppError) -> Self {
        Self {
            error,
            force_offer: None,
        }
    }
}

/// The answer of `POST /ui/standby/promote`: this Hub is the main Hub now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Promoted {
    /// The new epoch, `<counter>:<instance>`.
    pub epoch: String,
    /// The epoch has not been confirmed by the cloud yet; the first heartbeat settles it.
    pub provisional: bool,
    /// Kitchen displays that must be paired again with a fresh code.
    #[ts(type = "number")]
    pub kitchen_displays_to_repair: i64,
}

/// What `lease.promote` records, and (as JSON) the value of `ops.last_promote`: the epochs either
/// side of the promote and how far behind the standby was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LeasePromote {
    /// The epoch of the main Hub the standby followed (empty when it had applied nothing).
    pub old_epoch: String,
    /// The last `seq` of the old epoch the standby had applied: the fork point.
    #[ts(type = "number")]
    pub fork_seq: i64,
    /// The new epoch, `<counter>:<instance>`.
    pub new_epoch: String,
    /// The cloud had not confirmed `new_epoch` when the writer opened.
    pub provisional: bool,
    /// Change rows of the main Hub the standby had not copied.
    #[ts(type = "number")]
    pub lag_rows: i64,
    /// A forced promote (Story 9.22).
    #[serde(default)]
    pub force: bool,
    /// What the force went past: `LEASE_FRESH`, the code of the cloud's refusal (comma-separated)
    /// or [`FORCE_REASON_PLAIN`]. Set exactly when `force` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub force_reason: Option<String>,
    /// Players erased before the promote and erased again on the standby.
    #[ts(type = "number")]
    pub erasures_reapplied: i64,
    /// Kitchen displays that now show "Hub changed — re-pair".
    #[ts(type = "number")]
    pub kitchen_displays_to_repair: i64,
    /// When the writer opened, UTC ms.
    #[ts(type = "number")]
    pub promoted_at: i64,
    /// The Manager who promoted (their display name), set by the Hub when it applies the record.
    #[serde(default)]
    pub by: Option<String>,
}

fn text_ok(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

impl LeasePromote {
    /// Whether the facts could describe a promote this build performs.
    pub fn is_consistent(&self) -> bool {
        text_ok(&self.old_epoch)
            && !self.new_epoch.is_empty()
            && text_ok(&self.new_epoch)
            && self.new_epoch != self.old_epoch
            && self.fork_seq >= 0
            && self.lag_rows >= 0
            && self.erasures_reapplied >= 0
            && self.kitchen_displays_to_repair >= 0
            && self.promoted_at > 0
            && match (&self.force_reason, self.force) {
                (Some(r), true) => !r.is_empty() && text_ok(r),
                (None, false) => true,
                _ => false,
            }
            && self.by.as_ref().is_none_or(|b| !b.is_empty() && text_ok(b))
    }

    /// The value of `ops.last_promote`, or `None` when it is empty or malformed.
    pub fn parse(value: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(value).ok()?;
        parsed.is_consistent().then_some(parsed)
    }

    /// The JSON stored in `ops.last_promote`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> LeasePromote {
        LeasePromote {
            old_epoch: "3:01JMAIN".into(),
            fork_seq: 120,
            new_epoch: "4:01JSTBY".into(),
            provisional: false,
            lag_rows: 2,
            force: false,
            force_reason: None,
            erasures_reapplied: 1,
            kitchen_displays_to_repair: 2,
            promoted_at: 1_800_000_000_000,
            by: None,
        }
    }

    #[test]
    fn the_lease_is_fresh_until_fifteen_seconds_of_silence() {
        assert!(main_lease_fresh(Some(0)));
        assert!(main_lease_fresh(Some(14_999)));
        assert!(!main_lease_fresh(Some(15_000)));
        assert!(!main_lease_fresh(None));
    }

    #[test]
    fn a_record_round_trips_and_checks_itself() {
        let f = LeasePromote {
            by: Some("Asha".into()),
            ..facts()
        };
        assert_eq!(LeasePromote::parse(&f.to_json()), Some(f));
        assert_eq!(LeasePromote::parse(""), None);
        for bad in [
            LeasePromote {
                new_epoch: String::new(),
                ..facts()
            },
            LeasePromote {
                new_epoch: "3:01JMAIN".into(),
                ..facts()
            },
            LeasePromote {
                lag_rows: -1,
                ..facts()
            },
            LeasePromote {
                promoted_at: 0,
                ..facts()
            },
            LeasePromote {
                force: true,
                ..facts()
            },
            LeasePromote {
                force_reason: Some("LEASE_FRESH".into()),
                ..facts()
            },
            LeasePromote {
                force: true,
                force_reason: Some(String::new()),
                ..facts()
            },
            LeasePromote {
                by: Some("a\nb".into()),
                ..facts()
            },
        ] {
            assert_eq!(LeasePromote::parse(&bad.to_json()), None, "{bad:?}");
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(serde_json::from_str::<PromoteRequest>(r#"{"pin":"1","x":1}"#).is_err());
        assert_eq!(
            serde_json::from_str::<PromoteRequest>("{}").unwrap(),
            PromoteRequest {
                pin: None,
                force: false,
                data_loss_acknowledged: false
            }
        );
    }

    #[test]
    fn a_forced_record_names_what_it_went_past() {
        let f = LeasePromote {
            force: true,
            force_reason: Some("LEASE_FRESH".into()),
            ..facts()
        };
        assert!(f.is_consistent());
        assert_eq!(LeasePromote::parse(&f.to_json()), Some(f.clone()));
    }

    #[test]
    fn the_lag_reads_in_words() {
        assert_eq!(lag_text(12), "12 changes not yet copied");
        assert_eq!(lag_text(1), "1 change not yet copied");
        assert_eq!(lag_text(0), "All changes copied");
        let offer = ForceOffer::new(12);
        assert_eq!(offer.message, FORCE_PROMOTE_MESSAGE);
        assert_eq!(offer.lag_text, "12 changes not yet copied");
    }
}

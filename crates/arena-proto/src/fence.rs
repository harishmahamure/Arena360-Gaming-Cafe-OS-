//! The branch fence's claim (Story 9.3, AD-24).
//!
//! The cloud holds one fence epoch per branch and issues every epoch counter by compare-and-set
//! on it. `POST /v1/fence/claim` moves the fence from exactly `expected` to
//! `(expected counter + 1):<instance_id>`; a stale `expected` is refused with `FENCED`, whose
//! message names the current fence ([`fenced_epoch`] reads it back). The heartbeat settles a
//! provisional epoch against the same fence.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Why a Hub claims the fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FenceClaimReason {
    /// A manual promote of a standby.
    Promote,
    /// A restore from a backup or an export.
    Restore,
    /// A lease taken after the previous holder's expired.
    Lease,
    /// A Hub settling the provisional epoch it minted offline.
    Confirm,
}

/// Where the new epoch forks from its parent: the parent's last `seq` the new epoch keeps. Rows of
/// the parent past it are never applied by the new epoch and its commands past it are re-queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct FenceFork {
    pub parent_epoch: String,
    #[ts(type = "number")]
    pub parent_seq: i64,
}

/// A Hub's claim of the next epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct FenceClaimRequest {
    /// The fence epoch the Hub believes is current, exactly as text.
    pub expected: String,
    /// The `hub_instance_id` the new epoch is minted for.
    pub instance_id: String,
    pub reason: FenceClaimReason,
    pub fork: FenceFork,
}

/// The claim won: the fence is now this epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct FenceClaimResponse {
    pub epoch: String,
}

/// The fence epoch a `FENCED` message names: the text in `(epoch <fence>)`.
pub fn fenced_epoch(message: &str) -> Option<&str> {
    let rest = message.split_once("(epoch ")?.1;
    let epoch = rest.split_once(')')?.0.trim();
    (!epoch.is_empty()).then_some(epoch)
}

/// The counter and instance of an epoch `<counter>:<instance>`, or `None` when it is not one.
pub fn epoch_parts(epoch: &str) -> Option<(i64, &str)> {
    let (counter, instance) = epoch.split_once(':')?;
    if instance.is_empty()
        || counter.is_empty()
        || counter.len() > 18
        || !counter.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((counter.parse().ok()?, instance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fence_is_read_out_of_a_fenced_message() {
        assert_eq!(
            fenced_epoch("A newer Hub has taken over this branch (epoch 4:HUBB)."),
            Some("4:HUBB")
        );
        assert_eq!(fenced_epoch("A newer Hub has taken over."), None);
        assert_eq!(fenced_epoch("(epoch )"), None);
    }

    #[test]
    fn an_epoch_splits_into_counter_and_instance() {
        assert_eq!(epoch_parts("12:abc"), Some((12, "abc")));
        for bad in [
            "",
            "x:abc",
            "1:",
            ":a",
            "-1:a",
            "1234567890123456789:a",
            "abc",
        ] {
            assert_eq!(epoch_parts(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_claim_round_trips_with_snake_case_reasons() {
        let req = FenceClaimRequest {
            expected: "3:A".into(),
            instance_id: "B".into(),
            reason: FenceClaimReason::Promote,
            fork: FenceFork {
                parent_epoch: "3:A".into(),
                parent_seq: 40,
            },
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["reason"], "promote");
        assert_eq!(
            serde_json::from_value::<FenceClaimRequest>(json).unwrap(),
            req
        );
    }
}

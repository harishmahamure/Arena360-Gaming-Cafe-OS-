//! The handover of a branch from its Android Hub to a Windows Hub (Story 10.17).
//!
//! A manager confirms the move on the tablet with a PIN and the tablet shows a 6-digit handover
//! code. The Windows Hub (found by address on the same LAN) sends the code to
//! `POST /v1/handover/claim`; the tablet then stops taking intents, lets its writer queue drain
//! and writes a final `.a360` snapshot (`purpose: migrate`). The Windows Hub pulls it from
//! `GET /v1/handover/snapshot` with the transfer token the claim answered, checks it, and says so
//! at `POST /v1/handover/ack`, after which the tablet only shows that the branch has moved.
//!
//! The code never crosses the wire: both sides prove they know it over the TLS channel they
//! share (`handover_proof` in `arena-core`), so a certificate in the middle cannot ask for the
//! snapshot.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ErrorCode;

/// `POST`: the Windows Hub starts the handover with the code.
pub const HANDOVER_CLAIM_PATH: &str = "/v1/handover/claim";
/// `GET`: the Windows Hub pulls the final snapshot (`Authorization: Bearer <transfer token>`).
pub const HANDOVER_SNAPSHOT_PATH: &str = "/v1/handover/snapshot";
/// `POST`: the Windows Hub says it holds a snapshot that checked out.
pub const HANDOVER_ACK_PATH: &str = "/v1/handover/ack";
/// The `Content-Type` of the snapshot body (the `.a360` file bytes).
pub const HANDOVER_SNAPSHOT_TYPE: &str = "application/vnd.arena360.export";
/// The purpose a handover snapshot states in its header.
pub const HANDOVER_PURPOSE: &str = "migrate";
/// Wrong codes a handover code survives; the next attempt invalidates it (as kiosk pairing).
pub const HANDOVER_FAILURES_PER_CODE: u32 = crate::PAIR_FAILURES_PER_CODE;
/// How long a handover code stays good.
pub const HANDOVER_CODE_TTL_MS: i64 = 15 * 60 * 1000;
/// The `hub_meta` key a Windows Hub keeps the received snapshot's facts under (Story 10.18
/// imports it).
pub const HANDOVER_RECEIVED_KEY: &str = "handover_received";
/// The `hub_meta` key an Android Hub keeps its handover state under.
pub const HANDOVER_STATE_KEY: &str = "handover";

/// Where a handover stands, on either Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum HandoverPhase {
    /// Android: the code is on screen; the Hub still serves everything.
    Code,
    /// Android: the Windows Hub claimed the code; intents are refused and the snapshot is made
    /// and pulled.
    Transferring,
    /// Android: the snapshot is in the backup folder for copying to the PC; the Hub stays
    /// stopped until the PC confirms the import or the manager cancels.
    File,
    /// Android: the Windows Hub holds the snapshot; this Hub serves nothing and is read-only.
    Moved,
    /// Windows: the snapshot was pulled (or picked) and checked; Story 10.18 imports it.
    Received,
}

impl HandoverPhase {
    /// Whether an Android Hub in this phase refuses intents.
    pub fn stops_hub(self) -> bool {
        matches!(self, Self::Transferring | Self::File | Self::Moved)
    }
}

/// The handover as the Hub UI shows it (`HubSnapshot.handover`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverView {
    pub phase: HandoverPhase,
    /// The Windows Hub's name (Android), or the Android Hub's branch (Windows).
    pub target_hub_name: String,
    /// The 6-digit code, while `phase` is `code`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
    /// The last change `seq` the snapshot holds, once there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_seq: Option<i64>,
    /// The snapshot file in the backup folder, in phase `file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file_name: Option<String>,
}

/// `POST /ui/handover/start` and `/ui/handover/cancel`: the manager's PIN confirms either.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverPinRequest {
    #[serde(default)]
    #[ts(optional)]
    pub pin: Option<String>,
}

/// `POST /ui/migration/join` on the Windows Hub: the Android Hub's address and the code on its
/// screen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverJoinRequest {
    /// `host` or `host:port` of the Android Hub's LAN listener (`:7401` when no port is given).
    pub address: String,
    pub code: String,
}

/// `POST /v1/handover/claim`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverClaim {
    /// The Windows Hub's name, which must be the one the cloud told the Android Hub.
    pub hub_name: String,
    /// `handover_proof(code, certificate fingerprint the Windows Hub saw, "claim", hub_name)`.
    pub proof: String,
}

/// The answer to a good claim.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverClaimed {
    /// Bearer token for the snapshot and the acknowledgement.
    pub transfer_token: String,
    /// The Android Hub's certificate fingerprint (the Windows Hub checks it is the one it saw).
    pub fingerprint: String,
    /// `handover_proof(code, fingerprint, "reply", transfer_token)`.
    pub proof: String,
    /// The snapshot's facts, for the checks the Windows Hub makes.
    pub snapshot: HandoverSnapshotFacts,
}

/// What the Windows Hub checks the pulled snapshot against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverSnapshotFacts {
    pub branch_id: String,
    pub epoch: String,
    #[ts(type = "number")]
    pub last_seq: i64,
    pub sha256: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// `POST /v1/handover/ack`: the snapshot arrived and its hash, branch and `seq` matched.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverAck {
    pub sha256: String,
    #[ts(type = "number")]
    pub last_seq: i64,
}

/// A refusal on the handover routes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandoverRefused {
    pub code: ErrorCode,
    pub message: String,
    /// The code was invalidated by this attempt and the tablet shows a new one.
    #[serde(default)]
    pub code_replaced: bool,
}

/// The Android Hub's report in a heartbeat once the Windows Hub holds the snapshot
/// (`migration.handed_over`, Story 10.17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HandedOver {
    pub migration_id: String,
    #[ts(type = "number")]
    pub last_seq: i64,
}

/// Whether `code` has the shape of a handover code.
pub fn valid_handover_code(code: &str) -> bool {
    crate::is_pair_code(code)
}

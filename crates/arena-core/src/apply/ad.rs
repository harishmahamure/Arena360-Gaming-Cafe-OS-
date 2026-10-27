//! `ad.request_recorded` and `ad.impression_recorded` (system only): one ad request or ad
//! impression, as one row of `ad_requests` / `ad_impressions` with its slot and the time. The
//! idempotency key is the plugin's event id, so a duplicate event is the same intent and writes
//! nothing (`ttl_dedupe`).
//!
//! The rows are local bookkeeping read by the heartbeat, not branch data: they are outside the
//! change log (`Written` stays empty, so no change record is appended and replay does not
//! rebuild them). They carry no actor, device, creative, advertiser or advertising id. The only
//! thing that learns the slot names is this file; the Hub knows nothing else about ads.

use arena_proto::{AdSlot, AppError, ErrorCode, Intent, IntentPayload};
use arena_store::{AdKind, StoreTx, current_branch_id, insert_ad};

use super::Target;
use super::mutate::Written;

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>) -> Result<Target, AppError> {
    Ok(Target::Branch(current_branch_id(tx)?))
}

fn parts(payload: &IntentPayload) -> Option<(AdKind, AdSlot)> {
    match payload {
        IntentPayload::AdImpressionRecorded(p) => Some((AdKind::Impression, p.slot)),
        IntentPayload::AdRequestRecorded(p) => Some((AdKind::Request, p.slot)),
        _ => None,
    }
}

pub(super) fn validate(intent: &Intent, branch: Option<&String>) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid(
            "base_rev",
            &format!("{} takes no base_rev", intent.kind.as_str()),
        ));
    }
    if branch.is_none() {
        return Err(invalid("branch", "there is no branch to record an ad for"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (kind, slot) = parts(&intent.payload)
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "not an ad intent"))?;
    insert_ad(tx, kind, slot.as_str(), now_ms)?;
    Ok(Vec::new())
}

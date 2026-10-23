//! **ttl_dedupe.** Runs after epoch and before base_rev.
//!
//! 1. **Dedupe.** If `applied_intents` already holds the intent's idempotency key, the stored
//!    outcome is returned unchanged (a stored success as the same [`Applied`], a stored
//!    rejection as the same [`AppError`]) and nothing else is written. If the key was recorded
//!    for a different intent body (its [`fingerprint`] differs), the intent fails
//!    `IDEMPOTENCY_CONFLICT`, which is never recorded. Dedupe is checked before
//!    the TTL, so a retry of a recorded intent always gets its first outcome back, even once
//!    the intent itself is past its TTL (records are kept 30 days, longer than the 7-day TTL).
//! 2. **TTL.** `now - issued_at > INTENT_TTL_MS` fails `EXPIRED`; `issued_at` more than
//!    `MAX_CLOCK_SKEW_MS` in the future fails `VALIDATION_FAILED` on `issued_at`.
//!
//! **Recording.** A success is recorded in the intent's own transaction, after append_changes
//! ([`record_success`]). A deterministic business rejection is recorded by the Writer in a
//! separate short transaction after the rollback ([`record_rejection`]): `STALE`,
//! `ALREADY_BOOTSTRAPPED`, and `VALIDATION_FAILED` on a field that depends only on the
//! payload ([`RECORDABLE_FIELDS`]). Permission denials, `NOT_PRIMARY`, `EXPIRED`, `INTERNAL`,
//! `IDEMPOTENCY_CONFLICT`, frame errors and validation that depends on time or on other rows
//! (`issued_at`, `key`, `kind`) are never recorded.

use arena_proto::{Actor, AppError, ErrorCode, Intent, IntentKind, IntentPayload, ReasonCode};
use arena_store::{
    AppliedIntentRecord, ChangeOp, StoreTx, find_applied_intent, insert_applied_intent,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::frame::{canonical_key, is_ulid};
use super::{Applied, ApplyCtx, ChangeMeta, guards};

/// How old an intent may be: 7 days.
pub const INTENT_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How far in the future `issued_at` may be: 5 minutes of client clock skew.
pub const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1000;

/// `VALIDATION_FAILED` fields whose check depends only on the intent's own payload, so the
/// same body always fails the same way.
pub(crate) const RECORDABLE_FIELDS: &[&str] = &[
    "value",
    "base_rev",
    "currency",
    "tz",
    "name",
    "branch_code",
    "day_start_min",
    "fy_start_month",
    "intent_id",
    "class",
    "label",
    "token_hash",
    "version",
    "display_name",
    "password_hash",
    "pin_hash",
    "reason",
];

/// `VALIDATION_FAILED` field prefixes that depend only on the payload (`perms[i]`).
pub(crate) const RECORDABLE_FIELD_PREFIXES: &[&str] = &["perms["];

/// What the fingerprint covers: the intent's body — everything except its `intent_id`,
/// `issued_at`, idempotency key and `elevated_by`. The elevation is who approved it, not what
/// it asks for: a retry after re-entering a PIN (a new ticket, or another approver) is the same
/// intent. The permission step still checks the elevation on every attempt.
#[derive(Serialize)]
struct FingerprintInput<'a> {
    kind: IntentKind,
    payload: &'a IntentPayload,
    base_rev: Option<i64>,
    actor: &'a Actor,
    /// Left out when unset, so fingerprints of intents without a reason are unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<ReasonCode>,
}

/// SHA-256 (lower-case hex) of the canonical MessagePack encoding of the intent's payload
/// kind, payload, `base_rev`, actor and `reason` (never `elevated_by`).
pub fn fingerprint(intent: &Intent) -> Result<String, AppError> {
    let input = FingerprintInput {
        kind: intent.payload.kind(),
        payload: &intent.payload,
        base_rev: intent.base_rev,
        actor: &intent.actor,
        reason: intent.reason,
    };
    let bytes = rmp_serde::to_vec_named(&input)
        .map_err(|e| internal(format!("encode fingerprint: {e}")))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[derive(Serialize, Deserialize)]
struct StoredChange {
    epoch: String,
    seq: i64,
    tbl: String,
    row_id: String,
    op: String,
}

/// What `applied_intents.outcome` holds, as MessagePack.
#[derive(Serialize, Deserialize)]
enum StoredOutcome {
    Applied {
        intent_id: String,
        changes: Vec<StoredChange>,
    },
    Rejected(AppError),
}

fn internal(message: String) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

fn encode(outcome: &Result<Applied, AppError>) -> Result<Vec<u8>, AppError> {
    let stored = match outcome {
        Ok(a) => StoredOutcome::Applied {
            intent_id: a.intent_id.clone(),
            changes: a
                .changes
                .iter()
                .map(|c| StoredChange {
                    epoch: c.epoch.clone(),
                    seq: c.seq,
                    tbl: c.tbl.clone(),
                    row_id: c.row_id.clone(),
                    op: c.op.as_str().to_owned(),
                })
                .collect(),
        },
        Err(e) => StoredOutcome::Rejected(e.clone()),
    };
    rmp_serde::to_vec_named(&stored).map_err(|e| internal(format!("encode outcome: {e}")))
}

fn decode(bytes: &[u8]) -> Result<Result<Applied, AppError>, AppError> {
    let stored: StoredOutcome =
        rmp_serde::from_slice(bytes).map_err(|e| internal(format!("decode outcome: {e}")))?;
    Ok(match stored {
        StoredOutcome::Applied { intent_id, changes } => Ok(Applied {
            intent_id,
            changes: changes
                .into_iter()
                .map(|c| {
                    Ok(ChangeMeta {
                        op: ChangeOp::parse(&c.op)
                            .ok_or_else(|| internal(format!("stored op {:?}", c.op)))?,
                        epoch: c.epoch,
                        seq: c.seq,
                        tbl: c.tbl,
                        row_id: c.row_id,
                    })
                })
                .collect::<Result<_, AppError>>()?,
        }),
        StoredOutcome::Rejected(e) => Err(e),
    })
}

/// The step. `Ok(Some(outcome))` is a dedupe hit: the caller returns `outcome` as is.
pub(super) fn ttl_dedupe(
    tx: &StoreTx<'_>,
    intent: &Intent,
    now_ms: i64,
) -> Result<Option<Result<Applied, AppError>>, AppError> {
    // The Writer checks the frame before queuing; this keeps a direct `apply()` caller honest.
    if !is_ulid(&intent.idempotency_key) {
        return Err(AppError::with_field(
            ErrorCode::BadFrame,
            "idempotency_key",
            "idempotency_key must be a ULID",
        ));
    }
    if let Some(stored) = find_applied_intent(tx, &canonical_key(&intent.idempotency_key))? {
        if stored.fingerprint != fingerprint(intent)? {
            return Err(AppError::with_field(
                ErrorCode::IdempotencyConflict,
                "idempotency_key",
                "idempotency_key was already used for a different intent",
            ));
        }
        return decode(&stored.outcome).map(Some);
    }
    if now_ms.saturating_sub(intent.issued_at) > INTENT_TTL_MS {
        return Err(AppError::new(
            ErrorCode::Expired,
            "the intent is older than its 7-day TTL",
        ));
    }
    if intent.issued_at.saturating_sub(now_ms) > MAX_CLOCK_SKEW_MS {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "issued_at",
            // The measured skew, so a client can log how far its clock is ahead.
            format!(
                "issued_at is too far in the future ({} ms ahead of the hub clock)",
                intent.issued_at - now_ms
            ),
        ));
    }
    Ok(None)
}

fn record(
    tx: &StoreTx<'_>,
    intent: &Intent,
    outcome: &Result<Applied, AppError>,
    now_ms: i64,
) -> Result<(), AppError> {
    insert_applied_intent(
        tx,
        &AppliedIntentRecord {
            idempotency_key: canonical_key(&intent.idempotency_key),
            intent_id: intent.intent_id.clone(),
            intent_kind: intent.payload.kind().as_str().to_owned(),
            fingerprint: fingerprint(intent)?,
            outcome: encode(outcome)?,
            applied_at: now_ms,
        },
    )?;
    Ok(())
}

/// Records a success inside the intent's own transaction.
pub(super) fn record_success(
    tx: &StoreTx<'_>,
    intent: &Intent,
    applied: &Applied,
    now_ms: i64,
) -> Result<(), AppError> {
    record(tx, intent, &Ok(applied.clone()), now_ms)
}

/// Whether a rejection is deterministic for its body, so a retry must get the same answer
/// back. Permission denials are never recorded: a later grant must take effect.
pub(crate) fn is_recordable(err: &AppError) -> bool {
    match err.code {
        ErrorCode::ValidationFailed => err.field.as_deref().is_some_and(|f| {
            RECORDABLE_FIELDS.contains(&f)
                || RECORDABLE_FIELD_PREFIXES.iter().any(|p| f.starts_with(p))
        }),
        ErrorCode::Stale | ErrorCode::AlreadyBootstrapped => true,
        _ => false,
    }
}

/// Records a deterministic rejection, in its own short transaction after the intent's rollback.
/// Writes nothing (and succeeds) when the error is not recordable, the key is malformed or this
/// writer no longer holds the epoch. An already-recorded key keeps its first outcome.
pub(crate) fn record_rejection(
    tx: &StoreTx<'_>,
    ctx: &ApplyCtx<'_>,
    intent: &Intent,
    err: &AppError,
) -> Result<(), AppError> {
    if !is_recordable(err) || !is_ulid(&intent.idempotency_key) {
        return Ok(());
    }
    if guards::epoch(tx, ctx).is_err() {
        return Ok(());
    }
    record(tx, intent, &Err(err.clone()), ctx.clock.now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_round_trip() {
        let ok = Ok(Applied {
            intent_id: "I".into(),
            changes: vec![ChangeMeta {
                epoch: "1:H".into(),
                seq: 4,
                tbl: "settings".into(),
                row_id: "B/k".into(),
                op: ChangeOp::Update,
            }],
        });
        assert_eq!(decode(&encode(&ok).unwrap()).unwrap(), ok);
        for err in [
            AppError::new(ErrorCode::Stale, "row changed"),
            AppError::with_field(ErrorCode::ValidationFailed, "key", "no such setting"),
        ] {
            let e = Err(err);
            assert_eq!(decode(&encode(&e).unwrap()).unwrap(), e);
        }
        assert_eq!(decode(b"\xc1").unwrap_err().code, ErrorCode::Internal);
    }

    #[test]
    fn fingerprint_covers_the_body_but_not_ids_or_time() {
        use arena_proto::{ActorKind, SettingsSet};
        let base = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::SettingsSet,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: 1,
            base_rev: Some(1),
            actor: Actor {
                kind: ActorKind::Staff,
                id: "a".into(),
            },
            elevated_by: None,
            reason: None,
            payload: IntentPayload::SettingsSet(SettingsSet {
                key: "k".into(),
                value: "v".into(),
            }),
        };
        let fp = fingerprint(&base).unwrap();
        assert_eq!(fp.len(), 64);
        assert!(
            fp.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        let same = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: 2,
            // The envelope kind is not trusted; the payload's kind is.
            kind: IntentKind::BranchBootstrap,
            ..base.clone()
        };
        assert_eq!(fingerprint(&same).unwrap(), fp);
        let mut value = base.clone();
        value.payload = IntentPayload::SettingsSet(SettingsSet {
            key: "k".into(),
            value: "w".into(),
        });
        let mut actor = base.clone();
        actor.actor.id = "b".into();
        let mut rev = base.clone();
        rev.base_rev = Some(2);
        let mut elevated = base.clone();
        elevated.elevated_by = Some(arena_proto::ElevatedBy {
            kind: arena_proto::ElevationKind::Pin,
            id: "m".into(),
            mode: arena_proto::ElevationMode::Hub,
            verifier_version: 1,
        });
        let mut reason = base.clone();
        reason.reason = Some(ReasonCode::Lost);
        for other in [value, actor, rev, reason] {
            assert_ne!(fingerprint(&other).unwrap(), fp);
        }
        // Who approved it is not part of the body.
        assert_eq!(fingerprint(&elevated).unwrap(), fp);
    }

    #[test]
    fn only_deterministic_rejections_are_recordable() {
        let e = |code| AppError::new(code, "x");
        for code in [ErrorCode::Stale, ErrorCode::AlreadyBootstrapped] {
            assert!(is_recordable(&e(code)), "{code:?}");
        }
        for code in [
            ErrorCode::Forbidden,
            ErrorCode::PermissionDenied,
            ErrorCode::NotPrimary,
            ErrorCode::Expired,
            ErrorCode::Internal,
            ErrorCode::BadFrame,
            ErrorCode::IdempotencyConflict,
            ErrorCode::ValidationFailed,
            ErrorCode::PinRequired,
            ErrorCode::ReasonRequired,
        ] {
            assert!(!is_recordable(&e(code)), "{code:?}");
        }
        let vf = |f: &str| AppError::with_field(ErrorCode::ValidationFailed, f, "x");
        for field in RECORDABLE_FIELDS {
            assert!(is_recordable(&vf(field)), "{field}");
        }
        assert!(is_recordable(&vf("perms[3]")));
        // Fields whose answer depends on other rows are never recorded.
        for field in [
            "issued_at",
            "key",
            "kind",
            "name_taken",
            "login",
            "role_id",
            "active",
        ] {
            assert!(!is_recordable(&vf(field)), "{field}");
        }
    }
}

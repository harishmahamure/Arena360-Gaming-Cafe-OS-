//! Messages a running session asks for (Story 8.30): system intents that queue one
//! `notify_outbox` row about a session, or mark a low-balance crossing as over.
//!
//! * `notify.requested` (kind `low_balance`, `session_ending` or `console_5min`) is refused unless
//!   the branch is on a paid tier with that message switched on, the session is running for the
//!   named profiled player, and the player's latest transactional consent is granted: a guest, a
//!   player who did not agree, a Free branch or a switched-off message writes no row. A session
//!   has one `session_ending` and one `console_5min` row for good (an Extend never repeats it) and
//!   a wallet one `low_balance` row per crossing. The row is due at the end of the Undo window.
//! * `notify.rearm` marks the latest `low_balance` row as left behind by a wallet that rose above
//!   the level again, so the next fall below it queues a new row.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, Intent, IntentPayload, KEY_LICENCE_TIER,
    NOTIFY_KIND_LOW_BALANCE, NOTIFY_PARAM_MAX_CHARS, NOTIFY_PURPOSE_TRANSACTIONAL, NotifyRequested,
    SESSION_NOTIFY_KINDS, UNDO_WINDOW_MS, WhatsappRefusal, whatsapp_refusal,
};
use arena_store::{
    ChangeOp, LiveSession, NewNotify, RowKey, StoreTx, find_notify, find_session, find_setting,
    insert_notify, latest_consent, latest_low_balance, rearm_notify, session_notice_exists,
};

use super::mutate::Written;
use super::session::invalid;
use crate::notify_trigger::{param_names, switch_key};

/// What a `notify.requested` or `notify.rearm` is judged against.
#[derive(Default)]
pub(crate) struct Ask {
    tier: Option<String>,
    switch_on: bool,
    consent: Option<(bool, String)>,
    session: Option<LiveSession>,
    /// A row of this message already exists for the session (or the crossing).
    already: bool,
    /// For a rearm: the row, if it is a `low_balance` row not yet rearmed.
    rearmable: Option<bool>,
}

pub(super) fn load(tx: &StoreTx<'_>, payload: &IntentPayload) -> Result<Ask, AppError> {
    match payload {
        IntentPayload::NotifyRequested(p) => {
            let Some(key) = switch_key(&p.kind) else {
                return Ok(Ask::default());
            };
            let already = if p.kind == NOTIFY_KIND_LOW_BALANCE {
                latest_low_balance(tx, &p.player_id)?.is_some_and(|(_, rearmed)| rearmed.is_none())
            } else {
                session_notice_exists(tx, &p.session_id, &p.kind)?
            };
            Ok(Ask {
                tier: find_setting(tx, KEY_LICENCE_TIER)?.map(|s| s.value),
                switch_on: find_setting(tx, key)?.is_some_and(|s| s.value == "true"),
                consent: latest_consent(tx, &p.player_id, CONSENT_TRANSACTIONAL)?,
                session: find_session(tx, &p.session_id)?,
                already,
                rearmable: None,
            })
        }
        IntentPayload::NotifyRearm(p) => Ok(Ask {
            rearmable: Some(
                find_notify(tx, &p.outbox_id)?
                    .is_some_and(|r| r.kind == NOTIFY_KIND_LOW_BALANCE && r.rearmed_at.is_none()),
            ),
            ..Ask::default()
        }),
        _ => Ok(Ask::default()),
    }
}

fn params_ok(p: &NotifyRequested) -> Result<(), AppError> {
    let names = param_names(&p.kind);
    if p.params.len() != names.len() || !names.iter().all(|n| p.params.contains_key(*n)) {
        return Err(invalid("params", "these parameters do not fit the message"));
    }
    if p.params
        .values()
        .any(|v| v.trim().is_empty() || v.chars().count() > NOTIFY_PARAM_MAX_CHARS)
    {
        return Err(invalid("params", "a parameter is empty or too long"));
    }
    Ok(())
}

fn running(s: &LiveSession) -> bool {
    matches!(s.state.as_str(), "active" | "paused" | "grace")
}

pub(super) fn validate(intent: &Intent, a: &Ask) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "notification intents take no base_rev"));
    }
    let p = match &intent.payload {
        IntentPayload::NotifyRequested(p) => p,
        IntentPayload::NotifyRearm(_) => {
            return if a.rearmable == Some(true) {
                Ok(())
            } else {
                Err(AppError::with_field(
                    ErrorCode::Stale,
                    "outbox_id",
                    "this crossing is not waiting to be rearmed",
                ))
            };
        }
        _ => {
            return Err(AppError::new(
                ErrorCode::Internal,
                "the loaded target does not match the payload",
            ));
        }
    };
    if !SESSION_NOTIFY_KINDS.contains(&p.kind.as_str()) {
        return Err(invalid("kind", "this message is not about a session"));
    }
    params_ok(p)?;
    let session =
        a.session.as_ref().filter(|s| running(s)).ok_or_else(|| {
            AppError::with_field(ErrorCode::Stale, "session_id", "no such session")
        })?;
    if session.player_id.as_deref() != Some(p.player_id.as_str()) {
        return Err(invalid("player_id", "this session is not the player's"));
    }
    if let Some(refusal) = whatsapp_refusal(
        a.tier.as_deref(),
        a.switch_on,
        true,
        a.consent.as_ref().map(|c| c.0),
    ) {
        return Err(match refusal {
            WhatsappRefusal::NotEntitled => AppError::with_field(
                ErrorCode::FeatureNotEntitled,
                "kind",
                "player messages are on the paid tiers",
            ),
            WhatsappRefusal::ConsentRequired => AppError::with_field(
                ErrorCode::ConsentRequired,
                "player_id",
                "the player did not agree to service messages",
            ),
            WhatsappRefusal::SwitchedOff => {
                invalid("kind", "this message is switched off for this branch")
            }
        });
    }
    if a.already {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "kind",
            "this message was already queued",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    a: &Ask,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    match &intent.payload {
        IntentPayload::NotifyRequested(p) => {
            let Some((_, policy_version)) = &a.consent else {
                return Err(AppError::new(
                    ErrorCode::Internal,
                    "a queued message lost its consent",
                ));
            };
            let id = ulid::Ulid::generate().to_string();
            let params: BTreeMap<&str, &str> = p
                .params
                .iter()
                .map(|(k, v)| (k.as_str(), v.trim()))
                .collect();
            let params = serde_json::to_string(&params).map_err(|e| {
                AppError::new(ErrorCode::Internal, format!("message parameters: {e}"))
            })?;
            insert_notify(
                tx,
                &NewNotify {
                    id: &id,
                    purpose: NOTIFY_PURPOSE_TRANSACTIONAL,
                    kind: &p.kind,
                    order_id: None,
                    session_id: Some(&p.session_id),
                    player_id: &p.player_id,
                    consent_policy_version: policy_version,
                    not_before: now_ms + UNDO_WINDOW_MS,
                    params: Some(&params),
                },
            )?;
            Ok(vec![Written {
                key: RowKey::NotifyOutbox { id },
                op: ChangeOp::Insert,
                before: None,
            }])
        }
        IntentPayload::NotifyRearm(p) => {
            rearm_notify(tx, &p.outbox_id, now_ms)?;
            Ok(vec![Written {
                key: RowKey::NotifyOutbox {
                    id: p.outbox_id.clone(),
                },
                op: ChangeOp::Update,
                before: None,
            }])
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

/// The console message needs a TV name and the PC message a minutes count: the names a kind
/// takes are in [`param_names`], the kinds it may be raised for in
/// [`SESSION_NOTIFY_KINDS`].
#[cfg(test)]
mod kinds {
    use super::*;
    use arena_proto::{NOTIFY_KIND_CONSOLE_5MIN, NOTIFY_KIND_SESSION_ENDING};

    #[test]
    fn every_session_kind_has_a_switch_and_parameters() {
        for kind in SESSION_NOTIFY_KINDS {
            assert!(switch_key(kind).is_some(), "{kind}");
            assert!(!param_names(kind).is_empty(), "{kind}");
        }
        assert_eq!(param_names(NOTIFY_KIND_SESSION_ENDING), ["minutes"]);
        assert_eq!(param_names(NOTIFY_KIND_CONSOLE_5MIN), ["console"]);
        assert_eq!(param_names(NOTIFY_KIND_LOW_BALANCE), ["amount_minor"]);
    }
}

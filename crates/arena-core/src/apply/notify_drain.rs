//! The results of the Hub's notification drain (Story 8.29): system intents that close one queued
//! `notify_outbox` row.
//!
//! * `notify.sent` (the cloud sent the message, with the channel) records the irreversible effect
//!   "notification sent" against the row's order, so the sale it is about can no longer be undone.
//! * `notify.failed` (the cloud could not send it) keeps the code. The sale is untouched.
//! * `notify.skip` and `notify.expire` mark a row that will never be sent, with the reason.
//!
//! Each applies to a `queued` row only: a repeat of a result is `STALE` and changes nothing.

use arena_proto::{
    AppError, EffectKind, ErrorCode, Intent, IntentPayload, NOTIFY_STATE_EXPIRED,
    NOTIFY_STATE_FAILED, NOTIFY_STATE_SENT, NOTIFY_STATE_SKIPPED, NotifyChannel,
};
use arena_store::{
    ChangeOp, NotifyRow, RowKey, StoreTx, find_notify, finish_notify, insert_effect,
};

use super::mutate::Written;
use super::session::invalid;

/// The most characters a failure code or a skip reason keeps.
const NOTE_MAX_CHARS: usize = 100;

#[derive(Default)]
pub(crate) struct NotifyTarget {
    row: Option<NotifyRow>,
}

pub(super) fn load(tx: &StoreTx<'_>, payload: &IntentPayload) -> Result<NotifyTarget, AppError> {
    let id = match payload {
        IntentPayload::NotifySent(p) => &p.outbox_id,
        IntentPayload::NotifyFailed(p) => &p.outbox_id,
        IntentPayload::NotifySkip(p) => &p.outbox_id,
        IntentPayload::NotifyExpire(p) => &p.outbox_id,
        _ => return Ok(NotifyTarget::default()),
    };
    Ok(NotifyTarget {
        row: find_notify(tx, id)?,
    })
}

fn note(field: &str, text: &str) -> Result<(), AppError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > NOTE_MAX_CHARS {
        return Err(invalid(field, "this note is 1 to 100 characters long"));
    }
    Ok(())
}

fn queued(t: &NotifyTarget) -> Result<&NotifyRow, AppError> {
    let Some(row) = &t.row else {
        return Err(AppError::with_field(
            ErrorCode::NotFound,
            "outbox_id",
            "no such notification",
        ));
    };
    if row.state != arena_proto::NOTIFY_STATE_QUEUED {
        return Err(AppError::with_field(
            ErrorCode::Stale,
            "outbox_id",
            "this notification has already been settled",
        ));
    }
    Ok(row)
}

pub(super) fn validate(intent: &Intent, t: &NotifyTarget) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid("base_rev", "notification intents take no base_rev"));
    }
    queued(t)?;
    match &intent.payload {
        IntentPayload::NotifySent(_) | IntentPayload::NotifyExpire(_) => Ok(()),
        IntentPayload::NotifyFailed(p) => note("code", &p.code),
        IntentPayload::NotifySkip(p) => note("reason", &p.reason),
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &NotifyTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let row = queued(t)?;
    let (state, channel, code, reason) = match &intent.payload {
        IntentPayload::NotifySent(p) => (
            NOTIFY_STATE_SENT,
            Some(match p.channel {
                NotifyChannel::Whatsapp => "whatsapp",
                NotifyChannel::Sms => "sms",
            }),
            None,
            None,
        ),
        IntentPayload::NotifyFailed(p) => (NOTIFY_STATE_FAILED, None, Some(p.code.trim()), None),
        IntentPayload::NotifySkip(p) => (NOTIFY_STATE_SKIPPED, None, None, Some(p.reason.trim())),
        IntentPayload::NotifyExpire(_) => (NOTIFY_STATE_EXPIRED, None, None, None),
        _ => {
            return Err(AppError::new(
                ErrorCode::Internal,
                "the loaded target does not match the payload",
            ));
        }
    };
    finish_notify(tx, &row.id, state, channel, code, reason)?;
    if state == NOTIFY_STATE_SENT {
        insert_effect(
            tx,
            &ulid::Ulid::generate().to_string(),
            EffectKind::NotificationSent,
            Some(&intent.intent_id),
            Some(&row.order_id),
            now_ms,
        )?;
    }
    Ok(vec![Written {
        key: RowKey::NotifyOutbox { id: row.id.clone() },
        op: ChangeOp::Update,
        before: None,
    }])
}

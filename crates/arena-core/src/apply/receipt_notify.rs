//! WhatsApp receipts through the notification outbox (Story 5.19).
//!
//! * A settle whose receipt choice is `whatsapp` ([`queue_on_settle`]) checks, inside its own
//!   transaction, that the branch is on a paid tier with the message switched on and that the sale
//!   is for a profiled player whose latest transactional consent is granted. It then writes the
//!   `notify_outbox` row of purpose `transactional`, kind `receipt`, with
//!   `not_before` at the end of the Undo window. The row names the player, never the phone number.
//!   Nothing here sends a message and no invoice number is issued: the Undo-window task numbers the
//!   sale and the Epic 8 drain sends once the number exists.
//! * An Undo of the settle ([`skip_on_revert`]) marks the queued row `skipped`, so it is never sent.

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, Intent, IntentPayload, KEY_LICENCE_TIER,
    KEY_NOTIFY_WHATSAPP_RECEIPT, NOTIFY_KIND_RECEIPT, NOTIFY_PURPOSE_TRANSACTIONAL, ReceiptChoice,
    UNDO_WINDOW_MS, WhatsappRefusal, whatsapp_refusal,
};
use arena_store::{
    ChangeOp, NewNotify, RowKey, StoreTx, find_setting, insert_notify, latest_consent,
    order_player_id, queued_notify_ids, skip_notify,
};

use super::mutate::Written;
use super::session::invalid;

/// Whether the intent settles a sale with the WhatsApp receipt choice.
fn wants_whatsapp(intent: &Intent) -> bool {
    let choice = match &intent.payload {
        IntentPayload::SessionSettle(s) => s.receipt,
        IntentPayload::PosSale(s) => s.receipt,
        IntentPayload::PlanSell(s) => s.receipt,
        _ => None,
    };
    choice == Some(ReceiptChoice::Whatsapp)
}

fn switch_on(tx: &StoreTx<'_>) -> Result<bool, AppError> {
    Ok(find_setting(tx, KEY_NOTIFY_WHATSAPP_RECEIPT)?.is_some_and(|s| s.value == "true"))
}

/// The refusal error of a WhatsApp choice, `None` when it is allowed.
fn refusal_error(refusal: WhatsappRefusal) -> AppError {
    match refusal {
        WhatsappRefusal::NotEntitled => AppError::with_field(
            ErrorCode::FeatureNotEntitled,
            "receipt",
            "WhatsApp receipts are on the paid tiers",
        ),
        WhatsappRefusal::ConsentRequired => AppError::with_field(
            ErrorCode::ConsentRequired,
            "receipt",
            "a WhatsApp receipt needs a player who agreed to service messages",
        ),
        WhatsappRefusal::SwitchedOff => invalid(
            "receipt",
            "WhatsApp receipts are switched off for this branch",
        ),
    }
}

/// The WhatsApp step of a settle: writes the `notify_outbox` row of the sale in `order_id`, or
/// refuses the settle (the whole transaction rolls back) when the choice is not allowed. A settle
/// with any other choice, and a bill with no total (it takes no invoice number, so there is no
/// receipt to send), is left as it is.
pub(super) fn queue_on_settle(
    tx: &StoreTx<'_>,
    intent: &Intent,
    order_id: &str,
    total_minor: i64,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    if !wants_whatsapp(intent) {
        return Ok(());
    }
    let tier = find_setting(tx, KEY_LICENCE_TIER)?.map(|s| s.value);
    let player = order_player_id(tx, order_id)?;
    let consent = match &player {
        Some(p) => latest_consent(tx, p, CONSENT_TRANSACTIONAL)?,
        None => None,
    };
    if let Some(refusal) = whatsapp_refusal(
        tier.as_deref(),
        switch_on(tx)?,
        player.is_some(),
        consent.as_ref().map(|c| c.0),
    ) {
        return Err(refusal_error(refusal));
    }
    if total_minor <= 0 {
        return Ok(());
    }
    let (Some(player_id), Some((_, policy_version))) = (player, consent) else {
        return Err(AppError::new(
            ErrorCode::Internal,
            "a WhatsApp settle lost its player",
        ));
    };
    let id = ulid::Ulid::generate().to_string();
    insert_notify(
        tx,
        &NewNotify {
            id: &id,
            purpose: NOTIFY_PURPOSE_TRANSACTIONAL,
            kind: NOTIFY_KIND_RECEIPT,
            order_id: Some(order_id),
            session_id: None,
            player_id: &player_id,
            consent_policy_version: &policy_version,
            not_before: now_ms + UNDO_WINDOW_MS,
            params: None,
            reminder_day: None,
        },
    )?;
    written.push(Written {
        key: RowKey::NotifyOutbox { id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(())
}

/// The Undo step of a settle: every message of the order still waiting is marked `skipped`.
pub(super) fn skip_on_revert(
    tx: &StoreTx<'_>,
    order_id: &str,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    for id in queued_notify_ids(tx, order_id)? {
        if skip_notify(tx, &id)? {
            written.push(Written {
                key: RowKey::NotifyOutbox { id },
                op: ChangeOp::Update,
                before: None,
            });
        }
    }
    Ok(())
}

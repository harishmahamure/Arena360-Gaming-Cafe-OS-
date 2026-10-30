//! Owner alerts raised by the Hub (Story 8.34): the shift close, the low-stock crossing, the credit
//! auto-pause, the refund and the manual discount each queue one `notify_outbox` row of purpose
//! `owner_alert` in the transaction of the event. The drain posts it to `POST /v1/notify`, where the
//! cloud raises the alert under the row's id.
//!
//! * Only a branch on a paid tier queues anything; a Free branch queues nothing.
//! * The row is due at the end of the Undo window, so an Undo of the event can still skip it
//!   (`receipt_notify::skip_on_revert`, through the row's `order_id`).
//! * `(kind, source_ref)` is unique, so an event raises its alert once: an intent replay or a
//!   second hook call finds the row and queues nothing.
//! * Text parameters are cut to [`OWNER_ALERT_PARAM_MAX_CHARS`] characters; money is stored raw
//!   as `amount_minor` and formatted by the drain. No row names a player but a `credit_overdue`.
//! * The limits come from the setting `owner_alert.thresholds`; without a readable row the
//!   defaults apply.

use std::collections::BTreeMap;

use arena_proto::{
    AppError, ErrorCode, KEY_LICENCE_TIER, KEY_OWNER_ALERT_THRESHOLDS, NOTIFY_PURPOSE_OWNER_ALERT,
    OWNER_ALERT_PARAM_MAX_CHARS, OwnerAlertKind, OwnerAlertThresholds, UNDO_WINDOW_MS,
    tier_is_paid,
};
use arena_store::{
    ChangeOp, NewNotify, RowKey, StoreTx, find_setting, insert_notify, owner_alert_exists,
};

use super::mutate::Written;

/// The consent policy version of an owner alert: the owner, not a player, is the audience.
pub(crate) const OWNER_AUDIENCE: &str = "owner";

/// An alert to queue.
pub(super) struct Alert<'a> {
    pub(super) kind: OwnerAlertKind,
    /// The event it is raised for: a shift, a Hub alert, `<player>:<day>`, a refund or an order.
    pub(super) source_ref: &'a str,
    /// The order whose Undo skips the alert.
    pub(super) order_id: Option<&'a str>,
    /// The player a `credit_overdue` is about; empty for every other kind.
    pub(super) player_id: &'a str,
    pub(super) params: BTreeMap<&'static str, String>,
}

/// The branch's limits: the setting's value, or the defaults when it is missing or unreadable.
pub(super) fn thresholds(tx: &StoreTx<'_>) -> Result<OwnerAlertThresholds, AppError> {
    Ok(find_setting(tx, KEY_OWNER_ALERT_THRESHOLDS)?
        .and_then(|s| OwnerAlertThresholds::parse(&s.value))
        .unwrap_or_default())
}

/// `text` as an alert parameter: control characters become spaces, the ends are trimmed and the
/// whole is cut to [`OWNER_ALERT_PARAM_MAX_CHARS`] characters. Empty reads as a dash.
pub(super) fn text_param(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cut: String = cleaned
        .trim()
        .chars()
        .take(OWNER_ALERT_PARAM_MAX_CHARS)
        .collect();
    let cut = cut.trim_end().to_owned();
    if cut.is_empty() { "-".to_owned() } else { cut }
}

/// Queues `alert` for the end of the Undo window, unless the branch is not on a paid tier or the
/// event already has its alert. Pushes the new row onto `written`.
pub(super) fn queue(
    tx: &StoreTx<'_>,
    alert: &Alert<'_>,
    now_ms: i64,
    written: &mut Vec<Written>,
) -> Result<(), AppError> {
    let paid = find_setting(tx, KEY_LICENCE_TIER)?.is_some_and(|s| tier_is_paid(&s.value));
    let kind = alert.kind.as_str();
    if !paid || owner_alert_exists(tx, kind, alert.source_ref)? {
        return Ok(());
    }
    let params = serde_json::to_string(&alert.params)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("alert parameters: {e}")))?;
    let id = ulid::Ulid::generate().to_string();
    insert_notify(
        tx,
        &NewNotify {
            id: &id,
            purpose: NOTIFY_PURPOSE_OWNER_ALERT,
            kind,
            order_id: alert.order_id,
            session_id: None,
            player_id: alert.player_id,
            consent_policy_version: OWNER_AUDIENCE,
            not_before: now_ms + UNDO_WINDOW_MS,
            params: Some(&params),
            reminder_day: None,
            source_ref: Some(alert.source_ref),
        },
    )?;
    written.push(Written {
        key: RowKey::NotifyOutbox { id },
        op: ChangeOp::Insert,
        before: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_parameter_is_clean_and_at_most_forty_characters() {
        assert_eq!(text_param("  Asha \n Rao "), "Asha   Rao");
        assert_eq!(text_param(""), "-");
        assert_eq!(text_param(" \t "), "-");
        let long = "é".repeat(60);
        assert_eq!(text_param(&long).chars().count(), 40);
    }
}

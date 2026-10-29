//! Kitchen displays (Story 7.6): `kds.pair`, `kds.revoke`, `kds.seen` and the high-contrast
//! setting of a display.
//!
//! * `kds.pair` (system actor; tier-gated `New`) inserts a `kitchen_displays` row holding the
//!   Argon2id hash of the token secret the Hub's pairing route computed outside the transaction.
//!   The writer mints the id. At most [`KDS_MAX_DISPLAYS`] displays are not revoked
//!   (`DEVICE_LIMIT`); displays are not `devices`, so they never count against the device cap.
//! * `kds.revoke` (`device.manage`) sets `revoked_at`; the LAN layer then closes the display's
//!   socket `TOKEN_REVOKED`. A revoked display frees its slot at once.
//! * `kds.seen` (system actor) sets `last_seen`; the LAN layer submits it when a display connects
//!   or its last-seen is more than 60 s old.
//! * `settings.set` of `display.hc.<display id>` writes `kitchen_displays.high_contrast` and the
//!   display's change row instead of a `settings` row (none is seeded for a display).
//!
//! The token hash is never returned to a client and never logged.

use arena_proto::{
    AppError, ErrorCode, Intent, IntentPayload, KDS_LIMIT_MESSAGE, KDS_MAX_DISPLAYS,
    KDS_SEEN_INTERVAL_MS, SettingsSet, is_display_name, parse_hc_key,
};
use arena_store::{
    ChangeOp, DisplayRow, NewDisplay, RowKey, StoreTx, count_live_displays, find_display,
    find_staff, insert_display, set_display_high_contrast, set_display_last_seen,
    set_display_revoked,
};

use super::mutate::Written;
use crate::devices::is_argon2id_hash;
use crate::settings_defaults::HC_RULE;

#[derive(Default)]
pub(crate) struct KdsTarget {
    display: Option<DisplayRow>,
    /// The displays that are not revoked.
    live: i64,
    /// For `kds.pair`: whether `created_by` is an active staff member.
    creator_ok: bool,
}

fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

/// Whether `settings.set` targets the high-contrast key of a kitchen display; the display, if so.
pub(super) fn hc_target(tx: &StoreTx<'_>, s: &SettingsSet) -> Result<Option<KdsTarget>, AppError> {
    let Some(arena_proto::HcTarget::Device(id)) = parse_hc_key(&s.key) else {
        return Ok(None);
    };
    Ok(find_display(tx, id)?.map(|d| KdsTarget {
        display: Some(d),
        ..KdsTarget::default()
    }))
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<KdsTarget, AppError> {
    Ok(match p {
        IntentPayload::KdsPair(k) => KdsTarget {
            live: count_live_displays(tx)?,
            creator_ok: find_staff(tx, &k.created_by)?.is_some_and(|s| s.active),
            ..KdsTarget::default()
        },
        IntentPayload::KdsRevoke(r) => KdsTarget {
            display: find_display(tx, &r.display_id)?,
            ..KdsTarget::default()
        },
        IntentPayload::KdsSeen(r) => KdsTarget {
            display: find_display(tx, &r.display_id)?,
            ..KdsTarget::default()
        },
        _ => KdsTarget::default(),
    })
}

/// **base_rev.** A high-contrast change needs the `rev` of the display it was based on; `kds.revoke`
/// may carry one; the other two take none.
pub(super) fn base_rev(intent: &Intent, t: &KdsTarget) -> Result<(), AppError> {
    let stale = || {
        AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "the kitchen display changed since it was read",
        )
    };
    match &intent.payload {
        IntentPayload::SettingsSet(_) => {
            let Some(base) = intent.base_rev else {
                return Err(invalid("base_rev", "settings.set requires base_rev"));
            };
            match &t.display {
                Some(d) if d.rev != base => Err(stale()),
                _ => Ok(()),
            }
        }
        IntentPayload::KdsRevoke(_) => match (intent.base_rev, &t.display) {
            (Some(base), Some(d)) if base != d.rev => Err(stale()),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn live_display<'a>(t: &'a KdsTarget, field: &str) -> Result<&'a DisplayRow, AppError> {
    t.display
        .as_ref()
        .ok_or_else(|| invalid(field, "no such kitchen display"))
}

pub(super) fn validate(intent: &Intent, t: &KdsTarget) -> Result<(), AppError> {
    match &intent.payload {
        IntentPayload::KdsPair(k) => {
            if intent.base_rev.is_some() {
                return Err(invalid("base_rev", "kds.pair takes no base_rev"));
            }
            if !is_display_name(&k.name) {
                return Err(invalid("name", "the name is 1 to 40 printable characters"));
            }
            if !is_argon2id_hash(&k.token_hash) {
                return Err(invalid(
                    "token_hash",
                    "token_hash must be an argon2id PHC string",
                ));
            }
            if !t.creator_ok {
                return Err(invalid("created_by", "no such staff member"));
            }
            if t.live >= KDS_MAX_DISPLAYS {
                return Err(AppError::new(ErrorCode::DeviceLimit, KDS_LIMIT_MESSAGE));
            }
            Ok(())
        }
        IntentPayload::KdsRevoke(_) => {
            let d = live_display(t, "display_id")?;
            if d.revoked_at.is_some() {
                return Err(invalid("display_id", "the display is already revoked"));
            }
            Ok(())
        }
        IntentPayload::KdsSeen(_) => {
            if intent.base_rev.is_some() {
                return Err(invalid("base_rev", "kds.seen takes no base_rev"));
            }
            let d = live_display(t, "display_id")?;
            if d.revoked_at.is_some() {
                return Err(invalid("display_id", "the display is revoked"));
            }
            // At most one write a minute: every write bumps the row's `rev`, which a Manager's
            // high-contrast or revoke intent is based on, so a reconnect loop must not churn it.
            if d.last_seen
                .is_some_and(|at| intent.issued_at - at < KDS_SEEN_INTERVAL_MS)
            {
                return Err(invalid(
                    "display_id",
                    "the display was seen under a minute ago",
                ));
            }
            Ok(())
        }
        IntentPayload::SettingsSet(s) => {
            let d = live_display(t, "key")?;
            if d.revoked_at.is_some() {
                return Err(invalid("key", "no such setting"));
            }
            if !HC_RULE.accepts(&s.value) {
                return Err(invalid(
                    "value",
                    format!("value is not valid for {}", s.key),
                ));
            }
            Ok(())
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "the loaded target does not match the payload",
        )),
    }
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    t: &KdsTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let update = |id: &str| Written {
        key: RowKey::KitchenDisplays { id: id.to_owned() },
        op: ChangeOp::Update,
        before: None,
    };
    match &intent.payload {
        IntentPayload::KdsPair(k) => {
            let id = ulid::Ulid::generate().to_string();
            insert_display(
                tx,
                &NewDisplay {
                    id: &id,
                    name: k.name.trim(),
                    token_hash: &k.token_hash,
                    created_by: Some(&k.created_by),
                },
            )?;
            Ok(vec![Written {
                key: RowKey::KitchenDisplays { id },
                op: ChangeOp::Insert,
                before: None,
            }])
        }
        IntentPayload::KdsRevoke(_) => {
            let d = live_display(t, "display_id")?;
            set_display_revoked(tx, &d.id, now_ms)?;
            Ok(vec![update(&d.id)])
        }
        IntentPayload::KdsSeen(_) => {
            let d = live_display(t, "display_id")?;
            set_display_last_seen(tx, &d.id, now_ms)?;
            Ok(vec![update(&d.id)])
        }
        IntentPayload::SettingsSet(s) => {
            let d = live_display(t, "key")?;
            set_display_high_contrast(tx, &d.id, s.value == "true")?;
            Ok(vec![update(&d.id)])
        }
        _ => Err(AppError::new(
            ErrorCode::Internal,
            "a kitchen display intent reached mutate without its target",
        )),
    }
}

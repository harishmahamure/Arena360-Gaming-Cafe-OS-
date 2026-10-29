//! What the kiosk's `extend-options` panel shows (Story 6.21): +30 min, +1 h and +2 h priced at the
//! live rate, and the ways the player can pay for them.
//!
//! Read from one snapshot, by device: the kiosk names no player and no session, the Hub finds
//! both from the session running on the paired PC. Only a profiled player on a running pack or
//! wallet session can extend; a postpaid session, a pass or block, or a guest cannot, and is
//! refused here rather than shown a panel that could never apply.
//!
//! The price is [`crate::pricing::price_of`] at the rate [`crate::attract::live_rate`] resolves,
//! the same figures the `session.extend` rule charges, so the quote and the charge cannot drift.

use arena_proto::{
    AppError, EXTEND_OPTIONS_MIN, ErrorCode, ExtendPackSource, ExtendQuote, PlayerExtendOptions,
};
use arena_store::Store;
use chrono::TimeZone;

use crate::locale::{locale_rows, resolve};

/// The live session on `device_id`, when the player on it may extend: a profiled player on a
/// running pack or wallet session.
fn extendable(store: &Store, device_id: &str) -> Result<arena_store::LiveSession, AppError> {
    let invalid = |message: &str| AppError::new(ErrorCode::ValidationFailed, message);
    let session = store
        .live_sessions()?
        .into_iter()
        .find(|s| s.device_id == device_id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no session on this PC"))?;
    if session.player_id.is_none() {
        return Err(invalid("a guest session cannot be extended"));
    }
    if session.mode != "pack" && session.mode != "wallet" {
        return Err(invalid("only a prepaid session can be extended"));
    }
    Ok(session)
}

/// The branch's closing time as it writes the time of day (`11:00 pm`).
fn closing_label(
    branch: Option<arena_store::BranchLocale>,
    settings: &[arena_store::SettingRow],
    now_ms: i64,
) -> Option<String> {
    let ms = crate::session_screen::closing_in(branch.clone(), settings, now_ms)?;
    let at = now_ms.checked_add(i64::try_from(ms).ok()?)?;
    let tz = resolve(&locale_rows(&branch?, settings), now_ms).1.tz;
    let local = tz.timestamp_millis_opt(at).single()?;
    Some(local.format("%-I:%M %P").to_string())
}

/// The extend panel's quotes and sources for the player on `device_id` at `now_ms`.
pub fn extend_options(
    store: &Store,
    device_id: &str,
    now_ms: i64,
) -> Result<PlayerExtendOptions, AppError> {
    let session = extendable(store, device_id)?;
    let Some(player_id) = session.player_id.as_deref() else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "no player on this session",
        ));
    };
    let (device, packs, rates, rules, branch, settings, balance, reserved, credit) = store
        .read_consistent(|s| {
            Ok((
                s.device_meta(device_id)?,
                s.player_packs(player_id)?,
                s.rates()?,
                s.pricing_rules()?,
                s.branch_locale()?,
                s.settings()?,
                s.wallet_balance(player_id)?,
                s.reserved_minor(player_id)?,
                s.credit(player_id)?,
            ))
        })?;
    let group_id = device.and_then(|d| d.group_id);
    let tz = branch
        .as_ref()
        .map(|b| resolve(&locale_rows(b, &settings), now_ms).1.tz);
    let rate = match (&group_id, tz) {
        (Some(g), Some(tz)) => rates.iter().find(|r| &r.group_id == g).map(|base| {
            crate::attract::live_rate(&rules, g, base.price_per_hour_minor, tz, now_ms).0
        }),
        _ => None,
    };
    // No rate, no wallet or credit price: only a pack can pay for time on a PC with no group rate.
    let quotes = EXTEND_OPTIONS_MIN
        .iter()
        .map(|&minutes| ExtendQuote {
            minutes,
            price_minor: rate.map_or(0, |r| crate::pricing::price_of(r, minutes)),
        })
        .collect();
    let mut owned: Vec<_> = packs
        .into_iter()
        .filter(|p| p.kind == "pack" && p.expires_at > now_ms && p.minutes_left.unwrap_or(0) > 0)
        .filter(|p| group_id.as_ref().is_some_and(|g| p.group_ids.contains(g)))
        .filter(|p| session.pack_id.as_deref() != Some(p.id.as_str()))
        .collect();
    owned.sort_by_key(|p| p.expires_at);
    Ok(PlayerExtendOptions {
        quotes,
        wallet_minor: (balance - reserved).max(0),
        packs: owned
            .into_iter()
            .map(|p| ExtendPackSource {
                minutes_left: p.minutes_left.unwrap_or(0),
                pack_id: p.id,
                name: p.name,
            })
            .collect(),
        credit_headroom_minor: credit
            .filter(|c| c.limit_minor > 0 && !c.paused)
            .map(|c| (c.limit_minor - c.outstanding_minor).max(0)),
        closing_label: closing_label(branch, &settings, now_ms),
    })
}

/// The id of the session on `device_id` an extend applies to (the same rule as
/// [`extend_options`]); `session.extend` takes no `base_rev`.
pub fn extend_target(store: &Store, device_id: &str) -> Result<String, AppError> {
    Ok(extendable(store, device_id)?.id)
}

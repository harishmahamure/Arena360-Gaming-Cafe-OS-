//! The plan picker's options (Story 6.9): what a logged-in player can start on one PC.
//!
//! Read from one snapshot. Only options valid for the PC's group are listed: each owned pack
//! and pass, each active block plan whose group list has the group, and the wallet at the group's
//! live rate. An expired pack or pass, or a pass or block whose play window is closed now, is
//! listed but not selectable, with its tag. Postpaid is never offered. Order: packs, passes,
//! blocks, then the wallet.

use arena_proto::{
    AppError, MAX_PLAN_OPTIONS, PlanOption, PlanTag, PlayerChoice, PlayerPlansResult, PlayerWallet,
};
use arena_store::Store;
use chrono_tz::Tz;

use crate::locale::{locale_rows, snapshot_locale};
use crate::windows::PlayWindow;

fn clock(min: i64) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

/// The options `player_id` has on `device_id` at `now_ms`. A PC with no group (or none at all)
/// has none.
pub fn player_options(
    store: &Store,
    player_id: &str,
    device_id: &str,
    now_ms: i64,
) -> Result<PlayerPlansResult, AppError> {
    let (device, packs, plans, rates, rules, branch, settings, balance, reserved) = store
        .read_consistent(|s| {
            Ok((
                s.device_meta(device_id)?,
                s.player_packs(player_id)?,
                s.plans()?,
                s.rates()?,
                s.pricing_rules()?,
                s.branch_locale()?,
                s.settings()?,
                s.wallet_balance(player_id)?,
                s.reserved_minor(player_id)?,
            ))
        })?;
    let none = PlayerPlansResult {
        wallet: None,
        options: Vec::new(),
    };
    let group_id = device
        .filter(|d| d.deleted_at.is_none() && d.state != "maintenance")
        .and_then(|d| d.group_id);
    let (Some(group_id), Some(branch)) = (group_id, branch) else {
        return Ok(none);
    };
    let tz = snapshot_locale(&locale_rows(&branch, &settings), now_ms)
        .tz
        .parse::<Tz>()
        .ok();
    // The instant a window ends when it is open now.
    let open_end = |start: Option<i64>, end: Option<i64>| {
        PlayWindow::new(start, end)
            .zip(tz)
            .and_then(|(w, tz)| w.ends_at(now_ms, tz))
    };
    let label =
        |start: Option<i64>, end: Option<i64>| PlayWindow::new(start, end).map(|w| w.label());

    let mut pack_cards = Vec::new();
    let mut pass_cards = Vec::new();
    for p in packs.iter().filter(|p| p.group_ids.contains(&group_id)) {
        let expired = p.expires_at <= now_ms;
        if p.kind == "pass" {
            let open = open_end(p.window_start_min, p.window_end_min).is_some();
            let tag = if expired {
                Some(PlanTag::Expired)
            } else if !open {
                Some(PlanTag::OutsideWindow)
            } else {
                None
            };
            pass_cards.push(PlanOption {
                choice: PlayerChoice::Pass {
                    pack_id: p.id.clone(),
                },
                name: p.name.clone(),
                selectable: tag.is_none(),
                tag,
                minutes_left: None,
                expires_at: Some(p.expires_at),
                window: label(p.window_start_min, p.window_end_min),
                ends_label: None,
                price_minor: None,
            });
        } else {
            let left = p.minutes_left.unwrap_or(0);
            let expired = expired || left <= 0;
            pack_cards.push(PlanOption {
                choice: PlayerChoice::Pack {
                    pack_id: p.id.clone(),
                },
                name: p.name.clone(),
                selectable: !expired,
                tag: expired.then_some(PlanTag::Expired),
                minutes_left: Some(left.max(0)),
                expires_at: Some(p.expires_at),
                window: None,
                ends_label: None,
                price_minor: None,
            });
        }
    }
    let mut block_cards = Vec::new();
    for (plan, _) in plans
        .iter()
        .filter(|(p, g)| p.kind == "block" && p.active && g.contains(&group_id))
    {
        let open = open_end(plan.window_start_min, plan.window_end_min).is_some();
        block_cards.push(PlanOption {
            choice: PlayerChoice::Block {
                plan_id: plan.plan_id.clone(),
            },
            name: plan.name.clone(),
            selectable: open,
            tag: (!open).then_some(PlanTag::OutsideWindow),
            minutes_left: None,
            expires_at: None,
            window: label(plan.window_start_min, plan.window_end_min),
            ends_label: plan.window_end_min.filter(|_| open).map(clock),
            price_minor: Some(plan.price_minor),
        });
    }
    // Selectable first; the ones that expire soonest lead among them.
    let order = |cards: &mut Vec<PlanOption>| {
        cards.sort_by_key(|c| (!c.selectable, c.expires_at.unwrap_or(i64::MAX)));
    };
    order(&mut pack_cards);
    order(&mut pass_cards);
    block_cards.sort_by_key(|c| !c.selectable);

    let wallet = match (tz, rates.iter().find(|r| r.group_id == group_id)) {
        (Some(tz), Some(base)) => {
            let (rate_minor, rule) =
                crate::attract::live_rate(&rules, &group_id, base.price_per_hour_minor, tz, now_ms);
            Some(PlayerWallet {
                balance_minor: (balance - reserved).max(0),
                rate_minor,
                rule,
            })
        }
        _ => None,
    };
    let mut options = pack_cards;
    options.extend(pass_cards);
    options.extend(block_cards);
    if wallet.is_some() {
        options.push(PlanOption {
            choice: PlayerChoice::Wallet,
            name: String::new(),
            selectable: true,
            tag: None,
            minutes_left: None,
            expires_at: None,
            window: None,
            ends_label: None,
            price_minor: None,
        });
    }
    options.truncate(MAX_PLAN_OPTIONS);
    Ok(PlayerPlansResult { wallet, options })
}

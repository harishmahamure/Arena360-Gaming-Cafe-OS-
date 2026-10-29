//! What the kiosk's account menu reads (Story 6.17): the live session on a PC, what its player
//! holds, and the hash their own lock screen checks.
//!
//! All of it is read from one snapshot, by device: a kiosk names no player and no session, the Hub
//! finds them from the session running on the paired PC.

use arena_proto::{
    ACCOUNT_ORDER_LIMIT, AccountOrder, AccountOrderItem, AccountPack, AccountPackEntry, AppError,
    ErrorCode, PlayerAccountPage, PlayerAccountResult,
};
use arena_store::Store;

use crate::players::{KEK_LEN, open_field};

/// The live session on one PC, as far as the account menu needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSession {
    pub session_id: String,
    pub rev: i64,
    /// The profiled player; `None` for a guest.
    pub player_id: Option<String>,
    /// Pack, pass, wallet or block time: paid for already, so ending it leaves no bill.
    pub prepaid: bool,
}

/// The session running (or held at its end) on `device_id`, if any.
pub fn device_session(store: &Store, device_id: &str) -> Result<Option<DeviceSession>, AppError> {
    Ok(store
        .live_sessions()?
        .into_iter()
        .find(|s| s.device_id == device_id)
        .map(|s| DeviceSession {
            prepaid: ["pack", "pass", "wallet", "block"].contains(&s.mode.as_str()),
            session_id: s.id,
            rev: s.rev,
            player_id: s.player_id,
        }))
}

/// The wallet balance and the unexpired packs and passes `player_id` has for `device_id`'s group.
pub fn player_account(
    store: &Store,
    player_id: &str,
    device_id: &str,
    now_ms: i64,
) -> Result<PlayerAccountResult, AppError> {
    let (device, packs, balance, reserved) = store.read_consistent(|s| {
        Ok((
            s.device_meta(device_id)?,
            s.player_packs(player_id)?,
            s.wallet_balance(player_id)?,
            s.reserved_minor(player_id)?,
        ))
    })?;
    let group_id = device.and_then(|d| d.group_id);
    let mut owned: Vec<AccountPack> = packs
        .into_iter()
        .filter(|p| p.expires_at > now_ms)
        .filter(|p| group_id.as_ref().is_some_and(|g| p.group_ids.contains(g)))
        .filter(|p| p.kind == "pass" || p.minutes_left.unwrap_or(0) > 0)
        .map(|p| AccountPack {
            minutes_left: (p.kind != "pass").then(|| p.minutes_left.unwrap_or(0)),
            name: p.name,
            expires_at: p.expires_at,
        })
        .collect();
    owned.sort_by_key(|p| p.expires_at);
    Ok(PlayerAccountResult {
        balance_minor: (balance - reserved).max(0),
        packs: owned,
    })
}

/// How many expired packs and passes the Wallet & packs tab keeps listing.
const EXPIRED_PACK_LIMIT: usize = 10;

/// `phone` with every digit but the last four replaced by a bullet; a `+` and spaces stay.
pub fn mask_phone(phone: &str) -> String {
    let digits = phone.chars().filter(char::is_ascii_digit).count();
    let mut seen = 0;
    phone
        .chars()
        .map(|c| {
            if !c.is_ascii_digit() {
                return c;
            }
            seen += 1;
            if seen + 4 > digits { c } else { '•' }
        })
        .collect()
}

/// What the Account page shows for `player_id` (Story 6.34), read in one snapshot: their opened
/// name and masked phone, the wallet balance, every pack and pass they own (unexpired first, then
/// the latest expired ones with their unused time) and their newest orders at this branch.
pub fn account_page(
    store: &Store,
    kek: &[u8; KEK_LEN],
    player_id: &str,
    now_ms: i64,
) -> Result<PlayerAccountPage, AppError> {
    let (player, packs, balance, reserved, groups, orders) = store.read_consistent(|s| {
        Ok((
            s.player(player_id)?,
            s.player_packs(player_id)?,
            s.wallet_balance(player_id)?,
            s.reserved_minor(player_id)?,
            s.group_names()?,
            s.player_orders(player_id, ACCOUNT_ORDER_LIMIT)?,
        ))
    })?;
    let Some((row, _, wrapped)) = player else {
        return Err(AppError::new(ErrorCode::NotFound, "no such player"));
    };
    let open = |field, enc: &str| open_field(kek, player_id, &wrapped, field, enc);
    let group_name = |id: &String| {
        groups
            .iter()
            .find(|(g, _)| g == id)
            .map_or_else(|| id.clone(), |(_, name)| name.clone())
    };
    let mut entries: Vec<AccountPackEntry> = packs
        .into_iter()
        .map(|p| {
            let expired = p.expires_at <= now_ms;
            AccountPackEntry {
                groups: p.group_ids.iter().map(group_name).collect(),
                minutes_left: (p.kind != "pass").then(|| p.minutes_left.unwrap_or(0)),
                window_min: p.window_start_min.zip(p.window_end_min),
                name: p.name,
                expires_at: p.expires_at,
                expired,
            }
        })
        // A pack with no time left has nothing to show; an expired one shows what went unused.
        .filter(|e| e.minutes_left.is_none_or(|m| m > 0))
        .collect();
    entries.sort_by_key(|e| {
        (
            e.expired,
            if e.expired {
                -e.expires_at
            } else {
                e.expires_at
            },
        )
    });
    let live = entries.iter().filter(|e| !e.expired).count();
    entries.truncate(live + EXPIRED_PACK_LIMIT);
    Ok(PlayerAccountPage {
        name: open("name", &row.name_enc)?,
        username: row.username,
        phone_masked: mask_phone(&open("phone", &row.phone_enc)?),
        balance_minor: (balance - reserved).max(0),
        packs: entries,
        orders: orders
            .into_iter()
            .map(|o| AccountOrder {
                order_id: o.order_id,
                at: o.created_at,
                items: o
                    .items
                    .into_iter()
                    .map(|(name, qty)| AccountOrderItem { name, qty })
                    .collect(),
                total_minor: o.total_minor,
                status: o.status,
                invoice_no: o.invoice_no,
            })
            .collect(),
    })
}

/// The stored password hash of `player_id`, for the lock screen's check.
pub fn player_password_hash(store: &Store, player_id: &str) -> Result<Option<String>, AppError> {
    Ok(store
        .player(player_id)?
        .map(|(row, _, _)| row.password_hash))
}

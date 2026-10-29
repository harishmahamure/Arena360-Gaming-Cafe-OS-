//! What the kiosk's account menu reads (Story 6.17): the live session on a PC, what its player
//! holds, and the hash their own lock screen checks.
//!
//! All of it is read from one snapshot, by device: a kiosk names no player and no session, the Hub
//! finds them from the session running on the paired PC.

use arena_proto::{AccountPack, AppError, PlayerAccountResult};
use arena_store::Store;

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

/// The stored password hash of `player_id`, for the lock screen's check.
pub fn player_password_hash(store: &Store, player_id: &str) -> Result<Option<String>, AppError> {
    Ok(store
        .player(player_id)?
        .map(|(row, _, _)| row.password_hash))
}

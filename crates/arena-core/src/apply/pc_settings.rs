//! `player.pc_settings_set` (Story 6.35): a profiled player's PC settings saved to their profile.
//! The Hub builds the intent (device actor) from the paired kiosk's `player.pc_settings_set`
//! frame; the device rule allows it only for the player on the device's own live session. The
//! patch is merged into the player's row: the first save inserts it (rev 1), every later one
//! replaces it. Nothing is stored for a bad value.

use arena_proto::{AppError, ErrorCode, Intent, PlayerPcSettingsSet};
use arena_store::{
    ChangeOp, RowKey, StoreTx, find_pc_settings, find_player, insert_pc_settings,
    update_pc_settings,
};

use super::Target;
use super::mutate::Written;

/// The player and their stored settings.
pub(crate) struct PcSettingsTarget {
    player_exists: bool,
    stored: Option<arena_proto::PcSettings>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &PlayerPcSettingsSet) -> Result<Target, AppError> {
    Ok(Target::PcSettings(Box::new(PcSettingsTarget {
        player_exists: find_player(tx, &p.player_id)?.is_some(),
        stored: find_pc_settings(tx, &p.player_id)?,
    })))
}

pub(super) fn validate(
    intent: &Intent,
    p: &PlayerPcSettingsSet,
    t: &PcSettingsTarget,
) -> Result<(), AppError> {
    if intent.base_rev.is_some() {
        return Err(invalid(
            "base_rev",
            "player.pc_settings_set takes no base_rev",
        ));
    }
    p.settings.validate()?;
    if p.settings.is_empty() {
        return Err(invalid("settings", "choose at least one PC setting"));
    }
    if !t.player_exists {
        return Err(invalid("player_id", "no such player"));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &PlayerPcSettingsSet,
    t: &PcSettingsTarget,
) -> Result<Vec<Written>, AppError> {
    let op = match &t.stored {
        Some(stored) => {
            update_pc_settings(tx, &p.player_id, &stored.merged(&p.settings))?;
            ChangeOp::Update
        }
        None => {
            insert_pc_settings(tx, &p.player_id, &p.settings)?;
            ChangeOp::Insert
        }
    };
    Ok(vec![Written {
        key: RowKey::PlayerPcSettings {
            player_id: p.player_id.clone(),
        },
        op,
        before: None,
    }])
}

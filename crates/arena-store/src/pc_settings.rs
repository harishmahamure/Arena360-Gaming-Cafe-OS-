//! PC settings saved to a player's profile (Story 6.35): the `player_pc_settings` change-log
//! table, one row per player. Only `apply()` writes it, through [`insert_pc_settings`] and
//! [`update_pc_settings`]; the readers feed the Hub's `player.pc_settings` answer.

use arena_proto::{AppError, ErrorCode, PcSettings};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

/// The player's stored settings, if they ever saved any.
pub fn find_pc_settings(
    conn: &Connection,
    player_id: &str,
) -> Result<Option<PcSettings>, AppError> {
    conn.query_row(
        "SELECT mouse_speed, display_mode, audio_output, keyboard_layout
         FROM player_pc_settings WHERE player_id = ?1",
        [player_id],
        |r| {
            Ok(PcSettings {
                mouse_speed: r
                    .get::<_, Option<i64>>(0)?
                    .and_then(|s| u8::try_from(s).ok()),
                display_mode: r.get(1)?,
                audio_output: r.get(2)?,
                keyboard_layout: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Adds a player's first row with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_pc_settings(
    conn: &Connection,
    player_id: &str,
    s: &PcSettings,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO player_pc_settings (player_id, mouse_speed, display_mode, audio_output,
                                        keyboard_layout, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0)",
        params![
            player_id,
            s.mouse_speed.map(i64::from),
            s.display_mode,
            s.audio_output,
            s.keyboard_layout
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Replaces a player's stored settings with `s` (the caller merged the patch in); the stamp step
/// then bumps `rev`.
pub fn update_pc_settings(
    conn: &Connection,
    player_id: &str,
    s: &PcSettings,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE player_pc_settings
             SET mouse_speed = ?2, display_mode = ?3, audio_output = ?4, keyboard_layout = ?5
             WHERE player_id = ?1",
            params![
                player_id,
                s.mouse_speed.map(i64::from),
                s.display_mode,
                s.audio_output,
                s.keyboard_layout
            ],
        )
        .map_err(db_error)?;
    if n == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::Internal,
            format!("no player_pc_settings row {player_id}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE players (player_id TEXT PRIMARY KEY);
             INSERT INTO players VALUES ('P1');",
        )
        .unwrap();
        conn.execute_batch(include_str!("../migrations/0067_player_pc_settings.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn a_row_is_inserted_read_and_replaced() {
        let conn = conn();
        assert_eq!(find_pc_settings(&conn, "P1").unwrap(), None);
        let first = PcSettings {
            mouse_speed: Some(14),
            keyboard_layout: Some("00000409".into()),
            ..Default::default()
        };
        insert_pc_settings(&conn, "P1", &first).unwrap();
        assert_eq!(find_pc_settings(&conn, "P1").unwrap(), Some(first.clone()));
        let next = first.merged(&PcSettings {
            display_mode: Some("1920x1080@144".into()),
            ..Default::default()
        });
        update_pc_settings(&conn, "P1", &next).unwrap();
        assert_eq!(find_pc_settings(&conn, "P1").unwrap(), Some(next));
        assert_eq!(
            update_pc_settings(&conn, "P2", &first).unwrap_err().code,
            ErrorCode::Internal
        );
    }

    #[test]
    fn a_mouse_speed_outside_windows_range_is_refused() {
        let conn = conn();
        for bad in [0u8, 21] {
            let s = PcSettings {
                mouse_speed: Some(bad),
                ..Default::default()
            };
            assert!(insert_pc_settings(&conn, "P1", &s).is_err(), "{bad}");
        }
        assert_eq!(find_pc_settings(&conn, "P1").unwrap(), None);
    }
}

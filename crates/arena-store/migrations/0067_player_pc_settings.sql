-- 0067: PC settings saved to a player's profile (Story 6.35). One row per player who ever saved
-- a PC setting at a kiosk, written by `player.pc_settings_set` (the Hub builds the intent from
-- the paired kiosk's frame) and applied on whichever PC their next session starts on.
-- Every column but the key is NULL until the player chooses it: `mouse_speed` is Windows' 1 to
-- 20, `display_mode` is `WxH@Hz`, `audio_output` is the output device's name, `keyboard_layout`
-- is the layout's 8-digit id. The Hub never edits these per player.
CREATE TABLE player_pc_settings (
    player_id       TEXT PRIMARY KEY REFERENCES players (player_id),
    mouse_speed     INTEGER NULL CHECK (mouse_speed IS NULL OR mouse_speed BETWEEN 1 AND 20),
    display_mode    TEXT NULL,
    audio_output    TEXT NULL,
    keyboard_layout TEXT NULL,
    rev             INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

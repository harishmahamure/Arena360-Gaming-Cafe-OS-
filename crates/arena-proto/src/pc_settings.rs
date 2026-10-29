//! PC settings saved to the player's profile (Story 6.35): mouse speed, display mode, sound
//! output and keyboard layout follow a regular player to any PC of the branch.
//!
//! - `kiosk.display_modes` (a branch setting every paired PC reads): the display modes a player
//!   may pick, `WxH@Hz` separated by commas. Empty means each PC's own current mode only.
//! - `player.pc_settings{}` -> `player.pc_settings_result{..}`: the settings saved for the player
//!   on this PC's session (all absent for a guest or a player with nothing saved).
//! - `player.pc_settings_set{..}` -> `player.pc_settings_set_result{saved}`: the player changed
//!   some of them; the Hub stores a profiled player's choices and answers `saved:false` for a
//!   guest.
//!
//! The kiosk names no player: the Hub finds the player from the session on the PC.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppError, ErrorCode};

/// The branch setting that lists the allowed display modes.
pub const KEY_DISPLAY_MODES: &str = "kiosk.display_modes";
/// Empty: each PC's current mode only.
pub const DEFAULT_DISPLAY_MODES: &str = "";

/// Windows' pointer speed runs from 1 (slowest) to 20 (fastest).
pub const MOUSE_SPEED_MIN: u8 = 1;
pub const MOUSE_SPEED_MAX: u8 = 20;
/// Windows' default pointer speed.
pub const MOUSE_SPEED_DEFAULT: u8 = 10;

/// The most display modes the branch list holds.
pub const DISPLAY_MODES_MAX: usize = 30;
/// A display mode that is not kept within this many seconds goes back.
pub const DISPLAY_REVERT_SECS: u32 = 15;
/// The longest sound output name or keyboard layout id the Hub stores.
pub const PC_SETTING_TEXT_MAX: usize = 128;

const DIMENSION_MIN: u32 = 320;
const DIMENSION_MAX: u32 = 16_384;
const HZ_MIN: u32 = 20;
const HZ_MAX: u32 = 1_000;

/// A display mode: resolution and refresh rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

impl fmt::Display for DisplayMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}@{}", self.width, self.height, self.hz)
    }
}

fn number(part: &str, min: u32, max: u32) -> Option<u32> {
    // Plain digits, no sign and no leading zero: one canonical spelling per mode.
    if part.is_empty()
        || part.len() > 6
        || !part.bytes().all(|b| b.is_ascii_digit())
        || part.starts_with('0')
    {
        return None;
    }
    part.parse::<u32>().ok().filter(|n| (min..=max).contains(n))
}

/// Reads `WxH@Hz` (`1920x1080@144`): digits only, no leading zeros, a size of 320 to 16384 a side
/// and a refresh of 20 to 1000 Hz. Anything else is `None`.
pub fn parse_display_mode(value: &str) -> Option<DisplayMode> {
    let (size, hz) = value.split_once('@')?;
    let (width, height) = size.split_once('x')?;
    Some(DisplayMode {
        width: number(width, DIMENSION_MIN, DIMENSION_MAX)?,
        height: number(height, DIMENSION_MIN, DIMENSION_MAX)?,
        hz: number(hz, HZ_MIN, HZ_MAX)?,
    })
}

/// Why a display mode list is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayModesProblem {
    /// An entry that is not `WxH@Hz`.
    Shape,
    /// The same mode twice.
    Duplicate,
    /// More than [`DISPLAY_MODES_MAX`].
    TooMany,
}

impl DisplayModesProblem {
    pub fn message(self) -> &'static str {
        match self {
            Self::Shape => "list display modes such as 1920x1080@144, separated by commas",
            Self::Duplicate => "each display mode must be listed once",
            Self::TooMany => "list at most 30 display modes",
        }
    }
}

/// Reads the branch's allowed display modes, in the order given. Spaces around an entry are
/// tolerated; empty is an empty list.
pub fn parse_display_modes(value: &str) -> Result<Vec<DisplayMode>, DisplayModesProblem> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<DisplayMode> = Vec::new();
    for part in value.split(',') {
        let mode = parse_display_mode(part.trim()).ok_or(DisplayModesProblem::Shape)?;
        if out.contains(&mode) {
            return Err(DisplayModesProblem::Duplicate);
        }
        out.push(mode);
    }
    if out.len() > DISPLAY_MODES_MAX {
        return Err(DisplayModesProblem::TooMany);
    }
    Ok(out)
}

/// Whether `value` is a mouse speed Windows takes.
pub fn valid_mouse_speed(value: u8) -> bool {
    (MOUSE_SPEED_MIN..=MOUSE_SPEED_MAX).contains(&value)
}

fn valid_text(value: &str) -> bool {
    let n = value.chars().count();
    n > 0
        && n <= PC_SETTING_TEXT_MAX
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// A keyboard layout id: the 8 hex digits Windows names a layout by (`00000409`).
pub fn valid_keyboard_layout(value: &str) -> bool {
    value.len() == 8
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

/// The PC settings a player chose, or a part of them: an absent field is "not chosen". The sound
/// output is the device's name (the same on every PC of one build), the keyboard layout its
/// 8-digit id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub mouse_speed: Option<u8>,
    /// `WxH@Hz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub display_mode: Option<String>,
    /// The sound output's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub audio_output: Option<String>,
    /// The keyboard layout's 8-digit id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub keyboard_layout: Option<String>,
}

impl PcSettings {
    /// Whether nothing is chosen.
    pub fn is_empty(&self) -> bool {
        self.mouse_speed.is_none()
            && self.display_mode.is_none()
            && self.audio_output.is_none()
            && self.keyboard_layout.is_none()
    }

    /// The values must be ones a PC could apply: a mouse speed of 1 to 20, a `WxH@Hz` mode, a
    /// sound output name of 1 to 128 characters, an 8-digit layout id. `VALIDATION_FAILED` naming
    /// the field, never echoing a value.
    pub fn validate(&self) -> Result<(), AppError> {
        let bad = |field: &str, message: &str| {
            Err(AppError::with_field(
                ErrorCode::ValidationFailed,
                field,
                message,
            ))
        };
        if self.mouse_speed.is_some_and(|s| !valid_mouse_speed(s)) {
            return bad("mouse_speed", "the mouse speed is 1 to 20");
        }
        if let Some(mode) = &self.display_mode
            && parse_display_mode(mode).is_none()
        {
            return bad("display_mode", "a display mode looks like 1920x1080@144");
        }
        if let Some(out) = &self.audio_output
            && !valid_text(out)
        {
            return bad(
                "audio_output",
                "the sound output needs a name of 1 to 128 characters",
            );
        }
        if let Some(layout) = &self.keyboard_layout
            && !valid_keyboard_layout(layout)
        {
            return bad(
                "keyboard_layout",
                "a keyboard layout is 8 hexadecimal digits",
            );
        }
        Ok(())
    }

    /// `self` with every field `patch` chose replaced by the patch's.
    pub fn merged(&self, patch: &PcSettings) -> PcSettings {
        PcSettings {
            mouse_speed: patch.mouse_speed.or(self.mouse_speed),
            display_mode: patch
                .display_mode
                .clone()
                .or_else(|| self.display_mode.clone()),
            audio_output: patch
                .audio_output
                .clone()
                .or_else(|| self.audio_output.clone()),
            keyboard_layout: patch
                .keyboard_layout
                .clone()
                .or_else(|| self.keyboard_layout.clone()),
        }
    }
}

/// A keyboard layout installed on a PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcLayout {
    /// The 8-digit id a setting is saved by.
    pub id: String,
    /// What the player sees ("English (United States)").
    pub name: String,
}

/// The PC settings panel's content (Story 6.35): what the kiosk service answers the app's
/// `pc_settings` request. It holds this PC's current values and the choices the player has here:
/// the display modes both allowed by the branch and supported by this monitor, this PC's sound
/// outputs and its installed keyboard layouts. It never leaves the PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcSettingsPanel {
    /// The pointer speed, 1 to 20.
    pub mouse_speed: u8,
    /// The current display mode, `WxH@Hz`.
    pub display_mode: String,
    /// The modes to choose from (allowed and supported), as `WxH@Hz`.
    pub display_modes: Vec<String>,
    /// This PC's sound outputs, by name.
    pub audio_outputs: Vec<String>,
    /// The default sound output now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub audio_output: Option<String>,
    pub keyboard_layouts: Vec<PcLayout>,
    /// The active keyboard layout's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string | null", optional)]
    pub keyboard_layout: Option<String>,
    /// While a display change waits for "Keep this": the milliseconds before it goes back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub revert_ms: Option<u64>,
}

/// What the kiosk service answers a change (`pc_settings_set`) or "Keep this" (`pc_settings_keep`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PcSettingsChanged {
    /// The Hub saved it to the player's profile. `false` for a guest and for a display change that
    /// still waits for "Keep this".
    pub saved: bool,
    /// A display change waits for "Keep this": the milliseconds before it goes back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "number | null", optional)]
    pub revert_ms: Option<u64>,
}

/// `player.pc_settings_result` (Hub -> kiosk): the settings saved for the player on this PC's
/// session. Empty for a guest and for a player who saved nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPcSettingsResult {
    #[serde(default)]
    pub settings: PcSettings,
}

/// `player.pc_settings_set_result` (Hub -> kiosk): whether the Hub stored the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPcSettingsSetResult {
    /// `false` for a guest: the change is theirs for this session only.
    pub saved: bool,
}

/// The `player.pc_settings_set` intent (Hub-built, device actor): the player's own PC settings
/// patch. The kiosk cannot send it; it sends the frame and the Hub builds this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPcSettingsSet {
    pub player_id: String,
    pub settings: PcSettings,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, FrameBody, parse_frame};

    #[test]
    fn display_modes_have_one_canonical_spelling() {
        let m = parse_display_mode("1920x1080@144").unwrap();
        assert_eq!((m.width, m.height, m.hz), (1920, 1080, 144));
        assert_eq!(m.to_string(), "1920x1080@144");
        for bad in [
            "",
            "1920x1080",
            "1920x1080@",
            "@60",
            "1920*1080@60",
            "01920x1080@60",
            "1920x1080@060",
            "+1920x1080@60",
            "1920x1080@60.5",
            "1920 x1080@60",
            "100x100@60",
            "1920x1080@5",
            "1920x1080@1001",
            "99999x1080@60",
            "1920x1080@60@60",
            "1920X1080@60",
        ] {
            assert_eq!(parse_display_mode(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_branch_list_is_modes_separated_by_commas() {
        assert!(parse_display_modes("").unwrap().is_empty());
        assert!(parse_display_modes("  ").unwrap().is_empty());
        let list = parse_display_modes("1920x1080@144, 1920x1080@60 ,2560x1440@60").unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].to_string(), "1920x1080@60");
        for bad in ["1920x1080", "1920x1080@60,", ",1920x1080@60", "a,b"] {
            assert_eq!(
                parse_display_modes(bad),
                Err(DisplayModesProblem::Shape),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse_display_modes("1920x1080@60,1920x1080@60"),
            Err(DisplayModesProblem::Duplicate)
        );
        let many = (0..=DISPLAY_MODES_MAX)
            .map(|i| format!("{}x1080@60", 1000 + i))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_display_modes(&many),
            Err(DisplayModesProblem::TooMany)
        );
    }

    #[test]
    fn settings_are_validated_by_field() {
        let ok = PcSettings {
            mouse_speed: Some(20),
            display_mode: Some("1920x1080@60".into()),
            audio_output: Some("Speakers (Realtek Audio)".into()),
            keyboard_layout: Some("00000409".into()),
        };
        assert!(ok.validate().is_ok());
        assert!(PcSettings::default().validate().is_ok());
        assert!(PcSettings::default().is_empty() && !ok.is_empty());
        for (bad, field) in [
            (
                PcSettings {
                    mouse_speed: Some(0),
                    ..Default::default()
                },
                "mouse_speed",
            ),
            (
                PcSettings {
                    mouse_speed: Some(21),
                    ..Default::default()
                },
                "mouse_speed",
            ),
            (
                PcSettings {
                    display_mode: Some("big".into()),
                    ..Default::default()
                },
                "display_mode",
            ),
            (
                PcSettings {
                    audio_output: Some(String::new()),
                    ..Default::default()
                },
                "audio_output",
            ),
            (
                PcSettings {
                    audio_output: Some("x".repeat(PC_SETTING_TEXT_MAX + 1)),
                    ..Default::default()
                },
                "audio_output",
            ),
            (
                PcSettings {
                    audio_output: Some("a\nb".into()),
                    ..Default::default()
                },
                "audio_output",
            ),
            (
                PcSettings {
                    keyboard_layout: Some("409".into()),
                    ..Default::default()
                },
                "keyboard_layout",
            ),
            (
                PcSettings {
                    keyboard_layout: Some("0000040g".into()),
                    ..Default::default()
                },
                "keyboard_layout",
            ),
        ] {
            let e = bad.validate().unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.field.as_deref(), Some(field));
        }
    }

    #[test]
    fn a_patch_replaces_only_what_it_chose() {
        let stored = PcSettings {
            mouse_speed: Some(5),
            display_mode: Some("1920x1080@60".into()),
            ..Default::default()
        };
        let patch = PcSettings {
            mouse_speed: Some(12),
            keyboard_layout: Some("00000409".into()),
            ..Default::default()
        };
        assert_eq!(
            stored.merged(&patch),
            PcSettings {
                mouse_speed: Some(12),
                display_mode: Some("1920x1080@60".into()),
                audio_output: None,
                keyboard_layout: Some("00000409".into()),
            }
        );
    }

    #[test]
    fn the_pc_settings_frames_round_trip() {
        let req = parse_frame(r#"{"v":1,"kind":"player.pc_settings","rid":"a"}"#).unwrap();
        assert_eq!(req.body, FrameBody::PlayerPcSettingsRequest);
        let set =
            parse_frame(r#"{"v":1,"kind":"player.pc_settings_set","rid":"b","mouse_speed":7}"#)
                .unwrap();
        assert_eq!(
            set.body,
            FrameBody::PlayerPcSettingsSet(PcSettings {
                mouse_speed: Some(7),
                ..Default::default()
            })
        );
        for body in [
            FrameBody::PlayerPcSettingsResult(PlayerPcSettingsResult {
                settings: PcSettings {
                    display_mode: Some("1920x1080@60".into()),
                    ..Default::default()
                },
            }),
            FrameBody::PlayerPcSettingsResult(PlayerPcSettingsResult::default()),
            FrameBody::PlayerPcSettingsSetResult(PlayerPcSettingsSetResult { saved: false }),
        ] {
            let frame = Frame::new("r", body);
            assert_eq!(parse_frame(&frame.to_text()).unwrap(), frame);
        }
        let v = serde_json::to_value(Frame::new(
            "r",
            FrameBody::PlayerPcSettingsSetResult(PlayerPcSettingsSetResult { saved: true }),
        ))
        .unwrap();
        assert_eq!(v["kind"], "player.pc_settings_set_result");
        assert_eq!(v["saved"], true);
    }
}

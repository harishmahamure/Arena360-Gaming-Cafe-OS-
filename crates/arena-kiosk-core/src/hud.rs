//! The overlay HUD's pure rules (Story 6.14): which key opens the expanded panel, when a
//! fullscreen-exclusive game hides everything, and where the HUD window sits.
//!
//! The HUD is a second always-on-top, non-activating window in the top-right of the primary
//! monitor. It never takes focus except while the player has expanded it with Ctrl+Shift+E.

use crate::keys::{KeyEvent, Modifiers, vk};
use crate::monitors::Rect;

/// Gap between the HUD and the monitor's top and right edges, in logical px.
pub const MARGIN: f64 = 16.0;

/// The widest the HUD gets, in logical px: the info toast and the expanded panel.
pub const MAX_WIDTH: f64 = 640.0;

/// Whether `event`, with `mods` held, is Ctrl+Shift+E going down: the hotkey that expands the
/// HUD into its panel. Alt and the Windows key must be up, so Ctrl+Alt+Shift+E and the Windows
/// chords keep their own meaning.
pub fn is_expand_hotkey(event: KeyEvent, mods: Modifiers) -> bool {
    event.down && event.vk == vk::E && mods.ctrl && mods.shift && !mods.alt && !mods.win
}

/// `SHQueryUserNotificationState`'s answer (`QUERY_USER_NOTIFICATION_STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    /// The user can be notified (no fullscreen app).
    Free,
    /// The screen is locked, or the screensaver runs.
    NotPresent,
    /// A fullscreen app that is not Direct3D exclusive (a borderless game, a video).
    Busy,
    /// A Direct3D fullscreen-exclusive game.
    D3dFullScreen,
    /// Presentation mode.
    Presentation,
    /// Windows' quiet time.
    AcceptsNotifications,
    /// Quiet time (Windows 7 and later).
    QuietTime,
    /// A Windows Store app in the foreground.
    App,
}

impl Notification {
    /// From the raw `QUERY_USER_NOTIFICATION_STATE` value; `None` for any value Windows has not
    /// defined.
    pub fn from_raw(raw: i32) -> Option<Self> {
        Some(match raw {
            1 => Self::NotPresent,
            2 => Self::Busy,
            3 => Self::D3dFullScreen,
            4 => Self::Presentation,
            5 => Self::AcceptsNotifications,
            6 => Self::QuietTime,
            7 => Self::App,
            0 => Self::Free,
            _ => return None,
        })
    }

    /// Whether nothing may be drawn over the foreground: only a Direct3D fullscreen-exclusive
    /// game. A borderless fullscreen window (including the kiosk's own shell) reports `Busy`,
    /// so `Busy` alone never hides the HUD.
    pub fn hides_overlay(self) -> bool {
        self == Self::D3dFullScreen
    }
}

/// Whether the HUD window is on screen: the page asked for content (`width > 0` and
/// `height > 0`), and no fullscreen-exclusive game is in the foreground.
pub fn window_visible(content: (f64, f64), exclusive_fullscreen: bool) -> bool {
    content.0 > 0.0 && content.1 > 0.0 && !exclusive_fullscreen
}

/// Whether a time warning must flash the taskbar instead of drawing (Story 6.19): a mark of 5
/// minutes or less (the default 5 and 1; a branch's own marks follow, Story 6.20), newly reached,
/// while a fullscreen-exclusive game hides the HUD. Longer marks are the chip's warning border only.
pub fn warning_flash(previous: Option<u8>, now: Option<u8>, exclusive_fullscreen: bool) -> bool {
    exclusive_fullscreen && previous != now && matches!(now, Some(1..=5))
}

/// The HUD window's rectangle in physical px: `content` (logical px, clamped to the monitor and
/// [`MAX_WIDTH`]) at the top-right of `monitor`, `MARGIN` in from both edges, at `scale`.
pub fn place(monitor: Rect, scale: f64, content: (f64, f64)) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = (content.0.clamp(1.0, MAX_WIDTH) * scale).round() as i32;
    let width = width.min(monitor.width.max(1));
    let height = ((content.1.max(1.0)) * scale).round() as i32;
    let height = height.min(monitor.height.max(1));
    let margin = (MARGIN * scale).round() as i32;
    let x = (monitor.x + monitor.width - width - margin).max(monitor.x);
    let y = monitor.y + margin.min((monitor.height - height).max(0));
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool, alt: bool, win: bool) -> Modifiers {
        Modifiers {
            ctrl,
            alt,
            shift,
            win,
        }
    }

    #[test]
    fn only_a_new_5_or_1_minute_mark_over_an_exclusive_game_flashes() {
        assert!(warning_flash(Some(10), Some(5), true));
        assert!(warning_flash(None, Some(1), true));
        assert!(
            warning_flash(None, Some(3), true),
            "a branch's own short mark"
        );
        assert!(
            !warning_flash(Some(5), Some(5), true),
            "not again for the same mark"
        );
        assert!(
            !warning_flash(None, Some(10), true),
            "10 min is the chip only"
        );
        assert!(
            !warning_flash(Some(10), Some(5), false),
            "the HUD draws it instead"
        );
        assert!(
            !warning_flash(Some(5), None, true),
            "an extension clears it"
        );
    }

    #[test]
    fn ctrl_shift_e_going_down_is_the_hotkey() {
        let e = KeyEvent::down(vk::E);
        assert!(is_expand_hotkey(e, mods(true, true, false, false)));
        assert!(!is_expand_hotkey(
            KeyEvent::up(vk::E),
            mods(true, true, false, false)
        ));
        assert!(!is_expand_hotkey(e, mods(true, false, false, false)));
        assert!(!is_expand_hotkey(e, mods(false, true, false, false)));
        assert!(!is_expand_hotkey(e, mods(true, true, true, false)));
        assert!(!is_expand_hotkey(e, mods(true, true, false, true)));
        assert!(!is_expand_hotkey(
            KeyEvent::down(vk::D),
            mods(true, true, false, false)
        ));
    }

    #[test]
    fn only_a_d3d_exclusive_game_hides_the_overlay() {
        for raw in 0..=7 {
            let n = Notification::from_raw(raw).unwrap();
            assert_eq!(n.hides_overlay(), raw == 3, "{raw}");
        }
        assert_eq!(Notification::from_raw(8), None);
        assert_eq!(Notification::from_raw(-1), None);
    }

    #[test]
    fn the_window_shows_only_with_content_and_no_exclusive_game() {
        assert!(window_visible((260.0, 56.0), false));
        assert!(!window_visible((260.0, 56.0), true));
        assert!(!window_visible((0.0, 0.0), false));
        assert!(!window_visible((260.0, 0.0), false));
    }

    #[test]
    fn the_hud_sits_top_right_of_the_monitor() {
        let m = Rect::new(0, 0, 1920, 1080);
        assert_eq!(place(m, 1.0, (260.0, 56.0)), Rect::new(1644, 16, 260, 56));
        // 125% scale: sizes and margin scale; the right edge stays 20 px in.
        let r = place(m, 1.25, (260.0, 56.0));
        assert_eq!((r.width, r.height), (325, 70));
        assert_eq!(r.x + r.width, 1920 - 20);
        assert_eq!(r.y, 20);
    }

    #[test]
    fn the_hud_follows_a_monitor_that_is_not_at_the_origin() {
        let m = Rect::new(-1920, 100, 1920, 1080);
        let r = place(m, 1.0, (640.0, 200.0));
        assert_eq!(r, Rect::new(-1920 + 1920 - 640 - 16, 116, 640, 200));
    }

    #[test]
    fn the_hud_never_exceeds_the_toast_width_or_the_monitor() {
        let m = Rect::new(0, 0, 800, 300);
        let r = place(m, 1.0, (2000.0, 2000.0));
        assert_eq!(r.width, 640);
        assert_eq!(r.height, 300);
        assert_eq!(r.y, 0);
        assert_eq!(place(m, f64::NAN, (100.0, 50.0)).width, 100);
        assert_eq!(place(m, 0.0, (100.0, 50.0)).width, 100);
    }
}

//! The kiosk's low-level keyboard filter, as a pure decision.
//!
//! The Kiosk app's `WH_KEYBOARD_LL` hook feeds every key event through a [`KeyFilter`], which
//! tracks the modifiers itself (a swallowed key never reaches the system's key state) and
//! [`decide`]s whether the event is swallowed. Swallowed: either Windows key and any chord with
//! one, Alt+F4, Alt+Tab, Alt+Esc, Ctrl+Esc and Alt+Space. Ctrl+Alt+Shift+A is always passed (it
//! is reserved for the admin exit, Story 1.20). Ctrl+Alt+Del cannot be hooked; the player's
//! policy keys cover it (no Task Manager, lock, log off or password change).

/// Windows virtual-key codes the filter knows.
pub mod vk {
    pub const TAB: u32 = 0x09;
    pub const SHIFT: u32 = 0x10;
    pub const CONTROL: u32 = 0x11;
    pub const MENU: u32 = 0x12;
    pub const ESCAPE: u32 = 0x1B;
    pub const SPACE: u32 = 0x20;
    pub const A: u32 = 0x41;
    pub const D: u32 = 0x44;
    pub const E: u32 = 0x45;
    pub const LWIN: u32 = 0x5B;
    pub const RWIN: u32 = 0x5C;
    pub const F4: u32 = 0x73;
    pub const LSHIFT: u32 = 0xA0;
    pub const RSHIFT: u32 = 0xA1;
    pub const LCONTROL: u32 = 0xA2;
    pub const RCONTROL: u32 = 0xA3;
    pub const LMENU: u32 = 0xA4;
    pub const RMENU: u32 = 0xA5;
}

/// One key going down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u32,
    pub down: bool,
}

impl KeyEvent {
    pub fn down(vk: u32) -> Self {
        Self { vk, down: true }
    }
    pub fn up(vk: u32) -> Self {
        Self { vk, down: false }
    }
}

/// The modifiers held when an event arrives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// What the hook does with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Swallow,
    Pass,
}

fn is_win(vk: u32) -> bool {
    matches!(vk, vk::LWIN | vk::RWIN)
}

/// Whether `event`, with `mods` held, is swallowed.
pub fn decide(event: KeyEvent, mods: Modifiers) -> Decision {
    // Reserved for the admin exit (Story 1.20): never swallowed.
    if event.vk == vk::A && mods.ctrl && mods.alt && mods.shift && !mods.win {
        return Decision::Pass;
    }
    if is_win(event.vk) || mods.win {
        return Decision::Swallow;
    }
    let blocked = (mods.alt && matches!(event.vk, vk::F4 | vk::TAB | vk::ESCAPE | vk::SPACE))
        || (mods.ctrl && event.vk == vk::ESCAPE);
    if blocked {
        Decision::Swallow
    } else {
        Decision::Pass
    }
}

/// Tracks which modifiers are held, from the events themselves.
#[derive(Debug, Clone, Default)]
pub struct KeyFilter {
    lwin: bool,
    rwin: bool,
    lctrl: bool,
    rctrl: bool,
    lalt: bool,
    ralt: bool,
    lshift: bool,
    rshift: bool,
}

impl KeyFilter {
    /// Nothing held (usable in a `static`).
    pub const fn new() -> Self {
        Self {
            lwin: false,
            rwin: false,
            lctrl: false,
            rctrl: false,
            lalt: false,
            ralt: false,
            lshift: false,
            rshift: false,
        }
    }

    /// The modifiers currently held.
    pub fn modifiers(&self) -> Modifiers {
        Modifiers {
            ctrl: self.lctrl || self.rctrl,
            alt: self.lalt || self.ralt,
            shift: self.lshift || self.rshift,
            win: self.lwin || self.rwin,
        }
    }

    fn track(&mut self, e: KeyEvent) {
        let slot = match e.vk {
            vk::LWIN => &mut self.lwin,
            vk::RWIN => &mut self.rwin,
            vk::LCONTROL | vk::CONTROL => &mut self.lctrl,
            vk::RCONTROL => &mut self.rctrl,
            vk::LMENU | vk::MENU => &mut self.lalt,
            vk::RMENU => &mut self.ralt,
            vk::LSHIFT | vk::SHIFT => &mut self.lshift,
            vk::RSHIFT => &mut self.rshift,
            _ => return,
        };
        *slot = e.down;
    }

    /// Overwrites the held state with `real` (the OS's live modifier state), so a key-up missed
    /// while the window had no focus, or a modifier held before the hook was installed, cannot
    /// leave a modifier stuck. Call it before deciding, and once at install.
    pub fn resync(&mut self, real: Modifiers) {
        // Collapse to the left slots; the right ones clear (only the OR matters to `decide`).
        self.lctrl = real.ctrl;
        self.rctrl = false;
        self.lalt = real.alt;
        self.ralt = false;
        self.lshift = real.shift;
        self.rshift = false;
        self.lwin = real.win;
        self.rwin = false;
    }

    /// Decides `event` against the modifiers held before it, then records it. `alt_flag` is the
    /// hook's own "Alt is down" bit (`LLKHF_ALTDOWN`), which also covers an Alt pressed before
    /// the hook was installed.
    pub fn on_event(&mut self, event: KeyEvent, alt_flag: bool) -> Decision {
        let mut mods = self.modifiers();
        mods.alt |= alt_flag;
        let d = decide(event, mods);
        self.track(event);
        d
    }

    /// Resyncs from the OS's live state (`real`), then decides and records `event`. The hook
    /// uses this with `GetAsyncKeyState` so a missed key-up or a pre-held modifier never leaves
    /// a chord mis-detected.
    pub fn on_event_synced(
        &mut self,
        event: KeyEvent,
        alt_flag: bool,
        real: Modifiers,
    ) -> Decision {
        self.resync(real);
        self.on_event(event, alt_flag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(f: &mut KeyFilter, events: &[KeyEvent]) -> Vec<Decision> {
        events.iter().map(|e| f.on_event(*e, false)).collect()
    }

    use Decision::{Pass, Swallow};

    #[test]
    fn the_windows_keys_are_swallowed() {
        for k in [vk::LWIN, vk::RWIN] {
            assert_eq!(decide(KeyEvent::down(k), Modifiers::default()), Swallow);
            assert_eq!(decide(KeyEvent::up(k), Modifiers::default()), Swallow);
        }
    }

    #[test]
    fn a_windows_chord_is_swallowed() {
        let mut f = KeyFilter::default();
        assert_eq!(
            feed(
                &mut f,
                &[
                    KeyEvent::down(vk::LWIN),
                    KeyEvent::down(vk::D),
                    KeyEvent::up(vk::D),
                    KeyEvent::up(vk::LWIN),
                ]
            ),
            [Swallow; 4]
        );
        // Released: D types again.
        assert_eq!(f.on_event(KeyEvent::down(vk::D), false), Pass);
    }

    #[test]
    fn alt_f4_and_the_switchers_are_swallowed() {
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        for k in [vk::F4, vk::TAB, vk::ESCAPE, vk::SPACE] {
            assert_eq!(decide(KeyEvent::down(k), alt), Swallow, "{k:#x}");
        }
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(decide(KeyEvent::down(vk::ESCAPE), ctrl), Swallow);
        let mut f = KeyFilter::default();
        assert_eq!(
            feed(&mut f, &[KeyEvent::down(vk::LMENU), KeyEvent::down(vk::F4)]),
            [Pass, Swallow]
        );
        // The hook's Alt bit counts even when the Alt press was not seen.
        let mut f = KeyFilter::default();
        assert_eq!(f.on_event(KeyEvent::down(vk::TAB), true), Swallow);
    }

    #[test]
    fn the_admin_chord_passes() {
        let mut f = KeyFilter::default();
        assert_eq!(
            feed(
                &mut f,
                &[
                    KeyEvent::down(vk::LCONTROL),
                    KeyEvent::down(vk::LMENU),
                    KeyEvent::down(vk::LSHIFT),
                    KeyEvent::down(vk::A),
                    KeyEvent::up(vk::A),
                ]
            ),
            [Pass; 5]
        );
        // With a Windows key held it is a Windows chord.
        let all = Modifiers {
            ctrl: true,
            alt: true,
            shift: true,
            win: true,
        };
        assert_eq!(decide(KeyEvent::down(vk::A), all), Swallow);
    }

    #[test]
    fn a_missed_win_key_up_is_corrected_by_a_resync() {
        let mut f = KeyFilter::default();
        // Win went down, then up happened off-window and the hook never saw it.
        assert_eq!(f.on_event(KeyEvent::down(vk::LWIN), false), Swallow);
        // Without a resync the filter still thinks Win is held, so D would be swallowed.
        assert!(f.modifiers().win);
        // The OS says nothing is held: the resync clears it and D types.
        assert_eq!(
            f.on_event_synced(KeyEvent::down(vk::D), false, Modifiers::default()),
            Pass
        );
        assert!(!f.modifiers().win);
    }

    #[test]
    fn a_win_or_ctrl_held_before_install_is_seen_via_resync() {
        let mut f = KeyFilter::default();
        // Win was already down when the hook came up; the first event the hook sees is D.
        let win = Modifiers {
            win: true,
            ..Default::default()
        };
        assert_eq!(
            f.on_event_synced(KeyEvent::down(vk::D), false, win),
            Swallow
        );
        // Ctrl pre-held turns Esc into a swallowed chord.
        let mut f = KeyFilter::default();
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            f.on_event_synced(KeyEvent::down(vk::ESCAPE), false, ctrl),
            Swallow
        );
    }

    #[test]
    fn plain_typing_passes() {
        let mut f = KeyFilter::default();
        for k in (0x41..=0x5A).chain([vk::SPACE, vk::TAB, vk::ESCAPE, vk::F4]) {
            assert_eq!(f.on_event(KeyEvent::down(k), false), Pass, "{k:#x}");
            assert_eq!(f.on_event(KeyEvent::up(k), false), Pass, "{k:#x}");
        }
        // Shift and Ctrl alone are not blocked.
        assert_eq!(
            feed(
                &mut f,
                &[
                    KeyEvent::down(vk::LSHIFT),
                    KeyEvent::down(vk::A),
                    KeyEvent::up(vk::LSHIFT),
                    KeyEvent::down(vk::RCONTROL),
                    KeyEvent::down(vk::A),
                ]
            ),
            [Pass; 5]
        );
    }
}

//! A scripted PC for tests: it holds a PC's settings in memory, records what it was asked and
//! fails a call on request.

use std::collections::HashSet;
use std::sync::Mutex;

use arena_proto::PcLayout;

use super::{PcSettingsHost, PcSnapshot};

struct State {
    pc: PcSnapshot,
    calls: Vec<String>,
    failing: HashSet<&'static str>,
}

/// The fake PC: a 1920x1080@60 monitor that also does 144 Hz and 2560x1440, speed 10, two sound
/// outputs and two keyboard layouts.
pub struct FakeHost(Mutex<State>);

impl Default for FakeHost {
    fn default() -> Self {
        Self(Mutex::new(State {
            pc: PcSnapshot {
                mouse_speed: 10,
                display_mode: "1920x1080@60".into(),
                display_modes: vec![
                    "1280x720@60".into(),
                    "1920x1080@60".into(),
                    "1920x1080@144".into(),
                    "2560x1440@60".into(),
                ],
                audio_outputs: vec!["Speakers".into(), "Headset".into()],
                audio_output: Some("Speakers".into()),
                keyboard_layouts: vec![
                    PcLayout {
                        id: "04090409".into(),
                        name: "English (United States)".into(),
                    },
                    PcLayout {
                        id: "04070407".into(),
                        name: "German (Germany)".into(),
                    },
                ],
                keyboard_layout: Some("04090409".into()),
            },
            calls: Vec::new(),
            failing: HashSet::new(),
        }))
    }
}

impl FakeHost {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the PC is set to now.
    pub fn pc(&self) -> PcSnapshot {
        self.state().pc.clone()
    }

    /// Changes what the PC offers or is set to.
    pub fn with(&self, f: impl FnOnce(&mut PcSnapshot)) {
        f(&mut self.state().pc);
    }

    /// Every `set_*` asked so far, in order (`mouse=14`, `display=1920x1080@144`, `audio=Headset`,
    /// `layout=04070407`), failed ones included.
    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    /// Makes every later `mouse`, `display`, `audio` or `layout` call fail (or work again).
    pub fn fail(&self, what: &'static str, failing: bool) {
        let mut state = self.state();
        if failing {
            state.failing.insert(what);
        } else {
            state.failing.remove(what);
        }
    }

    fn call(&self, what: &'static str, value: String) -> Result<(), String> {
        let mut state = self.state();
        state.calls.push(format!("{what}={value}"));
        if state.failing.contains(what) {
            return Err(format!("{what} failed"));
        }
        Ok(())
    }
}

impl PcSettingsHost for FakeHost {
    fn snapshot(&self) -> Result<PcSnapshot, String> {
        Ok(self.state().pc.clone())
    }

    fn set_mouse_speed(&self, speed: u8) -> Result<(), String> {
        self.call("mouse", speed.to_string())?;
        self.state().pc.mouse_speed = speed;
        Ok(())
    }

    fn set_display_mode(&self, mode: &str) -> Result<(), String> {
        self.call("display", mode.to_owned())?;
        self.state().pc.display_mode = mode.to_owned();
        Ok(())
    }

    fn set_audio_output(&self, name: &str) -> Result<(), String> {
        self.call("audio", name.to_owned())?;
        self.state().pc.audio_output = Some(name.to_owned());
        Ok(())
    }

    fn set_keyboard_layout(&self, id: &str) -> Result<(), String> {
        self.call("layout", id.to_owned())?;
        self.state().pc.keyboard_layout = Some(id.to_owned());
        Ok(())
    }
}

//! A scripted host for tests (and any host with no cleanup of its own): it records what it was
//! asked and fails a step a set number of times on request.

use std::collections::HashMap;
use std::sync::Mutex;

use super::CleanupHost;

#[derive(Default)]
struct State {
    calls: Vec<String>,
    /// Failures still to come, per step name.
    failures: HashMap<&'static str, u32>,
    volume: Option<u8>,
}

/// The fake host.
#[derive(Default)]
pub struct FakeHost(Mutex<State>);

impl FakeHost {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The next `times` runs of `step` (`clipboard`, `temp_files`, `volume`) fail.
    pub fn fail(&self, step: &'static str, times: u32) {
        self.state().failures.insert(step, times);
    }

    /// The steps asked so far, in order (`clipboard`, `temp_files`, `volume=40`), failed ones
    /// included.
    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    /// The volume last set.
    pub fn volume(&self) -> Option<u8> {
        self.state().volume
    }

    fn step(&self, step: &'static str, call: String) -> Result<(), String> {
        let mut s = self.state();
        s.calls.push(call);
        match s.failures.get_mut(step) {
            Some(n) if *n > 0 => {
                *n -= 1;
                Err(format!("fake: {step} failed"))
            }
            _ => Ok(()),
        }
    }
}

impl CleanupHost for FakeHost {
    fn clear_clipboard(&self) -> Result<(), String> {
        self.step(super::STEP_CLIPBOARD, "clipboard".into())
    }

    fn clear_temp_files(&self) -> Result<(), String> {
        self.step(super::STEP_TEMP_FILES, "temp_files".into())
    }

    fn set_volume(&self, percent: u8) -> Result<(), String> {
        self.step(super::STEP_VOLUME, format!("volume={percent}"))?;
        self.state().volume = Some(percent);
        Ok(())
    }
}

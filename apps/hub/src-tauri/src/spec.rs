use std::io;
use std::path::Path;

use arena_proto::{DeviceSpec, check_device, meminfo_total_bytes};

pub const DB_FILE: &str = "branch.db";

pub fn free_bytes(dir: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

pub fn ram_bytes() -> io::Result<u64> {
    let text = std::fs::read_to_string("/proc/meminfo")?;
    meminfo_total_bytes(&text)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no MemTotal in /proc/meminfo"))
}

pub fn probe(dir: &Path) -> io::Result<DeviceSpec> {
    Ok(check_device(ram_bytes()?, free_bytes(dir)?))
}

#[derive(Default)]
pub struct Gate {
    started: bool,
}

impl Gate {
    pub fn stopped(&mut self) {
        self.started = false;
    }

    pub fn run(
        &mut self,
        dir: &Path,
        probe: impl FnOnce(&Path) -> io::Result<DeviceSpec>,
        start: impl FnOnce(),
        log: impl FnOnce(&str),
    ) -> DeviceSpec {
        let pass = DeviceSpec { failures: vec![] };
        if self.started {
            return pass;
        }
        if !dir.join(DB_FILE).exists() {
            match probe(dir) {
                Ok(spec) if !spec.ok() => return spec,
                Ok(_) => {}
                Err(e) => log(&format!("device spec probe failed, starting anyway: {e}")),
            }
        }
        self.started = true;
        start();
        pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn failing() -> DeviceSpec {
        check_device(1_000_000_000, 14_000_000_000)
    }

    fn run(dir: &Path, gate: &mut Gate, result: io::Result<DeviceSpec>) -> (DeviceSpec, u32) {
        let starts = Cell::new(0);
        let spec = gate.run(dir, |_| result, || starts.set(starts.get() + 1), |_| {});
        (spec, starts.get())
    }

    #[test]
    fn a_failing_device_does_not_start_the_hub_or_create_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let mut gate = Gate::default();
        let (spec, starts) = run(dir.path(), &mut gate, Ok(failing()));
        assert_eq!((spec.failures.len(), starts), (2, 0));
        assert!(!dir.path().join(DB_FILE).exists());
    }

    #[test]
    fn check_again_starts_the_hub_once_the_device_passes_and_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut gate = Gate::default();
        run(dir.path(), &mut gate, Ok(failing()));
        let ok = check_device(4_000_000_000, 30_000_000_000);
        let (spec, starts) = run(dir.path(), &mut gate, Ok(ok));
        assert_eq!((spec.ok(), starts), (true, 1));
        let (spec, starts) = run(dir.path(), &mut gate, Ok(failing()));
        assert_eq!((spec.ok(), starts), (true, 0));
    }

    #[test]
    fn a_branch_that_already_has_its_database_is_never_blocked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(DB_FILE), b"").unwrap();
        let (spec, starts) = run(dir.path(), &mut Gate::default(), Ok(failing()));
        assert_eq!((spec.ok(), starts), (true, 1));
    }

    #[test]
    fn a_probe_that_cannot_read_the_device_starts_the_hub_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let logged = Cell::new(false);
        let starts = Cell::new(0);
        let spec = Gate::default().run(
            dir.path(),
            |_| Err(io::Error::other("no meminfo")),
            || starts.set(1),
            |m| logged.set(m.contains("no meminfo")),
        );
        assert_eq!((spec.ok(), starts.get(), logged.get()), (true, 1, true));
    }

    #[test]
    fn the_probe_reads_this_device_where_proc_meminfo_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            probe(dir.path()).is_ok(),
            Path::new("/proc/meminfo").exists()
        );
    }

    #[test]
    fn a_hub_that_exited_is_started_again_by_the_next_check() {
        let dir = tempfile::tempdir().unwrap();
        let mut gate = Gate::default();
        let ok = || Ok(check_device(4_000_000_000, 30_000_000_000));
        assert_eq!(run(dir.path(), &mut gate, ok()).1, 1);
        assert_eq!(run(dir.path(), &mut gate, ok()).1, 0);
        gate.stopped();
        assert_eq!(run(dir.path(), &mut gate, ok()).1, 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_probe_reads_real_values_on_linux() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ram_bytes().unwrap() > 0);
        assert!(free_bytes(dir.path()).unwrap() > 0);
    }

    #[test]
    fn free_bytes_reads_the_volume_of_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_bytes(dir.path()).unwrap() > 0);
        assert!(free_bytes(&dir.path().join("missing")).is_err());
    }
}

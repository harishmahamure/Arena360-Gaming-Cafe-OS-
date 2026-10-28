//! The Windows side of the scan: where Steam, Epic, Riot and the common folders are on this PC,
//! and the version info of an executable (`ProductName`, `CompanyName`).
//!
//! - **Steam:** `InstallPath` under `HKLM\SOFTWARE\WOW6432Node\Valve\Steam` (then
//!   `HKLM\SOFTWARE\Valve\Steam`), then `Program Files (x86)\Steam`.
//! - **Epic and Riot:** under `%ProgramData%`.
//! - **Folders:** `%ProgramFiles%`, `%ProgramFiles(x86)%`, `C:\Games` and `X:\Games` on every
//!   fixed drive (`GetDriveTypeW`).
//! - **Version info:** `GetFileVersionInfoW` and `VerQueryValueW`, which read the file's
//!   resources without running it.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetFileVersionInfoSizeW, GetFileVersionInfoW, GetLogicalDrives, VerQueryValueW,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::PCWSTR;

use super::{Locations, ScanFs, ScanHost, StdFs, VersionInfo};

/// `DRIVE_FIXED`.
const DRIVE_FIXED: u32 = 3;

/// The real PC.
#[derive(Debug, Default)]
pub struct WinHost {
    fs: StdFs,
}

impl WinHost {
    pub fn new() -> Self {
        Self::default()
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn env_path(name: &str, fallback: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(fallback))
}

/// A string value of `HKLM\<sub>`, if it exists.
fn registry_string(sub: &str, name: &str) -> Option<String> {
    let sub = wide(sub);
    let name = wide(name);
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `len` bytes; the key and value names are NUL-terminated.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast::<c_void>()),
            Some(&mut len),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let units = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..units]);
    let s = s.trim_end_matches('\0').trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// The roots (`C:\`) of the fixed drives.
fn fixed_drives() -> Vec<PathBuf> {
    // SAFETY: plain queries.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", char::from(b'A' + i)))
        .filter(|root| {
            let w = wide(root);
            // SAFETY: `w` is NUL-terminated.
            unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) == DRIVE_FIXED }
        })
        .map(PathBuf::from)
        .collect()
}

impl ScanHost for WinHost {
    fn fs(&self) -> &dyn ScanFs {
        &self.fs
    }

    fn locations(&self) -> Locations {
        let pf = env_path("ProgramFiles", r"C:\Program Files");
        let pf86 = env_path("ProgramFiles(x86)", r"C:\Program Files (x86)");
        let data = env_path("ProgramData", r"C:\ProgramData");
        let mut steam_roots: Vec<PathBuf> =
            [r"SOFTWARE\WOW6432Node\Valve\Steam", r"SOFTWARE\Valve\Steam"]
                .iter()
                .filter_map(|k| registry_string(k, "InstallPath"))
                .map(PathBuf::from)
                .collect();
        steam_roots.push(pf86.join("Steam"));
        let mut game_folders = vec![PathBuf::from(r"C:\Games")];
        for root in fixed_drives() {
            let games = root.join("Games");
            if !game_folders.iter().any(|g| {
                g.to_string_lossy()
                    .eq_ignore_ascii_case(&games.to_string_lossy())
            }) {
                game_folders.push(games);
            }
        }
        let mut program_folders = vec![pf.clone()];
        if pf86 != pf {
            program_folders.push(pf86.clone());
        }
        Locations {
            steam_roots,
            epic_manifests: data
                .join("Epic")
                .join("EpicGamesLauncher")
                .join("Data")
                .join("Manifests"),
            epic_launcher_dirs: vec![
                pf86.join("Epic Games").join("Launcher"),
                pf.join("Epic Games").join("Launcher"),
            ],
            riot_installs: data.join("Riot Games").join("RiotClientInstalls.json"),
            game_folders,
            program_folders,
        }
    }
}

/// A string of a version resource's `StringFileInfo\<lang>` block.
fn query_string(block: &[u8], lang: &str, name: &str) -> Option<String> {
    let sub = wide(&format!("\\StringFileInfo\\{lang}\\{name}"));
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: `block` is a version resource read by GetFileVersionInfoW; on success `ptr`
    // points inside it and holds `len` UTF-16 units.
    let ok = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(sub.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    if !ok.as_bool() || ptr.is_null() || len == 0 {
        return None;
    }
    // SAFETY: see above; `len` is in characters and the read is bounded by it.
    let units = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize) };
    let s = String::from_utf16_lossy(units);
    let s = s.trim_end_matches('\0').trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// The language and code page pairs the resource declares (`\VarFileInfo\Translation`), as
/// the `XXXXXXXX` hex of `\StringFileInfo`.
fn translations(block: &[u8]) -> Vec<String> {
    let sub = wide("\\VarFileInfo\\Translation");
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: as in `query_string`; here `len` is in bytes.
    let ok = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(sub.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    let mut out = Vec::new();
    if ok.as_bool() && !ptr.is_null() {
        // SAFETY: `ptr` holds `len` bytes of (language, code page) u16 pairs.
        let words = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize / 2) };
        for pair in words.chunks_exact(2).take(4) {
            out.push(format!("{:04x}{:04x}", pair[0], pair[1]));
        }
    }
    // Common fallbacks: US English Unicode and Windows-1252, then neutral.
    for fallback in ["040904b0", "040904e4", "000004b0"] {
        if !out.iter().any(|t| t == fallback) {
            out.push(fallback.to_owned());
        }
    }
    out
}

/// The product name and publisher of the executable at `path` (empty when it has no version
/// info or cannot be read).
pub fn version_info(path: &Path) -> VersionInfo {
    let file = wide(&path.to_string_lossy());
    // SAFETY: `file` is NUL-terminated.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None) };
    // A resource over 1 MiB is not a sane version block.
    if size == 0 || size > 1 << 20 {
        return VersionInfo::default();
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` holds `size` bytes.
    let read = unsafe {
        GetFileVersionInfoW(
            PCWSTR(file.as_ptr()),
            None,
            size,
            block.as_mut_ptr().cast::<c_void>(),
        )
    };
    if read.is_err() {
        return VersionInfo::default();
    }
    let langs = translations(&block);
    let first = |name: &str| langs.iter().find_map(|l| query_string(&block, l, name));
    VersionInfo {
        product_name: first("ProductName").unwrap_or_default(),
        publisher: first("CompanyName").unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notepad_has_version_info_and_a_missing_file_has_none() {
        let notepad = Path::new(r"C:\Windows\System32\notepad.exe");
        if notepad.exists() {
            let info = version_info(notepad);
            assert!(info.publisher.contains("Microsoft"), "{info:?}");
            assert!(!info.product_name.is_empty(), "{info:?}");
        }
        assert_eq!(
            version_info(Path::new(r"C:\no\such\file.exe")),
            VersionInfo::default()
        );
    }

    #[test]
    fn the_locations_name_the_common_folders() {
        let loc = WinHost::new().locations();
        assert!(loc.game_folders.iter().any(|g| g.ends_with("Games")));
        assert!(!loc.program_folders.is_empty());
        assert!(loc.epic_manifests.ends_with("Manifests"));
        assert!(loc.riot_installs.ends_with("RiotClientInstalls.json"));
        assert!(loc.steam_roots.last().unwrap().ends_with("Steam"));
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    struct Never;
    impl crate::scan::Pause for Never {
        fn checkpoint(&self) -> Result<(), crate::scan::Cancelled> {
            Ok(())
        }
    }

    /// A manual smoke run on the machine at hand: `cargo test -p arena-kiosk-svc
    /// scan::windows::smoke -- --ignored --nocapture`.
    #[test]
    #[ignore = "scans this PC; run by hand"]
    fn scan_this_pc() {
        let started = std::time::Instant::now();
        let r = crate::scan::scan(&WinHost::new(), &Never).unwrap();
        println!(
            "{} entries, {} skipped, launchers {:?}, in {:?}",
            r.entries.len(),
            r.skipped,
            r.launchers,
            started.elapsed()
        );
        for e in r.entries.iter().take(15) {
            println!(
                "{} | {} | {} | {}",
                e.launcher, e.path, e.product_name, e.publisher
            );
        }
        r.into_report(i64::MAX / 2).validate(i64::MAX / 2).unwrap();
    }
}

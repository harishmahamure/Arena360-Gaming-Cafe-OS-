//! The installed-software scan (Story 6.4).
//!
//! The service looks at its own PC for games and launchers and reports what it found as the
//! device intent `kiosk.scan_reported` ([`schedule`] decides when). The walker here reads
//! nothing but directory listings, a few small manifest files and each executable's version
//! info (product name and publisher); it never hashes, runs, installs or sends the content of
//! a file.
//!
//! **Sources**, in this order (the first source to find a path keeps it; paths are compared
//! without case and at most [`MAX_SCAN_ENTRIES`] are kept):
//! 1. Steam: the install folder (registry, then the default location) when it holds
//!    `steam.exe`; `steamapps\libraryfolders.vdf` names more library folders; every
//!    `appmanifest_*.acf` names an `installdir` under `steamapps\common`.
//! 2. Epic: `ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item` (`InstallLocation`,
//!    `LaunchExecutable`).
//! 3. Riot: `ProgramData\Riot Games\RiotClientInstalls.json` (the client and the game folders
//!    in `associated_client`).
//! 4. Folders: `C:\Games` and each fixed drive's `Games` folder, then Program Files and
//!    Program Files (x86) (Windows' own folders are skipped).
//!
//! Installer, uninstaller, crash-handler and redistributable executables are left out. A folder
//! or file that cannot be read is logged and counted in `skipped`, and the scan goes on; one
//! that does not exist is not an error. The scan runs on a blocking thread and calls
//! [`Pause::checkpoint`] before every folder it lists, which is where it waits while a session
//! is on the PC.

pub mod schedule;
#[cfg(windows)]
pub mod windows;

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use arena_proto::{
    KioskScanReported, MAX_SCAN_ENTRIES, MAX_SCAN_FILE_NAME_CHARS, MAX_SCAN_PATH_CHARS,
    MAX_SCAN_TEXT_CHARS, ScanEntry, ScanLauncher,
};

/// What kind of thing a directory entry is (symbolic links and junctions are `Other`: they are
/// never followed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Dir,
    Other,
}

/// One entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub path: PathBuf,
    pub kind: ItemKind,
}

/// A folder listing. `unreadable` counts entries the OS refused to describe.
#[derive(Debug, Default)]
pub struct DirListing {
    pub items: Vec<DirItem>,
    pub unreadable: u32,
}

/// The `ProductName` and `CompanyName` of an executable's version info (empty when it has
/// none).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub product_name: String,
    pub publisher: String,
}

/// The file system the scan reads (tests put a fake behind it).
pub trait ScanFs: Send + Sync {
    /// Lists `dir`. `NotFound` means it does not exist; any other error means it cannot be read.
    fn list(&self, dir: &Path) -> io::Result<DirListing>;
    /// Reads a small text file (at most [`MAX_TEXT_BYTES`]).
    fn read_text(&self, file: &Path) -> io::Result<String>;
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn version_info(&self, file: &Path) -> VersionInfo;
}

/// The real file system. Version info is read on Windows only (elsewhere it is empty).
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFs;

impl ScanFs for StdFs {
    fn list(&self, dir: &Path) -> io::Result<DirListing> {
        let mut listing = DirListing::default();
        for entry in std::fs::read_dir(dir)? {
            let item = entry.and_then(|e| {
                let kind = e.file_type().map(|t| {
                    if t.is_file() {
                        ItemKind::File
                    } else if t.is_dir() {
                        ItemKind::Dir
                    } else {
                        ItemKind::Other
                    }
                })?;
                Ok(DirItem {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: e.path(),
                    kind,
                })
            });
            match item {
                Ok(i) => listing.items.push(i),
                Err(_) => listing.unreadable += 1,
            }
        }
        Ok(listing)
    }

    fn read_text(&self, file: &Path) -> io::Result<String> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(file)?
            .take(MAX_TEXT_BYTES)
            .read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(text.trim_start_matches('\u{feff}').to_owned())
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn version_info(&self, file: &Path) -> VersionInfo {
        #[cfg(windows)]
        {
            windows::version_info(file)
        }
        #[cfg(not(windows))]
        {
            let _ = file;
            VersionInfo::default()
        }
    }
}

/// Where the sources are on this PC.
#[derive(Debug, Clone, Default)]
pub struct Locations {
    /// Candidate Steam install folders: the registry's, then the default one.
    pub steam_roots: Vec<PathBuf>,
    /// `...\Epic\EpicGamesLauncher\Data\Manifests`.
    pub epic_manifests: PathBuf,
    /// Candidate Epic launcher install folders.
    pub epic_launcher_dirs: Vec<PathBuf>,
    /// `...\Riot Games\RiotClientInstalls.json`.
    pub riot_installs: PathBuf,
    /// `C:\Games` and each fixed drive's `Games` folder.
    pub game_folders: Vec<PathBuf>,
    /// Program Files and Program Files (x86).
    pub program_folders: Vec<PathBuf>,
}

/// A PC to scan: its file system and where the sources are.
pub trait ScanHost: Send + Sync {
    fn fs(&self) -> &dyn ScanFs;
    fn locations(&self) -> Locations;
}

/// The scan was cancelled (the service is stopping); nothing is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// Where the scan waits. Called before every folder is listed: it returns once the scan may
/// go on (no session is on the PC) or `Err(Cancelled)` when it must stop.
pub trait Pause {
    fn checkpoint(&self) -> Result<(), Cancelled>;
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    pub entries: Vec<ScanEntry>,
    pub launchers: Vec<ScanLauncher>,
    /// Folders and files that could not be read.
    pub skipped: u32,
}

impl ScanResult {
    /// The report of this result, finished at `scanned_at`.
    pub fn into_report(self, scanned_at: i64) -> KioskScanReported {
        KioskScanReported {
            scanned_at,
            entries: self.entries,
            launchers: self.launchers,
            skipped: self.skipped,
        }
    }
}

/// The largest manifest or JSON file the scan reads.
pub const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
/// How deep a launcher's game folder is walked (the folder itself is depth 0).
pub const LAUNCHER_DEPTH: u32 = 5;
/// How deep `Games` folders are walked.
pub const GAMES_DEPTH: u32 = 5;
/// How deep Program Files folders are walked.
pub const PROGRAM_DEPTH: u32 = 3;

/// Folders of Program Files that hold Windows itself, other launchers' own files or nothing a
/// player runs (compared without case; `*` ends a prefix).
const PROGRAM_SKIP: &[&str] = &[
    "common files",
    "windows*",
    "microsoft*",
    "internet explorer",
    "reference assemblies",
    "dotnet",
    "modifiablewindowsapps",
    "steam",
    "epic games",
    "riot client",
    "arena*",
];

fn skipped_folder(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    PROGRAM_SKIP.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *p,
    })
}

/// Whether an executable's file name is an installer, uninstaller, crash handler or
/// redistributable (compared without case).
pub fn is_noise_exe(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    const CONTAINS: &[&str] = &[
        "unins",
        "uninst",
        "setup",
        "install",
        "crash",
        "redist",
        "dxsetup",
        "dxwebsetup",
        "dotnet",
        "ndp4",
        "oalinst",
        "prereq",
        "vcredist",
        "vc_redist",
    ];
    CONTAINS.iter().any(|p| name.contains(p))
}

fn is_exe(name: &str) -> bool {
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe")
}

/// Text from a version resource: control characters become spaces, then it is trimmed and cut
/// to the report's limit.
fn clean_text(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().chars().take(MAX_SCAN_TEXT_CHARS).collect()
}

/// Whether `s` is a single file or folder name (no separators, no `..`, no drive).
fn is_plain_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['\\', '/', ':']) && !s.contains('\0')
}

// ---- VDF (Steam's key-value text format) ----

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn vdf_tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                out.push(Token::Str(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every value of the key `key` (any case, at any depth) in VDF `text`.
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut tokens = vdf_tokens(text).into_iter().peekable();
    while let Some(t) = tokens.next() {
        if let Token::Str(k) = t
            && matches!(tokens.peek(), Some(Token::Str(_)))
            && let Some(Token::Str(v)) = tokens.next()
            && k.eq_ignore_ascii_case(key)
        {
            out.push(v);
        }
    }
    out
}

// ---- the walker ----

struct Walker<'a> {
    fs: &'a dyn ScanFs,
    pause: &'a dyn Pause,
    entries: Vec<ScanEntry>,
    seen: HashSet<String>,
    skipped: u32,
}

impl<'a> Walker<'a> {
    fn full(&self) -> bool {
        self.entries.len() >= MAX_SCAN_ENTRIES
    }

    fn skip(&mut self, what: &str, path: &Path, error: &io::Error) {
        tracing::warn!(path = %path.display(), error = %error, "scan: {what}; skipped");
        self.skipped = self.skipped.saturating_add(1);
    }

    /// The items of `dir` (after the pause point), or `None` when it is missing or unreadable
    /// (an unreadable one is counted).
    fn list(&mut self, dir: &Path) -> Result<Option<Vec<DirItem>>, Cancelled> {
        self.pause.checkpoint()?;
        match self.fs.list(dir) {
            Ok(listing) => {
                if listing.unreadable > 0 {
                    tracing::warn!(path = %dir.display(), entries = listing.unreadable, "scan: entries could not be read; skipped");
                    self.skipped = self.skipped.saturating_add(listing.unreadable);
                }
                Ok(Some(listing.items))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                self.skip("folder cannot be read", dir, &e);
                Ok(None)
            }
        }
    }

    /// A small text file, or `None` when it is missing or unreadable (an unreadable one is
    /// counted).
    fn read(&mut self, file: &Path) -> Option<String> {
        match self.fs.read_text(file) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                self.skip("file cannot be read", file, &e);
                None
            }
        }
    }

    /// Adds the executable at `path` unless it is noise, too long, or already found.
    fn add(&mut self, path: &Path, name: &str, launcher: &str) {
        if self.full() || !is_exe(name) || is_noise_exe(name) {
            return;
        }
        let full = shown(path);
        if full.chars().count() > MAX_SCAN_PATH_CHARS
            || name.chars().count() > MAX_SCAN_FILE_NAME_CHARS
            || full.chars().any(char::is_control)
        {
            return;
        }
        if !self.seen.insert(full.to_lowercase()) {
            return;
        }
        let info = self.fs.version_info(path);
        self.entries.push(ScanEntry {
            path: full,
            file_name: name.to_owned(),
            product_name: clean_text(&info.product_name),
            publisher: clean_text(&info.publisher),
            launcher: launcher.to_owned(),
        });
    }

    /// Walks `dir` (executables first, then sub-folders in name order) down to `max_depth`.
    fn walk(
        &mut self,
        dir: &Path,
        depth: u32,
        max_depth: u32,
        launcher: &str,
        skip_dir: &dyn Fn(&str) -> bool,
    ) -> Result<(), Cancelled> {
        if self.full() {
            return Ok(());
        }
        let Some(mut items) = self.list(dir)? else {
            return Ok(());
        };
        items.sort_by_key(|i| i.name.to_lowercase());
        for item in items.iter().filter(|i| i.kind == ItemKind::File) {
            self.add(&item.path, &item.name, launcher);
        }
        if depth >= max_depth {
            return Ok(());
        }
        for item in items.iter().filter(|i| i.kind == ItemKind::Dir) {
            if self.full() {
                break;
            }
            if !skip_dir(&item.name) {
                self.walk(&item.path, depth + 1, max_depth, launcher, skip_dir)?;
            }
        }
        Ok(())
    }

    fn walk_game(&mut self, dir: &Path, launcher: &str) -> Result<(), Cancelled> {
        self.walk(dir, 0, LAUNCHER_DEPTH, launcher, &|_| false)
    }

    // ---- sources ----

    fn steam(&mut self, roots: &[PathBuf]) -> Result<Option<ScanLauncher>, Cancelled> {
        let Some(root) = roots.iter().find(|r| self.fs.is_file(&r.join("steam.exe"))) else {
            return Ok(None);
        };
        let launcher = launcher_of("steam", root);
        let mut libraries: Vec<PathBuf> = vec![root.clone()];
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Some(text) = self.read(&vdf) {
            for p in vdf_values(&text, "path") {
                let p = PathBuf::from(p);
                if p.is_absolute() && !libraries.iter().any(|l| same_path(l, &p)) {
                    libraries.push(p);
                }
            }
        }
        for lib in libraries {
            if self.full() {
                break;
            }
            let apps = lib.join("steamapps");
            let Some(mut items) = self.list(&apps)? else {
                continue;
            };
            items.sort_by_key(|i| i.name.to_lowercase());
            for m in items.iter().filter(|i| {
                i.kind == ItemKind::File && {
                    let n = i.name.to_ascii_lowercase();
                    n.starts_with("appmanifest_") && n.ends_with(".acf")
                }
            }) {
                if self.full() {
                    break;
                }
                let Some(text) = self.read(&m.path) else {
                    continue;
                };
                let Some(dir) = vdf_values(&text, "installdir")
                    .into_iter()
                    .find(|d| is_plain_name(d))
                else {
                    tracing::warn!(path = %m.path.display(), "scan: a Steam manifest names no usable install folder");
                    continue;
                };
                self.walk_game(&apps.join("common").join(dir), "steam")?;
            }
        }
        Ok(launcher)
    }

    fn epic(&mut self, loc: &Locations) -> Result<Option<ScanLauncher>, Cancelled> {
        let launcher_dir = loc.epic_launcher_dirs.iter().find(|d| self.fs.is_dir(d));
        let manifests = self.list(&loc.epic_manifests)?;
        let installed = launcher_dir.is_some() || manifests.is_some();
        if let Some(mut items) = manifests {
            items.sort_by_key(|i| i.name.to_lowercase());
            for m in items.iter().filter(|i| {
                i.kind == ItemKind::File && i.name.to_ascii_lowercase().ends_with(".item")
            }) {
                if self.full() {
                    break;
                }
                let Some(text) = self.read(&m.path) else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                    self.skip(
                        "manifest is not valid JSON",
                        &m.path,
                        &io::Error::from(io::ErrorKind::InvalidData),
                    );
                    continue;
                };
                let text_of = |k: &str| json.get(k).and_then(|v| v.as_str()).unwrap_or("").trim();
                let install = text_of("InstallLocation");
                if install.is_empty() {
                    continue;
                }
                let install = PathBuf::from(install);
                let exe = text_of("LaunchExecutable");
                if is_plain_or_relative(exe) {
                    let path = exe
                        .split(['\\', '/'])
                        .fold(install.clone(), |p, part| p.join(part));
                    if self.fs.is_file(&path) {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.add(&path, &name, "epic");
                    }
                }
                self.walk_game(&install, "epic")?;
            }
        }
        let path = launcher_dir.cloned().or_else(|| {
            installed.then(|| {
                loc.epic_manifests
                    .ancestors()
                    .nth(2)
                    .unwrap_or(&loc.epic_manifests)
                    .to_path_buf()
            })
        });
        Ok(path.and_then(|p| launcher_of("epic", &p)))
    }

    fn riot(&mut self, loc: &Locations) -> Result<Option<ScanLauncher>, Cancelled> {
        let Some(text) = self.read(&loc.riot_installs) else {
            return Ok(None);
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            self.skip(
                "Riot install list is not valid JSON",
                &loc.riot_installs,
                &io::Error::from(io::ErrorKind::InvalidData),
            );
            return Ok(None);
        };
        let client = ["rc_default", "rc_live", "rc_beta"]
            .iter()
            .filter_map(|k| json.get(k).and_then(|v| v.as_str()))
            .find(|s| !s.trim().is_empty());
        let games: Vec<String> = json
            .get("associated_client")
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let folder = client
            .and_then(|c| Path::new(c).parent().map(Path::to_path_buf))
            .or_else(|| {
                games
                    .first()
                    .and_then(|g| Path::new(g.trim_end_matches(['\\', '/'])).parent())
                    .map(Path::to_path_buf)
            });
        let launcher = folder.and_then(|f| launcher_of("riot", &f));
        for game in games {
            if self.full() {
                break;
            }
            let dir = PathBuf::from(game);
            if dir.is_absolute() {
                self.walk_game(&dir, "riot")?;
            }
        }
        Ok(launcher)
    }
}

/// A launcher record for `path`, or `None` when the path is too long to report.
fn launcher_of(name: &str, path: &Path) -> Option<ScanLauncher> {
    let path = shown(path);
    let path = path.trim_end_matches(['\\', '/']).to_owned();
    let n = path.chars().count();
    if n == 0 || n > MAX_SCAN_PATH_CHARS || path.chars().any(char::is_control) {
        tracing::warn!(launcher = name, "scan: the install path cannot be reported");
        return None;
    }
    Some(ScanLauncher {
        launcher: name.to_owned(),
        path,
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    shown(a)
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(shown(b).trim_end_matches(['\\', '/']))
}

/// A path as it is reported: on Windows with backslashes only (Riot's list and Epic's
/// manifests use forward slashes), so the same file is never reported twice.
fn shown(path: &Path) -> String {
    let s = path.display().to_string();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s
    }
}

/// A relative path with no `..` part, e.g. `Game\Binaries\Win64\game.exe`.
fn is_plain_or_relative(p: &str) -> bool {
    !p.is_empty()
        && !p.contains(':')
        && !p.starts_with(['\\', '/'])
        && !p.split(['\\', '/']).any(|part| part == "..")
}

/// Scans `host`. Returns [`Cancelled`] when `pause` says so; nothing is reported then.
pub fn scan(host: &dyn ScanHost, pause: &dyn Pause) -> Result<ScanResult, Cancelled> {
    let loc = host.locations();
    let mut w = Walker {
        fs: host.fs(),
        pause,
        entries: Vec::new(),
        seen: HashSet::new(),
        skipped: 0,
    };
    let mut launchers = Vec::new();
    launchers.extend(w.steam(&loc.steam_roots)?);
    launchers.extend(w.epic(&loc)?);
    launchers.extend(w.riot(&loc)?);
    for dir in &loc.game_folders {
        w.walk(dir, 0, GAMES_DEPTH, "none", &|_| false)?;
    }
    for dir in &loc.program_folders {
        w.walk(dir, 0, PROGRAM_DEPTH, "none", &skipped_folder)?;
    }
    Ok(ScanResult {
        entries: w.entries,
        launchers,
        skipped: w.skipped,
    })
}

#[cfg(test)]
mod tests;

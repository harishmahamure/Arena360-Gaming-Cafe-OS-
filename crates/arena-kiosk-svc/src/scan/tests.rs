use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

/// A fake PC on a temp dir, with the real file system underneath.
struct Pc {
    _dir: tempfile::TempDir,
    root: PathBuf,
    fs: Wrapped,
    loc: Locations,
}

/// The real file system with optional refusals and version info.
#[derive(Default)]
struct Wrapped {
    /// Folders that refuse to be listed (access denied).
    denied: Mutex<Vec<PathBuf>>,
    /// Files that refuse to be read.
    unreadable: Mutex<Vec<PathBuf>>,
    versions: Mutex<HashMap<String, VersionInfo>>,
    lists: AtomicUsize,
}

impl ScanFs for Wrapped {
    fn list(&self, dir: &Path) -> io::Result<DirListing> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        if self.denied.lock().unwrap().iter().any(|d| d == dir) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        StdFs.list(dir)
    }

    fn read_text(&self, file: &Path) -> io::Result<String> {
        if self.unreadable.lock().unwrap().iter().any(|d| d == file) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        StdFs.read_text(file)
    }

    fn is_dir(&self, path: &Path) -> bool {
        StdFs.is_dir(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        StdFs.is_file(path)
    }

    fn version_info(&self, file: &Path) -> VersionInfo {
        let name = file.file_name().unwrap().to_string_lossy().to_lowercase();
        self.versions
            .lock()
            .unwrap()
            .get(&name)
            .cloned()
            .unwrap_or_default()
    }
}

impl ScanHost for Pc {
    fn fs(&self) -> &dyn ScanFs {
        &self.fs
    }

    fn locations(&self) -> Locations {
        self.loc.clone()
    }
}

struct NeverPause;

impl Pause for NeverPause {
    fn checkpoint(&self) -> Result<(), Cancelled> {
        Ok(())
    }
}

impl Pc {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let loc = Locations {
            steam_roots: vec![root.join("PF86").join("Steam")],
            epic_manifests: root
                .join("PD")
                .join("Epic")
                .join("EpicGamesLauncher")
                .join("Data")
                .join("Manifests"),
            epic_launcher_dirs: vec![root.join("PF86").join("Epic Games").join("Launcher")],
            riot_installs: root
                .join("PD")
                .join("Riot Games")
                .join("RiotClientInstalls.json"),
            game_folders: vec![root.join("Games"), root.join("D").join("Games")],
            program_folders: vec![root.join("PF"), root.join("PF86")],
        };
        Self {
            _dir: dir,
            root,
            fs: Wrapped::default(),
            loc,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root.clone(), |p, part| p.join(part))
    }

    fn file(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }

    fn scan(&self) -> ScanResult {
        scan(self, &NeverPause).unwrap()
    }

    /// `(launcher, path relative to the root with `/`)` of every entry, in report order.
    fn found(&self, r: &ScanResult) -> Vec<(String, String)> {
        r.entries
            .iter()
            .map(|e| {
                let rel = Path::new(&e.path).strip_prefix(&self.root).unwrap();
                (
                    e.launcher.clone(),
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                )
            })
            .collect()
    }

    /// A Steam install with its default library, `n` games installed.
    fn steam(&self) {
        self.file("PF86/Steam/steam.exe", "");
    }

    fn vdf_path(&self, rel: &str) -> String {
        self.path(rel).display().to_string().replace('\\', "\\\\")
    }
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect()
}

#[test]
fn a_pc_with_nothing_installed_reports_nothing_and_skips_nothing() {
    let pc = Pc::new();
    assert_eq!(pc.scan(), ScanResult::default());
}

#[test]
fn steam_games_come_from_the_library_folders_and_app_manifests() {
    let pc = Pc::new();
    pc.steam();
    // The default library has one game; a second library on another "drive" has another.
    pc.file(
        "PF86/Steam/steamapps/libraryfolders.vdf",
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"apps\" {{ \"730\" \"1\" }}\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            pc.vdf_path("PF86/Steam"),
            pc.vdf_path("D/SteamLibrary")
        ),
    );
    pc.file(
        "PF86/Steam/steamapps/appmanifest_730.acf",
        "\"AppState\"\n{\n\t\"appid\"\t\"730\"\n\t\"installdir\"\t\"Counter-Strike Global Offensive\"\n}\n",
    );
    pc.file(
        "PF86/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/win64/cs2.exe",
        "",
    );
    pc.file(
        "PF86/Steam/steamapps/common/Counter-Strike Global Offensive/UnityCrashHandler64.exe",
        "",
    );
    pc.file(
        "PF86/Steam/steamapps/common/Counter-Strike Global Offensive/readme.txt",
        "",
    );
    pc.file(
        "D/SteamLibrary/steamapps/appmanifest_570.acf",
        "\"AppState\" { \"installdir\" \"dota 2 beta\" }",
    );
    pc.file(
        "D/SteamLibrary/steamapps/common/dota 2 beta/game/bin/win64/dota2.exe",
        "",
    );
    pc.file(
        "D/SteamLibrary/steamapps/common/dota 2 beta/vc_redist.x64.exe",
        "",
    );
    let r = pc.scan();
    assert_eq!(
        pc.found(&r),
        pairs(&[
            (
                "steam",
                "PF86/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/win64/cs2.exe"
            ),
            (
                "steam",
                "D/SteamLibrary/steamapps/common/dota 2 beta/game/bin/win64/dota2.exe"
            ),
        ])
    );
    assert_eq!(r.launchers.len(), 1);
    assert_eq!(r.launchers[0].launcher, "steam");
    assert_eq!(
        Path::new(&r.launchers[0].path),
        pc.path("PF86/Steam").as_path()
    );
    assert_eq!(r.skipped, 0);
    assert_eq!(r.entries[0].file_name, "cs2.exe");
}

#[test]
fn steam_is_installed_only_when_steam_exe_is_there() {
    let pc = Pc::new();
    fs::create_dir_all(pc.path("PF86/Steam/steamapps")).unwrap();
    assert!(pc.scan().launchers.is_empty());
    pc.steam();
    assert_eq!(pc.scan().launchers.len(), 1);
}

#[test]
fn an_install_dir_that_climbs_out_of_common_is_ignored() {
    let pc = Pc::new();
    pc.steam();
    pc.file("PF86/Steam/evil/outside.exe", "");
    for bad in ["..\\\\evil", "../evil", "C:\\\\Windows", "..", "a/b"] {
        pc.file(
            "PF86/Steam/steamapps/appmanifest_1.acf",
            &format!("\"AppState\" {{ \"installdir\" \"{bad}\" }}"),
        );
        let r = pc.scan();
        assert!(
            r.entries
                .iter()
                .all(|e| !e.path.to_lowercase().contains("outside")),
            "{bad}: {:?}",
            r.entries
        );
    }
}

#[test]
fn epic_games_come_from_the_item_manifests_launch_executable_first() {
    let pc = Pc::new();
    fs::create_dir_all(pc.path("PF86/Epic Games/Launcher")).unwrap();
    let install = pc.path("E/Fortnite");
    pc.file("E/Fortnite/FortniteGame/Binaries/Win64/Fortnite.exe", "");
    pc.file("E/Fortnite/Engine/Binaries/ThirdParty/zzz-helper.exe", "");
    pc.file(
        "E/Fortnite/Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe",
        "",
    );
    let manifests = "PD/Epic/EpicGamesLauncher/Data/Manifests";
    pc.file(
        &format!("{manifests}/ABC.item"),
        &serde_json::json!({
            "DisplayName": "Fortnite",
            "InstallLocation": install,
            "LaunchExecutable": "FortniteGame/Binaries/Win64/Fortnite.exe",
        })
        .to_string(),
    );
    pc.file(&format!("{manifests}/readme.txt"), "not a manifest");
    // A manifest naming a launch executable that climbs out is not trusted.
    pc.file(
        &format!("{manifests}/EVIL.item"),
        &serde_json::json!({"InstallLocation": pc.path("E/Other"), "LaunchExecutable": "../Fortnite/x.exe"})
            .to_string(),
    );
    let r = pc.scan();
    assert_eq!(
        pc.found(&r),
        pairs(&[
            (
                "epic",
                "E/Fortnite/FortniteGame/Binaries/Win64/Fortnite.exe"
            ),
            (
                "epic",
                "E/Fortnite/Engine/Binaries/ThirdParty/zzz-helper.exe"
            ),
        ])
    );
    assert_eq!(r.launchers.len(), 1);
    assert_eq!(r.launchers[0].launcher, "epic");
    assert_eq!(
        Path::new(&r.launchers[0].path),
        pc.path("PF86/Epic Games/Launcher").as_path()
    );
}

#[test]
fn epic_is_installed_when_only_its_manifests_exist() {
    let pc = Pc::new();
    fs::create_dir_all(pc.path("PD/Epic/EpicGamesLauncher/Data/Manifests")).unwrap();
    let r = pc.scan();
    assert_eq!(r.launchers.len(), 1);
    assert_eq!(
        Path::new(&r.launchers[0].path),
        pc.path("PD/Epic/EpicGamesLauncher").as_path()
    );
}

#[test]
fn an_unparsable_epic_manifest_is_skipped_and_counted() {
    let pc = Pc::new();
    pc.file(
        "PD/Epic/EpicGamesLauncher/Data/Manifests/BAD.item",
        "{ not json",
    );
    let r = pc.scan();
    assert_eq!((r.entries.len(), r.skipped), (0, 1));
}

#[test]
fn riot_games_come_from_the_install_list() {
    let pc = Pc::new();
    let client = pc.path("R/Riot Client/RiotClientServices.exe");
    let valorant = format!("{}\\", pc.path("R/VALORANT/live").display());
    pc.file(
        "R/VALORANT/live/ShooterGame/Binaries/Win64/VALORANT-Win64-Shipping.exe",
        "",
    );
    pc.file("R/VALORANT/live/VALORANT.exe", "");
    pc.file(
        "PD/Riot Games/RiotClientInstalls.json",
        &serde_json::json!({
            "rc_default": client,
            "rc_live": client,
            "associated_client": { valorant: client },
        })
        .to_string(),
    );
    let r = pc.scan();
    assert_eq!(
        pc.found(&r),
        pairs(&[
            ("riot", "R/VALORANT/live/VALORANT.exe"),
            (
                "riot",
                "R/VALORANT/live/ShooterGame/Binaries/Win64/VALORANT-Win64-Shipping.exe"
            ),
        ])
    );
    assert_eq!(r.launchers.len(), 1);
    assert_eq!(r.launchers[0].launcher, "riot");
    assert_eq!(
        Path::new(&r.launchers[0].path),
        pc.path("R/Riot Client").as_path()
    );
    // A list that is not JSON means no Riot, and is counted.
    pc.file("PD/Riot Games/RiotClientInstalls.json", "nope");
    let r = pc.scan();
    assert!(r.launchers.is_empty());
    assert_eq!(r.skipped, 1);
}

#[test]
fn launchers_are_listed_steam_then_epic_then_riot() {
    let pc = Pc::new();
    pc.file("PD/Riot Games/RiotClientInstalls.json", &{
        serde_json::json!({"rc_default": pc.path("R/Riot Client/RiotClientServices.exe")})
            .to_string()
    });
    fs::create_dir_all(pc.path("PF86/Epic Games/Launcher")).unwrap();
    pc.steam();
    let names: Vec<_> = pc
        .scan()
        .launchers
        .into_iter()
        .map(|l| l.launcher)
        .collect();
    assert_eq!(names, ["steam", "epic", "riot"]);
}

#[test]
fn common_folders_are_scanned_and_windows_own_folders_are_not() {
    let pc = Pc::new();
    pc.file("Games/Hades/Hades.exe", "");
    pc.file("Games/Hades/unins000.exe", "");
    pc.file("D/Games/Quake/quake.exe", "");
    pc.file("PF/Some Studio/Game/game.exe", "");
    pc.file("PF/Some Studio/Game/Setup.exe", "");
    pc.file("PF/Some Studio/Game/UnityCrashHandler64.exe", "");
    pc.file("PF/Some Studio/Game/vcredist_x86.exe", "");
    pc.file("PF/Common Files/shared.exe", "");
    pc.file("PF/Windows Defender/MpCmdRun.exe", "");
    pc.file("PF/Microsoft Office/word.exe", "");
    pc.file("PF/Internet Explorer/iexplore.exe", "");
    pc.file("PF/Arena360/arena-kiosk-ui.exe", "");
    pc.file("PF86/Steam/steam.exe", "");
    // Deeper than the Program Files depth: not reached.
    pc.file("PF/a/b/c/d/deep.exe", "");
    let r = pc.scan();
    assert_eq!(
        pc.found(&r),
        pairs(&[
            ("none", "Games/Hades/Hades.exe"),
            ("none", "D/Games/Quake/quake.exe"),
            ("none", "PF/Some Studio/Game/game.exe"),
        ]),
        "Steam's own folder is covered by the Steam source; its steam.exe is its launcher"
    );
}

#[test]
fn a_launcher_source_wins_over_a_folder_and_paths_are_compared_without_case() {
    let pc = Pc::new();
    pc.steam();
    pc.file(
        "PF86/Steam/steamapps/appmanifest_1.acf",
        "\"AppState\" { \"installdir\" \"G\" }",
    );
    pc.file("PF86/Steam/steamapps/common/G/g.exe", "");
    // The same executable is also under a scanned folder, reached through different casing.
    let mut loc = pc.loc.clone();
    loc.game_folders = vec![pc.path("PF86/Steam/steamapps/common")];
    let host = Fixed {
        fs: &pc.fs,
        loc: loc.clone(),
    };
    let r = scan(&host, &NeverPause).unwrap();
    assert_eq!(r.entries.len(), 1);
    assert_eq!(r.entries[0].launcher, "steam");
    // Upper-cased duplicate through the folder source is also dropped.
    let upper = pc.path("PF86/Steam/steamapps/common/G/g.exe");
    assert!(
        r.entries[0]
            .path
            .eq_ignore_ascii_case(&upper.display().to_string())
    );
}

struct Fixed<'a> {
    fs: &'a Wrapped,
    loc: Locations,
}

impl ScanHost for Fixed<'_> {
    fn fs(&self) -> &dyn ScanFs {
        self.fs
    }

    fn locations(&self) -> Locations {
        self.loc.clone()
    }
}

#[test]
fn only_five_hundred_entries_are_kept_and_the_rest_dropped() {
    let pc = Pc::new();
    for i in 0..520 {
        pc.file(&format!("Games/g{i:03}/g{i:03}.exe"), "");
    }
    let r = pc.scan();
    assert_eq!(r.entries.len(), MAX_SCAN_ENTRIES);
    assert_eq!(r.entries[0].file_name, "g000.exe");
    assert_eq!(r.entries[499].file_name, "g499.exe");
    r.into_report(1).validate(1).unwrap();
}

#[test]
fn an_unreadable_folder_is_skipped_logged_and_counted_and_the_scan_goes_on() {
    let pc = Pc::new();
    pc.file("Games/a/a.exe", "");
    pc.file("Games/locked/locked.exe", "");
    pc.file("Games/z/z.exe", "");
    pc.fs.denied.lock().unwrap().push(pc.path("Games/locked"));
    let r = pc.scan();
    assert_eq!(
        pc.found(&r),
        pairs(&[("none", "Games/a/a.exe"), ("none", "Games/z/z.exe")])
    );
    assert_eq!(r.skipped, 1);
    // A denied source root and an unreadable manifest are counted too.
    pc.steam();
    pc.file(
        "PF86/Steam/steamapps/appmanifest_1.acf",
        "\"x\" { \"installdir\" \"G\" }",
    );
    pc.fs
        .unreadable
        .lock()
        .unwrap()
        .push(pc.path("PF86/Steam/steamapps/appmanifest_1.acf"));
    pc.fs.denied.lock().unwrap().push(pc.path("PF"));
    fs::create_dir_all(pc.path("PF")).unwrap();
    let r = pc.scan();
    assert_eq!(r.skipped, 3, "locked, the manifest and Program Files");
    assert_eq!(r.entries.len(), 2);
}

#[test]
fn a_folder_that_does_not_exist_is_not_an_error() {
    let pc = Pc::new();
    let r = pc.scan();
    assert_eq!(r.skipped, 0);
}

#[test]
fn product_name_and_publisher_come_from_version_info_and_are_cleaned() {
    let pc = Pc::new();
    pc.file("Games/g/game.exe", "");
    pc.file("Games/g/plain.exe", "");
    pc.fs.versions.lock().unwrap().insert(
        "game.exe".into(),
        VersionInfo {
            product_name: "  Great\u{7} Game ".into(),
            publisher: "x".repeat(300),
        },
    );
    let r = pc.scan();
    let game = r
        .entries
        .iter()
        .find(|e| e.file_name == "game.exe")
        .unwrap();
    assert_eq!(game.product_name, "Great  Game");
    assert_eq!(game.publisher.chars().count(), MAX_SCAN_TEXT_CHARS);
    let plain = r
        .entries
        .iter()
        .find(|e| e.file_name == "plain.exe")
        .unwrap();
    assert_eq!(
        (plain.product_name.as_str(), plain.publisher.as_str()),
        ("", "")
    );
    r.into_report(1).validate(1).unwrap();
}

#[test]
fn an_overlong_path_or_name_is_left_out() {
    let fs = Wrapped::default();
    let mut w = Walker {
        fs: &fs,
        pause: &NeverPause,
        entries: Vec::new(),
        seen: HashSet::new(),
        skipped: 0,
    };
    let long_dir = "d".repeat(MAX_SCAN_PATH_CHARS);
    w.add(
        &PathBuf::from(format!(r"C:\{long_dir}\x.exe")),
        "x.exe",
        "none",
    );
    let long_name = format!("{}.exe", "n".repeat(MAX_SCAN_FILE_NAME_CHARS));
    w.add(
        &PathBuf::from(format!(r"C:\g\{long_name}")),
        &long_name,
        "none",
    );
    assert!(w.entries.is_empty());
    w.add(&PathBuf::from(r"C:\g\ok.exe"), "ok.exe", "none");
    w.add(&PathBuf::from(r"c:\G\OK.exe"), "OK.exe", "epic");
    assert_eq!(
        w.entries.len(),
        1,
        "the same path without case is one entry"
    );
    assert_eq!(w.entries[0].launcher, "none");
}

#[test]
fn noise_executables_are_recognised_by_name() {
    for noise in [
        "unins000.exe",
        "Uninstall.exe",
        "setup.exe",
        "Installer.exe",
        "UnityCrashHandler64.exe",
        "CrashReportClient.exe",
        "crashpad_handler.exe",
        "vc_redist.x64.exe",
        "VCREDIST_x86.EXE",
        "dxsetup.exe",
        "DXWebSetup.exe",
        "dotnetfx35.exe",
        "oalinst.exe",
        "UE4PrereqSetup_x64.exe",
    ] {
        assert!(is_noise_exe(noise), "{noise}");
    }
    for game in [
        "cs2.exe",
        "VALORANT-Win64-Shipping.exe",
        "Hades.exe",
        "dota2.exe",
    ] {
        assert!(!is_noise_exe(game), "{game}");
    }
    assert!(is_exe("A.EXE") && is_exe("a.exe") && !is_exe("a.dll") && !is_exe(".exe"));
}

#[test]
fn vdf_values_are_found_by_key_at_any_depth_with_escapes() {
    let text = "// c\n\"libraryfolders\"\n{\n \"0\" { \"path\" \"C:\\\\Steam\" \"label\" \"\" }\n \"1\" { \"PATH\" \"D:\\\\Lib \\\"x\\\"\" }\n}\n\"label\" \"path\"\n";
    assert_eq!(
        vdf_values(text, "path"),
        [r"C:\Steam".to_owned(), r#"D:\Lib "x""#.to_owned()],
        "a value that reads `path` is not a key"
    );
    assert!(vdf_values("", "path").is_empty());
    assert!(vdf_values("\"unterminated", "path").is_empty());
}

// ---- pausing ----

/// Pauses while `busy` is set, counting the checkpoints.
struct Gate {
    busy: AtomicBool,
    cancelled: AtomicBool,
    checkpoints: AtomicUsize,
    waiting: AtomicUsize,
}

impl Pause for Gate {
    fn checkpoint(&self) -> Result<(), Cancelled> {
        self.checkpoints.fetch_add(1, Ordering::SeqCst);
        while self.busy.load(Ordering::SeqCst) {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(Cancelled);
            }
            self.waiting.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
}

fn gate(busy: bool) -> Arc<Gate> {
    Arc::new(Gate {
        busy: AtomicBool::new(busy),
        cancelled: AtomicBool::new(false),
        checkpoints: AtomicUsize::new(0),
        waiting: AtomicUsize::new(0),
    })
}

#[test]
fn the_scan_pauses_before_each_folder_while_a_session_runs_and_resumes_after() {
    let pc = Arc::new(Pc::new());
    for i in 0..5 {
        pc.file(&format!("Games/g{i}/g{i}.exe"), "");
    }
    let g = gate(true);
    let worker = {
        let (pc, g) = (Arc::clone(&pc), Arc::clone(&g));
        std::thread::spawn(move || scan(pc.as_ref(), g.as_ref()))
    };
    // The scan reaches its first folder and waits there: no folder is listed meanwhile.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while g.waiting.load(Ordering::SeqCst) < 3 {
        assert!(std::time::Instant::now() < deadline, "never paused");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(pc.fs.lists.load(Ordering::SeqCst), 0);
    assert!(!worker.is_finished());
    // The session ends: the scan goes on and finishes with everything.
    g.busy.store(false, Ordering::SeqCst);
    let r = worker.join().unwrap().unwrap();
    assert_eq!(r.entries.len(), 5);
    assert!(g.checkpoints.load(Ordering::SeqCst) >= 6);
}

#[test]
fn a_session_that_starts_mid_scan_holds_the_next_folder() {
    let pc = Arc::new(Pc::new());
    for i in 0..4 {
        pc.file(&format!("Games/g{i}/g{i}.exe"), "");
    }
    // Busy from the third checkpoint on.
    struct Starts {
        gate: Arc<Gate>,
    }
    impl Pause for Starts {
        fn checkpoint(&self) -> Result<(), Cancelled> {
            if self.gate.checkpoints.load(Ordering::SeqCst) == 2 {
                self.gate.busy.store(true, Ordering::SeqCst);
            }
            self.gate.checkpoint()
        }
    }
    let g = gate(false);
    let worker = {
        let (pc, g) = (Arc::clone(&pc), Arc::clone(&g));
        std::thread::spawn(move || scan(pc.as_ref(), &Starts { gate: g }))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while g.waiting.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "never paused");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let listed = pc.fs.lists.load(Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert_eq!(
        pc.fs.lists.load(Ordering::SeqCst),
        listed,
        "nothing is listed while the session runs"
    );
    g.busy.store(false, Ordering::SeqCst);
    assert_eq!(worker.join().unwrap().unwrap().entries.len(), 4);
}

#[test]
fn a_cancelled_scan_reports_nothing() {
    let pc = Pc::new();
    pc.file("Games/g/g.exe", "");
    let g = gate(true);
    g.cancelled.store(true, Ordering::SeqCst);
    assert_eq!(scan(&pc, g.as_ref()), Err(Cancelled));
}

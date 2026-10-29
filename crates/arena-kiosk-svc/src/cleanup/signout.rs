//! Launcher, Discord and browser sign-out (Story 6.12): the next player must never see the last
//! player's Steam, Epic, Riot or Discord account, their browser profile or their saved
//! Windows credentials.
//!
//! One [`SignOut`] step per [`Target`] (so a failure names the launcher or browser in
//! `kiosk.cleanup_failed`), then one [`Credentials`] step. A target's step
//!
//! 1. ends the target's programs and checks they are gone (the removal below starts only after
//!    the launcher has exited, so it cannot rewrite its login files behind the cleanup),
//! 2. removes the saved login tokens and remembered-account files (a browser's whole profile
//!    folder), then puts back the clean template when the branch keeps one, and
//! 3. for Steam, clears the remembered account in the player's registry (through the host).
//!
//! A target that is not installed has none of those files and no programs, so its step passes
//! with nothing to do. Nothing here follows a link: the player owns their profile and may plant
//! a junction where a folder should be, and the service must not empty what it points at.
//! Errors name the target and a count, never a file.

use std::io;
use std::path::{Path, PathBuf};

use super::{CleanupStep, StepEnv, close};
use crate::procs::CloseScope;

/// The step name of the Windows Credential Manager clean-up.
pub const STEP_CREDENTIALS: &str = "credentials";

/// A launcher, chat app or browser that keeps a login on the PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Steam,
    Epic,
    Riot,
    Discord,
    Chrome,
    Edge,
    Firefox,
}

/// Where a removal starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Root {
    /// The player's `AppData\Local`.
    Local,
    /// The player's `AppData\Roaming`.
    Roaming,
    /// The Steam install folder.
    Steam,
}

/// One thing to remove: below the root, `/`-separated. A last part ending in `*` matches every
/// entry of the folder that starts with the rest.
struct Removal(Root, &'static str);

impl Target {
    /// Every target, in the order the pipeline signs them out.
    pub const ALL: [Self; 7] = [
        Self::Steam,
        Self::Epic,
        Self::Riot,
        Self::Discord,
        Self::Chrome,
        Self::Edge,
        Self::Firefox,
    ];

    /// The step name (also the `step` of a `kiosk.cleanup_failed`): `a-z 0-9 _` only.
    pub fn step(self) -> &'static str {
        match self {
            Self::Steam => "signout_steam",
            Self::Epic => "signout_epic",
            Self::Riot => "signout_riot",
            Self::Discord => "signout_discord",
            Self::Chrome => "reset_chrome",
            Self::Edge => "reset_edge",
            Self::Firefox => "reset_firefox",
        }
    }

    /// The name in a failure's detail.
    pub fn label(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Riot => "Riot",
            Self::Discord => "Discord",
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
        }
    }

    /// The image names of its programs. `msedgewebview2.exe` (the kiosk app's own web view) is
    /// not Edge's.
    pub fn images(self) -> &'static [&'static str] {
        match self {
            Self::Steam => &["steam.exe", "steamwebhelper.exe"],
            Self::Epic => &[
                "EpicGamesLauncher.exe",
                "EpicWebHelper.exe",
                "EOSOverlayRenderer-Win64-Shipping.exe",
            ],
            Self::Riot => &[
                "RiotClientServices.exe",
                "RiotClientUx.exe",
                "RiotClientUxRender.exe",
                "RiotClientCrashHandler.exe",
            ],
            Self::Discord => &["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe"],
            Self::Chrome => &["chrome.exe"],
            Self::Edge => &["msedge.exe"],
            Self::Firefox => &["firefox.exe"],
        }
    }

    fn removals(self) -> &'static [Removal] {
        match self {
            Self::Steam => &[
                Removal(Root::Steam, "config/loginusers.vdf"),
                Removal(Root::Steam, "config/config.vdf"),
                Removal(Root::Steam, "ssfn*"),
                Removal(Root::Local, "Steam/htmlcache"),
            ],
            Self::Epic => &[
                Removal(
                    Root::Local,
                    "EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini",
                ),
                Removal(Root::Local, "EpicGamesLauncher/Saved/webcache*"),
            ],
            Self::Riot => &[Removal(Root::Local, "Riot Games/Riot Client/Data")],
            Self::Discord => &[
                Removal(Root::Roaming, "discord"),
                Removal(Root::Roaming, "discordptb"),
                Removal(Root::Roaming, "discordcanary"),
            ],
            Self::Chrome => &[Removal(Root::Local, "Google/Chrome/User Data")],
            Self::Edge => &[Removal(Root::Local, "Microsoft/Edge/User Data")],
            Self::Firefox => &[
                Removal(Root::Roaming, "Mozilla/Firefox"),
                Removal(Root::Local, "Mozilla/Firefox"),
            ],
        }
    }

    fn is_browser(self) -> bool {
        matches!(self, Self::Chrome | Self::Edge | Self::Firefox)
    }

    /// The folder name of its clean template below the branch's template folder.
    fn template(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            _ => "firefox",
        }
    }
}

/// Signs one launcher or resets one browser.
pub struct SignOut(pub Target);

impl CleanupStep for SignOut {
    fn name(&self) -> &'static str {
        self.0.step()
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        let target = self.0;
        let label = target.label();
        let report = close(env.ctx, CloseScope::Images(target.images()))?;
        if report.failed > 0 {
            return Err(format!("{label} is still running"));
        }
        let Some(profile) = env.host.profile_dir()? else {
            // No profile yet: nothing was ever left there.
            return Ok(());
        };
        let local = profile.join("AppData").join("Local");
        let roaming = profile.join("AppData").join("Roaming");
        let steam = env.host.steam_dir();
        let mut left = 0usize;
        let mut problems: Vec<String> = Vec::new();
        for Removal(root, rel) in target.removals() {
            let base = match root {
                Root::Local => &local,
                Root::Roaming => &roaming,
                Root::Steam => match &steam {
                    Some(dir) => dir,
                    None => continue,
                },
            };
            match remove_rel(base, rel) {
                Ok(n) => left += n,
                Err(e) => problems.push(e),
            }
        }
        if left > 0 {
            problems.push(format!(
                "{left} saved item{} could not be removed",
                if left == 1 { "" } else { "s" }
            ));
        }
        if target == Target::Steam
            && let Err(e) = env.host.clear_steam_login()
        {
            problems.push(e);
        }
        if target.is_browser()
            && problems.is_empty()
            && let Some(templates) = env.host.browser_templates()
        {
            let from = templates.join(target.template());
            for (sub, to) in [("Local", &local), ("Roaming", &roaming)] {
                if let Err(e) = copy_tree(&from.join(sub), to) {
                    problems.push(e);
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("{label}: {}", problems.join("; ")))
        }
    }
}

/// Removes the player's saved Windows credentials (Credential Manager).
pub struct Credentials;

impl CleanupStep for Credentials {
    fn name(&self) -> &'static str {
        STEP_CREDENTIALS
    }

    fn run(&self, env: &StepEnv<'_>) -> Result<(), String> {
        env.host.clear_credentials()
    }
}

/// Removes one entry: a file, a folder with everything in it, or a link itself (never what it
/// points at).
pub(super) fn remove_entry(path: &Path) -> io::Result<()> {
    let kind = std::fs::symlink_metadata(path)?.file_type();
    if kind.is_symlink() {
        // A directory link goes with `remove_dir`, a file link with `remove_file`.
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
    } else if kind.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Removes `rel` (see [`Removal`]) below `base`. Returns how many entries could not be removed.
/// A missing path is fine; a folder on the way that is a link, or not a folder, is an error.
fn remove_rel(base: &Path, rel: &str) -> Result<usize, String> {
    let mut parts: Vec<&str> = rel.split('/').collect();
    let leaf = parts.pop().unwrap_or_default();
    let mut dir = base.to_path_buf();
    for part in parts {
        dir.push(part);
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("a folder on the way is a link, not a folder".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("a folder cannot be read: {e}")),
        }
    }
    let victims: Vec<PathBuf> = match leaf.strip_suffix('*') {
        None => vec![dir.join(leaf)],
        Some(prefix) => {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
                Err(e) => return Err(format!("a folder cannot be read: {e}")),
            };
            let prefix = prefix.to_lowercase();
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with(&prefix)
                })
                .map(|e| e.path())
                .collect()
        }
    };
    let mut left = 0;
    for path in victims {
        match remove_entry(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => left += 1,
        }
    }
    Ok(left)
}

/// Copies the folder `from` into `to`, merging with what is there. Links in the template are
/// skipped, and nothing is written through a link in `to`. A missing `from` is fine (the
/// branch keeps no template).
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(from) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => return Err("the browser template is not a folder".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("the browser template cannot be read: {e}")),
    }
    let mut failed = 0usize;
    copy_into(from, to, &mut failed);
    match failed {
        0 => Ok(()),
        n => Err(format!(
            "{n} template item{} could not be copied",
            if n == 1 { "" } else { "s" }
        )),
    }
}

fn copy_into(from: &Path, to: &Path, failed: &mut usize) {
    if let Ok(meta) = std::fs::symlink_metadata(to)
        && meta.file_type().is_symlink()
    {
        *failed += 1;
        return;
    }
    if std::fs::create_dir_all(to).is_err() {
        *failed += 1;
        return;
    }
    let Ok(entries) = std::fs::read_dir(from) else {
        *failed += 1;
        return;
    };
    for entry in entries.flatten() {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let Ok(meta) = std::fs::symlink_metadata(&src) else {
            *failed += 1;
            continue;
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            copy_into(&src, &dst, failed);
        } else if std::fs::symlink_metadata(&dst).is_ok_and(|m| m.file_type().is_symlink())
            || std::fs::copy(&src, &dst).is_err()
        {
            *failed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn step_names_fit_the_hub_s_rules() {
        for t in Target::ALL {
            let step = t.step();
            assert!((1..=arena_proto::MAX_CLEANUP_STEP_LEN).contains(&step.len()));
            assert!(
                step.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            );
        }
    }

    #[test]
    fn edge_is_not_the_kiosk_web_view() {
        assert!(
            !Target::Edge
                .images()
                .iter()
                .any(|i| i.eq_ignore_ascii_case("msedgewebview2.exe"))
        );
    }

    #[test]
    fn removes_a_file_a_folder_and_a_prefix_match_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let b = dir.path();
        touch(&b.join("config/loginusers.vdf"));
        touch(&b.join("config/keep.vdf"));
        touch(&b.join("ssfn123"));
        touch(&b.join("SSFN9"));
        touch(&b.join("steam.exe"));
        touch(&b.join("Riot Games/Riot Client/Data/deep/x.yaml"));
        assert_eq!(remove_rel(b, "config/loginusers.vdf"), Ok(0));
        assert_eq!(remove_rel(b, "ssfn*"), Ok(0));
        assert_eq!(remove_rel(b, "Riot Games/Riot Client/Data"), Ok(0));
        assert!(!b.join("config/loginusers.vdf").exists());
        assert!(b.join("config/keep.vdf").exists());
        assert!(!b.join("ssfn123").exists());
        assert!(!b.join("SSFN9").exists());
        assert!(b.join("steam.exe").exists());
        assert!(!b.join("Riot Games/Riot Client/Data").exists());
        assert!(b.join("Riot Games/Riot Client").exists());
    }

    #[test]
    fn a_missing_path_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(remove_rel(dir.path(), "Google/Chrome/User Data"), Ok(0));
        assert_eq!(remove_rel(dir.path(), "Epic/Saved/webcache*"), Ok(0));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_on_the_way_is_refused_and_a_link_leaf_loses_only_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let (b, outside) = (dir.path().join("base"), dir.path().join("outside"));
        touch(&outside.join("Data/secret.txt"));
        std::fs::create_dir_all(&b).unwrap();
        std::os::unix::fs::symlink(&outside, b.join("Riot Games")).unwrap();
        assert!(remove_rel(&b, "Riot Games/Data").is_err());
        assert!(outside.join("Data/secret.txt").exists());
        std::os::unix::fs::symlink(&outside, b.join("discord")).unwrap();
        assert_eq!(remove_rel(&b, "discord"), Ok(0));
        assert!(!b.join("discord").exists());
        assert!(outside.join("Data/secret.txt").exists());
    }

    #[test]
    fn a_template_is_merged_in_and_a_missing_one_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let (tpl, dest) = (dir.path().join("tpl"), dir.path().join("dest"));
        assert_eq!(copy_tree(&tpl, &dest), Ok(()));
        assert!(!dest.exists());
        touch(&tpl.join("Google/Chrome/User Data/First Run"));
        touch(&tpl.join("Google/Chrome/User Data/Default/Preferences"));
        assert_eq!(copy_tree(&tpl, &dest), Ok(()));
        assert!(dest.join("Google/Chrome/User Data/First Run").exists());
        assert!(
            dest.join("Google/Chrome/User Data/Default/Preferences")
                .exists()
        );
    }
}

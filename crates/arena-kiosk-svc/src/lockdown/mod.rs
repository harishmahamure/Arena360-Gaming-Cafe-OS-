//! `lockdown apply|restore`: runs the pure lockdown plan (`arena_kiosk_core::lockdown`) on the
//! player account's own hive, and nowhere else.
//!
//! - **Apply:** make sure the player account exists (a local standard user; created with a
//!   random password that is never stored) and has a profile, refuse it if it is an
//!   administrator, record every touched name's previous value in `lockdown-backup.json`
//!   *before* writing anything, then set the lockdown values. Applying again keeps the first
//!   backup, so the originals are never overwritten by the lockdown's own values.
//! - **Restore:** write back exactly what the backup recorded (deleting the names that were
//!   absent), then remove the backup. The account itself is kept.
//!
//! The OS parts sit behind [`PlayerAccounts`] and [`RegistryBackend`]; on Windows they are the
//! NetUser / profile / hive-loading executor in [`windows`], in tests the in-memory fakes.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use arena_kiosk_core::lockdown::{
    BACKUP_VERSION, LockdownBackup, RegOp, RegValue, WINLOGON_KEY, capture_backup, distinct_keys,
    is_protected, plan_apply, plan_restore, touched,
};

#[cfg(windows)]
pub mod windows;

/// The message (and exit code 2) of `lockdown` on any OS but Windows.
pub const ONLY_ON_WINDOWS: &str = "lockdown is only available on Windows";

/// The kiosk app's fixed file name; apply refuses any other shell target.
pub const KIOSK_UI_EXE: &str = "arena-kiosk-ui.exe";

/// Values under one hive root (the player's `HKEY_USERS\<SID>` or its loaded `NTUSER.DAT`).
pub trait RegistryBackend {
    /// The value of `key\name`; `None` when the key or the value does not exist.
    fn get(&self, key: &str, name: &str) -> io::Result<Option<RegValue>>;
    /// Sets `key\name`, creating the key when needed.
    fn set(&mut self, key: &str, name: &str, value: &RegValue) -> io::Result<()>;
    /// Removes `key\name`; an absent key or value is fine.
    fn delete(&mut self, key: &str, name: &str) -> io::Result<()>;
    /// Whether `key` exists.
    fn key_exists(&self, key: &str) -> io::Result<bool>;
    /// Removes `key` only when it has no values or subkeys; an absent or non-empty key is fine.
    fn delete_key_if_empty(&mut self, key: &str) -> io::Result<()>;

    /// The key's DACL as SDDL (protected keys, so restore can put it back). `None` off Windows.
    fn key_sddl(&self, _key: &str) -> io::Result<Option<String>> {
        Ok(None)
    }
    /// Tightens the key's DACL to SYSTEM + Administrators full control, the player read-only, so
    /// the player cannot edit it. A no-op off Windows.
    fn protect_key(&mut self, _key: &str, _player_sid: &str) -> io::Result<()> {
        Ok(())
    }
    /// Puts a key's DACL back to `sddl`. A no-op off Windows.
    fn set_key_sddl(&mut self, _key: &str, _sddl: &str) -> io::Result<()> {
        Ok(())
    }
}

/// An in-memory hive (tests and dry runs). Key and value names compare case-insensitively, as
/// in the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeRegistry {
    values: BTreeMap<(String, String), RegValue>,
    /// Keys that exist with no values (created, or emptied and not removed).
    empty_keys: BTreeSet<String>,
    /// The DACL SDDL set on each key (protected keys only in practice).
    sddl: BTreeMap<String, String>,
    /// When set, a write to this value name fails (to test partial applies).
    pub fail_writes_to: Option<String>,
}

fn fold(key: &str, name: &str) -> (String, String) {
    (key.to_ascii_lowercase(), name.to_ascii_lowercase())
}

fn lc(key: &str) -> String {
    key.to_ascii_lowercase()
}

impl FakeRegistry {
    pub fn with(values: impl IntoIterator<Item = (&'static str, &'static str, RegValue)>) -> Self {
        let mut r = Self::default();
        for (k, n, v) in values {
            r.values.insert(fold(k, n), v);
        }
        r
    }

    /// Marks `key` as an existing (empty) key.
    pub fn with_empty_key(mut self, key: &str) -> Self {
        self.empty_keys.insert(lc(key));
        self
    }

    /// Gives an existing `key` a starting DACL (without changing its values).
    pub fn with_key_sddl(mut self, key: &str, sddl: &str) -> Self {
        self.sddl.insert(lc(key), sddl.to_owned());
        self
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn has_values(&self, key: &str) -> bool {
        self.values.keys().any(|(k, _)| *k == lc(key))
    }

    /// Every value, by lower-cased `(key, name)`.
    pub fn values(&self) -> &BTreeMap<(String, String), RegValue> {
        &self.values
    }

    /// The DACL SDDL recorded for `key`.
    pub fn sddl_of(&self, key: &str) -> Option<&str> {
        self.sddl.get(&lc(key)).map(String::as_str)
    }
}

impl RegistryBackend for FakeRegistry {
    fn get(&self, key: &str, name: &str) -> io::Result<Option<RegValue>> {
        Ok(self.values.get(&fold(key, name)).cloned())
    }

    fn set(&mut self, key: &str, name: &str, value: &RegValue) -> io::Result<()> {
        if self
            .fail_writes_to
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
        {
            return Err(io::Error::other(format!("write to {name} refused")));
        }
        self.empty_keys.remove(&lc(key));
        self.values.insert(fold(key, name), value.clone());
        Ok(())
    }

    fn delete(&mut self, key: &str, name: &str) -> io::Result<()> {
        self.values.remove(&fold(key, name));
        if !self.has_values(key) {
            self.empty_keys.insert(lc(key));
        }
        Ok(())
    }

    fn key_exists(&self, key: &str) -> io::Result<bool> {
        Ok(self.empty_keys.contains(&lc(key)) || self.has_values(key))
    }

    fn delete_key_if_empty(&mut self, key: &str) -> io::Result<()> {
        if !self.has_values(key) {
            self.empty_keys.remove(&lc(key));
            self.sddl.remove(&lc(key));
        }
        Ok(())
    }

    fn key_sddl(&self, key: &str) -> io::Result<Option<String>> {
        Ok(self.sddl.get(&lc(key)).cloned())
    }

    fn protect_key(&mut self, key: &str, player_sid: &str) -> io::Result<()> {
        self.sddl.insert(
            lc(key),
            format!("D:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;{player_sid})"),
        );
        Ok(())
    }

    fn set_key_sddl(&mut self, key: &str, sddl: &str) -> io::Result<()> {
        self.sddl.insert(lc(key), sddl.to_owned());
        Ok(())
    }
}

/// Runs `ops` in order, stopping at the first failure.
pub fn run_ops(reg: &mut dyn RegistryBackend, ops: &[RegOp]) -> io::Result<()> {
    for op in ops {
        match op {
            RegOp::Set { key, name, value } => reg.set(key, name, value),
            RegOp::Delete { key, name } => reg.delete(key, name),
            RegOp::DeleteKeyIfEmpty { key } => reg.delete_key_if_empty(key),
        }
        .map_err(|e| io::Error::new(e.kind(), format!("{op:?}: {e}")))?;
    }
    Ok(())
}

/// Tightens the DACL of every protected key the lockdown touched, so the player cannot undo it.
fn protect_keys(
    reg: &mut dyn RegistryBackend,
    shell_path: &str,
    player_sid: &str,
) -> io::Result<()> {
    for key in distinct_keys(shell_path) {
        if is_protected(&key) {
            reg.protect_key(&key, player_sid)?;
        }
    }
    Ok(())
}

/// Puts back the DACL of every protected key that existed before apply (created keys are
/// deleted by the restore ops, so they need no SDDL).
fn restore_key_sddls(reg: &mut dyn RegistryBackend, backup: &LockdownBackup) -> io::Result<()> {
    for k in &backup.keys {
        if k.existed
            && is_protected(&k.key)
            && let Some(sddl) = &k.previous_sddl
        {
            reg.set_key_sddl(&k.key, sddl)?;
        }
    }
    Ok(())
}

/// A local player account name for the lockdown: 1–20 characters (the Windows SAM limit), the
/// `config` character set, and no trailing dot or space.
pub fn valid_player_name(name: &str) -> bool {
    crate::config::valid_account_name(name)
        && name.len() <= 20
        && !name.ends_with('.')
        && !name.ends_with(' ')
}

/// The player account as the executor found or made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerAccount {
    /// `S-1-…`.
    pub sid: String,
    /// Whether this run created it.
    pub created: bool,
}

/// The account and hive side of the executor.
pub trait PlayerAccounts {
    /// Makes sure `name` is a local standard user with a profile (creating both when missing).
    /// An administrator account is refused.
    fn ensure_player(&mut self, name: &str) -> io::Result<PlayerAccount>;

    /// The SID of an existing local user `name`, or `None` when there is no such account.
    fn find_player(&mut self, name: &str) -> io::Result<Option<String>>;

    /// Runs `f` on the player's hive (its signed-in `HKEY_USERS\<sid>`, else its `NTUSER.DAT`
    /// loaded for the call and unloaded after, whatever `f` returns).
    fn with_hive(
        &mut self,
        name: &str,
        sid: &str,
        f: &mut dyn FnMut(&mut dyn RegistryBackend) -> io::Result<()>,
    ) -> io::Result<()>;
}

/// Reads the backup; `None` when there is none. On Windows a file not owned by SYSTEM or
/// Administrators is refused.
pub fn load_backup(path: &Path) -> io::Result<Option<LockdownBackup>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    crate::config::check_owner(path)?;
    let backup: LockdownBackup = serde_json::from_str(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {e}", path.display()),
        )
    })?;
    if backup.version != BACKUP_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is backup version {}, not {BACKUP_VERSION}; restore it by hand",
                path.display(),
                backup.version
            ),
        ));
    }
    Ok(Some(backup))
}

/// Writes the backup atomically.
pub fn save_backup(path: &Path, backup: &LockdownBackup) -> io::Result<()> {
    let text = serde_json::to_string_pretty(backup)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::config::write_atomic(path, text.as_bytes())
}

fn same_account(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Locks `player_user` down with `shell_path` (the full path of `arena-kiosk-ui.exe`) as its
/// shell. The backup is written before the first registry write; an existing backup is kept.
pub fn apply(
    accounts: &mut dyn PlayerAccounts,
    backup_path: &Path,
    player_user: &str,
    shell_path: &str,
) -> io::Result<LockdownBackup> {
    if !valid_player_name(player_user) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{player_user} is not a valid local account name (1–20 chars, no trailing dot)"
            ),
        ));
    }
    // The shell must be our app, and must exist, or a failed lockdown points the player's Shell
    // at a missing exe (no desktop, no kiosk).
    let exe_name = Path::new(shell_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if !exe_name.eq_ignore_ascii_case(KIOSK_UI_EXE) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the shell must be {KIOSK_UI_EXE}, not {exe_name}"),
        ));
    }
    if !Path::new(shell_path).is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the shell exe {shell_path} does not exist; not locking down"),
        ));
    }
    let existing = load_backup(backup_path)?;
    if let Some(b) = &existing
        && !same_account(&b.player_user, player_user)
    {
        return Err(io::Error::other(format!(
            "the lockdown is applied to {}; run `lockdown restore` before locking down {player_user}",
            b.player_user
        )));
    }
    let account = accounts.ensure_player(player_user)?;
    if let Some(b) = &existing
        && let Some(sid) = &b.player_sid
        && sid != &account.sid
    {
        return Err(io::Error::other(format!(
            "{player_user} is not the account the backup was taken from ({sid}); restore by hand"
        )));
    }
    let mut stored = None;
    accounts.with_hive(player_user, &account.sid, &mut |reg| {
        let mut current = BTreeMap::new();
        for (key, name) in touched(shell_path) {
            let v = reg.get(&key, &name)?;
            current.insert((key, name), v);
        }
        let mut existed = BTreeMap::new();
        let mut sddls = BTreeMap::new();
        for key in distinct_keys(shell_path) {
            let present = reg.key_exists(&key)?;
            if present && is_protected(&key) {
                sddls.insert(key.clone(), reg.key_sddl(&key)?);
            }
            existed.insert(key, present);
        }
        let backup = capture_backup(
            existing.clone(),
            player_user,
            Some(&account.sid),
            shell_path,
            |k, n| {
                current
                    .get(&(k.to_owned(), n.to_owned()))
                    .cloned()
                    .flatten()
            },
            |k| existed.get(k).copied().unwrap_or(false),
            |k| sddls.get(k).cloned().flatten(),
        );
        save_backup(backup_path, &backup)?;
        stored = Some(backup);
        run_ops(reg, &plan_apply(shell_path))?;
        // Tighten the protected keys last, so the writes above still go through.
        protect_keys(reg, shell_path, &account.sid)
    })?;
    stored.ok_or_else(|| io::Error::other("the hive callback did not run"))
}

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The backed-up values were written back and the backup removed.
    Restored { player_user: String },
    /// No backup: nothing was locked down by this installer.
    NothingToRestore,
    /// The account no longer exists (its values went with it); the backup was removed.
    AccountGone { player_user: String },
}

/// Puts back every value the backup recorded. `player_user`, when given, must be the account
/// the backup was taken from.
pub fn restore(
    accounts: &mut dyn PlayerAccounts,
    backup_path: &Path,
    player_user: Option<&str>,
) -> io::Result<RestoreOutcome> {
    let Some(backup) = load_backup(backup_path)? else {
        return Ok(RestoreOutcome::NothingToRestore);
    };
    if let Some(u) = player_user
        && !same_account(u, &backup.player_user)
    {
        return Err(io::Error::other(format!(
            "the backup is for {}, not {u}",
            backup.player_user
        )));
    }
    let user = backup.player_user.clone();
    let Some(sid) = accounts.find_player(&user)? else {
        std::fs::remove_file(backup_path)?;
        return Ok(RestoreOutcome::AccountGone { player_user: user });
    };
    if let Some(recorded) = &backup.player_sid
        && recorded != &sid
    {
        return Err(io::Error::other(format!(
            "{user} is a different account now ({sid}, the backup is for {recorded}); not restoring"
        )));
    }
    let ops = plan_restore(&backup);
    // The backup is removed only after the hive is put back; a failure keeps it for a retry.
    accounts.with_hive(&user, &sid, &mut |reg| {
        run_ops(reg, &ops)?;
        restore_key_sddls(reg, &backup)
    })?;
    std::fs::remove_file(backup_path)?;
    Ok(RestoreOutcome::Restored { player_user: user })
}

/// Last-resort fallback for the uninstaller: set the player's `Winlogon\Shell` back to
/// `explorer.exe` (and give the key its old DACL back when the backup has it), so a PC whose
/// restore failed still boots to a usable desktop instead of a removed exe. The backup, if any,
/// is left in place for a later proper restore.
pub fn reset_shell(
    accounts: &mut dyn PlayerAccounts,
    backup_path: &Path,
    player_user: Option<&str>,
) -> io::Result<RestoreOutcome> {
    let (user, recorded_sid, winlogon_sddl) = match load_backup(backup_path)? {
        Some(b) => (
            b.player_user.clone(),
            b.player_sid.clone(),
            b.key(WINLOGON_KEY).and_then(|k| k.previous_sddl.clone()),
        ),
        None => (
            player_user
                .map(str::to_owned)
                .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned()),
            None,
            None,
        ),
    };
    let Some(sid) = accounts.find_player(&user)? else {
        return Ok(RestoreOutcome::AccountGone { player_user: user });
    };
    if let Some(recorded) = &recorded_sid
        && recorded != &sid
    {
        return Err(io::Error::other(format!(
            "{user} is a different account now ({sid}); not resetting its shell"
        )));
    }
    accounts.with_hive(&user, &sid, &mut |reg| {
        reg.set(WINLOGON_KEY, "Shell", &RegValue::Sz("explorer.exe".into()))?;
        if let Some(sddl) = &winlogon_sddl {
            reg.set_key_sddl(WINLOGON_KEY, sddl)?;
        }
        Ok(())
    })?;
    Ok(RestoreOutcome::Restored { player_user: user })
}

/// A 32-character password from `random` (at least 32 bytes), using upper case, lower case,
/// digits and symbols; `None` when it lacks one of the four classes (draw again), so it meets
/// any Windows complexity policy.
pub fn password_from_random(random: &[u8]) -> Option<String> {
    const ALPHABET: &[u8] =
        b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!#$%&()*+,-./:;<=>?@[]^_{}~";
    if random.len() < 32 {
        return None;
    }
    // 256 is not a multiple of the alphabet: keep bytes below the largest multiple (no bias).
    let limit = 256 - 256 % ALPHABET.len();
    let pw: String = random
        .iter()
        .filter(|&&b| usize::from(b) < limit)
        .take(32)
        .map(|&b| char::from(ALPHABET[usize::from(b) % ALPHABET.len()]))
        .collect();
    let ok = pw.len() == 32
        && pw.chars().any(|c| c.is_ascii_uppercase())
        && pw.chars().any(|c| c.is_ascii_lowercase())
        && pw.chars().any(|c| c.is_ascii_digit())
        && pw.chars().any(|c| c.is_ascii_punctuation());
    ok.then_some(pw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_kiosk_core::lockdown::{
        POLICIES_EXPLORER_KEY, POLICIES_SYSTEM_KEY, PROTECTED_KEYS, WINLOGON_KEY,
    };

    /// A real file named like the kiosk app (apply refuses a missing or mis-named shell).
    fn shell() -> String {
        static P: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        P.get_or_init(|| {
            let dir = std::env::temp_dir().join("arena-kiosk-lockdown-test");
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join(KIOSK_UI_EXE);
            std::fs::write(&p, b"stub").unwrap();
            p
        })
        .display()
        .to_string()
    }

    /// Local accounts with their hives.
    #[derive(Default)]
    struct FakeAccounts {
        users: BTreeMap<String, (String, bool)>,
        hives: BTreeMap<String, FakeRegistry>,
        /// Accounts this fake created.
        created: Vec<String>,
        hive_opens: usize,
    }

    impl FakeAccounts {
        fn with_user(name: &str, sid: &str, admin: bool, hive: FakeRegistry) -> Self {
            let mut a = Self::default();
            a.users
                .insert(name.to_ascii_lowercase(), (sid.into(), admin));
            a.hives.insert(sid.into(), hive);
            a
        }
        fn hive(&self, name: &str) -> &FakeRegistry {
            &self.hives[&self.users[&name.to_ascii_lowercase()].0]
        }
    }

    impl PlayerAccounts for FakeAccounts {
        fn ensure_player(&mut self, name: &str) -> io::Result<PlayerAccount> {
            // Windows account names ignore case.
            let name = name.to_ascii_lowercase();
            if let Some((sid, admin)) = self.users.get(&name) {
                if *admin {
                    return Err(io::Error::other(format!("{name} is an administrator")));
                }
                return Ok(PlayerAccount {
                    sid: sid.clone(),
                    created: false,
                });
            }
            let sid = format!("S-1-5-21-9-9-9-{}", 1000 + self.users.len());
            self.users.insert(name.clone(), (sid.clone(), false));
            self.hives.insert(sid.clone(), FakeRegistry::default());
            self.created.push(name);
            Ok(PlayerAccount { sid, created: true })
        }

        fn find_player(&mut self, name: &str) -> io::Result<Option<String>> {
            Ok(self
                .users
                .get(&name.to_ascii_lowercase())
                .map(|(s, _)| s.clone()))
        }

        fn with_hive(
            &mut self,
            _name: &str,
            sid: &str,
            f: &mut dyn FnMut(&mut dyn RegistryBackend) -> io::Result<()>,
        ) -> io::Result<()> {
            self.hive_opens += 1;
            let hive = self
                .hives
                .get_mut(sid)
                .ok_or_else(|| io::Error::other("no hive"))?;
            f(hive)
        }
    }

    fn backup_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lockdown-backup.json");
        (dir, p)
    }

    #[test]
    fn apply_creates_the_player_and_locks_its_hive_only() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts =
            FakeAccounts::with_user("admin", "S-1-5-21-9-9-9-500", true, FakeRegistry::default());
        let backup = apply(&mut accounts, &path, "player", &shell).unwrap();
        assert_eq!(accounts.created, ["player"]);
        assert!(backup.entries.iter().all(|e| e.previous.is_none()));
        assert_eq!(backup.player_sid.as_deref(), Some("S-1-5-21-9-9-9-1001"));
        assert_eq!(load_backup(&path).unwrap(), Some(backup));
        let hive = accounts.hive("player");
        assert_eq!(hive.len(), plan_apply(&shell).len());
        assert_eq!(
            hive.get(WINLOGON_KEY, "Shell").unwrap(),
            Some(RegValue::Sz(format!("\"{shell}\"")))
        );
        assert_eq!(
            hive.get(POLICIES_SYSTEM_KEY, "DisableTaskMgr").unwrap(),
            Some(RegValue::Dword(1))
        );
        // The protected keys are tightened to deny the player write.
        for key in PROTECTED_KEYS {
            let sddl = hive.sddl_of(key).expect("protected key has a DACL");
            assert!(sddl.starts_with("D:P"), "{key}: {sddl}");
            assert!(
                sddl.contains("S-1-5-21-9-9-9-1001"),
                "{key} names the player"
            );
        }
        // The administrator's hive is untouched.
        assert!(accounts.hive("admin").is_empty());
    }

    #[test]
    fn a_missing_or_mis_named_shell_is_refused() {
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::default();
        // Right name, does not exist.
        assert!(
            apply(
                &mut accounts,
                &path,
                "player",
                r"C:\nope\arena-kiosk-ui.exe"
            )
            .is_err()
        );
        // Exists, wrong name.
        let other = std::env::temp_dir()
            .join("arena-kiosk-lockdown-test")
            .join("evil.exe");
        std::fs::write(&other, b"x").unwrap();
        assert!(apply(&mut accounts, &path, "player", &other.display().to_string()).is_err());
        assert!(!path.exists(), "nothing was written");
    }

    #[test]
    fn an_invalid_player_name_is_refused() {
        let (_d, path) = backup_path();
        let shell = shell();
        let mut accounts = FakeAccounts::default();
        for bad in ["name_longer_than_twenty_chars", "trailing.", "space ", ""] {
            assert!(apply(&mut accounts, &path, bad, &shell).is_err(), "{bad:?}");
        }
        assert!(valid_player_name("player"));
        assert!(valid_player_name("kiosk-1"));
    }

    #[test]
    fn applying_twice_changes_nothing_and_keeps_the_originals() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::with_user(
            "player",
            "S-1-5-21-9-9-9-1001",
            false,
            FakeRegistry::with([(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(0))]),
        );
        let first = apply(&mut accounts, &path, "player", &shell).unwrap();
        let hive_after_first = accounts.hive("player").clone();
        let file_after_first = std::fs::read_to_string(&path).unwrap();
        let second = apply(&mut accounts, &path, "Player", &shell).unwrap();
        assert_eq!(second, first);
        assert_eq!(accounts.hive("player"), &hive_after_first);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), file_after_first);
        assert!(accounts.created.is_empty());
    }

    #[test]
    fn restore_writes_back_values_dacls_and_deletes_created_keys() {
        let shell = shell();
        let (_d, path) = backup_path();
        // Explorer policies already exists with its own DACL; the accessibility keys do not.
        let original = FakeRegistry::with([
            (POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(0)),
            (POLICIES_EXPLORER_KEY, "NoDesktop", RegValue::Dword(1)),
        ])
        .with_key_sddl(POLICIES_EXPLORER_KEY, "D:(A;;KA;;;BU)");
        let mut accounts =
            FakeAccounts::with_user("player", "S-1-5-21-9-9-9-1001", false, original.clone());
        apply(&mut accounts, &path, "player", &shell).unwrap();
        assert_eq!(
            accounts
                .hive("player")
                .get(POLICIES_EXPLORER_KEY, "NoRun")
                .unwrap(),
            Some(RegValue::Dword(1))
        );
        // Tightened away from its original DACL.
        assert_ne!(
            accounts.hive("player").sddl_of(POLICIES_EXPLORER_KEY),
            Some("D:(A;;KA;;;BU)")
        );
        let outcome = restore(&mut accounts, &path, None).unwrap();
        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                player_user: "player".into()
            }
        );
        let hive = accounts.hive("player");
        assert_eq!(hive, &original, "values, DACLs and created keys all back");
        assert_eq!(hive.get(WINLOGON_KEY, "Shell").unwrap(), None);
        assert!(!path.exists(), "the backup is removed after a restore");
        assert!(accounts.users.contains_key("player"));
        assert_eq!(
            restore(&mut accounts, &path, None).unwrap(),
            RestoreOutcome::NothingToRestore
        );
    }

    #[test]
    fn an_administrator_is_never_locked_down() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::with_user(
            "player",
            "S-1-5-21-9-9-9-500",
            true,
            FakeRegistry::default(),
        );
        assert!(apply(&mut accounts, &path, "player", &shell).is_err());
        assert!(accounts.hive("player").is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn the_backup_is_written_before_a_failing_apply_so_restore_still_works() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut hive = FakeRegistry::with([(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(0))]);
        hive.fail_writes_to = Some("NoLogoff".into());
        let mut accounts = FakeAccounts::with_user("player", "S-1-5-21-9-9-9-1001", false, hive);
        assert!(apply(&mut accounts, &path, "player", &shell).is_err());
        assert!(path.exists());
        let sid = accounts.users["player"].0.clone();
        accounts.hives.get_mut(&sid).unwrap().fail_writes_to = None;
        restore(&mut accounts, &path, Some("player")).unwrap();
        assert_eq!(
            accounts.hive("player"),
            &FakeRegistry::with([(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(0))])
        );
    }

    #[test]
    fn a_failing_restore_keeps_the_backup_for_a_retry() {
        let shell = shell();
        let (_d, path) = backup_path();
        // NoRun has a previous value, so restore sets it back (and can fail doing so).
        let mut accounts = FakeAccounts::with_user(
            "player",
            "S-1-5-21-9-9-9-1001",
            false,
            FakeRegistry::with([(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(5))]),
        );
        apply(&mut accounts, &path, "player", &shell).unwrap();
        // A restore write fails mid-way.
        let sid = accounts.users["player"].0.clone();
        accounts.hives.get_mut(&sid).unwrap().fail_writes_to = Some("NoRun".into());
        assert!(restore(&mut accounts, &path, None).is_err());
        assert!(
            path.exists(),
            "the backup is kept so the restore can be retried"
        );
        // Clear the fault and retry.
        accounts.hives.get_mut(&sid).unwrap().fail_writes_to = None;
        assert_eq!(
            restore(&mut accounts, &path, None).unwrap(),
            RestoreOutcome::Restored {
                player_user: "player".into()
            }
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_backup_without_a_sid_still_restores() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::with_user(
            "player",
            "S-1-5-21-9-9-9-1001",
            false,
            FakeRegistry::default(),
        );
        let mut backup = apply(&mut accounts, &path, "player", &shell).unwrap();
        // An older backup that never recorded a SID (the mismatch check must not trip).
        backup.player_sid = None;
        save_backup(&path, &backup).unwrap();
        assert_eq!(load_backup(&path).unwrap().unwrap().player_sid, None);
        assert_eq!(
            restore(&mut accounts, &path, None).unwrap(),
            RestoreOutcome::Restored {
                player_user: "player".into()
            }
        );
    }

    #[test]
    fn reset_shell_points_the_player_back_at_explorer() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::with_user(
            "player",
            "S-1-5-21-9-9-9-1001",
            false,
            FakeRegistry::default()
                .with_empty_key(WINLOGON_KEY)
                .with_key_sddl(WINLOGON_KEY, "D:(A;;KA;;;BU)"),
        );
        apply(&mut accounts, &path, "player", &shell).unwrap();
        // The lockdown made the kiosk the shell; reset points it back at Explorer.
        let outcome = reset_shell(&mut accounts, &path, None).unwrap();
        assert_eq!(
            outcome,
            RestoreOutcome::Restored {
                player_user: "player".into()
            }
        );
        assert_eq!(
            accounts.hive("player").get(WINLOGON_KEY, "Shell").unwrap(),
            Some(RegValue::Sz("explorer.exe".into()))
        );
        // The backup is kept so a proper restore can still run.
        assert!(path.exists());
        // Winlogon's DACL is handed back to the player.
        assert_eq!(
            accounts.hive("player").sddl_of(WINLOGON_KEY),
            Some("D:(A;;KA;;;BU)")
        );
    }

    #[test]
    fn mismatched_accounts_are_refused() {
        let shell = shell();
        let (_d, path) = backup_path();
        let mut accounts = FakeAccounts::default();
        apply(&mut accounts, &path, "player", &shell).unwrap();
        // Another account while a lockdown is recorded.
        assert!(apply(&mut accounts, &path, "other", &shell).is_err());
        assert!(restore(&mut accounts, &path, Some("other")).is_err());
        // The account was deleted and re-created: a new SID, not restored into.
        let (sid, _) = accounts.users.remove("player").unwrap();
        let hive = accounts.hives.remove(&sid).unwrap();
        accounts
            .users
            .insert("player".into(), ("S-1-5-21-9-9-9-7777".into(), false));
        accounts.hives.insert("S-1-5-21-9-9-9-7777".into(), hive);
        assert!(restore(&mut accounts, &path, None).is_err());
        assert!(apply(&mut accounts, &path, "player", &shell).is_err());
        // Gone entirely: the backup is dropped.
        accounts.users.clear();
        assert_eq!(
            restore(&mut accounts, &path, None).unwrap(),
            RestoreOutcome::AccountGone {
                player_user: "player".into()
            }
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_corrupt_or_wrong_version_backup_is_an_error_not_a_fresh_start() {
        let shell = shell();
        let (_d, path) = backup_path();
        std::fs::write(&path, "{nope").unwrap();
        let mut accounts = FakeAccounts::default();
        assert!(apply(&mut accounts, &path, "player", &shell).is_err());
        assert!(restore(&mut accounts, &path, None).is_err());
        assert_eq!(accounts.hive_opens, 0);
        // A backup from a newer format is refused, not reused.
        std::fs::write(
            &path,
            r#"{"version":999,"player_user":"player","entries":[],"keys":[]}"#,
        )
        .unwrap();
        assert!(load_backup(&path).is_err());
    }

    #[test]
    fn passwords_are_32_chars_of_every_class() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut draws = 0;
        for _ in 0..200 {
            let bytes: Vec<u8> = (0..64)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect();
            if let Some(pw) = password_from_random(&bytes) {
                draws += 1;
                assert_eq!(pw.len(), 32);
                assert!(pw.chars().any(|c| c.is_ascii_punctuation()));
            }
        }
        assert!(draws > 150, "most draws succeed ({draws})");
        assert_eq!(password_from_random(&[0u8; 16]), None);
        // One class only.
        assert_eq!(password_from_random(&[0u8; 64]), None);
    }
}

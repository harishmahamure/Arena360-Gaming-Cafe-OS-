//! The `player` account's lockdown as a pure plan of registry operations.
//!
//! Every key is relative to the player's hive root (`HKEY_USERS\<SID>` or the temporarily
//! loaded `NTUSER.DAT`); nothing here touches HKLM or another account. [`plan_apply`] lists the
//! values the lockdown sets, [`capture_backup`] records what each touched name held before (or
//! that it was absent) and whether each key already existed (and, on Windows, its previous
//! security descriptor), and [`plan_restore`] turns that backup back into the operations that
//! put every name back exactly as it was, deleting the keys the lockdown itself created when
//! they are left empty.

use serde::{Deserialize, Serialize};

/// The per-user Winlogon key; its `Shell` value replaces Explorer.
pub const WINLOGON_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
/// Per-user system policies (Task Manager, lock, change password, registry tools).
pub const POLICIES_SYSTEM_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\System";
/// Per-user Explorer policies (Run, close, log off, drives, autorun).
pub const POLICIES_EXPLORER_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";
/// Sticky Keys (the Shift×5 hotkey): `Flags="506"` turns the hotkey off and the feature off.
pub const STICKY_KEYS_KEY: &str = r"Control Panel\Accessibility\StickyKeys";
/// Filter/Keyboard Response (hold-Shift-8-seconds hotkey): `Flags="122"`.
pub const KEYBOARD_RESPONSE_KEY: &str = r"Control Panel\Accessibility\Keyboard Response";
/// Toggle Keys (hold-NumLock-5-seconds hotkey): `Flags="58"`.
pub const TOGGLE_KEYS_KEY: &str = r"Control Panel\Accessibility\ToggleKeys";

/// Every drive letter except C: (bit 0 = A:, bit 2 = C:, bit 25 = Z:).
pub const ALL_DRIVES_BUT_C: u32 = 0x03FF_FFFB;

/// The keys whose DACL is tightened after apply (SYSTEM + Administrators full, player read), so
/// the player cannot undo the lockdown by editing its own hive. Their previous security
/// descriptors are recorded for restore.
pub const PROTECTED_KEYS: &[&str] = &[WINLOGON_KEY, POLICIES_SYSTEM_KEY, POLICIES_EXPLORER_KEY];

/// The current backup format. A backup of any other version is refused (not silently reused).
pub const BACKUP_VERSION: u32 = 1;

/// A registry value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RegValue {
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_SZ`.
    Sz(String),
    /// A previous value of any other type (`REG_EXPAND_SZ`, `REG_BINARY`, …), kept byte for
    /// byte with its type code so a restore is exact. The lockdown itself never sets one.
    Raw { kind: u32, bytes: Vec<u8> },
}

/// One registry operation under the player's hive root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RegOp {
    Set {
        key: String,
        name: String,
        value: RegValue,
    },
    /// Removes the value; a value that is already absent is fine.
    Delete { key: String, name: String },
    /// Removes the key, but only when it has no values and no subkeys left (used on restore to
    /// drop a key the lockdown created). A key that still holds anything, or is absent, is left
    /// as it is.
    DeleteKeyIfEmpty { key: String },
}

/// What one touched name held before the lockdown (`None`: it was absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub key: String,
    pub name: String,
    pub previous: Option<RegValue>,
}

/// What one touched key looked like before the lockdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBackup {
    pub key: String,
    /// Whether the key already existed (so a created key is deleted on restore if left empty).
    pub existed: bool,
    /// Its DACL as SDDL before the lockdown tightened it (protected keys that existed). `None`
    /// when not captured (a non-protected key, a key the lockdown created, or off Windows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_sddl: Option<String>,
}

/// The values a lockdown replaced, stored as JSON
/// (`%PROGRAMDATA%\Arena360\Kiosk\lockdown-backup.json`) before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockdownBackup {
    pub version: u32,
    /// The account the lockdown was applied to.
    pub player_user: String,
    /// Its SID (`S-1-…`), when the executor resolved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_sid: Option<String>,
    pub entries: Vec<BackupEntry>,
    /// The keys the lockdown touched and how they looked before.
    #[serde(default)]
    pub keys: Vec<KeyBackup>,
}

impl LockdownBackup {
    /// The recorded previous value of `key\name`, if that name is in the backup.
    pub fn entry(&self, key: &str, name: &str) -> Option<&BackupEntry> {
        self.entries
            .iter()
            .find(|e| e.key.eq_ignore_ascii_case(key) && e.name.eq_ignore_ascii_case(name))
    }

    /// The recorded key backup for `key`, if any.
    pub fn key(&self, key: &str) -> Option<&KeyBackup> {
        self.keys.iter().find(|k| k.key.eq_ignore_ascii_case(key))
    }
}

/// The `Winlogon\Shell` command line: the kiosk app, quoted (the install dir has spaces).
pub fn shell_command(shell_path: &str) -> String {
    format!("\"{}\"", shell_path.trim_matches('"'))
}

fn set(key: &str, name: &str, value: RegValue) -> RegOp {
    RegOp::Set {
        key: key.to_owned(),
        name: name.to_owned(),
        value,
    }
}

/// The operations that lock the player account down, with `shell_path` (the full path of
/// `arena-kiosk-ui.exe`) as its shell.
pub fn plan_apply(shell_path: &str) -> Vec<RegOp> {
    use RegValue::{Dword, Sz};
    vec![
        set(WINLOGON_KEY, "Shell", Sz(shell_command(shell_path))),
        set(POLICIES_SYSTEM_KEY, "DisableTaskMgr", Dword(1)),
        set(POLICIES_SYSTEM_KEY, "DisableLockWorkstation", Dword(1)),
        set(POLICIES_SYSTEM_KEY, "DisableChangePassword", Dword(1)),
        // No reg.exe / regedit for the player, so it cannot edit what is left writable.
        set(POLICIES_SYSTEM_KEY, "DisableRegistryTools", Dword(1)),
        set(POLICIES_EXPLORER_KEY, "NoRun", Dword(1)),
        set(POLICIES_EXPLORER_KEY, "NoClose", Dword(1)),
        set(POLICIES_EXPLORER_KEY, "NoLogoff", Dword(1)),
        set(POLICIES_EXPLORER_KEY, "NoDriveTypeAutoRun", Dword(0xFF)),
        set(POLICIES_EXPLORER_KEY, "NoAutorun", Dword(1)),
        set(POLICIES_EXPLORER_KEY, "NoDrives", Dword(ALL_DRIVES_BUT_C)),
        set(
            POLICIES_EXPLORER_KEY,
            "NoViewOnDrive",
            Dword(ALL_DRIVES_BUT_C),
        ),
        // The accessibility hotkeys (Shift×5, hold-Shift, NumLock×5) open a dialog that escapes
        // the kiosk; the documented "off" flag values turn both the hotkey and the feature off.
        set(STICKY_KEYS_KEY, "Flags", Sz("506".into())),
        set(KEYBOARD_RESPONSE_KEY, "Flags", Sz("122".into())),
        set(TOGGLE_KEYS_KEY, "Flags", Sz("58".into())),
    ]
}

/// Every `(key, name)` the lockdown touches, in plan order.
pub fn touched(shell_path: &str) -> Vec<(String, String)> {
    plan_apply(shell_path)
        .into_iter()
        .filter_map(|op| match op {
            RegOp::Set { key, name, .. } | RegOp::Delete { key, name } => Some((key, name)),
            RegOp::DeleteKeyIfEmpty { .. } => None,
        })
        .collect()
}

/// The distinct keys the lockdown touches, in first-seen order.
pub fn distinct_keys(shell_path: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (key, _) in touched(shell_path) {
        if !keys.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
            keys.push(key);
        }
    }
    keys
}

/// Whether `key` is one of the protected keys whose DACL is tightened (and SDDL backed up).
pub fn is_protected(key: &str) -> bool {
    PROTECTED_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// The backup to store before applying. An `existing` backup (a lockdown already applied) is
/// kept as it is, so applying again never records the lockdown's own values as "previous"; only
/// a touched name or key it does not list yet (one a newer release added) is read from the
/// hive. `read` returns a value, `key_exists` whether a key is present, and `key_sddl` the DACL
/// SDDL of a protected key (it is only called for protected keys that exist).
pub fn capture_backup(
    existing: Option<LockdownBackup>,
    player_user: &str,
    player_sid: Option<&str>,
    shell_path: &str,
    mut read: impl FnMut(&str, &str) -> Option<RegValue>,
    mut key_exists: impl FnMut(&str) -> bool,
    mut key_sddl: impl FnMut(&str) -> Option<String>,
) -> LockdownBackup {
    let mut backup = existing.unwrap_or_else(|| LockdownBackup {
        version: BACKUP_VERSION,
        player_user: player_user.to_owned(),
        player_sid: player_sid.map(str::to_owned),
        entries: Vec::new(),
        keys: Vec::new(),
    });
    if backup.player_sid.is_none() {
        backup.player_sid = player_sid.map(str::to_owned);
    }
    for key in distinct_keys(shell_path) {
        if backup.key(&key).is_none() {
            let existed = key_exists(&key);
            let previous_sddl = (existed && is_protected(&key))
                .then(|| key_sddl(&key))
                .flatten();
            backup.keys.push(KeyBackup {
                key,
                existed,
                previous_sddl,
            });
        }
    }
    for (key, name) in touched(shell_path) {
        if backup.entry(&key, &name).is_none() {
            let previous = read(&key, &name);
            backup.entries.push(BackupEntry {
                key,
                name,
                previous,
            });
        }
    }
    backup
}

/// The operations that put every backed-up name back — its previous value, or deleted when it
/// was absent — and then drop any key the lockdown created that is now empty (deepest first, so
/// a created parent of a created child is removed too).
pub fn plan_restore(backup: &LockdownBackup) -> Vec<RegOp> {
    let mut ops: Vec<RegOp> = backup
        .entries
        .iter()
        .map(|e| match &e.previous {
            Some(v) => RegOp::Set {
                key: e.key.clone(),
                name: e.name.clone(),
                value: v.clone(),
            },
            None => RegOp::Delete {
                key: e.key.clone(),
                name: e.name.clone(),
            },
        })
        .collect();
    let mut created: Vec<&String> = backup
        .keys
        .iter()
        .filter(|k| !k.existed)
        .map(|k| &k.key)
        .collect();
    // Deepest path first: a child key is removed before its (also created) parent.
    created.sort_by_key(|k| std::cmp::Reverse(k.matches('\\').count()));
    for key in created {
        ops.push(RegOp::DeleteKeyIfEmpty { key: key.clone() });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const SHELL: &str = r"C:\Program Files\Arena 360 Kiosk\arena-kiosk-ui.exe";

    #[derive(Clone, Default, PartialEq, Eq, Debug)]
    struct Hive {
        values: BTreeMap<(String, String), RegValue>,
        /// Keys that exist with no values (created, or emptied but not removed).
        empty_keys: std::collections::BTreeSet<String>,
    }

    impl Hive {
        fn key_exists(&self, key: &str) -> bool {
            self.empty_keys.contains(key) || self.values.keys().any(|(k, _)| k == key)
        }
        fn run(&mut self, ops: &[RegOp]) {
            for op in ops {
                match op {
                    RegOp::Set { key, name, value } => {
                        self.empty_keys.remove(key);
                        self.values
                            .insert((key.clone(), name.clone()), value.clone());
                    }
                    RegOp::Delete { key, name } => {
                        self.values.remove(&(key.clone(), name.clone()));
                        if !self.values.keys().any(|(k, _)| k == key) {
                            self.empty_keys.insert(key.clone());
                        }
                    }
                    RegOp::DeleteKeyIfEmpty { key } => {
                        if !self.values.keys().any(|(k, _)| k == key) {
                            self.empty_keys.remove(key);
                        }
                    }
                }
            }
        }
    }

    fn apply(hive: &mut Hive, existing: Option<LockdownBackup>) -> LockdownBackup {
        let snapshot = hive.clone();
        let backup = capture_backup(
            existing,
            "player",
            Some("S-1-5-21-1-2-3-1001"),
            SHELL,
            |k, n| snapshot.values.get(&(k.to_owned(), n.to_owned())).cloned(),
            |k| snapshot.key_exists(k),
            |_| None,
        );
        // Creating a key the plan writes: mark it present.
        for key in distinct_keys(SHELL) {
            hive.empty_keys.remove(&key);
        }
        hive.run(&plan_apply(SHELL));
        backup
    }

    #[test]
    fn apply_sets_every_value_and_records_all_absent() {
        let ops = plan_apply(SHELL);
        assert!(ops.iter().all(|op| matches!(op, RegOp::Set { .. })));
        // The spec's values are all present.
        assert!(ops.contains(&set(
            WINLOGON_KEY,
            "Shell",
            RegValue::Sz(format!("\"{SHELL}\""))
        )));
        assert!(ops.contains(&set(
            POLICIES_SYSTEM_KEY,
            "DisableTaskMgr",
            RegValue::Dword(1)
        )));
        assert!(ops.contains(&set(
            POLICIES_SYSTEM_KEY,
            "DisableRegistryTools",
            RegValue::Dword(1)
        )));
        assert!(ops.contains(&set(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(1))));
        assert!(ops.contains(&set(
            POLICIES_EXPLORER_KEY,
            "NoDrives",
            RegValue::Dword(0x03FF_FFFB)
        )));
        assert!(ops.contains(&set(STICKY_KEYS_KEY, "Flags", RegValue::Sz("506".into()))));
        assert!(ops.contains(&set(
            KEYBOARD_RESPONSE_KEY,
            "Flags",
            RegValue::Sz("122".into())
        )));
        assert!(ops.contains(&set(TOGGLE_KEYS_KEY, "Flags", RegValue::Sz("58".into()))));
        assert_eq!(ALL_DRIVES_BUT_C & 0b100, 0);
        assert_eq!(ALL_DRIVES_BUT_C | 0b100, (1 << 26) - 1);

        let mut hive = Hive::default();
        let backup = apply(&mut hive, None);
        assert_eq!(backup.version, BACKUP_VERSION);
        assert_eq!(backup.entries.len(), ops.len());
        assert!(backup.entries.iter().all(|e| e.previous.is_none()));
        // Every distinct key is recorded, and none existed.
        assert_eq!(backup.keys.len(), distinct_keys(SHELL).len());
        assert!(backup.keys.iter().all(|k| !k.existed));
        assert_eq!(hive.values.len(), ops.len());
    }

    #[test]
    fn applying_twice_keeps_the_original_backup() {
        let mut hive = Hive::default();
        hive.values.insert(
            (POLICIES_EXPLORER_KEY.into(), "NoRun".into()),
            RegValue::Dword(0),
        );
        let first = apply(&mut hive, None);
        let after_first = hive.clone();
        let second = apply(&mut hive, Some(first.clone()));
        assert_eq!(second, first);
        assert_eq!(hive, after_first);
        assert_eq!(
            first
                .entry(POLICIES_EXPLORER_KEY, "NoRun")
                .unwrap()
                .previous,
            Some(RegValue::Dword(0))
        );
        // Explorer policies key existed (it held NoRun).
        assert!(first.key(POLICIES_EXPLORER_KEY).unwrap().existed);
    }

    #[test]
    fn a_name_missing_from_an_old_backup_is_read_from_the_hive() {
        let mut hive = Hive::default();
        let mut old = apply(&mut hive, None);
        old.entries.retain(|e| e.name != "NoViewOnDrive");
        hive.values
            .remove(&(POLICIES_EXPLORER_KEY.to_owned(), "NoViewOnDrive".to_owned()));
        let merged = apply(&mut hive, Some(old));
        assert_eq!(
            merged
                .entry(POLICIES_EXPLORER_KEY, "NoViewOnDrive")
                .unwrap()
                .previous,
            None
        );
        assert_eq!(merged.entries.len(), touched(SHELL).len());
    }

    #[test]
    fn restore_puts_back_values_and_deletes_created_empty_keys() {
        let mut hive = Hive::default();
        hive.values.insert(
            (POLICIES_EXPLORER_KEY.into(), "NoRun".into()),
            RegValue::Dword(0),
        );
        // An unrelated value under an existing key is never touched.
        hive.values.insert(
            (POLICIES_EXPLORER_KEY.into(), "Other".into()),
            RegValue::Sz("x".into()),
        );
        let before = hive.clone();
        let backup = apply(&mut hive, None);
        let ops = plan_restore(&backup);
        assert!(ops.contains(&RegOp::Delete {
            key: WINLOGON_KEY.into(),
            name: "Shell".into()
        }));
        assert!(ops.contains(&set(POLICIES_EXPLORER_KEY, "NoRun", RegValue::Dword(0))));
        // The accessibility keys, created by apply, are dropped if empty.
        assert!(ops.contains(&RegOp::DeleteKeyIfEmpty {
            key: STICKY_KEYS_KEY.into()
        }));
        // An existing key (Explorer policies) is never queued for deletion.
        assert!(!ops.contains(&RegOp::DeleteKeyIfEmpty {
            key: POLICIES_EXPLORER_KEY.into()
        }));
        hive.run(&ops);
        assert_eq!(hive, before);
    }

    #[test]
    fn restore_keeps_a_created_key_that_still_holds_something() {
        let mut hive = Hive::default();
        let backup = apply(&mut hive, None);
        // Someone left a value under the (created) sticky-keys key.
        hive.values.insert(
            (STICKY_KEYS_KEY.into(), "Unrelated".into()),
            RegValue::Dword(7),
        );
        hive.run(&plan_restore(&backup));
        assert_eq!(
            hive.values
                .get(&(STICKY_KEYS_KEY.into(), "Unrelated".into())),
            Some(&RegValue::Dword(7))
        );
    }

    #[test]
    fn raw_previous_values_restore_byte_for_byte() {
        let mut hive = Hive::default();
        let raw = RegValue::Raw {
            kind: 3,
            bytes: vec![0x91, 0, 0, 0],
        };
        hive.values.insert(
            (POLICIES_EXPLORER_KEY.into(), "NoDriveTypeAutoRun".into()),
            raw.clone(),
        );
        let before = hive.clone();
        let backup = apply(&mut hive, None);
        let json = serde_json::to_string(&backup).unwrap();
        let back: LockdownBackup = serde_json::from_str(&json).unwrap();
        assert_eq!(back, backup);
        hive.run(&plan_restore(&back));
        assert_eq!(hive, before);
    }

    #[test]
    fn protected_key_sddl_is_captured_only_for_existing_protected_keys() {
        let mut hive = Hive::default();
        // Winlogon exists; the policies keys and accessibility keys do not.
        hive.values.insert(
            (WINLOGON_KEY.into(), "Shell".into()),
            RegValue::Sz("explorer.exe".into()),
        );
        let snapshot = hive.clone();
        let backup = capture_backup(
            None,
            "player",
            Some("S-1-5-21-1-2-3-1001"),
            SHELL,
            |k, n| snapshot.values.get(&(k.to_owned(), n.to_owned())).cloned(),
            |k| snapshot.key_exists(k),
            |k| (k == WINLOGON_KEY).then(|| "D:(A;;KA;;;BU)".to_owned()),
        );
        assert_eq!(
            backup.key(WINLOGON_KEY).unwrap().previous_sddl.as_deref(),
            Some("D:(A;;KA;;;BU)")
        );
        assert!(backup.key(WINLOGON_KEY).unwrap().existed);
        // A protected key that did not exist records no SDDL.
        assert_eq!(backup.key(POLICIES_SYSTEM_KEY).unwrap().previous_sddl, None);
        assert!(!backup.key(POLICIES_SYSTEM_KEY).unwrap().existed);
        // A non-protected key is never asked for its SDDL.
        assert!(!is_protected(STICKY_KEYS_KEY));
        assert_eq!(backup.key(STICKY_KEYS_KEY).unwrap().previous_sddl, None);
    }

    #[test]
    fn the_shell_is_quoted_once() {
        assert_eq!(shell_command(r"C:\a b\k.exe"), r#""C:\a b\k.exe""#);
        assert_eq!(shell_command(r#""C:\a b\k.exe""#), r#""C:\a b\k.exe""#);
    }
}

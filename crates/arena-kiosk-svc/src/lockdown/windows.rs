//! The Windows executor: the local player account (`NetUserAdd`), its profile
//! (`CreateProfile`) and its hive (`HKEY_USERS\<SID>` when signed in, else its `NTUSER.DAT`
//! loaded under `HKEY_USERS\ArenaPlayerTemp` with `RegLoadKeyW`). Nothing here writes HKLM or
//! another account's hive. Run elevated (the installer does).

use std::io;
use std::sync::{Mutex, MutexGuard, OnceLock};

use arena_kiosk_core::lockdown::RegValue;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER,
    ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE, HLOCAL, LUID, LocalFree, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::NetManagement::{
    LG_INCLUDE_INDIRECT, LOCALGROUP_USERS_INFO_0, NERR_Success, NERR_UserNotFound,
    NetApiBufferFree, NetUserAdd, NetUserGetInfo, NetUserGetLocalGroups, UF_DONT_EXPIRE_PASSWD,
    UF_PASSWD_CANT_CHANGE, UF_SCRIPT, USER_INFO_1, USER_PRIV_ADMIN, USER_PRIV_USER,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, DACL_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_BACKUP_NAME,
    SE_PRIVILEGE_ENABLED, SE_RESTORE_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE, KEY_WRITE,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegFlushKey, RegGetKeySecurity,
    RegGetValueW, RegLoadKeyW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW,
    RegSetKeySecurity, RegSetValueExW, RegUnLoadKeyW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::CreateProfile;
use windows::core::{PCWSTR, PWSTR};

use super::{PlayerAccount, PlayerAccounts, RegistryBackend, password_from_random};
use crate::winsec::{account_sid_any, random_bytes, user_sid, wide};

/// Where a signed-out player's `NTUSER.DAT` is loaded for the call. A per-run suffix keeps two
/// runs from colliding on the mount point.
pub const TEMP_HIVE_PREFIX: &str = "ArenaPlayerTemp";

/// `READ_CONTROL` and `WRITE_DAC` as registry access masks (reading and writing a key's DACL).
const READ_CONTROL: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0002_0000);
const WRITE_DAC: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000);

/// Built-in alias RIDs (`S-1-5-32-<rid>`) whose members can escape a kiosk: Administrators,
/// Power Users, Backup Operators, Remote Desktop Users and Hyper-V Administrators.
const PRIVILEGED_ALIAS_RIDS: &[&str] = &["544", "547", "551", "555", "578"];

/// Serializes `with_hive` (and the temp-hive mount) across threads in this process.
fn hive_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// A held machine-wide named mutex, released on drop. Serializes `with_hive` across processes
/// (a service start and an installer run), so two hive mounts never overlap.
struct NamedMutex(HANDLE);

impl NamedMutex {
    fn acquire() -> io::Result<Self> {
        use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let name = wide(r"Global\ArenaKioskLockdown");
        // SAFETY: NUL-terminated name; the handle is owned by the guard.
        let h = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|e| io::Error::other(format!("CreateMutexW: {e}")))?;
        // SAFETY: an owned mutex handle.
        let w = unsafe { WaitForSingleObject(h, 60_000) };
        if w != WAIT_OBJECT_0 && w != WAIT_ABANDONED {
            // SAFETY: owned handle.
            unsafe {
                let _ = CloseHandle(h);
            }
            return Err(io::Error::other(
                "another lockdown run holds the lock; try again shortly",
            ));
        }
        Ok(Self(h))
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::ReleaseMutex;
        // SAFETY: an owned, acquired mutex handle.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn reg_err(what: &str, rc: WIN32_ERROR) -> io::Error {
    let e = io::Error::from_raw_os_error(rc.0 as i32);
    io::Error::new(e.kind(), format!("{what}: {e}"))
}

fn check(what: &str, rc: WIN32_ERROR) -> io::Result<()> {
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(reg_err(what, rc))
    }
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: an owned, open key.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(
    root: HKEY,
    sub: &str,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> io::Result<Option<Key>> {
    let w = wide(sub);
    let mut h = HKEY::default();
    // SAFETY: `w` is NUL-terminated; `h` is owned by the guard on success.
    let rc = unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), None, access, &mut h) };
    match rc {
        ERROR_SUCCESS => Ok(Some(Key(h))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        rc => Err(reg_err(&format!("open {sub}"), rc)),
    }
}

/// Values under one hive root.
pub struct WinRegistry {
    root: HKEY,
}

impl RegistryBackend for WinRegistry {
    fn get(&self, key: &str, name: &str) -> io::Result<Option<RegValue>> {
        let Some(k) = open(self.root, key, KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let wname = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        // SAFETY: a size query on an open key.
        let rc = unsafe {
            RegQueryValueExW(
                k.0,
                PCWSTR(wname.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut len),
            )
        };
        match rc {
            ERROR_SUCCESS | ERROR_MORE_DATA => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            rc => return Err(reg_err(&format!("query {key}\\{name}"), rc)),
        }
        // The value can grow between the size query and the read; loop on ERROR_MORE_DATA.
        for _ in 0..8 {
            let mut data = vec![0u8; len as usize];
            // SAFETY: `data` holds `len` bytes.
            let rc = unsafe {
                RegQueryValueExW(
                    k.0,
                    PCWSTR(wname.as_ptr()),
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut len),
                )
            };
            match rc {
                ERROR_SUCCESS => {
                    data.truncate(len as usize);
                    return Ok(Some(decode(kind, data)));
                }
                ERROR_MORE_DATA => continue,
                ERROR_FILE_NOT_FOUND => return Ok(None),
                rc => return Err(reg_err(&format!("query {key}\\{name}"), rc)),
            }
        }
        Err(io::Error::other(format!(
            "{key}\\{name} kept growing while being read"
        )))
    }

    fn set(&mut self, key: &str, name: &str, value: &RegValue) -> io::Result<()> {
        let wkey = wide(key);
        let mut h = HKEY::default();
        // SAFETY: `wkey` is NUL-terminated; `h` is owned by the guard on success.
        check(&format!("create {key}"), unsafe {
            RegCreateKeyExW(
                self.root,
                PCWSTR(wkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut h,
                None,
            )
        })?;
        let k = Key(h);
        let (kind, bytes) = encode(value);
        let wname = wide(name);
        // SAFETY: an open key and a valid buffer.
        check(&format!("set {key}\\{name}"), unsafe {
            RegSetValueExW(k.0, PCWSTR(wname.as_ptr()), None, kind, Some(&bytes))
        })
    }

    fn delete(&mut self, key: &str, name: &str) -> io::Result<()> {
        let Some(k) = open(self.root, key, KEY_SET_VALUE)? else {
            return Ok(());
        };
        let wname = wide(name);
        // SAFETY: an open key.
        match unsafe { RegDeleteValueW(k.0, PCWSTR(wname.as_ptr())) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            rc => Err(reg_err(&format!("delete {key}\\{name}"), rc)),
        }
    }

    fn key_exists(&self, key: &str) -> io::Result<bool> {
        Ok(open(self.root, key, KEY_READ)?.is_some())
    }

    fn delete_key_if_empty(&mut self, key: &str) -> io::Result<()> {
        let Some(k) = open(self.root, key, KEY_READ)? else {
            return Ok(());
        };
        let (mut subkeys, mut values) = (0u32, 0u32);
        // SAFETY: an open key; only the two counts are requested.
        check(&format!("query {key}"), unsafe {
            RegQueryInfoKeyW(
                k.0,
                None,
                None,
                None,
                Some(&mut subkeys),
                None,
                None,
                Some(&mut values),
                None,
                None,
                None,
                None,
            )
        })?;
        drop(k);
        if subkeys != 0 || values != 0 {
            return Ok(());
        }
        let wkey = wide(key);
        // SAFETY: NUL-terminated; deletes the (now-empty) subkey of the hive root.
        match unsafe { RegDeleteKeyW(self.root, PCWSTR(wkey.as_ptr())) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            rc => Err(reg_err(&format!("delete key {key}"), rc)),
        }
    }

    fn key_sddl(&self, key: &str) -> io::Result<Option<String>> {
        let Some(k) = open(self.root, key, READ_CONTROL)? else {
            return Ok(None);
        };
        let mut len = 0u32;
        // SAFETY: a size query (the DACL only).
        let rc = unsafe { RegGetKeySecurity(k.0, DACL_SECURITY_INFORMATION, None, &mut len) };
        if rc != ERROR_SUCCESS && rc != ERROR_INSUFFICIENT_BUFFER {
            return Err(reg_err(&format!("read security of {key}"), rc));
        }
        let mut buf = vec![0u8; len as usize];
        let sd = PSECURITY_DESCRIPTOR(buf.as_mut_ptr().cast());
        // SAFETY: `buf` holds `len` bytes.
        check(&format!("read security of {key}"), unsafe {
            RegGetKeySecurity(k.0, DACL_SECURITY_INFORMATION, Some(sd), &mut len)
        })?;
        let mut text = PWSTR::null();
        // SAFETY: `sd` is valid; `text` is LocalAlloc'd, freed below.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .map_err(|e| io::Error::other(e.to_string()))?;
        // SAFETY: a LocalAlloc'd NUL-terminated string.
        let s = unsafe { text.to_string() }.map_err(|e| io::Error::other(e.to_string()));
        unsafe {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        }
        s.map(Some)
    }

    fn protect_key(&mut self, key: &str, player_sid: &str) -> io::Result<()> {
        // SYSTEM and Administrators keep full control; the player may only read. `P` breaks
        // inheritance so a looser parent cannot re-grant the player write.
        let sddl = format!("D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;{player_sid})");
        self.set_key_security(key, &sddl, true)
    }

    fn set_key_sddl(&mut self, key: &str, sddl: &str) -> io::Result<()> {
        self.set_key_security(key, sddl, false)
    }
}

impl WinRegistry {
    /// Applies `sddl` as the key's DACL (breaking inheritance when `protected`).
    fn set_key_security(&self, key: &str, sddl: &str, protected: bool) -> io::Result<()> {
        let Some(k) = open(self.root, key, WRITE_DAC)? else {
            return Ok(());
        };
        let wsddl = wide(sddl);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wsddl` is NUL-terminated; `sd` is LocalAlloc'd, freed below.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wsddl.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(|e| io::Error::other(e.to_string()))?;
        let mut info = DACL_SECURITY_INFORMATION;
        if protected {
            info |= PROTECTED_DACL_SECURITY_INFORMATION;
        }
        // SAFETY: an open key with WRITE_DAC; `sd` is a valid descriptor.
        let rc = unsafe { RegSetKeySecurity(k.0, info, sd) };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(sd.0)));
        }
        check(&format!("set security of {key}"), rc)
    }
}

fn decode(kind: REG_VALUE_TYPE, data: Vec<u8>) -> RegValue {
    if kind == REG_DWORD && data.len() == 4 {
        return RegValue::Dword(u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
    }
    if kind == REG_SZ && data.len().is_multiple_of(2) {
        let mut units: Vec<u16> = data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        while units.last() == Some(&0) {
            units.pop();
        }
        if let Ok(s) = String::from_utf16(&units) {
            // Only when it re-encodes to the same bytes (one trailing NUL), else keep it raw.
            if encode(&RegValue::Sz(s.clone())).1 == data {
                return RegValue::Sz(s);
            }
        }
    }
    RegValue::Raw {
        kind: kind.0,
        bytes: data,
    }
}

fn encode(value: &RegValue) -> (REG_VALUE_TYPE, Vec<u8>) {
    match value {
        RegValue::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
        RegValue::Sz(s) => (
            REG_SZ,
            wide(s).iter().flat_map(|u| u.to_le_bytes()).collect(),
        ),
        RegValue::Raw { kind, bytes } => (REG_VALUE_TYPE(*kind), bytes.clone()),
    }
}

/// The real accounts and hives.
#[derive(Debug, Default)]
pub struct WinAccounts;

/// `USER_INFO_1` of local account `name`, or `None` when there is none.
fn user_priv(name: &str) -> io::Result<Option<u32>> {
    let w = wide(name);
    let mut buf: *mut u8 = std::ptr::null_mut();
    // SAFETY: `buf` receives a NetApi buffer, freed below.
    let rc = unsafe { NetUserGetInfo(PCWSTR::null(), PCWSTR(w.as_ptr()), 1, &mut buf) };
    if rc == NERR_UserNotFound {
        return Ok(None);
    }
    if rc != NERR_Success {
        return Err(io::Error::other(format!("NetUserGetInfo({name}): {rc}")));
    }
    // SAFETY: level 1 returns a USER_INFO_1.
    let privilege = unsafe { (*(buf as *const USER_INFO_1)).usri1_priv.0 };
    unsafe {
        let _ = NetApiBufferFree(Some(buf.cast()));
    }
    Ok(Some(privilege))
}

/// The names of the local groups `name` belongs to (directly and indirectly).
fn local_groups(name: &str) -> io::Result<Vec<String>> {
    let w = wide(name);
    let mut buf: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: `buf` receives a NetApi buffer, freed below.
    let rc = unsafe {
        NetUserGetLocalGroups(
            PCWSTR::null(),
            PCWSTR(w.as_ptr()),
            0,
            LG_INCLUDE_INDIRECT,
            &mut buf,
            u32::MAX,
            &mut read,
            &mut total,
        )
    };
    if rc != NERR_Success {
        return Err(io::Error::other(format!(
            "NetUserGetLocalGroups({name}): {rc}"
        )));
    }
    let mut groups = Vec::new();
    // SAFETY: level 0 returns `read` LOCALGROUP_USERS_INFO_0 entries.
    unsafe {
        let entries =
            std::slice::from_raw_parts(buf as *const LOCALGROUP_USERS_INFO_0, read as usize);
        for e in entries {
            if !e.lgrui0_name.is_null() {
                groups.push(e.lgrui0_name.to_string().unwrap_or_default());
            }
        }
        let _ = NetApiBufferFree(Some(buf.cast()));
    }
    Ok(groups)
}

/// Whether `name` is in any built-in group that could escape the kiosk (by resolving each group
/// to its SID and matching the privileged alias RIDs, so it works in any UI language).
fn in_privileged_group(name: &str) -> io::Result<bool> {
    for group in local_groups(name)? {
        let Ok(sid) = account_sid_any(&group) else {
            continue;
        };
        if let Some(rid) = sid.strip_prefix("S-1-5-32-")
            && PRIVILEGED_ALIAS_RIDS.contains(&rid)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Creates local standard user `name` with a random password nobody is told.
fn create_user(name: &str) -> io::Result<()> {
    let password = loop {
        if let Some(p) = password_from_random(&random_bytes(64)?) {
            break p;
        }
    };
    let mut wname = wide(name);
    let mut wpass = wide(&password);
    drop(password);
    let info = USER_INFO_1 {
        usri1_name: PWSTR(wname.as_mut_ptr()),
        usri1_password: PWSTR(wpass.as_mut_ptr()),
        usri1_priv: USER_PRIV_USER,
        usri1_flags: UF_SCRIPT | UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE,
        ..Default::default()
    };
    let mut parm = 0u32;
    // SAFETY: `info` and its strings live across the call.
    let rc = unsafe {
        NetUserAdd(
            PCWSTR::null(),
            1,
            (&info as *const USER_INFO_1).cast(),
            Some(&mut parm),
        )
    };
    wpass.iter_mut().for_each(|u| *u = 0);
    if rc != NERR_Success {
        return Err(io::Error::other(format!(
            "NetUserAdd({name}): {rc} (parameter {parm})"
        )));
    }
    Ok(())
}

/// Creates the profile of `sid` (an existing one is fine).
fn ensure_profile(name: &str, sid: &str) -> io::Result<()> {
    let wsid = wide(sid);
    let wname = wide(name);
    let mut path = vec![0u16; 260];
    // SAFETY: NUL-terminated inputs and a writable output buffer.
    match unsafe { CreateProfile(PCWSTR(wsid.as_ptr()), PCWSTR(wname.as_ptr()), &mut path) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => Ok(()),
        Err(e) => Err(io::Error::other(format!("CreateProfile({name}): {e}"))),
    }
}

/// The profile folder of `sid` from the (read-only) ProfileList.
fn profile_dir(sid: &str) -> io::Result<String> {
    let key = wide(&format!(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\{sid}"
    ));
    let value = wide("ProfileImagePath");
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: `buf` holds `len` bytes; REG_EXPAND_SZ is expanded by RegGetValueW.
    check("read ProfileImagePath", unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    })?;
    let units = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..units]);
    Ok(s.trim_end_matches('\0').to_owned())
}

/// Enables SeRestorePrivilege and SeBackupPrivilege on this process (needed by RegLoadKeyW).
fn enable_hive_privileges() -> io::Result<()> {
    // SAFETY: plain token calls; the token handle is closed below.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| io::Error::other(format!("OpenProcessToken: {e}")))?;
        let result = (|| {
            for name in [SE_RESTORE_NAME, SE_BACKUP_NAME] {
                let mut luid = LUID::default();
                LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid)
                    .map_err(|e| io::Error::other(format!("LookupPrivilegeValueW: {e}")))?;
                let tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
                    .map_err(|e| io::Error::other(format!("AdjustTokenPrivileges: {e}")))?;
                // Succeeds with ERROR_NOT_ALL_ASSIGNED when the privilege is not held.
                let last = windows::Win32::Foundation::GetLastError();
                if last != ERROR_SUCCESS {
                    return Err(io::Error::other(
                        "the backup and restore privileges are not held; run elevated",
                    ));
                }
            }
            Ok(())
        })();
        let _ = CloseHandle(token);
        result
    }
}

/// The temporarily loaded hive; unloaded on drop. Each run mounts under a unique name so two
/// runs never collide on the mount point.
struct LoadedHive {
    root: HKEY,
    mount: String,
}

impl LoadedHive {
    fn load(ntuser: &str) -> io::Result<Self> {
        enable_hive_privileges()?;
        let mount = format!("{TEMP_HIVE_PREFIX}-{}", std::process::id());
        let sub = wide(&mount);
        // A mount left behind by a crashed run at this same name.
        // SAFETY: NUL-terminated name.
        unsafe {
            let _ = RegUnLoadKeyW(HKEY_USERS, PCWSTR(sub.as_ptr()));
        }
        let file = wide(ntuser);
        // SAFETY: NUL-terminated inputs.
        check(&format!("load {ntuser}"), unsafe {
            RegLoadKeyW(HKEY_USERS, PCWSTR(sub.as_ptr()), PCWSTR(file.as_ptr()))
        })?;
        match open(HKEY_USERS, &mount, KEY_READ | KEY_WRITE) {
            Ok(Some(root)) => {
                let h = root.0;
                // Closed by this guard's drop, before the unload.
                std::mem::forget(root);
                Ok(Self { root: h, mount })
            }
            other => {
                unload(&mount);
                Err(match other {
                    Err(e) => e,
                    _ => io::Error::other("the loaded hive cannot be opened"),
                })
            }
        }
    }
}

/// Unloads `HKEY_USERS\<mount>`, retrying while a handle is still being closed.
fn unload(mount: &str) {
    let sub = wide(mount);
    for _ in 0..20 {
        // SAFETY: NUL-terminated name.
        if unsafe { RegUnLoadKeyW(HKEY_USERS, PCWSTR(sub.as_ptr())) } == ERROR_SUCCESS {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    tracing::error!("HKEY_USERS\\{mount} could not be unloaded");
    eprintln!("lockdown: HKEY_USERS\\{mount} could not be unloaded; reboot to release it");
}

impl Drop for LoadedHive {
    fn drop(&mut self) {
        // SAFETY: an open key, closed once.
        unsafe {
            let _ = RegFlushKey(self.root);
            let _ = RegCloseKey(self.root);
        }
        unload(&self.mount);
    }
}

impl PlayerAccounts for WinAccounts {
    fn ensure_player(&mut self, name: &str) -> io::Result<PlayerAccount> {
        let created = match user_priv(name)? {
            Some(p) if p == USER_PRIV_ADMIN.0 => {
                return Err(io::Error::other(format!(
                    "{name} is an administrator; the lockdown is only for a standard account"
                )));
            }
            Some(_) => {
                // A standard user can still be in a privileged group (Backup Operators, RDP
                // Users, Power Users, Hyper-V Admins): those escape the kiosk too.
                if in_privileged_group(name)? {
                    return Err(io::Error::other(format!(
                        "{name} is in a privileged group (Administrators, Power Users, Backup \
                         Operators, Remote Desktop Users or Hyper-V Administrators); not locking \
                         it down"
                    )));
                }
                false
            }
            None => {
                create_user(name)?;
                true
            }
        };
        let sid = user_sid(name)?;
        ensure_profile(name, &sid)?;
        Ok(PlayerAccount { sid, created })
    }

    fn find_player(&mut self, name: &str) -> io::Result<Option<String>> {
        if user_priv(name)?.is_none() {
            return Ok(None);
        }
        user_sid(name).map(Some)
    }

    fn with_hive(
        &mut self,
        _name: &str,
        sid: &str,
        f: &mut dyn FnMut(&mut dyn RegistryBackend) -> io::Result<()>,
    ) -> io::Result<()> {
        // One hive mount and one set of writes at a time: within this process and across
        // processes (a service start racing an installer run).
        let _guard = hive_lock();
        let _machine = NamedMutex::acquire()?;
        // Signed in: the hive is already loaded under its SID.
        if let Some(root) = open(HKEY_USERS, sid, KEY_READ | KEY_WRITE)? {
            let mut reg = WinRegistry { root: root.0 };
            let r = f(&mut reg);
            // SAFETY: an open key.
            unsafe {
                let _ = RegFlushKey(root.0);
            }
            return r;
        }
        let ntuser = format!(r"{}\NTUSER.DAT", profile_dir(sid)?);
        let hive = LoadedHive::load(&ntuser)?;
        let mut reg = WinRegistry { root: hive.root };
        let r = f(&mut reg);
        drop(hive);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_their_registry_encoding() {
        for v in [
            RegValue::Dword(0x03FF_FFFB),
            RegValue::Sz(r#""C:\Program Files\Arena 360 Kiosk\arena-kiosk-ui.exe""#.into()),
            RegValue::Raw {
                kind: 3,
                bytes: vec![1, 2, 3],
            },
        ] {
            let (kind, bytes) = encode(&v);
            assert_eq!(decode(kind, bytes), v);
        }
        // An odd REG_SZ (no terminator) is kept raw so it is restored byte for byte.
        let odd = vec![b'a', 0];
        assert_eq!(
            decode(REG_SZ, odd.clone()),
            RegValue::Raw {
                kind: REG_SZ.0,
                bytes: odd
            }
        );
    }
}

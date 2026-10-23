//! Windows security helpers: the protected data directory, file owner checks, account SIDs and
//! OS randomness.
//!
//! The data directory (`%PROGRAMDATA%\Arena360\Kiosk`) gets a protected DACL that grants full
//! control to SYSTEM and Administrators only (inherited by everything inside, nothing inherited
//! from `ProgramData`, which would let Users read and create files). The service sets and
//! verifies it at every start and in `service install`, and refuses to start when it cannot.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, IsWellKnownSid, LookupAccountNameW,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SID_NAME_USE, SidTypeUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows::core::{PCWSTR, PWSTR};

/// The data directory's DACL: SYSTEM and Administrators, full control, inherited by children.
pub const DATA_DIR_SDDL: &str = "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    wide(&p.display().to_string())
}

pub(crate) fn win(e: windows::core::Error) -> io::Error {
    io::Error::other(e.to_string())
}

/// Takes a LocalAlloc'd wide string.
unsafe fn take_pwstr(p: PWSTR) -> io::Result<String> {
    // SAFETY: the caller passes a NUL-terminated string from a Win32 call.
    let out = unsafe { p.to_string() }.map_err(|e| io::Error::other(e.to_string()));
    unsafe {
        let _ = LocalFree(Some(HLOCAL(p.0.cast())));
    }
    out
}

/// Sets the protected SYSTEM + Administrators DACL on `dir` (creating it first), then reads it
/// back and fails unless it is exactly that.
pub fn protect_data_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let sddl = wide(DATA_DIR_SDDL);
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` is NUL-terminated; `sd` is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
    }
    .map_err(win)?;
    let result = (|| {
        let mut present = false.into();
        let mut defaulted = false.into();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        // SAFETY: `sd` is a valid descriptor.
        unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) }
            .map_err(win)?;
        let path = wide_path(dir);
        // SAFETY: `path` is NUL-terminated and `dacl` points into `sd`.
        let rc = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl),
                None,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc.0 as i32));
        }
        Ok(())
    })();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    result?;
    let got = dacl_sddl(dir)?;
    if !dacl_is_system_and_admins_only(&got) {
        return Err(io::Error::other(format!(
            "the data directory DACL is {got}, not SYSTEM + Administrators only"
        )));
    }
    Ok(())
}

/// The DACL of `path` as SDDL.
pub fn dacl_sddl(path: &Path) -> io::Result<String> {
    let w = wide_path(path);
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sd` receives a LocalAlloc'd descriptor, freed below.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &mut sd,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(rc.0 as i32));
    }
    let mut text = PWSTR::null();
    // SAFETY: `sd` is valid; `text` receives a LocalAlloc'd string.
    let r = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
    };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    r.map_err(win)?;
    unsafe { take_pwstr(text) }
}

/// Whether an SDDL DACL is protected and every ACE is an allow for SYSTEM or Administrators.
pub fn dacl_is_system_and_admins_only(sddl: &str) -> bool {
    let Some(rest) = sddl.strip_prefix("D:P") else {
        return false;
    };
    let aces: Vec<&str> = rest
        .split('(')
        .skip(1)
        .map(|a| a.trim_end_matches(')'))
        .collect();
    !aces.is_empty()
        && aces.iter().all(|ace| {
            let f: Vec<&str> = ace.split(';').collect();
            f.len() >= 6 && f[0] == "A" && matches!(f[5], "SY" | "BA")
        })
}

/// Whether `path` is owned by SYSTEM or Administrators (anything else could have been planted
/// by a less trusted account).
pub fn owner_is_trusted(path: &Path) -> io::Result<bool> {
    let w = wide_path(path);
    let mut owner = PSID::default();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `owner` points into `sd`, freed below after use.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(w.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut sd,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(rc.0 as i32));
    }
    // SAFETY: `owner` is valid while `sd` lives.
    let trusted = unsafe {
        IsWellKnownSid(owner, WinLocalSystemSid).as_bool()
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
    };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    Ok(trusted)
}

/// The string SID of local *user* account `name` (a group or alias is refused).
pub fn user_sid(name: &str) -> io::Result<String> {
    let wname = wide(name);
    let mut sid_len = 0u32;
    let mut dom_len = 0u32;
    let mut use_ = SID_NAME_USE::default();
    // SAFETY: a size query.
    let _ = unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(wname.as_ptr()),
            None,
            &mut sid_len,
            None,
            &mut dom_len,
            &mut use_,
        )
    };
    if sid_len == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no account named {name}"),
        ));
    }
    let mut sid = vec![0u8; sid_len as usize];
    let mut dom = vec![0u16; dom_len.max(1) as usize];
    // SAFETY: the buffers have the sizes the first call reported.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(wname.as_ptr()),
            Some(PSID(sid.as_mut_ptr().cast())),
            &mut sid_len,
            Some(PWSTR(dom.as_mut_ptr())),
            &mut dom_len,
            &mut use_,
        )
    }
    .map_err(win)?;
    if use_ != SidTypeUser {
        return Err(io::Error::other(format!("{name} is not a user account")));
    }
    sid_string(PSID(sid.as_mut_ptr().cast()))
}

/// A SID as its `S-1-…` string.
pub fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: the caller passes a valid SID.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(win)?;
    unsafe { take_pwstr(text) }
}

/// `n` bytes from the OS RNG.
pub fn random_bytes(n: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    // SAFETY: `buf` is writable for its length.
    let st = unsafe { BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if st.is_err() {
        return Err(io::Error::other(format!("BCryptGenRandom: {:#x}", st.0)));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_protected_dir_is_system_and_admins_only() {
        assert!(dacl_is_system_and_admins_only(DATA_DIR_SDDL));
        assert!(!dacl_is_system_and_admins_only("D:AI(A;OICI;FA;;;SY)"));
        assert!(!dacl_is_system_and_admins_only(
            "D:P(A;OICI;FA;;;SY)(A;OICI;FR;;;BU)"
        ));
        assert!(!dacl_is_system_and_admins_only("D:P"));
    }
}

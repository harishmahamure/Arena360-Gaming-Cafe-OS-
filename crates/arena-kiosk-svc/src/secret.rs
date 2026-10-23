//! The device token at rest.
//!
//! - Windows: DPAPI with machine scope (`CryptProtectData` + `CRYPTPROTECT_LOCAL_MACHINE`) plus
//!   optional entropy: 32 random bytes generated once into `<data>\entropy.bin`. Machine scope
//!   alone would let any local account decrypt the blob; the entropy lives in the protected data
//!   folder (SYSTEM and Administrators only), so only they can. The token is never written
//!   unencrypted on Windows, and a token or entropy file not owned by SYSTEM/Administrators is
//!   refused.
//! - Elsewhere (dev and tests only): a plain file with mode 0600, and a `warn` line saying it is
//!   not encrypted.

use std::io;
use std::path::{Path, PathBuf};

/// Keeps one secret.
pub trait SecretStore: Send + Sync {
    /// Replaces the stored secret.
    fn save(&self, secret: &[u8]) -> io::Result<()>;
    /// The stored secret, or `None` when there is none yet.
    fn load(&self) -> io::Result<Option<Vec<u8>>>;
}

/// The platform's store for the token file at `path`.
pub fn platform_store(path: impl Into<PathBuf>) -> Box<dyn SecretStore> {
    #[cfg(windows)]
    {
        Box::new(dpapi::DpapiStore { path: path.into() })
    }
    #[cfg(not(windows))]
    {
        Box::new(PlainFileStore { path: path.into() })
    }
}

/// The DPAPI entropy file (Windows), next to the token.
pub const ENTROPY_FILE: &str = "entropy.bin";

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Dev and test builds off Windows: the secret as a 0600 file, NOT encrypted.
#[cfg(not(windows))]
#[derive(Debug, Clone)]
pub struct PlainFileStore {
    pub path: PathBuf,
}

#[cfg(not(windows))]
impl SecretStore for PlainFileStore {
    fn save(&self, secret: &[u8]) -> io::Result<()> {
        tracing::warn!(
            path = %self.path.display(),
            "the device token is stored NOT encrypted (non-Windows dev build)"
        );
        crate::config::write_atomic(&self.path, secret)
    }

    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        let found = read_optional(&self.path)?;
        if found.is_some() {
            tracing::warn!(
                path = %self.path.display(),
                "reading a device token that is NOT encrypted (non-Windows dev build)"
            );
        }
        Ok(found)
    }
}

#[cfg(windows)]
mod dpapi {
    use super::*;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
        CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;

    /// DPAPI, machine scope.
    #[derive(Debug, Clone)]
    pub struct DpapiStore {
        pub path: PathBuf,
    }

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        }
    }

    /// Copies DPAPI's output and frees it.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        }
        v
    }

    /// The service-only entropy next to `token`: read, or generated once (`create_new`).
    pub fn entropy(token: &std::path::Path) -> io::Result<Vec<u8>> {
        use std::io::Write;
        let path = token.with_file_name(ENTROPY_FILE);
        match std::fs::read(&path) {
            Ok(b) if b.len() == 32 => {
                crate::config::check_owner(&path)?;
                return Ok(b);
            }
            Ok(_) => return Err(io::Error::other("entropy.bin is corrupt")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let fresh = crate::winsec::random_bytes(32)?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        f.write_all(&fresh)?;
        f.sync_all()?;
        Ok(fresh)
    }

    pub fn protect(plain: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(plain);
        let extra = blob(entropy);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` borrows `plain` for the call; `out` is written by DPAPI.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&extra),
                None,
                None,
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|e| io::Error::other(format!("CryptProtectData: {e}")))?;
        Ok(take(out))
    }

    pub fn unprotect(sealed: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(sealed);
        let extra = blob(entropy);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&extra),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|e| io::Error::other(format!("CryptUnprotectData: {e}")))?;
        Ok(take(out))
    }

    impl SecretStore for DpapiStore {
        fn save(&self, secret: &[u8]) -> io::Result<()> {
            let sealed = protect(secret, &entropy(&self.path)?)?;
            crate::config::write_atomic(&self.path, &sealed)
        }

        fn load(&self) -> io::Result<Option<Vec<u8>>> {
            match read_optional(&self.path)? {
                None => Ok(None),
                Some(sealed) => {
                    crate::config::check_owner(&self.path)?;
                    unprotect(&sealed, &entropy(&self.path)?).map(Some)
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dpapi_round_trips_and_the_file_is_not_the_plain_token() {
            let dir = tempfile::tempdir().unwrap();
            let store = DpapiStore {
                path: dir.path().join("token.bin"),
            };
            assert_eq!(store.load().unwrap(), None);
            let token = b"01JDEVICE.0123456789abcdef";
            store.save(token).unwrap();
            let on_disk = std::fs::read(&store.path).unwrap();
            assert!(!on_disk.windows(token.len()).any(|w| w == token));
            assert_eq!(store.load().unwrap().as_deref(), Some(&token[..]));
            // Without the service's entropy the blob does not open.
            assert!(unprotect(&on_disk, &[0u8; 32]).is_err());
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_dev_file_is_owner_only_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlainFileStore {
            path: dir.path().join("token.bin"),
        };
        assert_eq!(store.load().unwrap(), None);
        store.save(b"secret").unwrap();
        store.save(b"secret-2").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some(&b"secret-2"[..]));
        let mode = std::fs::metadata(&store.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

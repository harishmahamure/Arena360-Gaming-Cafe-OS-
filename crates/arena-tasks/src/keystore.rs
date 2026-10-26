use std::io;
use std::path::{Path, PathBuf};

use arena_core::players::{KEK_LEN, Kek};
use arena_proto::{AppError, ErrorCode};

pub const HUB_TOKEN_FILE: &str = "hub-token.bin";
pub const ENTROPY_FILE: &str = "hub-token.entropy";

pub trait Keystore: Send + Sync {
    fn put(&self, secret: &[u8]) -> io::Result<()>;
    fn get(&self) -> io::Result<Option<Vec<u8>>>;
}

pub const KEK_FILE: &str = "branch-kek.bin";

pub fn platform_keystore(data_dir: &Path) -> Box<dyn Keystore> {
    named_keystore(data_dir, HUB_TOKEN_FILE)
}

pub fn named_keystore(data_dir: &Path, file: &str) -> Box<dyn Keystore> {
    let path = data_dir.join(file);
    #[cfg(windows)]
    {
        Box::new(DpapiKeystore { path })
    }
    #[cfg(not(windows))]
    {
        Box::new(FileKeystore { path })
    }
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn random_bytes(n: usize) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut out)
        .map_err(|_| io::Error::other("the system random source failed"))?;
    Ok(out)
}

pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let nonce: String = random_bytes(8)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let tmp = path.with_file_name(format!(".{name}.{nonce}.tmp"));
    let written = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[derive(Debug, Clone)]
pub struct FileKeystore {
    pub path: PathBuf,
}

impl Keystore for FileKeystore {
    fn put(&self, secret: &[u8]) -> io::Result<()> {
        if !cfg!(windows) {
            tracing::warn!(
                path = %self.path.display(),
                "a secret is stored NOT encrypted (0600 file, non-Windows dev build)"
            );
        }
        write_private(&self.path, secret)
    }

    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        read_optional(&self.path)
    }
}

pub struct KeystoreKek {
    store: Box<dyn Keystore>,
    cached: std::sync::Mutex<Option<[u8; KEK_LEN]>>,
}

impl KeystoreKek {
    pub fn new(store: Box<dyn Keystore>) -> Self {
        Self {
            store,
            cached: std::sync::Mutex::new(None),
        }
    }
}

fn kek_error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("branch key store: {e}"))
}

impl Kek for KeystoreKek {
    fn kek(&self) -> Result<[u8; KEK_LEN], AppError> {
        let mut cached = self.cached.lock().map_err(kek_error)?;
        if let Some(k) = *cached {
            return Ok(k);
        }
        let key: [u8; KEK_LEN] = match self.store.get().map_err(kek_error)? {
            Some(b) => b
                .try_into()
                .map_err(|_| kek_error("the stored key is corrupt"))?,
            None => {
                let fresh: [u8; KEK_LEN] = random_bytes(KEK_LEN)
                    .map_err(kek_error)?
                    .try_into()
                    .map_err(|_| kek_error("bad key length"))?;
                self.store.put(&fresh).map_err(kek_error)?;
                fresh
            }
        };
        *cached = Some(key);
        Ok(key)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AndroidKeystore;

impl Keystore for AndroidKeystore {
    fn put(&self, _secret: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the Android Keystore ships with the Android Hub",
        ))
    }

    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the Android Keystore ships with the Android Hub",
        ))
    }
}

#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct DpapiKeystore {
    pub path: PathBuf,
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

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        }
        v
    }

    fn entropy(token: &Path) -> io::Result<Vec<u8>> {
        let path = token.with_file_name(ENTROPY_FILE);
        match std::fs::read(&path) {
            Ok(b) if b.len() == 32 => return Ok(b),
            Ok(_) => return Err(io::Error::other("the hub token entropy is corrupt")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let fresh = random_bytes(32)?;
        write_private(&path, &fresh)?;
        Ok(fresh)
    }

    pub fn protect(plain: &[u8], extra: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(plain);
        let extra = blob(extra);
        let mut out = CRYPT_INTEGER_BLOB::default();
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

    pub fn unprotect(sealed: &[u8], extra: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(sealed);
        let extra = blob(extra);
        let mut out = CRYPT_INTEGER_BLOB::default();
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

    impl Keystore for DpapiKeystore {
        fn put(&self, secret: &[u8]) -> io::Result<()> {
            let sealed = protect(secret, &entropy(&self.path)?)?;
            write_private(&self.path, &sealed)
        }

        fn get(&self) -> io::Result<Option<Vec<u8>>> {
            match read_optional(&self.path)? {
                None => Ok(None),
                Some(sealed) => unprotect(&sealed, &entropy(&self.path)?).map(Some),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platform_keystore_round_trips_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let ks = platform_keystore(dir.path());
        assert_eq!(ks.get().unwrap(), None);
        ks.put(b"hub_01H.secret-one").unwrap();
        assert_eq!(
            ks.get().unwrap().as_deref(),
            Some(&b"hub_01H.secret-one"[..])
        );
        ks.put(b"hub_01H.secret-two").unwrap();
        assert_eq!(
            ks.get().unwrap().as_deref(),
            Some(&b"hub_01H.secret-two"[..])
        );
        let again = platform_keystore(dir.path());
        assert_eq!(
            again.get().unwrap().as_deref(),
            Some(&b"hub_01H.secret-two"[..])
        );
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn the_dev_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        platform_keystore(dir.path()).put(b"x").unwrap();
        let mode = std::fs::metadata(dir.path().join(HUB_TOKEN_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_never_writes_the_plain_token() {
        let dir = tempfile::tempdir().unwrap();
        platform_keystore(dir.path())
            .put(b"hub_01H.plain-secret")
            .unwrap();
        let file = std::fs::read(dir.path().join(HUB_TOKEN_FILE)).unwrap();
        assert!(!file.windows(12).any(|w| w == b"plain-secret"));
    }

    #[test]
    fn the_branch_key_is_minted_once_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let first = KeystoreKek::new(named_keystore(dir.path(), KEK_FILE))
            .kek()
            .unwrap();
        let again = KeystoreKek::new(named_keystore(dir.path(), KEK_FILE));
        assert_eq!(again.kek().unwrap(), first);
        assert_eq!(again.kek().unwrap(), first);
        assert_ne!(first, [0; KEK_LEN]);
        assert!(!dir.path().join(HUB_TOKEN_FILE).exists());
        std::fs::write(dir.path().join(KEK_FILE), b"short").unwrap();
        assert!(
            KeystoreKek::new(named_keystore(dir.path(), KEK_FILE))
                .kek()
                .is_err()
        );
    }

    #[test]
    fn the_android_stub_refuses() {
        assert_eq!(
            AndroidKeystore.put(b"x").unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(AndroidKeystore.get().is_err());
    }
}

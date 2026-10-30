use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arena_core::players::{KEK_LEN, Kek};
use arena_proto::{AppError, ErrorCode};

pub const HUB_TOKEN_FILE: &str = "hub-token.bin";
pub const ENTROPY_FILE: &str = "hub-token.entropy";

pub trait Keystore: Send + Sync {
    fn put(&self, secret: &[u8]) -> io::Result<()>;
    fn get(&self) -> io::Result<Option<Vec<u8>>>;
    /// Removes the secret. The default overwrites it with nothing; file stores remove the file.
    fn delete(&self) -> io::Result<()> {
        self.put(&[])
    }
}

pub const KEK_FILE: &str = "branch-kek.bin";
/// The one-time private key of a replacement Hub waiting for the branch key (Story 9.9).
pub const RESTORE_KEY_FILE: &str = "restore-key.bin";

impl<T: Keystore + ?Sized> Keystore for Arc<T> {
    fn put(&self, secret: &[u8]) -> io::Result<()> {
        (**self).put(secret)
    }

    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        (**self).get()
    }

    fn delete(&self) -> io::Result<()> {
        (**self).delete()
    }
}

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

fn remove_optional(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
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

    fn delete(&self) -> io::Result<()> {
        remove_optional(&self.path)
    }
}

pub struct KeystoreKek {
    store: Box<dyn Keystore>,
    cached: std::sync::Mutex<Option<[u8; KEK_LEN]>>,
    blocked_while_lost: Option<arena_core::PairGate>,
}

impl KeystoreKek {
    pub fn new(store: Box<dyn Keystore>) -> Self {
        Self {
            store,
            cached: std::sync::Mutex::new(None),
            blocked_while_lost: None,
        }
    }

    /// Never reads or mints a key while the gate says the Hub's keys are lost: a new key for an
    /// existing branch would silently orphan its encrypted data.
    pub fn refusing_while_lost(mut self, gate: arena_core::PairGate) -> Self {
        self.blocked_while_lost = Some(gate);
        self
    }
}

/// A fresh random branch key, for pairing to write.
pub fn mint_kek() -> io::Result<[u8; KEK_LEN]> {
    random_bytes(KEK_LEN)?
        .try_into()
        .map_err(|_| io::Error::other("bad key length"))
}

fn kek_error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, format!("branch key store: {e}"))
}

impl Kek for KeystoreKek {
    fn kek(&self) -> Result<[u8; KEK_LEN], AppError> {
        if self
            .blocked_while_lost
            .as_ref()
            .is_some_and(|g| g.needs_repair())
        {
            return Err(kek_error("this Hub needs to be paired again"));
        }
        let mut cached = self.cached.lock().map_err(kek_error)?;
        if let Some(k) = *cached {
            return Ok(k);
        }
        let key: [u8; KEK_LEN] = match self.store.get().map_err(kek_error)? {
            Some(b) => b
                .try_into()
                .map_err(|_| kek_error("the stored key is corrupt"))?,
            None => {
                let fresh = mint_kek().map_err(kek_error)?;
                self.store.put(&fresh).map_err(kek_error)?;
                fresh
            }
        };
        *cached = Some(key);
        Ok(key)
    }
}

/// A secret that cannot be unwrapped any more: the platform key that wrapped it is gone (the
/// Android Keystore was reset, the app was restored onto another device, or the key was
/// invalidated). It is not an I/O fault: only a new pairing recovers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyLost;

impl std::fmt::Display for KeyLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the key that wrapped this secret is lost")
    }
}

impl std::error::Error for KeyLost {}

pub fn key_lost_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, KeyLost)
}

/// `true` when a keystore error means the wrapping key is gone, not that the disk failed.
pub fn is_key_lost(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<KeyLost>())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapError {
    /// The wrapping key is missing or the wrapped bytes do not verify under it.
    Lost,
    /// Anything else (the platform call failed); retrying later may work.
    Failed(String),
}

/// Wraps and unwraps small secrets with a platform key that never leaves the platform (the
/// Android Keystore, reached through JNI by the Android shell). The wrapped bytes are opaque.
pub trait SecretWrapper: Send + Sync {
    fn wrap(&self, plain: &[u8]) -> Result<Vec<u8>, WrapError>;
    fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>, WrapError>;
}

/// A keystore file that holds only wrapped bytes. Missing file is `None`; a file that the
/// wrapper cannot open is a "lost" error (see [`is_key_lost`]).
pub struct WrappedKeystore {
    path: PathBuf,
    wrapper: Arc<dyn SecretWrapper>,
}

impl WrappedKeystore {
    pub fn new(path: PathBuf, wrapper: Arc<dyn SecretWrapper>) -> Self {
        Self { path, wrapper }
    }
}

impl Keystore for WrappedKeystore {
    fn put(&self, secret: &[u8]) -> io::Result<()> {
        let wrapped = self.wrapper.wrap(secret).map_err(|e| match e {
            WrapError::Lost => key_lost_error(),
            WrapError::Failed(detail) => io::Error::other(format!("wrap: {detail}")),
        })?;
        write_private(&self.path, &wrapped)
    }

    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        let Some(wrapped) = read_optional(&self.path)? else {
            return Ok(None);
        };
        self.wrapper
            .unwrap(&wrapped)
            .map(Some)
            .map_err(|e| match e {
                WrapError::Lost => key_lost_error(),
                WrapError::Failed(detail) => io::Error::other(format!("unwrap: {detail}")),
            })
    }

    fn delete(&self) -> io::Result<()> {
        remove_optional(&self.path)
    }
}

/// The Android Hub's keystore: files holding bytes wrapped by a non-exportable Android Keystore
/// key. The key lives in Kotlin; Rust only sees the [`SecretWrapper`].
pub struct AndroidKeystore(WrappedKeystore);

impl AndroidKeystore {
    pub fn new(path: PathBuf, wrapper: Arc<dyn SecretWrapper>) -> Self {
        Self(WrappedKeystore::new(path, wrapper))
    }
}

impl Keystore for AndroidKeystore {
    fn put(&self, secret: &[u8]) -> io::Result<()> {
        self.0.put(secret)
    }

    fn get(&self) -> io::Result<Option<Vec<u8>>> {
        self.0.get()
    }

    fn delete(&self) -> io::Result<()> {
        self.0.delete()
    }
}

/// The secret files an Android Hub keeps, as one set: the device token and the branch key.
pub struct AndroidSecrets {
    pub token: Arc<dyn Keystore>,
    pub kek: Arc<dyn Keystore>,
    /// The replacement Hub's one-time key (Story 9.9); absent until a Hub waits for a restore, and
    /// not part of lost-key detection.
    pub restore: Arc<dyn Keystore>,
}

impl AndroidSecrets {
    pub fn new(data_dir: &Path, wrapper: &Arc<dyn SecretWrapper>) -> Self {
        let file = |name: &str| -> Arc<dyn Keystore> {
            Arc::new(AndroidKeystore::new(
                data_dir.join(name),
                Arc::clone(wrapper),
            ))
        };
        Self {
            token: file(HUB_TOKEN_FILE),
            kek: file(KEK_FILE),
            restore: file(RESTORE_KEY_FILE),
        }
    }

    /// Lost-key detection, run at start before any branch key can be minted. A wrapped file that
    /// cannot be unwrapped is lost; so is a missing one while the Hub has a `hub_identity`
    /// (pairing writes both). A plain I/O error is not a lost key.
    pub fn lost(&self, has_identity: bool) -> bool {
        self.lost_expecting(has_identity, true)
    }

    /// As [`Self::lost`]. A Hub waiting to be restored (Story 9.9) never made a branch key, so
    /// with `expects_kek` false a missing branch key file is not a lost key.
    pub fn lost_expecting(&self, has_identity: bool, expects_kek: bool) -> bool {
        [("token", &self.token), ("branch key", &self.kek)]
            .into_iter()
            .any(|(what, store)| match store.get() {
                Ok(None) if what == "branch key" && !expects_kek => false,
                Ok(Some(_)) => false,
                Ok(None) => has_identity,
                Err(e) if is_key_lost(&e) => true,
                Err(e) => {
                    tracing::warn!(secret = what, error = %e, "keystore read failed at start");
                    false
                }
            })
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

        fn delete(&self) -> io::Result<()> {
            remove_optional(&self.path)
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

    /// A stand-in for the Kotlin wrapper: XOR with a key byte plus a check byte, so a wrapper
    /// with another key fails to open what the first one wrapped.
    struct Fake(u8);

    impl SecretWrapper for Fake {
        fn wrap(&self, plain: &[u8]) -> Result<Vec<u8>, WrapError> {
            let mut out = vec![self.0 ^ 0x5a];
            out.extend(plain.iter().map(|b| b ^ self.0));
            Ok(out)
        }

        fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>, WrapError> {
            match wrapped.split_first() {
                Some((check, rest)) if *check == self.0 ^ 0x5a => {
                    Ok(rest.iter().map(|b| b ^ self.0).collect())
                }
                _ => Err(WrapError::Lost),
            }
        }
    }

    fn android(dir: &Path, key: u8) -> AndroidSecrets {
        AndroidSecrets::new(dir, &(Arc::new(Fake(key)) as Arc<dyn SecretWrapper>))
    }

    #[test]
    fn a_wrapped_keystore_round_trips_and_never_writes_the_secret_in_clear() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = android(dir.path(), 0x21);
        assert_eq!(secrets.token.get().unwrap(), None);
        secrets.token.put(b"hub_01H.plain-secret").unwrap();
        assert_eq!(
            secrets.token.get().unwrap().as_deref(),
            Some(&b"hub_01H.plain-secret"[..])
        );
        let file = std::fs::read(dir.path().join(HUB_TOKEN_FILE)).unwrap();
        assert!(!file.windows(12).any(|w| w == b"plain-secret"));
        let again = android(dir.path(), 0x21);
        assert_eq!(
            again.token.get().unwrap().as_deref(),
            Some(&b"hub_01H.plain-secret"[..])
        );
    }

    #[test]
    fn another_wrapper_key_means_lost_not_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        android(dir.path(), 0x21).token.put(b"hub_x.y").unwrap();
        let err = android(dir.path(), 0x22).token.get().unwrap_err();
        assert!(is_key_lost(&err), "{err}");
        assert!(!is_key_lost(&io::Error::other("disk")));
    }

    /// A Keystore that is momentarily unavailable: unwrap fails, but the key is not gone.
    struct Flaky;

    impl SecretWrapper for Flaky {
        fn wrap(&self, plain: &[u8]) -> Result<Vec<u8>, WrapError> {
            Ok(plain.to_vec())
        }

        fn unwrap(&self, _wrapped: &[u8]) -> Result<Vec<u8>, WrapError> {
            Err(WrapError::Failed("keystore busy".into()))
        }
    }

    #[test]
    fn a_transient_keystore_failure_is_never_mistaken_for_a_lost_key() {
        let dir = tempfile::tempdir().unwrap();
        android(dir.path(), 0x21).token.put(b"hub_x.y").unwrap();
        android(dir.path(), 0x21).kek.put(&[7; KEK_LEN]).unwrap();
        let flaky = AndroidSecrets::new(dir.path(), &(Arc::new(Flaky) as Arc<dyn SecretWrapper>));
        let err = flaky.token.get().unwrap_err();
        assert!(!is_key_lost(&err), "{err}");
        assert!(!flaky.lost(true));
    }

    #[test]
    fn lost_detection_needs_an_unwrap_failure_or_a_missing_file_with_an_identity() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = android(dir.path(), 0x21);
        assert!(!secrets.lost(false), "a fresh install is not lost");
        assert!(secrets.lost(true), "no files but an identity is lost");
        secrets.token.put(b"hub_x.y").unwrap();
        assert!(secrets.lost(true), "the branch key file is missing");
        secrets.kek.put(&[7; KEK_LEN]).unwrap();
        assert!(!secrets.lost(true));
        assert!(android(dir.path(), 0x22).lost(true));
        assert!(android(dir.path(), 0x22).lost(false));
    }

    #[test]
    fn a_hub_waiting_to_be_restored_has_no_branch_key_and_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = android(dir.path(), 0x21);
        secrets.token.put(b"hub_x.y").unwrap();
        assert!(secrets.lost(true));
        assert!(!secrets.lost_expecting(true, false));
        // The token is still expected, and an unwrap failure is still a lost key.
        assert!(android(dir.path(), 0x22).lost_expecting(true, false));
    }

    #[test]
    fn the_branch_key_is_not_minted_while_the_hub_needs_to_pair_again() {
        let dir = tempfile::tempdir().unwrap();
        let gate = arena_core::PairGate::needing_repair();
        let kek = KeystoreKek::new(Box::new(AndroidKeystore::new(
            dir.path().join(KEK_FILE),
            Arc::new(Fake(0x21)),
        )))
        .refusing_while_lost(gate.clone());
        assert!(kek.kek().is_err());
        assert!(!dir.path().join(KEK_FILE).exists(), "nothing was minted");
        gate.mark_paired();
        let first = kek.kek().unwrap();
        assert_eq!(kek.kek().unwrap(), first);
        let file = std::fs::read(dir.path().join(KEK_FILE)).unwrap();
        assert!(!file.windows(KEK_LEN).any(|w| w == first));
    }
}

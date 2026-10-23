//! The Hub's LAN TLS certificate: a self-signed ECDSA P-256 certificate generated once at first
//! start (SAN `arena360.local` plus the hostname) and reused afterwards. Clients pin its SHA-256
//! fingerprint; there is no CA.
//!
//! Files: `<data>/tls/hub.crt` and `<data>/tls/hub.key` (PEM). The key file is created
//! owner-only (`0600`) on Unix; on Windows it inherits the ACL of the data directory
//! (`%PROGRAMDATA%\Arena360\Hub`, which the installer restricts).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arena_proto::{AppError, ErrorCode};
use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

/// The name every Hub certificate carries (mDNS arrives in Epic 2).
pub const LOCAL_SAN: &str = "arena360.local";
/// The certificate folder inside the data directory.
pub const TLS_DIR: &str = "tls";
pub const CERT_FILE: &str = "hub.crt";
pub const KEY_FILE: &str = "hub.key";

fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// The SHA-256 of a DER certificate, as lower-case hex.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A loaded or freshly generated Hub certificate.
#[derive(Clone)]
pub struct HubCert {
    pub cert_pem: String,
    key_pem: String,
    /// SHA-256 of the certificate's DER, lower-case hex.
    pub fingerprint: String,
}

impl std::fmt::Debug for HubCert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubCert")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl HubCert {
    /// Parses a PEM certificate and key, computing the fingerprint.
    pub fn from_pem(cert_pem: String, key_pem: String) -> Result<Self, AppError> {
        let der = CertificateDer::from_pem_slice(cert_pem.as_bytes())
            .map_err(|e| internal(format!("hub certificate: {e}")))?;
        PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
            .map_err(|e| internal(format!("hub key: {e}")))?;
        Ok(Self {
            fingerprint: fingerprint(&der),
            cert_pem,
            key_pem,
        })
    }

    /// Generates a new self-signed ECDSA P-256 certificate for `arena360.local` and
    /// `hostname`.
    pub fn generate(hostname: Option<&str>) -> Result<Self, AppError> {
        let mut sans = vec![LOCAL_SAN.to_owned()];
        if let Some(h) = hostname.map(str::trim).filter(|h| is_dns_label_list(h))
            && !sans.iter().any(|s| s.eq_ignore_ascii_case(h))
        {
            sans.push(h.to_owned());
        }
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
            .map_err(|e| internal(format!("generate hub key: {e}")))?;
        let mut params = rcgen::CertificateParams::new(sans)
            .map_err(|e| internal(format!("certificate params: {e}")))?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, LOCAL_SAN);
        // Valid for 25 years: there is no renewal flow; a reinstall regenerates it.
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        params.not_after = rcgen::date_time_ymd(2050, 1, 1);
        let cert = params
            .self_signed(&key)
            .map_err(|e| internal(format!("self-sign hub certificate: {e}")))?;
        Ok(Self {
            fingerprint: fingerprint(cert.der()),
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        })
    }

    /// The rustls server configuration serving this certificate (ring provider, TLS 1.2+).
    pub fn server_config(&self) -> Result<Arc<ServerConfig>, AppError> {
        let cert = CertificateDer::from_pem_slice(self.cert_pem.as_bytes())
            .map_err(|e| internal(format!("hub certificate: {e}")))?;
        let key = PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes())
            .map_err(|e| internal(format!("hub key: {e}")))?;
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| internal(format!("tls versions: {e}")))?
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .map_err(|e| internal(format!("tls server config: {e}")))?;
        Ok(Arc::new(config))
    }
}

/// Whether `h` looks like a DNS name rcgen and clients accept (letters, digits, `-`, `.`).
fn is_dns_label_list(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// This machine's hostname, best effort: `COMPUTERNAME` (Windows), `HOSTNAME`, the kernel's
/// hostname file, then the `hostname` command.
pub fn hostname() -> Option<String> {
    let from_env = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_owned())
        .find(|v| !v.is_empty());
    if from_env.is_some() {
        return from_env;
    }
    for file in ["/proc/sys/kernel/hostname", "/etc/hostname"] {
        if let Ok(v) = fs::read_to_string(file) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_owned());
            }
        }
    }
    let out = std::process::Command::new("hostname").output().ok()?;
    let v = String::from_utf8(out.stdout).ok()?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_owned())
}

/// `<data>/tls`.
pub fn tls_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(TLS_DIR)
}

/// Loads `<data>/tls/hub.{crt,key}`, or generates and (when `persist`) writes them on first
/// start. A follower passes `persist = false`: it reuses the writer's files and, if there are
/// none yet, serves a certificate kept in memory only.
pub fn load_or_create(
    data_dir: &Path,
    hostname: Option<&str>,
    persist: bool,
) -> Result<HubCert, AppError> {
    let dir = tls_dir(data_dir);
    let (cert_path, key_path) = (dir.join(CERT_FILE), dir.join(KEY_FILE));
    match (cert_path.exists(), key_path.exists()) {
        (true, true) => {
            let read = |p: &Path| {
                fs::read_to_string(p).map_err(|e| internal(format!("read {}: {e}", p.display())))
            };
            return HubCert::from_pem(read(&cert_path)?, read(&key_path)?);
        }
        // Never regenerate over half a pair: paired clients pinned the old certificate, and a
        // silent new one would lock them all out. An operator must restore or remove both.
        (true, false) | (false, true) => {
            return Err(internal(format!(
                "tls files incomplete: {} needs both {CERT_FILE} and {KEY_FILE}",
                dir.display()
            )));
        }
        (false, false) => {}
    }
    let cert = HubCert::generate(hostname)?;
    if persist {
        fs::create_dir_all(&dir).map_err(|e| internal(format!("create {}: {e}", dir.display())))?;
        write_file(&key_path, cert.key_pem.as_bytes(), true)?;
        write_file(&cert_path, cert.cert_pem.as_bytes(), false)?;
    }
    Ok(cert)
}

/// Writes `bytes` to `path` through `<file>.tmp` and a rename, so a crash never leaves a
/// half-written file. A stale temp file from an earlier crash is removed first and the temp
/// file is created fresh (`create_new`). `secret` files are owner-only (`0600`) on Unix.
fn write_file(path: &Path, bytes: &[u8], secret: bool) -> Result<(), AppError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let err = |e: std::io::Error| internal(format!("write {}: {e}", path.display()));
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(err(e)),
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp).map_err(err)?;
    // The open mode is filtered by the umask; set it explicitly too.
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(err)?;
    }
    #[cfg(not(unix))]
    let _ = secret;
    f.write_all(bytes).map_err(err)?;
    f.sync_all().map_err(err)?;
    drop(f);
    fs::rename(&tmp, path).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), Some("hub-pc"), true).unwrap();
        assert_eq!(first.fingerprint.len(), 64);
        assert!(
            first
                .fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let again = load_or_create(dir.path(), Some("other"), true).unwrap();
        assert_eq!(again.fingerprint, first.fingerprint);
        first.server_config().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(tls_dir(dir.path()).join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn half_a_pair_fails_instead_of_regenerating() {
        for keep in [CERT_FILE, KEY_FILE] {
            let dir = tempfile::tempdir().unwrap();
            load_or_create(dir.path(), None, true).unwrap();
            let other = if keep == CERT_FILE {
                KEY_FILE
            } else {
                CERT_FILE
            };
            fs::remove_file(tls_dir(dir.path()).join(other)).unwrap();
            let before = fs::read(tls_dir(dir.path()).join(keep)).unwrap();
            let err = load_or_create(dir.path(), None, true).unwrap_err();
            assert_eq!(err.code, ErrorCode::Internal);
            assert!(
                err.message.contains("tls files incomplete"),
                "{}",
                err.message
            );
            assert_eq!(fs::read(tls_dir(dir.path()).join(keep)).unwrap(), before);
        }
    }

    #[test]
    fn stale_temp_files_are_replaced_and_names_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let tls = tls_dir(dir.path());
        fs::create_dir_all(&tls).unwrap();
        fs::write(tls.join("hub.key.tmp"), b"stale").unwrap();
        fs::write(tls.join("hub.crt.tmp"), b"stale").unwrap();
        let cert = load_or_create(dir.path(), None, true).unwrap();
        assert!(!tls.join("hub.key.tmp").exists() && !tls.join("hub.crt.tmp").exists());
        let again = load_or_create(dir.path(), None, true).unwrap();
        assert_eq!(again.fingerprint, cert.fingerprint);
    }

    #[test]
    fn follower_does_not_write_files() {
        let dir = tempfile::tempdir().unwrap();
        let cert = load_or_create(dir.path(), None, false).unwrap();
        assert!(!tls_dir(dir.path()).exists());
        assert_eq!(cert.fingerprint.len(), 64);
    }

    #[test]
    fn sans_name_local_and_hostname() {
        let cert = HubCert::generate(Some("Front-Desk")).unwrap();
        let der = CertificateDer::from_pem_slice(cert.cert_pem.as_bytes()).unwrap();
        let bytes = der.as_ref();
        let has = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
        assert!(has(LOCAL_SAN) && has("Front-Desk"));
        // A name rcgen cannot carry is skipped rather than failing the start.
        HubCert::generate(Some("bad name!")).unwrap();
    }
}

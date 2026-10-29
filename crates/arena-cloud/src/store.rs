//! Object storage: the [`ObjectStore`] trait, an S3 implementation for Cloudflare R2 (MinIO in
//! dev and CI) and an in-memory one for tests.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aws_sdk_s3::config::{
    BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation,
    timeout::TimeoutConfig,
};
use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};

use crate::config::{get_env, is_loopback_http};

/// The backup bucket's endpoint (`https://<account>.r2.cloudflarestorage.com`).
pub const S3_ENDPOINT_ENV: &str = "ARENA_BACKUP_S3_ENDPOINT";
/// The backup bucket (one per environment).
pub const S3_BUCKET_ENV: &str = "ARENA_BACKUP_S3_BUCKET";
/// The bucket-scoped API token's access key id.
pub const S3_ACCESS_KEY_ID_ENV: &str = "ARENA_BACKUP_S3_ACCESS_KEY_ID";
/// The bucket-scoped API token's secret (never logged).
pub const S3_SECRET_ACCESS_KEY_ENV: &str = "ARENA_BACKUP_S3_SECRET_ACCESS_KEY";

/// The longest one S3 operation (with its retries) may take.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(300);

/// An object store failure. The message never carries a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// A flat key → bytes store.
pub trait ObjectStore: Send + Sync {
    /// Writes (or replaces) `key`.
    fn put(&self, key: &str, body: Vec<u8>) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Reads `key`.
    fn get(&self, key: &str) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send;
    /// Every key that starts with `prefix`, in any order.
    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send;
    /// Removes `key` (no error when it is already gone).
    fn delete(&self, key: &str) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// An in-memory store for tests.
#[derive(Debug, Default)]
pub struct MemoryStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    heads: AtomicU64,
    reads: AtomicU64,
    lists: AtomicU64,
}

/// What a store answered, counted (tests of the owner read cache).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreCalls {
    pub heads: u64,
    pub reads: u64,
    pub lists: u64,
}

/// The ETag of `body` in the in-memory store: the SHA-256 of the bytes, quoted like S3's.
fn memory_etag(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("\"{}\"", crate::auth::hex(&Sha256::digest(body)[..16]))
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.objects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every key, sorted.
    pub fn keys(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    /// How many head, read and etag-list calls the owner read path made.
    pub fn calls(&self) -> StoreCalls {
        StoreCalls {
            heads: self.heads.load(Ordering::SeqCst),
            reads: self.reads.load(Ordering::SeqCst),
            lists: self.lists.load(Ordering::SeqCst),
        }
    }

    /// The ETag of `key`, or none when it is not there.
    pub fn head(&self, key: &str) -> Option<String> {
        self.heads.fetch_add(1, Ordering::SeqCst);
        self.lock().get(key).map(|b| memory_etag(b))
    }

    /// `key`'s bytes and ETag, or none when it is not there.
    pub fn read(&self, key: &str) -> Option<(Vec<u8>, String)> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.lock().get(key).map(|b| (b.clone(), memory_etag(b)))
    }

    /// Every key under `prefix` with its ETag.
    pub fn list_etags(&self, prefix: &str) -> Vec<(String, String)> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.lock()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, b)| (k.clone(), memory_etag(b)))
            .collect()
    }
}

impl ObjectStore for MemoryStore {
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError> {
        self.lock().insert(key.to_owned(), body);
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        self.lock()
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError(format!("no object {key}")))
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .lock()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.lock().remove(key);
        Ok(())
    }
}

/// Where the S3 store is and how it signs.
#[derive(Clone, PartialEq, Eq)]
pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("access_key_id", &"[redacted]")
            .field("secret_access_key", &"[redacted]")
            .finish()
    }
}

impl S3Config {
    /// Reads the four `ARENA_BACKUP_S3_*` variables. The endpoint must be `https://` (plain
    /// `http://` only on loopback, for MinIO). Errors name the variable, never its value.
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Self, String> {
        Self::from_env_prefixed(env, BACKUP_S3_PREFIX)
    }

    /// Reads `<prefix>_ENDPOINT`, `_BUCKET`, `_ACCESS_KEY_ID` and `_SECRET_ACCESS_KEY` (the
    /// backup bucket's four are `ARENA_BACKUP_S3_*`, the data bucket's `ARENA_DATA_S3_*`), with
    /// the checks of [`S3Config::from_env`].
    pub fn from_env_prefixed(
        env: &impl Fn(&str) -> Option<OsString>,
        prefix: &str,
    ) -> Result<Self, String> {
        let need = |suffix: &str| {
            let k = format!("{prefix}_{suffix}");
            get_env(env, &k)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{k} is not set"))
        };
        let endpoint = need("ENDPOINT")?;
        if !(endpoint.starts_with("https://") || is_loopback_http(&endpoint)) {
            return Err(format!(
                "{prefix}_ENDPOINT must be an https:// URL (http:// only on loopback)"
            ));
        }
        Ok(Self {
            endpoint,
            bucket: need("BUCKET")?,
            access_key_id: need("ACCESS_KEY_ID")?,
            secret_access_key: need("SECRET_ACCESS_KEY")?,
        })
    }
}

const BACKUP_S3_PREFIX: &str = "ARENA_BACKUP_S3";

/// The `aws-sdk-s3` client for `cfg`: region `auto`, path-style addressing (R2 and MinIO both
/// accept it), checksums only when an operation requires them (R2 rejects the SDK's default
/// CRC trailers), static credentials and the smithy HTTPS client on rustls + ring.
pub(crate) fn s3_client(cfg: &S3Config) -> aws_sdk_s3::Client {
    let http = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
        .build_https();
    let conf = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("auto"))
        .endpoint_url(&cfg.endpoint)
        .force_path_style(true)
        .credentials_provider(Credentials::new(
            &cfg.access_key_id,
            &cfg.secret_access_key,
            None,
            None,
            "arena-backup-env",
        ))
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
        .timeout_config(
            TimeoutConfig::builder()
                .operation_timeout(OPERATION_TIMEOUT)
                .build(),
        )
        .http_client(http)
        .build();
    aws_sdk_s3::Client::from_conf(conf)
}

/// S3 (R2) through `aws-sdk-s3`: region `auto`, path-style addressing (R2 and MinIO both
/// accept it), checksums only when an operation requires them (R2 rejects the SDK's default
/// CRC trailers), and the smithy HTTPS client on rustls + ring with the platform's roots.
#[derive(Clone)]
pub struct S3Store {
    client: aws_sdk_s3::Client,
    bucket: String,
    /// The credentials, only to scrub them from error text.
    redact: [String; 2],
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Store")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl S3Store {
    pub fn new(cfg: &S3Config) -> Self {
        Self {
            client: s3_client(cfg),
            bucket: cfg.bucket.clone(),
            redact: [cfg.access_key_id.clone(), cfg.secret_access_key.clone()],
        }
    }

    /// `what key: <the SDK's error chain>`, with both credentials scrubbed.
    fn err<E>(&self, what: &str, key: &str, e: E) -> StoreError
    where
        E: std::error::Error + 'static,
    {
        let mut msg = format!("{what} {key}: {}", DisplayErrorContext(e));
        for secret in &self.redact {
            if !secret.is_empty() {
                msg = msg.replace(secret.as_str(), "[redacted]");
            }
        }
        StoreError(msg)
    }

    /// Creates the bucket when it does not exist yet. Dev and tests only: production buckets
    /// are made by an operator, and the backup token cannot create buckets.
    pub async fn create_bucket(&self) -> Result<(), StoreError> {
        match self
            .client
            .create_bucket()
            .bucket(&self.bucket)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e)
                if e.as_service_error().is_some_and(|s| {
                    s.is_bucket_already_owned_by_you() || s.is_bucket_already_exists()
                }) =>
            {
                Ok(())
            }
            Err(e) => Err(self.err("create bucket", &self.bucket, e)),
        }
    }
}

impl S3Store {
    /// The ETag of `key`, or none when it is not there.
    pub async fn head(&self, key: &str) -> Result<Option<String>, StoreError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(out.e_tag().map(str::to_owned)),
            Err(e) if e.as_service_error().is_some_and(|s| s.is_not_found()) => Ok(None),
            Err(e) => Err(self.err("head", key, e)),
        }
    }

    /// `key`'s bytes and ETag, or none when it is not there.
    pub async fn read(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, StoreError> {
        let out = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => out,
            Err(e) if e.as_service_error().is_some_and(|s| s.is_no_such_key()) => {
                return Ok(None);
            }
            Err(e) => return Err(self.err("get", key, e)),
        };
        let etag = out.e_tag().unwrap_or_default().to_owned();
        let body = out
            .body
            .collect()
            .await
            .map_err(|e| self.err("read", key, e))?;
        Ok(Some((body.into_bytes().to_vec(), etag)))
    }

    /// Every key under `prefix` with its ETag.
    pub async fn list_etags(&self, prefix: &str) -> Result<Vec<(String, String)>, StoreError> {
        let mut keys = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let out = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .set_continuation_token(token.take())
                .send()
                .await
                .map_err(|e| self.err("list", prefix, e))?;
            keys.extend(out.contents().iter().filter_map(|o| {
                Some((
                    o.key()?.to_owned(),
                    o.e_tag().unwrap_or_default().to_owned(),
                ))
            }));
            match out.next_continuation_token() {
                Some(t) if out.is_truncated() == Some(true) => token = Some(t.to_owned()),
                _ => return Ok(keys),
            }
        }
    }
}

impl ObjectStore for S3Store {
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type("application/octet-stream")
            .body(ByteStream::from(body))
            .send()
            .await
            .map(|_| ())
            .map_err(|e| self.err("put", key, e))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| self.err("get", key, e))?;
        let body = out
            .body
            .collect()
            .await
            .map_err(|e| self.err("read", key, e))?;
        Ok(body.into_bytes().to_vec())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut keys = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let out = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .set_continuation_token(token.take())
                .send()
                .await
                .map_err(|e| self.err("list", prefix, e))?;
            keys.extend(
                out.contents()
                    .iter()
                    .filter_map(|o| o.key().map(str::to_owned)),
            );
            match out.next_continuation_token() {
                Some(t) if out.is_truncated() == Some(true) => token = Some(t.to_owned()),
                _ => return Ok(keys),
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| self.err("delete", key, e))
    }
}

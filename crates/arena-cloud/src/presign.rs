//! Presigned upload links for the data bucket (Story 8.1, AD-4, AD-36): the object keys under a
//! branch's own prefix `tenants/<tenant>/branches/<branch>/`, the signer that turns a key into a
//! short-lived PUT URL, the epoch order the fence uses and the per-branch count of links issued.
//!
//! Signing is local (no network). Nothing here trusts a request: an epoch or a date with a path
//! character is refused, never cleaned up.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use arena_proto::PRESIGN_TTL_S;
use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::presigning::PresigningConfig;

use crate::config::get_env;
use crate::store::{MemoryStore, ObjectStore, S3Config, S3Store, s3_client};

/// The variable-name prefix of the data bucket: `ARENA_DATA_S3_ENDPOINT`, `_BUCKET`,
/// `_ACCESS_KEY_ID` (a secret) and `_SECRET_ACCESS_KEY` (a secret).
pub const DATA_S3_PREFIX: &str = "ARENA_DATA_S3";

/// `--dev`: the MinIO of `deploy/dev/compose.yaml`.
const DEV_ENDPOINT: &str = "http://127.0.0.1:9000";
const DEV_BUCKET: &str = "arena-dev";
const DEV_ACCESS_KEY_ID: &str = "arena-dev";
const DEV_SECRET_ACCESS_KEY: &str = "arena-dev-password";

/// The largest change-log sequence number a batch key holds (twelve digits).
pub const MAX_SEQ: i64 = 999_999_999_999;
/// The longest instance part of an epoch.
const MAX_INSTANCE_CHARS: usize = 64;

/// Whether `s` has a character that moves a key out of its place: a path separator or `..`.
pub fn has_path_chars(s: &str) -> bool {
    s.contains(['/', '\\']) || s.contains("..")
}

/// Why an epoch was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochError {
    /// A `/`, `\` or `..` in it.
    PathCharacters,
    /// Not `<counter>:<instance>`.
    Malformed,
}

/// A Hub's epoch, `<counter>:<instance>`: the counter is decimal digits, the instance 1 to 64
/// letters, digits, `-` or `_`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epoch {
    text: String,
    counter: u64,
    instance: String,
}

impl Epoch {
    pub fn parse(s: &str) -> Result<Self, EpochError> {
        if has_path_chars(s) {
            return Err(EpochError::PathCharacters);
        }
        let (counter, instance) = s.split_once(':').ok_or(EpochError::Malformed)?;
        let counter = (!counter.is_empty()
            && counter.len() <= 20
            && counter.bytes().all(|b| b.is_ascii_digit()))
        .then(|| counter.parse::<u64>().ok())
        .flatten()
        .ok_or(EpochError::Malformed)?;
        let instance_ok = !instance.is_empty()
            && instance.len() <= MAX_INSTANCE_CHARS
            && instance
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        if !instance_ok {
            return Err(EpochError::Malformed);
        }
        Ok(Self {
            text: s.to_owned(),
            counter,
            instance: instance.to_owned(),
        })
    }

    /// The epoch as it was written.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether this epoch is older than `fence`: by counter (numerically), then by instance.
    pub fn below(&self, fence: &Self) -> bool {
        (self.counter, &self.instance) < (fence.counter, &fence.instance)
    }
}

/// Whether a request at `epoch` is below the branch's `fence` (the stored text). With no fence
/// yet nothing is below it. A stored fence that is not an epoch cannot be ordered, so only the
/// same text passes.
pub fn is_fenced(epoch: &Epoch, fence: Option<&str>) -> bool {
    match fence {
        None => false,
        Some(f) => match Epoch::parse(f) {
            Ok(f) => epoch.below(&f),
            Err(_) => epoch.as_str() != f,
        },
    }
}

/// `tenants/<tenant>/branches/<branch>/`.
pub fn branch_prefix(tenant_id: &str, branch_id: &str) -> String {
    format!("tenants/{tenant_id}/branches/{branch_id}/")
}

/// `changes/<epoch>/<first:012>-<last:012>.zst` under the prefix.
pub fn batch_key(prefix: &str, epoch: &Epoch, first: i64, last: i64) -> String {
    format!(
        "{prefix}changes/{}/{first:012}-{last:012}.zst",
        epoch.as_str()
    )
}

/// `state/latest.json` under the prefix.
pub fn summary_key(prefix: &str) -> String {
    format!("{prefix}state/latest.json")
}

/// `rollups/<date>.json` under the prefix; `date` is a validated `yyyy-mm-dd`.
pub fn rollup_key(prefix: &str, date: &str) -> String {
    format!("{prefix}rollups/{date}.json")
}

/// `snapshots/<date>.db.zst` under the prefix; `date` is a validated `yyyy-mm-dd`.
pub fn snapshot_key(prefix: &str, date: &str) -> String {
    format!("{prefix}snapshots/{date}.db.zst")
}

/// `keys/player-keys.bin` under the prefix (Story 9.2): the player key backup, overwritten.
pub fn keys_key(prefix: &str) -> String {
    format!("{prefix}{}", arena_proto::KEYS_OBJECT_KEY)
}

/// Turns object keys into presigned PUT URLs for the data bucket.
#[derive(Clone)]
pub struct Presigner {
    client: aws_sdk_s3::Client,
    bucket: String,
    /// The credentials, only to scrub them from error text.
    redact: [String; 2],
    /// Lists the bucket (Story 8.3): the same bucket and credentials as the links.
    lister: S3Store,
    /// Tests: keys listed from memory instead of the bucket.
    listing: Option<Arc<MemoryStore>>,
}

impl std::fmt::Debug for Presigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presigner")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl Presigner {
    pub fn new(cfg: &S3Config) -> Self {
        Self {
            client: s3_client(cfg),
            bucket: cfg.bucket.clone(),
            redact: [cfg.access_key_id.clone(), cfg.secret_access_key.clone()],
            lister: S3Store::new(cfg),
            listing: None,
        }
    }

    /// Tests: lists the keys of `keys` instead of the bucket.
    pub fn with_memory_listing(mut self, keys: Arc<MemoryStore>) -> Self {
        self.listing = Some(keys);
        self
    }

    /// Every key under `prefix` in the data bucket, in any order (Story 8.3).
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        match &self.listing {
            Some(mem) => mem.list(prefix).await,
            None => self.lister.list(prefix).await,
        }
        .map_err(|e| e.to_string())
    }

    /// Writes `body` to `key` in the data bucket with the same credentials as the links (Story
    /// 8.36: the audit builder's output).
    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), String> {
        match &self.listing {
            Some(mem) => mem.put(key, body).await,
            None => self.lister.put(key, body).await,
        }
        .map_err(|e| e.to_string())
    }

    /// Removes `key` from the data bucket (no error when it is already gone).
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        match &self.listing {
            Some(mem) => mem.delete(key).await,
            None => self.lister.delete(key).await,
        }
        .map_err(|e| e.to_string())
    }

    /// The ETag of `key` in the data bucket, or none when it is not there (Story 8.9).
    pub async fn head(&self, key: &str) -> Result<Option<String>, String> {
        match &self.listing {
            Some(mem) => Ok(mem.head(key)),
            None => self.lister.head(key).await.map_err(|e| e.to_string()),
        }
    }

    /// `key`'s bytes and ETag from the data bucket, or none when it is not there (Story 8.9).
    pub async fn read(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, String> {
        match &self.listing {
            Some(mem) => Ok(mem.read(key)),
            None => self.lister.read(key).await.map_err(|e| e.to_string()),
        }
    }

    /// Every key under `prefix` in the data bucket with its ETag (Story 8.9).
    pub async fn list_etags(&self, prefix: &str) -> Result<Vec<(String, String)>, String> {
        match &self.listing {
            Some(mem) => Ok(mem.list_etags(prefix)),
            None => self
                .lister
                .list_etags(prefix)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    /// The presigner for the `ARENA_DATA_S3_*` bucket. With none of the four variables set,
    /// `--dev` uses the dev MinIO and anything else has no presigner (`None`: the route
    /// answers 503). A half-set or wrong configuration is an error naming the variable.
    pub fn from_env(
        dev: bool,
        env: &impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<Self>, String> {
        let any_set = ["ENDPOINT", "BUCKET", "ACCESS_KEY_ID", "SECRET_ACCESS_KEY"]
            .iter()
            .any(|s| get_env(env, &format!("{DATA_S3_PREFIX}_{s}")).is_some_and(|v| !v.is_empty()));
        if any_set {
            let cfg = S3Config::from_env_prefixed(env, DATA_S3_PREFIX)?;
            return Ok(Some(Self::new(&cfg)));
        }
        Ok(dev.then(|| {
            Self::new(&S3Config {
                endpoint: DEV_ENDPOINT.to_owned(),
                bucket: DEV_BUCKET.to_owned(),
                access_key_id: DEV_ACCESS_KEY_ID.to_owned(),
                secret_access_key: DEV_SECRET_ACCESS_KEY.to_owned(),
            })
        }))
    }

    /// A PUT URL for `key`, valid for [`PRESIGN_TTL_S`] seconds from now.
    pub async fn presign_put(&self, key: &str) -> Result<String, String> {
        let cfg = PresigningConfig::expires_in(Duration::from_secs(PRESIGN_TTL_S))
            .map_err(|e| format!("presign config: {e}"))?;
        let req = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| {
                let mut msg = format!("presign {key}: {}", DisplayErrorContext(e));
                for secret in &self.redact {
                    if !secret.is_empty() {
                        msg = msg.replace(secret.as_str(), "[redacted]");
                    }
                }
                msg
            })?;
        Ok(req.uri().to_owned())
    }
}

/// What a link was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutKind {
    Batch,
    Summary,
    Rollup,
    Snapshot,
    Keys,
}

impl PutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Summary => "summary",
            Self::Rollup => "rollup",
            Self::Snapshot => "snapshot",
            Self::Keys => "keys",
        }
    }
}

/// R2 Class A operations (a PUT) in paise per million: 4.50 US dollars at about Rs 83 to the
/// dollar (Story 8.42). The price list and the rate move, so this is the estimate's one input.
pub const R2_CLASS_A_PAISE_PER_MILLION: u64 = 37_500;

/// The PUTs one branch was given links for, and the requests refused for its rate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PutCounts {
    pub batch: u64,
    pub summary: u64,
    pub rollup: u64,
    pub snapshot: u64,
    pub keys: u64,
    /// Calls refused `RATE_LIMITED`.
    pub refused: u64,
}

impl PutCounts {
    /// Every link issued.
    pub fn issued(&self) -> u64 {
        self.batch + self.summary + self.rollup + self.snapshot + self.keys
    }

    /// What the links issued cost in R2 Class A operations at `paise_per_million`, in paise (rounded
    /// up). Links issued stand for the PUTs, so this is the cost estimate the metric publishes.
    pub fn cost_paise(&self, paise_per_million: u64) -> u64 {
        let paise = u128::from(self.issued()) * u128::from(paise_per_million);
        u64::try_from(paise.div_ceil(1_000_000)).unwrap_or(u64::MAX)
    }
}

/// Per tenant and branch counts of R2 PUTs (links issued), kept in process and logged. The
/// OpenTelemetry/Grafana export is deferred with the Story 2.3 exporter.
#[derive(Debug, Default)]
pub struct PutMetrics {
    counts: Mutex<HashMap<(String, String), PutCounts>>,
}

impl PutMetrics {
    fn with<R>(&self, tenant_id: &str, branch_id: &str, f: impl FnOnce(&mut PutCounts) -> R) -> R {
        let mut map = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        f(map
            .entry((tenant_id.to_owned(), branch_id.to_owned()))
            .or_default())
    }

    /// Counts one link of `kind` and logs the branch's totals.
    pub fn issued(&self, tenant_id: &str, branch_id: &str, kind: PutKind) {
        let c = self.with(tenant_id, branch_id, |c| {
            match kind {
                PutKind::Batch => c.batch += 1,
                PutKind::Summary => c.summary += 1,
                PutKind::Rollup => c.rollup += 1,
                PutKind::Snapshot => c.snapshot += 1,
                PutKind::Keys => c.keys += 1,
            }
            *c
        });
        tracing::info!(
            tenant_id,
            branch_id,
            kind = kind.as_str(),
            puts_issued = c.issued(),
            cost_estimate_paise = c.cost_paise(R2_CLASS_A_PAISE_PER_MILLION),
            "presigned PUT issued"
        );
    }

    /// Counts one call refused for its rate and logs the branch's total.
    pub fn refused(&self, tenant_id: &str, branch_id: &str) {
        let c = self.with(tenant_id, branch_id, |c| {
            c.refused += 1;
            *c
        });
        tracing::warn!(
            tenant_id,
            branch_id,
            presign_refused = c.refused,
            "presign refused: rate limited"
        );
    }

    /// The branch's counts so far.
    pub fn counts(&self, tenant_id: &str, branch_id: &str) -> PutCounts {
        self.with(tenant_id, branch_id, |c| *c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch(s: &str) -> Epoch {
        Epoch::parse(s).unwrap()
    }

    #[test]
    fn an_epoch_is_a_counter_and_an_instance() {
        assert_eq!(epoch("7:HUB_a-1").as_str(), "7:HUB_a-1");
        for bad in [
            "",
            "7",
            ":A",
            "7:",
            "x:A",
            "-1:A",
            "+1:A",
            "7:A B",
            "7:A:B",
            "7:é",
            "99999999999999999999999:A",
        ] {
            assert_eq!(Epoch::parse(bad), Err(EpochError::Malformed), "{bad:?}");
        }
        for bad in ["../7:A", "7:A/B", "7:A\\B", "7:..", "1:a..b", "/"] {
            assert_eq!(
                Epoch::parse(bad),
                Err(EpochError::PathCharacters),
                "{bad:?}"
            );
        }
        assert!(Epoch::parse(&format!("1:{}", "a".repeat(65))).is_err());
        assert!(Epoch::parse(&format!("1:{}", "a".repeat(64))).is_ok());
    }

    #[test]
    fn epochs_order_by_counter_numerically_then_instance() {
        assert!(epoch("9:A").below(&epoch("10:A")), "numeric, not text");
        assert!(epoch("1:A").below(&epoch("1:B")));
        assert!(!epoch("1:B").below(&epoch("1:A")));
        assert!(!epoch("2:A").below(&epoch("2:A")), "equal is not below");
        assert!(!epoch("10:A").below(&epoch("9:Z")));
        assert!(!epoch("007:A").below(&epoch("7:A")));
    }

    #[test]
    fn a_missing_fence_fences_nobody_and_an_odd_one_only_lets_the_same_text_pass() {
        assert!(!is_fenced(&epoch("1:A"), None));
        assert!(is_fenced(&epoch("1:A"), Some("2:A")));
        assert!(!is_fenced(&epoch("2:A"), Some("2:A")));
        assert!(!is_fenced(&epoch("3:A"), Some("2:A")));
        assert!(is_fenced(&epoch("3:A"), Some("legacy")));
    }

    #[test]
    fn keys_live_under_the_branch_prefix_with_padded_sequence_numbers() {
        let p = branch_prefix("T1", "B1");
        assert_eq!(p, "tenants/T1/branches/B1/");
        assert_eq!(
            batch_key(&p, &epoch("3:HUB"), 1, 250),
            "tenants/T1/branches/B1/changes/3:HUB/000000000001-000000000250.zst"
        );
        assert_eq!(summary_key(&p), "tenants/T1/branches/B1/state/latest.json");
        assert_eq!(
            rollup_key(&p, "2026-10-29"),
            "tenants/T1/branches/B1/rollups/2026-10-29.json"
        );
        assert_eq!(
            snapshot_key(&p, "2026-10-29"),
            "tenants/T1/branches/B1/snapshots/2026-10-29.db.zst"
        );
    }

    fn cfg() -> S3Config {
        S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            bucket: "data".into(),
            access_key_id: "AKID-SECRET".into(),
            secret_access_key: "SAK-SECRET".into(),
        }
    }

    #[tokio::test]
    async fn a_link_is_path_style_valid_600_seconds_and_carries_no_secret() {
        let url = Presigner::new(&cfg())
            .presign_put("tenants/T1/branches/B1/state/latest.json")
            .await
            .unwrap();
        assert!(
            url.starts_with("http://127.0.0.1:9000/data/tenants/T1/branches/B1/state/latest.json?"),
            "{url}"
        );
        assert!(url.contains("X-Amz-Expires=600"), "{url}");
        assert!(url.contains("X-Amz-Signature="), "{url}");
        assert!(!url.contains("SAK-SECRET"), "{url}");
        assert!(!format!("{:?}", Presigner::new(&cfg())).contains("SECRET"));
    }

    #[test]
    fn the_data_bucket_comes_from_its_own_variables() {
        let none = |_: &str| None;
        assert!(Presigner::from_env(false, &none).unwrap().is_none());
        assert!(Presigner::from_env(true, &none).unwrap().is_some());
        let half = |k: &str| (k == "ARENA_DATA_S3_BUCKET").then(|| OsString::from("b"));
        let e = Presigner::from_env(true, &half).unwrap_err();
        assert!(e.contains("ARENA_DATA_S3_ENDPOINT"), "{e}");
        // The backup bucket's variables are not the data bucket's.
        let backup = |k: &str| {
            k.starts_with("ARENA_BACKUP_S3_")
                .then(|| OsString::from("x"))
        };
        assert!(Presigner::from_env(false, &backup).unwrap().is_none());
        let full = |k: &str| match k {
            "ARENA_DATA_S3_ENDPOINT" => {
                Some(OsString::from("https://acct.r2.cloudflarestorage.com"))
            }
            "ARENA_DATA_S3_BUCKET" => Some(OsString::from("b")),
            "ARENA_DATA_S3_ACCESS_KEY_ID" => Some(OsString::from("id")),
            "ARENA_DATA_S3_SECRET_ACCESS_KEY" => Some(OsString::from("sk")),
            _ => None,
        };
        assert!(Presigner::from_env(false, &full).unwrap().is_some());
        let plain = |k: &str| match k {
            "ARENA_DATA_S3_ENDPOINT" => Some(OsString::from("http://example.com")),
            _ => full(k),
        };
        let e = Presigner::from_env(false, &plain).unwrap_err();
        assert!(
            e.contains("ARENA_DATA_S3_ENDPOINT must be an https:// URL"),
            "{e}"
        );
    }

    #[test]
    fn puts_are_counted_per_tenant_and_branch() {
        let m = PutMetrics::default();
        m.issued("T1", "B1", PutKind::Batch);
        m.issued("T1", "B1", PutKind::Summary);
        m.issued("T1", "B1", PutKind::Batch);
        m.issued("T1", "B2", PutKind::Rollup);
        m.refused("T1", "B1");
        assert_eq!(
            m.counts("T1", "B1"),
            PutCounts {
                batch: 2,
                summary: 1,
                refused: 1,
                ..PutCounts::default()
            }
        );
        assert_eq!(m.counts("T1", "B1").issued(), 3);
        assert_eq!(m.counts("T1", "B2").rollup, 1);
        assert_eq!(m.counts("T2", "B1"), PutCounts::default());
    }

    #[test]
    fn the_cost_estimate_is_the_links_issued_at_the_price_per_million_rounded_up() {
        let c = PutCounts {
            batch: 2_000,
            summary: 1_000,
            ..PutCounts::default()
        };
        assert_eq!(c.cost_paise(R2_CLASS_A_PAISE_PER_MILLION), 113);
        assert_eq!(c.cost_paise(0), 0);
        assert_eq!(PutCounts::default().cost_paise(1_000_000), 0);
    }
}

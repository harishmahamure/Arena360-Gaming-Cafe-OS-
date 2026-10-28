//! Where a paid branch's setup template lives (Story 5.37): two objects under the branch's
//! prefix of the object store (R2), `setup-template.json` (the Hub's setup file, byte for byte)
//! and `setup-template.meta.json` (its format version, SHA-256, size and upload time). The
//! metadata is written last, so a half-written upload is never described.

use std::ffi::OsString;
use std::sync::Arc;

use arena_proto::SetupTemplateStored;
use sha2::{Digest, Sha256};

use crate::auth::hex;
use crate::store::{MemoryStore, ObjectStore, S3Config, S3Store, StoreError};

/// The prefix every branch's template objects live under, `branches/<branch id>/`.
pub fn branch_prefix(branch_id: &str) -> String {
    format!("branches/{branch_id}/")
}

/// The key of the Hub's setup file.
pub fn file_key(branch_id: &str) -> String {
    format!("{}setup-template.json", branch_prefix(branch_id))
}

/// The key of the file's description.
pub fn meta_key(branch_id: &str) -> String {
    format!("{}setup-template.meta.json", branch_prefix(branch_id))
}

/// The object store the templates are kept in.
#[derive(Debug, Clone)]
pub enum TemplateStore {
    /// `--dev` and tests.
    Memory(Arc<MemoryStore>),
    /// R2 (MinIO in dev and CI): the bucket of the backup job.
    S3(S3Store),
}

impl TemplateStore {
    pub fn memory() -> Self {
        Self::Memory(Arc::new(MemoryStore::new()))
    }

    /// The `ARENA_BACKUP_S3_*` bucket, or `None` when those variables are not all set (a
    /// cloud without them answers 503 to a template upload). A wrong value is an error.
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Option<Self>, String> {
        if env(crate::store::S3_BUCKET_ENV).is_none() {
            return Ok(None);
        }
        let cfg = S3Config::from_env(env)?;
        Ok(Some(Self::S3(S3Store::new(&cfg))))
    }

    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError> {
        match self {
            Self::Memory(s) => s.put(key, body).await,
            Self::S3(s) => s.put(key, body).await,
        }
    }

    pub async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        match self {
            Self::Memory(s) => s.get(key).await,
            Self::S3(s) => s.get(key).await,
        }
    }

    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        match self {
            Self::Memory(s) => s.list(prefix).await,
            Self::S3(s) => s.list(prefix).await,
        }
    }

    /// Keeps `body` (a setup file of format `version`) as the branch's template, replacing the
    /// one before it.
    pub async fn store(
        &self,
        branch_id: &str,
        body: Vec<u8>,
        version: u32,
        now: i64,
    ) -> Result<SetupTemplateStored, StoreError> {
        let stored = SetupTemplateStored {
            version,
            sha256: hex(&Sha256::digest(&body)),
            bytes: body.len() as u64,
            uploaded_at: now,
        };
        let meta = serde_json::to_vec(&stored).map_err(|e| StoreError(e.to_string()))?;
        self.put(&file_key(branch_id), body).await?;
        self.put(&meta_key(branch_id), meta).await?;
        Ok(stored)
    }

    /// What the branch's template is, or `None` when it has none.
    pub async fn describe(
        &self,
        branch_id: &str,
    ) -> Result<Option<SetupTemplateStored>, StoreError> {
        let key = meta_key(branch_id);
        if !self.list(&key).await?.contains(&key) {
            return Ok(None);
        }
        let raw = self.get(&key).await?;
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|e| StoreError(format!("{key}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_template_and_its_description_are_stored_under_the_branch_prefix() {
        let store = TemplateStore::memory();
        assert_eq!(store.describe("B1").await.unwrap(), None);
        let stored = store.store("B1", b"{}".to_vec(), 1, 42).await.unwrap();
        assert_eq!(stored.bytes, 2);
        assert_eq!(
            stored.sha256,
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
        assert_eq!(store.describe("B1").await.unwrap(), Some(stored));
        assert_eq!(store.get(&file_key("B1")).await.unwrap(), b"{}");
        assert_eq!(store.describe("B2").await.unwrap(), None);
        let TemplateStore::Memory(m) = &store else {
            unreachable!()
        };
        assert_eq!(
            m.keys(),
            [
                "branches/B1/setup-template.json",
                "branches/B1/setup-template.meta.json"
            ]
        );
    }

    #[test]
    fn without_a_bucket_there_is_no_store_and_a_half_set_one_is_an_error() {
        let none = |_: &str| None;
        assert!(TemplateStore::from_env(&none).unwrap().is_none());
        let half = |k: &str| (k == crate::store::S3_BUCKET_ENV).then(|| OsString::from("b"));
        assert!(TemplateStore::from_env(&half).is_err());
    }
}

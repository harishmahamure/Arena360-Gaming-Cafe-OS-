//! An S3 endpoint for tests: MinIO.
//!
//! - `ARENA_TEST_S3_URL` set (CI's service container, or `deploy/dev/compose.yaml` locally):
//!   that endpoint, with `ARENA_TEST_S3_ACCESS_KEY_ID` / `ARENA_TEST_S3_SECRET_ACCESS_KEY`
//!   (default: the dev compose credentials). Each test makes its own bucket.
//! - Otherwise the test is skipped with a printed reason, unless `ARENA_REQUIRE_S3=1`, which
//!   makes the absence fail.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};

use arena_cloud::store::{S3Config, S3Store};

/// A test endpoint and a fresh, empty bucket on it.
pub struct TestBucket {
    pub cfg: S3Config,
    pub store: S3Store,
}

fn require() -> bool {
    std::env::var("ARENA_REQUIRE_S3").is_ok_and(|v| v == "1")
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A new bucket on the test endpoint, or `None` (skipped).
pub async fn bucket() -> Option<TestBucket> {
    let endpoint = std::env::var("ARENA_TEST_S3_URL").unwrap_or_default();
    if endpoint.is_empty() {
        assert!(
            !require(),
            "ARENA_REQUIRE_S3=1 but ARENA_TEST_S3_URL is not set"
        );
        eprintln!(
            "skipping: ARENA_TEST_S3_URL is not set (docker compose -f deploy/dev/compose.yaml up -d, then ARENA_TEST_S3_URL=http://127.0.0.1:9000)"
        );
        return None;
    }
    let var = |k: &str, d: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| d.to_owned())
    };
    let name = format!(
        "arena-test-{}-{}-{}",
        std::process::id(),
        arena_cloud::now_ms(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let cfg = S3Config {
        endpoint,
        bucket: name,
        access_key_id: var("ARENA_TEST_S3_ACCESS_KEY_ID", "arena-dev"),
        secret_access_key: var("ARENA_TEST_S3_SECRET_ACCESS_KEY", "arena-dev-password"),
    };
    let store = S3Store::new(&cfg);
    store.create_bucket().await.expect("create the test bucket");
    Some(TestBucket { cfg, store })
}

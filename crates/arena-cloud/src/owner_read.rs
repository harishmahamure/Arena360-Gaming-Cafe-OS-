//! What the owner read API keeps in memory (Story 8.9, AD-35): a branch's `state/latest.json`
//! and its months of `rollups/<date>.json`, each served for [`OWNER_READ_CACHE_MS`] without
//! asking the bucket and then re-checked against the bucket's ETag, so an unchanged file is never
//! downloaded twice.
//!
//! The cloud holds no row of any branch: these are the Hub's own files, held per tenant and
//! branch in process and dropped on restart. Keys are built from the tenant and branch ids the
//! caller passes (the session's own), never from request input.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use arena_proto::{
    BranchSummary, OWNER_READ_CACHE_MS, RollupV1, is_rollup_date, rollup_object_key,
};
use serde::Deserialize;

use crate::presign::{Presigner, branch_prefix, summary_key};
use crate::rollups::RollupMetrics;

/// The most entries either map holds before it is emptied (a restart-sized reset, not an LRU).
const MAX_ENTRIES: usize = 4096;

#[derive(Clone)]
struct LiveEntry {
    checked_at: i64,
    etag: Option<String>,
    summary: Option<Arc<BranchSummary>>,
}

/// A day's file as the cloud read it.
#[derive(Clone)]
enum DayFile {
    Readable(Arc<RollupV1>),
    /// At a schema the cloud does not read, or not a rollup at all: the day counts as missing.
    Unreadable,
}

#[derive(Clone)]
struct DayEntry {
    etag: String,
    file: DayFile,
}

#[derive(Clone)]
struct MonthEntry {
    checked_at: i64,
    days: BTreeMap<String, DayEntry>,
}

/// The in-memory copies of the owner read API.
#[derive(Default)]
pub struct ReadCache {
    live: Mutex<HashMap<(String, String), LiveEntry>>,
    months: Mutex<HashMap<(String, String, String), MonthEntry>>,
}

impl std::fmt::Debug for ReadCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadCache").finish_non_exhaustive()
    }
}

fn fresh(checked_at: i64, now: i64) -> bool {
    (0..OWNER_READ_CACHE_MS).contains(&(now - checked_at))
}

#[derive(Deserialize)]
struct SchemaOf {
    schema: u32,
}

impl ReadCache {
    /// The branch's `state/latest.json`, or none while it has none. Within the cache window the
    /// answer comes from memory; after it the bucket's ETag decides whether to download again.
    pub async fn live(
        &self,
        presigner: &Presigner,
        tenant_id: &str,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<Arc<BranchSummary>>, String> {
        let id = (tenant_id.to_owned(), branch_id.to_owned());
        let cached = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned();
        if let Some(c) = cached.as_ref().filter(|c| fresh(c.checked_at, now)) {
            return Ok(c.summary.clone());
        }
        let key = summary_key(&branch_prefix(tenant_id, branch_id));
        let etag = presigner.head(&key).await?;
        let entry = match (cached, etag) {
            (Some(c), etag) if c.etag == etag && etag.as_ref().is_some_and(|e| !e.is_empty()) => {
                LiveEntry {
                    checked_at: now,
                    ..c
                }
            }
            (_, None) => LiveEntry {
                checked_at: now,
                etag: None,
                summary: None,
            },
            (_, Some(_)) => match presigner.read(&key).await? {
                None => LiveEntry {
                    checked_at: now,
                    etag: None,
                    summary: None,
                },
                Some((bytes, etag)) => LiveEntry {
                    checked_at: now,
                    etag: Some(etag),
                    summary: match serde_json::from_slice::<BranchSummary>(&bytes) {
                        Ok(s) => Some(Arc::new(s)),
                        Err(e) => {
                            tracing::warn!(tenant_id, branch_id, error = %e, "owner read: summary file unreadable");
                            None
                        }
                    },
                },
            },
        };
        let mut map = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        if map.len() >= MAX_ENTRIES {
            map.clear();
        }
        let summary = entry.summary.clone();
        map.insert(id, entry);
        Ok(summary)
    }

    /// The accepted rollups of `dates` (`yyyy-mm-dd`, as built by the caller), by date. A day
    /// that is absent, or at a schema the cloud does not read (`ROLLUP_SCHEMA_OLD`, recorded in
    /// `metrics`), is not in the answer.
    pub async fn rollups(
        &self,
        presigner: &Presigner,
        metrics: &RollupMetrics,
        tenant_id: &str,
        branch_id: &str,
        dates: &[String],
        now: i64,
    ) -> Result<BTreeMap<String, Arc<RollupV1>>, String> {
        let mut months: Vec<&str> = dates.iter().filter_map(|d| d.get(..7)).collect();
        months.dedup();
        let mut out = BTreeMap::new();
        for month in months {
            let entry = self
                .month(presigner, metrics, tenant_id, branch_id, month, now)
                .await?;
            for d in dates.iter().filter(|d| d.starts_with(month)) {
                if let Some(DayEntry {
                    file: DayFile::Readable(r),
                    ..
                }) = entry.days.get(d)
                {
                    out.insert(d.clone(), Arc::clone(r));
                }
            }
        }
        Ok(out)
    }

    async fn month(
        &self,
        presigner: &Presigner,
        metrics: &RollupMetrics,
        tenant_id: &str,
        branch_id: &str,
        month: &str,
        now: i64,
    ) -> Result<MonthEntry, String> {
        let id = (tenant_id.to_owned(), branch_id.to_owned(), month.to_owned());
        let cached = self
            .months
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned();
        if let Some(c) = cached.as_ref().filter(|c| fresh(c.checked_at, now)) {
            return Ok(c.clone());
        }
        let base = branch_prefix(tenant_id, branch_id);
        let listed = presigner
            .list_etags(&format!("{base}rollups/{month}-"))
            .await?;
        let current = crate::rollups::current_rollup_schema();
        let mut days = BTreeMap::new();
        let mut stale: Vec<(String, String)> = Vec::new();
        for (key, etag) in listed {
            let Some(rel) = key.strip_prefix(&base) else {
                continue;
            };
            let Some(date) = rel
                .strip_prefix("rollups/")
                .and_then(|r| r.strip_suffix(".json"))
            else {
                continue;
            };
            if !is_rollup_date(date) || !date.starts_with(month) || rollup_object_key(date) != rel {
                continue;
            }
            match cached.as_ref().and_then(|c| c.days.get(date)) {
                Some(old) if old.etag == etag && !etag.is_empty() => {
                    days.insert(date.to_owned(), old.clone());
                }
                _ => stale.push((date.to_owned(), key)),
            }
        }
        let reads =
            futures_util::future::join_all(stale.iter().map(|(_, key)| presigner.read(key))).await;
        for ((date, _), read) in stale.into_iter().zip(reads) {
            let Some((bytes, etag)) = read? else {
                continue;
            };
            let file = match serde_json::from_slice::<SchemaOf>(&bytes) {
                Ok(SchemaOf { schema }) => {
                    if metrics
                        .record_file(tenant_id, branch_id, &date, schema, current)
                        .is_err()
                    {
                        DayFile::Unreadable
                    } else {
                        match serde_json::from_slice::<RollupV1>(&bytes) {
                            Ok(r) => DayFile::Readable(Arc::new(r)),
                            Err(e) => {
                                tracing::warn!(tenant_id, branch_id, date, error = %e, "owner read: rollup file unreadable");
                                DayFile::Unreadable
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(tenant_id, branch_id, date, error = %e, "owner read: rollup file is not a rollup");
                    DayFile::Unreadable
                }
            };
            days.insert(date, DayEntry { etag, file });
        }
        let entry = MonthEntry {
            checked_at: now,
            days,
        };
        let mut map = self.months.lock().unwrap_or_else(PoisonError::into_inner);
        if map.len() >= MAX_ENTRIES {
            map.clear();
        }
        map.insert(id, entry.clone());
        Ok(entry)
    }
}

//! Matching a stored scan to the catalogue (Story 6.5). After a kiosk's `kiosk.scan_reported`
//! is applied, the Hub matches its executables against the cached catalogue signatures and
//! records the result with `game.matched` as the LAN system actor. The catalogue cache
//! (`reference.db`) can be read here and not inside `apply()`, so this layer does the matching.
//!
//! It runs in the background: the kiosk's reply never waits for it, and a failure only logs (the
//! next scan matches again). Without a catalogue cache the matches are empty, which still
//! brings unmatched library entries' install paths up to date.

use std::sync::Arc;

use arena_core::{Clock, WriterHandle, game_proposals::match_scan, media_cache::MediaCache};
use arena_proto::{
    Actor, ActorKind, ErrorCode, GameMatched, Intent, IntentKind, IntentPayload, ScanEntry,
};

use super::LanHub;
use super::session::LAN_SYSTEM_ACTOR;

/// What a stored scan needs to be matched.
pub(crate) struct StoredScan {
    pub(crate) device_id: String,
    pub(crate) scanned_at: i64,
    pub(crate) entries: Vec<ScanEntry>,
}

impl StoredScan {
    /// The scan a `kiosk.scan_reported` intent carries (the session's actor is the device).
    pub(crate) fn of(intent: &Intent) -> Option<Self> {
        match &intent.payload {
            IntentPayload::KioskScanReported(r) => Some(Self {
                device_id: intent.actor.id.clone(),
                scanned_at: r.scanned_at,
                entries: r.entries.clone(),
            }),
            _ => None,
        }
    }
}

/// Matches `scan` and records it, in the background.
pub(crate) fn spawn(hub: &LanHub, scan: StoredScan) {
    if !hub.is_primary() {
        return;
    }
    let Some(writer) = hub.writer.clone() else {
        return;
    };
    let cache = hub.media.clone();
    let clock = Arc::clone(&hub.clock);
    tokio::spawn(async move { run(writer, cache, clock, scan).await });
}

async fn run(
    writer: WriterHandle,
    cache: Option<Arc<MediaCache>>,
    clock: Arc<dyn Clock>,
    scan: StoredScan,
) {
    let signatures = match cache {
        Some(cache) => match tokio::task::spawn_blocking(move || cache.signatures()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "scan match: could not read the catalogue"
                );
                return;
            }
            Err(_) => return,
        },
        None => Vec::new(),
    };
    let entries = scan.entries;
    let matches = match tokio::task::spawn_blocking(move || match_scan(&entries, &signatures)).await
    {
        Ok(m) => m,
        Err(_) => return,
    };
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::GameMatched,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: clock.now_ms(),
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: LAN_SYSTEM_ACTOR.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::GameMatched(GameMatched {
            device_id: scan.device_id.clone(),
            scanned_at: scan.scanned_at,
            matches,
        }),
    };
    match writer.submit_intent(intent).await {
        Ok(_) => {}
        // A newer scan already replaced this one, or the same matches are recorded.
        Err(e) if e.code == ErrorCode::ValidationFailed => {
            tracing::debug!(device_id = %scan.device_id, "scan match: nothing to record");
        }
        Err(e) => {
            tracing::warn!(device_id = %scan.device_id, code = e.code.as_str(), "scan match: could not record the matches");
        }
    }
}

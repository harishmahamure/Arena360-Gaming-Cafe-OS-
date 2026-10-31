//! `arena-core`: the Hub domain. Time comes from an injected [`Clock`]; every write goes
//! through the single [`Writer`] actor, and every branch write goes through [`apply`].

mod ad_metrics;
mod apply;
pub mod attract;
pub mod audit;
pub mod auto_export;
pub mod billing;
mod catalog;
mod clock;
mod commands;
pub mod credit_reminder;
pub mod devices;
pub mod entitlement;
mod export_everything;
pub mod game_plays;
pub mod game_proposals;
pub mod gst;
pub mod gst_archive;
mod import_csv;
mod import_file;
pub mod import_plans;
pub mod import_players;
pub mod import_products;
pub mod import_starter;
pub mod invoice;
pub mod key_backup;
pub mod launch_times;
pub mod live_floor;
pub mod locale;
pub mod media_cache;
pub mod notify_drain;
pub mod notify_trigger;
pub mod overdue;
pub mod pairing;
mod pdf;
pub mod player_account;
pub mod player_extend;
pub mod player_plans;
pub mod players;
mod pricing;
pub mod printer;
pub mod rbac;
pub mod report_export;
mod reports;
pub mod restore_fetch;
pub mod rollup;
pub mod rollup_plan;
mod session_screen;
mod session_timer;
pub mod set_aside;
mod settings_defaults;
pub mod setup;
pub mod setup_file;
pub mod shop;
mod snapshot;
pub mod staff;
mod status;
mod stock_report;
pub mod summary;
pub mod windows;
mod writer;

use arena_proto::AppError;
use arena_store::{META_HUB_INSTANCE_ID, Store, insert_meta_if_absent, set_meta};

pub use ad_metrics::AdPending;
pub use apply::{
    Applied, ApplyCtx, CLEANUP_FAILED_CODE, ChangeMeta, INTENT_TTL_MS, MAX_CLOCK_SKEW_MS,
    MAX_KIOSK_LOG_CODE_LEN, MAX_KIOSK_LOG_DETAIL_CHARS, MAX_SETTING_VALUE_CHARS, MAX_VERSION_LEN,
    PROCESS_BLOCKED_CODE, Written, apply, coupon_preview, decrement, discount_preview, emit,
    fingerprint, is_ulid, lock_branch_code, validate_frame,
};
pub use arena_store::{ChangeOp, META_TLS_FINGERPRINT};
pub use audit::{AuditQuery, AuditReader};
pub use clock::{Clock, ManualClock, SystemClock};
pub use commands::{
    BACKUP_NOW, CREDIT_REMIND, HANDLED_COMMAND_KINDS, backup_now_check, command_intent,
};
pub use devices::{DeviceDirectory, LoginCandidate, VerifiedDevice};
pub use game_plays::MostPlayedCache;
pub use pairing::{HubPairer, PairFuture, PairGate};
pub use pricing::{Resolved, conflicts as rule_conflicts, resolve as resolve_rate};
pub use restore_fetch::{RestoreFetchControl, RestoreRebuildControl};
pub use session_screen::{session_screens, session_sync};
pub use session_timer::{TimedSession, TimerAction, TimerEngine, TimerEvent};
pub use settings_defaults::{
    DEFAULT_SETTINGS, DEFERRED_KEYS, KEY_CURRENCY, KEY_DAY_START_MIN, KEY_FY_START_MONTH,
    KEY_NUMBER_FORMAT, KEY_SYMBOL_POSITION, KEY_TZ, LOCALE_KEYS, SETTING_RULES, SettingRule,
    bootstrap_locale_settings, setting_rule,
};
pub use snapshot::{
    KeyBackupPlan, KotJob, RateFlips, SnapshotReader, UploadStart, branch_setup, build_snapshot,
    read_snapshot,
};
pub use staff::StaffDirectory;
pub use status::{
    CloudTime, Epoch, HubStatus, LeaseInfo, PairingState, Role, UploadStatus, status_channel,
};
pub use writer::{LeaseRenewal, LicenceState, Writer, WriterHandle};

/// Returns this Hub's instance id, minting and persisting a ULID on first start.
///
/// A store without the writer lock (a follower) never writes: if no id is stored yet (the
/// writer has not finished its first start), it gets a fresh id that is not persisted.
pub fn load_or_mint_instance_id(store: &mut Store) -> Result<String, AppError> {
    if let Some(id) = store.meta(META_HUB_INSTANCE_ID)? {
        return Ok(id);
    }
    let fresh = ulid::Ulid::generate().to_string();
    if !store.holds_writer_lock() {
        return Ok(fresh);
    }
    store.write_tx(|tx| insert_meta_if_absent(tx, META_HUB_INSTANCE_ID, &fresh))
}

/// Records the LAN TLS certificate's fingerprint in `hub_meta.tls_fingerprint` (a meta write,
/// like the instance id) when it differs from the stored one, with a `warn` line when another
/// fingerprint was stored before (pinned clients will refuse the new certificate). A follower
/// writes nothing.
pub fn record_tls_fingerprint(store: &mut Store, fingerprint: &str) -> Result<(), AppError> {
    let stored = store.meta(META_TLS_FINGERPRINT)?;
    if stored.as_deref() == Some(fingerprint) {
        return Ok(());
    }
    if let Some(old) = &stored {
        // Paired clients pinned the old fingerprint and will refuse this certificate.
        tracing::warn!(
            stored = %old,
            current = %fingerprint,
            "the lan certificate fingerprint differs from hub_meta.tls_fingerprint"
        );
    }
    if !store.holds_writer_lock() {
        return Ok(());
    }
    store.write_tx(|tx| set_meta(tx, META_TLS_FINGERPRINT, fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_fingerprint_is_recorded_by_the_writer_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut primary = Store::open(&path, 0).unwrap();
        record_tls_fingerprint(&mut primary, "aa").unwrap();
        record_tls_fingerprint(&mut primary, "bb").unwrap();
        let mut follower = Store::open(&path, 0).unwrap();
        record_tls_fingerprint(&mut follower, "cc").unwrap();
        assert_eq!(
            primary.meta(META_TLS_FINGERPRINT).unwrap().as_deref(),
            Some("bb")
        );
    }

    #[test]
    fn instance_id_is_minted_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let first = {
            let mut store = Store::open(&path, 0).unwrap();
            load_or_mint_instance_id(&mut store).unwrap()
        };
        assert_eq!(first.len(), 26);
        assert!(first.parse::<ulid::Ulid>().is_ok());
        let mut store = Store::open(&path, 0).unwrap();
        assert_eq!(load_or_mint_instance_id(&mut store).unwrap(), first);
    }

    #[test]
    fn follower_does_not_persist_an_instance_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let primary = Store::open(&path, 0).unwrap();
        let mut follower = Store::open(&path, 0).unwrap();
        assert!(!follower.holds_writer_lock());
        let id = load_or_mint_instance_id(&mut follower).unwrap();
        assert_eq!(id.len(), 26);
        assert_eq!(primary.meta(META_HUB_INSTANCE_ID).unwrap(), None);
    }
}

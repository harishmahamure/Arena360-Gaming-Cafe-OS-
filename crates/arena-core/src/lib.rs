//! `arena-core`: the Hub domain. Time comes from an injected [`Clock`]; every write goes
//! through the single [`Writer`] actor, and every branch write goes through [`apply`].

mod apply;
mod clock;
pub mod devices;
pub mod rbac;
mod settings_defaults;
mod snapshot;
pub mod staff;
mod status;
mod writer;

use arena_proto::AppError;
use arena_store::{META_HUB_INSTANCE_ID, Store, insert_meta_if_absent, set_meta};

pub use apply::{
    Applied, ApplyCtx, ChangeMeta, INTENT_TTL_MS, KNOWN_CURRENCIES, MAX_CLOCK_SKEW_MS,
    MAX_SETTING_VALUE_CHARS, MAX_VERSION_LEN, apply, emit, fingerprint, is_ulid, validate_frame,
};
pub use arena_store::{ChangeOp, META_TLS_FINGERPRINT};
pub use clock::{Clock, ManualClock, SystemClock};
pub use devices::{DeviceDirectory, VerifiedDevice};
pub use settings_defaults::{DEFAULT_SETTINGS, SETTING_RULES, SettingRule, setting_rule};
pub use snapshot::{SnapshotReader, build_snapshot, read_snapshot};
pub use staff::StaffDirectory;
pub use status::{Epoch, HubStatus, LeaseInfo, Role, status_channel};
pub use writer::{LeaseRenewal, Writer, WriterHandle};

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

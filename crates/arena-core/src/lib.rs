//! `arena-core`: the Hub domain. Time comes from an injected [`Clock`]; every write goes
//! through the single [`Writer`] actor, and every branch write goes through [`apply`].

mod apply;
mod clock;
mod settings_defaults;
mod status;
mod writer;

use arena_proto::AppError;
use arena_store::{META_HUB_INSTANCE_ID, Store, insert_meta_if_absent};

pub use apply::{
    Applied, ApplyCtx, ChangeMeta, INTENT_TTL_MS, KNOWN_CURRENCIES, MAX_CLOCK_SKEW_MS,
    MAX_SETTING_VALUE_CHARS, apply, emit, fingerprint, is_ulid, validate_frame,
};
pub use clock::{Clock, ManualClock, SystemClock};
pub use settings_defaults::{DEFAULT_SETTINGS, SETTING_RULES, SettingRule, setting_rule};
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

#[cfg(test)]
mod tests {
    use super::*;

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

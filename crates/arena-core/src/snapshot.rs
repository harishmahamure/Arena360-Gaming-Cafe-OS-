//! The Hub UI's branch snapshot ([`HubSnapshot`]): the branch name and the devices that are not
//! revoked. Read on its own read-only connection, never through the writer.

use std::path::Path;
use std::sync::Mutex;

use arena_proto::{
    AppError, ClientClass, ErrorCode, HubSnapshot, SettingEntry, SnapshotBranch, SnapshotDevice,
};
use arena_store::{BranchSummary, DeviceRow, Store};

/// Builds the snapshot from the branch row and every device row. Revoked devices are left
/// out, and so is a row with a class the protocol does not know. Every device is unassigned
/// (`zone: null`) until zones exist.
pub fn build_snapshot(branch: Option<BranchSummary>, devices: Vec<DeviceRow>) -> HubSnapshot {
    HubSnapshot {
        branch: branch.map(|b| SnapshotBranch {
            name: b.name,
            branch_code: b.branch_code,
        }),
        devices: devices
            .into_iter()
            .filter(|d| d.revoked_at.is_none())
            .filter_map(|d| {
                let Some(class) = ClientClass::parse(&d.class) else {
                    tracing::warn!(
                        device_id = %d.device_id,
                        class = %d.class,
                        "snapshot: skipping a device with an unknown class"
                    );
                    return None;
                };
                Some(SnapshotDevice {
                    class,
                    device_id: d.device_id,
                    label: d.label,
                    zone: None,
                })
            })
            .collect(),
    }
}

/// Reads the snapshot from `store`: the branch and the devices in one read transaction.
pub fn read_snapshot(store: &Store) -> Result<HubSnapshot, AppError> {
    let (branch, devices) = store.read_consistent(|s| Ok((s.branch_summary()?, s.devices()?)))?;
    Ok(build_snapshot(branch, devices))
}

/// A read-only connection that serves [`HubSnapshot`]s on a primary or a follower.
pub struct SnapshotReader {
    reader: Mutex<Store>,
}

impl std::fmt::Debug for SnapshotReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotReader").finish_non_exhaustive()
    }
}

impl SnapshotReader {
    /// Opens a read-only connection to the branch database at `db_path`.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        Ok(Self {
            reader: Mutex::new(Store::open_reader(db_path)?),
        })
    }

    /// The current snapshot. Blocking (SQLite).
    pub fn snapshot(&self) -> Result<HubSnapshot, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        read_snapshot(&store)
    }

    /// Every branch setting with its `rev` (empty before bootstrap). Blocking (SQLite).
    pub fn settings(&self) -> Result<Vec<SettingEntry>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store
            .settings()?
            .into_iter()
            .map(|s| SettingEntry {
                key: s.key,
                value: s.value,
                rev: s.rev,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_store::{NewBranch, NewDevice, insert_branch, insert_device, set_device_revoked};

    fn seed(path: &Path, with_branch: bool) {
        let mut store = Store::open(path, 0).unwrap();
        if !with_branch {
            return;
        }
        store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "Dev Branch",
                        branch_code: "DEV01",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                for (id, label) in [("D1", "Dev Kiosk"), ("D2", "Old Kiosk")] {
                    insert_device(
                        tx,
                        &NewDevice {
                            device_id: id,
                            branch_id: "B",
                            class: "paired",
                            label,
                            token_hash: "$argon2id$x",
                        },
                    )?;
                }
                set_device_revoked(tx, "D2", 5)
            })
            .unwrap();
    }

    #[test]
    fn snapshot_has_the_branch_and_only_live_devices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        seed(&path, true);
        let snap = SnapshotReader::open(&path).unwrap().snapshot().unwrap();
        assert_eq!(
            snap,
            HubSnapshot {
                branch: Some(SnapshotBranch {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                }),
                devices: vec![SnapshotDevice {
                    device_id: "D1".into(),
                    label: "Dev Kiosk".into(),
                    class: ClientClass::Paired,
                    zone: None,
                }],
            }
        );
    }

    #[test]
    fn snapshot_before_bootstrap_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        seed(&path, false);
        let snap = SnapshotReader::open(&path).unwrap().snapshot().unwrap();
        assert_eq!(
            snap,
            HubSnapshot {
                branch: None,
                devices: vec![],
            }
        );
    }

    #[test]
    fn unknown_device_classes_are_skipped() {
        let row = DeviceRow {
            device_id: "X".into(),
            branch_id: "B".into(),
            class: "toaster".into(),
            label: "?".into(),
            token_hash: String::new(),
            reported_version: None,
            revoked_at: None,
            rev: 1,
        };
        assert!(build_snapshot(None, vec![row]).devices.is_empty());
    }
}

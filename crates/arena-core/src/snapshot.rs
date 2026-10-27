//! The Hub UI's branch snapshot ([`HubSnapshot`]): the branch name and the devices that are not
//! revoked. Read on its own read-only connection, never through the writer.

use std::path::Path;
use std::sync::Mutex;

use arena_proto::{
    AppError, ClientClass, DeviceStatus, ErrorCode, HubSnapshot, SettingEntry, SetupStatus,
    SnapshotBranch, SnapshotDevice, SnapshotGroup, SnapshotLocale, SnapshotUi, SnapshotZone,
};
use arena_store::{BranchSummary, DeviceMetaRow, DeviceRow, GroupRow, Store, ZoneRow};

use crate::locale::{locale_rows, next_rollover, snapshot_locale};

/// Builds the snapshot from the branch row and every device row. Revoked devices are left
/// out, and so is a row with a class the protocol does not know. Every device is unassigned
/// (`zone: null`) until zones exist.
pub fn build_snapshot(
    branch: Option<BranchSummary>,
    devices: Vec<DeviceRow>,
    meta: Vec<DeviceMetaRow>,
    groups: Vec<GroupRow>,
    zones: Vec<ZoneRow>,
    locale: Option<SnapshotLocale>,
    ui: SnapshotUi,
) -> HubSnapshot {
    let meta: std::collections::HashMap<String, DeviceMetaRow> = meta
        .into_iter()
        .filter(|m| m.deleted_at.is_none())
        .map(|m| (m.device_id.clone(), m))
        .collect();
    HubSnapshot {
        sessions: Vec::new(),
        bills: Vec::new(),
        locale,
        ui,
        // The probe's state is the Hub status's, not the database's: the snapshot handler
        // fills it in.
        connectivity: arena_proto::Connectivity::default(),
        setup: None,
        lan: None,
        paired: true,
        pairing_revoked: None,
        licence_unverified: None,
        clock_wrong: None,
        device_cap: None,
        branch: branch.map(|b| SnapshotBranch {
            name: b.name,
            branch_code: b.branch_code,
            code_locked: b.code_locked.then_some(true),
        }),
        groups: groups
            .into_iter()
            .map(|g| SnapshotGroup {
                id: g.group_id,
                name: g.name,
                kind: g.kind,
                colour: g.colour,
                sort_order: g.sort_order,
                rev: g.rev,
                rate_minor: None,
            })
            .collect(),
        zones: zones
            .into_iter()
            .map(|z| SnapshotZone {
                id: z.zone_id,
                name: z.name,
                sort_order: z.sort_order,
                rev: z.rev,
            })
            .collect(),
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
                let m = meta.get(&d.device_id);
                Some(SnapshotDevice {
                    class,
                    label: m.map_or(d.label, |m| m.label.clone()),
                    kind: m.and_then(|m| arena_proto::DeviceKind::parse(&m.kind)),
                    group_id: m.and_then(|m| m.group_id.clone()),
                    meta_rev: m.map(|m| m.rev),
                    tv_number: m.and_then(|m| m.tv_number),
                    zone: m.and_then(|m| m.zone_id.clone()),
                    floor_x: m.and_then(|m| m.floor_x),
                    floor_y: m.and_then(|m| m.floor_y),
                    manual: d.token_hash.is_empty(),
                    status: m
                        .and_then(|m| DeviceStatus::parse(&m.state))
                        .filter(|s| s.held())
                        .unwrap_or(if d.token_hash.is_empty() {
                            DeviceStatus::Online
                        } else {
                            DeviceStatus::Offline
                        }),
                    last_seen_at: None,
                    maintenance_note: m.and_then(|m| m.maintenance_note.clone()),
                    device_id: d.device_id,
                    // Presence is the LAN registry's, not the database's: the snapshot
                    // handler fills it in.
                    online: false,
                })
            })
            .collect(),
    }
}

fn bills(
    sessions: &[arena_store::LiveSession],
    segments: &[arena_store::SegmentRow],
    (orders, order_lines): (Vec<arena_store::OpenOrder>, Vec<arena_store::OrderLineRow>),
    now_ms: i64,
) -> Vec<arena_proto::SnapshotBill> {
    let bill = |session_id: String,
                device_id: String,
                order_id: Option<String>,
                order_rev: Option<i64>,
                lines: Vec<arena_proto::BillLine>| {
        let subtotal_minor = lines.iter().map(|l| l.amount_minor).sum();
        arena_proto::SnapshotBill {
            session_id,
            device_id,
            order_id,
            order_rev,
            lines,
            subtotal_minor,
            paid_minor: 0,
            remaining_minor: subtotal_minor,
        }
    };
    let live = sessions.iter().map(|s| {
        let own: Vec<_> = segments
            .iter()
            .filter(|g| g.session_id == s.id)
            .cloned()
            .collect();
        let lines = crate::billing::bill(
            &own,
            s.rounding_min,
            s.minimum_min,
            s.paused_total_ms,
            now_ms,
        );
        bill(s.id.clone(), s.device_id.clone(), None, None, lines)
    });
    let open = orders.into_iter().map(|o| {
        let lines = order_lines
            .iter()
            .filter(|l| l.order_id == o.id)
            .map(|l| arena_proto::BillLine {
                device_id: l.device_id.clone(),
                from_at: l.from_at,
                to_at: l.to_at,
                minutes: l.minutes,
                rate_minor_per_hour: l.rate_minor_per_hour,
                amount_minor: l.amount_minor,
            })
            .collect();
        bill(
            o.session_id.unwrap_or_default(),
            o.device_id,
            Some(o.id),
            Some(o.rev),
            lines,
        )
    });
    live.chain(open).collect()
}

/// Reads the snapshot from `store` at `now_ms`: the branch, the devices and the locale in one
/// read transaction.
pub fn read_snapshot(store: &Store, now_ms: i64) -> Result<HubSnapshot, AppError> {
    let (
        branch,
        devices,
        meta,
        groups,
        rates,
        zones,
        locale,
        ui,
        setup,
        sessions,
        segments,
        orders,
    ) = store.read_consistent(|s| {
        Ok((
            s.branch_summary()?,
            s.devices()?,
            s.devices_meta()?,
            s.groups()?,
            s.rates()?,
            s.zones()?,
            read_locale(s, now_ms)?,
            read_ui(s)?,
            read_setup(s)?,
            s.live_sessions()?,
            s.live_segments()?,
            s.open_orders()?,
        ))
    })?;
    let mut snapshot = build_snapshot(branch, devices, meta, groups, zones, locale, ui);
    snapshot.setup = setup;
    snapshot.bills = bills(&sessions, &segments, orders, now_ms);
    snapshot.sessions = sessions
        .into_iter()
        .map(|s| arena_proto::SnapshotSession {
            id: s.id,
            device_id: s.device_id,
            state: s.state,
            started_at: s.started_at,
            rev: s.rev,
            player_id: s.player_id,
            username: s.username,
            guest_no: s.guest_no,
        })
        .collect();
    for g in &mut snapshot.groups {
        g.rate_minor = rates
            .iter()
            .find(|r| r.group_id == g.id)
            .map(|r| r.price_per_hour_minor);
    }
    snapshot.device_cap = Some(read_device_cap(&store.settings()?));
    Ok(snapshot)
}

pub fn branch_setup(
    snapshot: HubSnapshot,
    presence: impl Fn(&str) -> (bool, Option<i64>),
) -> arena_proto::BranchSetup {
    arena_proto::BranchSetup {
        devices: snapshot
            .devices
            .into_iter()
            .filter(|d| d.class == ClientClass::Paired)
            .map(|mut d| {
                let (online, seen) = presence(&d.device_id);
                d.apply_presence(online, seen);
                arena_proto::SetupDevice {
                    id: d.device_id,
                    label: d.label,
                    kind: d.kind.unwrap_or_default(),
                    group_id: d.group_id,
                    status: d.status,
                }
            })
            .collect(),
        groups: snapshot
            .groups
            .into_iter()
            .map(|g| arena_proto::SetupGroup {
                id: g.id,
                name: g.name,
                kind: g.kind,
                colour: g.colour,
                sort: g.sort_order,
                has_rate: g.rate_minor.is_some(),
            })
            .collect(),
        status: snapshot.setup,
    }
}

fn read_device_cap(settings: &[arena_store::SettingRow]) -> u32 {
    settings
        .iter()
        .find(|s| s.key == arena_proto::KEY_DEVICE_CAP)
        .and_then(|s| s.value.parse().ok())
        .unwrap_or(arena_proto::Tier::Free.defaults().limits.devices)
}

fn read_setup(store: &Store) -> Result<Option<SetupStatus>, AppError> {
    if store.branch_summary()?.is_none() {
        return Ok(None);
    }
    let settings = store.settings()?;
    Ok(Some(crate::setup::setup_status(
        |key| {
            settings
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.value.as_str())
        },
        crate::setup::hub_tier(),
        u32::try_from(store.products_count()?).unwrap_or(0),
        u32::try_from(store.groups_without_rate()?).unwrap_or(0),
        store.manager_exists()?,
    )))
}

/// The setting behind [`SnapshotUi::idle_lock_minutes`] (`display.hc.hub` is behind
/// [`SnapshotUi::hc`]).
pub const KEY_IDLE_LOCK_MINUTES: &str = "idle_lock_minutes";

/// The Hub UI settings (the defaults before bootstrap).
fn read_ui(store: &Store) -> Result<SnapshotUi, AppError> {
    let settings = store.settings()?;
    let value = |key: &str| {
        settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.as_str())
    };
    Ok(SnapshotUi::from_settings(
        value(KEY_IDLE_LOCK_MINUTES),
        value(arena_proto::KEY_HC_HUB),
    ))
}

/// The locale in effect at `now_ms` (`None` before bootstrap).
fn read_locale(store: &Store, now_ms: i64) -> Result<Option<SnapshotLocale>, AppError> {
    let Some(branch) = store.branch_locale()? else {
        return Ok(None);
    };
    let rows = locale_rows(&branch, &store.settings()?);
    Ok(Some(snapshot_locale(&rows, now_ms)))
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

    /// The database file this reader reads.
    pub fn db_path(&self) -> Result<std::path::PathBuf, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store.path().to_path_buf())
    }

    pub fn paired_device(&self, device_id: &str) -> Result<bool, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store
            .devices()?
            .iter()
            .any(|d| d.device_id == device_id && !d.token_hash.is_empty()))
    }

    pub fn session_screens(
        &self,
        now_ms: i64,
    ) -> Result<Vec<(String, arena_proto::SessionScreen)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        crate::session_screen::session_screens(&store, now_ms)
    }

    pub fn settlement(&self, order_id: &str) -> Result<Option<arena_store::Settlement>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.settlement(order_id)
    }

    pub fn ending(&self, session_id: &str) -> Result<Option<arena_store::Ending>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.ending(session_id)
    }

    pub fn owner_wrote(&self, epoch: &str, seq: i64) -> bool {
        self.reader
            .lock()
            .ok()
            .and_then(|s| s.change_actor_kind(epoch, seq).ok().flatten())
            .is_some_and(|k| k == "owner")
    }

    pub fn player_revs(&self) -> Result<Vec<(String, i64)>, AppError> {
        self.reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?
            .player_revs()
    }

    pub fn player(
        &self,
        player_id: &str,
    ) -> Result<Option<(arena_store::PlayerRow, i64, String)>, AppError> {
        self.reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?
            .player(player_id)
    }

    pub fn latest_consent(
        &self,
        player_id: &str,
        purpose: &str,
    ) -> Result<Option<(bool, String)>, AppError> {
        self.reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?
            .latest_consent(player_id, purpose)
    }

    pub fn branch(&self) -> Result<Option<SnapshotBranch>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store.branch_summary()?.map(|b| SnapshotBranch {
            name: b.name,
            branch_code: b.branch_code,
            code_locked: b.code_locked.then_some(true),
        }))
    }

    /// The snapshot at `now_ms`. Blocking (SQLite).
    pub fn snapshot(&self, now_ms: i64) -> Result<HubSnapshot, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        read_snapshot(&store, now_ms)
    }

    /// The branch locale in effect at `now_ms` and when its next deferred change takes effect
    /// (UTC ms, `None` when nothing waits); `None` before bootstrap. Blocking (SQLite).
    pub fn locale_and_rollover(
        &self,
        now_ms: i64,
    ) -> Result<Option<(SnapshotLocale, Option<i64>)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let Some(branch) = s.branch_locale()? else {
                return Ok(None);
            };
            let rows = locale_rows(&branch, &s.settings()?);
            Ok(Some((
                snapshot_locale(&rows, now_ms),
                next_rollover(&rows, now_ms),
            )))
        })
    }

    /// The Hub UI settings now (the defaults before bootstrap). Blocking (SQLite).
    pub fn ui(&self) -> Result<SnapshotUi, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        read_ui(&store)
    }

    /// The branch locale in effect at `now_ms` (`None` before bootstrap). Blocking (SQLite).
    pub fn locale(&self, now_ms: i64) -> Result<Option<SnapshotLocale>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| read_locale(s, now_ms))
    }

    pub fn products(&self) -> Result<Vec<arena_proto::Product>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(crate::catalog::products)
    }

    pub fn catalog(&self, ids: &[String]) -> Result<Vec<arena_proto::CatalogEntry>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| crate::catalog::entries(s, ids))
    }

    pub fn setup(&self) -> Result<Option<SetupStatus>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(read_setup)
    }

    pub fn setting_rev_and_replay(
        &self,
        key: &str,
        idempotency_key: &str,
    ) -> Result<(Option<i64>, bool), AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let rev = s
                .settings()?
                .into_iter()
                .find(|r| r.key == key)
                .map(|r| r.rev);
            Ok((
                rev,
                s.has_applied_intent(&idempotency_key.to_ascii_uppercase())?,
            ))
        })
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
    use arena_store::{
        NewBranch, NewDevice, insert_branch, insert_device, insert_device_meta, set_device_revoked,
    };

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
                insert_device_meta(tx, "D1", "pc", "Front PC", None, None)?;
                set_device_revoked(tx, "D2", 5)
            })
            .unwrap();
    }

    #[test]
    fn snapshot_has_the_branch_and_only_live_devices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        seed(&path, true);
        let snap = SnapshotReader::open(&path).unwrap().snapshot(0).unwrap();
        assert_eq!(
            snap,
            HubSnapshot {
                branch: Some(SnapshotBranch {
                    name: "Dev Branch".into(),
                    branch_code: "DEV01".into(),
                    code_locked: None,
                }),
                groups: vec![],
                zones: vec![],
                sessions: vec![],
                bills: vec![],
                devices: vec![SnapshotDevice {
                    device_id: "D1".into(),
                    label: "Front PC".into(),
                    class: ClientClass::Paired,
                    zone: None,
                    online: false,
                    kind: Some(arena_proto::DeviceKind::Pc),
                    group_id: None,
                    meta_rev: Some(0),
                    tv_number: None,
                    floor_x: None,
                    floor_y: None,
                    manual: false,
                    status: DeviceStatus::Offline,
                    last_seen_at: None,
                    maintenance_note: None,
                }],
                locale: Some(SnapshotLocale {
                    currency: "INR".into(),
                    exponent: 2,
                    symbol: "₹".into(),
                    symbol_position: arena_proto::SymbolPosition::Before,
                    number_format: arena_proto::NumberFormat::Indian,
                    tz: "UTC".into(),
                    day_start_min: 0,
                    fy_start_month: 1,
                    pending: None,
                }),
                ui: SnapshotUi::default(),
                connectivity: arena_proto::Connectivity::default(),
                setup: Some(SetupStatus {
                    opening_hours_set: false,
                    backup_location_set: false,
                    ad_consent: arena_proto::AdConsentState::Missing,
                    printer: arena_proto::PrinterState::Unset,
                    products_count: 0,
                    groups_without_rate: 0,
                    manager_exists: false,
                    test_session: Default::default(),
                    live: false,
                }),
                lan: None,
                paired: true,
                pairing_revoked: None,
                licence_unverified: None,
                clock_wrong: None,
                device_cap: Some(20),
            }
        );
    }

    #[test]
    fn the_setup_block_has_paired_devices_with_presence_and_the_groups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        seed(&path, true);
        let snap = SnapshotReader::open(&path).unwrap().snapshot(0).unwrap();
        let block = branch_setup(snap, |id| (id == "D1", Some(5)));
        assert_eq!(block.devices.len(), 1);
        assert_eq!(block.devices[0].id, "D1");
        assert_eq!(block.devices[0].status, DeviceStatus::Online);
        assert!(block.groups.is_empty() && !block.grouped());
    }

    #[test]
    fn snapshot_before_bootstrap_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        seed(&path, false);
        let snap = SnapshotReader::open(&path).unwrap().snapshot(0).unwrap();
        assert_eq!(
            snap,
            HubSnapshot {
                branch: None,
                groups: vec![],
                zones: vec![],
                sessions: vec![],
                bills: vec![],
                devices: vec![],
                locale: None,
                ui: SnapshotUi::default(),
                connectivity: arena_proto::Connectivity::default(),
                setup: None,
                lan: None,
                paired: true,
                pairing_revoked: None,
                licence_unverified: None,
                clock_wrong: None,
                device_cap: Some(20),
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
        assert!(
            build_snapshot(
                None,
                vec![row],
                vec![],
                vec![],
                vec![],
                None,
                SnapshotUi::default()
            )
            .devices
            .is_empty()
        );
    }
}

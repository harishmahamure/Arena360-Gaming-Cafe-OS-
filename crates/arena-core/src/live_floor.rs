//! The live floor (Story 8.40): the trimmed, read-only view of devices and running sessions that
//! the owner sees through the tunnel. It carries times and ids only: no player names, balances,
//! bills or orders.

use std::collections::HashMap;

use arena_proto::{
    AppError, DeviceStatus, KEY_LICENCE_FLAGS, LiveFloor, LiveTile, LiveTileSession, LiveTileState,
    LiveZone, TUNNEL_FLAG,
};
use arena_store::{DeviceMetaRow, DeviceRow, LiveSession, Store, ZoneRow};

/// Builds the floor from the rows. A device appears when it is a paired one, not revoked, and
/// has a live (not deleted) floor row; presence is not known here (`Online`/`Offline` are the
/// registry's to set with [`set_presence`]).
pub fn build_live_floor(
    now_ms: i64,
    devices: Vec<DeviceRow>,
    meta: Vec<DeviceMetaRow>,
    zones: Vec<ZoneRow>,
    sessions: Vec<LiveSession>,
) -> LiveFloor {
    let meta: HashMap<String, DeviceMetaRow> = meta
        .into_iter()
        .filter(|m| m.deleted_at.is_none())
        .map(|m| (m.device_id.clone(), m))
        .collect();
    let mut zones: Vec<ZoneRow> = zones
        .into_iter()
        .filter(|z| z.deleted_at.is_none())
        .collect();
    zones.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
    let zone_ids: Vec<&str> = zones.iter().map(|z| z.zone_id.as_str()).collect();
    let by_device: HashMap<&str, &LiveSession> =
        sessions.iter().map(|s| (s.device_id.as_str(), s)).collect();
    let mut tiles: Vec<LiveTile> = devices
        .iter()
        .filter(|d| d.revoked_at.is_none() && d.class == "paired")
        .filter_map(|d| {
            let m = meta.get(&d.device_id)?;
            let held = DeviceStatus::parse(&m.state).filter(|s| s.held());
            let session = by_device.get(d.device_id.as_str()).copied();
            let state = match (session, held) {
                (Some(s), _) => session_state(&s.state),
                (None, Some(DeviceStatus::Maintenance)) => LiveTileState::Maintenance,
                (None, Some(DeviceStatus::Reserved)) => LiveTileState::Reserved,
                // A held `in_session` with no session row reads as free, not as a stale session.
                (None, _) => LiveTileState::Offline,
            };
            Some(LiveTile {
                device_id: d.device_id.clone(),
                label: m.label.clone(),
                zone: m.zone_id.clone().filter(|z| zone_ids.contains(&z.as_str())),
                state,
                session: session.map(tile_session),
            })
        })
        .collect();
    tiles.sort_by(|a, b| (&a.label, &a.device_id).cmp(&(&b.label, &b.device_id)));
    LiveFloor {
        generated_at: now_ms,
        zones: zones
            .into_iter()
            .map(|z| LiveZone {
                id: z.zone_id,
                name: z.name,
            })
            .collect(),
        tiles,
    }
}

fn session_state(state: &str) -> LiveTileState {
    match state {
        "paused" => LiveTileState::Paused,
        "grace" => LiveTileState::Grace,
        "expired" => LiveTileState::Expired,
        _ => LiveTileState::Active,
    }
}

fn tile_session(s: &LiveSession) -> LiveTileSession {
    LiveTileSession {
        started_at: s.started_at,
        paused_at: s.paused_at,
        paused_total_ms: s.paused_total_ms,
        ends_at: s.ends_at,
        grace_ends_at: s
            .ends_at
            .filter(|_| s.state == "grace")
            .map(|at| at + i64::from(s.grace_min) * 60_000),
        player_id: s.player_id.clone(),
        guest_no: s.guest_no.filter(|_| s.player_id.is_none()),
    }
}

/// Sets the presence of the tiles without a session: `online(device_id)` is the LAN registry's
/// answer. A device with a session, in maintenance or reserved keeps its state.
pub fn set_presence(floor: &mut LiveFloor, online: impl Fn(&str) -> bool) {
    for tile in &mut floor.tiles {
        if matches!(tile.state, LiveTileState::Offline | LiveTileState::Online) {
            tile.state = if online(&tile.device_id) {
                LiveTileState::Online
            } else {
                LiveTileState::Offline
            };
        }
    }
}

/// Reads the floor from `store` (presence not yet set).
pub fn read_live_floor(store: &Store, now_ms: i64) -> Result<LiveFloor, AppError> {
    Ok(build_live_floor(
        now_ms,
        store.devices()?,
        store.devices_meta()?,
        store.zones()?,
        store.live_sessions()?,
    ))
}

/// This branch's id and whether its licence carries the `tunnel` flag, for admitting a live
/// viewer. `None` when the branch is not bootstrapped.
pub fn read_live_gate(store: &Store) -> Result<Option<(String, bool)>, AppError> {
    let Some(branch) = store.branch_summary()? else {
        return Ok(None);
    };
    let flags = store
        .settings()?
        .into_iter()
        .find(|s| s.key == KEY_LICENCE_FLAGS)
        .is_some_and(|s| arena_proto::has_flag(&s.value, TUNNEL_FLAG));
    Ok(Some((branch.branch_id, flags)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, class: &str) -> DeviceRow {
        DeviceRow {
            device_id: id.into(),
            branch_id: "b".into(),
            class: class.into(),
            label: id.into(),
            token_hash: "x".into(),
            reported_version: None,
            revoked_at: None,
            rev: 1,
        }
    }

    fn meta(id: &str, label: &str, zone: Option<&str>, state: &str) -> DeviceMetaRow {
        DeviceMetaRow {
            device_id: id.into(),
            kind: "pc".into(),
            label: label.into(),
            group_id: None,
            rev: 1,
            deleted_at: None,
            tv_number: None,
            zone_id: zone.map(str::to_owned),
            floor_x: None,
            floor_y: None,
            state: state.into(),
            prior_state: None,
            maintenance_note: None,
        }
    }

    fn zone(id: &str, name: &str, sort: i64) -> ZoneRow {
        ZoneRow {
            zone_id: id.into(),
            name: name.into(),
            sort_order: sort,
            rev: 1,
            deleted_at: None,
        }
    }

    fn session(device: &str, state: &str) -> LiveSession {
        LiveSession {
            id: format!("s-{device}"),
            device_id: device.into(),
            state: state.into(),
            started_at: 1_000,
            player_id: None,
            username: Some("Asha Rao".into()),
            guest_no: Some(4),
            time_up_mode: "lock".into(),
            mode: "wallet".into(),
            rev: 1,
            rounding_min: 0,
            minimum_min: 0,
            paused_total_ms: 5,
            business_day: "2026-10-30".into(),
            fy: "2026-27".into(),
            ended_at: None,
            ended_by: None,
            ends_at: Some(9_000),
            paused_at: None,
            pause_reason: None,
            pause_note: None,
            pack_id: None,
            reserved_minor: 77_777,
            grace_min: 2,
            extended_min: 0,
            rate_id: None,
            group_id: "g".into(),
            group_bill_id: None,
        }
    }

    #[test]
    fn tiles_group_by_zone_carry_session_times_and_leave_out_names_and_money() {
        let floor = build_live_floor(
            5_000,
            vec![
                device("d1", "paired"),
                device("d2", "paired"),
                device("d3", "paired"),
                device("k1", "kds"),
            ],
            vec![
                meta("d1", "PC 1", Some("z1"), "online"),
                meta("d2", "PC 2", Some("gone"), "maintenance"),
                meta("d3", "PC 3", None, "online"),
                meta("k1", "Kitchen", None, "online"),
            ],
            vec![zone("z2", "Back", 2), zone("z1", "Front", 1)],
            vec![session("d1", "grace")],
        );
        assert_eq!(floor.generated_at, 5_000);
        assert_eq!(
            floor
                .zones
                .iter()
                .map(|z| z.name.as_str())
                .collect::<Vec<_>>(),
            ["Front", "Back"]
        );
        assert_eq!(
            floor.tiles.len(),
            3,
            "a kitchen display is not a floor tile"
        );
        let d1 = &floor.tiles[0];
        assert_eq!(
            (d1.label.as_str(), d1.zone.as_deref()),
            ("PC 1", Some("z1"))
        );
        assert_eq!(d1.state, LiveTileState::Grace);
        let s = d1.session.as_ref().unwrap();
        assert_eq!(s.grace_ends_at, Some(9_000 + 120_000));
        assert_eq!(s.guest_no, Some(4), "a guest shows its number");
        assert_eq!(s.player_id, None);
        assert_eq!(floor.tiles[1].state, LiveTileState::Maintenance);
        assert_eq!(
            floor.tiles[1].zone, None,
            "a deleted zone leaves the tile Unassigned"
        );
        let json = serde_json::to_string(&floor).unwrap();
        assert!(!json.contains("Asha") && !json.contains("77777") && !json.contains("wallet"));
    }

    #[test]
    fn a_player_session_shows_the_id_not_the_guest_number() {
        let mut s = session("d1", "active");
        s.player_id = Some("p-1".into());
        let floor = build_live_floor(
            1,
            vec![device("d1", "paired")],
            vec![meta("d1", "PC 1", None, "in_session")],
            vec![],
            vec![s],
        );
        let t = floor.tiles[0].session.as_ref().unwrap();
        assert_eq!((t.player_id.as_deref(), t.guest_no), (Some("p-1"), None));
        assert_eq!(floor.tiles[0].state, LiveTileState::Active);
    }

    #[test]
    fn presence_sets_only_tiles_without_a_session_or_hold() {
        let mut floor = build_live_floor(
            1,
            vec![
                device("d1", "paired"),
                device("d2", "paired"),
                device("d3", "paired"),
            ],
            vec![
                meta("d1", "PC 1", None, "online"),
                meta("d2", "PC 2", None, "online"),
                meta("d3", "PC 3", None, "maintenance"),
            ],
            vec![],
            vec![session("d2", "active")],
        );
        set_presence(&mut floor, |id| id == "d1" || id == "d3");
        let state = |id: &str| {
            floor
                .tiles
                .iter()
                .find(|t| t.device_id == id)
                .unwrap()
                .state
        };
        assert_eq!(state("d1"), LiveTileState::Online);
        assert_eq!(state("d2"), LiveTileState::Active);
        assert_eq!(state("d3"), LiveTileState::Maintenance);
    }
}

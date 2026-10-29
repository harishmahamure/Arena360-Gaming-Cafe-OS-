//! The Hub UI's branch snapshot ([`HubSnapshot`]): the branch name and the devices that are not
//! revoked. Read on its own read-only connection, never through the writer.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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
        group_bills: Vec::new(),
        shift: None,
        alerts: Vec::new(),
        counter_tickets: Vec::new(),
        locale,
        ui,
        // The probe's state is the Hub status's, not the database's: the snapshot handler
        // fills it in.
        connectivity: arena_proto::Connectivity::default(),
        setup: None,
        lan: None,
        paired: true,
        pairing_revoked: None,
        needs_repair: None,
        licence_unverified: None,
        clock_wrong: None,
        device_cap: None,
        tier: None,
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
                active_rule: None,
                next_change_at: None,
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
                    controller_check: None,
                    device_id: d.device_id,
                    // Presence is the LAN registry's, not the database's: the snapshot
                    // handler fills it in.
                    online: false,
                })
            })
            .collect(),
    }
}

pub(crate) fn credit_view(c: &arena_store::CreditRow) -> arena_proto::CreditView {
    arena_proto::CreditView {
        limit_minor: c.limit_minor,
        outstanding_minor: c.outstanding_minor,
        reminder_days: c.reminder_days.clone(),
        paused: c.paused,
    }
}

fn bills(
    sessions: &[arena_store::LiveSession],
    segments: &[arena_store::SegmentRow],
    (orders, order_lines): (Vec<arena_store::OpenOrder>, Vec<arena_store::OrderLineRow>),
    tickets: &[(arena_store::TicketRow, Vec<arena_store::TicketLineRow>)],
    rules: &[arena_store::PricingRuleRow],
    now_ms: i64,
) -> Vec<arena_proto::SnapshotBill> {
    let bill = |session_id: String,
                device_id: String,
                order_id: Option<String>,
                order_rev: Option<i64>,
                wallet_minor: Option<i64>,
                credit: Option<arena_proto::CreditView>,
                session_rev: Option<i64>,
                guest: Option<bool>,
                player_id: Option<String>,
                group_bill_id: Option<String>,
                lines: Vec<arena_proto::BillLine>,
                items: Vec<arena_proto::BillItem>| {
        let subtotal_minor = lines.iter().map(|l| l.amount_minor).sum::<i64>()
            + items.iter().map(|i| i.amount_minor).sum::<i64>();
        arena_proto::SnapshotBill {
            session_id,
            device_id,
            order_id,
            order_rev,
            wallet_minor,
            credit,
            session_rev,
            guest,
            player_id,
            group_bill_id,
            lines,
            items,
            items_order_id: None,
            ready: None,
            tickets: Vec::new(),
            subtotal_minor,
            paid_minor: 0,
            remaining_minor: subtotal_minor,
        }
    };
    let rule_name = |id: Option<&str>| {
        id.and_then(|id| rules.iter().find(|r| r.rule_id == id))
            .map(|r| r.name.clone())
    };
    // The products placed on a bill, by order id.
    let items_of = |order_id: &str| -> Vec<arena_proto::BillItem> {
        order_lines
            .iter()
            .filter(|l| l.order_id == order_id)
            .filter_map(|l| {
                Some(arena_proto::BillItem {
                    line_id: l.id.clone(),
                    served: l.served,
                    label: l.label.clone()?,
                    qty: l.qty.unwrap_or(1),
                    amount_minor: l.amount_minor,
                })
            })
            .collect()
    };
    // The open kitchen tickets of an order (Story 7.3).
    let tickets_of = |order_id: &str| -> Vec<arena_proto::KitchenTicketView> {
        tickets
            .iter()
            .filter(|(t, _)| t.order_id == order_id)
            .map(|(t, lines)| ticket_view(t, lines))
            .collect()
    };
    // A running postpaid session shows its placed products on its own live bill; its order is
    // not a separate bill to settle until the session ends. A block session's order is its bill.
    let folded = |order: &arena_store::OpenOrder| {
        order
            .session_id
            .as_ref()
            .is_some_and(|id| sessions.iter().any(|s| &s.id == id && s.mode != "block"))
    };
    let live = sessions.iter().map(|s| {
        let own: Vec<_> = segments
            .iter()
            .filter(|g| g.session_id == s.id)
            .cloned()
            .collect();
        let mut lines =
            crate::billing::bill(&own, s.rounding_min, s.minimum_min, s.paused_at, now_ms);
        for (line, seg) in lines.iter_mut().zip(&own) {
            line.rule_name = rule_name(seg.rule_id.as_deref());
        }
        let placed = orders
            .iter()
            .find(|o| o.session_id.as_deref() == Some(s.id.as_str()) && folded(o))
            .map(|o| o.id.clone());
        let mut live_bill = bill(
            s.id.clone(),
            s.device_id.clone(),
            None,
            None,
            None,
            None,
            Some(s.rev),
            s.player_id.is_none().then_some(true),
            s.player_id.clone(),
            s.group_bill_id.clone(),
            lines,
            placed.as_deref().map(items_of).unwrap_or_default(),
        );
        live_bill.ready = placed
            .as_deref()
            .and_then(|id| orders.iter().find(|o| o.id == id))
            .and_then(|o| o.ready_at)
            .map(|_| true);
        live_bill.tickets = placed.as_deref().map(tickets_of).unwrap_or_default();
        live_bill.items_order_id = placed.filter(|_| !live_bill.items.is_empty());
        live_bill
    });
    let open = orders.iter().filter(|o| !folded(o)).map(|o| {
        let guest = o.player_id.is_none() && o.session_id.is_some();
        let items = items_of(&o.id);
        let lines = order_lines
            .iter()
            .filter(|l| l.order_id == o.id && l.label.is_none())
            .map(|l| arena_proto::BillLine {
                device_id: l.device_id.clone(),
                from_at: l.from_at,
                to_at: l.to_at,
                minutes: l.minutes,
                rate_minor_per_hour: l.rate_minor_per_hour,
                amount_minor: l.amount_minor,
                rule_name: rule_name(l.rule_id.as_deref()),
            })
            .collect();
        let mut open_bill = bill(
            o.session_id.clone().unwrap_or_default(),
            o.device_id.clone(),
            Some(o.id.clone()),
            Some(o.rev),
            o.player_id.as_ref().map(|_| o.wallet_minor.unwrap_or(0)),
            o.credit.as_ref().map(credit_view),
            o.session_rev,
            guest.then_some(true),
            o.player_id.clone(),
            o.group_bill_id.clone(),
            lines,
            items,
        );
        open_bill.ready = o.ready_at.map(|_| true);
        open_bill.tickets = tickets_of(&o.id);
        if o.session_id.is_none() {
            // An unpaid counter order settles for its priced total, tax included (Story 6.32).
            open_bill.subtotal_minor = o.total_minor;
            open_bill.remaining_minor = o.total_minor;
        }
        open_bill
    });
    live.chain(open).collect()
}

/// A kitchen ticket as the Hub shows it.
pub(crate) fn ticket_view(
    t: &arena_store::TicketRow,
    lines: &[arena_store::TicketLineRow],
) -> arena_proto::KitchenTicketView {
    arena_proto::KitchenTicketView {
        id: t.id.clone(),
        order_id: t.order_id.clone(),
        number: t.number,
        seat: t.seat.clone(),
        stage: t.stage.clone(),
        placed_at: t.placed_at,
        rev: t.rev,
        lines: lines
            .iter()
            .map(|l| arena_proto::KitchenTicketLineView {
                line_id: l.order_line_id.clone(),
                label: l.label.clone(),
                qty: l.qty,
                notes: l.notes.clone(),
            })
            .collect(),
    }
}

fn group_bills(
    rows: Vec<arena_store::GroupBillRow>,
    sessions: &[arena_store::LiveSession],
    (ended, segments): (Vec<arena_store::LiveSession>, Vec<arena_store::SegmentRow>),
    rules: &[arena_store::PricingRuleRow],
    now_ms: i64,
) -> Vec<arena_proto::SnapshotGroupBill> {
    rows.into_iter()
        .map(|g| {
            let member_ids = sessions
                .iter()
                .filter(|s| s.group_bill_id.as_deref() == Some(g.id.as_str()))
                .map(|s| s.id.clone())
                .collect();
            let mut ended_lines = Vec::new();
            for m in ended
                .iter()
                .filter(|m| m.group_bill_id.as_deref() == Some(g.id.as_str()))
            {
                let own: Vec<_> = segments
                    .iter()
                    .filter(|seg| seg.session_id == m.id)
                    .cloned()
                    .collect();
                let mut lines =
                    crate::billing::bill(&own, m.rounding_min, m.minimum_min, None, now_ms);
                for (line, seg) in lines.iter_mut().zip(&own) {
                    line.rule_name = seg
                        .rule_id
                        .as_deref()
                        .and_then(|id| rules.iter().find(|r| r.rule_id == id))
                        .map(|r| r.name.clone());
                }
                ended_lines.extend(lines);
            }
            arena_proto::SnapshotGroupBill {
                id: g.id,
                rev: g.rev,
                payer_player_id: g.payer_player_id,
                payer_username: g.payer_username,
                payer_guest_no: g.payer_guest_no,
                member_ids,
                ended_lines,
            }
        })
        .collect()
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
        alerts,
        rules,
        group_rows,
        ended_group,
        unchecked,
        shift,
        tickets,
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
            s.alerts_with_products()?,
            s.pricing_rules()?,
            s.group_bills()?,
            s.ended_group_sessions()?,
            s.unchecked_consoles()?,
            s.shift_state()?,
            s.open_tickets()?,
        ))
    })?;
    let mut snapshot = build_snapshot(branch, devices, meta, groups, zones, locale, ui);
    snapshot.setup = setup;
    snapshot.shift = Some(shift);
    for (device_id, session_id) in unchecked {
        if let Some(d) = snapshot
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
        {
            d.controller_check = Some(session_id);
        }
    }
    snapshot.bills = bills(&sessions, &segments, orders, &tickets, &rules, now_ms);
    snapshot.group_bills = group_bills(group_rows, &sessions, ended_group, &rules, now_ms);
    snapshot.counter_tickets = tickets
        .iter()
        .filter(|(t, _)| t.device_id.is_none())
        .map(|(t, lines)| ticket_view(t, lines))
        .collect();
    snapshot.alerts = alerts
        .into_iter()
        .filter_map(|(a, product)| {
            let ticket_number = a
                .ticket_id
                .as_deref()
                .and_then(|id| tickets.iter().find(|(t, _)| t.id == id))
                .map(|(t, _)| t.number);
            Some(arena_proto::SnapshotAlert {
                kind: arena_proto::AlertKind::parse(&a.kind)?,
                id: a.id,
                device_id: a.device_id,
                raised_at: a.raised_at,
                product_id: a.product_id,
                product,
                qty: a.qty,
                threshold: a.threshold,
                ticket_id: a.ticket_id,
                ticket_number,
            })
        })
        .collect();
    snapshot.sessions = sessions
        .into_iter()
        .map(|s| arena_proto::SnapshotSession {
            grace_ends_at: s
                .ends_at
                .filter(|_| s.state == "grace")
                .map(|at| at + i64::from(s.grace_min) * 60_000),
            id: s.id,
            device_id: s.device_id,
            state: s.state,
            started_at: s.started_at,
            rev: s.rev,
            player_id: s.player_id,
            username: s.username,
            guest_no: s.guest_no,
            paused_at: s.paused_at,
            paused_total_ms: s.paused_total_ms,
            pause_reason: s
                .pause_reason
                .as_deref()
                .and_then(arena_proto::PauseReason::parse),
            pause_note: s.pause_note,
            ends_at: s.ends_at,
            mode: Some(s.mode),
            pack_id: s.pack_id,
            group_bill_id: s.group_bill_id,
        })
        .collect();
    let tz = snapshot
        .locale
        .as_ref()
        .and_then(|l| l.tz.parse::<chrono_tz::Tz>().ok());
    for g in &mut snapshot.groups {
        g.rate_minor = rates
            .iter()
            .find(|r| r.group_id == g.id)
            .map(|r| r.price_per_hour_minor);
        let (Some(base), Some(tz)) = (g.rate_minor, tz) else {
            continue;
        };
        let resolved = crate::pricing::resolve(&rules, &g.id, base, tz, now_ms);
        g.next_change_at = resolved.next_change_at;
        g.active_rule = resolved
            .rule_id
            .and_then(|id| rules.iter().find(|r| r.rule_id == id))
            .map(|r| arena_proto::SnapshotActiveRule {
                rule_id: r.rule_id.clone(),
                name: r.name.clone(),
                adjust_kind: r.adjust_kind.clone(),
                adjust_value: r.adjust_value,
                rate_minor: resolved.rate_minor,
                until: resolved.until,
            });
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

/// What the timer owes running sessions for rule changes: the sessions with a rate change due
/// now, and the earliest upcoming rule boundary for the others (`None` when no rule is near).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateFlips {
    pub due: Vec<String>,
    pub next_at: Option<i64>,
}

fn rate_flips_in(store: &Store, now_ms: i64) -> Result<RateFlips, AppError> {
    let (sessions, segments, rates, rules, branch, settings) = store.read_consistent(|s| {
        Ok((
            s.live_sessions()?,
            s.live_segments()?,
            s.all_rates()?,
            s.pricing_rules()?,
            s.branch_locale()?,
            s.settings()?,
        ))
    })?;
    let Some(branch) = branch else {
        return Ok(RateFlips::default());
    };
    let tz = crate::locale::resolve(&locale_rows(&branch, &settings), now_ms)
        .1
        .tz;
    let mut out = RateFlips::default();
    for s in &sessions {
        let Some(base) = s
            .rate_id
            .as_deref()
            .and_then(|id| rates.iter().find(|r| r.rate_id == id))
        else {
            continue;
        };
        let own: Vec<_> = segments
            .iter()
            .filter(|g| g.session_id == s.id)
            .cloned()
            .collect();
        if crate::apply::rate_flip::pending(s, &own, base, &rules, tz, now_ms).is_some() {
            out.due.push(s.id.clone());
        } else if let Some(at) = crate::pricing::resolve(
            &rules,
            &base.group_id,
            base.price_per_hour_minor,
            tz,
            now_ms,
        )
        .next_change_at
        {
            out.next_at = Some(out.next_at.map_or(at, |n| n.min(at)));
        }
    }
    Ok(out)
}

/// A read-only connection that serves [`HubSnapshot`]s on a primary or a follower.
pub struct SnapshotReader {
    reader: Mutex<Store>,
    /// Set by the backup-folder watch while the chosen folder cannot be written; setup status
    /// then reports the backup location as not set.
    backup_unavailable: AtomicBool,
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
            backup_unavailable: AtomicBool::new(false),
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

    /// Whether the live session `session_id` is paused (`false` when it is not live).
    pub fn session_paused(&self, session_id: &str) -> Result<bool, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store
            .live_sessions()?
            .iter()
            .any(|s| s.id == session_id && s.state == "paused"))
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

    pub fn timed_sessions(&self, now_ms: i64) -> Result<Vec<crate::TimedSession>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        crate::session_screen::timed_sessions(&store, now_ms)
    }

    /// The minutes-left marks the branch warns its players at, longest first (Story 6.20).
    pub fn warn_marks(&self) -> Result<Vec<u8>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let settings = store.settings()?;
        Ok(arena_proto::warn_marks_from(
            settings
                .iter()
                .find(|r| r.key == arena_proto::KEY_BILLING_WARN_MARKS)
                .map(|r| r.value.as_str()),
        ))
    }

    /// The rate changes due on running sessions at `now_ms`, and when to look again.
    pub fn rate_flips(&self, now_ms: i64) -> Result<RateFlips, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        rate_flips_in(&store, now_ms)
    }

    /// Settled sales still without an invoice number, oldest settle first: `(order id, settled_at)`.
    pub fn unnumbered_orders(&self, limit: i64) -> Result<Vec<(String, i64)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.unnumbered_orders(limit)
    }

    /// Receipts waiting on the side-effect outbox, oldest first (Story 5.17).
    pub fn queued_prints(&self, limit: i64) -> Result<Vec<arena_store::OutboxRow>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.queued_prints(limit)
    }

    /// Drawer kicks waiting on the side-effect outbox, oldest first (Story 5.18).
    pub fn queued_drawers(&self, limit: i64) -> Result<Vec<arena_store::OutboxRow>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.queued_drawers(limit)
    }

    /// The ESC/POS bytes of an order's receipt at the saved paper width, marked "Reprint" when
    /// `reprint` is set; `None` when there is no such order.
    pub fn receipt_bytes(
        &self,
        order_id: &str,
        reprint: bool,
        now_ms: i64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let source = {
            let store = self
                .reader
                .lock()
                .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
            store.receipt_source(order_id)?
        };
        let Some(source) = source else {
            return Ok(None);
        };
        let settings = self.settings()?;
        let snapshot = self.snapshot(now_ms)?;
        let name = snapshot.branch.as_ref().map_or("", |b| b.name.as_str());
        Ok(Some(crate::printer::job::render_order(
            &settings,
            name,
            snapshot.locale.as_ref(),
            &source,
            reprint,
        )))
    }

    /// The ESC/POS bytes of a credit note at the saved paper width; `None` when there is no such
    /// refund.
    pub fn credit_note_bytes(
        &self,
        refund_id: &str,
        now_ms: i64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let source = {
            let store = self
                .reader
                .lock()
                .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
            store.credit_note_source(refund_id)?
        };
        let Some(source) = source else {
            return Ok(None);
        };
        let settings = self.settings()?;
        let snapshot = self.snapshot(now_ms)?;
        let name = snapshot.branch.as_ref().map_or("", |b| b.name.as_str());
        Ok(Some(crate::printer::job::render_credit_note_order(
            &settings,
            name,
            snapshot.locale.as_ref(),
            &source,
        )))
    }

    pub fn settlement(&self, order_id: &str) -> Result<Option<arena_store::Settlement>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.settlement(order_id)
    }

    /// An order's `rev`, total and when `placement_id` placed its lines (see `Store::placement`).
    pub fn placement(
        &self,
        order_id: &str,
        placement_id: &str,
    ) -> Result<Option<(i64, i64, i64)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.placement(order_id, placement_id)
    }

    /// The session an order is billed to.
    pub fn order_session(&self, order_id: &str) -> Result<Option<String>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.order_session(order_id)
    }

    /// The session a kitchen ticket's order is billed to.
    pub fn ticket_session(&self, ticket_id: &str) -> Result<Option<String>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.ticket_session(ticket_id)
    }

    /// The session a line sits on (`None` when the line or its order is gone).
    pub fn line_session(&self, line_id: &str) -> Result<Option<String>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.line_session(line_id)
    }

    /// The PC and name of a product line (`None` when the line or its order is gone); a cancel
    /// reads it first so the PC can be told the item's name (Story 6.33).
    pub fn line_device_label(&self, line_id: &str) -> Result<Option<(String, String)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.line_device_label(line_id)
    }

    /// The kind of the intent `intent_id` (`None` when it wrote nothing).
    pub fn intent_kind(&self, intent_id: &str) -> Result<Option<String>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.intent_kind(intent_id)
    }

    /// The session and order a placement put products on.
    pub fn placed_order(&self, placement_id: &str) -> Result<Option<(String, String)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.placed_order(placement_id)
    }

    pub fn is_pos_sale(&self, order_id: &str) -> Result<bool, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.is_pos_sale(order_id)
    }

    pub fn wallet(
        &self,
        player_id: &str,
        now_ms: i64,
    ) -> Result<arena_proto::WalletView, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let txns = store
            .wallet_txns(player_id)?
            .into_iter()
            .map(|t| arena_proto::WalletTxnView {
                id: t.id,
                at: t.at,
                delta_minor: t.delta_minor,
                reason: t.reason,
                method: t.method,
                staff: t.staff,
            })
            .collect();
        let credit = store.credit(player_id)?;
        let aging = match &credit {
            Some(_) => match crate::locale::current_business_day(&store, now_ms)? {
                Some(today) => Some(store.credit_aging(player_id, &today)?),
                None => None,
            },
            None => None,
        };
        Ok(arena_proto::WalletView {
            player_id: player_id.to_owned(),
            balance_minor: store.wallet_balance(player_id)?,
            credit: credit.as_ref().map(credit_view),
            aging,
            txns,
            plans: store
                .player_packs(player_id)?
                .into_iter()
                .map(|p| arena_proto::OwnedPlanView {
                    expired: p.expires_at <= now_ms,
                    id: p.id,
                    plan_id: p.plan_id,
                    plan_kind: p.kind,
                    name: p.name,
                    minutes_left: p.minutes_left,
                    minutes_total: p.minutes_total,
                    window_start_min: p.window_start_min,
                    window_end_min: p.window_end_min,
                    group_ids: p.group_ids,
                    bought_at: p.bought_at,
                    expires_at: p.expires_at,
                })
                .collect(),
        })
    }

    pub fn outstanding(&self, player_id: &str) -> Result<i64, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store.credit(player_id)?.map_or(0, |c| c.outstanding_minor))
    }

    pub fn sessions_today(&self, now_ms: i64) -> Result<arena_proto::SessionListView, AppError> {
        use arena_proto::SessionListState as S;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let business_day = crate::locale::current_business_day(&store, now_ms)?.unwrap_or_default();
        let sessions = store
            .sessions_for_day(&business_day)?
            .into_iter()
            .map(|s| {
                let state = match (s.state.as_str(), s.order_status.as_deref()) {
                    ("active" | "paused" | "grace", _) => S::Running,
                    ("expired", _) if s.ended_at.is_none() => S::Running,
                    (_, Some("open")) => S::Unpaid,
                    _ => S::Settled,
                };
                arena_proto::SessionListRow {
                    id: s.id,
                    device_id: s.device_id,
                    device: s.device,
                    username: s.username,
                    guest_no: s.guest_no,
                    mode: s.mode,
                    started_at: s.started_at,
                    ended_at: s.ended_at,
                    bill_minor: s.total_minor,
                    state,
                    receipt_order_id: s
                        .order_id
                        .clone()
                        .filter(|_| s.invoice_no.is_some() && state == S::Settled),
                    invoice_no: s.invoice_no,
                    order_id: s.order_id.filter(|_| state == S::Unpaid),
                    controller_check: s.controller_check.as_deref().and_then(|c| match c {
                        "returned" => Some(arena_proto::ControllerResult::Returned),
                        "missing" => Some(arena_proto::ControllerResult::Missing),
                        _ => None,
                    }),
                }
            })
            .collect();
        Ok(arena_proto::SessionListView {
            business_day,
            sessions,
        })
    }

    /// The business day's settled counter sales, newest first (Recent sales on POS).
    pub fn pos_sales(&self, now_ms: i64) -> Result<arena_proto::PosSalesView, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let business_day = crate::locale::current_business_day(&store, now_ms)?.unwrap_or_default();
        let sales = store.pos_sales(&business_day)?;
        Ok(arena_proto::PosSalesView {
            business_day,
            sales,
        })
    }

    /// The reveal for a stated count against the open shift (Story 5.26): `SHIFT_NOT_OPEN` when no
    /// shift is open.
    pub fn shift_count(&self, counted_minor: i64) -> Result<arena_proto::ShiftCount, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.shift_count(counted_minor)?.ok_or_else(|| {
            AppError::with_field(
                ErrorCode::ShiftNotOpen,
                "shift",
                "There is no shift to close.",
            )
        })
    }

    /// The Z-report of a closed shift (Story 5.28): the one stored at close, or for a shift closed
    /// before that, one built from its rows now (`rebuilt`). `NOT_FOUND` for an unknown shift,
    /// `STALE` while it is still open. Never writes.
    pub fn z_report(&self, shift_id: &str, now_ms: i64) -> Result<arena_proto::ZReport, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let detail = store.shift_detail(shift_id)?.ok_or_else(|| {
            AppError::with_field(ErrorCode::NotFound, "shift_id", "There is no such shift.")
        })?;
        if detail.open {
            return Err(AppError::with_field(
                ErrorCode::Stale,
                "shift_id",
                "This shift is still open.",
            ));
        }
        if let Some(stored) = detail
            .z_report
            .as_deref()
            .and_then(|j| serde_json::from_str::<arena_proto::ZReport>(j).ok())
        {
            return Ok(stored);
        }
        let rows = store.z_report_rows(shift_id)?.ok_or_else(|| {
            AppError::with_field(ErrorCode::NotFound, "shift_id", "There is no such shift.")
        })?;
        let tz = crate::locale::current_tz(&store, now_ms)?;
        let mut report = crate::reports::z_report(&rows, tz);
        report.rebuilt = true;
        Ok(report)
    }

    /// The closed shifts, newest first (the Shifts report, Story 5.28).
    pub fn shift_list(&self) -> Result<arena_proto::ShiftList, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let shifts = store
            .closed_shifts(arena_proto::SHIFT_LIST_LIMIT)?
            .into_iter()
            .filter_map(|s| {
                Some(arena_proto::ShiftListRow {
                    opened_at: s.opened_at,
                    closed_at: s.closed_at?,
                    opened_by: s.opened_by,
                    closed_by: s.closed_by,
                    counted_minor: s.counted_minor.unwrap_or(0),
                    variance_minor: s.variance_minor.unwrap_or(0),
                    shift_id: s.id,
                })
            })
            .collect();
        Ok(arena_proto::ShiftList { shifts })
    }

    /// A month of the GST return (Story 5.29), built from the stored invoices and credit notes.
    /// `VALIDATION_FAILED` on a bad month (`month`) or in a branch that is not GST registered
    /// (`tax`). Never writes.
    pub fn gst_month(&self, month: &str) -> Result<arena_proto::GstMonth, AppError> {
        Ok(self.gst_return(month)?.1.month)
    }

    /// The export of a month (Story 5.29): the file name, the ZIP and the month's totals.
    pub fn gst_export(
        &self,
        month: &str,
    ) -> Result<(String, Vec<u8>, arena_proto::GstMonth), AppError> {
        let (code, built) = self.gst_return(month)?;
        Ok((
            arena_proto::gst_file_name(&code, month),
            built.csv.to_zip(),
            built.month,
        ))
    }

    fn gst_return(&self, month: &str) -> Result<(String, crate::gst::GstReturn), AppError> {
        let (from, to) = crate::gst::month_range(month)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let tax = store
            .settings()?
            .into_iter()
            .find(|s| s.key == arena_proto::KEY_TAX)
            .and_then(|s| arena_proto::TaxSettings::parse(&s.value))
            .filter(crate::gst::gst_applies)
            .ok_or_else(|| {
                AppError::with_field(
                    ErrorCode::ValidationFailed,
                    "tax",
                    "The GST return applies to a GST-registered branch.",
                )
            })?;
        let code = store
            .branch_summary()?
            .map(|b| b.branch_code)
            .unwrap_or_default();
        let rows = store.gst_rows(&from, &to)?;
        Ok((code, crate::gst::build(&rows, &tax, month)))
    }

    /// The ESC/POS bytes of a closed shift's Z-report at the saved paper width, marked "Reprint"
    /// when `reprint` is set; `None` when there is no such shift or it is still open.
    pub fn z_report_bytes(
        &self,
        shift_id: &str,
        reprint: bool,
        now_ms: i64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let report = match self.z_report(shift_id, now_ms) {
            Ok(r) => r,
            Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::Stale) => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let settings = self.settings()?;
        let snapshot = self.snapshot(now_ms)?;
        let name = snapshot.branch.as_ref().map_or("", |b| b.name.as_str());
        Ok(Some(crate::printer::job::z_report_bytes(
            &settings,
            name,
            snapshot.locale.as_ref(),
            &report,
            reprint,
        )))
    }

    /// Daily sales for the business days `from` to `to` (Story 5.27), `VALIDATION_FAILED` on a
    /// bad range.
    pub fn daily_sales(
        &self,
        from: &str,
        to: &str,
        now_ms: i64,
    ) -> Result<arena_proto::DailySales, AppError> {
        crate::reports::parse_range(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let tz = crate::locale::current_tz(&store, now_ms)?;
        let rows = store.sales_report_rows(from, to)?;
        Ok(crate::reports::daily_sales(&rows, from, to, tz))
    }

    /// Sessions for the business days `from` to `to` (Story 5.27).
    pub fn sessions_report(
        &self,
        from: &str,
        to: &str,
        now_ms: i64,
    ) -> Result<arena_proto::SessionsReport, AppError> {
        let days = crate::reports::parse_range(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let tz = crate::locale::current_tz(&store, now_ms)?;
        let rows = store.sessions_report_rows(from, to, now_ms)?;
        Ok(crate::reports::sessions_report(&rows, from, to, days, tz))
    }

    /// The Players report for the business days `from` to `to` (Story 5.30). Names are opened with
    /// `kek`; credit is aged as of the business day at `now_ms`.
    pub fn players_report(
        &self,
        from: &str,
        to: &str,
        now_ms: i64,
        kek: &[u8; crate::players::KEK_LEN],
    ) -> Result<arena_proto::PlayersReport, AppError> {
        crate::reports::parse_range(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let today = crate::locale::current_business_day(&store, now_ms)?
            .unwrap_or_else(|| crate::players::utc_day(now_ms));
        let rows = store.players_report_rows(from, to, &today)?;
        Ok(crate::reports::players_report(&rows, from, to, &today, kek))
    }

    /// The Staff report for the business days `from` to `to` (Story 5.30).
    pub fn staff_report(&self, from: &str, to: &str) -> Result<arena_proto::StaffReport, AppError> {
        crate::reports::parse_range(from, to)?;
        let (shift_from, shift_to) = crate::reports::shift_read_window(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let day_of = crate::locale::business_day_fn(&store)?;
        let rows = store.staff_report_rows(from, to, shift_from, shift_to)?;
        Ok(crate::reports::staff_report(&rows, from, to, &day_of))
    }

    /// Stock on hand per tracked product with its value at the average cost (Story 5.31).
    pub fn stock_report(&self) -> Result<arena_proto::StockReport, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(crate::stock_report::stock_report(&store.stock_levels()?))
    }

    /// Every stock movement of the business days `from` to `to`, newest first, optionally of one
    /// product and one move reason (Story 5.31).
    pub fn stock_moves(
        &self,
        from: &str,
        to: &str,
        product_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<arena_proto::StockMoves, AppError> {
        crate::reports::parse_range(from, to)?;
        let (from_ms, to_ms) = crate::reports::shift_read_window(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let day_of = crate::locale::business_day_fn(&store)?;
        let rows = store.stock_moves_rows(from_ms, to_ms, product_id, reason, false)?;
        Ok(crate::stock_report::stock_moves(&rows, from, to, &day_of))
    }

    /// Waste by reason and by product for the business days `from` to `to` (Story 5.31).
    pub fn waste_report(&self, from: &str, to: &str) -> Result<arena_proto::WasteReport, AppError> {
        crate::reports::parse_range(from, to)?;
        let (from_ms, to_ms) = crate::reports::shift_read_window(from, to)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let day_of = crate::locale::business_day_fn(&store)?;
        let rows = store.stock_moves_rows(from_ms, to_ms, None, None, true)?;
        Ok(crate::stock_report::waste_report(&rows, from, to, &day_of))
    }

    /// The invoice numbered `invoice_no` for the Refund screen: `INVOICE_NOT_FOUND` when no settled
    /// order carries that number.
    pub fn refund_invoice(&self, invoice_no: &str) -> Result<arena_proto::RefundInvoice, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.refund_invoice(invoice_no)?.ok_or_else(|| {
            AppError::with_field(
                ErrorCode::InvoiceNotFound,
                "no",
                "There is no such invoice.",
            )
        })
    }

    pub fn wallet_balance(&self, player_id: &str) -> Result<i64, AppError> {
        self.reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?
            .wallet_balance(player_id)
    }

    pub fn credit_collection(
        &self,
        order_id: &str,
    ) -> Result<Option<arena_proto::CreditCollection>, AppError> {
        self.reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?
            .credit_collection(order_id)
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
        let mut snapshot = read_snapshot(&store, now_ms)?;
        if let Some(setup) = snapshot.setup.as_mut() {
            setup.backup_location_set &= !self.backup_unavailable.load(Ordering::Relaxed);
        }
        Ok(snapshot)
    }

    /// Whether the chosen backup folder was last found unavailable.
    pub fn backup_unavailable(&self) -> bool {
        self.backup_unavailable.load(Ordering::Relaxed)
    }

    /// Records whether the chosen backup folder is unavailable; `true` when that changed.
    pub fn set_backup_unavailable(&self, unavailable: bool) -> bool {
        self.backup_unavailable.swap(unavailable, Ordering::Relaxed) != unavailable
    }

    /// The value of one setting; `None` when the row does not exist. Blocking (SQLite).
    pub fn setting_value(&self, key: &str) -> Result<Option<String>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        Ok(store
            .settings()?
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| s.value))
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

    pub fn products(&self, now_ms: i64) -> Result<Vec<arena_proto::Product>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| crate::catalog::products(s, now_ms))
    }

    /// A dry-run of a product CSV (Story 5.33): the counts and problems, and the rows that could
    /// be imported, read against the products the Hub holds now. Writes nothing. Blocking
    /// (SQLite).
    pub fn product_import(
        &self,
        csv: &str,
        now_ms: i64,
    ) -> Result<
        (
            arena_proto::ProductImportPreview,
            Vec<arena_proto::ImportProductRow>,
        ),
        AppError,
    > {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let exponent = read_locale(s, now_ms)?.map_or(2, |l| l.exponent);
            let ctx = crate::import_products::context(s)?;
            let analysis = crate::import_products::analyse(csv, &ctx, exponent)?;
            Ok((analysis.preview, analysis.rows))
        })
    }

    /// A dry-run of a player CSV (Story 5.34): the counts and problems, the rows that would
    /// create a player and the usernames already in use, read against the profiles the Hub holds
    /// now. Writes nothing. Blocking (SQLite).
    pub fn player_import(
        &self,
        csv: &str,
        now_ms: i64,
        kek: &[u8; crate::players::KEK_LEN],
    ) -> Result<
        (
            arena_proto::PlayerImportPreview,
            Vec<crate::import_players::PlayerLine>,
            std::collections::HashSet<String>,
        ),
        AppError,
    > {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let exponent = read_locale(s, now_ms)?.map_or(2, |l| l.exponent);
            let ctx = crate::import_players::context(s)?;
            let analysis = crate::import_players::analyse(csv, &ctx, exponent, kek)?;
            Ok((analysis.preview, analysis.rows, ctx.usernames))
        })
    }

    /// A dry-run of a plan CSV (Story 5.34): the counts and problems and the rows that could be
    /// imported, read against the plans and groups the Hub holds now. Writes nothing. Blocking
    /// (SQLite).
    pub fn plan_import(
        &self,
        csv: &str,
        now_ms: i64,
    ) -> Result<
        (
            arena_proto::PlanImportPreview,
            Vec<arena_proto::ImportPlanRow>,
        ),
        AppError,
    > {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let exponent = read_locale(s, now_ms)?.map_or(2, |l| l.exponent);
            let ctx = crate::import_plans::context(s)?;
            let analysis = crate::import_plans::analyse(csv, &ctx, exponent)?;
            Ok((analysis.preview, analysis.rows))
        })
    }

    /// The starter templates (Story 5.35), priced in the branch currency. Blocking (SQLite).
    pub fn starter_templates(
        &self,
        now_ms: i64,
    ) -> Result<Vec<arena_proto::StarterTemplateInfo>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let exponent = read_locale(s, now_ms)?.map_or(2, |l| l.exponent);
            Ok(crate::import_starter::templates(exponent))
        })
    }

    /// A dry-run of a starter template (Story 5.35): what it would add and what is skipped, and
    /// the plan and product rows to commit, read against the Hub now. Writes nothing. Blocking
    /// (SQLite).
    pub fn starter_import(
        &self,
        template_id: &str,
        now_ms: i64,
    ) -> Result<
        (
            arena_proto::StarterPreview,
            Vec<arena_proto::ImportPlanRow>,
            Vec<arena_proto::ImportProductRow>,
        ),
        AppError,
    > {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let exponent = read_locale(s, now_ms)?.map_or(2, |l| l.exponent);
            let products = crate::import_products::context(s)?;
            let plans = crate::import_plans::context(s)?;
            let a = crate::import_starter::analyse(template_id, &products, &plans, exponent)?;
            Ok((a.preview, a.plan_rows, a.product_rows))
        })
    }

    /// The branch's setup file (Story 5.36): groups with rates, products, bundles, plans, pricing
    /// rules and coupons, as the text an export saves. Blocking (SQLite).
    pub fn setup_export(&self, now_ms: i64) -> Result<String, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let file = store.read_with(|conn| crate::setup_file::export(conn, now_ms))?;
        crate::setup_file::to_text(&file)
    }

    /// A dry-run of a setup file (Story 5.36): what it would create and what is skipped, read
    /// against the Hub now. A file from a newer format is `UNSUPPORTED`. Writes nothing.
    /// Blocking (SQLite).
    pub fn setup_import(
        &self,
        json: &str,
        now_ms: i64,
    ) -> Result<(arena_proto::SetupPreview, arena_proto::SetupFile), AppError> {
        let file = crate::setup_file::parse(json)?;
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let analysis = store.read_with(|conn| {
            let ctx = crate::setup_file::Context::load(conn, now_ms)?;
            crate::setup_file::analyse(&file, &ctx)
        })?;
        Ok((analysis.preview, file))
    }

    /// The Inventory → Expiring and Expired lists as of `now_ms`. Blocking (SQLite).
    pub fn expiry(&self, now_ms: i64) -> Result<arena_proto::ExpiryView, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| crate::catalog::expiry_view(s, now_ms))
    }

    /// The Inventory → Low stock page. Blocking (SQLite).
    pub fn low_stock(&self) -> Result<arena_proto::LowStockView, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(crate::catalog::low_stock_view)
    }

    pub fn rules(&self) -> Result<Vec<arena_proto::RuleView>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let rows = s.pricing_rules()?;
            let conflicts = crate::pricing::conflicts(&rows);
            Ok(rows
                .into_iter()
                .zip(conflicts)
                .map(|(r, conflicts)| arena_proto::RuleView {
                    id: r.rule_id,
                    name: r.name,
                    group_id: r.group_id,
                    rule_kind: r.rule_kind,
                    days_mask: r.days_mask,
                    start_min: r.start_min,
                    end_min: r.end_min,
                    date_from: r.date_from,
                    date_to: r.date_to,
                    adjust_kind: r.adjust_kind,
                    adjust_value: r.adjust_value,
                    priority: r.priority,
                    active: r.active,
                    rev: r.rev,
                    conflicts,
                })
                .collect())
        })
    }

    /// Every coupon with its targets, for Settings → Coupons and the audit of promotions.
    pub fn coupons(&self) -> Result<Vec<arena_proto::CouponView>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            Ok(s.coupons()?
                .into_iter()
                .map(|(c, targets)| arena_proto::CouponView {
                    id: c.coupon_id,
                    code: c.code,
                    percent_bp: c.percent_bp,
                    amount_minor: c.amount_minor,
                    valid_from: c.valid_from,
                    valid_to: c.valid_to,
                    usage_limit: c.usage_limit,
                    used: c.used,
                    min_bill_minor: c.min_bill_minor,
                    targets: targets
                        .into_iter()
                        .map(|(kind, id)| arena_proto::CouponTarget { kind, id })
                        .collect(),
                    active: c.active,
                    rev: c.rev,
                })
                .collect())
        })
    }

    /// The launch times of the last [`arena_proto::LAUNCH_MEDIAN_DAYS`] days before `now_ms`, per
    /// live paired PC (Story 6.25).
    pub fn game_launch_times(&self, now_ms: i64) -> Result<arena_proto::LaunchTimesView, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let since = now_ms - arena_proto::LAUNCH_MEDIAN_DAYS * 24 * 60 * 60 * 1000;
        store.read_consistent(|s| Ok(crate::launch_times::build(s.game_launches(since)?)))
    }

    /// Every live paired PC with its last installed-software scan, ordered by name (Story 6.4).
    /// The scan fields are `None` for a PC that was never scanned.
    pub fn kiosk_scans(&self) -> Result<Vec<arena_proto::DeviceScanView>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            Ok(s.kiosk_scan_summaries()?
                .into_iter()
                .map(|r| arena_proto::DeviceScanView {
                    device_id: r.device_id,
                    name: r.label,
                    scanned_at: r.scanned_at,
                    entry_count: r.entry_count,
                    skipped: r.skipped,
                    launchers: r
                        .launchers
                        .map(|j| serde_json::from_str(&j).unwrap_or_default()),
                })
                .collect())
        })
    }

    /// The stored scan of one device: when it was made and its executables (Story 6.5). `None`
    /// when the device was never scanned.
    pub fn device_scan(
        &self,
        device_id: &str,
    ) -> Result<Option<(i64, Vec<arena_proto::ScanEntry>)>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            Ok(s.kiosk_scans_full()?
                .into_iter()
                .find(|r| r.device_id == device_id)
                .map(|r| {
                    (
                        r.scanned_at,
                        serde_json::from_str(&r.entries).unwrap_or_default(),
                    )
                }))
        })
    }

    /// What "Found on PCs" is built from, read in one snapshot (Story 6.5): every live paired PC
    /// with its stored scan and catalogue matches, the library with its installs, and the
    /// dismissed proposal keys.
    pub fn proposal_inputs(&self) -> Result<crate::game_proposals::ProposalInputs, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            let scans = s.kiosk_scans_full()?;
            let matches = s.game_matches()?;
            let devices = s
                .kiosk_scan_summaries()?
                .into_iter()
                .filter_map(|d| {
                    let scan = scans.iter().find(|r| r.device_id == d.device_id)?;
                    Some(crate::game_proposals::ScannedDevice {
                        entries: serde_json::from_str(&scan.entries).unwrap_or_default(),
                        // Matches made from an older scan do not describe this one.
                        matches: matches
                            .iter()
                            .find(|m| m.device_id == d.device_id && m.scanned_at == scan.scanned_at)
                            .map(|m| serde_json::from_str(&m.matches).unwrap_or_default())
                            .unwrap_or_default(),
                        device_id: d.device_id,
                        label: d.label,
                    })
                })
                .collect();
            Ok(crate::game_proposals::ProposalInputs {
                devices,
                library: library_views(s)?,
                dismissed: s.library_dismissals()?,
            })
        })
    }

    /// Every live entry of the branch game and app library, enabled or not, ordered by name
    /// (Story 6.3). The name, launcher and age rating of a linked entry are as stored; the API
    /// layer shows the catalogue's.
    pub fn library(&self) -> Result<Vec<arena_proto::LibraryEntryView>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(library_views)
    }

    /// What a coupon code would do to a bill, by the pricing a settle uses (Story 5.21).
    pub fn coupon_preview(
        &self,
        req: &arena_proto::CouponPreviewRequest,
        now_ms: i64,
    ) -> Result<arena_proto::CouponPreview, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_with(|conn| crate::apply::coupon_preview(conn, req, now_ms))
    }

    /// What a manual discount (after the coupon the request names) would do to a bill, by the
    /// pricing a settle uses (Story 5.22).
    pub fn discount_preview(
        &self,
        req: &arena_proto::DiscountPreviewRequest,
        now_ms: i64,
    ) -> Result<arena_proto::DiscountPreview, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_with(|conn| crate::apply::discount_preview(conn, req, now_ms))
    }

    /// The Settings → Staff list: the active and invited members with the `users` limit in
    /// effect (the licence's, else the Free default). Deactivated members are not listed.
    pub fn staff_list(&self) -> Result<arena_proto::StaffList, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let (rows, used, cap) = store.staff_overview()?;
        let limit = cap
            .and_then(|v| v.parse().ok())
            .unwrap_or(i64::from(arena_proto::Tier::Free.defaults().limits.users));
        Ok(arena_proto::StaffList {
            staff: rows
                .into_iter()
                .filter(|r| r.active)
                .map(|r| arena_proto::StaffView {
                    id: r.staff_id,
                    name: r.display_name,
                    login: r.login,
                    role: r.role_name,
                    status: if r.invited {
                        arena_proto::StaffStatus::Invited
                    } else {
                        arena_proto::StaffStatus::Active
                    },
                    rev: r.rev,
                })
                .collect(),
            used,
            limit,
        })
    }

    pub fn plans(&self) -> Result<Vec<arena_proto::PlanView>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| {
            Ok(s.plans()?
                .into_iter()
                .map(|(p, group_ids)| arena_proto::PlanView {
                    id: p.plan_id,
                    plan_kind: p.kind,
                    name: p.name,
                    price_minor: p.price_minor,
                    minutes: p.minutes,
                    validity_days: p.validity_days,
                    window_start_min: p.window_start_min,
                    window_end_min: p.window_end_min,
                    group_ids,
                    active: p.active,
                    rev: p.rev,
                })
                .collect())
        })
    }

    pub fn catalog(
        &self,
        ids: &[String],
        now_ms: i64,
    ) -> Result<Vec<arena_proto::CatalogEntry>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        store.read_consistent(|s| crate::catalog::entries(s, ids, now_ms))
    }

    pub fn setup(&self) -> Result<Option<SetupStatus>, AppError> {
        let store = self
            .reader
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal, "snapshot reader lock poisoned"))?;
        let mut setup = store.read_consistent(read_setup)?;
        if let Some(s) = setup.as_mut() {
            s.backup_location_set &= !self.backup_unavailable.load(Ordering::Relaxed);
        }
        Ok(setup)
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

/// The live library entries with their install paths (Story 6.3, 6.5).
fn library_views(s: &Store) -> Result<Vec<arena_proto::LibraryEntryView>, AppError> {
    let installs = s.library_installs()?;
    Ok(s.library_entries()?
        .into_iter()
        .map(|e| arena_proto::LibraryEntryView {
            installs: installs
                .iter()
                .filter(|i| i.entry_id == e.id)
                .map(|i| arena_proto::LibraryInstall {
                    device_id: i.device_id.clone(),
                    path: i.path.clone(),
                    match_key: i.match_key.clone(),
                })
                .collect(),
            id: e.id,
            kind: e.kind,
            name: e.name,
            launcher: e.launcher,
            executable_path: e.executable_path,
            launch_uri: e.launch_uri,
            launch_args: e.launch_args,
            category: e.category,
            tags: e.tags,
            age_rating: e.age_rating,
            enabled: e.enabled,
            catalog_id: e.catalog_id,
            rev: e.rev,
        })
        .collect())
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
                group_bills: vec![],
                shift: Some(arena_proto::ShiftState {
                    open: None,
                    carried_minor: 0,
                }),
                alerts: vec![],
                counter_tickets: vec![],
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
                    controller_check: None,
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
                    ad_rating: arena_proto::AdRating::PG,
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
                needs_repair: None,
                licence_unverified: None,
                clock_wrong: None,
                device_cap: Some(20),
                tier: None,
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
                group_bills: vec![],
                shift: Some(arena_proto::ShiftState {
                    open: None,
                    carried_minor: 0,
                }),
                alerts: vec![],
                counter_tickets: vec![],
                devices: vec![],
                locale: None,
                ui: SnapshotUi::default(),
                connectivity: arena_proto::Connectivity::default(),
                setup: None,
                lan: None,
                paired: true,
                pairing_revoked: None,
                needs_repair: None,
                licence_unverified: None,
                clock_wrong: None,
                device_cap: Some(20),
                tier: None,
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

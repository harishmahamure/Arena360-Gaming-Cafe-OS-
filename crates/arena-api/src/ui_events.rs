//! `GET /ui/events?token=<session token>`: the Hub pushes committed changes to every open Hub UI
//! view as server-sent events (loopback only, behind the same origin guard as `/ui/snapshot`).
//! The UI never polls.
//!
//! - `event: settings.changed`, data [`SettingsChangedEvent`]: an intent wrote `settings`, or a
//!   deferred tz / day-start / FY change came due at a business-day start; the data carries the
//!   locale now in effect and the keys involved.
//! - `event: applied`, data [`AppliedEvent`]: any other committed intent.
//! - `event: session.locked`, data `{}`, and `event: session.unlocked`, data
//!   [`UnlockOk`](arena_proto::UnlockOk): this stream's own session was locked or unlocked (from
//!   another window sharing the token), so every window locks and unlocks together.
//! - `event: bill.settled`, data [`BillSettledEvent`]: a `session.settle` committed (the receipt hook);
//!   the view reloads the snapshot, which no longer lists the bill.
//! - `event: order.placed`, data [`OrderPlacedEvent`](arena_proto::OrderPlacedEvent): an `order.place`
//!   committed; open bills and the floor reload.
//! - `event: order.line_changed`, data [`OrderLineEvent`](arena_proto::OrderLineEvent): an
//!   `order.line_served` or `order.line_cancel` committed (`cancelled` tells which); open bills and the
//!   floor reload. A cancel sends no `bill.reverted`.
//! - `event: order.voided`, data [`OrderVoidEvent`](arena_proto::OrderVoidEvent): an `order.void` committed;
//!   open bills and the floor reload.
//! - `event: wallet.changed`, data [`WalletChangedEvent`]: a wallet balance moved (a top-up, a credit spend, a
//!   wallet payment or an undo of either); open profiles and bills reload.
//! - `event: bill.reverted`, data [`BillRevertedEvent`]: a `session.end.reverted` or
//!   `session.settle.reverted` committed; the view reloads the snapshot, which lists the bill again
//!   as unpaid (a settle undone) or no longer (an end undone).
//! - `event: effect.recorded`, data [`EffectRecordedEvent`]: reserved for an irreversible effect (a
//!   printed receipt, a drawer kick, an invoice number) recorded against a bill, which closes its
//!   undo toast. Nothing records an effect before Epic 5, so the Hub does not send it yet.
//! - `event: devices.changed`, data `{}`: a device came online or went offline on the LAN, or
//!   the Hub's LAN addresses changed, an alert was raised or acknowledged, or a session's rate segment changed; the view reloads
//!   the snapshot (which carries each device's `online`, the `lan` block and the open alerts).
//! - `event: groups.changed`, data `{"by_owner": bool}`: a device group, floor zone or a device's
//!   group or place changed (`by_owner`: an owner setup command wrote it); sent with `devices.changed`.
//! - `event: device.admin_unlock_ack`, data
//!   [`AdminUnlockAckEvent`](arena_proto::AdminUnlockAckEvent): a kiosk granted a remote unlock
//!   (the floor waits up to 5 s for it).
//! - `event: connectivity.changed`, data [`Connectivity`](arena_proto::Connectivity): the WAN
//!   probe's state moved (`online`, `internet_offline`, `cloud_unreachable`); sent once per
//!   change of state, never for a probe that found the same state; also sent once, with the
//!   current value, as the first event of every newly opened stream (so a change between the
//!   snapshot load and the stream opening is not missed). The snapshot carries it too.
//! - `event: entitlement.changed`, data [`EntitlementChangedEvent`](arena_proto::EntitlementChangedEvent):
//!   the Hub's effective tier moved (a licence applied, expired or failed its check; `tier: null` when
//!   unpaired); sent once per change, never on open (the snapshot carries `tier`).
//! - `event: resync`, data `{}`: this stream fell behind and missed events, or the Hub's role
//!   changed (the stream then closes so the client reopens it). The view reloads the snapshot.
//!
//! One [`EventsHub`] task per Hub process turns each committed intent into its event once and
//! fans it out to every stream; it also wakes at the next business-day start while a locale
//! change waits. The token goes in the query string (an `EventSource` cannot send headers). An
//! unknown or ended token is `AUTH_FAILED` (401). Each stream checks its session in memory on
//! every wake (present, within 12 h), and re-reads the staff row when a session ended, when an
//! intent touched staff or roles, and at most every [`EventsConfig::full_recheck`]; once the
//! session is gone the stream closes.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{Applied, Clock, HubStatus, Role, SnapshotReader, WriterHandle};
use arena_proto::{
    AppliedEvent, BillRevertedEvent, BillSettledEvent, OrderLineEvent, OrderPlacedEvent,
    OrderVoidEvent, SessionStartedEvent, SettingsChangedEvent, SnapshotLocale, SnapshotUi,
    WalletChangedEvent,
};
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use serde::Deserialize;
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

use crate::ui_auth::{UiAuth, auth_failed, fail};

/// Keep-alive comment interval.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Timings and buffer of the `/ui/events` fan-out.
#[derive(Debug, Clone)]
pub struct EventsConfig {
    /// How often an idle stream checks its session in memory, and the longest the hub task
    /// sleeps before re-reading the clock while a locale change waits.
    pub tick: Duration,
    /// The longest a stream goes without re-reading its staff row.
    pub full_recheck: Duration,
    /// Events buffered per stream before it lags (and gets `resync`).
    pub capacity: usize,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(30),
            full_recheck: Duration::from_secs(30),
            capacity: 256,
        }
    }
}

/// One event, built once and shared by every stream.
#[derive(Debug, Clone)]
pub(crate) enum Pushed {
    Event {
        name: &'static str,
        data: Arc<str>,
        /// The intent wrote staff or roles: streams re-read their session.
        touches_staff: bool,
    },
    /// The Hub's role changed or its writer closed: `resync`, then close.
    Reset,
    /// For the streams of one session token only.
    Session {
        token: Arc<str>,
        name: &'static str,
        data: Arc<str>,
    },
}

/// The per-process fan-out (see the module docs).
#[derive(Debug)]
pub struct EventsHub {
    tx: broadcast::Sender<Pushed>,
}

impl EventsHub {
    fn spawn(auth: &UiAuth) -> Arc<Self> {
        let (tx, _) = broadcast::channel(auth.events_config().capacity.max(1));
        let task = HubTask {
            tx: tx.clone(),
            reader: Arc::clone(auth.reader()),
            clock: Arc::clone(auth.clock()),
            events: auth.writer().map(WriterHandle::subscribe),
            status: auth.status().cloned(),
            presence: auth.presence().changes(),
            acks: auth.presence().admin_unlock_acks(),
            pc_acks: auth.presence().pc_acks(),
            pairing: auth.pairing().map(|p| p.subscribe()),
            tick: auth.events_config().tick,
        };
        tokio::spawn(task.run());
        Arc::new(Self { tx })
    }

    /// Sends a `session.*` event to the streams of `token`.
    pub(crate) fn push_session(&self, token: &str, name: &'static str, data: String) {
        let _ = self.tx.send(Pushed::Session {
            token: token.into(),
            name,
            data: data.into(),
        });
    }

    fn subscribe(&self) -> broadcast::Receiver<Pushed> {
        self.tx.subscribe()
    }
}

struct HubTask {
    tx: broadcast::Sender<Pushed>,
    reader: Arc<SnapshotReader>,
    clock: Arc<dyn Clock>,
    events: Option<broadcast::Receiver<Applied>>,
    status: Option<watch::Receiver<HubStatus>>,
    presence: Option<watch::Receiver<u64>>,
    acks: Option<broadcast::Receiver<arena_proto::AdminUnlockAckEvent>>,
    pc_acks: Option<broadcast::Receiver<arena_proto::PcAckEvent>>,
    pairing: Option<tokio::sync::mpsc::Receiver<crate::pairing::CodeChanged>>,
    tick: Duration,
}

/// The keys a locale has waiting.
fn pending_keys(locale: Option<&SnapshotLocale>) -> Vec<String> {
    let Some(p) = locale.and_then(|l| l.pending.as_ref()) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    if p.tz.is_some() {
        keys.push("branch.tz".to_owned());
    }
    if p.day_start_min.is_some() {
        keys.push("branch.day_start_min".to_owned());
    }
    if p.fy_start_month.is_some() {
        keys.push("branch.fy_start_month".to_owned());
    }
    keys
}

/// The setting keys `applied` wrote (`settings` row ids are `branch_id/key`).
fn setting_keys(applied: &Applied) -> Vec<String> {
    applied
        .changes
        .iter()
        .filter(|c| c.tbl == "settings")
        .map(|c| {
            c.row_id
                .split_once('/')
                .map_or(c.row_id.clone(), |(_, k)| k.to_owned())
        })
        .collect()
}

fn session_started(applied: &Applied) -> Option<SessionStartedEvent> {
    let row = |tbl: &str| applied.changes.iter().find(|c| c.tbl == tbl);
    let session = row("sessions").filter(|c| c.op == arena_core::ChangeOp::Insert)?;
    Some(SessionStartedEvent {
        session_id: session.row_id.clone(),
        device_id: row("devices_meta")?.row_id.clone(),
    })
}

fn bill_settled(applied: &Applied) -> Option<BillSettledEvent> {
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "payments" && c.op == arena_core::ChangeOp::Insert)?;
    let order = applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == arena_core::ChangeOp::Update)?;
    Some(BillSettledEvent {
        order_id: order.row_id.clone(),
        collection: None,
    })
}

fn collected_order(applied: &Applied) -> Option<String> {
    let has = |tbl: &str, op: arena_core::ChangeOp| {
        applied.changes.iter().any(|c| c.tbl == tbl && c.op == op)
    };
    if !has("payments", arena_core::ChangeOp::Insert)
        || !has("credit_balances", arena_core::ChangeOp::Update)
    {
        return None;
    }
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == arena_core::ChangeOp::Insert)
        .map(|c| c.row_id.clone())
}

/// The order a commit inserted together with its lines and payments: a candidate walk-up sale
/// (the caller checks the order kind).
fn sold_order(applied: &Applied) -> Option<String> {
    let has = |tbl: &str| {
        applied
            .changes
            .iter()
            .any(|c| c.tbl == tbl && c.op == arena_core::ChangeOp::Insert)
    };
    if !has("payments") || !has("order_lines") {
        return None;
    }
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == arena_core::ChangeOp::Insert)
        .map(|c| c.row_id.clone())
}

/// Whether a commit inserted order lines on an order without a payment: a candidate placement
/// (the caller looks the intent id up as a placement).
fn placed_candidate(applied: &Applied) -> bool {
    let has = |tbl: &str| {
        applied
            .changes
            .iter()
            .any(|c| c.tbl == tbl && c.op == arena_core::ChangeOp::Insert)
    };
    has("order_lines") && !has("payments")
}

/// Whether a commit updated or deleted order lines (a candidate serve or cancel; the caller looks
/// the intent kind up).
fn line_candidate(applied: &Applied) -> Option<(String, bool)> {
    let line = applied.changes.iter().find(|c| {
        c.tbl == "order_lines"
            && matches!(
                c.op,
                arena_core::ChangeOp::Update | arena_core::ChangeOp::Delete
            )
    })?;
    Some((line.row_id.clone(), line.op == arena_core::ChangeOp::Delete))
}

/// The order a commit changed without touching its lines or payments (a candidate void; the
/// caller looks the intent kind up).
fn void_candidate(applied: &Applied) -> Option<String> {
    let touched = |tbl: &str| applied.changes.iter().any(|c| c.tbl == tbl);
    if touched("payments") || touched("order_lines") || touched("sessions") {
        return None;
    }
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "orders" && c.op == arena_core::ChangeOp::Update)
        .map(|c| c.row_id.clone())
}

fn wallet_changed(applied: &Applied) -> Option<WalletChangedEvent> {
    applied
        .changes
        .iter()
        .find(|c| c.tbl == "wallets" || c.tbl == "credit_balances")
        .map(|c| WalletChangedEvent {
            player_id: c.row_id.clone(),
        })
}

fn bill_reverted(applied: &Applied) -> Option<BillRevertedEvent> {
    let order = applied.changes.iter().find(|c| c.tbl == "orders")?;
    // An Undo of End that keeps the order (products were placed on it) or of one placement
    // deletes order lines and leaves the order standing.
    let reopened = order.op == arena_core::ChangeOp::Delete
        || applied.changes.iter().any(|c| {
            matches!(c.tbl.as_str(), "payments" | "order_lines")
                && c.op == arena_core::ChangeOp::Delete
        });
    reopened.then(|| BillRevertedEvent {
        order_id: order.row_id.clone(),
    })
}

fn role_of(status: &Option<watch::Receiver<HubStatus>>) -> Option<Role> {
    status.as_ref().map(|s| s.borrow().role)
}

fn lan_of(
    status: &Option<watch::Receiver<HubStatus>>,
) -> Option<(Vec<std::net::Ipv4Addr>, Option<String>)> {
    status.as_ref().map(|s| {
        let s = s.borrow();
        (s.lan_addresses.clone(), s.lan_network.clone())
    })
}

fn tier_of(status: &Option<watch::Receiver<HubStatus>>) -> Option<arena_proto::Tier> {
    status.as_ref().and_then(|s| s.borrow().tier)
}

fn connectivity_of(
    status: &Option<watch::Receiver<HubStatus>>,
) -> Option<arena_proto::Connectivity> {
    status.as_ref().map(|s| s.borrow().connectivity)
}

impl HubTask {
    /// The locale now, its next rollover and the Hub UI settings; `None` on a read failure
    /// (logged).
    async fn read(&self) -> Option<(Option<SnapshotLocale>, Option<i64>, SnapshotUi)> {
        let reader = Arc::clone(&self.reader);
        let now = self.clock.now_ms();
        let read = move || {
            // A failed UI-settings read never holds back the locale: the defaults go out.
            let ui = reader.ui().unwrap_or_else(|e| {
                tracing::warn!(code = e.code.as_str(), detail = %e.message, "ui events: ui settings read failed");
                SnapshotUi::default()
            });
            Ok::<_, arena_proto::AppError>((reader.locale_and_rollover(now)?, ui))
        };
        match tokio::task::spawn_blocking(read).await {
            Ok(Ok((Some((locale, at)), ui))) => Some((Some(locale), at, ui)),
            Ok(Ok((None, ui))) => Some((None, None, ui)),
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), detail = %e.message, "ui events: locale read failed");
                None
            }
            Err(e) => {
                tracing::warn!(detail = %e, "ui events: locale task failed");
                None
            }
        }
    }

    fn send(&self, name: &'static str, data: String, touches_staff: bool) {
        let _ = self.tx.send(Pushed::Event {
            name,
            data: data.into(),
            touches_staff,
        });
    }

    async fn run(mut self) {
        let mut role = role_of(&self.status);
        let mut link = connectivity_of(&self.status).map(|c| c.state);
        let mut tier = tier_of(&self.status);
        let mut lan = lan_of(&self.status);
        let (mut waiting, mut rollover) = match self.read().await {
            Some((locale, at, _)) => (pending_keys(locale.as_ref()), at),
            None => (Vec::new(), None),
        };
        loop {
            let sleep = rollover.map(|at| {
                let left = u64::try_from(at - self.clock.now_ms()).unwrap_or(0);
                Duration::from_millis(left).min(self.tick)
            });
            tokio::select! {
                r = async {
                    match &mut self.events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match r {
                    Ok(applied) => {
                        let keys = setting_keys(&applied);
                        let touches_staff = applied
                            .changes
                            .iter()
                            .any(|c| matches!(c.tbl.as_str(), "staff" | "roles" | "role_perms"));
                        if applied
                            .changes
                            .iter()
                            .any(|c| matches!(c.tbl.as_str(), "devices" | "devices_meta" | "alerts" | "session_segments"))
                        {
                            self.send("devices.changed", "{}".into(), false);
                        }
                        if let Some(settled) = bill_settled(&applied) {
                            let data = serde_json::to_string(&settled).unwrap_or_default();
                            self.send("bill.settled", data, false);
                        }
                        if let Some(order_id) = collected_order(&applied) {
                            let (reader, id) = (Arc::clone(&self.reader), order_id.clone());
                            let collection = tokio::task::spawn_blocking(move || reader.credit_collection(&id))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .flatten();
                            let settled = BillSettledEvent { order_id, collection };
                            let data = serde_json::to_string(&settled).unwrap_or_default();
                            self.send("bill.settled", data, false);
                        } else if let Some(order_id) = sold_order(&applied) {
                            let (reader, id) = (Arc::clone(&self.reader), order_id.clone());
                            let walk_up = tokio::task::spawn_blocking(move || reader.is_pos_sale(&id))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .unwrap_or(false);
                            if walk_up {
                                let settled = BillSettledEvent { order_id, collection: None };
                                let data = serde_json::to_string(&settled).unwrap_or_default();
                                self.send("bill.settled", data, false);
                            }
                        }
                        if placed_candidate(&applied) {
                            let (reader, id) = (Arc::clone(&self.reader), applied.intent_id.clone());
                            let placed = tokio::task::spawn_blocking(move || reader.placed_order(&id))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .flatten();
                            if let Some((session_id, order_id)) = placed {
                                let event = OrderPlacedEvent { session_id, order_id };
                                let data = serde_json::to_string(&event).unwrap_or_default();
                                self.send("order.placed", data, false);
                            }
                        }
                        // Serving or cancelling a line changes the bill's lines but is no Undo.
                        let line_event = match line_candidate(&applied) {
                            Some((line_id, deleted)) => {
                                let (reader, id) = (Arc::clone(&self.reader), applied.intent_id.clone());
                                let kind = tokio::task::spawn_blocking(move || reader.intent_kind(&id))
                                    .await
                                    .ok()
                                    .and_then(Result::ok)
                                    .flatten();
                                matches!(kind.as_deref(), Some("order.line_served" | "order.line_cancel"))
                                    .then_some(OrderLineEvent { line_id, cancelled: deleted })
                            }
                            None => None,
                        };
                        if let Some(event) = &line_event {
                            let data = serde_json::to_string(event).unwrap_or_default();
                            self.send("order.line_changed", data, false);
                        } else if let Some(reverted) = bill_reverted(&applied) {
                            let data = serde_json::to_string(&reverted).unwrap_or_default();
                            self.send("bill.reverted", data, false);
                        }
                        if let Some(order_id) = void_candidate(&applied) {
                            let (reader, id) = (Arc::clone(&self.reader), applied.intent_id.clone());
                            let kind = tokio::task::spawn_blocking(move || reader.intent_kind(&id))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .flatten();
                            if kind.as_deref() == Some("order.void") {
                                let data = serde_json::to_string(&OrderVoidEvent { order_id })
                                    .unwrap_or_default();
                                self.send("order.voided", data, false);
                            }
                        }
                        if let Some(changed) = wallet_changed(&applied) {
                            let data = serde_json::to_string(&changed).unwrap_or_default();
                            self.send("wallet.changed", data, false);
                        }
                        if let Some(started) = session_started(&applied) {
                            let data = serde_json::to_string(&started).unwrap_or_default();
                            self.send("session.started", data, false);
                        }
                        if let Some(c) = applied
                            .changes
                            .iter()
                            .find(|c| matches!(c.tbl.as_str(), "device_groups" | "floor_zones" | "devices_meta" | "rates" | "pricing_rules"))
                        {
                            let (reader, epoch, seq) =
                                (Arc::clone(&self.reader), c.epoch.clone(), c.seq);
                            let owner = tokio::task::spawn_blocking(move || {
                                reader.owner_wrote(&epoch, seq)
                            })
                            .await
                            .unwrap_or(false);
                            self.send("groups.changed", format!(r#"{{"by_owner":{owner}}}"#), false);
                        }
                        if applied
                            .changes
                            .iter()
                            .any(|c| {
                                matches!(
                                    c.tbl.as_str(),
                                    "library_entries"
                                        | "kiosk_scans"
                                        | "library_installs"
                                        | "game_matches"
                                        | "library_dismissals"
                                )
                            })
                        {
                            self.send("library.changed", "{}".into(), false);
                        }
                        let products = crate::lan::registry::product_ids(&applied);
                        if !products.is_empty() {
                            let reader = Arc::clone(&self.reader);
                            let now_ms = self.clock.now_ms();
                            let entries = tokio::task::spawn_blocking(move || {
                                reader.catalog(&products, now_ms)
                            })
                            .await;
                            match entries {
                                Ok(Ok(products)) => {
                                    let data = serde_json::json!({"products": products}).to_string();
                                    self.send("catalog.changed", data, false);
                                }
                                _ => self.send("resync", "{}".into(), false),
                            }
                        }
                        if keys.is_empty() {
                            let data = serde_json::to_string(&AppliedEvent {
                                intent_id: applied.intent_id.clone(),
                            })
                            .unwrap_or_default();
                            self.send("applied", data, touches_staff);
                            continue;
                        }
                        match self.read().await {
                            Some((locale, at, ui)) => {
                                waiting = pending_keys(locale.as_ref());
                                rollover = at;
                                let data = serde_json::to_string(&SettingsChangedEvent { locale, keys, ui })
                                    .unwrap_or_default();
                                self.send("settings.changed", data, touches_staff);
                            }
                            None => self.send("resync", "{}".into(), touches_staff),
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "ui events: the fan-out fell behind the writer");
                        self.send("resync", "{}".into(), true);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Demoted or shutting down: clients reopen and find out.
                        self.events = None;
                        let _ = self.tx.send(Pushed::Reset);
                    }
                },
                changed = async {
                    match &mut self.presence {
                        Some(rx) => rx.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => {
                    if changed {
                        self.send("devices.changed", "{}".into(), false);
                    } else {
                        self.presence = None;
                    }
                },
                changed = async {
                    match &mut self.pairing {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match changed {
                    Some(change) => {
                        let data = change.to_json();
                        let _ = self.tx.send(Pushed::Session {
                            token: change.owner,
                            name: "pairing.code_changed",
                            data: data.into(),
                        });
                    }
                    None => self.pairing = None,
                },
                ack = async {
                    match &mut self.acks {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match ack {
                    Ok(ack) => {
                        let data = serde_json::to_string(&ack).unwrap_or_default();
                        self.send("device.admin_unlock_ack", data, false);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => self.acks = None,
                },
                ack = async {
                    match &mut self.pc_acks {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match ack {
                    Ok(ack) => {
                        let data = serde_json::to_string(&ack).unwrap_or_default();
                        self.send("device.pc_ack", data, false);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => self.pc_acks = None,
                },
                changed = async {
                    match &mut self.status {
                        Some(rx) => rx.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => {
                    if !changed {
                        self.status = None;
                        continue;
                    }
                    if let Some(c) = connectivity_of(&self.status)
                        && Some(c.state) != link
                    {
                        link = Some(c.state);
                        let data = serde_json::to_string(&c).unwrap_or_default();
                        self.send("connectivity.changed", data, false);
                    }
                    let now_lan = lan_of(&self.status);
                    if now_lan != lan {
                        lan = now_lan;
                        self.send("devices.changed", "{}".into(), false);
                    }
                    let now_tier = tier_of(&self.status);
                    if now_tier != tier {
                        tier = now_tier;
                        let data = serde_json::to_string(&arena_proto::EntitlementChangedEvent { tier })
                            .unwrap_or_default();
                        self.send("entitlement.changed", data, false);
                    }
                    let now = role_of(&self.status);
                    if now != role {
                        role = now;
                        let _ = self.tx.send(Pushed::Reset);
                    }
                },
                _ = async {
                    match sleep {
                        Some(d) => tokio::time::sleep(d).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if rollover.is_some_and(|at| self.clock.now_ms() >= at)
                        && let Some((locale, at, ui)) = self.read().await
                    {
                        let now_waiting = pending_keys(locale.as_ref());
                        let keys: Vec<String> =
                            waiting.iter().filter(|k| !now_waiting.contains(k)).cloned().collect();
                        waiting = now_waiting;
                        rollover = at;
                        let data =
                            serde_json::to_string(&SettingsChangedEvent { locale, keys, ui }).unwrap_or_default();
                        self.send("settings.changed", data, false);
                    }
                },
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventsQuery {
    token: Option<String>,
}

struct OpenStream {
    auth: Arc<UiAuth>,
    token: String,
}

impl OpenStream {
    fn new(auth: &Arc<UiAuth>, token: &str) -> Self {
        auth.stream_opened(token);
        Self {
            auth: Arc::clone(auth),
            token: token.to_owned(),
        }
    }
}

impl Drop for OpenStream {
    fn drop(&mut self) {
        self.auth.stream_closed(&self.token);
    }
}

struct Stream {
    _open: OpenStream,
    auth: Arc<UiAuth>,
    token: String,
    events: broadcast::Receiver<Pushed>,
    ends: Option<watch::Receiver<u64>>,
    recheck: tokio::time::Interval,
    last_full: Instant,
    closing: bool,
    /// The connectivity now, sent first so a view never misses a change made between its
    /// snapshot load and the stream opening.
    opening: Option<Event>,
}

enum Wake {
    Pushed(Result<Pushed, broadcast::error::RecvError>),
    SessionEnded(bool),
    Recheck,
}

fn resync() -> Event {
    Event::default().event("resync").data("{}")
}

impl Stream {
    /// Whether the session still stands: in memory always, the staff row when `full` or when
    /// the last full check is older than `full_recheck`.
    async fn alive(&mut self, full: bool) -> bool {
        if !self.auth.session_live(&self.token) {
            return false;
        }
        if full || self.last_full.elapsed() >= self.auth.events_config().full_recheck {
            self.last_full = Instant::now();
            return self.auth.session_for(&self.token).await.is_ok();
        }
        true
    }
}

/// `GET /ui/events?token=`.
pub(crate) async fn events(
    State(auth): State<Arc<UiAuth>>,
    Query(q): Query<EventsQuery>,
) -> Response {
    let Some(token) = q.token.filter(|t| !t.is_empty()) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.session_for(&token).await {
        return fail(e);
    }
    let hub = Arc::clone(auth.events_hub().get_or_init(|| EventsHub::spawn(&auth)));
    let mut recheck = tokio::time::interval(auth.events_config().tick);
    recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // An interval's first tick fires at once: start the stream idle instead.
    recheck.reset();
    let opening = auth.status().map(|s| {
        let c = s.borrow().connectivity;
        Event::default()
            .event("connectivity.changed")
            .data(serde_json::to_string(&c).unwrap_or_default())
    });
    let state = Stream {
        _open: OpenStream::new(&auth, &token),
        events: hub.subscribe(),
        ends: Some(auth.session_ends()),
        auth,
        token,
        recheck,
        last_full: Instant::now(),
        closing: false,
        opening,
    };
    let body = stream::unfold(state, |mut st| async move {
        loop {
            if st.closing {
                return None;
            }
            if let Some(event) = st.opening.take() {
                return Some((Ok::<_, Infallible>(event), st));
            }
            let wake = tokio::select! {
                r = st.events.recv() => Wake::Pushed(r),
                alive = async {
                    match &mut st.ends {
                        Some(rx) => rx.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => Wake::SessionEnded(alive),
                _ = st.recheck.tick() => Wake::Recheck,
            };
            let full = match &wake {
                Wake::SessionEnded(_) => true,
                Wake::Pushed(Ok(Pushed::Event { touches_staff, .. })) => *touches_staff,
                _ => false,
            };
            if !st.alive(full).await {
                return None;
            }
            match wake {
                Wake::Pushed(Ok(Pushed::Event { name, data, .. })) => {
                    let event = Event::default().event(name).data(&*data);
                    return Some((Ok::<_, Infallible>(event), st));
                }
                Wake::Pushed(Ok(Pushed::Session { token, name, data })) => {
                    if *token == *st.token {
                        let event = Event::default().event(name).data(&*data);
                        return Some((Ok(event), st));
                    }
                }
                Wake::Pushed(Ok(Pushed::Reset)) => {
                    st.closing = true;
                    return Some((Ok(resync()), st));
                }
                Wake::Pushed(Err(broadcast::error::RecvError::Lagged(n))) => {
                    tracing::warn!(missed = n, "ui events: a stream fell behind");
                    return Some((Ok(resync()), st));
                }
                Wake::Pushed(Err(broadcast::error::RecvError::Closed)) => return None,
                Wake::SessionEnded(false) => st.ends = None,
                Wake::SessionEnded(true) | Wake::Recheck => {}
            }
        }
    });
    Sse::new(body)
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_auth::SESSION_TTL_MS;
    use crate::ui_auth::tests::{Hub, call, footer_set, hub, hub_with, intent_as, token_for};
    use arena_proto::{Actor, ActorKind, IntentPayload, SettingsSet};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use futures_util::StreamExt;
    use serde_json::Value;

    fn applied(changes: &[(&str, &str, arena_core::ChangeOp)]) -> Applied {
        Applied {
            intent_id: "I".into(),
            changes: changes
                .iter()
                .enumerate()
                .map(|(i, (tbl, row_id, op))| arena_core::ChangeMeta {
                    epoch: "1:H".into(),
                    seq: i as i64 + 1,
                    tbl: (*tbl).into(),
                    row_id: (*row_id).into(),
                    op: *op,
                })
                .collect(),
        }
    }

    #[test]
    fn a_revert_is_told_apart_from_a_settle() {
        use arena_core::ChangeOp::{Delete, Insert, Update};
        let settle = applied(&[("payments", "P", Insert), ("orders", "O", Update)]);
        assert!(bill_settled(&settle).is_some());
        assert!(bill_reverted(&settle).is_none());
        let unsettle = applied(&[("payments", "P", Delete), ("orders", "O", Update)]);
        assert!(bill_settled(&unsettle).is_none());
        assert_eq!(bill_reverted(&unsettle).unwrap().order_id, "O");
        let unend = applied(&[
            ("sessions", "S", Update),
            ("order_lines", "L", Delete),
            ("orders", "O", Delete),
        ]);
        assert_eq!(bill_reverted(&unend).unwrap().order_id, "O");
        let end = applied(&[("sessions", "S", Update), ("orders", "O", Insert)]);
        assert!(bill_reverted(&end).is_none());
    }

    #[test]
    fn a_served_or_cancelled_line_is_a_line_candidate_and_a_placement_is_not() {
        use arena_core::ChangeOp::{Delete, Insert, Update};
        let served = applied(&[
            ("stock", "P", Update),
            ("stock_moves", "M", Insert),
            ("stock_holds", "H", Update),
            ("order_lines", "L", Update),
        ]);
        assert_eq!(line_candidate(&served), Some(("L".into(), false)));
        let cancelled = applied(&[
            ("stock_holds", "H", Update),
            ("order_lines", "L", Delete),
            ("orders", "O", Update),
        ]);
        assert_eq!(line_candidate(&cancelled), Some(("L".into(), true)));
        let place = applied(&[
            ("orders", "O", Insert),
            ("order_lines", "L", Insert),
            ("stock_holds", "H", Insert),
        ]);
        assert!(line_candidate(&place).is_none());
    }

    #[test]
    fn a_commit_that_only_updates_an_order_is_a_void_candidate() {
        use arena_core::ChangeOp::{Delete, Insert, Update};
        let void = applied(&[
            ("stock_holds", "H", Update),
            ("stock", "P", Update),
            ("stock_moves", "M", Insert),
            ("orders", "O", Update),
        ]);
        assert_eq!(void_candidate(&void), Some("O".into()));
        let cancelled = applied(&[("order_lines", "L", Delete), ("orders", "O", Update)]);
        assert!(void_candidate(&cancelled).is_none());
        let settled = applied(&[("payments", "Y", Insert), ("orders", "O", Update)]);
        assert!(void_candidate(&settled).is_none());
        let ended = applied(&[("sessions", "S", Update), ("orders", "O", Update)]);
        assert!(void_candidate(&ended).is_none());
    }

    #[test]
    fn an_undo_that_keeps_the_order_is_a_revert_and_a_placement_is_a_candidate() {
        use arena_core::ChangeOp::{Delete, Insert, Update};
        // Undo of End on an order that holds placed products: the order stays, its time lines go.
        let unend = applied(&[
            ("sessions", "S", Update),
            ("order_lines", "L", Delete),
            ("orders", "O", Update),
        ]);
        assert_eq!(bill_reverted(&unend).unwrap().order_id, "O");
        // Undo of one placement.
        let unplace = applied(&[
            ("stock_moves", "M", Insert),
            ("order_lines", "L", Delete),
            ("orders", "O", Update),
        ]);
        assert_eq!(bill_reverted(&unplace).unwrap().order_id, "O");
        // A placement and an End insert order lines without a payment; a sale has one.
        let place = applied(&[("orders", "O", Insert), ("order_lines", "L", Insert)]);
        assert!(placed_candidate(&place));
        assert!(bill_reverted(&place).is_none() && bill_settled(&place).is_none());
        let sale = applied(&[
            ("orders", "O", Insert),
            ("order_lines", "L", Insert),
            ("payments", "P", Insert),
        ]);
        assert!(!placed_candidate(&sale));
    }

    #[test]
    fn a_credit_collection_names_its_order_and_is_not_a_settle_or_a_top_up() {
        use arena_core::ChangeOp::{Delete, Insert, Update};
        let collect = applied(&[
            ("orders", "CO", Insert),
            ("payments", "P", Insert),
            ("wallet_txns", "T", Insert),
            ("credit_balances", "P2", Update),
        ]);
        assert_eq!(collected_order(&collect).as_deref(), Some("CO"));
        assert!(bill_settled(&collect).is_none());
        assert_eq!(wallet_changed(&collect).unwrap().player_id, "P2");
        let topup = applied(&[
            ("orders", "TO", Insert),
            ("payments", "P", Insert),
            ("wallets", "P1", Update),
        ]);
        assert!(collected_order(&topup).is_none());
        let settle = applied(&[
            ("payments", "P", Insert),
            ("orders", "O", Update),
            ("credit_balances", "P2", Update),
        ]);
        assert!(collected_order(&settle).is_none());
        let undo = applied(&[
            ("payments", "P", Delete),
            ("orders", "CO", Delete),
            ("credit_balances", "P2", Update),
        ]);
        assert!(collected_order(&undo).is_none());
        assert_eq!(bill_reverted(&undo).unwrap().order_id, "CO");
    }

    #[test]
    fn a_wallet_change_names_the_player_and_a_top_up_is_not_a_settle() {
        use arena_core::ChangeOp::{Insert, Update};
        let topup = applied(&[
            ("orders", "TO", Insert),
            ("payments", "P", Insert),
            ("wallets", "P1", Update),
            ("wallet_txns", "T", Insert),
        ]);
        assert_eq!(wallet_changed(&topup).unwrap().player_id, "P1");
        assert!(bill_settled(&topup).is_none());
        let credit = applied(&[
            ("orders", "O", Update),
            ("wallet_txns", "T", Insert),
            ("credit_balances", "P2", Update),
        ]);
        assert_eq!(wallet_changed(&credit).unwrap().player_id, "P2");
        let other = applied(&[("orders", "O", Update)]);
        assert!(wallet_changed(&other).is_none());
    }
    use tower::ServiceExt;

    type Frames = std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Send>,
    >;

    /// Opens a stream and, when it opened, reads its first event: the current connectivity.
    async fn open(hub: &Hub, token: Option<&str>) -> (StatusCode, Frames) {
        let (status, mut frames) = open_raw(hub, token).await;
        if status == StatusCode::OK {
            let (event, _) = next_event(&mut frames).await.unwrap();
            assert_eq!(event, "connectivity.changed");
        }
        (status, frames)
    }

    async fn open_raw(hub: &Hub, token: Option<&str>) -> (StatusCode, Frames) {
        let uri = match token {
            Some(t) => format!("/ui/events?token={t}"),
            None => "/ui/events".to_owned(),
        };
        let req = Request::get(uri)
            .header(header::HOST, "127.0.0.1:7409")
            .body(Body::empty())
            .unwrap();
        let res = hub.app.clone().oneshot(req).await.unwrap();
        (res.status(), Box::pin(res.into_body().into_data_stream()))
    }

    /// The next event (skipping keep-alive comments): `(event, data)`, or `None` once the
    /// stream has ended.
    async fn next_event(frames: &mut Frames) -> Option<(String, Value)> {
        let mut buf = String::new();
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(5), frames.next())
                .await
                .expect("an event within 5 s")?
                .unwrap();
            buf.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = buf.find("\n\n") {
                let block: String = buf.drain(..end + 2).collect();
                let mut event = None;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(e) = line.strip_prefix("event: ") {
                        event = Some(e.to_owned());
                    } else if let Some(d) = line.strip_prefix("data: ") {
                        data.push_str(d);
                    }
                }
                if let Some(event) = event {
                    return Some((event, serde_json::from_str(&data).unwrap()));
                }
            }
        }
    }

    fn manager() -> Actor {
        Actor {
            kind: ActorKind::Staff,
            id: "ignored".into(),
        }
    }

    fn currency(value: &str, rev: i64) -> Value {
        serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "branch.currency".into(),
                value: value.into(),
            }),
            Some(rev),
        ) })
    }

    #[tokio::test]
    async fn a_bad_or_missing_token_is_401() {
        let hub = hub().await;
        for token in [None, Some(""), Some("nope")] {
            let (status, _) = open(&hub, token).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
        }
    }

    #[tokio::test]
    async fn settings_changes_push_the_new_locale_and_the_keys() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&token),
            Some(currency("USD", 1)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "settings.changed");
        assert_eq!(data["keys"], serde_json::json!(["branch.currency"]));
        assert_eq!(data["locale"]["currency"], "USD");
        assert_eq!(data["locale"]["symbol"], "$");
        assert_eq!(data["locale"]["exponent"], 2);
        assert_eq!(data["locale"]["tz"], "Asia/Kolkata");
        assert_eq!(data["locale"]["pending"], Value::Null);

        // Any other intent is `applied` with the intent id only.
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let other = intent_as(
            owner,
            IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                staff_id: Some(hub.counter_id.clone()),
                login: "counter".into(),
                display_name: "Till".into(),
                role_id: hub.counter_role.clone(),
                password_hash: None,
                pin_hash: None,
                active: true,
                setup_code_hash: None,
                setup_expires_at: None,
            }),
            Some(1),
        );
        let id = other.intent_id.clone();
        hub.writer.submit_intent(other).await.unwrap();
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "applied");
        assert_eq!(data, serde_json::json!({ "intent_id": id }));
    }

    #[tokio::test]
    async fn a_device_coming_online_or_going_offline_is_pushed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        // Give the stream's hub task a moment to subscribe.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some("D1".into()), tx);
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(
            (event.as_str(), data),
            ("devices.changed", serde_json::json!({}))
        );
        drop(session);
        let (event, _) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "devices.changed");
    }

    #[tokio::test]
    async fn a_used_pairing_code_is_renewed_on_the_panels_own_stream_only() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let other = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&manager)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, mut others) = open(&hub, Some(&other)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (status, body) = call(&hub, "POST", "/ui/pairing-code", Some(&manager), None).await;
        assert_eq!(status, StatusCode::OK);
        let first = body["code"].as_str().unwrap().to_owned();
        hub.pairing.consume(&first).unwrap();
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "pairing.code_changed");
        let renewed = data["code"].as_str().unwrap().to_owned();
        assert!(arena_proto::is_pair_code(&renewed));
        hub.clock.advance(arena_proto::PAIR_CODE_TTL_MS);
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "pairing.code_changed");
        assert_eq!(
            data["expires_at"].as_i64().unwrap(),
            hub.clock.now_ms() + arena_proto::PAIR_CODE_TTL_MS
        );
        assert!(hub.pairing.consume(&renewed).is_err());
        let quiet = tokio::time::timeout(Duration::from_millis(200), others.next()).await;
        if let Ok(Some(Ok(chunk))) = quiet {
            assert!(
                !std::str::from_utf8(&chunk)
                    .unwrap()
                    .contains("pairing.code_changed")
            );
        }
    }

    #[tokio::test]
    async fn a_code_is_not_renewed_once_the_panels_stream_is_gone() {
        let hub = hub().await;
        let manager = token_for(&hub, "manager", "m-pass").await;
        let (_, frames) = open(&hub, Some(&manager)).await;
        let (_, body) = call(&hub, "POST", "/ui/pairing-code", Some(&manager), None).await;
        let first = body["code"].as_str().unwrap().to_owned();
        drop(frames);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while hub.auth.has_stream(&manager) {
            assert!(
                std::time::Instant::now() < deadline,
                "the stream never closed"
            );
            tokio::task::yield_now().await;
        }
        hub.pairing.consume(&first).unwrap();
        assert!(!hub.pairing.has_live_code());
    }

    #[tokio::test]
    async fn a_second_managers_code_tells_the_first_panel_it_was_replaced() {
        let hub = hub().await;
        let first = token_for(&hub, "manager", "m-pass").await;
        let second = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&first)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        call(&hub, "POST", "/ui/pairing-code", Some(&first), None).await;
        let (status, _) = call(&hub, "POST", "/ui/pairing-code", Some(&second), None).await;
        assert_eq!(status, StatusCode::OK);
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "pairing.code_changed");
        assert_eq!(data, serde_json::json!({ "replaced": true }));
    }

    #[tokio::test]
    async fn a_registered_device_pushes_devices_changed_without_any_lan_session() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let register = intent_as(
            Actor {
                kind: ActorKind::Owner,
                id: "owner".into(),
            },
            IntentPayload::DeviceRegister(arena_proto::DeviceRegister::new(
                arena_proto::ClientClass::Paired,
                "PC-01",
                arena_core::devices::hash_token_secret("s").unwrap(),
            )),
            None,
        );
        hub.writer.submit_intent(register).await.unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, _) = next_event(&mut frames).await.unwrap();
            if event == "devices.changed" {
                break;
            }
            seen.push(event);
            assert!(seen.len() < 5, "{seen:?}");
        }
        assert_eq!(hub.lan.len(), 0);
    }

    #[tokio::test]
    async fn a_session_start_pushes_session_started_with_devices_changed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let submit = |payload| hub.writer.submit_intent(intent_as(owner(), payload, None));
        let device = submit(IntentPayload::DeviceRegister(
            arena_proto::DeviceRegister::new(
                arena_proto::ClientClass::Paired,
                "PC-01",
                arena_core::devices::hash_token_secret("s").unwrap(),
            ),
        ))
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone();
        let group = submit(IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }))
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone();
        submit(IntentPayload::RateUpsert(arena_proto::RateUpsert {
            group_id: group.clone(),
            price_per_hour_minor: 12_000,
        }))
        .await
        .unwrap();
        submit(IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
            device_id: device.clone(),
            group_id: Some(group),
        }))
        .await
        .unwrap();
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let applied = submit(IntentPayload::SessionStart(arena_proto::SessionStart {
            group_with: None,
            device_id: device.clone(),
            mode: arena_proto::SessionMode::Postpaid,
            player_id: None,
            pack_id: None,
            plan_id: None,
        }))
        .await
        .unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "session.started" {
                assert_eq!(
                    data,
                    serde_json::json!({
                        "session_id": applied.changes[0].row_id,
                        "device_id": device
                    })
                );
                break;
            }
            seen.push(event);
            assert!(seen.len() < 6, "{seen:?}");
        }
        assert!(seen.contains(&"devices.changed".to_owned()), "{seen:?}");
    }

    #[tokio::test]
    async fn an_alert_pushes_devices_changed_and_the_ui_cannot_raise_one() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let device = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister::new(
                    arena_proto::ClientClass::Paired,
                    "PC-01",
                    arena_core::devices::hash_token_secret("s").unwrap(),
                )),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let raise = IntentPayload::AlertRaise(arena_proto::AlertRaise::new(
            arena_proto::AlertKind::Fixture,
            Some(device.clone()),
        ));
        let body = serde_json::json!({ "intent": intent_as(manager(), raise.clone(), None) });
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(
            (status, reply["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("PERMISSION_DENIED"))
        );
        let system = Actor {
            kind: ActorKind::System,
            id: "hub".into(),
        };
        let applied = hub
            .writer
            .submit_intent(intent_as(system, raise, None))
            .await
            .unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, _) = next_event(&mut frames).await.unwrap();
            if event == "devices.changed" {
                break;
            }
            seen.push(event);
            assert!(seen.len() < 5, "{seen:?}");
        }
        let body = serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::AlertAck(arena_proto::AlertAck {
                alert_id: applied.changes[0].row_id.clone(),
            }),
            None,
        ) });
        let (status, reply) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        loop {
            let (event, _) = next_event(&mut frames).await.unwrap();
            if event == "devices.changed" {
                break;
            }
        }
    }

    #[tokio::test]
    async fn a_settle_pushes_bill_settled_with_the_order() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let submit = |payload, rev| hub.writer.submit_intent(intent_as(owner(), payload, rev));
        let device = submit(
            IntentPayload::DeviceRegister(arena_proto::DeviceRegister::new(
                arena_proto::ClientClass::Paired,
                "PC-01",
                arena_core::devices::hash_token_secret("s").unwrap(),
            )),
            None,
        )
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone();
        let group = submit(
            IntentPayload::GroupUpsert(arena_proto::GroupUpsert {
                id: None,
                name: "PCs".into(),
                group_kind: "pc".into(),
                colour: "teal".into(),
                sort_order: 1,
            }),
            None,
        )
        .await
        .unwrap()
        .changes[0]
            .row_id
            .clone();
        submit(
            IntentPayload::RateUpsert(arena_proto::RateUpsert {
                group_id: group.clone(),
                price_per_hour_minor: 12_000,
            }),
            None,
        )
        .await
        .unwrap();
        submit(
            IntentPayload::DeviceAssign(arena_proto::DeviceAssign {
                device_id: device.clone(),
                group_id: Some(group),
            }),
            None,
        )
        .await
        .unwrap();
        let started = submit(
            IntentPayload::SessionStart(arena_proto::SessionStart {
                group_with: None,
                device_id: device,
                mode: arena_proto::SessionMode::Postpaid,
                player_id: None,
                pack_id: None,
                plan_id: None,
            }),
            None,
        )
        .await
        .unwrap();
        let ended = submit(
            IntentPayload::SessionEnd(arena_proto::SessionEnd {
                session_id: started.changes[0].row_id.clone(),
            }),
            Some(1),
        )
        .await
        .unwrap();
        let order = ended
            .changes
            .iter()
            .find(|c| c.tbl == "orders")
            .unwrap()
            .row_id
            .clone();
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        submit(
            IntentPayload::SessionSettle(arena_proto::SessionSettle {
                business: None,
                coupon: None,
                discount: None,
                order_id: order.clone(),
                credit_override: None,
                payments: vec![arena_proto::PaymentRow {
                    method: arena_proto::PaymentMethod::Cash,
                    amount_minor: 6_000,
                    reference: None,
                }],
                receipt: None,
            }),
            Some(1),
        )
        .await
        .unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "bill.settled" {
                assert_eq!(data, serde_json::json!({"order_id": order}));
                break;
            }
            seen.push(event);
            assert!(seen.len() < 6, "{seen:?}");
        }
    }

    #[tokio::test]
    async fn a_product_change_pushes_catalog_changed_with_availability() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let product = |name: &str, track_stock: bool| {
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id: None,
                name: name.into(),
                category: String::new(),
                price_minor: 100,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active: true,
                bundle: false,
                components: Vec::new(),
            })
        };
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let tracked = hub
            .writer
            .submit_intent(intent_as(owner(), product("Cola", true), None))
            .await
            .unwrap();
        let free = hub
            .writer
            .submit_intent(intent_as(owner(), product("Chips", false), None))
            .await
            .unwrap();
        let mut seen = Vec::new();
        while seen.len() < 2 {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "catalog.changed" {
                seen.push(data);
            }
        }
        assert_eq!(
            seen[0],
            serde_json::json!({"products": [{"product_id": tracked.changes[0].row_id, "available": false, "reason": "out_of_stock"}]})
        );
        assert_eq!(
            seen[1],
            serde_json::json!({"products": [{"product_id": free.changes[0].row_id, "available": true}]})
        );
    }

    #[tokio::test]
    async fn a_library_change_pushes_library_changed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let entry = IntentPayload::LibraryUpsert(arena_proto::LibraryUpsert {
            id: None,
            entry_kind: "game".into(),
            name: "CS2".into(),
            launcher: "steam".into(),
            executable_path: r"C:\Games\cs2.exe".into(),
            launch_uri: String::new(),
            launch_args: String::new(),
            category: String::new(),
            tags: Vec::new(),
            age_rating: String::new(),
            enabled: true,
            catalog_id: None,
            installs: Vec::new(),
        });
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        hub.writer
            .submit_intent(intent_as(owner, entry, None))
            .await
            .unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "library.changed" {
                assert_eq!(data, serde_json::json!({}));
                break;
            }
            seen.push(event);
            assert!(seen.len() < 6, "{seen:?}");
        }
    }

    #[tokio::test]
    async fn a_kiosk_scan_pushes_library_changed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let device = hub
            .writer
            .submit_intent(intent_as(
                Actor {
                    kind: ActorKind::Owner,
                    id: "owner".into(),
                },
                IntentPayload::DeviceRegister(arena_proto::DeviceRegister::new(
                    arena_proto::ClientClass::Paired,
                    "PC-01",
                    arena_core::devices::hash_token_secret("s").unwrap(),
                )),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let scan = intent_as(
            Actor {
                kind: ActorKind::Device,
                id: device,
            },
            IntentPayload::KioskScanReported(arena_proto::KioskScanReported {
                scanned_at: 5,
                entries: Vec::new(),
                launchers: Vec::new(),
                skipped: 0,
            }),
            None,
        );
        hub.writer.submit_intent(scan).await.unwrap();
        let mut seen = Vec::new();
        loop {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "library.changed" {
                assert_eq!(data, serde_json::json!({}));
                break;
            }
            seen.push(event);
            assert!(seen.len() < 6, "{seen:?}");
        }
    }

    #[tokio::test]
    async fn a_component_change_also_reports_the_bundles_that_hold_it() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let product = |id: Option<String>, name: &str, active: bool, parts: &[&str]| {
            IntentPayload::ProductUpsert(arena_proto::ProductUpsert {
                id,
                name: name.into(),
                category: String::new(),
                price_minor: 100,
                tax_rate_bp: None,
                hsn: String::new(),
                track_stock: false,
                image: String::new(),
                barcode: String::new(),
                kitchen: false,
                favourite: false,
                active,
                bundle: !parts.is_empty(),
                components: parts
                    .iter()
                    .map(|p| arena_proto::BundleComponent {
                        product_id: (*p).into(),
                        qty: 2,
                    })
                    .collect(),
            })
        };
        let owner = || Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        let chips = hub
            .writer
            .submit_intent(intent_as(owner(), product(None, "Chips", true, &[]), None))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        let combo = hub
            .writer
            .submit_intent(intent_as(
                owner(),
                product(None, "Combo", true, &[chips.as_str()]),
                None,
            ))
            .await
            .unwrap()
            .changes[0]
            .row_id
            .clone();
        hub.writer
            .submit_intent(intent_as(
                owner(),
                product(Some(chips.clone()), "Chips", false, &[]),
                Some(1),
            ))
            .await
            .unwrap();
        let mut seen = Vec::new();
        while seen.len() < 3 {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event == "catalog.changed" {
                seen.push(data);
            }
        }
        assert_eq!(
            seen[1],
            serde_json::json!({"products": [{"product_id": combo, "available": true}]})
        );
        assert_eq!(
            seen[2],
            serde_json::json!({"products": [
                {"product_id": chips, "available": false, "reason": "inactive"},
                {"product_id": combo, "available": false, "reason": "component_inactive"}
            ]})
        );
    }

    #[tokio::test]
    async fn a_kiosks_remote_unlock_ack_is_pushed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        // Give the stream's hub task a moment to subscribe.
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub.lan.ack_admin_unlock(arena_proto::AdminUnlockAckEvent {
            exit_id: "E1".into(),
            device_id: "D1".into(),
        });
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(
            (event.as_str(), data),
            (
                "device.admin_unlock_ack",
                serde_json::json!({"exit_id": "E1", "device_id": "D1"})
            )
        );
    }

    #[tokio::test]
    async fn a_kiosks_pc_ack_is_pushed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let _session = hub
            .lan
            .register(arena_proto::ClientClass::Paired, Some("D1".into()), tx);
        hub.lan
            .push_pc_command("D1", "C1", arena_proto::PcAction::Lock, None, None);
        assert!(hub.lan.ack_pc("D1", "C1"));
        let (event, data) = loop {
            let (event, data) = next_event(&mut frames).await.unwrap();
            if event != "devices.changed" {
                break (event, data);
            }
        };
        assert_eq!(
            (event.as_str(), data),
            (
                "device.pc_ack",
                serde_json::json!({"cmd_id": "C1", "device_id": "D1"})
            )
        );
    }

    #[tokio::test]
    async fn a_deferred_tz_change_is_pushed_as_pending() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        let body = serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "branch.tz".into(),
                value: "Asia/Dubai".into(),
            }),
            Some(1),
        ) });
        let (status, _) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(data["keys"], serde_json::json!(["branch.tz"]));
        assert_eq!(data["locale"]["tz"], "Asia/Kolkata");
        // T0 is 2027-01-15 13:30 in Kolkata: business day D = 2027-01-15.
        assert_eq!(
            data["locale"]["pending"]["tz"],
            serde_json::json!({"value": "Asia/Dubai", "effective_from": "2027-01-16"})
        );
    }

    #[tokio::test]
    async fn every_open_stream_gets_the_event() {
        let hub = hub().await;
        let a = token_for(&hub, "manager", "m-pass").await;
        let b = token_for(&hub, "counter", "c-pass").await;
        let (_, mut fa) = open(&hub, Some(&a)).await;
        let (_, mut fb) = open(&hub, Some(&b)).await;
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&a),
            Some(footer_set(manager(), 1)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        for frames in [&mut fa, &mut fb] {
            let (event, data) = next_event(frames).await.unwrap();
            assert_eq!(event, "settings.changed");
            assert_eq!(data["keys"], serde_json::json!(["receipt.footer"]));
            assert_eq!(data["locale"]["currency"], "INR");
        }
    }

    #[tokio::test]
    async fn signing_out_closes_the_stream() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        let (status, _) = call(&hub, "POST", "/ui/auth/sign-out", Some(&token), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(next_event(&mut frames).await, None, "the stream ended");
        let (status, _) = open(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn deactivating_the_member_closes_the_stream() {
        let hub = hub().await;
        let token = token_for(&hub, "counter", "c-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        let owner = Actor {
            kind: ActorKind::Owner,
            id: "owner".into(),
        };
        hub.writer
            .submit_intent(intent_as(
                owner,
                IntentPayload::StaffUpsert(arena_proto::StaffUpsert {
                    staff_id: Some(hub.counter_id.clone()),
                    login: "counter".into(),
                    display_name: "Counter".into(),
                    role_id: hub.counter_role.clone(),
                    password_hash: None,
                    pin_hash: None,
                    active: false,
                    setup_code_hash: None,
                    setup_expires_at: None,
                }),
                Some(1),
            ))
            .await
            .unwrap();
        assert_eq!(next_event(&mut frames).await, None);
    }

    fn fast() -> EventsConfig {
        EventsConfig {
            tick: Duration::from_millis(20),
            full_recheck: Duration::from_millis(20),
            capacity: 256,
        }
    }

    #[tokio::test]
    async fn a_lagging_stream_gets_resync() {
        let hub = hub_with(EventsConfig {
            capacity: 2,
            ..EventsConfig::default()
        })
        .await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        // Five changes before the stream reads anything: more than its buffer holds.
        let rev = |n| footer_set(manager(), n);
        for n in 1..=5 {
            let (status, _) = call(&hub, "POST", "/ui/intent", Some(&token), Some(rev(n))).await;
            assert_eq!(status, StatusCode::OK);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!((event.as_str(), data), ("resync", serde_json::json!({})));
        // It carries on with what is still buffered.
        let (event, _) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "settings.changed");
    }

    #[tokio::test]
    async fn the_stream_closes_when_the_session_reaches_its_ttl() {
        let hub = hub_with(fast()).await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        hub.clock.advance(SESSION_TTL_MS);
        assert_eq!(next_event(&mut frames).await, None);
    }

    #[tokio::test]
    async fn a_deferred_tz_is_pushed_when_its_business_day_starts() {
        let hub = hub_with(fast()).await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        let body = serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "branch.tz".into(),
                value: "Asia/Dubai".into(),
            }),
            Some(1),
        ) });
        let (status, _) = call(&hub, "POST", "/ui/intent", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(data["locale"]["tz"], "Asia/Kolkata");
        // 2027-01-16 06:00 IST (00:30 UTC) starts the change's business day. That is 16.5 h
        // after T0, past the 12 h session TTL, so a fresh session watches the rollover.
        let rollover = 1_800_000_000_000 + (16 * 60 + 30) * 60 * 1000;
        hub.clock.set(rollover - 60_000);
        let token2 = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token2)).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        hub.clock.set(rollover);
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "settings.changed");
        assert_eq!(data["keys"], serde_json::json!(["branch.tz"]));
        assert_eq!(data["locale"]["tz"], "Asia/Dubai");
        assert_eq!(data["locale"]["pending"], Value::Null);
    }

    #[tokio::test]
    async fn a_new_stream_gets_the_current_connectivity_first() {
        use arena_proto::{Connectivity, ConnectivityState};
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        hub._status.send_modify(|s| {
            s.connectivity = Connectivity {
                state: ConnectivityState::CloudUnreachable,
                checked_at: Some(9),
            }
        });
        let (status, mut frames) = open_raw(&hub, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            next_event(&mut frames).await.unwrap(),
            (
                "connectivity.changed".to_owned(),
                serde_json::json!({"state": "cloud_unreachable", "checked_at": 9})
            )
        );
    }

    #[tokio::test]
    async fn a_connectivity_change_is_pushed_once_per_state() {
        use arena_proto::{Connectivity, ConnectivityState};
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let set = |state, at| {
            hub._status.send_modify(|s| {
                s.connectivity = Connectivity {
                    state,
                    checked_at: Some(at),
                }
            })
        };
        // The first probe finds the state it started with: nothing is pushed.
        set(ConnectivityState::Online, 1);
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(ConnectivityState::InternetOffline, 2);
        tokio::time::sleep(Duration::from_millis(50)).await;
        // A later probe with the same state is not pushed again.
        set(ConnectivityState::InternetOffline, 3);
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(ConnectivityState::CloudUnreachable, 4);
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(ConnectivityState::Online, 5);
        let mut seen = Vec::new();
        for _ in 0..3 {
            let (event, data) = next_event(&mut frames).await.unwrap();
            assert_eq!(event, "connectivity.changed");
            seen.push(data);
        }
        assert_eq!(
            seen,
            vec![
                serde_json::json!({"state": "internet_offline", "checked_at": 2}),
                serde_json::json!({"state": "cloud_unreachable", "checked_at": 4}),
                serde_json::json!({"state": "online", "checked_at": 5}),
            ]
        );
    }

    #[tokio::test]
    async fn a_tier_change_is_pushed_once_and_a_repeat_is_not() {
        use arena_proto::Tier;
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let set = |tier| hub._status.send_modify(|s| s.tier = tier);
        // A heartbeat that finds the tier it already had pushes nothing.
        set(Some(Tier::Free));
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(Some(Tier::Free));
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(Some(Tier::Basic));
        tokio::time::sleep(Duration::from_millis(50)).await;
        set(None);
        let mut seen = Vec::new();
        for _ in 0..3 {
            let (event, data) = next_event(&mut frames).await.unwrap();
            assert_eq!(event, "entitlement.changed");
            seen.push(data);
        }
        assert_eq!(
            seen,
            vec![
                serde_json::json!({"tier": "free"}),
                serde_json::json!({"tier": "basic"}),
                serde_json::json!({"tier": null}),
            ]
        );
    }

    #[tokio::test]
    async fn a_lan_address_change_pushes_devices_changed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub._status
            .send_modify(|s| s.lan_addresses = vec![std::net::Ipv4Addr::new(10, 0, 0, 4)]);
        assert_eq!(
            next_event(&mut frames).await.unwrap(),
            ("devices.changed".to_owned(), serde_json::json!({}))
        );
    }

    #[tokio::test]
    async fn a_lan_network_name_change_alone_pushes_devices_changed() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub._status
            .send_modify(|s| s.lan_network = Some("Ethernet".into()));
        assert_eq!(
            next_event(&mut frames).await.unwrap(),
            ("devices.changed".to_owned(), serde_json::json!({}))
        );
    }

    #[tokio::test]
    async fn a_role_change_sends_resync_and_closes_the_stream() {
        let hub = hub().await;
        let token = token_for(&hub, "manager", "m-pass").await;
        let (_, mut frames) = open(&hub, Some(&token)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub._status
            .send_modify(|s| s.role = arena_core::Role::Follower);
        let (event, _) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "resync");
        assert_eq!(next_event(&mut frames).await, None);
    }

    #[tokio::test]
    async fn a_locked_session_keeps_receiving_and_gets_the_new_idle_lock() {
        let hub = hub().await;
        let manager_token = token_for(&hub, "manager", "m-pass").await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let (status, mut frames) = open(&hub, Some(&counter)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(&hub, "POST", "/ui/lock", Some(&counter), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (event, _) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "session.locked");
        let set = serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "idle_lock_minutes".into(),
                value: "10".into(),
            }),
            Some(1),
        ) });
        let (status, body) =
            call(&hub, "POST", "/ui/intent", Some(&manager_token), Some(set)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "settings.changed");
        assert_eq!(data["keys"], serde_json::json!(["idle_lock_minutes"]));
        assert_eq!(
            data["ui"],
            serde_json::json!({"idle_lock_minutes": 10, "hc": false})
        );
    }

    #[tokio::test]
    async fn turning_hub_high_contrast_on_pushes_ui_hc() {
        let hub = hub().await;
        let manager_token = token_for(&hub, "manager", "m-pass").await;
        let (status, mut frames) = open(&hub, Some(&manager_token)).await;
        assert_eq!(status, StatusCode::OK);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let set = serde_json::json!({ "intent": intent_as(
            manager(),
            IntentPayload::SettingsSet(SettingsSet {
                key: "display.hc.hub".into(),
                value: "true".into(),
            }),
            Some(1),
        ) });
        let (status, body) =
            call(&hub, "POST", "/ui/intent", Some(&manager_token), Some(set)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (event, data) = next_event(&mut frames).await.unwrap();
        assert_eq!(event, "settings.changed");
        assert_eq!(data["keys"], serde_json::json!(["display.hc.hub"]));
        assert_eq!(data["ui"]["hc"], true);
    }

    #[tokio::test]
    async fn lock_and_unlock_reach_only_the_streams_of_the_same_token() {
        let hub = hub().await;
        let counter = token_for(&hub, "counter", "c-pass").await;
        let other = token_for(&hub, "manager", "m-pass").await;
        let (_, mut mine) = open(&hub, Some(&counter)).await;
        let (_, mut theirs) = open(&hub, Some(&other)).await;
        let (status, _) = call(&hub, "POST", "/ui/lock", Some(&counter), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (event, data) = next_event(&mut mine).await.unwrap();
        assert_eq!(
            (event.as_str(), data),
            ("session.locked", serde_json::json!({}))
        );
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/unlock",
            Some(&counter),
            Some(serde_json::json!({"pin": "2222"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (event, data) = next_event(&mut mine).await.unwrap();
        assert_eq!(event, "session.unlocked");
        assert_eq!(data["switched"], true);
        assert_eq!(data["staff"]["staff_id"], hub.manager_id.as_str());
        // The other token's stream heard nothing of it: its next event is a real change.
        let (status, _) = call(
            &hub,
            "POST",
            "/ui/intent",
            Some(&other),
            Some(footer_set(manager(), 1)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (event, _) = next_event(&mut theirs).await.unwrap();
        assert_eq!(event, "settings.changed");
    }
}

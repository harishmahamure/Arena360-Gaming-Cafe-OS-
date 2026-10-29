//! Connected LAN sessions, the push fan-out and revocation.
//!
//! Every committed intent from the writer broadcast is pushed to every `paired` session as
//! `event{kind:"applied"}`. Browser classes (`kds`, `phone`) only get the changes to tables on
//! their allow-list ([`browser_tables`], empty for now), and nothing when none remain. When the
//! intent wrote `kiosk.*` or `branch.fulfilment` settings, their values now follow as `event{kind:"settings.changed"}`,
//! to `paired` sessions only (browser classes never see settings). Each
//! session has its own bounded queue; a session whose queue is full is dropped, so a slow or
//! failing client never blocks the others. After an intent that touched a `devices` row,
//! sessions of a device that is now revoked get `TOKEN_REVOKED` and are closed. If the fan-out
//! lags behind the writer broadcast, events were lost, so every session is dropped (clients
//! reconnect and resync) after revoked devices got their `TOKEN_REVOKED`.
//!
//! **Attract screen (Story 6.7).** On every fan-out tick each connected paired device gets its own
//! `event{kind:"attract"}`: the branch name, its group's rate with the pricing rule in effect
//! (rules start and end by the clock, so the tick is the trigger) and the library's curated
//! videos. Browser classes never get it.
//!
//! **Admin exit (Story 1.20).** After an intent that wrote staff or roles, the manager verifier
//! is re-read and, when its version changed, pushed as `event{kind:"verifier"}` to every
//! `paired` session (each session also gets it right after its hello). A `remote` row inserted
//! into `device_admin_exits` is pushed as `event{kind:"admin_unlock", exit_id}` to that
//! device's paired sessions only. The kiosk's `kiosk.admin_unlock_ack` is published on
//! [`Registry::admin_unlock_acks`] for the Hub UI.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use arena_core::ChangeOp;
use arena_core::{Applied, DeviceDirectory};
use arena_proto::{
    AdminExitMode, AdminUnlockAckEvent, AppError, AppliedSummary, ChangeOpKind, ChangeSummary,
    ClientClass, ErrorCode, EventFrame, Frame, FrameBody, HcTarget, HubEvent, LockReason,
    ManagerVerifier, PAIRED_SETTINGS_PREFIX, PcAckEvent, PcAction, SessionScreen, SettingEntry,
    SettingsValues, TEST_SESSION_MS, TestSessionState, TestSessionStatus, may_read_setting,
    parse_hc_key,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

/// Why a session is being closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kill {
    /// Close after sending this error frame.
    Error(AppError),
    /// Close without a frame (queue overflow, shutdown).
    Drop,
}

const SESSION_TICK: std::time::Duration = std::time::Duration::from_secs(5);

const PENDING_CMD_TTL: std::time::Duration = std::time::Duration::from_secs(600);

pub(crate) const OFFLINE_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

struct Entry {
    class: ClientClass,
    device_id: Option<String>,
    out: mpsc::Sender<Message>,
    kill: Arc<watch::Sender<Option<Kill>>>,
}

/// The set of live sessions.
pub(crate) struct Registry {
    sessions: Mutex<HashMap<u64, Entry>>,
    next_id: AtomicU64,
    next_event: AtomicU64,
    /// Set by [`Registry::kill_all`]: a session registered afterwards starts killed.
    shutting_down: AtomicBool,
    /// Bumped whenever a paired device session is added or removed (device presence).
    presence: watch::Sender<u64>,
    /// When each paired device last connected or disconnected.
    seen: Mutex<HashMap<String, (tokio::time::Instant, i64)>>,
    /// Remote unlocks a kiosk acknowledged (for the Hub UI).
    acks: broadcast::Sender<AdminUnlockAckEvent>,
    pending_cmds: Mutex<HashMap<(String, String), std::time::Instant>>,
    pc_acks: broadcast::Sender<PcAckEvent>,
    /// The manager-verifier version last pushed to every paired session (-1: none yet).
    verifier_pushed: AtomicI64,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            sessions: Mutex::default(),
            next_id: AtomicU64::default(),
            next_event: AtomicU64::default(),
            shutting_down: AtomicBool::default(),
            presence: watch::channel(0).0,
            seen: Mutex::default(),
            acks: broadcast::channel(64).0,
            pending_cmds: Mutex::default(),
            pc_acks: broadcast::channel(64).0,
            verifier_pushed: AtomicI64::new(-1),
        }
    }
}

/// The `event` frame carrying `event`, with a Hub-minted rid.
fn event_frame(rid: String, event: HubEvent) -> Message {
    Message::text(Frame::new(rid, FrameBody::Event(EventFrame { event })).to_text())
}

/// The `event{kind:"verifier"}` frame for `verifier`.
pub(crate) fn verifier_message(rid: String, verifier: ManagerVerifier) -> Message {
    event_frame(rid, HubEvent::Verifier { verifier })
}

/// Whether `applied` wrote staff, roles or permission rows (the verifier may have changed).
fn touches_staff(applied: &Applied) -> bool {
    applied
        .changes
        .iter()
        .any(|c| matches!(c.tbl.as_str(), "staff" | "roles" | "role_perms"))
}

/// The exits `applied` inserted.
fn inserted_exits(applied: &Applied) -> Vec<String> {
    applied
        .changes
        .iter()
        .filter(|c| c.tbl == "device_admin_exits" && c.op == ChangeOp::Insert)
        .map(|c| c.row_id.clone())
        .collect()
}

/// A registered session: its kill switch and two receivers (writer task, reader loop) created
/// before the session became visible, so no kill is ever missed. Dropping it removes the
/// session from the registry.
pub(crate) struct SessionGuard {
    registry: Arc<Registry>,
    id: u64,
    pub(crate) kill: Arc<watch::Sender<Option<Kill>>>,
    pub(crate) writer_kill: watch::Receiver<Option<Kill>>,
    pub(crate) reader_kill: watch::Receiver<Option<Kill>>,
}

/// Tables whose changes a browser class may see in pushed events. Empty until the kitchen
/// and console epics add theirs.
pub(crate) fn browser_tables(class: ClientClass) -> &'static [&'static str] {
    match class {
        ClientClass::Paired | ClientClass::Kds | ClientClass::Phone => &[],
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        let removed = self
            .registry
            .sessions
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&self.id));
        if let Some(id) = removed
            .filter(|e| e.class == ClientClass::Paired)
            .and_then(|e| e.device_id)
        {
            self.registry.mark_seen(&id);
            self.registry.presence.send_modify(|n| *n += 1);
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let registry = Arc::clone(&self.registry);
                rt.spawn(async move {
                    tokio::time::sleep(OFFLINE_GRACE).await;
                    registry.presence.send_modify(|n| *n += 1);
                });
            }
        }
    }
}

pub(crate) fn summary(applied: &Applied) -> AppliedSummary {
    AppliedSummary {
        intent_id: applied.intent_id.clone(),
        changes: applied
            .changes
            .iter()
            .map(|c| ChangeSummary {
                epoch: c.epoch.clone(),
                seq: c.seq,
                tbl: c.tbl.clone(),
                row_id: c.row_id.clone(),
                op: match c.op {
                    ChangeOp::Insert => ChangeOpKind::Insert,
                    ChangeOp::Update => ChangeOpKind::Update,
                    ChangeOp::Delete => ChangeOpKind::Delete,
                },
            })
            .collect(),
    }
}

/// Sets a session's kill reason. An error reason replaces a plain drop (so a revoked device
/// always learns why), but nothing replaces an error.
pub(crate) fn set_kill(kill: &watch::Sender<Option<Kill>>, reason: Kill) {
    kill.send_if_modified(|k| match (k.as_ref(), &reason) {
        (Some(Kill::Error(_)), _) | (Some(Kill::Drop), Kill::Drop) => false,
        _ => {
            *k = Some(reason.clone());
            true
        }
    });
}

pub(crate) fn revoked_error() -> AppError {
    AppError::new(ErrorCode::TokenRevoked, "this device has been revoked")
}

impl Registry {
    /// Adds a session whose pushes go to `out`. It stays registered until the guard is
    /// dropped.
    pub(crate) fn register(
        self: &Arc<Self>,
        class: ClientClass,
        device_id: Option<String>,
        out: mpsc::Sender<Message>,
    ) -> SessionGuard {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (kill, reader_kill) = watch::channel(None);
        let kill = Arc::new(kill);
        let writer_kill = kill.subscribe();
        let presence = class == ClientClass::Paired && device_id.is_some();
        let seen_id = device_id.clone();
        if let Ok(mut map) = self.sessions.lock() {
            map.insert(
                id,
                Entry {
                    class,
                    device_id,
                    out,
                    kill: Arc::clone(&kill),
                },
            );
            // Checked under the lock `kill_all` takes, so a session is either killed by it or
            // sees the flag here.
            if self.shutting_down.load(Ordering::SeqCst) {
                set_kill(&kill, Kill::Drop);
            }
        }
        if presence {
            if let Some(id) = &seen_id {
                self.mark_seen(id);
            }
            self.presence.send_modify(|n| *n += 1);
        }
        SessionGuard {
            registry: Arc::clone(self),
            id,
            kill,
            writer_kill,
            reader_kill,
        }
    }

    /// Bumped whenever a paired device session is added or removed.
    pub(crate) fn presence_changes(&self) -> watch::Receiver<u64> {
        self.presence.subscribe()
    }

    /// The remote unlocks kiosks acknowledge from now on.
    pub(crate) fn admin_unlock_acks(&self) -> broadcast::Receiver<AdminUnlockAckEvent> {
        self.acks.subscribe()
    }

    /// Publishes a kiosk's acknowledgement of a remote unlock (no subscriber is fine).
    pub(crate) fn ack_admin_unlock(&self, ack: AdminUnlockAckEvent) {
        let _ = self.acks.send(ack);
    }

    pub(crate) fn pc_acks(&self) -> broadcast::Receiver<PcAckEvent> {
        self.pc_acks.subscribe()
    }

    pub(crate) fn ack_pc(&self, device_id: &str, cmd_id: &str) -> bool {
        let Ok(mut pending) = self.pending_cmds.lock() else {
            return false;
        };
        if pending
            .remove(&(device_id.to_owned(), cmd_id.to_owned()))
            .is_none()
        {
            return false;
        }
        let _ = self.pc_acks.send(PcAckEvent {
            cmd_id: cmd_id.to_owned(),
            device_id: device_id.to_owned(),
        });
        true
    }

    fn track(&self, device_id: &str, cmd_id: &str) {
        if let Ok(mut pending) = self.pending_cmds.lock() {
            pending.retain(|_, at| at.elapsed() < PENDING_CMD_TTL);
            pending.insert(
                (device_id.to_owned(), cmd_id.to_owned()),
                std::time::Instant::now(),
            );
        }
    }

    pub(crate) fn push_pc_command(
        &self,
        device_id: &str,
        cmd_id: &str,
        action: PcAction,
        text: Option<String>,
        reason: Option<LockReason>,
    ) -> usize {
        self.track(device_id, cmd_id);
        let msg = event_frame(
            self.next_rid(),
            HubEvent::PcCommand {
                cmd_id: cmd_id.to_owned(),
                action,
                text,
                reason,
            },
        );
        self.push_paired(&msg, Some(device_id))
    }

    /// Pushes one staff message, under one `cmd_id`, to each of `devices` that is online, and
    /// returns how many PCs it reached. Every kiosk acks under its own device id.
    pub(crate) fn push_message_all(&self, devices: &[String], cmd_id: &str, text: &str) -> usize {
        devices
            .iter()
            .filter(|device_id| {
                self.push_pc_command(
                    device_id,
                    cmd_id,
                    PcAction::Message,
                    Some(text.to_owned()),
                    None,
                ) > 0
            })
            .count()
    }

    pub(crate) fn push_session_started(
        &self,
        device_id: &str,
        cmd_id: &str,
        session: SessionScreen,
    ) -> usize {
        self.track(device_id, cmd_id);
        let msg = event_frame(
            self.next_rid(),
            HubEvent::SessionStarted {
                cmd_id: cmd_id.to_owned(),
                session,
            },
        );
        self.push_paired(&msg, Some(device_id))
    }

    pub(crate) fn push_timer_event(&self, event: arena_core::TimerEvent) -> usize {
        match event {
            arena_core::TimerEvent::Warning {
                session_id,
                device_id,
                minutes_left,
            } => {
                let msg = event_frame(
                    self.next_rid(),
                    HubEvent::SessionWarning {
                        session_id,
                        minutes_left,
                    },
                );
                self.push_paired(&msg, Some(&device_id))
            }
            arena_core::TimerEvent::Expired {
                device_id, held, ..
            } => {
                let cmd_id = ulid::Ulid::generate().to_string();
                let reason = if held {
                    LockReason::TimesUpSeeCounter
                } else {
                    LockReason::TimesUp
                };
                self.push_pc_command(&device_id, &cmd_id, PcAction::Lock, None, Some(reason))
            }
        }
    }

    async fn tick_sessions(&self, devices: &Arc<DeviceDirectory>) {
        if self.device_ids().is_empty() {
            return;
        }
        let dir = Arc::clone(devices);
        match tokio::task::spawn_blocking(move || dir.session_screens(epoch_ms())).await {
            Ok(Ok(screens)) => {
                for (device_id, screen) in screens {
                    let msg = event_frame(self.next_rid(), HubEvent::SessionTick(screen));
                    self.push_paired(&msg, Some(&device_id));
                }
            }
            Ok(Err(e)) => tracing::warn!(code = e.code.as_str(), "session tick read failed"),
            Err(e) => tracing::warn!(error = %e, "session tick read panicked"),
        }
    }

    /// Pushes each connected device its attract view (Story 6.7). Rule starts and ends are by
    /// the clock, so a tick (not an intent) is the trigger.
    async fn tick_attract(
        &self,
        devices: &Arc<DeviceDirectory>,
        media: &Option<Arc<arena_core::media_cache::MediaCache>>,
    ) {
        let ids = self.device_ids();
        if ids.is_empty() {
            return;
        }
        let dir = Arc::clone(devices);
        let cache = media.clone();
        let read = tokio::task::spawn_blocking(move || {
            let lookup = |game: &str| {
                cache
                    .as_ref()
                    .map(|c| arena_core::attract::GameMediaHashes {
                        video: c.video_sha256(game).ok().flatten(),
                        cover: c.cover_sha256(game).ok().flatten(),
                    })
                    .unwrap_or_default()
            };
            dir.attract_views(&ids, epoch_ms(), &lookup)
        })
        .await;
        match read {
            Ok(Ok(views)) => {
                for (device_id, view) in views {
                    let msg = event_frame(self.next_rid(), HubEvent::Attract(Box::new(view)));
                    self.push_paired(&msg, Some(&device_id));
                }
            }
            Ok(Err(e)) => tracing::warn!(code = e.code.as_str(), "attract tick read failed"),
            Err(e) => tracing::warn!(error = %e, "attract tick read panicked"),
        }
    }

    fn next_rid(&self) -> String {
        format!("ev-{}", self.next_event.fetch_add(1, Ordering::Relaxed))
    }

    /// Sends `msg` to the paired sessions matching `want` (every paired session, or one
    /// device's). A session whose queue is full or closed is dropped. Returns how many got it.
    fn push_paired(&self, msg: &Message, device_id: Option<&str>) -> usize {
        let Ok(map) = self.sessions.lock() else {
            return 0;
        };
        let mut sent = 0;
        for (id, entry) in map.iter() {
            if entry.class != ClientClass::Paired
                || device_id.is_some_and(|d| entry.device_id.as_deref() != Some(d))
            {
                continue;
            }
            if entry.out.try_send(msg.clone()).is_err() {
                tracing::warn!(session = id, "session queue full or closed; dropping it");
                set_kill(&entry.kill, Kill::Drop);
            } else {
                sent += 1;
            }
        }
        sent
    }

    /// Pushes the manager verifier to every paired session (never to a browser class).
    pub(crate) fn push_verifier(&self, verifier: ManagerVerifier) {
        self.verifier_pushed
            .store(verifier.version, Ordering::Relaxed);
        let msg = verifier_message(self.next_rid(), verifier);
        self.push_paired(&msg, None);
    }

    /// Pushes `admin_unlock{exit_id}` to `device_id`'s paired sessions. Returns how many got it.
    pub(crate) fn push_admin_unlock(&self, device_id: &str, exit_id: &str) -> usize {
        let msg = event_frame(
            self.next_rid(),
            HubEvent::AdminUnlock {
                exit_id: exit_id.to_owned(),
            },
        );
        self.push_paired(&msg, Some(device_id))
    }

    pub(crate) fn push_test_session(&self, device_id: &str) -> usize {
        let msg = event_frame(
            self.next_rid(),
            HubEvent::TestSession {
                lock_after_ms: TEST_SESSION_MS,
            },
        );
        self.push_paired(&msg, Some(device_id))
    }

    async fn push_test_sessions(&self, devices: &Arc<DeviceDirectory>, applied: &Applied) {
        let key = arena_proto::KEY_TEST_SESSION;
        if !applied
            .changes
            .iter()
            .any(|c| c.tbl == "settings" && c.row_id.ends_with(&format!("/{key}")))
        {
            return;
        }
        let dir = Arc::clone(devices);
        match tokio::task::spawn_blocking(move || dir.settings(&[key.to_owned()])).await {
            Ok(Ok(rows)) => {
                let running = rows
                    .first()
                    .and_then(|r| TestSessionStatus::parse(&r.value))
                    .filter(|s| s.state == TestSessionState::Running);
                if let Some(id) = running.and_then(|s| s.device_id) {
                    let sent = self.push_test_session(&id);
                    tracing::info!(device_id = %id, sessions = sent, "test session pushed");
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(code = e.code.as_str(), "test session read failed")
            }
            Err(e) => tracing::warn!(error = %e, "test session read panicked"),
        }
    }

    /// Re-reads the verifier and pushes it when its version changed since the last push.
    async fn push_verifier_if_changed(&self, devices: &Arc<DeviceDirectory>) {
        let dir = Arc::clone(devices);
        match tokio::task::spawn_blocking(move || dir.manager_verifier()).await {
            Ok(Ok(v)) => {
                if v.version != self.verifier_pushed.load(Ordering::Relaxed) {
                    tracing::info!(
                        version = v.version,
                        "pushing the manager verifier to paired sessions"
                    );
                    self.push_verifier(v);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    code = e.code.as_str(),
                    "manager verifier read failed; dropping paired sessions"
                );
                self.drop_paired();
            }
            Err(e) => {
                tracing::warn!(error = %e, "manager verifier read panicked; dropping paired sessions");
                self.drop_paired();
            }
        }
    }

    /// Pushes every remote exit `applied` inserted to its kiosk.
    async fn push_remote_exits(&self, devices: &Arc<DeviceDirectory>, applied: &Applied) {
        for exit_id in inserted_exits(applied) {
            let dir = Arc::clone(devices);
            let id = exit_id.clone();
            match tokio::task::spawn_blocking(move || dir.admin_exit(&id)).await {
                Ok(Ok(Some(row))) if row.mode == AdminExitMode::Remote.as_str() => {
                    let sent = self.push_admin_unlock(&row.device_id, &exit_id);
                    tracing::info!(exit_id = %exit_id, device_id = %row.device_id, sessions = sent, "remote unlock pushed");
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    tracing::warn!(exit_id = %exit_id, code = e.code.as_str(), "admin exit read failed")
                }
                Err(e) => {
                    tracing::warn!(exit_id = %exit_id, error = %e, "admin exit read panicked")
                }
            }
        }
    }

    fn mark_seen(&self, device_id: &str) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(
                device_id.to_owned(),
                (tokio::time::Instant::now(), epoch_ms()),
            );
        }
    }

    /// Whether `device_id` shows online (connected, or gone for less than [`OFFLINE_GRACE`])
    /// and when it was last seen (epoch ms).
    pub(crate) fn status(&self, device_id: &str) -> (bool, Option<i64>) {
        if self.has_paired_session(device_id) {
            return (true, Some(epoch_ms()));
        }
        let seen = self
            .seen
            .lock()
            .ok()
            .and_then(|s| s.get(device_id).copied());
        match seen {
            Some((at, ms)) => (at.elapsed() < OFFLINE_GRACE, Some(ms)),
            None => (false, None),
        }
    }

    fn has_paired_session(&self, device_id: &str) -> bool {
        self.sessions.lock().is_ok_and(|map| {
            map.values().any(|e| {
                e.class == ClientClass::Paired && e.device_id.as_deref() == Some(device_id)
            })
        })
    }

    /// Whether `device_id` has a live (not closing) `paired` session.
    pub(crate) fn is_online(&self, device_id: &str) -> bool {
        let Ok(map) = self.sessions.lock() else {
            return false;
        };
        map.values().any(|e| {
            e.class == ClientClass::Paired
                && e.device_id.as_deref() == Some(device_id)
                && e.kill.borrow().is_none()
        })
    }

    /// How many sessions are connected.
    pub(crate) fn len(&self) -> usize {
        self.sessions.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Pushes one committed intent without waiting: the whole summary to `paired` sessions,
    /// only allow-listed tables to browser classes (nothing when none remain). A session whose
    /// queue is full or closed is dropped.
    pub(crate) fn push_applied(&self, applied: &Applied) {
        let n = self.next_event.fetch_add(1, Ordering::Relaxed);
        let full = summary(applied);
        let message = |s: AppliedSummary| {
            let frame = Frame::new(
                format!("ev-{n}"),
                FrameBody::Event(EventFrame {
                    event: HubEvent::Applied(s),
                }),
            );
            Message::text(frame.to_text())
        };
        let paired = message(full.clone());
        let mut per_class: HashMap<ClientClass, Option<Message>> = HashMap::new();
        let Ok(map) = self.sessions.lock() else {
            return;
        };
        for (id, entry) in map.iter() {
            let msg = if entry.class == ClientClass::Paired {
                Some(paired.clone())
            } else {
                per_class
                    .entry(entry.class)
                    .or_insert_with(|| {
                        let allowed = browser_tables(entry.class);
                        let changes: Vec<ChangeSummary> = full
                            .changes
                            .iter()
                            .filter(|c| allowed.contains(&c.tbl.as_str()))
                            .cloned()
                            .collect();
                        (!changes.is_empty()).then(|| {
                            message(AppliedSummary {
                                intent_id: full.intent_id.clone(),
                                changes,
                            })
                        })
                    })
                    .clone()
            };
            let Some(msg) = msg else { continue };
            if entry.out.try_send(msg).is_err() {
                tracing::warn!(session = id, "session queue full or closed; dropping it");
                set_kill(&entry.kill, Kill::Drop);
            }
        }
    }

    /// Pushes `settings.changed` to every `paired` session, each with the subset of `values`
    /// it may read: every session gets the `kiosk.*` values, and a device's own
    /// `display.hc.<device_id>` reaches only that device. A session with nothing to read gets
    /// nothing. A session whose queue is full or closed is dropped.
    pub(crate) fn push_settings(&self, values: Vec<SettingEntry>) {
        if values.is_empty() {
            return;
        }
        let Ok(map) = self.sessions.lock() else {
            return;
        };
        for (id, entry) in map.iter() {
            if entry.class != ClientClass::Paired {
                continue;
            }
            let mine: Vec<SettingEntry> = values
                .iter()
                .filter(|v| may_read_setting(entry.class, &v.key, entry.device_id.as_deref()))
                .cloned()
                .collect();
            if mine.is_empty() {
                continue;
            }
            let n = self.next_event.fetch_add(1, Ordering::Relaxed);
            let frame = Frame::new(
                format!("ev-{n}"),
                FrameBody::Event(EventFrame {
                    event: HubEvent::SettingsChanged(SettingsValues { values: mine }),
                }),
            );
            if entry.out.try_send(Message::text(frame.to_text())).is_err() {
                tracing::warn!(session = id, "session queue full or closed; dropping it");
                set_kill(&entry.kill, Kill::Drop);
            }
        }
    }

    /// Pushes `catalog.changed` for the products `applied` wrote to every `paired` session.
    async fn push_catalog(&self, devices: &Arc<DeviceDirectory>, applied: &Applied) {
        let ids = product_ids(applied);
        if ids.is_empty() {
            return;
        }
        let dir = Arc::clone(devices);
        match tokio::task::spawn_blocking(move || dir.catalog(&ids, epoch_ms())).await {
            Ok(Ok(products)) => {
                let n = self.next_event.fetch_add(1, Ordering::Relaxed);
                let frame = Frame::new(
                    format!("ev-{n}"),
                    FrameBody::Event(EventFrame {
                        event: HubEvent::CatalogChanged { products },
                    }),
                );
                let message = Message::text(frame.to_text());
                let Ok(map) = self.sessions.lock() else {
                    return;
                };
                for (id, entry) in map.iter().filter(|(_, e)| e.class == ClientClass::Paired) {
                    if entry.out.try_send(message.clone()).is_err() {
                        tracing::warn!(session = id, "session queue full or closed; dropping it");
                        set_kill(&entry.kill, Kill::Drop);
                    }
                }
            }
            _ => {
                tracing::warn!("catalog read failed; dropping paired sessions");
                self.drop_paired();
            }
        }
    }

    /// Reads the paired-readable settings `applied` wrote (`kiosk.*` and device
    /// `display.hc.*`) and pushes them (nothing when it wrote none).
    async fn push_kiosk_settings(&self, devices: &Arc<DeviceDirectory>, applied: &Applied) {
        let keys = paired_setting_keys(applied);
        if keys.is_empty() {
            return;
        }
        let dir = Arc::clone(devices);
        match tokio::task::spawn_blocking(move || dir.settings(&keys)).await {
            Ok(Ok(values)) => self.push_settings(values),
            Ok(Err(e)) => {
                // The sessions would miss a list change: drop them so they reconnect and re-read.
                tracing::warn!(
                    code = e.code.as_str(),
                    "kiosk settings read failed; dropping paired sessions"
                );
                self.drop_paired();
            }
            Err(e) => {
                tracing::warn!(error = %e, "kiosk settings read panicked; dropping paired sessions");
                self.drop_paired();
            }
        }
    }

    fn drop_paired(&self) {
        if let Ok(map) = self.sessions.lock() {
            for entry in map.values().filter(|e| e.class == ClientClass::Paired) {
                set_kill(&entry.kill, Kill::Drop);
            }
        }
    }

    /// Closes every session of `device_id` with `kill`.
    pub(crate) fn kill_device(&self, device_id: &str, kill: Kill) {
        let Ok(map) = self.sessions.lock() else {
            return;
        };
        for entry in map.values() {
            if entry.device_id.as_deref() == Some(device_id) {
                set_kill(&entry.kill, kill.clone());
            }
        }
    }

    /// Closes every session; sessions registered afterwards start killed (shutdown).
    pub(crate) fn kill_all(&self) {
        if let Ok(map) = self.sessions.lock() {
            self.shutting_down.store(true, Ordering::SeqCst);
            for entry in map.values() {
                set_kill(&entry.kill, Kill::Drop);
            }
        }
    }

    /// Drops every live session (after lost events); later sessions are unaffected.
    fn drop_all(&self) {
        if let Ok(map) = self.sessions.lock() {
            for entry in map.values() {
                set_kill(&entry.kill, Kill::Drop);
            }
        }
    }

    /// The device ids with a live session.
    fn device_ids(&self) -> Vec<String> {
        let Ok(map) = self.sessions.lock() else {
            return Vec::new();
        };
        let mut ids: Vec<String> = map.values().filter_map(|e| e.device_id.clone()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Closes the sessions of every listed device that is now revoked (`TOKEN_REVOKED`), or
    /// whose status cannot be read (a plain drop: the client reconnects and is re-checked).
    async fn close_revoked(&self, devices: &Arc<DeviceDirectory>, ids: Vec<String>) {
        for id in ids {
            let dir = Arc::clone(devices);
            let check = id.clone();
            let revoked = tokio::task::spawn_blocking(move || dir.is_revoked(&check)).await;
            match revoked {
                Ok(Ok(true)) => {
                    tracing::info!(device_id = %id, "device revoked; closing its sessions");
                    self.kill_device(&id, Kill::Error(revoked_error()));
                }
                Ok(Ok(false)) => {}
                Ok(Err(e)) => {
                    tracing::warn!(device_id = %id, code = e.code.as_str(), "revocation check failed");
                    self.kill_device(&id, Kill::Drop);
                }
                Err(e) => {
                    tracing::warn!(device_id = %id, error = %e, "revocation check panicked");
                    self.kill_device(&id, Kill::Drop);
                }
            }
        }
    }

    /// Runs the fan-out until the writer broadcast closes.
    pub(crate) async fn fan_out(
        self: Arc<Self>,
        mut events: broadcast::Receiver<Applied>,
        devices: Arc<DeviceDirectory>,
        media: Option<Arc<arena_core::media_cache::MediaCache>>,
    ) {
        let mut tick = tokio::time::interval(SESSION_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let event = tokio::select! {
                e = events.recv() => e,
                _ = tick.tick() => {
                    self.tick_sessions(&devices).await;
                    self.tick_attract(&devices, &media).await;
                    continue;
                }
            };
            match event {
                Ok(applied) => {
                    self.push_applied(&applied);
                    self.push_kiosk_settings(&devices, &applied).await;
                    self.push_catalog(&devices, &applied).await;
                    if touches_staff(&applied) {
                        self.push_verifier_if_changed(&devices).await;
                    }
                    self.push_remote_exits(&devices, &applied).await;
                    self.push_test_sessions(&devices, &applied).await;
                    let mut touched: Vec<String> = applied
                        .changes
                        .iter()
                        .filter(|c| c.tbl == "devices")
                        .map(|c| c.row_id.clone())
                        .collect();
                    touched.sort();
                    touched.dedup();
                    if !touched.is_empty() {
                        self.close_revoked(&devices, touched).await;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Sessions missed events and cannot resync in place: revoked devices get
                    // TOKEN_REVOKED first, then every session is dropped and reconnects.
                    tracing::warn!(missed = n, "lan fan-out lagged behind the writer");
                    let ids = self.device_ids();
                    self.close_revoked(&devices, ids).await;
                    self.drop_all();
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}

pub(crate) fn product_ids(applied: &Applied) -> Vec<String> {
    let mut ids: Vec<String> = applied
        .changes
        .iter()
        .filter(|c| matches!(c.tbl.as_str(), "products" | "stock"))
        .map(|c| c.row_id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The setting keys `applied` wrote that some paired session may read: `kiosk.*`,
/// `branch.fulfilment`, `branch.tz`, the kiosk billing keys and every device's `display.hc.<device_id>` (`settings` row ids are
/// `branch_id/key`). Which session gets which is decided per session in
/// [`Registry::push_settings`].
pub(crate) fn paired_setting_keys(applied: &Applied) -> Vec<String> {
    let mut keys: Vec<String> = applied
        .changes
        .iter()
        .filter(|c| c.tbl == "settings")
        .filter_map(|c| c.row_id.split_once('/').map(|(_, k)| k.to_owned()))
        .filter(|k| {
            k.starts_with(PAIRED_SETTINGS_PREFIX)
                || k == arena_proto::KEY_FULFILMENT
                || k == arena_proto::KEY_BRANCH_TZ
                || arena_proto::KIOSK_BILLING_KEYS.contains(&k.as_str())
                || matches!(parse_hc_key(k), Some(HcTarget::Device(_)))
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::ChangeMeta;

    fn applied() -> Applied {
        Applied {
            intent_id: "I".into(),
            changes: vec![ChangeMeta {
                epoch: "1:H".into(),
                seq: 1,
                tbl: "devices".into(),
                row_id: "D".into(),
                op: ChangeOp::Insert,
            }],
        }
    }

    #[tokio::test]
    async fn full_queue_drops_only_that_session() {
        let reg = Arc::new(Registry::default());
        let (slow_tx, _slow_rx) = mpsc::channel(1);
        let slow = reg.register(ClientClass::Paired, None, slow_tx);
        let slow_kill_rx = slow.kill.subscribe();
        let (fast_tx, mut fast_rx) = mpsc::channel(8);
        let _fast = reg.register(ClientClass::Paired, Some("D".into()), fast_tx);
        let fast_kill_rx = _fast.kill.subscribe();
        assert_eq!(reg.len(), 2);

        reg.push_applied(&applied());
        reg.push_applied(&applied());
        assert_eq!(*slow_kill_rx.borrow(), Some(Kill::Drop));
        assert_eq!(*fast_kill_rx.borrow(), None);
        let Some(Message::Text(t)) = fast_rx.recv().await else {
            panic!("text")
        };
        let frame = arena_proto::parse_frame(t.as_str()).unwrap();
        let FrameBody::Event(EventFrame {
            event: HubEvent::Applied(s),
        }) = frame.body
        else {
            panic!("{frame:?}")
        };
        assert_eq!(s.changes[0].op, ChangeOpKind::Insert);
        fast_rx.recv().await.unwrap();

        reg.kill_device("D", Kill::Error(revoked_error()));
        assert_eq!(*fast_kill_rx.borrow(), Some(Kill::Error(revoked_error())));
        drop(slow);
        assert_eq!(reg.len(), 1);
    }

    #[tokio::test]
    async fn catalog_changed_reaches_paired_sessions_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = arena_store::Store::open(&path, 0).unwrap();
        let row = |id: &str, active: bool| arena_store::ProductRow {
            product_id: id.into(),
            name: id.into(),
            category: String::new(),
            price_minor: 1,
            tax_rate_bp: 0,
            hsn: String::new(),
            track_stock: false,
            image: String::new(),
            barcode: None,
            kitchen: false,
            favourite: false,
            active,
            bundle: false,
            rev: 0,
            deleted_at: None,
        };
        store
            .write_tx(|tx| {
                arena_store::insert_product(tx, &row("A", true))?;
                arena_store::insert_product(tx, &row("B", false))
            })
            .unwrap();
        let dir_ = Arc::new(DeviceDirectory::open(&path).unwrap());
        let reg = Arc::new(Registry::default());
        let (tx, mut paired) = mpsc::channel(8);
        let _p = reg.register(ClientClass::Paired, Some("P".into()), tx);
        let (tx, mut kds) = mpsc::channel(8);
        let _k = reg.register(ClientClass::Kds, Some("K".into()), tx);
        let change = |id: &str| ChangeMeta {
            epoch: "1:H".into(),
            seq: 1,
            tbl: "products".into(),
            row_id: id.into(),
            op: ChangeOp::Update,
        };
        reg.push_catalog(
            &dir_,
            &Applied {
                intent_id: "I".into(),
                changes: vec![change("B"), change("A"), change("A"), change("gone")],
            },
        )
        .await;
        let Some(Message::Text(t)) = paired.recv().await else {
            panic!("text")
        };
        let FrameBody::Event(EventFrame {
            event: HubEvent::CatalogChanged { products },
        }) = arena_proto::parse_frame(t.as_str()).unwrap().body
        else {
            panic!("not catalog.changed")
        };
        let got: Vec<_> = products
            .iter()
            .map(|p| (p.product_id.as_str(), p.available, p.reason.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("A", true, None),
                ("B", false, Some("inactive")),
                ("gone", false, Some("inactive"))
            ]
        );
        assert!(kds.try_recv().is_err());
        reg.push_catalog(
            &dir_,
            &Applied {
                intent_id: "I".into(),
                changes: vec![],
            },
        )
        .await;
        assert!(paired.try_recv().is_err());
    }

    #[tokio::test]
    async fn one_message_key_reaches_every_online_pc_and_each_acks_under_its_own_device() {
        let reg = Arc::new(Registry::default());
        let (tx_a, mut rx_a) = mpsc::channel(8);
        let _a = reg.register(ClientClass::Paired, Some("A".into()), tx_a);
        let (tx_b, mut rx_b) = mpsc::channel(8);
        let _b = reg.register(ClientClass::Paired, Some("B".into()), tx_b);
        let both = ["A".to_owned(), "B".to_owned(), "OFFLINE".to_owned()];
        assert_eq!(reg.push_message_all(&both, "K1", "hi"), 2);
        assert!(rx_a.try_recv().is_ok() && rx_b.try_recv().is_ok());
        let mut acks = reg.pc_acks();
        assert!(reg.ack_pc("A", "K1"));
        assert!(!reg.ack_pc("A", "K1"), "an ack is spent once");
        assert!(reg.ack_pc("B", "K1"));
        assert_eq!(acks.try_recv().unwrap().device_id, "A");
        assert_eq!(acks.try_recv().unwrap().device_id, "B");
    }

    #[tokio::test]
    async fn only_a_live_paired_session_counts_as_online() {
        let reg = Arc::new(Registry::default());
        assert!(!reg.is_online("P"));
        let (tx, _rx) = mpsc::channel(8);
        let paired = reg.register(ClientClass::Paired, Some("P".into()), tx);
        let (tx, _rx2) = mpsc::channel(8);
        let _kds = reg.register(ClientClass::Kds, Some("K".into()), tx);
        assert!(reg.is_online("P"));
        assert!(!reg.is_online("K"), "a kds session is not a paired one");
        let mut changes = reg.presence_changes();
        assert!(!changes.has_changed().unwrap());
        reg.kill_device("P", Kill::Drop);
        assert!(!reg.is_online("P"), "a closing session is offline");
        drop(paired);
        assert!(!reg.is_online("P"));
        assert!(changes.has_changed().unwrap(), "the close bumps presence");
        changes.mark_unchanged();
        drop(_kds);
        assert!(
            !changes.has_changed().unwrap(),
            "a kds session is not presence"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_device_shows_offline_after_15_s_disconnected_and_presence_is_pushed() {
        let reg = Arc::new(Registry::default());
        assert_eq!(reg.status("P"), (false, None));
        let (tx, _rx) = mpsc::channel(8);
        let guard = reg.register(ClientClass::Paired, Some("P".into()), tx);
        assert!(reg.status("P").0);
        let mut changes = reg.presence_changes();
        changes.mark_unchanged();
        drop(guard);
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        let (online, seen) = reg.status("P");
        assert!(online && seen.is_some(), "still online inside the grace");
        tokio::time::sleep(OFFLINE_GRACE - std::time::Duration::from_secs(1)).await;
        assert!(reg.status("P").0);
        assert!(!changes.has_changed().unwrap());
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert!(changes.has_changed().unwrap(), "the 15 s mark is pushed");
        let (online, seen2) = reg.status("P");
        assert!(!online);
        assert_eq!(seen2, seen, "last_seen_at stays at the disconnect");
        let (tx, _rx) = mpsc::channel(8);
        let _back = reg.register(ClientClass::Paired, Some("P".into()), tx);
        assert!(reg.status("P").0);
    }

    #[tokio::test]
    async fn browser_classes_get_no_applied_events_for_unlisted_tables() {
        let reg = Arc::new(Registry::default());
        let (kds_tx, mut kds_rx) = mpsc::channel(8);
        let _kds = reg.register(ClientClass::Kds, Some("K".into()), kds_tx);
        let (phone_tx, mut phone_rx) = mpsc::channel(8);
        let _phone = reg.register(ClientClass::Phone, None, phone_tx);
        let (paired_tx, mut paired_rx) = mpsc::channel(8);
        let _paired = reg.register(ClientClass::Paired, Some("P".into()), paired_tx);
        reg.push_applied(&applied());
        assert!(paired_rx.try_recv().is_ok());
        assert!(kds_rx.try_recv().is_err());
        assert!(phone_rx.try_recv().is_err());
    }

    #[test]
    fn only_kiosk_setting_writes_are_pushed_as_settings() {
        let change = |tbl: &str, row_id: &str| ChangeMeta {
            epoch: "1:H".into(),
            seq: 1,
            tbl: tbl.into(),
            row_id: row_id.into(),
            op: ChangeOp::Update,
        };
        let applied = Applied {
            intent_id: "I".into(),
            changes: vec![
                change("settings", "B/kiosk.process_block"),
                change("settings", "B/branch.currency"),
                change("devices", "B/kiosk.process_allow"),
                change("settings", "B/kiosk.process_allow"),
                change("settings", "B/display.hc.hub"),
                change("settings", "B/display.hc.01JABCDEFGHJKMNPQRSTVWXYZ0"),
                change("settings", "B/branch.fulfilment"),
                change("settings", "B/kiosk.volume_default"),
                change("settings", "B/kiosk.booster_priority"),
                change("settings", "B/billing.grace_min"),
                change("settings", "B/billing.time_up_mode"),
                change("settings", "B/billing.warn_marks"),
                change("settings", "B/billing.low_balance_minor"),
                change("settings", "B/billing.rounding_min"),
                change("settings", "B/branch.opening_hours"),
                change("settings", "B/system.backup_dir"),
            ],
        };
        assert_eq!(
            paired_setting_keys(&applied),
            [
                "billing.grace_min",
                "billing.low_balance_minor",
                "billing.time_up_mode",
                "billing.warn_marks",
                "branch.fulfilment",
                "display.hc.01JABCDEFGHJKMNPQRSTVWXYZ0",
                "kiosk.booster_priority",
                "kiosk.process_allow",
                "kiosk.process_block",
                "kiosk.volume_default"
            ]
        );
    }

    #[tokio::test]
    async fn settings_changed_reaches_paired_sessions_only() {
        let reg = Arc::new(Registry::default());
        let (kds_tx, mut kds_rx) = mpsc::channel(8);
        let _kds = reg.register(ClientClass::Kds, Some("K".into()), kds_tx);
        let (phone_tx, mut phone_rx) = mpsc::channel(8);
        let _phone = reg.register(ClientClass::Phone, None, phone_tx);
        let (paired_tx, mut paired_rx) = mpsc::channel(8);
        let _paired = reg.register(ClientClass::Paired, Some("P".into()), paired_tx);
        let entry = |key: &str| SettingEntry {
            key: key.into(),
            value: "[]".into(),
            rev: 2,
        };
        reg.push_settings(vec![entry("kiosk.process_allow"), entry("branch.currency")]);
        let Some(Message::Text(t)) = paired_rx.recv().await else {
            panic!("text")
        };
        let frame = arena_proto::parse_frame(t.as_str()).unwrap();
        let FrameBody::Event(EventFrame {
            event: HubEvent::SettingsChanged(v),
        }) = frame.body
        else {
            panic!("{frame:?}")
        };
        assert_eq!(v.values, vec![entry("kiosk.process_allow")]);
        assert!(kds_rx.try_recv().is_err());
        assert!(phone_rx.try_recv().is_err());
        // Nothing a paired session may read: nothing is sent.
        reg.push_settings(vec![entry("branch.currency")]);
        assert!(paired_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_device_hc_change_reaches_only_that_device() {
        const A: &str = "01JABCDEFGHJKMNPQRSTVWXYZA";
        const B: &str = "01JABCDEFGHJKMNPQRSTVWXYZB";
        let reg = Arc::new(Registry::default());
        let (a_tx, mut a_rx) = mpsc::channel(8);
        let _a = reg.register(ClientClass::Paired, Some(A.into()), a_tx);
        let (b_tx, mut b_rx) = mpsc::channel(8);
        let _b = reg.register(ClientClass::Paired, Some(B.into()), b_tx);
        let (kds_tx, mut kds_rx) = mpsc::channel(8);
        let _kds = reg.register(ClientClass::Kds, Some(A.into()), kds_tx);
        let entry = |key: String| SettingEntry {
            key,
            value: "true".into(),
            rev: 1,
        };
        let values = |rx: &mut mpsc::Receiver<Message>| {
            let Ok(Message::Text(t)) = rx.try_recv() else {
                panic!("text")
            };
            let frame = arena_proto::parse_frame(t.as_str()).unwrap();
            let FrameBody::Event(EventFrame {
                event: HubEvent::SettingsChanged(v),
            }) = frame.body
            else {
                panic!("{frame:?}")
            };
            v.values
        };
        reg.push_settings(vec![
            entry(format!("display.hc.{A}")),
            entry("display.hc.hub".into()),
        ]);
        assert_eq!(values(&mut a_rx), vec![entry(format!("display.hc.{A}"))]);
        assert!(b_rx.try_recv().is_err(), "B gets nothing of A's");
        assert!(kds_rx.try_recv().is_err());
        // A list change still reaches both, each with its own HC value only.
        reg.push_settings(vec![
            entry("kiosk.process_mode".into()),
            entry(format!("display.hc.{B}")),
        ]);
        assert_eq!(values(&mut a_rx), vec![entry("kiosk.process_mode".into())]);
        assert_eq!(
            values(&mut b_rx),
            vec![
                entry("kiosk.process_mode".into()),
                entry(format!("display.hc.{B}"))
            ]
        );
    }

    #[tokio::test]
    async fn the_verifier_and_admin_unlocks_reach_paired_sessions_only() {
        let reg = Arc::new(Registry::default());
        let (kds_tx, mut kds_rx) = mpsc::channel(8);
        let _kds = reg.register(ClientClass::Kds, Some("K".into()), kds_tx);
        let (phone_tx, mut phone_rx) = mpsc::channel(8);
        let _phone = reg.register(ClientClass::Phone, None, phone_tx);
        let (a_tx, mut a_rx) = mpsc::channel(8);
        let _a = reg.register(ClientClass::Paired, Some("A".into()), a_tx);
        let (b_tx, mut b_rx) = mpsc::channel(8);
        let _b = reg.register(ClientClass::Paired, Some("B".into()), b_tx);
        reg.push_verifier(ManagerVerifier {
            version: 7,
            entries: vec![],
        });
        for rx in [&mut a_rx, &mut b_rx] {
            let Some(Message::Text(t)) = rx.recv().await else {
                panic!("text")
            };
            let frame = arena_proto::parse_frame(t.as_str()).unwrap();
            assert!(matches!(
                frame.body,
                FrameBody::Event(EventFrame {
                    event: HubEvent::Verifier { .. }
                })
            ));
        }
        assert!(
            kds_rx.try_recv().is_err(),
            "a kds session never gets the verifier"
        );
        assert!(phone_rx.try_recv().is_err());
        assert_eq!(reg.push_admin_unlock("A", "E1"), 1);
        let Some(Message::Text(t)) = a_rx.recv().await else {
            panic!("text")
        };
        let frame = arena_proto::parse_frame(t.as_str()).unwrap();
        assert_eq!(
            frame.body,
            FrameBody::Event(EventFrame {
                event: HubEvent::AdminUnlock {
                    exit_id: "E1".into()
                }
            })
        );
        assert!(b_rx.try_recv().is_err(), "only that device's sessions");
        assert_eq!(reg.push_admin_unlock("K", "E2"), 0, "never a kds session");
        let mut acks = reg.admin_unlock_acks();
        reg.ack_admin_unlock(AdminUnlockAckEvent {
            exit_id: "E1".into(),
            device_id: "A".into(),
        });
        assert_eq!(acks.recv().await.unwrap().exit_id, "E1");
    }

    #[tokio::test]
    async fn a_pc_command_reaches_one_device_and_only_its_own_ack_counts() {
        let reg = Arc::new(Registry::default());
        let (a_tx, mut a_rx) = mpsc::channel(8);
        let _a = reg.register(ClientClass::Paired, Some("A".into()), a_tx);
        let (b_tx, mut b_rx) = mpsc::channel(8);
        let _b = reg.register(ClientClass::Paired, Some("B".into()), b_tx);
        let (k_tx, mut k_rx) = mpsc::channel(8);
        let _k = reg.register(ClientClass::Kds, Some("K".into()), k_tx);
        assert_eq!(
            reg.push_pc_command("A", "C1", PcAction::Message, Some("hi".into()), None),
            1
        );
        let Some(Message::Text(t)) = a_rx.recv().await else {
            panic!("text")
        };
        assert_eq!(
            arena_proto::parse_frame(t.as_str()).unwrap().body,
            FrameBody::Event(EventFrame {
                event: HubEvent::PcCommand {
                    cmd_id: "C1".into(),
                    action: PcAction::Message,
                    text: Some("hi".into()),
                    reason: None
                }
            })
        );
        assert!(b_rx.try_recv().is_err() && k_rx.try_recv().is_err());
        assert_eq!(
            reg.push_pc_command("K", "C2", PcAction::Lock, None, None),
            0
        );
        let mut acks = reg.pc_acks();
        assert!(!reg.ack_pc("B", "C1"), "another device's ack");
        assert!(!reg.ack_pc("A", "nope"), "no such command");
        assert!(reg.ack_pc("A", "C1"));
        assert!(!reg.ack_pc("A", "C1"), "acked once per push");
        assert_eq!(acks.recv().await.unwrap().device_id, "A");
        assert_eq!(
            reg.push_pc_command("A", "C1", PcAction::Message, None, None),
            1
        );
        assert!(reg.ack_pc("A", "C1"), "a retry's push is pending again");
    }

    #[tokio::test]
    async fn a_kill_between_register_and_use_is_not_missed() {
        let reg = Arc::new(Registry::default());
        let (tx, _rx) = mpsc::channel(8);
        let mut guard = reg.register(ClientClass::Paired, Some("D".into()), tx);
        reg.kill_device("D", Kill::Error(revoked_error()));
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            guard.reader_kill.changed(),
        )
        .await
        .unwrap()
        .unwrap();
        guard.writer_kill.changed().await.unwrap();
        assert_eq!(
            *guard.writer_kill.borrow(),
            Some(Kill::Error(revoked_error()))
        );

        // After shutdown a new session starts killed.
        reg.kill_all();
        let (tx, _rx) = mpsc::channel(8);
        let mut late = reg.register(ClientClass::Phone, None, tx);
        late.reader_kill.changed().await.unwrap();
        assert_eq!(*late.reader_kill.borrow(), Some(Kill::Drop));
    }

    /// A database with a revoked device `R` and a live device `L`.
    fn directory() -> (tempfile::TempDir, Arc<DeviceDirectory>) {
        use arena_store::{
            NewBranch, NewDevice, Store, insert_branch, insert_device, set_device_revoked,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = Store::open(&path, 0).unwrap();
        store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "N",
                        branch_code: "C",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                for id in ["R", "L"] {
                    insert_device(
                        tx,
                        &NewDevice {
                            device_id: id,
                            branch_id: "B",
                            class: "paired",
                            label: "x",
                            token_hash: "h",
                        },
                    )?;
                }
                set_device_revoked(tx, "R", 1)
            })
            .unwrap();
        (dir, Arc::new(DeviceDirectory::open(&path).unwrap()))
    }

    #[tokio::test]
    async fn a_lagged_fan_out_drops_every_session_and_revokes_first() {
        let (_dir, devices) = directory();
        let reg = Arc::new(Registry::default());
        let mut guards = Vec::new();
        for (class, id) in [
            (ClientClass::Paired, Some("R")),
            (ClientClass::Paired, Some("L")),
            (ClientClass::Phone, None),
        ] {
            let (tx, rx) = mpsc::channel(64);
            guards.push((reg.register(class, id.map(str::to_owned), tx), rx));
        }
        let (events_tx, events_rx) = broadcast::channel(1);
        let quiet = Applied {
            intent_id: "Q".into(),
            changes: vec![],
        };
        for _ in 0..3 {
            events_tx.send(quiet.clone()).unwrap();
        }
        drop(events_tx);
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Arc::clone(&reg).fan_out(events_rx, devices, None),
        )
        .await
        .unwrap();
        let reasons: Vec<_> = guards
            .iter()
            .map(|(g, _)| g.reader_kill.borrow().clone())
            .collect();
        assert_eq!(
            reasons,
            vec![
                Some(Kill::Error(revoked_error())),
                Some(Kill::Drop),
                Some(Kill::Drop)
            ]
        );
    }

    #[tokio::test]
    async fn the_tick_pushes_each_paired_device_its_own_attract_view() {
        let (_dir, devices) = directory();
        let reg = Arc::new(Registry::default());
        let (tx, mut one) = mpsc::channel(8);
        let _l = reg.register(ClientClass::Paired, Some("L".into()), tx);
        let (tx, mut other) = mpsc::channel(8);
        let _m = reg.register(ClientClass::Paired, Some("M".into()), tx);
        let (tx, mut phone) = mpsc::channel(8);
        let _p = reg.register(ClientClass::Phone, None, tx);
        let (_events_tx, events_rx) = broadcast::channel(4);
        let job = tokio::spawn(Arc::clone(&reg).fan_out(events_rx, devices, None));
        for rx in [&mut one, &mut other] {
            let Some(Message::Text(t)) =
                tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                    .await
                    .unwrap()
            else {
                panic!("text")
            };
            let FrameBody::Event(EventFrame {
                event: HubEvent::Attract(view),
            }) = arena_proto::parse_frame(t.as_str()).unwrap().body
            else {
                panic!("attract")
            };
            // Neither device is placed in a group: the counter handles it.
            assert_eq!(view.branch_name, "N");
            assert_eq!(view.state, arena_proto::AttractState::AskCounter);
            assert_eq!((view.rate_minor, view.rule), (None, None));
        }
        job.abort();
        assert!(phone.try_recv().is_err(), "a browser class never gets it");
    }
    #[tokio::test]
    async fn the_fan_out_pushes_catalog_changed_after_applied() {
        let (_dir, devices) = directory();
        let reg = Arc::new(Registry::default());
        let (tx, mut paired) = mpsc::channel(8);
        let _p = reg.register(ClientClass::Paired, Some("P".into()), tx);
        let (events_tx, events_rx) = broadcast::channel(4);
        events_tx
            .send(Applied {
                intent_id: "I".into(),
                changes: vec![ChangeMeta {
                    epoch: "1:H".into(),
                    seq: 1,
                    tbl: "products".into(),
                    row_id: "gone".into(),
                    op: ChangeOp::Update,
                }],
            })
            .unwrap();
        drop(events_tx);
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Arc::clone(&reg).fan_out(events_rx, devices, None),
        )
        .await
        .unwrap();
        let mut kinds = Vec::new();
        while let Ok(Message::Text(t)) = paired.try_recv() {
            let FrameBody::Event(EventFrame { event }) =
                arena_proto::parse_frame(t.as_str()).unwrap().body
            else {
                panic!("event")
            };
            kinds.push(match event {
                // The tick may fire between the events.
                HubEvent::Attract(_) => continue,
                HubEvent::Applied(_) => "applied".to_owned(),
                HubEvent::CatalogChanged { products } => {
                    format!(
                        "catalog:{}:{}",
                        products[0].product_id, products[0].available
                    )
                }
                other => panic!("{other:?}"),
            });
        }
        assert_eq!(kinds, ["applied", "catalog:gone:false"]);
    }
}

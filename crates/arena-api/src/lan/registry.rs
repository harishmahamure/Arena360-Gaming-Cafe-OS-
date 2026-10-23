//! Connected LAN sessions, the push fan-out and revocation.
//!
//! Every committed intent from the writer broadcast is pushed to every `paired` session as
//! `event{kind:"applied"}`. Browser classes (`kds`, `phone`) only get the changes to tables on
//! their allow-list ([`browser_tables`], empty for now), and nothing when none remain. Each
//! session has its own bounded queue; a session whose queue is full is dropped, so a slow or
//! failing client never blocks the others. After an intent that touched a `devices` row,
//! sessions of a device that is now revoked get `TOKEN_REVOKED` and are closed. If the fan-out
//! lags behind the writer broadcast, events were lost, so every session is dropped (clients
//! reconnect and resync) after revoked devices got their `TOKEN_REVOKED`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use arena_core::ChangeOp;
use arena_core::{Applied, DeviceDirectory};
use arena_proto::{
    AppError, AppliedSummary, ChangeOpKind, ChangeSummary, ClientClass, ErrorCode, EventFrame,
    Frame, FrameBody, HubEvent,
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
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            sessions: Mutex::default(),
            next_id: AtomicU64::default(),
            next_event: AtomicU64::default(),
            shutting_down: AtomicBool::default(),
            presence: watch::channel(0).0,
        }
    }
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
        if removed.is_some_and(|e| e.class == ClientClass::Paired && e.device_id.is_some()) {
            self.registry.presence.send_modify(|n| *n += 1);
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
    ) {
        loop {
            match events.recv().await {
                Ok(applied) => {
                    self.push_applied(&applied);
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
            Arc::clone(&reg).fan_out(events_rx, devices),
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
}

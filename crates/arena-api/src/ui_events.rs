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
//! - `event: devices.changed`, data `{}`: a device came online or went offline on the LAN; the
//!   view reloads the snapshot (which carries each device's `online`).
//! - `event: device.admin_unlock_ack`, data
//!   [`AdminUnlockAckEvent`](arena_proto::AdminUnlockAckEvent): a kiosk granted a remote unlock
//!   (the floor waits up to 5 s for it).
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
use arena_proto::{AppliedEvent, SettingsChangedEvent, SnapshotLocale, SnapshotUi};
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

fn role_of(status: &Option<watch::Receiver<HubStatus>>) -> Option<Role> {
    status.as_ref().map(|s| s.borrow().role)
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

struct Stream {
    auth: Arc<UiAuth>,
    token: String,
    events: broadcast::Receiver<Pushed>,
    ends: Option<watch::Receiver<u64>>,
    recheck: tokio::time::Interval,
    last_full: Instant,
    closing: bool,
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
    let state = Stream {
        events: hub.subscribe(),
        ends: Some(auth.session_ends()),
        auth,
        token,
        recheck,
        last_full: Instant::now(),
        closing: false,
    };
    let body = stream::unfold(state, |mut st| async move {
        loop {
            if st.closing {
                return None;
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
    use tower::ServiceExt;

    type Frames = std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Send>,
    >;

    async fn open(hub: &Hub, token: Option<&str>) -> (StatusCode, Frames) {
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
        assert_eq!(data["ui"], serde_json::json!({"idle_lock_minutes": 10}));
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

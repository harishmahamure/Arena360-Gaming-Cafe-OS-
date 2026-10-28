//! One LAN session: the `hello` handshake, then frames until either side closes.
//!
//! **Handshake.** The first frame must be `hello`; anything else (or nothing within
//! [`HELLO_TIMEOUT`]) gets an error frame and a close. `paired` is accepted over TLS only;
//! `kds` and `phone` on `:7401/ws` only. `paired` and `kds` present a device token, checked
//! with argon2id against `devices` (`TOKEN_INVALID` / `TOKEN_REVOKED`, then closed); `phone`
//! is anonymous. A paired client must be in the Hub's major release and at most two minors
//! behind (`UNSUPPORTED` naming the minimum, then closed). A changed reported version is
//! recorded with `device.version_reported` as the system actor.
//!
//! **After hello.** Errors never close the session: an unparseable frame gets `BAD_FRAME`, an
//! unknown kind `UNSUPPORTED`. `intent.submit` goes to the writer with the session's actor
//! (`{device, device_id}`), overwriting any actor in the frame and dropping any `elevated_by`.
//! A follower refuses every intent with `NOT_PRIMARY`; an intent outside the class scope gets
//! `FORBIDDEN` and a `warn` line. `settings.get{keys}` answers `settings.values` on a primary or
//! a follower, for paired sessions and `kiosk.*` keys only (anything else is `FORBIDDEN`, the
//! whole request); keys the branch lacks are left out.
//!
//! **Admin exit (Story 1.20).** A paired session gets the manager verifier as
//! `event{kind:"verifier"}` right after `hello.ok`. `kiosk.verify_admin_pin{pin_proof}` checks a
//! manager PIN (argon2id, active staff whose role allows `device.remote`, at most 50) and answers
//! `kiosk.admin_pin_ok{staff_id, verifier_version}` or `PIN_REJECTED`; five wrong PINs in a row
//! pause that device for 60 s (`AUTH_LOCKED`). The PIN is never logged. `kiosk.admin_unlock_ack
//! {exit_id}` (the kiosk granted a remote unlock of its own) answers `ack` and tells the Hub UI.
//! Only `device.admin_exit` keeps the kiosk's `elevated_by` (apply() checks it); every other LAN
//! intent has it dropped.
//!
//! **Player login (Story 6.8).** `player.authenticate{username, password}` (paired sessions only)
//! answers `player.auth_result{outcome}`; see [`super::player_login`]. The credentials are never
//! logged.
//!
//! **Queues.** Replies and pushes share one bounded queue drained by the session's writer
//! task, so a slow client only ever stalls itself; the fan-out drops a session whose queue is
//! full.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{WriterHandle, devices::VerifiedDevice};
use arena_proto::{
    Actor, ActorKind, AppError, ClientClass, DeviceVersionReported, ErrorCode, Frame, FrameBody,
    Hello, HelloOk, Intent, IntentKind, IntentPayload, IntentResult, MAX_SETTINGS_GET_KEYS,
    SettingsValues, Versions, check_envelope, in_scope, may_read_setting, parse_frame,
    parse_frame_value, parse_json,
};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use super::registry::{Kill, revoked_error, set_kill, summary};
use super::{LanHub, Release};

/// How long a new connection may take to send `hello`.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Each session's outbound queue (replies and pushes).
pub const SESSION_QUEUE: usize = 256;
/// The largest accepted frame; a larger one gets `BAD_FRAME` and the session is closed (the
/// WebSocket cannot carry on after an oversized message).
pub const MAX_FRAME_BYTES: usize = 1 << 20;
/// After hello the Hub sends a `ping` frame this often.
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
/// A session with no inbound frame for this long is closed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
/// Concurrent argon2id token checks across the whole Hub.
pub const VERIFY_CONCURRENCY: usize = 4;
/// Intents one session may have in flight at once.
const MAX_IN_FLIGHT: usize = 16;
/// How long a closing session may spend sending its last frames.
const CLOSE_GRACE: Duration = Duration::from_secs(1);
/// The system actor the LAN layer submits its own intents as.
pub const LAN_SYSTEM_ACTOR: &str = "hub-lan";

/// How the connection reached the Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    /// TLS on `:7400` or `:7401`.
    Tls,
    /// Plain HTTP `GET /ws` on `:7401`.
    Plain,
}

type Sink<S> = SplitSink<WebSocketStream<S>, Message>;
type Stream<S> = SplitStream<WebSocketStream<S>>;

fn text(frame: &Frame) -> Message {
    Message::text(frame.to_text())
}

fn app(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

/// Sends `frame` and a close, within [`CLOSE_GRACE`].
async fn reject<S>(sink: &mut Sink<S>, frame: Frame)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let _ = timeout(CLOSE_GRACE, async {
        let _ = sink.send(text(&frame)).await;
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "".into(),
            })))
            .await;
    })
    .await;
}

fn too_large() -> AppError {
    app(
        ErrorCode::BadFrame,
        format!("frames are at most {MAX_FRAME_BYTES} bytes"),
    )
}

/// The first text frame; `Err` with the error to reject a binary or oversized one with,
/// `None` when the peer went away.
async fn first_text<S>(stream: &mut Stream<S>) -> Option<Result<String, AppError>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match stream.next().await? {
            Ok(Message::Text(t)) => return Some(Ok(t.as_str().to_owned())),
            Ok(Message::Binary(_)) => {
                return Some(Err(app(
                    ErrorCode::BadFrame,
                    "frames must be UTF-8 JSON text",
                )));
            }
            Err(WsError::Capacity(_)) => return Some(Err(too_large())),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// Parses the first frame, which must be `hello`. `Err` is the frame to reject with.
fn parse_hello(text: &str) -> Result<(String, Hello), Box<Frame>> {
    let frame = parse_frame(text).map_err(|e| Box::new(e.to_frame()))?;
    match frame.body {
        FrameBody::Hello(h) => Ok((frame.rid, h)),
        other => Err(Box::new(Frame::error(
            frame.rid,
            app(
                ErrorCode::BadFrame,
                format!("the first frame must be hello, not {}", other.kind()),
            ),
        ))),
    }
}

/// Who the session is.
struct Authed {
    class: ClientClass,
    device: Option<VerifiedDevice>,
    actor: Actor,
}

impl Authed {
    fn device_id(&self) -> Option<&str> {
        self.device.as_ref().map(|d| d.device_id.as_str())
    }
}

async fn authenticate(
    hub: &LanHub,
    transport: Transport,
    peer: SocketAddr,
    hello: &Hello,
) -> Result<Authed, AppError> {
    let class = hello.class;
    if hub.throttle.is_blocked(peer.ip(), Instant::now()) {
        return Err(app(
            ErrorCode::TokenInvalid,
            "the device token is not valid",
        ));
    }
    match (class, transport) {
        (ClientClass::Paired, Transport::Tls)
        | (ClientClass::Kds | ClientClass::Phone, Transport::Plain) => {}
        (ClientClass::Paired, Transport::Plain) => {
            return Err(app(
                ErrorCode::Forbidden,
                "paired clients must connect over TLS",
            ));
        }
        (ClientClass::Kds | ClientClass::Phone, Transport::Tls) => {
            return Err(app(
                ErrorCode::Forbidden,
                "kds and phone clients connect on :7401/ws",
            ));
        }
    }
    let device = if class.uses_device_token() {
        let dir = Arc::clone(&hub.devices);
        let credential = hello.credential.clone();
        let _permit = Arc::clone(&hub.verify_permits)
            .acquire_owned()
            .await
            .map_err(|_| app(ErrorCode::Internal, "token checks are shut down"))?;
        let verified = tokio::task::spawn_blocking(move || dir.verify(class, &credential))
            .await
            .map_err(|e| app(ErrorCode::Internal, format!("token check failed: {e}")))?;
        match verified {
            Ok(v) => Some(v),
            Err(e) => {
                if matches!(e.code, ErrorCode::TokenInvalid | ErrorCode::TokenRevoked) {
                    hub.throttle.record_failure(peer.ip(), Instant::now());
                }
                return Err(e);
            }
        }
    } else {
        None
    };
    if class == ClientClass::Paired {
        let hub_release = Release::parse(&hub.version)
            .ok_or_else(|| app(ErrorCode::Internal, "the hub version is not x.y.z"))?;
        let client = Release::parse(&hello.versions.app).ok_or_else(|| {
            AppError::with_field(
                ErrorCode::BadFrame,
                "versions.app",
                "versions.app must be x.y.z",
            )
        })?;
        if !hub_release.accepts(client) {
            return Err(app(
                ErrorCode::Unsupported,
                format!(
                    "app version {client} is not supported by hub {hub_release}: minimum {}",
                    hub_release.minimum_client()
                ),
            ));
        }
    }
    let actor = match &device {
        Some(d) => Actor {
            kind: ActorKind::Device,
            id: d.device_id.clone(),
        },
        None => Actor {
            kind: ActorKind::Device,
            id: format!("phone:{}", ulid::Ulid::generate()),
        },
    };
    Ok(Authed {
        class,
        device,
        actor,
    })
}

/// Records the device's reported version when it changed (system actor, best effort).
fn report_version(hub: &LanHub, authed: &Authed, version: &str) {
    let (Some(device), Some(writer)) = (&authed.device, &hub.writer) else {
        return;
    };
    if !hub.is_primary()
        || device.reported_version.as_deref() == Some(version)
        || Release::parse(version).is_none()
    {
        return;
    }
    let now = hub.clock.now_ms();
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::DeviceVersionReported,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        // Two sessions of one device racing to report: only the first applies; the other is
        // STALE and ignored.
        base_rev: Some(device.rev),
        actor: Actor {
            kind: ActorKind::System,
            id: LAN_SYSTEM_ACTOR.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::DeviceVersionReported(DeviceVersionReported {
            device_id: device.device_id.clone(),
            version: version.to_owned(),
        }),
    };
    let writer = writer.clone();
    let device_id = device.device_id.clone();
    tokio::spawn(async move {
        match writer.submit_intent(intent).await {
            Ok(_) => {}
            Err(e) if e.code == ErrorCode::Stale => {
                tracing::debug!(device_id = %device_id, "version already reported by another session");
            }
            Err(e) => {
                tracing::warn!(device_id = %device_id, code = e.code.as_str(), "could not record the reported version");
            }
        }
    });
}

/// Drains the session's queue into the socket until it is killed or the queue closes. On a
/// kill it sends the kill's error frame (if any) and a close within [`CLOSE_GRACE`].
async fn write_loop<S>(
    mut sink: Sink<S>,
    mut out: mpsc::Receiver<Message>,
    mut kill: watch::Receiver<Option<Kill>>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let msg = tokio::select! {
            biased;
            changed = kill.changed() => return finish(&mut sink, &kill, changed.is_ok()).await,
            msg = out.recv() => msg,
        };
        let Some(msg) = msg else {
            return finish(&mut sink, &kill, false).await;
        };
        tokio::select! {
            biased;
            changed = kill.changed() => return finish(&mut sink, &kill, changed.is_ok()).await,
            sent = sink.send(msg) => if sent.is_err() { return; },
        }
    }
}

/// Sends the kill's error frame (when `killed` and it has one) and a close.
async fn finish<S>(sink: &mut Sink<S>, kill: &watch::Receiver<Option<Kill>>, killed: bool)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let reason = if killed { kill.borrow().clone() } else { None };
    let _ = timeout(CLOSE_GRACE, async {
        if let Some(Kill::Error(e)) = reason {
            let _ = sink.send(text(&Frame::error("", e))).await;
        }
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "".into(),
            })))
            .await;
        let _ = sink.close().await;
    })
    .await;
}

/// Runs a session on an upgraded WebSocket until it ends.
pub(crate) async fn run<S>(
    ws: WebSocketStream<S>,
    transport: Transport,
    peer: SocketAddr,
    hub: Arc<LanHub>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = ws.split();
    let first = match timeout(HELLO_TIMEOUT, first_text(&mut stream)).await {
        Err(_) => {
            let e = app(ErrorCode::BadFrame, "no hello within 10 s");
            return reject(&mut sink, Frame::error("", e)).await;
        }
        Ok(None) => return,
        Ok(Some(Err(e))) => return reject(&mut sink, Frame::error("", e)).await,
        Ok(Some(Ok(text))) => text,
    };
    let (rid, hello) = match parse_hello(&first) {
        Ok(h) => h,
        Err(frame) => return reject(&mut sink, *frame).await,
    };
    let authed = match authenticate(&hub, transport, peer, &hello).await {
        Ok(a) => a,
        Err(e) => {
            tracing::info!(
                class = hello.class.as_str(),
                code = e.code.as_str(),
                "hello refused"
            );
            return reject(&mut sink, Frame::error(rid, e)).await;
        }
    };

    // Register before hello.ok so no push committed after it is missed, then re-check the
    // token: a revocation committed between the check and the registration is caught here.
    let (out_tx, out_rx) = mpsc::channel::<Message>(SESSION_QUEUE);
    let guard = hub.registry.register(
        authed.class,
        authed.device_id().map(str::to_owned),
        out_tx.clone(),
    );
    // Receivers created inside `register`, before the session was visible: a kill set at any
    // point after registration is seen (cloning keeps the unseen change).
    let kill_tx = Arc::clone(&guard.kill);
    let writer_kill = guard.writer_kill.clone();
    let mut kill = guard.reader_kill.clone();
    if let Some(id) = authed.device_id() {
        let dir = Arc::clone(&hub.devices);
        let id = id.to_owned();
        match tokio::task::spawn_blocking(move || dir.is_revoked(&id)).await {
            Ok(Ok(false)) => {}
            Ok(Ok(true)) => return reject(&mut sink, Frame::error(rid, revoked_error())).await,
            Ok(Err(_)) | Err(_) => {
                let e = app(ErrorCode::Internal, "could not check the device");
                return reject(&mut sink, Frame::error(rid, e)).await;
            }
        }
    }
    // Killed already (shutdown, or a revoke that landed before we subscribed was caught by
    // the re-check above): end without hello.ok.
    let early = kill.borrow().clone();
    if let Some(reason) = early {
        let e = match reason {
            Kill::Error(e) => e,
            Kill::Drop => app(ErrorCode::Internal, "the hub is closing"),
        };
        return reject(&mut sink, Frame::error(rid, e)).await;
    }
    let ok = Frame::new(
        rid,
        FrameBody::HelloOk(HelloOk {
            class: authed.class,
            device_id: authed.device_id().map(str::to_owned),
            versions: Versions {
                app: hub.version.clone(),
            },
            primary: hub.is_primary(),
        }),
    );
    if sink.send(text(&ok)).await.is_err() {
        return;
    }
    tracing::info!(
        class = authed.class.as_str(),
        device_id = authed.device_id().unwrap_or("-"),
        "lan session open"
    );
    report_version(&hub, &authed, &hello.versions.app);
    if authed.class == ClientClass::Paired && authed.device.is_some() {
        let dir = Arc::clone(&hub.devices);
        match tokio::task::spawn_blocking(move || dir.manager_verifier()).await {
            Ok(Ok(v)) => {
                let _ = out_tx.try_send(super::registry::verifier_message("ev-hello".into(), v));
            }
            Ok(Err(e)) => tracing::warn!(
                code = e.code.as_str(),
                "could not read the manager verifier"
            ),
            Err(e) => tracing::warn!(error = %e, "manager verifier read panicked"),
        }
    }

    let mut writer = tokio::spawn(write_loop(sink, out_rx, writer_kill));
    let permits = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let mut tasks = JoinSet::new();
    let ctx = Ctx {
        hub: &hub,
        authed: &authed,
        out: &out_tx,
    };
    let mut keepalive = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_inbound = Instant::now();
    let mut pings = 0u64;
    loop {
        tokio::select! {
            _ = kill.changed() => break,
            _ = &mut writer => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
            _ = keepalive.tick() => {
                if last_inbound.elapsed() >= IDLE_TIMEOUT {
                    tracing::info!(device_id = authed.device_id().unwrap_or("-"), "lan session idle; closing");
                    break;
                }
                pings += 1;
                let ping = Frame::new(format!("ka-{pings}"), FrameBody::Ping);
                // A full queue means the client is not reading; the fan-out will drop it.
                let _ = out_tx.try_send(text(&ping));
            }
            msg = stream.next() => {
                last_inbound = Instant::now();
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        ctx.handle(t.as_str(), &permits, &mut tasks).await;
                    }
                    Some(Ok(Message::Binary(_))) => {
                        let e = app(ErrorCode::BadFrame, "frames must be UTF-8 JSON text");
                        ctx.reply(Frame::error("", e)).await;
                    }
                    Some(Err(WsError::Capacity(_))) => {
                        set_kill(&kill_tx, Kill::Error(too_large()));
                        break;
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    // Let in-flight intents reply (they may already be committed), then give up on them.
    let _ = timeout(CLOSE_GRACE, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    if !writer.is_finished() {
        // A reason already set (e.g. TOKEN_REVOKED) is kept: the writer task sends its frame.
        set_kill(&kill_tx, Kill::Drop);
        if timeout(CLOSE_GRACE * 2, &mut writer).await.is_err() {
            writer.abort();
        }
    }
    drop(guard);
    tracing::info!(
        class = authed.class.as_str(),
        device_id = authed.device_id().unwrap_or("-"),
        "lan session closed"
    );
}

/// A free in-flight slot, or `HUB_SLOW` when the session already has [`MAX_IN_FLIGHT`]
/// intents waiting (never waits, so kills are still handled).
fn try_permit(permits: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit, AppError> {
    Arc::clone(permits).try_acquire_owned().map_err(|_| {
        app(
            ErrorCode::HubSlow,
            "too many intents in flight on this session",
        )
    })
}

struct Ctx<'a> {
    hub: &'a Arc<LanHub>,
    authed: &'a Authed,
    out: &'a mpsc::Sender<Message>,
}

impl Ctx<'_> {
    async fn reply(&self, frame: Frame) {
        let _ = self.out.send(text(&frame)).await;
    }

    async fn handle(&self, text: &str, permits: &Arc<Semaphore>, tasks: &mut JoinSet<()>) {
        let value = match parse_json(text) {
            Ok(v) => v,
            Err(e) => return self.reply(e.to_frame()).await,
        };
        let env = match check_envelope(&value) {
            Ok(env) => env,
            Err(e) => return self.reply(e.to_frame()).await,
        };
        let rid = env.rid.to_owned();
        match env.kind {
            "ping" => self.reply(Frame::new(rid, FrameBody::Pong)).await,
            "pong" => {}
            "hello" => {
                let e = app(ErrorCode::BadFrame, "hello was already accepted");
                self.reply(Frame::error(rid, e)).await;
            }
            "intent.submit" => match self.check_intent(&rid, value) {
                Ok(intent) => self.spawn_submit(rid, intent, permits, tasks).await,
                Err(e) => self.reply(Frame::error(rid, e)).await,
            },
            "settings.get" => match self.check_settings_get(value) {
                Ok(keys) => self.spawn_settings_get(rid, keys, permits, tasks).await,
                Err(e) => self.reply(Frame::error(rid, e)).await,
            },
            "kiosk.verify_admin_pin" => match self.check_verify_pin(value) {
                Ok((device, pin)) => {
                    self.spawn_verify_pin(rid, device, pin, permits, tasks)
                        .await
                }
                Err(e) => self.reply(Frame::error(rid, e)).await,
            },
            "kiosk.admin_unlock_ack" => match self.check_unlock_ack(value) {
                Ok((device, exit_id)) => {
                    self.spawn_unlock_ack(rid, device, exit_id, permits, tasks)
                        .await
                }
                Err(e) => self.reply(Frame::error(rid, e)).await,
            },
            "player.authenticate" => match self.check_player_authenticate(value) {
                Ok((device, request)) => {
                    self.spawn_player_authenticate(rid, device, request, permits, tasks)
                        .await
                }
                Err(e) => self.reply(Frame::error(rid, e)).await,
            },
            "kiosk.pc_ack" => {
                let reply = match self.check_pc_ack(value) {
                    Ok(()) => Frame::new(rid, FrameBody::Ack),
                    Err(e) => Frame::error(rid, e),
                };
                self.reply(reply).await;
            }
            other => {
                let e = app(
                    ErrorCode::Unsupported,
                    format!("{other} is not accepted from clients"),
                );
                self.reply(Frame::error(rid, e)).await;
            }
        }
    }

    /// The gate in front of the writer: primary, then class scope (on both the envelope and
    /// the payload kind, read before the body is parsed), then a handler, then the typed parse.
    /// The session's actor replaces whatever the frame carried.
    fn check_intent(&self, rid: &str, value: Value) -> Result<Intent, AppError> {
        if !self.hub.is_primary() {
            return Err(app(
                ErrorCode::NotPrimary,
                "this hub is not the primary writer",
            ));
        }
        let class = self.authed.class;
        for pointer in ["/intent/kind", "/intent/payload/kind"] {
            let Some(raw) = value.pointer(pointer).and_then(Value::as_str) else {
                continue;
            };
            let Some(kind) = IntentKind::parse(raw) else {
                return Err(app(
                    ErrorCode::Unsupported,
                    format!("unknown intent kind {raw:?}"),
                ));
            };
            if !in_scope(class, kind) {
                tracing::warn!(
                    class = class.as_str(),
                    kind = kind.as_str(),
                    device_id = self.authed.device_id().unwrap_or("-"),
                    "intent outside the session's class scope"
                );
                return Err(app(
                    ErrorCode::Forbidden,
                    format!(
                        "{} is not permitted for {} clients",
                        kind.as_str(),
                        class.as_str()
                    ),
                ));
            }
            if !kind.is_handled() {
                return Err(app(
                    ErrorCode::Unsupported,
                    format!("{} has no handler yet", kind.as_str()),
                ));
            }
        }
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::IntentSubmit(submit) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed intent.submit"));
        };
        debug_assert_eq!(frame.rid, rid);
        // A chatty kiosk cannot flood the change log: 60 events a minute per device, kiosk.log
        // and kiosk.process_blocked together (a `kiosk.scan_reported` is one small event a day, so
        // the limit does not apply to it). Over the limit is HUB_SLOW, which the kiosk treats
        // as transient (it keeps the event queued).
        if matches!(
            submit.intent.payload.kind(),
            IntentKind::KioskLog | IntentKind::KioskProcessBlocked
        ) && !self
            .hub
            .kiosk_log_rate
            .take(self.authed.device_id().unwrap_or("-"))
        {
            return Err(app(
                ErrorCode::HubSlow,
                "too many kiosk events from this device; retry later",
            ));
        }
        if let arena_proto::IntentPayload::DeviceTestSessionCompleted(c) = &submit.intent.payload
            && self.authed.device_id() != Some(c.device_id.as_str())
        {
            return Err(app(
                ErrorCode::Forbidden,
                "a kiosk completes only its own test session",
            ));
        }
        Ok(bind_to_session(submit.intent, &self.authed.actor))
    }

    /// The keys of a `settings.get` this session may read: paired only, 1–32 `kiosk.*` keys,
    /// `branch.fulfilment`, `branch.tz`, the kiosk billing keys or the session's own `display.hc.<device_id>`.
    fn check_settings_get(&self, value: Value) -> Result<Vec<String>, AppError> {
        let class = self.authed.class;
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::SettingsGet(get) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed settings.get"));
        };
        if get.keys.is_empty() || get.keys.len() > MAX_SETTINGS_GET_KEYS {
            return Err(AppError::with_field(
                ErrorCode::BadFrame,
                "keys",
                format!("keys must list 1–{MAX_SETTINGS_GET_KEYS} settings"),
            ));
        }
        if let Some(key) = get
            .keys
            .iter()
            .find(|k| !may_read_setting(class, k, self.authed.device_id()))
        {
            tracing::warn!(
                class = class.as_str(),
                device_id = self.authed.device_id().unwrap_or("-"),
                "settings.get outside the session's class scope"
            );
            let key: String = key.chars().take(64).collect();
            return Err(app(
                ErrorCode::Forbidden,
                format!("{key:?} may not be read by {} clients", class.as_str()),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut keys = get.keys;
        keys.retain(|k| seen.insert(k.clone()));
        Ok(keys)
    }

    async fn spawn_settings_get(
        &self,
        rid: String,
        keys: Vec<String>,
        permits: &Arc<Semaphore>,
        tasks: &mut JoinSet<()>,
    ) {
        let permit = match try_permit(permits) {
            Ok(p) => p,
            Err(e) => return self.reply(Frame::error(rid, e)).await,
        };
        let dir = Arc::clone(&self.hub.devices);
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let read = tokio::task::spawn_blocking(move || dir.settings(&keys)).await;
            let reply = match read {
                Ok(Ok(values)) => {
                    Frame::new(rid, FrameBody::SettingsValues(SettingsValues { values }))
                }
                Ok(Err(e)) => Frame::error(rid, e),
                Err(e) => Frame::error(
                    rid,
                    app(ErrorCode::Internal, format!("settings read failed: {e}")),
                ),
            };
            let _ = out.send(text(&reply)).await;
        });
    }

    /// The paired device asking, or `FORBIDDEN`.
    fn paired_device(&self, what: &str) -> Result<String, AppError> {
        match (self.authed.class, self.authed.device_id()) {
            (ClientClass::Paired, Some(id)) => Ok(id.to_owned()),
            _ => Err(app(
                ErrorCode::Forbidden,
                format!("{what} is for paired kiosks only"),
            )),
        }
    }

    fn check_verify_pin(&self, value: Value) -> Result<(String, String), AppError> {
        let device = self.paired_device("kiosk.verify_admin_pin")?;
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::VerifyAdminPin(v) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed kiosk.verify_admin_pin"));
        };
        Ok((device, v.pin_proof))
    }

    fn check_player_authenticate(
        &self,
        value: Value,
    ) -> Result<(String, arena_proto::PlayerAuthenticate), AppError> {
        let device = self.paired_device("player.authenticate")?;
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::PlayerAuthenticate(request) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed player.authenticate"));
        };
        request.validate()?;
        Ok((device, request))
    }

    async fn spawn_player_authenticate(
        &self,
        rid: String,
        device: String,
        request: arena_proto::PlayerAuthenticate,
        permits: &Arc<Semaphore>,
        tasks: &mut JoinSet<()>,
    ) {
        let permit = match try_permit(permits) {
            Ok(p) => p,
            Err(e) => return self.reply(Frame::error(rid, e)).await,
        };
        let hub = Arc::clone(self.hub);
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let reply = match super::player_login::authenticate_player(&hub, &device, request).await
            {
                Ok(result) => Frame::new(rid, FrameBody::PlayerAuthResult(result)),
                Err(e) => Frame::error(rid, e),
            };
            let _ = out.send(text(&reply)).await;
        });
    }

    fn check_unlock_ack(&self, value: Value) -> Result<(String, String), AppError> {
        let device = self.paired_device("kiosk.admin_unlock_ack")?;
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::AdminUnlockAck(a) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed kiosk.admin_unlock_ack"));
        };
        Ok((device, a.exit_id))
    }

    fn check_pc_ack(&self, value: Value) -> Result<(), AppError> {
        let device = self.paired_device("kiosk.pc_ack")?;
        let frame = parse_frame_value(value).map_err(|e| e.error)?;
        let FrameBody::PcAck(a) = frame.body else {
            return Err(app(ErrorCode::BadFrame, "malformed kiosk.pc_ack"));
        };
        if self.hub.registry.ack_pc(&device, &a.cmd_id) {
            return Ok(());
        }
        Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "cmd_id",
            "no pending command of this device has that id",
        ))
    }

    async fn spawn_verify_pin(
        &self,
        rid: String,
        device: String,
        pin: String,
        permits: &Arc<Semaphore>,
        tasks: &mut JoinSet<()>,
    ) {
        let permit = match try_permit(permits) {
            Ok(p) => p,
            Err(e) => return self.reply(Frame::error(rid, e)).await,
        };
        let hub = Arc::clone(self.hub);
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let reply = match verify_admin_pin(&hub, &device, pin).await {
                Ok(ok) => Frame::new(rid, FrameBody::AdminPinOk(ok)),
                Err(e) => Frame::error(rid, e),
            };
            let _ = out.send(text(&reply)).await;
        });
    }

    async fn spawn_unlock_ack(
        &self,
        rid: String,
        device: String,
        exit_id: String,
        permits: &Arc<Semaphore>,
        tasks: &mut JoinSet<()>,
    ) {
        let permit = match try_permit(permits) {
            Ok(p) => p,
            Err(e) => return self.reply(Frame::error(rid, e)).await,
        };
        let hub = Arc::clone(self.hub);
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let dir = Arc::clone(&hub.devices);
            let id = exit_id.clone();
            let row = tokio::task::spawn_blocking(move || dir.admin_exit(&id)).await;
            let reply = match row {
                Ok(Ok(Some(r)))
                    if r.device_id == device
                        && r.mode == arena_proto::AdminExitMode::Remote.as_str() =>
                {
                    tracing::info!(exit_id = %exit_id, device_id = %device, "remote unlock acknowledged");
                    hub.registry.ack_admin_unlock(arena_proto::AdminUnlockAckEvent {
                        exit_id,
                        device_id: device,
                    });
                    Frame::new(rid, FrameBody::Ack)
                }
                Ok(Ok(_)) => Frame::error(
                    rid,
                    AppError::with_field(
                        ErrorCode::ValidationFailed,
                        "exit_id",
                        "no remote unlock of this device has that id",
                    ),
                ),
                Ok(Err(e)) => Frame::error(rid, e),
                Err(e) => Frame::error(
                    rid,
                    app(ErrorCode::Internal, format!("exit read failed: {e}")),
                ),
            };
            let _ = out.send(text(&reply)).await;
        });
    }

    async fn spawn_submit(
        &self,
        rid: String,
        intent: Intent,
        permits: &Arc<Semaphore>,
        tasks: &mut JoinSet<()>,
    ) {
        let permit = match try_permit(permits) {
            Ok(p) => p,
            Err(e) => return self.reply(Frame::error(rid, e)).await,
        };
        // A stored scan is matched to the catalogue once it is applied (Story 6.5).
        let scan = super::scan_match::StoredScan::of(&intent);
        let hub = Arc::clone(self.hub);
        let writer: Option<WriterHandle> = self.hub.writer.clone();
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let reply = match writer {
                Some(w) => match w.submit_intent(intent).await {
                    Ok(applied) => {
                        if let Some(scan) = scan {
                            super::scan_match::spawn(&hub, scan);
                        }
                        Frame::new(
                            rid,
                            FrameBody::IntentResult(IntentResult {
                                applied: summary(&applied),
                            }),
                        )
                    }
                    Err(e) => Frame::error(rid, e),
                },
                None => Frame::error(
                    rid,
                    app(ErrorCode::NotPrimary, "this hub is not the primary writer"),
                ),
            };
            let _ = out.send(text(&reply)).await;
        });
    }
}

/// What a LAN `intent.submit` becomes: the session's actor replaces the frame's, and any
/// `elevated_by` is dropped (only the Hub's loopback ticket path may elevate) — except on
/// `device.admin_exit`, whose elevation is the manager PIN the kiosk checked (with the Hub, or
/// offline against the cached verifier); apply() re-checks it.
fn bind_to_session(mut intent: Intent, actor: &Actor) -> Intent {
    intent.actor = actor.clone();
    if intent.payload.kind().elevator_permission().is_none() {
        intent.elevated_by = None;
    }
    intent
}

/// Checks a kiosk's admin-exit PIN: the device's throttle, then argon2id against the active
/// staff whose role allows `device.remote` (never logged).
async fn verify_admin_pin(
    hub: &LanHub,
    device: &str,
    pin: String,
) -> Result<arena_proto::AdminPinOk, AppError> {
    let Some(staff) = hub.staff.clone() else {
        return Err(app(
            ErrorCode::Unsupported,
            "this hub does not check admin-exit PINs",
        ));
    };
    // Reserved before any check, so parallel attempts count toward the limit.
    if let Err(s) = hub.admin_pin.begin(device, Instant::now()) {
        return Err(app(
            ErrorCode::AuthLocked,
            format!("too many wrong PINs; try again in {s} s"),
        ));
    }
    let checked = async {
        let _permit = Arc::clone(&hub.verify_permits)
            .acquire_owned()
            .await
            .map_err(|_| app(ErrorCode::Internal, "PIN checks are shut down"))?;
        tokio::task::spawn_blocking(move || {
            staff.match_pin(&[arena_proto::Permission::DeviceRemote], "", &pin)
        })
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("PIN check failed: {e}")))?
    }
    .await;
    let matched = match checked {
        Ok(m) => m,
        Err(e) => {
            // A check that could not run counts neither way.
            hub.admin_pin.cancel(device);
            return Err(e);
        }
    };
    let Some((staff_id, _)) = matched else {
        let paused = hub.admin_pin.finish(device, false, Instant::now());
        tracing::warn!(device_id = %device, paused = paused.is_some(), "admin-exit PIN refused");
        return Err(match paused {
            Some(s) => app(
                ErrorCode::AuthLocked,
                format!("too many wrong PINs; try again in {s} s"),
            ),
            None => arena_core::staff::pin_rejected(),
        });
    };
    hub.admin_pin.finish(device, true, Instant::now());
    let dir = Arc::clone(&hub.devices);
    let verifier = tokio::task::spawn_blocking(move || dir.manager_verifier())
        .await
        .map_err(|e| app(ErrorCode::Internal, format!("verifier read failed: {e}")))??;
    tracing::info!(device_id = %device, staff_id = %staff_id, "admin-exit PIN accepted");
    Ok(arena_proto::AdminPinOk {
        staff_id,
        verifier_version: verifier.version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::HubCert;

    #[test]
    fn a_forged_elevation_is_dropped_and_the_actor_replaced() {
        let device = Actor {
            kind: ActorKind::Device,
            id: "01JDEVICE".into(),
        };
        let forged = Intent {
            intent_id: ulid::Ulid::generate().to_string(),
            kind: IntentKind::DeviceRevoke,
            idempotency_key: ulid::Ulid::generate().to_string(),
            issued_at: 0,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::Staff,
                id: "01JCOUNTER".into(),
            },
            elevated_by: Some(arena_proto::ElevatedBy {
                kind: arena_proto::ElevationKind::Pin,
                id: "01JMANAGER".into(),
                mode: arena_proto::ElevationMode::Hub,
                verifier_version: 1,
            }),
            reason: Some(arena_proto::ReasonCode::Lost),
            payload: IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke {
                device_id: "01JOTHER".into(),
            }),
        };
        let bound = bind_to_session(forged.clone(), &device);
        assert_eq!(bound.elevated_by, None);
        // The kiosk's admin exit keeps its PIN elevation for apply() to check.
        let exit = Intent {
            kind: IntentKind::DeviceAdminExit,
            payload: IntentPayload::DeviceAdminExit(arena_proto::DeviceAdminExit {
                exit_id: "01JAAAAAAAAAAAAAAAAAAAAAAA".into(),
                mode: arena_proto::AdminExitMode::CachedVerifier,
            }),
            ..forged.clone()
        };
        let kept = bind_to_session(exit.clone(), &device);
        assert_eq!(kept.elevated_by, exit.elevated_by);
        assert_eq!(kept.actor, device);
        assert_eq!(bound.actor, device);
        assert_eq!(bound.reason, forged.reason);
        assert_eq!(bound.payload, forged.payload);
    }
    use arena_core::{DeviceDirectory, HubStatus, ManualClock, status_channel};
    use arena_proto::parse_frame;

    /// A follower hub on an empty database (enough for phone sessions).
    fn hub() -> (tempfile::TempDir, Arc<LanHub>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        drop(arena_store::Store::open(&path, 0).unwrap());
        let (_tx, rx) = status_channel(HubStatus::opened(1, "H"));
        let hub = LanHub::new(
            None,
            rx,
            Arc::new(ManualClock::new(0)),
            Arc::new(DeviceDirectory::open(&path).unwrap()),
            HubCert::generate(None).unwrap().server_config().unwrap(),
            "0.1.0",
        );
        (dir, Arc::new(hub))
    }

    type Client = WebSocketStream<tokio::io::DuplexStream>;

    /// A plain session over an in-memory pipe.
    async fn connect(hub: &Arc<LanHub>) -> Client {
        let (server, client) = tokio::io::duplex(4 << 20);
        let hub = Arc::clone(hub);
        tokio::spawn(async move {
            let config = Some(crate::lan::listener::ws_config());
            let ws = tokio_tungstenite::accept_async_with_config(server, config)
                .await
                .unwrap();
            run(ws, Transport::Plain, "127.0.0.1:1".parse().unwrap(), hub).await;
        });
        tokio_tungstenite::client_async("ws://hub/ws", client)
            .await
            .unwrap()
            .0
    }

    /// The next text frame, or `None` once closed.
    async fn next(ws: &mut Client) -> Option<Frame> {
        loop {
            match ws.next().await? {
                Ok(Message::Text(t)) => return Some(parse_frame(t.as_str()).unwrap()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    fn code(f: &Frame) -> ErrorCode {
        match &f.body {
            FrameBody::Error(e) => e.error.code,
            other => panic!("{other:?}"),
        }
    }

    async fn phone_hello(ws: &mut Client) {
        let hello = Frame::new(
            "h",
            FrameBody::Hello(Hello {
                credential: String::new(),
                class: ClientClass::Phone,
                versions: Versions {
                    app: "0.1.0".into(),
                },
                capabilities: vec![],
            }),
        );
        ws.send(text(&hello)).await.unwrap();
        let ok = next(ws).await.unwrap();
        assert!(matches!(ok.body, FrameBody::HelloOk(_)), "{ok:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn no_hello_in_time_is_bad_frame_then_closed() {
        let (_dir, hub) = hub();
        let mut ws = connect(&hub).await;
        let started = Instant::now();
        let f = next(&mut ws).await.unwrap();
        assert_eq!(code(&f), ErrorCode::BadFrame);
        assert!(started.elapsed() >= HELLO_TIMEOUT);
        assert!(next(&mut ws).await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_hub_pings_and_closes_a_silent_session() {
        let (_dir, hub) = hub();
        let mut ws = connect(&hub).await;
        phone_hello(&mut ws).await;
        let started = Instant::now();
        let mut pings = 0;
        while let Some(f) = next(&mut ws).await {
            assert_eq!(f.body, FrameBody::Ping, "{f:?}");
            pings += 1;
        }
        assert!(pings >= 2, "{pings}");
        let idle = started.elapsed();
        assert!(
            idle >= IDLE_TIMEOUT && idle < IDLE_TIMEOUT + PING_INTERVAL,
            "{idle:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn answering_pings_keeps_a_session_open() {
        let (_dir, hub) = hub();
        let mut ws = connect(&hub).await;
        phone_hello(&mut ws).await;
        for _ in 0..6 {
            let f = next(&mut ws).await.unwrap();
            assert_eq!(f.body, FrameBody::Ping);
            ws.send(text(&Frame::new(f.rid, FrameBody::Pong)))
                .await
                .unwrap();
        }
        assert_eq!(hub.session_count(), 1);
    }

    #[tokio::test]
    async fn an_oversized_frame_is_bad_frame_then_closed() {
        let (_dir, hub) = hub();
        let mut ws = connect(&hub).await;
        phone_hello(&mut ws).await;
        ws.send(Message::text("x".repeat(MAX_FRAME_BYTES + 1)))
            .await
            .unwrap();
        let f = next(&mut ws).await.unwrap();
        assert_eq!(code(&f), ErrorCode::BadFrame);
        assert!(next(&mut ws).await.is_none());
    }

    #[test]
    fn no_free_in_flight_slot_is_hub_slow() {
        let permits = Arc::new(Semaphore::new(1));
        let held = try_permit(&permits).unwrap();
        assert_eq!(try_permit(&permits).unwrap_err().code, ErrorCode::HubSlow);
        drop(held);
        assert!(try_permit(&permits).is_ok());
    }
}

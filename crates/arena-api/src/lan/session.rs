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
//! (`{device, device_id}`), overwriting any actor in the frame. A follower refuses every intent
//! with `NOT_PRIMARY`; an intent outside the class scope gets `FORBIDDEN` and a `warn` line.
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
    Hello, HelloOk, Intent, IntentKind, IntentPayload, IntentResult, Versions, check_envelope,
    in_scope, parse_frame, parse_frame_value, parse_json,
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
        let mut intent = submit.intent;
        intent.actor = self.authed.actor.clone();
        Ok(intent)
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
        let writer: Option<WriterHandle> = self.hub.writer.clone();
        let out = self.out.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let reply = match writer {
                Some(w) => match w.submit_intent(intent).await {
                    Ok(applied) => Frame::new(
                        rid,
                        FrameBody::IntentResult(IntentResult {
                            applied: summary(&applied),
                        }),
                    ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::HubCert;
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

//! The shared Rust Hub client: pinned TLS, the `hello` handshake, request/reply by `rid` with a
//! 5 s timeout, and the stream of pushed events.
//!
//! **Pinning.** The TLS verifier accepts exactly one certificate: the one whose SHA-256
//! fingerprint was pinned. There is no CA and no fallback; a different certificate fails the
//! connect. The handshake signature is still verified against the pinned certificate's key.
//!
//! **Timeouts.** Each request waits at most [`REQUEST_TIMEOUT`] for its reply and then fails
//! [`ClientError::HubSlow`]. Requests are independent: a slow one never delays another.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use arena_proto::{
    AppError, AppliedSummary, ErrorCode, Frame, FrameBody, Hello, HelloOk, HubEvent, Intent,
    IntentResult, IntentSubmit, PROTOCOL_VERSION, parse_frame,
};
use futures_util::{SinkExt, Stream, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Uri;

/// How long a request waits for its reply before failing `HUB_SLOW`.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The default paired-app port.
pub const DEFAULT_PORT: u16 = 7400;
/// The port of a tunnel hostname (HTTPS).
pub const REMOTE_PORT: u16 = 443;
/// With nothing received for this long, the client sends its own `ping`.
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
/// With nothing received for this long, the connection counts as closed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
/// Pushed events buffered for [`HubClient::events`] before new ones are dropped.
const EVENTS_DEPTH: usize = 1024;
const OUT_DEPTH: usize = 256;

/// A pinned-TLS WebSocket to the Hub.
pub type HubStream = WebSocketStream<TlsStream<TcpStream>>;

/// Why a client call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The URL is not `wss://host[:port][/path]`, or the fingerprint is not 64 hex digits.
    BadInput(String),
    /// TCP connect or I/O failed.
    Io(String),
    /// The TLS handshake failed for a reason other than the pin.
    Tls(String),
    /// The Hub presented a certificate whose SHA-256 fingerprint is not the pinned one. There
    /// is no fallback: the connect fails. Both fingerprints are lower-case hex.
    PinMismatch { expected: String, presented: String },
    /// The WebSocket upgrade failed.
    Handshake(String),
    /// No reply within [`REQUEST_TIMEOUT`].
    HubSlow,
    /// The Hub answered with an error frame.
    Hub(AppError),
    /// The connection is closed.
    Closed,
}

impl ClientError {
    /// The protocol error code, when there is one (`HUB_SLOW` for a timeout).
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Self::HubSlow => Some(ErrorCode::HubSlow),
            Self::Hub(e) => Some(e.code),
            _ => None,
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(m) => write!(f, "bad input: {m}"),
            Self::Io(m) => write!(f, "i/o: {m}"),
            Self::Tls(m) => write!(f, "tls: {m}"),
            Self::PinMismatch {
                expected,
                presented,
            } => write!(
                f,
                "tls: the hub certificate {presented} does not match the pinned {expected}"
            ),
            Self::Handshake(m) => write!(f, "websocket: {m}"),
            Self::HubSlow => f.write_str("HUB_SLOW: the hub did not answer in time"),
            Self::Hub(e) => write!(f, "{e}"),
            Self::Closed => f.write_str("the connection is closed"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Normalises a fingerprint to 32 bytes (`aa:bb:…` and upper case are accepted).
pub fn parse_fingerprint(text: &str) -> Result<[u8; 32], ClientError> {
    let hex: String = text.chars().filter(|c| *c != ':').collect();
    let bad = || ClientError::BadInput("the fingerprint must be 64 hex digits".into());
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(out)
}

/// Lower-case hex of `bytes`.
pub fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Accepts only the certificate whose SHA-256 is pinned. A rejected certificate's fingerprint
/// is kept in `presented`, so the connect can report both.
#[derive(Debug)]
struct PinnedVerifier {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
    presented: Arc<Mutex<Option<[u8; 32]>>>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if got == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            if let Ok(mut p) = self.presented.lock() {
                *p = Some(got);
            }
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A rustls client configuration that trusts only the pinned certificate.
pub fn pinned_config(pinned_fingerprint: &str) -> Result<Arc<ClientConfig>, ClientError> {
    pinned_config_with(pinned_fingerprint, Arc::default())
}

pub(crate) fn pinned_config_with(
    pinned_fingerprint: &str,
    presented: Arc<Mutex<Option<[u8; 32]>>>,
) -> Result<Arc<ClientConfig>, ClientError> {
    let pin = parse_fingerprint(pinned_fingerprint)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| ClientError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier {
            pin,
            provider,
            presented,
        }))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Opens a pinned-TLS WebSocket to `url` (`wss://host[:port][/path]`), without any protocol
/// handshake. [`HubClient::connect`] builds on it.
pub async fn connect_ws(url: &str, pinned_fingerprint: &str) -> Result<HubStream, ClientError> {
    let presented: Arc<Mutex<Option<[u8; 32]>>> = Arc::default();
    let config = pinned_config_with(pinned_fingerprint, Arc::clone(&presented))?;
    let expected = hex_lower(&parse_fingerprint(pinned_fingerprint)?);
    open_ws(url, config, DEFAULT_PORT, |e| {
        match presented.lock().ok().and_then(|p| *p) {
            Some(got) => ClientError::PinMismatch {
                expected: expected.clone(),
                presented: hex_lower(&got),
            },
            None => ClientError::Tls(e.to_string()),
        }
    })
    .await
}

/// A rustls client configuration that trusts the public web roots (Mozilla's), for the
/// Cloudflare-terminated TLS of the branch's tunnel hostname (Story 8.41).
pub fn web_config() -> Result<Arc<ClientConfig>, ClientError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| ClientError::Tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Opens a WebSocket to a tunnel hostname (`wss://<branch>.tunnel…/`), verified against the
/// public web roots: Cloudflare terminates the TLS, so the Hub's own certificate cannot be pinned
/// here. The device token is what the Hub checks (Story 8.41).
pub async fn connect_ws_remote(url: &str) -> Result<HubStream, ClientError> {
    open_ws(url, web_config()?, REMOTE_PORT, |e| {
        ClientError::Tls(e.to_string())
    })
    .await
}

async fn open_ws(
    url: &str,
    config: Arc<ClientConfig>,
    default_port: u16,
    tls_error: impl FnOnce(&std::io::Error) -> ClientError,
) -> Result<HubStream, ClientError> {
    let uri: Uri = url
        .parse()
        .map_err(|e| ClientError::BadInput(format!("url: {e}")))?;
    if uri.scheme_str() != Some("wss") {
        return Err(ClientError::BadInput("the hub url must be wss://".into()));
    }
    let host = uri
        .host()
        .ok_or_else(|| ClientError::BadInput("the url has no host".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(default_port);
    let server_name = ServerName::try_from(host.clone())
        .map_err(|e| ClientError::BadInput(format!("host: {e}")))?;
    let tcp = timeout(REQUEST_TIMEOUT, TcpStream::connect((host.as_str(), port)))
        .await
        .map_err(|_| ClientError::Io("connect timed out".into()))?
        .map_err(|e| ClientError::Io(e.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let tls = timeout(
        REQUEST_TIMEOUT,
        TlsConnector::from(config).connect(server_name, tcp),
    )
    .await
    .map_err(|_| ClientError::Tls("tls handshake timed out".into()))?
    .map_err(|e| tls_error(&e))?;
    let (ws, _) = timeout(REQUEST_TIMEOUT, tokio_tungstenite::client_async(url, tls))
        .await
        .map_err(|_| ClientError::Handshake("upgrade timed out".into()))?
        .map_err(|e| ClientError::Handshake(e.to_string()))?;
    Ok(ws)
}

pub async fn capture_fingerprint(
    host: &str,
    port: u16,
    wait: Duration,
) -> Result<String, ClientError> {
    let presented: Arc<Mutex<Option<[u8; 32]>>> = Arc::default();
    let config = pinned_config_with(&"0".repeat(64), Arc::clone(&presented))?;
    let bare = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let server_name = match bare.parse::<std::net::IpAddr>() {
        Ok(ip) => ServerName::IpAddress(ip.into()),
        Err(_) => ServerName::try_from(bare.clone())
            .map_err(|e| ClientError::BadInput(format!("host: {e}")))?,
    };
    let attempt = async {
        let tcp = TcpStream::connect((bare.as_str(), port))
            .await
            .map_err(|e| ClientError::Io(e.to_string()))?;
        let _ = tcp.set_nodelay(true);
        TlsConnector::from(config)
            .connect(server_name, tcp)
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Tls(e.to_string()))
    };
    let outcome = timeout(wait, attempt)
        .await
        .map_err(|_| ClientError::Io("timed out".into()))?;
    match presented.lock().ok().and_then(|p| *p) {
        Some(got) => Ok(hex_lower(&got)),
        None => Err(outcome
            .err()
            .unwrap_or_else(|| ClientError::Tls("no certificate".into()))),
    }
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Frame>>>>;

/// Pushed events, in the order the Hub sent them.
pub struct EventStream {
    rx: mpsc::Receiver<HubEvent>,
}

impl EventStream {
    /// The next event (`None` once the connection is closed).
    pub async fn next_event(&mut self) -> Option<HubEvent> {
        self.rx.recv().await
    }
}

impl Stream for EventStream {
    type Item = HubEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<HubEvent>> {
        self.rx.poll_recv(cx)
    }
}

/// How the connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    /// The last error frame the Hub pushed without a request (e.g. `TOKEN_REVOKED`).
    pub error: Option<AppError>,
}

/// A connected, authenticated Hub session.
pub struct HubClient {
    out: mpsc::Sender<Message>,
    pending: Pending,
    events: Mutex<Option<EventStream>>,
    closed: watch::Receiver<Option<Closed>>,
    hello_ok: HelloOk,
    next_rid: AtomicU64,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for HubClient {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Builds the text of a request frame: `body` (a JSON object) plus `v`, `kind` and `rid`.
fn request_text(kind: &str, rid: &str, body: Value) -> Result<String, ClientError> {
    let mut map = match body {
        Value::Object(m) => m,
        Value::Null => serde_json::Map::new(),
        _ => {
            return Err(ClientError::BadInput(
                "a request body is a JSON object".into(),
            ));
        }
    };
    map.insert("v".into(), Value::from(PROTOCOL_VERSION));
    map.insert("kind".into(), Value::from(kind));
    map.insert("rid".into(), Value::from(rid));
    Ok(Value::Object(map).to_string())
}

impl HubClient {
    /// Connects to `url` over pinned TLS and sends `hello`. Fails with the Hub's error frame
    /// (`TOKEN_INVALID`, `TOKEN_REVOKED`, `UNSUPPORTED`, …) or `HUB_SLOW`.
    pub async fn connect(
        url: &str,
        pinned_fingerprint: &str,
        hello: Hello,
    ) -> Result<Self, ClientError> {
        Self::start(connect_ws(url, pinned_fingerprint).await?, hello).await
    }

    /// Connects to a tunnel hostname (Story 8.41) and sends `hello`. The TLS is Cloudflare's,
    /// checked against the public web roots; the device token is the credential.
    pub async fn connect_remote(url: &str, hello: Hello) -> Result<Self, ClientError> {
        Self::start(connect_ws_remote(url).await?, hello).await
    }

    async fn start(ws: HubStream, hello: Hello) -> Result<Self, ClientError> {
        let (mut sink, mut stream) = ws.split();

        let hello_text = Frame::new("hello", FrameBody::Hello(hello)).to_text();
        sink.send(Message::text(hello_text))
            .await
            .map_err(|_| ClientError::Closed)?;
        let reply = timeout(REQUEST_TIMEOUT, async {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(t)) => {
                        if let Ok(frame) = parse_frame(t.as_str()) {
                            return Some(frame);
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => return None,
                    Ok(_) => {}
                }
            }
            None
        })
        .await
        .map_err(|_| ClientError::HubSlow)?
        .ok_or(ClientError::Closed)?;
        let hello_ok = match reply.body {
            FrameBody::HelloOk(ok) => ok,
            FrameBody::Error(e) => return Err(ClientError::Hub(e.error)),
            other => {
                return Err(ClientError::Handshake(format!(
                    "expected hello.ok, got {}",
                    other.kind()
                )));
            }
        };

        let pending: Pending = Arc::default();
        let (events_tx, events_rx) = mpsc::channel(EVENTS_DEPTH);
        let (closed_tx, closed_rx) = watch::channel(None);
        let (out_tx, mut out_rx) = mpsc::channel::<Message>(OUT_DEPTH);

        let writer = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let reader_pending = Arc::clone(&pending);
        let reader_out = out_tx.clone();
        let reader = tokio::spawn(async move {
            let mut last_error = None;
            let mut last_inbound = Instant::now();
            let mut tick = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut pings = 0u64;
            loop {
                let msg = tokio::select! {
                    msg = stream.next() => msg,
                    _ = tick.tick() => {
                        let silent = last_inbound.elapsed();
                        if silent >= IDLE_TIMEOUT {
                            tracing::warn!("no frame from the hub for 45 s; treating it as closed");
                            break;
                        }
                        if silent >= PING_INTERVAL {
                            pings += 1;
                            let ping = request_text("ping", &format!("kp-{pings}"), Value::Null)
                                .unwrap_or_default();
                            let _ = reader_out.try_send(Message::text(ping));
                        }
                        continue;
                    }
                };
                let Some(msg) = msg else { break };
                last_inbound = Instant::now();
                let text = match msg {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };
                let Ok(frame) = parse_frame(text.as_str()) else {
                    tracing::debug!("ignoring an unparseable frame from the hub");
                    continue;
                };
                match frame.body {
                    FrameBody::Event(ev) => {
                        if events_tx.try_send(ev.event).is_err() {
                            tracing::warn!("event buffer full; dropping a hub event");
                        }
                        continue;
                    }
                    // The hub's keepalive: answer on the same rid.
                    FrameBody::Ping => {
                        let pong = Frame::new(frame.rid, FrameBody::Pong).to_text();
                        let _ = reader_out.try_send(Message::text(pong));
                        continue;
                    }
                    // A session-level error (e.g. TOKEN_REVOKED): it fails every request
                    // still waiting, and is reported by `closed()`.
                    FrameBody::Error(ref e) if frame.rid.is_empty() => {
                        last_error = Some(e.error.clone());
                        let waiters: Vec<_> = reader_pending
                            .lock()
                            .map(|mut p| p.drain().collect())
                            .unwrap_or_default();
                        for (rid, tx) in waiters {
                            let _ = tx.send(Frame::error(rid, e.error.clone()));
                        }
                        continue;
                    }
                    _ => {}
                }
                let waiter = reader_pending
                    .lock()
                    .ok()
                    .and_then(|mut p| p.remove(&frame.rid));
                if let Some(tx) = waiter {
                    let _ = tx.send(frame);
                }
            }
            let _ = closed_tx.send(Some(Closed { error: last_error }));
            // Dropping every waiter fails its request with `Closed`.
            if let Ok(mut p) = reader_pending.lock() {
                p.clear();
            }
        });

        Ok(Self {
            out: out_tx,
            pending,
            events: Mutex::new(Some(EventStream { rx: events_rx })),
            closed: closed_rx,
            hello_ok,
            next_rid: AtomicU64::new(1),
            tasks: [writer, reader],
        })
    }

    /// The Hub's `hello.ok`.
    pub fn hello_ok(&self) -> &HelloOk {
        &self.hello_ok
    }

    /// Sends a `kind` frame with `body` (a JSON object, or null) and waits for the frame with
    /// the same `rid`: `Ok` for a reply, [`ClientError::Hub`] for an error frame and
    /// [`ClientError::HubSlow`] after [`REQUEST_TIMEOUT`].
    pub async fn request(&self, kind: &str, body: Value) -> Result<Frame, ClientError> {
        if self.is_closed() {
            return Err(ClientError::Closed);
        }
        let rid = format!("r{}", self.next_rid.fetch_add(1, Ordering::Relaxed));
        let text = request_text(kind, &rid, body)?;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| ClientError::Closed)?
            .insert(rid.clone(), tx);
        let forget = || {
            if let Ok(mut p) = self.pending.lock() {
                p.remove(&rid);
            }
        };
        // Closed between the check and the insert: the reader already cleared the waiters.
        if self.is_closed() {
            forget();
            return Err(ClientError::Closed);
        }
        if self.out.send(Message::text(text)).await.is_err() {
            forget();
            return Err(ClientError::Closed);
        }
        match timeout(REQUEST_TIMEOUT, rx).await {
            Err(_) => {
                forget();
                Err(ClientError::HubSlow)
            }
            Ok(Err(_)) => Err(ClientError::Closed),
            Ok(Ok(frame)) => match frame.body {
                FrameBody::Error(e) => Err(ClientError::Hub(e.error)),
                _ => Ok(frame),
            },
        }
    }

    /// Submits an intent and returns what it applied.
    pub async fn submit(&self, intent: Intent) -> Result<AppliedSummary, ClientError> {
        let body = serde_json::to_value(IntentSubmit { intent })
            .map_err(|e| ClientError::BadInput(e.to_string()))?;
        let frame = self.request("intent.submit", body).await?;
        match frame.body {
            FrameBody::IntentResult(IntentResult { applied }) => Ok(applied),
            other => Err(ClientError::Handshake(format!(
                "expected intent.result, got {}",
                other.kind()
            ))),
        }
    }

    /// The stream of pushed events. It can be taken once; later calls return `None`.
    pub fn events(&self) -> Option<EventStream> {
        self.events.lock().ok()?.take()
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.closed.borrow().is_some()
    }

    /// Resolves when the connection has ended.
    pub async fn closed(&self) -> Closed {
        let mut rx = self.closed.clone();
        loop {
            if let Some(c) = rx.borrow().clone() {
                return c;
            }
            if rx.changed().await.is_err() {
                return Closed { error: None };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_parse_in_any_case_and_with_colons() {
        let hex = "ab".repeat(32);
        assert_eq!(parse_fingerprint(&hex).unwrap(), [0xab; 32]);
        assert_eq!(parse_fingerprint(&hex.to_uppercase()).unwrap(), [0xab; 32]);
        let colons = vec!["ab"; 32].join(":");
        assert_eq!(parse_fingerprint(&colons).unwrap(), [0xab; 32]);
        assert!(parse_fingerprint("ab").is_err());
        assert!(parse_fingerprint(&"zz".repeat(32)).is_err());
        // `from_str_radix` alone would take a sign.
        assert!(parse_fingerprint(&format!("+a{}", "ab".repeat(31))).is_err());
    }

    #[test]
    fn request_frames_carry_the_envelope() {
        let t = request_text("ping", "r1", Value::Null).unwrap();
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v, serde_json::json!({"v": 1, "kind": "ping", "rid": "r1"}));
        assert!(request_text("ping", "r1", Value::from(3)).is_err());
    }

    #[tokio::test]
    async fn only_wss_urls_are_accepted() {
        let fp = "ab".repeat(32);
        for url in ["ws://127.0.0.1:7400/", "http://x/", "nonsense"] {
            assert!(matches!(
                connect_ws(url, &fp).await,
                Err(ClientError::BadInput(_))
            ));
        }
        assert!(matches!(
            connect_ws("wss://127.0.0.1:7400/", "short").await,
            Err(ClientError::BadInput(_))
        ));
    }
}

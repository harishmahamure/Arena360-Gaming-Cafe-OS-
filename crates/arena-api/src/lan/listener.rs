//! The two LAN listeners and the `:7401` sniffer.

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::LanHub;
use super::session::{self, MAX_FRAME_BYTES, Transport};

/// TLS WebSocket for paired apps.
pub const DEFAULT_LAN_PORT: u16 = 7400;
/// Sniffed: TLS for paired apps, plain HTTP/WS for browser classes.
pub const DEFAULT_WEB_PORT: u16 = 7401;
/// How long a connection may take to finish its TLS or HTTP handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections served at once, across both ports.
pub const MAX_CONNECTIONS: usize = 512;
/// How long shutdown waits for connections to close.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// The largest plain HTTP request head accepted on `:7401`.
const MAX_HEAD_BYTES: usize = 8 * 1024;

/// The bound `:7400` and `:7401` listeners.
pub struct LanListeners {
    lan: TcpListener,
    web: TcpListener,
    /// The loopback listener the Cloudflare Tunnel forwards to (Story 8.40).
    tunnel: Option<TcpListener>,
}

impl LanListeners {
    /// The TLS port's address.
    pub fn lan_addr(&self) -> io::Result<SocketAddr> {
        self.lan.local_addr()
    }

    /// The sniffed port's address.
    pub fn web_addr(&self) -> io::Result<SocketAddr> {
        self.web.local_addr()
    }

    /// Also listens on `addr` for the Cloudflare Tunnel's connector (Story 8.40). Only a loopback
    /// address is accepted, so nothing but the local connector can reach it.
    pub async fn bind_tunnel(&mut self, addr: SocketAddr) -> io::Result<()> {
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the tunnel listener is loopback only",
            ));
        }
        self.tunnel = Some(TcpListener::bind(addr).await?);
        Ok(())
    }

    /// The tunnel listener's address, once bound.
    pub fn tunnel_addr(&self) -> Option<SocketAddr> {
        self.tunnel.as_ref().and_then(|l| l.local_addr().ok())
    }
}

/// Binds both LAN listeners on `ip` (the Hub uses `0.0.0.0`; tests use loopback and port 0).
pub async fn bind_lan(ip: IpAddr, lan_port: u16, web_port: u16) -> io::Result<LanListeners> {
    Ok(LanListeners {
        lan: TcpListener::bind(SocketAddr::new(ip, lan_port)).await?,
        web: TcpListener::bind(SocketAddr::new(ip, web_port)).await?,
        tunnel: None,
    })
}

pub(crate) fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

/// Serves both listeners until `shutdown` resolves, then closes every session. Also runs the
/// push fan-out while this Hub has a writer.
pub async fn serve_lan(
    listeners: LanListeners,
    hub: Arc<LanHub>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    let fan_out = hub.writer.as_ref().map(|w| {
        tokio::spawn(Arc::clone(&hub.registry).fan_out(
            w.subscribe(),
            Arc::clone(&hub.devices),
            hub.media.clone(),
        ))
    });
    let acceptor = TlsAcceptor::from(Arc::clone(&hub.tls));
    let LanListeners { lan, web, tunnel } = listeners;
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut conns = JoinSet::new();
    tokio::pin!(shutdown);
    'accept: loop {
        // A slot is taken before accepting, so at most MAX_CONNECTIONS are served; further
        // connections wait in the listen backlog.
        let slot = tokio::select! {
            () = &mut shutdown => break 'accept,
            Some(_) = conns.join_next(), if !conns.is_empty() => continue 'accept,
            slot = Arc::clone(&slots).acquire_owned() => match slot {
                Ok(s) => s,
                Err(_) => break 'accept,
            },
        };
        let (tcp, peer, port) = loop {
            tokio::select! {
                () = &mut shutdown => break 'accept,
                Some(_) = conns.join_next(), if !conns.is_empty() => {}
                res = lan.accept() => match res {
                    Ok((tcp, peer)) => break (tcp, peer, Port::Lan),
                    Err(e) => accept_failed(e).await,
                },
                res = web.accept() => match res {
                    Ok((tcp, peer)) => break (tcp, peer, Port::Web),
                    Err(e) => accept_failed(e).await,
                },
                res = accept_tunnel(&tunnel) => match res {
                    Ok((tcp, peer)) => break (tcp, peer, Port::Tunnel),
                    Err(e) => accept_failed(e).await,
                },
            }
        };
        let (acceptor, hub) = (acceptor.clone(), Arc::clone(&hub));
        conns.spawn(async move {
            let _slot = slot;
            match port {
                Port::Lan => serve_tls(tcp, peer, acceptor, hub, false).await,
                Port::Web => serve_web(tcp, peer, acceptor, hub).await,
                Port::Tunnel => serve_tunnel(tcp, peer, hub).await,
            }
        });
    }
    // Drain: close every session, give connections time to send their close, then abort.
    hub.registry.kill_all();
    let _ = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while conns.join_next().await.is_some() {}
    })
    .await;
    conns.abort_all();
    if let Some(job) = fan_out {
        job.abort();
    }
}

/// Which listener accepted a connection.
#[derive(Clone, Copy)]
enum Port {
    Lan,
    Web,
    Tunnel,
}

/// Accepts on the tunnel listener, or never when there is none.
async fn accept_tunnel(tunnel: &Option<TcpListener>) -> io::Result<(TcpStream, SocketAddr)> {
    match tunnel {
        Some(l) => l.accept().await,
        None => std::future::pending().await,
    }
}

/// The upgrade callback for the tunnel listener: the WebSocket lives at `/` or `/live` only.
#[allow(clippy::result_large_err)]
fn only_live(req: &Request, resp: Response) -> Result<Response, ErrorResponse> {
    match req.uri().path() {
        "/" | arena_proto::LIVE_PATH => Ok(resp),
        _ => {
            let mut err = ErrorResponse::new(Some("not found".into()));
            *err.status_mut() = StatusCode::NOT_FOUND;
            Err(err)
        }
    }
}

/// The Cloudflare Tunnel's connector, from loopback (Story 8.40): a WebSocket for live viewers
/// and nothing else.
async fn serve_tunnel(tcp: TcpStream, peer: SocketAddr, hub: Arc<LanHub>) {
    if !peer.ip().is_loopback() {
        return;
    }
    let _ = tcp.set_nodelay(true);
    let upgrade =
        tokio_tungstenite::accept_hdr_async_with_config(tcp, only_live, Some(ws_config()));
    match timeout(HANDSHAKE_TIMEOUT, upgrade).await {
        Ok(Ok(ws)) => session::run(ws, Transport::Tunnel, peer, hub).await,
        Ok(Err(e)) => tracing::debug!(error = %e, "tunnel websocket upgrade failed"),
        Err(_) => tracing::debug!("tunnel websocket upgrade timed out"),
    }
}

/// A failed accept (e.g. out of file descriptors) is logged and retried after a pause.
async fn accept_failed(e: io::Error) {
    tracing::warn!(error = %e, "lan accept failed");
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// The upgrade callback for TLS connections: the WebSocket lives at `/` only. The error type
/// is fixed by tungstenite.
#[allow(clippy::result_large_err)]
fn only_root(req: &Request, resp: Response) -> Result<Response, ErrorResponse> {
    if req.uri().path() == "/" {
        Ok(resp)
    } else {
        let mut err = ErrorResponse::new(Some("not found".into()));
        *err.status_mut() = StatusCode::NOT_FOUND;
        Err(err)
    }
}

/// TLS, then a WebSocket at `/` (paired apps).
async fn serve_tls(
    tcp: TcpStream,
    peer: SocketAddr,
    acceptor: TlsAcceptor,
    hub: Arc<LanHub>,
    web: bool,
) {
    let _ = tcp.set_nodelay(true);
    let mut tls = match timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return tracing::debug!(error = %e, "tls handshake failed"),
        Err(_) => return tracing::debug!("tls handshake timed out"),
    };
    if !web {
        return upgrade_tls(tls, peer, hub).await;
    }
    let Ok(Some(head)) = timeout(HANDSHAKE_TIMEOUT, read_head(&mut tls)).await else {
        return;
    };
    match route(&request_line(&head)) {
        Route::HubInfo => return answer_hub_info(&mut tls, &hub).await,
        Route::Pair if hub.pairing.is_some() => {
            let hub = Arc::clone(&hub);
            let head_bytes = head.clone();
            return answer_pair(&mut tls, &head_bytes, |body| async move {
                super::pair::handle(&hub, peer, &body).await
            })
            .await;
        }
        Route::Pair => return respond(&mut tls, "404 Not Found", "not found").await,
        Route::Media => return super::media::answer(&mut tls, peer, &head, &hub).await,
        _ => {}
    }
    let stream = Prefixed {
        prefix: head,
        pos: 0,
        inner: tls,
    };
    upgrade_tls(stream, peer, hub).await;
}

async fn upgrade_tls<S>(stream: S, peer: SocketAddr, hub: Arc<LanHub>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let upgrade =
        tokio_tungstenite::accept_hdr_async_with_config(stream, only_root, Some(ws_config()));
    match timeout(HANDSHAKE_TIMEOUT, upgrade).await {
        Ok(Ok(ws)) => session::run(ws, Transport::Tls, peer, hub).await,
        Ok(Err(e)) => tracing::debug!(error = %e, "websocket upgrade failed"),
        Err(_) => tracing::debug!("websocket upgrade timed out"),
    }
}

/// `:7401`: TLS by its first byte, else plain HTTP routed by path.
async fn serve_web(tcp: TcpStream, peer: SocketAddr, acceptor: TlsAcceptor, hub: Arc<LanHub>) {
    let mut first = [0u8; 1];
    match timeout(HANDSHAKE_TIMEOUT, tcp.peek(&mut first)).await {
        Ok(Ok(1)) => {}
        _ => return,
    }
    if first[0] == 0x16 {
        return serve_tls(tcp, peer, acceptor, hub, true).await;
    }
    let _ = tcp.set_nodelay(true);
    serve_plain(tcp, peer, hub).await;
}

/// What a plain request on `:7401` gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// `GET /ws`: a WebSocket for `kds` and `phone`.
    Ws,
    /// `GET /console/<tv>`: a placeholder page.
    Placeholder,
    /// `GET /kitchen`, `GET /kitchen/<asset>`: the Kitchen display app (Story 7.6).
    Kitchen,
    /// `POST /kitchen/pair`: a kitchen display pairs with a code.
    KitchenPair,
    /// `GET /kitchen/status`: whether the branch has the kitchen flag.
    KitchenStatus,
    HubInfo,
    Pair,
    /// `GET /media/<sha256>`: a cached game file for a paired kiosk (TLS only).
    Media,
    NotFound,
}

/// Routes a request line (`GET /path HTTP/1.1`).
pub(crate) fn route(request_line: &str) -> Route {
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Route::NotFound;
    };
    let path = target.split(['?', '#']).next().unwrap_or("");
    if method == "POST" && path == arena_proto::PAIR_PATH {
        return Route::Pair;
    }
    if method == "POST" && path == arena_proto::KDS_PAIR_PATH {
        return Route::KitchenPair;
    }
    if method != "GET" {
        return Route::NotFound;
    }
    if path.starts_with("/media/") {
        return Route::Media;
    }
    match path {
        "/ws" => Route::Ws,
        "/hub-info" => Route::HubInfo,
        "/kitchen" | "/kitchen/" => Route::Kitchen,
        arena_proto::KDS_STATUS_PATH => Route::KitchenStatus,
        arena_proto::KDS_PAIR_PATH => Route::NotFound,
        p if p.starts_with("/kitchen/") => Route::Kitchen,
        p => match p.strip_prefix("/console/") {
            Some(tv) if is_tv_id(tv.strip_suffix('/').unwrap_or(tv)) => Route::Placeholder,
            _ => Route::NotFound,
        },
    }
}

fn is_tv_id(tv: &str) -> bool {
    !tv.is_empty()
        && tv.len() <= 64
        && tv
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reads the request head (up to the blank line), keeping every byte read.
async fn read_head<S: AsyncRead + Unpin>(tcp: &mut S) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tcp.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Some(buf);
        }
        if buf.len() > MAX_HEAD_BYTES {
            return None;
        }
    }
}

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Arena 360</title></head><body><main>Arena 360</main></body></html>";

async fn respond<S: AsyncWrite + Unpin>(tcp: &mut S, status: &str, body: &str) {
    respond_as(tcp, status, "text/html; charset=utf-8", body).await;
}

async fn respond_as<S: AsyncWrite + Unpin>(
    tcp: &mut S,
    status: &str,
    content_type: &str,
    body: &str,
) {
    respond_bytes(tcp, status, content_type, body.as_bytes()).await;
}

async fn respond_bytes<S: AsyncWrite + Unpin>(
    tcp: &mut S,
    status: &str,
    content_type: &str,
    body: &[u8],
) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = timeout(HANDSHAKE_TIMEOUT, async {
        let _ = tcp.write_all(head.as_bytes()).await;
        let _ = tcp.write_all(body).await;
        let _ = tcp.shutdown().await;
    })
    .await;
}

fn request_line(head: &[u8]) -> String {
    let line_end = head
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(head.len());
    String::from_utf8_lossy(&head[..line_end]).into_owned()
}

async fn answer_hub_info<S: AsyncWrite + Unpin>(stream: &mut S, hub: &LanHub) {
    let (status, body) = match hub.hub_info().await {
        Some(info) => ("200 OK", serde_json::to_string(&info).unwrap_or_default()),
        None => (
            "503 Service Unavailable",
            serde_json::to_string(&arena_proto::AppError::new(
                arena_proto::ErrorCode::NoHub,
                "this hub has no branch yet",
            ))
            .unwrap_or_default(),
        ),
    };
    respond_as(stream, status, "application/json", &body).await;
}

/// Reads the request body and answers it with `handle` (the kiosk `POST /pair` and the kitchen
/// display `POST /kitchen/pair` share this).
async fn answer_pair<S, F, Fut>(stream: &mut S, head: &[u8], handle: F)
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = super::pair::Answer>,
{
    let answer = match timeout(HANDSHAKE_TIMEOUT, super::pair::read_body(stream, head)).await {
        Ok(Ok(body)) => handle(body).await,
        Ok(Err(e)) => super::pair::Answer {
            status: "400 Bad Request".into(),
            body: serde_json::to_string(&e).unwrap_or_default(),
            retry_after_s: None,
        },
        Err(_) => return,
    };
    write_answer(stream, answer).await;
}

async fn write_answer<S: AsyncWrite + Unpin>(stream: &mut S, answer: super::pair::Answer) {
    let retry = answer
        .retry_after_s
        .map(|s| format!("Retry-After: {s}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: close\r\n\r\n",
        answer.status,
        answer.body.len()
    );
    let _ = timeout(HANDSHAKE_TIMEOUT, async {
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(answer.body.as_bytes()).await;
        let _ = stream.shutdown().await;
    })
    .await;
}

/// The path after `/kitchen/` in a request line (`None` for `/kitchen` and `/kitchen/`).
fn kitchen_rel(line: &str) -> Option<String> {
    let target = line.split(' ').nth(1)?;
    let path = target.split(['?', '#']).next().unwrap_or("");
    path.strip_prefix("/kitchen/")
        .filter(|rel| !rel.is_empty())
        .map(str::to_owned)
}

/// `GET /kitchen`, `GET /kitchen/<asset>`.
async fn answer_kitchen<S: AsyncWrite + Unpin>(stream: &mut S, hub: &LanHub, line: &str) {
    match super::kds_pair::serve(hub, kitchen_rel(line).as_deref()).await {
        Some(asset) => respond_bytes(stream, "200 OK", asset.content_type, &asset.bytes).await,
        None => {
            let body = serde_json::to_string(&arena_proto::AppError::new(
                arena_proto::ErrorCode::NotFound,
                "not found",
            ))
            .unwrap_or_default();
            respond_as(stream, "404 Not Found", "application/json", &body).await;
        }
    }
}

async fn serve_plain(mut tcp: TcpStream, peer: SocketAddr, hub: Arc<LanHub>) {
    let Ok(Some(head)) = timeout(HANDSHAKE_TIMEOUT, read_head(&mut tcp)).await else {
        return;
    };
    let line = request_line(&head);
    let route = route(&line);
    // Story 7.6: a request to `/kitchen*` that carries a bearer token must carry a live `kds`
    // one; a paired-device token or any other token is refused.
    if matches!(
        route,
        Route::Kitchen | Route::KitchenPair | Route::KitchenStatus
    ) && !super::kds_pair::bearer_allowed(&hub, peer, &head).await
    {
        return write_answer(&mut tcp, super::kds_pair::forbidden()).await;
    }
    match route {
        Route::HubInfo => answer_hub_info(&mut tcp, &hub).await,
        Route::Placeholder => respond(&mut tcp, "200 OK", PLACEHOLDER).await,
        Route::Kitchen => answer_kitchen(&mut tcp, &hub, &line).await,
        Route::KitchenStatus => {
            let answer = super::kds_pair::status(&hub).await;
            write_answer(&mut tcp, answer).await;
        }
        Route::KitchenPair => {
            let hub = Arc::clone(&hub);
            let head_bytes = head.clone();
            answer_pair(&mut tcp, &head_bytes, |body| async move {
                super::kds_pair::pair(&hub, peer, &body).await
            })
            .await;
        }
        // Everything else is for TLS clients (pairing, media) or does not exist: plain HTTP
        // answers only the kitchen, the WebSocket, `/hub-info` and `/console/<tv>`.
        Route::NotFound | Route::Pair | Route::Media => {
            write_answer(&mut tcp, super::kds_pair::forbidden()).await
        }
        Route::Ws => {
            let stream = Prefixed {
                prefix: head,
                pos: 0,
                inner: tcp,
            };
            let upgrade = tokio_tungstenite::accept_async_with_config(stream, Some(ws_config()));
            match timeout(HANDSHAKE_TIMEOUT, upgrade).await {
                Ok(Ok(ws)) => session::run(ws, Transport::Plain, peer, hub).await,
                Ok(Err(e)) => tracing::debug!(error = %e, "websocket upgrade failed"),
                Err(_) => tracing::debug!("websocket upgrade timed out"),
            }
        }
    }
}

/// A stream that first replays bytes already read from `inner` (the sniffed HTTP head).
struct Prefixed<S> {
    prefix: Vec<u8>,
    pos: usize,
    inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos < self.prefix.len() {
            let n = buf.remaining().min(self.prefix.len() - self.pos);
            let start = self.pos;
            buf.put_slice(&self.prefix[start..start + n]);
            self.pos += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kitchen_path_after_the_prefix_is_read_from_the_request_line() {
        assert_eq!(kitchen_rel("GET /kitchen HTTP/1.1"), None);
        assert_eq!(kitchen_rel("GET /kitchen/ HTTP/1.1"), None);
        assert_eq!(
            kitchen_rel("GET /kitchen/_app/a.js?x=1 HTTP/1.1").as_deref(),
            Some("_app/a.js")
        );
    }

    #[test]
    fn routes_by_method_and_path() {
        assert_eq!(route("GET /ws HTTP/1.1"), Route::Ws);
        assert_eq!(route("GET /ws?x=1 HTTP/1.1"), Route::Ws);
        assert_eq!(route("GET /kitchen HTTP/1.1"), Route::Kitchen);
        assert_eq!(route("GET /kitchen/ HTTP/1.1"), Route::Kitchen);
        assert_eq!(
            route("GET /kitchen/_app/immutable/a.js HTTP/1.1"),
            Route::Kitchen
        );
        assert_eq!(route("GET /kitchen/status HTTP/1.1"), Route::KitchenStatus);
        assert_eq!(route("POST /kitchen/status HTTP/1.1"), Route::NotFound);
        assert_eq!(route("POST /kitchen/pair HTTP/1.1"), Route::KitchenPair);
        assert_eq!(route("POST /kitchen/pair/x HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /kitchen/pair HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /kitchenx HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /console/tv-1 HTTP/1.1"), Route::Placeholder);
        assert_eq!(route("GET /console/ HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /console/a/b HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /nope HTTP/1.1"), Route::NotFound);
        assert_eq!(route("POST /kitchen HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /hub-info HTTP/1.1"), Route::HubInfo);
        assert_eq!(route("GET /hub-info?x HTTP/1.1"), Route::HubInfo);
        assert_eq!(route("POST /hub-info HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /hub-info/x HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /media/abc HTTP/1.1"), Route::Media);
        assert_eq!(route("GET /media/ HTTP/1.1"), Route::Media);
        assert_eq!(route("GET /media/a?x=1 HTTP/1.1"), Route::Media);
        assert_eq!(route("POST /media/abc HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /media HTTP/1.1"), Route::NotFound);
        assert_eq!(route("garbage"), Route::NotFound);
        assert_eq!(route("POST /pair HTTP/1.1"), Route::Pair);
        assert_eq!(route("GET /pair HTTP/1.1"), Route::NotFound);
        assert_eq!(route("POST /pair/x HTTP/1.1"), Route::NotFound);
    }
}

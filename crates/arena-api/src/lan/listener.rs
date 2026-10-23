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
}

/// Binds both LAN listeners on `ip` (the Hub uses `0.0.0.0`; tests use loopback and port 0).
pub async fn bind_lan(ip: IpAddr, lan_port: u16, web_port: u16) -> io::Result<LanListeners> {
    Ok(LanListeners {
        lan: TcpListener::bind(SocketAddr::new(ip, lan_port)).await?,
        web: TcpListener::bind(SocketAddr::new(ip, web_port)).await?,
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
        tokio::spawn(Arc::clone(&hub.registry).fan_out(w.subscribe(), Arc::clone(&hub.devices)))
    });
    let acceptor = TlsAcceptor::from(Arc::clone(&hub.tls));
    let LanListeners { lan, web } = listeners;
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
        let (tcp, peer, tls_only) = loop {
            tokio::select! {
                () = &mut shutdown => break 'accept,
                Some(_) = conns.join_next(), if !conns.is_empty() => {}
                res = lan.accept() => match res {
                    Ok((tcp, peer)) => break (tcp, peer, true),
                    Err(e) => accept_failed(e).await,
                },
                res = web.accept() => match res {
                    Ok((tcp, peer)) => break (tcp, peer, false),
                    Err(e) => accept_failed(e).await,
                },
            }
        };
        let (acceptor, hub) = (acceptor.clone(), Arc::clone(&hub));
        conns.spawn(async move {
            let _slot = slot;
            if tls_only {
                serve_tls(tcp, peer, acceptor, hub).await;
            } else {
                serve_web(tcp, peer, acceptor, hub).await;
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
async fn serve_tls(tcp: TcpStream, peer: SocketAddr, acceptor: TlsAcceptor, hub: Arc<LanHub>) {
    let _ = tcp.set_nodelay(true);
    let tls = match timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return tracing::debug!(error = %e, "tls handshake failed"),
        Err(_) => return tracing::debug!("tls handshake timed out"),
    };
    let upgrade =
        tokio_tungstenite::accept_hdr_async_with_config(tls, only_root, Some(ws_config()));
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
        return serve_tls(tcp, peer, acceptor, hub).await;
    }
    let _ = tcp.set_nodelay(true);
    serve_plain(tcp, peer, hub).await;
}

/// What a plain request on `:7401` gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// `GET /ws`: a WebSocket for `kds` and `phone`.
    Ws,
    /// `GET /kitchen`, `GET /console/<tv>`: a placeholder page.
    Placeholder,
    NotFound,
}

/// Routes a request line (`GET /path HTTP/1.1`).
pub(crate) fn route(request_line: &str) -> Route {
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Route::NotFound;
    };
    if method != "GET" {
        return Route::NotFound;
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    match path {
        "/ws" => Route::Ws,
        "/kitchen" | "/kitchen/" => Route::Placeholder,
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
async fn read_head(tcp: &mut TcpStream) -> Option<Vec<u8>> {
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

async fn respond(tcp: &mut TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = timeout(HANDSHAKE_TIMEOUT, async {
        let _ = tcp.write_all(head.as_bytes()).await;
        let _ = tcp.write_all(body.as_bytes()).await;
        let _ = tcp.shutdown().await;
    })
    .await;
}

async fn serve_plain(mut tcp: TcpStream, peer: SocketAddr, hub: Arc<LanHub>) {
    let Ok(Some(head)) = timeout(HANDSHAKE_TIMEOUT, read_head(&mut tcp)).await else {
        return;
    };
    let line_end = head
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(head.len());
    let line = String::from_utf8_lossy(&head[..line_end]).into_owned();
    match route(&line) {
        Route::Placeholder => respond(&mut tcp, "200 OK", PLACEHOLDER).await,
        Route::NotFound => respond(&mut tcp, "404 Not Found", "not found").await,
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
    fn routes_by_method_and_path() {
        assert_eq!(route("GET /ws HTTP/1.1"), Route::Ws);
        assert_eq!(route("GET /ws?x=1 HTTP/1.1"), Route::Ws);
        assert_eq!(route("GET /kitchen HTTP/1.1"), Route::Placeholder);
        assert_eq!(route("GET /console/tv-1 HTTP/1.1"), Route::Placeholder);
        assert_eq!(route("GET /console/ HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /console/a/b HTTP/1.1"), Route::NotFound);
        assert_eq!(route("GET /nope HTTP/1.1"), Route::NotFound);
        assert_eq!(route("POST /kitchen HTTP/1.1"), Route::NotFound);
        assert_eq!(route("garbage"), Route::NotFound);
    }
}

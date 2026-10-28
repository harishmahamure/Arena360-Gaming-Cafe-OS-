//! `GET /media/<sha256>` on `:7401` (Story 6.2): the Hub serves its cached game art and videos to
//! paired kiosks, so a kiosk never reaches the internet for media.
//!
//! TLS only: a plain-HTTP request is 401. The caller presents its device token as
//! `Authorization: Bearer <token>`; it is checked as a `paired` device with the same argon2id
//! permits and failure throttle as `hello`, so a revoked or unknown token is 401 and repeated
//! failures block the address. A malformed or unknown hash, a file not downloaded yet or one
//! gone from the disk is 404 straight away. A hit is sent in chunks with immutable cache headers
//! (the name is the content hash), honouring one `Range`.

use std::io::SeekFrom;
use std::net::SocketAddr;
use std::time::Duration;

use arena_proto::{ClientClass, ErrorCode};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout};

use super::LanHub;

/// How long one write to a client may stall before the transfer is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// The size of each chunk read from the file.
const CHUNK: usize = 64 * 1024;
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// What a `Range` header asks for, against a file of a known length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeSpec {
    /// No usable range: send the whole file.
    Full,
    /// These bytes, both ends included.
    Partial(u64, u64),
    /// Nothing in the file is in range.
    Unsatisfiable,
}

/// Reads one `bytes=a-b`, `bytes=a-` or `bytes=-n` range. Several ranges, another unit or a
/// malformed value are ignored (the whole file is sent).
pub(crate) fn parse_range(header: Option<&str>, len: u64) -> RangeSpec {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeSpec::Full;
    };
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((from, to)) = spec.trim().split_once('-') else {
        return RangeSpec::Full;
    };
    let num = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    match (from.is_empty(), to.is_empty()) {
        // `-n`: the last n bytes.
        (true, false) => match num(to) {
            Some(0) => RangeSpec::Unsatisfiable,
            Some(_) if len == 0 => RangeSpec::Unsatisfiable,
            Some(n) => RangeSpec::Partial(len.saturating_sub(n), len - 1),
            None => RangeSpec::Full,
        },
        // `a-`
        (false, true) => match num(from) {
            Some(a) if a >= len => RangeSpec::Unsatisfiable,
            Some(a) => RangeSpec::Partial(a, len - 1),
            None => RangeSpec::Full,
        },
        // `a-b`
        (false, false) => match (num(from), num(to)) {
            (Some(a), Some(b)) if a > b => RangeSpec::Full,
            (Some(a), Some(_)) if a >= len => RangeSpec::Unsatisfiable,
            (Some(a), Some(b)) => RangeSpec::Partial(a, b.min(len - 1)),
            _ => RangeSpec::Full,
        },
        (true, true) => RangeSpec::Full,
    }
}

/// The value of header `name` (case-insensitive) in a request head.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The token of an `Authorization: Bearer <token>` header.
fn bearer(head: &str) -> Option<&str> {
    let value = header(head, "authorization")?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

/// The hash segment of `GET /media/<hash>[?query] HTTP/1.1`.
fn hash_of(head: &str) -> &str {
    let target = head.split(' ').nth(1).unwrap_or("");
    let path = target.split(['?', '#']).next().unwrap_or("");
    path.strip_prefix("/media/").unwrap_or("")
}

async fn send_all<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> bool {
    matches!(
        timeout(WRITE_TIMEOUT, stream.write_all(bytes)).await,
        Ok(Ok(()))
    )
}

/// An answer with no body.
pub(crate) async fn empty<S: AsyncWrite + Unpin>(stream: &mut S, status: &str, extra: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    if send_all(stream, head.as_bytes()).await {
        let _ = timeout(WRITE_TIMEOUT, stream.shutdown()).await;
    }
}

/// 401 with no detail about why.
pub(crate) async fn unauthorized<S: AsyncWrite + Unpin>(stream: &mut S) {
    empty(stream, "401 Unauthorized", "WWW-Authenticate: Bearer\r\n").await;
}

enum Auth {
    Ok,
    Refused,
    Failed,
}

async fn authenticate(hub: &LanHub, peer: SocketAddr, token: &str) -> Auth {
    if hub.throttle.is_blocked(peer.ip(), Instant::now()) {
        return Auth::Refused;
    }
    let Ok(_permit) = std::sync::Arc::clone(&hub.verify_permits)
        .acquire_owned()
        .await
    else {
        return Auth::Failed;
    };
    let devices = std::sync::Arc::clone(&hub.devices);
    let token = token.to_owned();
    let verified =
        tokio::task::spawn_blocking(move || devices.verify(ClientClass::Paired, &token)).await;
    match verified {
        Ok(Ok(_)) => Auth::Ok,
        Ok(Err(e)) if matches!(e.code, ErrorCode::TokenInvalid | ErrorCode::TokenRevoked) => {
            hub.throttle.record_failure(peer.ip(), Instant::now());
            Auth::Refused
        }
        _ => Auth::Failed,
    }
}

/// Answers one `GET /media/<sha256>` read from `stream` as `head`.
pub(crate) async fn answer<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    peer: SocketAddr,
    head: &[u8],
    hub: &LanHub,
) {
    let head = String::from_utf8_lossy(head);
    let Some(token) = bearer(&head) else {
        return unauthorized(stream).await;
    };
    match authenticate(hub, peer, token).await {
        Auth::Ok => {}
        Auth::Refused => return unauthorized(stream).await,
        Auth::Failed => return empty(stream, "500 Internal Server Error", "").await,
    }
    let Some(cache) = hub.media.clone() else {
        return empty(stream, "404 Not Found", "").await;
    };
    let hash = hash_of(&head).to_owned();
    let now = hub.clock.now_ms();
    let found = match tokio::task::spawn_blocking(move || cache.serve_lookup(&hash, now)).await {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), "media: the cache lookup failed");
            return empty(stream, "500 Internal Server Error", "").await;
        }
        Err(_) => return empty(stream, "500 Internal Server Error", "").await,
    };
    let Some(file) = found else {
        return empty(stream, "404 Not Found", "").await;
    };
    let etag = format!("\"{}\"", file.sha256);
    let common = format!(
        "ETag: {etag}\r\nCache-Control: {IMMUTABLE}\r\nX-Content-Type-Options: nosniff\r\nAccept-Ranges: bytes\r\n"
    );
    if header(&head, "if-none-match").is_some_and(|v| {
        v == "*"
            || v.split(',')
                .any(|t| t.trim().trim_start_matches("W/") == etag)
    }) {
        let reply = format!("HTTP/1.1 304 Not Modified\r\n{common}Connection: close\r\n\r\n");
        send_all(stream, reply.as_bytes()).await;
        let _ = timeout(WRITE_TIMEOUT, stream.shutdown()).await;
        return;
    }
    let (status, start, end, extra) = match parse_range(header(&head, "range"), file.bytes) {
        RangeSpec::Full => ("200 OK", 0, file.bytes, String::new()),
        RangeSpec::Partial(a, b) => (
            "206 Partial Content",
            a,
            b + 1,
            format!("Content-Range: bytes {a}-{b}/{}\r\n", file.bytes),
        ),
        RangeSpec::Unsatisfiable => {
            return empty(
                stream,
                "416 Range Not Satisfiable",
                &format!("Content-Range: bytes */{}\r\n", file.bytes),
            )
            .await;
        }
    };
    let mut disk = match tokio::fs::File::open(&file.path).await {
        Ok(f) => f,
        Err(_) => return empty(stream, "404 Not Found", "").await,
    };
    if start > 0 && disk.seek(SeekFrom::Start(start)).await.is_err() {
        return empty(stream, "500 Internal Server Error", "").await;
    }
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{extra}{common}Connection: close\r\n\r\n",
        file.content_type,
        end - start
    );
    if !send_all(stream, reply.as_bytes()).await {
        return;
    }
    let mut left = end - start;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let want = buf.len().min(left as usize);
        let Ok(n) = disk.read(&mut buf[..want]).await else {
            return;
        };
        if n == 0 {
            // The file shrank under us: the client sees a short body and retries.
            return;
        }
        if !send_all(stream, &buf[..n]).await {
            return;
        }
        left -= n as u64;
    }
    let _ = timeout(WRITE_TIMEOUT, stream.shutdown()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_range_is_honoured_and_the_rest_is_ignored_or_refused() {
        let r = |h: &str| parse_range(Some(h), 100);
        assert_eq!(parse_range(None, 100), RangeSpec::Full);
        assert_eq!(r("bytes=0-9"), RangeSpec::Partial(0, 9));
        assert_eq!(r("bytes=10-"), RangeSpec::Partial(10, 99));
        assert_eq!(r("bytes=-10"), RangeSpec::Partial(90, 99));
        assert_eq!(r("bytes=-500"), RangeSpec::Partial(0, 99));
        assert_eq!(r("bytes=90-500"), RangeSpec::Partial(90, 99));
        assert_eq!(r("bytes=99-99"), RangeSpec::Partial(99, 99));
        // Unsatisfiable.
        assert_eq!(r("bytes=100-"), RangeSpec::Unsatisfiable);
        assert_eq!(r("bytes=150-160"), RangeSpec::Unsatisfiable);
        assert_eq!(r("bytes=-0"), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), RangeSpec::Unsatisfiable);
        // Ignored: several ranges, another unit, malformed.
        for ignored in [
            "bytes=0-1,5-6",
            "items=0-1",
            "bytes=5-2",
            "bytes=a-b",
            "bytes=",
            "bytes=-",
            "bytes=1",
            "0-9",
        ] {
            assert_eq!(r(ignored), RangeSpec::Full, "{ignored}");
        }
    }

    #[test]
    fn the_request_head_gives_the_token_header_values_and_hash() {
        let head = "GET /media/AbC?x=1 HTTP/1.1\r\nHost: hub\r\nauthorization: Bearer  tok.en \r\nRange: bytes=0-1\r\n\r\n";
        assert_eq!(bearer(head), Some("tok.en"));
        assert_eq!(header(head, "RANGE"), Some("bytes=0-1"));
        assert_eq!(hash_of(head), "AbC");
        assert_eq!(bearer("GET /media/a HTTP/1.1\r\n\r\n"), None);
        assert_eq!(
            bearer("GET /media/a HTTP/1.1\r\nAuthorization: Basic abc\r\n\r\n"),
            None
        );
        assert_eq!(
            bearer("GET /media/a HTTP/1.1\r\nAuthorization: Bearer \r\n\r\n"),
            None
        );
        assert_eq!(hash_of("GET /media/ HTTP/1.1\r\n\r\n"), "");
    }
}

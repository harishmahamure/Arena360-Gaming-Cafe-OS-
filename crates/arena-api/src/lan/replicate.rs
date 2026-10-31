//! The main Hub serves its linked standby (Story 9.19, AD-4): `GET /v1/replicate/snapshot` (TLS,
//! `:7401`) and the WebSocket `/v1/replicate` (TLS, `:7400` and `:7401`).
//!
//! Both take the standby's device token: as `Authorization: Bearer` for the snapshot and in the
//! first `open` frame for the stream. The token must verify as a `paired` device and the device
//! must be in `standby_links`; the Hub must be primary. The stream sends rows in log order after
//! the standby's position, closed history first, then each commit within a polling tick
//! ([`LIVE_POLL`]); the key state of every player a batch touches rides in the same frame.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arena_proto::{
    AppError, ClientClass, ErrorCode, REPLICATE_BATCH_ROWS, REPLICATE_SNAPSHOT_PATH,
    ReplicateFrame, ReplicatePosition, ReplicatedChange, ReplicatedKey, SNAPSHOT_EPOCH_HEADER,
    SNAPSHOT_SEQ_HEADER,
};
use arena_store::{ChangeRecord, Store};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use super::LanHub;
use super::media::{empty, unauthorized};
use super::session::HELLO_TIMEOUT;

/// How often the live stream looks for new rows.
pub const LIVE_POLL: Duration = Duration::from_millis(200);
/// With nothing to send for this long, a `ping` keeps the line alive.
const PING_AFTER: Duration = Duration::from_secs(10);
/// A `batch` frame stays under this many bytes of rows (the frame limit is 1 MiB).
const BATCH_BYTES: usize = 256 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 64 * 1024;

/// Where the main Hub's database is, for the standby's snapshot and stream reads.
#[derive(Debug, Clone)]
pub struct ReplicationSource {
    pub db_path: PathBuf,
}

fn refusal(code: ErrorCode, message: &str) -> ReplicateFrame {
    ReplicateFrame::Refused {
        code,
        message: message.into(),
    }
}

fn wire(c: &ChangeRecord) -> ReplicatedChange {
    ReplicatedChange {
        epoch: c.epoch.clone(),
        seq: c.seq,
        tbl: c.tbl.clone(),
        row_id: c.row_id.clone(),
        op: c.op.as_str().to_owned(),
        row: c.row.clone(),
        intent_id: c.intent_id.clone(),
        intent_kind: c.intent_kind.clone(),
        actor_kind: c.actor_kind.clone(),
        actor_id: c.actor_id.clone(),
        elevated_by_kind: c.elevated_by_kind.clone(),
        elevated_by_id: c.elevated_by_id.clone(),
        elevated_by_mode: c.elevated_by_mode.clone(),
        elevated_by_verifier_version: c.elevated_by_verifier_version,
        at: c.at,
        business_day: c.business_day.clone(),
        fy: c.fy.clone(),
        reason: c.reason.clone(),
    }
}

/// Checks `token` for replication: a primary Hub, a verified `paired` device, linked as standby.
async fn authorise(hub: &LanHub, peer: SocketAddr, token: &str) -> Result<(), ReplicateFrame> {
    if !hub.is_primary() {
        return Err(refusal(
            ErrorCode::NotPrimary,
            "this hub is not the primary",
        ));
    }
    let Some(source) = hub.replication.clone() else {
        return Err(refusal(ErrorCode::Forbidden, "replication is not served"));
    };
    let invalid = || refusal(ErrorCode::TokenInvalid, "the token is not valid");
    if hub.throttle.is_blocked(peer.ip(), Instant::now()) {
        return Err(invalid());
    }
    let Ok(_permit) = Arc::clone(&hub.verify_permits).acquire_owned().await else {
        return Err(refusal(ErrorCode::Internal, "the hub is shutting down"));
    };
    let devices = Arc::clone(&hub.devices);
    let owned = token.to_owned();
    let verified =
        tokio::task::spawn_blocking(move || devices.verify(ClientClass::Paired, &owned)).await;
    let device = match verified {
        Ok(Ok(d)) => d,
        Ok(Err(e)) if matches!(e.code, ErrorCode::TokenInvalid | ErrorCode::TokenRevoked) => {
            hub.throttle.record_failure(peer.ip(), Instant::now());
            return Err(invalid());
        }
        _ => return Err(refusal(ErrorCode::Internal, "the token check failed")),
    };
    let linked = tokio::task::spawn_blocking(move || {
        Store::open_reader(&source.db_path)?.replication_linked(&device.device_id)
    })
    .await;
    match linked {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => Err(refusal(
            ErrorCode::Forbidden,
            "this device is not linked as the standby",
        )),
        _ => Err(refusal(ErrorCode::Internal, "the link check failed")),
    }
}

fn bearer(head: &str) -> Option<&str> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("authorization") {
            return None;
        }
        let (scheme, token) = v.trim().split_once(' ')?;
        (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
    })
}

/// Answers `GET /v1/replicate/snapshot` read from `stream` as `head`.
pub(crate) async fn answer_snapshot<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    peer: SocketAddr,
    head: &[u8],
    hub: &LanHub,
) {
    debug_assert!(REPLICATE_SNAPSHOT_PATH.starts_with('/'));
    let head = String::from_utf8_lossy(head);
    let Some(token) = bearer(&head) else {
        return unauthorized(stream).await;
    };
    if let Err(ReplicateFrame::Refused { code, .. }) = authorise(hub, peer, token).await {
        return match code {
            ErrorCode::TokenInvalid => unauthorized(stream).await,
            ErrorCode::Forbidden => empty(stream, "403 Forbidden", "").await,
            ErrorCode::NotPrimary => empty(stream, "503 Service Unavailable", "").await,
            _ => empty(stream, "500 Internal Server Error", "").await,
        };
    }
    let Some(source) = hub.replication.clone() else {
        return empty(stream, "403 Forbidden", "").await;
    };
    let scratch = source
        .db_path
        .with_file_name(format!("replicate-{}.snapshot", ulid::Ulid::generate()));
    let target = scratch.clone();
    let built = tokio::task::spawn_blocking(move || {
        arena_store::build_standby_snapshot(&source.db_path, &target)
    })
    .await;
    let result = match built {
        Ok(Ok(tip)) => send_snapshot(stream, &scratch, tip).await,
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), "replication: the snapshot failed");
            empty(stream, "500 Internal Server Error", "").await;
            Ok(())
        }
        Err(_) => {
            empty(stream, "500 Internal Server Error", "").await;
            Ok(())
        }
    };
    let _ = result;
    let _ = tokio::fs::remove_file(&scratch).await;
}

async fn send_snapshot<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    file: &std::path::Path,
    tip: Option<(String, i64)>,
) -> std::io::Result<()> {
    let mut disk = tokio::fs::File::open(file).await?;
    let len = disk.metadata().await?.len();
    let (epoch, seq) = tip.unwrap_or_default();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.sqlite3\r\nContent-Length: {len}\r\n{SNAPSHOT_EPOCH_HEADER}: {epoch}\r\n{SNAPSHOT_SEQ_HEADER}: {seq}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    timeout(WRITE_TIMEOUT, stream.write_all(reply.as_bytes()))
        .await
        .map_err(|_| std::io::ErrorKind::TimedOut)??;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = disk.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        timeout(WRITE_TIMEOUT, stream.write_all(&buf[..n]))
            .await
            .map_err(|_| std::io::ErrorKind::TimedOut)??;
    }
    let _ = timeout(WRITE_TIMEOUT, stream.shutdown()).await;
    Ok(())
}

async fn send<S>(
    sink: &mut futures_util::stream::SplitSink<WebSocketStream<S>, Message>,
    frame: &ReplicateFrame,
) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Ok(text) = serde_json::to_string(frame) else {
        return false;
    };
    matches!(
        timeout(WRITE_TIMEOUT, sink.send(Message::text(text))).await,
        Ok(Ok(()))
    )
}

/// What one read of the log yields.
struct Read {
    changes: Vec<ChangeRecord>,
    keys: Vec<ReplicatedKey>,
    /// More rows follow this batch right away.
    more: bool,
}

fn read_batch(source: &ReplicationSource, after: Option<(String, i64)>) -> Result<Read, AppError> {
    let store = Store::open_reader(&source.db_path)?;
    let rows = store.replication_changes_after(
        after.as_ref().map(|(e, s)| (e.as_str(), *s)),
        REPLICATE_BATCH_ROWS + 1,
    )?;
    let more = rows.len() > REPLICATE_BATCH_ROWS;
    let mut changes = Vec::new();
    let mut bytes = 0usize;
    let mut cut = false;
    for c in rows.into_iter().take(REPLICATE_BATCH_ROWS) {
        bytes += c.row.len() * 4 / 3 + 512;
        if bytes > BATCH_BYTES && !changes.is_empty() {
            cut = true;
            break;
        }
        changes.push(c);
    }
    let mut players: Vec<String> = changes
        .iter()
        .filter(|c| c.tbl == "players")
        .map(|c| c.row_id.clone())
        .collect();
    players.sort();
    players.dedup();
    let keys = store
        .replication_key_states(&players)?
        .into_iter()
        .map(|(player_id, dek_wrapped)| ReplicatedKey {
            player_id,
            dek_wrapped,
        })
        .collect();
    Ok(Read {
        changes,
        keys,
        more: more || cut,
    })
}

/// Serves one replication stream on an upgraded WebSocket until it ends.
pub(crate) async fn serve<S>(ws: WebSocketStream<S>, peer: SocketAddr, hub: Arc<LanHub>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = ws.split();
    let open = match timeout(HELLO_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<ReplicateFrame>(&text).ok(),
        _ => None,
    };
    let Some(ReplicateFrame::Open {
        token,
        schema_version,
        from,
    }) = open
    else {
        let _ = send(&mut sink, &refusal(ErrorCode::BadFrame, "expected open")).await;
        return;
    };
    if let Err(refused) = authorise(&hub, peer, &token).await {
        let _ = send(&mut sink, &refused).await;
        return;
    }
    let ours = hub.status.borrow().schema_version;
    if schema_version < ours {
        let _ = send(
            &mut sink,
            &refusal(
                ErrorCode::SchemaBehind,
                "update this PC to follow the main hub",
            ),
        )
        .await;
        return;
    }
    let Some(source) = hub.replication.clone() else {
        return;
    };

    let start = source.clone();
    let wanted = from.clone();
    let checked = tokio::task::spawn_blocking(move || -> Result<_, AppError> {
        let store = Store::open_reader(&start.db_path)?;
        if let Some(p) = &wanted
            && !store.replication_holds(&p.epoch, p.seq)?
        {
            return Ok(None);
        }
        Ok(Some(store.replication_tip()?))
    })
    .await;
    let tip = match checked {
        Ok(Ok(Some(tip))) => tip,
        Ok(Ok(None)) => {
            let _ = send(
                &mut sink,
                &refusal(
                    ErrorCode::ReplicateGap,
                    "the main hub does not hold that position",
                ),
            )
            .await;
            return;
        }
        _ => {
            let _ = send(
                &mut sink,
                &refusal(ErrorCode::Internal, "the log read failed"),
            )
            .await;
            return;
        }
    };
    let ready = ReplicateFrame::Ready {
        tip: tip.map(|(epoch, seq)| ReplicatePosition { epoch, seq }),
    };
    if !send(&mut sink, &ready).await {
        return;
    }

    let mut cursor: Option<(String, i64)> = from.map(|p| (p.epoch, p.seq));
    let mut last_sent = Instant::now();
    // Whether the standby was told it has caught up with the log's end.
    let mut caught_up = false;
    loop {
        if !hub.is_primary() {
            let _ = send(
                &mut sink,
                &refusal(ErrorCode::NotPrimary, "this hub is no longer the primary"),
            )
            .await;
            return;
        }
        let src = source.clone();
        let at = cursor.clone();
        let read = match tokio::task::spawn_blocking(move || read_batch(&src, at)).await {
            Ok(Ok(r)) => r,
            _ => {
                let _ = send(
                    &mut sink,
                    &refusal(ErrorCode::Internal, "the log read failed"),
                )
                .await;
                return;
            }
        };
        if read.changes.is_empty() {
            if !caught_up {
                let live = ReplicateFrame::Batch {
                    changes: Vec::new(),
                    keys: Vec::new(),
                    live: true,
                };
                if !send(&mut sink, &live).await {
                    return;
                }
                caught_up = true;
                last_sent = Instant::now();
            }
            if last_sent.elapsed() >= PING_AFTER {
                if !send(&mut sink, &ReplicateFrame::Ping).await {
                    return;
                }
                last_sent = Instant::now();
            }
            // Wait a tick, but notice the standby closing.
            tokio::select! {
                () = tokio::time::sleep(LIVE_POLL) => {}
                incoming = stream.next() => match incoming {
                    None | Some(Err(_) | Ok(Message::Close(_))) => return,
                    Some(Ok(_)) => {}
                },
            }
            continue;
        }
        if let Some(last) = read.changes.last() {
            cursor = Some((last.epoch.clone(), last.seq));
        }
        let frame = ReplicateFrame::Batch {
            changes: read.changes.iter().map(wire).collect(),
            keys: read.keys,
            live: !read.more,
        };
        caught_up = !read.more;
        if !send(&mut sink, &frame).await {
            return;
        }
        last_sent = Instant::now();
    }
}

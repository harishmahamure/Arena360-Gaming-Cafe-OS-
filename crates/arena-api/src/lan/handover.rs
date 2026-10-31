//! The Android Hub's side of the handover to a Windows Hub (Story 10.17): `POST
//! /v1/handover/claim`, `GET /v1/handover/snapshot` and `POST /v1/handover/ack` on `:7401`, TLS
//! only.
//!
//! The Windows Hub proves it knows the code on the tablet's screen without sending it
//! ([`arena_core::handover::handover_proof`], bound to the certificate it saw). A wrong proof
//! counts against the code and against the caller's address; the sixth wrong attempt replaces the
//! code. The snapshot and the acknowledgement take the transfer token the claim answered.

use std::net::SocketAddr;
use std::time::Duration;

use arena_core::handover::Handover;
use arena_proto::{
    AppError, ErrorCode, HANDOVER_SNAPSHOT_TYPE, HandoverAck, HandoverClaim, HandoverRefused,
    HubSuccessor, SUCCESSOR_TTL_MS, SignedSuccessor, SuccessorOffer,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout};

use super::LanHub;
use super::media::{empty, unauthorized};
use super::pair::Answer;

const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// Wrong claims one address may make in [`ATTEMPT_WINDOW`] before it is refused outright. Each
/// code already survives only five wrong tries, so this bounds how many fresh codes an address can
/// guess at, not the codes themselves.
const MAX_WRONG_PER_WINDOW: u32 = 30;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_PEERS: usize = 1024;

/// Wrong handover claims per address.
#[derive(Default)]
pub(crate) struct Attempts(
    std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, (Instant, u32)>>,
);

impl Attempts {
    pub(crate) fn blocked(&self, ip: std::net::IpAddr, now: Instant) -> bool {
        let Ok(map) = self.0.lock() else { return true };
        map.get(&ip).is_some_and(|(since, n)| {
            now.duration_since(*since) < ATTEMPT_WINDOW && *n >= MAX_WRONG_PER_WINDOW
        })
    }

    pub(crate) fn wrong(&self, ip: std::net::IpAddr, now: Instant) {
        let Ok(mut map) = self.0.lock() else { return };
        if map.len() >= MAX_PEERS {
            map.retain(|_, (since, _)| now.duration_since(*since) < ATTEMPT_WINDOW);
        }
        let entry = map.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= ATTEMPT_WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
    }
}
const CHUNK: usize = 64 * 1024;

fn json<T: serde::Serialize>(status: &str, body: &T) -> Answer {
    Answer {
        status: status.into(),
        body: serde_json::to_string(body).unwrap_or_default(),
        retry_after_s: None,
    }
}

fn refusal(r: &HandoverRefused) -> Answer {
    let status = crate::ui_auth::status_of(r.code);
    json(
        &format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Error")
        ),
        r,
    )
}

fn plain(code: ErrorCode, message: &str) -> Answer {
    refusal(&HandoverRefused {
        code,
        message: message.into(),
        code_replaced: false,
    })
}

fn handover_of(hub: &LanHub) -> Result<&Handover, Answer> {
    if !hub.is_primary() {
        return Err(plain(ErrorCode::NotPrimary, "this hub is not the primary"));
    }
    hub.handover
        .as_ref()
        .map(|h| &*h.0)
        .ok_or_else(|| plain(ErrorCode::Unsupported, "this hub does not hand over"))
}

pub(crate) fn bearer(head: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(head);
    head.split("\r\n").skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("authorization") {
            return None;
        }
        let (scheme, token) = v.trim().split_once(' ')?;
        (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty())
            .then(|| token.trim().to_owned())
    })
}

/// `POST /v1/handover/claim`.
pub(crate) async fn claim(hub: &LanHub, peer: SocketAddr, body: &[u8]) -> Answer {
    let handover = match handover_of(hub) {
        Ok(h) => h,
        Err(a) => return a,
    };
    let Ok(request) = serde_json::from_slice::<HandoverClaim>(body) else {
        return failed(AppError::new(
            ErrorCode::BadFrame,
            "the body must be {hub_name, proof}",
        ));
    };
    if hub.handover_attempts.blocked(peer.ip(), Instant::now()) {
        return plain(ErrorCode::RateLimited, "too many attempts; try again later");
    }
    let Some(fingerprint) = hub.status.borrow().tls_fingerprint.clone() else {
        return plain(ErrorCode::Internal, "this hub has no certificate");
    };
    let scratch = hub
        .handover
        .as_ref()
        .map(|h| h.1.clone())
        .unwrap_or_default();
    match handover.claim(&request, &fingerprint, &scratch).await {
        Ok(done) => {
            announce_successor(hub, request.successor.as_ref()).await;
            json("200 OK", &done)
        }
        Err(r) => {
            if matches!(
                r.code,
                ErrorCode::PairCodeInvalid | ErrorCode::PairCodeExpired
            ) {
                hub.handover_attempts.wrong(peer.ip(), Instant::now());
            }
            tracing::warn!(
                code = r.code.as_str(),
                replaced = r.code_replaced,
                "handover claim refused"
            );
            refusal(&r)
        }
    }
}

fn failed(e: AppError) -> Answer {
    let status = crate::ui_auth::status_of(e.code);
    Answer {
        status: format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Error")
        ),
        body: serde_json::to_string(&e).unwrap_or_default(),
        retry_after_s: None,
    }
}

/// `POST /v1/handover/ack`.
pub(crate) async fn ack(hub: &LanHub, token: Option<String>, body: &[u8]) -> Answer {
    let handover = match handover_of(hub) {
        Ok(h) => h,
        Err(a) => return a,
    };
    let (Some(token), Ok(ack)) = (token, serde_json::from_slice::<HandoverAck>(body)) else {
        return failed(AppError::new(
            ErrorCode::BadFrame,
            "the body must be {sha256, last_seq} with the transfer token",
        ));
    };
    match handover.acknowledge(&token, &ack).await {
        Ok(()) => {
            tracing::info!(
                last_seq = ack.last_seq,
                "the branch moved to the Windows Hub"
            );
            // The branch lives elsewhere now: end every session, so each kiosk goes to the
            // successor it was given.
            hub.registry.close_all_with(&AppError::new(
                ErrorCode::HubHandingOver,
                "this branch has moved to another hub",
            ));
            json("200 OK", &serde_json::json!({}))
        }
        Err(r) => refusal(&r),
    }
}

/// Gives every connected kiosk the Windows Hub's address, signed with this Hub's own key
/// (Story 10.19). Nothing happens when the Windows Hub sent no address, when it is malformed, or
/// when this Hub has no key or branch to sign for: the kiosks then pair again.
async fn announce_successor(hub: &LanHub, offer: Option<&SuccessorOffer>) {
    let (Some(offer), Some(signer)) = (offer, hub.signer.as_ref()) else {
        return;
    };
    if !valid_address(&offer.address) || !valid_fingerprint(&offer.fingerprint) {
        tracing::warn!("handover: the successor address or fingerprint is malformed; not sent");
        return;
    }
    let Some(branch_id) = hub.branch_id().await else {
        tracing::warn!("handover: this hub has no branch id; the successor is not sent");
        return;
    };
    let successor = HubSuccessor {
        address: offer.address.clone(),
        fingerprint: offer.fingerprint.to_ascii_lowercase(),
        branch_id,
        expires_at: hub.clock.now_ms() + SUCCESSOR_TTL_MS,
    };
    let (Ok(cert), Ok(signature)) = (signer.cert_der(), signer.sign(&successor.signing_bytes()))
    else {
        tracing::error!("handover: the successor could not be signed");
        return;
    };
    let sent = hub.registry.push_successor(SignedSuccessor {
        successor,
        cert: hex(&cert),
        signature: hex(&signature),
    });
    tracing::info!(
        kiosks = sent,
        "handover: kiosks told where the branch moves"
    );
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn valid_fingerprint(f: &str) -> bool {
    f.len() == 64 && f.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `host:port` with a non-zero port.
fn valid_address(a: &str) -> bool {
    a.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && port.parse::<u16>().is_ok_and(|p| p != 0)
            && (!host.contains(':') || (host.starts_with('[') && host.ends_with(']')))
    })
}

/// Answers `GET /v1/handover/snapshot` read from `stream` as `head`.
pub(crate) async fn answer_snapshot<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    peer: SocketAddr,
    head: &[u8],
    hub: &LanHub,
) {
    let Ok(handover) = handover_of(hub) else {
        return empty(stream, "503 Service Unavailable", "").await;
    };
    if hub.throttle.is_blocked(peer.ip(), Instant::now()) {
        return unauthorized(stream).await;
    }
    let Some(token) = bearer(head) else {
        return unauthorized(stream).await;
    };
    let Some((path, facts)) = handover.snapshot_for(&token).await else {
        hub.throttle.record_failure(peer.ip(), Instant::now());
        return unauthorized(stream).await;
    };
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return empty(stream, "500 Internal Server Error", "").await;
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {HANDOVER_SNAPSHOT_TYPE}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        facts.bytes
    );
    if timeout(WRITE_TIMEOUT, stream.write_all(reply.as_bytes()))
        .await
        .is_err()
    {
        return;
    }
    let mut buf = vec![0u8; CHUNK];
    loop {
        let Ok(n) = file.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            break;
        }
        if !matches!(
            timeout(WRITE_TIMEOUT, stream.write_all(&buf[..n])).await,
            Ok(Ok(()))
        ) {
            return;
        }
    }
    let _ = timeout(WRITE_TIMEOUT, stream.shutdown()).await;
}

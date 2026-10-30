use std::net::SocketAddr;
use std::sync::Arc;

use arena_core::devices::{format_token, hash_token_secret, new_paired_token_secret};
use arena_proto::{
    Actor, ActorKind, AppError, ClientClass, DeviceKind, DeviceRegister, ElevatedBy, ElevationKind,
    ElevationMode, ErrorCode, Intent, IntentKind, IntentPayload, PAIR_SYSTEM_ACTOR, PairRequest,
    PairResponse, PairSecret, Permission,
};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::LanHub;
use crate::pairing::{PinOutcome, pin_paused, rate_limited};

pub const MAX_BODY_BYTES: usize = 4 * 1024;

pub(crate) struct Answer {
    pub status: String,
    pub body: String,
    pub retry_after_s: Option<u32>,
}

fn failed(e: AppError) -> Answer {
    let code = crate::ui_auth::status_of(e.code);
    let status = format!(
        "{} {}",
        code.as_u16(),
        code.canonical_reason().unwrap_or("Error")
    );
    let e = if e.code == ErrorCode::Internal {
        tracing::warn!(detail = %e.message, "kiosk pairing failed");
        AppError::new(ErrorCode::Internal, "the hub could not pair this kiosk")
    } else {
        e
    };
    Answer {
        status,
        body: serde_json::to_string(&e).unwrap_or_default(),
        retry_after_s: None,
    }
}

fn waiting(e: AppError, seconds: u32) -> Answer {
    Answer {
        retry_after_s: Some(seconds),
        ..failed(e)
    }
}

pub(crate) fn content_length(head: &[u8]) -> Result<usize, AppError> {
    let bad = |m: &str| AppError::new(ErrorCode::BadFrame, m);
    let text = std::str::from_utf8(head).map_err(|_| bad("the request head is not text"))?;
    let mut length = None;
    for line in text.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(bad("transfer-encoding is not accepted"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad("content-length must be a number"));
            }
            let n: usize = value
                .parse()
                .map_err(|_| bad("content-length is too large"))?;
            if length.is_some() {
                return Err(bad("content-length must appear once"));
            }
            length = Some(n);
        }
    }
    length.ok_or_else(|| bad("content-length is required"))
}

pub(crate) async fn read_body<S: AsyncRead + Unpin>(
    stream: &mut S,
    head: &[u8],
) -> Result<Vec<u8>, AppError> {
    let too_big = || AppError::new(ErrorCode::BadFrame, "the pairing request is too large");
    let end = head
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .ok_or_else(|| AppError::new(ErrorCode::BadFrame, "no request head"))?;
    let want = content_length(&head[..end])?;
    if want > MAX_BODY_BYTES {
        return Err(too_big());
    }
    let mut body = head[end..].to_vec();
    if body.len() > want {
        return Err(AppError::new(
            ErrorCode::BadFrame,
            "the body is longer than its length",
        ));
    }
    let mut chunk = [0u8; 1024];
    while body.len() < want {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|_| AppError::new(ErrorCode::BadFrame, "the body ended early"))?;
        if n == 0 {
            return Err(AppError::new(ErrorCode::BadFrame, "the body ended early"));
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > want {
            return Err(too_big());
        }
    }
    Ok(body)
}

/// `remote` is a pairing request that came through the tunnel (Story 8.41).
pub(crate) async fn handle(
    hub: &Arc<LanHub>,
    peer: SocketAddr,
    body: &[u8],
    remote: bool,
) -> Answer {
    match pair(hub, peer, body, remote).await {
        Ok(ok) => Answer {
            status: "200 OK".into(),
            body: serde_json::to_string(&ok).unwrap_or_default(),
            retry_after_s: None,
        },
        Err(Refusal::Error(e)) => failed(e),
        Err(Refusal::Paused(s)) => waiting(pin_paused(), s),
        Err(Refusal::Limited(s)) => waiting(rate_limited(), s),
    }
}

enum Refusal {
    Error(AppError),
    Paused(u32),
    Limited(u32),
}

impl From<AppError> for Refusal {
    fn from(e: AppError) -> Self {
        Self::Error(e)
    }
}

fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

fn secret_of(req: &PairRequest) -> Option<&str> {
    match req.secret() {
        Ok(PairSecret::Code(code)) => Some(code),
        _ => None,
    }
}

async fn pair(
    hub: &Arc<LanHub>,
    peer: SocketAddr,
    body: &[u8],
    remote: bool,
) -> Result<PairResponse, Refusal> {
    let Some(pairing) = hub.pairing.clone() else {
        return Err(AppError::new(ErrorCode::NotFound, "pairing is not available").into());
    };
    pairing.attempt(peer.ip()).map_err(Refusal::Limited)?;
    let req: PairRequest = serde_json::from_slice(body)
        .map_err(|_| AppError::new(ErrorCode::BadFrame, "the body must be a pairing request"))?;
    let secret = req.validate()?;
    if remote {
        // A staff PIN typed on a device across the internet is not a pairing method.
        if matches!(secret, PairSecret::Pin(_)) {
            return Err(
                AppError::new(ErrorCode::Forbidden, "a remote device pairs with a code").into(),
            );
        }
        super::session::require_tunnel_flag(hub).await?;
    }
    hub.refuse_while_restoring()?;
    let writer = match hub.writer.clone() {
        Some(w) if hub.is_primary() => w,
        _ => {
            return Err(AppError::new(ErrorCode::NotPrimary, "this hub is not the primary").into());
        }
    };
    let fingerprint = hub
        .status
        .borrow()
        .tls_fingerprint
        .clone()
        .ok_or_else(|| internal("the hub has no certificate fingerprint"))?;
    let elevated_by = match secret {
        PairSecret::Code(_) => None,
        PairSecret::Pin(pin) => Some(check_pin(hub, &pairing, peer, pin).await?),
    };
    let secret = new_paired_token_secret()?;
    let hash = {
        let secret = secret.clone();
        tokio::task::spawn_blocking(move || hash_token_secret(&secret))
            .await
            .map_err(|e| internal(format!("hash task: {e}")))??
    };
    let now = hub.clock.now_ms();
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::DeviceRegister,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: PAIR_SYSTEM_ACTOR.into(),
        },
        elevated_by: elevated_by.clone(),
        reason: None,
        payload: IntentPayload::DeviceRegister(DeviceRegister {
            device_id: None,
            class: ClientClass::Paired,
            device_kind: DeviceKind::Pc,
            label: req.pc_name.clone(),
            token_hash: hash,
            tv_number: None,
            group_id: None,
        }),
    };
    let method = if elevated_by.is_some() { "pin" } else { "code" };
    let elevator = intent.elevated_by.as_ref().map(|e| e.id.clone());
    let taken = match secret_of(&req) {
        Some(code) => Some(pairing.take(code)?),
        None => None,
    };
    let applied = match writer.submit_intent(intent).await {
        Ok(applied) => applied,
        Err(e) => {
            if let Some(taken) = taken
                && e.code != ErrorCode::HubSlow
            {
                pairing.restore(taken);
            }
            return Err(e.into());
        }
    };
    if let Some(taken) = taken {
        pairing.used(taken);
    }
    let device_id = applied
        .changes
        .iter()
        .find(|c| c.tbl == "devices")
        .map(|c| c.row_id.clone())
        .ok_or_else(|| internal("device.register wrote no device row"))?;
    tracing::info!(
        device_id = %device_id,
        peer = %peer.ip(),
        method,
        remote,
        elevated_by = elevator.as_deref().unwrap_or(""),
        "kiosk paired"
    );
    Ok(PairResponse {
        token: format_token(&device_id, &secret),
        device_id,
        hub_fingerprint: fingerprint,
    })
}

async fn check_pin(
    hub: &Arc<LanHub>,
    pairing: &Arc<crate::pairing::Pairing>,
    peer: SocketAddr,
    pin: &str,
) -> Result<ElevatedBy, Refusal> {
    let ip = peer.ip();
    let Some(staff) = hub.staff.clone() else {
        return Err(AppError::new(ErrorCode::Unsupported, "PIN pairing is not available").into());
    };
    pairing.pin_begin(ip).map_err(Refusal::Paused)?;
    let permit = match Arc::clone(&hub.verify_permits).acquire_owned().await {
        Ok(p) => p,
        Err(_) => {
            pairing.pin_finish(ip, PinOutcome::Unchecked);
            return Err(internal("pin checks closed").into());
        }
    };
    let pin = pin.to_owned();
    let matched = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        staff.match_pin(&[Permission::DeviceManage], "", &pin)
    })
    .await;
    match matched {
        Ok(Ok(Some((staff_id, version)))) => {
            pairing.pin_finish(ip, PinOutcome::Matched);
            Ok(ElevatedBy {
                kind: ElevationKind::Pin,
                id: staff_id,
                mode: ElevationMode::Hub,
                verifier_version: version,
            })
        }
        Ok(Ok(None)) => match pairing.pin_finish(ip, PinOutcome::Wrong) {
            Some(seconds) => Err(Refusal::Paused(seconds)),
            None => Err(arena_core::staff::pin_rejected().into()),
        },
        Ok(Err(e)) => {
            pairing.pin_finish(ip, PinOutcome::Unchecked);
            Err(e.into())
        }
        Err(e) => {
            pairing.pin_finish(ip, PinOutcome::Unchecked);
            Err(internal(format!("pin task: {e}")).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_is_read_once_and_transfer_encoding_is_refused() {
        let head = b"POST /pair HTTP/1.1\r\nHost: h\r\ncontent-LENGTH: 12\r\n\r\n";
        assert_eq!(content_length(head).unwrap(), 12);
        for bad in [
            &b"POST /pair HTTP/1.1\r\n\r\n"[..],
            b"POST /pair HTTP/1.1\r\nContent-Length: 12\r\nContent-Length: 12\r\n\r\n",
            b"POST /pair HTTP/1.1\r\nContent-Length: 12\r\nContent-Length: 13\r\n\r\n",
            b"POST /pair HTTP/1.1\r\nContent-Length: 12\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"POST /pair HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"POST /pair HTTP/1.1\r\nContent-Length: +12\r\n\r\n",
            b"POST /pair HTTP/1.1\r\nContent-Length: 1, 2\r\n\r\n",
        ] {
            assert_eq!(
                content_length(bad).unwrap_err().code,
                ErrorCode::BadFrame,
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}

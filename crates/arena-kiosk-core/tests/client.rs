//! `HubClient` against a fake Hub: pinning (no fallback), `HUB_SLOW` per request without
//! blocking concurrent ones, and pushed events.

use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_kiosk_core::{ClientError, HubClient, REQUEST_TIMEOUT};
use arena_proto::{
    AppError, AppliedSummary, ClientClass, ErrorCode, EventFrame, Frame, FrameBody, Hello, HelloOk,
    HubEvent, Versions, parse_json,
};
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

struct FakeHub {
    url: String,
    fingerprint: String,
    /// Every text frame the hub received after hello.
    seen: Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A TLS WebSocket server that answers `hello` with `hello.ok` plus one pushed event. Every
/// other frame is recorded and, by its fields: `revoke` → an empty-rid `TOKEN_REVOKED` and a
/// close; `stray` → an error on another rid, then `pong`; `ping_me` → a hub `ping` (`ka-1`),
/// then `pong`; `close` → a close. The client's own keepalive pings (`kp-*`) get no answer.
/// Anything else gets `pong` after its `delay_ms`.
async fn fake_hub() -> FakeHub {
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = rcgen::CertificateParams::new(vec!["arena360.local".to_owned()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let fingerprint = hex(&Sha256::digest(cert.der()));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from_pem_slice(cert.pem().as_bytes()).unwrap()],
        PrivateKeyDer::from_pem_slice(key.serialize_pem().as_bytes()).unwrap(),
    )
    .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_by_hub = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let seen = Arc::clone(&seen_by_hub);
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Ok(ws) = tokio_tungstenite::accept_async(tls).await else {
                    return;
                };
                let (sink, mut stream) = ws.split();
                let sink = Arc::new(tokio::sync::Mutex::new(sink));
                while let Some(Ok(Message::Text(t))) = stream.next().await {
                    let v = parse_json(t.as_str()).unwrap();
                    let rid = v["rid"].as_str().unwrap().to_owned();
                    if v["kind"] == "hello" {
                        let ok = Frame::new(
                            rid,
                            FrameBody::HelloOk(HelloOk {
                                class: ClientClass::Paired,
                                device_id: Some("D".into()),
                                versions: Versions {
                                    app: "0.1.0".into(),
                                },
                                primary: true,
                            }),
                        );
                        let ev = Frame::new(
                            "ev-0",
                            FrameBody::Event(EventFrame {
                                event: HubEvent::Applied(AppliedSummary {
                                    intent_id: "I".into(),
                                    changes: vec![],
                                }),
                            }),
                        );
                        let mut s = sink.lock().await;
                        s.send(Message::text(ok.to_text())).await.unwrap();
                        s.send(Message::text(ev.to_text())).await.unwrap();
                        continue;
                    }
                    seen.lock().unwrap().push(v.clone());
                    let send = |f: Frame| {
                        let sink = Arc::clone(&sink);
                        async move {
                            let _ = sink.lock().await.send(Message::text(f.to_text())).await;
                        }
                    };
                    if rid.starts_with("kp-") {
                        continue;
                    }
                    if v.get("revoke").is_some() {
                        let e = AppError::new(ErrorCode::TokenRevoked, "revoked");
                        send(Frame::error("", e)).await;
                        let _ = sink.lock().await.close().await;
                        return;
                    }
                    if v.get("close").is_some() {
                        let _ = sink.lock().await.close().await;
                        return;
                    }
                    if v.get("stray").is_some() {
                        let e = AppError::new(ErrorCode::Internal, "not yours");
                        send(Frame::error("other", e)).await;
                    }
                    if v.get("ping_me").is_some() {
                        send(Frame::new("ka-1", FrameBody::Ping)).await;
                    }
                    let delay = v["delay_ms"].as_u64().unwrap_or(0);
                    let sink = Arc::clone(&sink);
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        let pong = Frame::new(rid, FrameBody::Pong);
                        let _ = sink.lock().await.send(Message::text(pong.to_text())).await;
                    });
                }
            });
        }
    });
    FakeHub {
        url: format!("wss://127.0.0.1:{port}/"),
        fingerprint,
        seen,
    }
}

fn hello() -> Hello {
    Hello {
        credential: "token".into(),
        class: ClientClass::Paired,
        versions: Versions {
            app: "0.1.0".into(),
        },
        capabilities: vec![],
    }
}

#[tokio::test]
async fn connects_with_the_pinned_fingerprint_and_receives_events() {
    let hub = fake_hub().await;
    let client = HubClient::connect(&hub.url, &hub.fingerprint, hello())
        .await
        .unwrap();
    assert_eq!(client.hello_ok().device_id.as_deref(), Some("D"));
    let mut events = client.events().unwrap();
    assert!(client.events().is_none(), "the stream is taken once");
    let ev = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .unwrap()
        .unwrap();
    let HubEvent::Applied(s) = ev;
    assert_eq!(s.intent_id, "I");
    let reply = client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(reply.body, FrameBody::Pong);
}

#[tokio::test]
async fn a_wrong_fingerprint_fails_with_no_fallback() {
    let hub = fake_hub().await;
    let mut wrong = hub.fingerprint.clone().into_bytes();
    wrong[0] = if wrong[0] == b'0' { b'1' } else { b'0' };
    let wrong = String::from_utf8(wrong).unwrap();
    let err = HubClient::connect(&hub.url, &wrong, hello())
        .await
        .err()
        .unwrap();
    // Both fingerprints are reported, so the log line can name them.
    assert_eq!(
        err,
        ClientError::PinMismatch {
            expected: wrong,
            presented: hub.fingerprint.clone(),
        }
    );
}

#[tokio::test]
async fn a_slow_reply_is_hub_slow_while_a_concurrent_request_succeeds() {
    let hub = fake_hub().await;
    let client = Arc::new(
        HubClient::connect(&hub.url, &hub.fingerprint, hello())
            .await
            .unwrap(),
    );
    let started = Instant::now();
    let slow = {
        let c = Arc::clone(&client);
        tokio::spawn(async move {
            c.request("ping", serde_json::json!({"delay_ms": 6_000}))
                .await
        })
    };
    // The second request answers at once, while the first is still waiting.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let fast = client
        .request("ping", serde_json::json!({"delay_ms": 0}))
        .await
        .unwrap();
    assert_eq!(fast.body, FrameBody::Pong);
    assert!(started.elapsed() < Duration::from_secs(2));

    let err = slow.await.unwrap().unwrap_err();
    assert_eq!(err, ClientError::HubSlow);
    assert_eq!(err.code(), Some(ErrorCode::HubSlow));
    let waited = started.elapsed();
    assert!(
        waited >= REQUEST_TIMEOUT && waited < REQUEST_TIMEOUT + Duration::from_secs(2),
        "{waited:?}"
    );
}

#[tokio::test]
async fn an_empty_rid_error_fails_pending_requests_and_closes() {
    let hub = fake_hub().await;
    let client = Arc::new(
        HubClient::connect(&hub.url, &hub.fingerprint, hello())
            .await
            .unwrap(),
    );
    let waiting = {
        let c = Arc::clone(&client);
        tokio::spawn(async move {
            c.request("ping", serde_json::json!({"delay_ms": 60_000}))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let err = client
        .request("ping", serde_json::json!({"revoke": true}))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::TokenRevoked));
    let err = waiting.await.unwrap().unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::TokenRevoked));
    let closed = tokio::time::timeout(Duration::from_secs(2), client.closed())
        .await
        .unwrap();
    assert_eq!(closed.error.map(|e| e.code), Some(ErrorCode::TokenRevoked));
    let started = Instant::now();
    let err = client
        .request("ping", serde_json::Value::Null)
        .await
        .unwrap_err();
    assert_eq!(err, ClientError::Closed);
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[tokio::test]
async fn an_error_on_another_rid_is_not_a_session_error() {
    let hub = fake_hub().await;
    let client = HubClient::connect(&hub.url, &hub.fingerprint, hello())
        .await
        .unwrap();
    let reply = client
        .request("ping", serde_json::json!({"stray": true}))
        .await
        .unwrap();
    assert_eq!(reply.body, FrameBody::Pong);
    let _ = client
        .request("ping", serde_json::json!({"close": true}))
        .await;
    let closed = tokio::time::timeout(Duration::from_secs(2), client.closed())
        .await
        .unwrap();
    assert_eq!(closed.error, None);
}

#[tokio::test]
async fn the_client_answers_hub_pings() {
    let hub = fake_hub().await;
    let client = HubClient::connect(&hub.url, &hub.fingerprint, hello())
        .await
        .unwrap();
    client
        .request("ping", serde_json::json!({"ping_me": true}))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let answered = hub
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "pong" && v["rid"] == "ka-1");
        if answered {
            break;
        }
        assert!(Instant::now() < deadline, "no pong for ka-1");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_silent_hub_is_pinged_then_treated_as_closed() {
    let hub = fake_hub().await;
    let client = HubClient::connect(&hub.url, &hub.fingerprint, hello())
        .await
        .unwrap();
    // Virtual time from here: the hub never answers the client's keepalive pings.
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let closed = client.closed().await;
    assert_eq!(closed.error, None);
    assert!(started.elapsed() >= arena_kiosk_core::IDLE_TIMEOUT);
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        hub.seen
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["rid"].as_str().is_some_and(|r| r.starts_with("kp-")))
    );
}

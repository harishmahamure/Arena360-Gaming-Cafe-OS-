//! The Hub's side of the branch fence (Story 9.3, AD-24).
//!
//! The cloud issues every epoch counter by compare-and-set on the branch fence. A Hub asks for
//! the next one with [`claim_fence`] (`POST /v1/fence/claim`); the heartbeat settles a provisional
//! epoch through the same fence. Either way the Hub remembers the highest counter it has seen in
//! `lease.max_seen_counter`, so a lease taken offline mints `max seen + 1`.

use arena_core::WriterHandle;
use arena_proto::{ErrorCode, FenceClaimRequest, epoch_parts, fenced_epoch};

use crate::cloud::{CloudClient, CloudFailure};

/// How a claim ended when the cloud answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The fence is now `epoch`, held by this Hub.
    Won { epoch: String },
    /// The fence was not the epoch the Hub expected. `current` is the fence the cloud named (a
    /// Hub that cannot read one learns nothing new and should ask the heartbeat).
    Fenced { current: Option<String> },
}

/// Raises the highest counter seen to the counter of `epoch` (`<counter>:<instance>`). Nothing
/// is recorded for a text that is not an epoch, or by a Hub whose writer is closed; neither
/// fails the caller.
pub(crate) async fn note_epoch(writer: &WriterHandle, epoch: &str) {
    let Some((counter, _)) = epoch_parts(epoch) else {
        return;
    };
    if let Err(e) = writer.note_epoch_seen(counter).await {
        tracing::warn!(
            code = e.code.as_str(),
            "fence: could not note the epoch seen"
        );
    }
}

/// Claims the next epoch for `req.instance_id`. A win and a `FENCED` refusal both teach the Hub a
/// counter, which is noted before this returns. Any other failure (the WAN, a bad token, a
/// refused body) comes back as it is, with nothing noted.
pub async fn claim_fence(
    cloud: &CloudClient,
    token: &str,
    writer: &WriterHandle,
    req: &FenceClaimRequest,
) -> Result<Claim, CloudFailure> {
    match cloud.fence_claim(token, req).await {
        Ok(won) => {
            note_epoch(writer, &won.epoch).await;
            Ok(Claim::Won { epoch: won.epoch })
        }
        Err(CloudFailure::Refused { error, .. }) if error.code == ErrorCode::Fenced => {
            let current = fenced_epoch(&error.message).map(str::to_owned);
            if let Some(epoch) = &current {
                note_epoch(writer, epoch).await;
            }
            Ok(Claim::Fenced { current })
        }
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudEndpoint;
    use crate::lease::LeaseTask;
    use arena_core::{Clock, HubStatus, ManualClock, status_channel};
    use arena_proto::{FenceClaimReason, FenceFork};
    use arena_store::Store;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const T0: i64 = 1_800_000_000_000;
    const OWN: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

    async fn serve(status: u16, reply: String) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = sock.write_all(format!("{head}{reply}").as_bytes()).await;
            }
        });
        (port, seen)
    }

    async fn writer() -> (tempfile::TempDir, std::path::PathBuf, WriterHandle) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let store = Store::open(&path, T0).unwrap();
        let (status, _rx) = status_channel(HubStatus::opened(84, OWN));
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(T0));
        let lease = LeaseTask::start(store, OWN.into(), clock, status)
            .await
            .unwrap();
        let writer = lease.writer().unwrap();
        std::mem::forget(lease);
        (dir, path, writer)
    }

    fn request() -> FenceClaimRequest {
        FenceClaimRequest {
            expected: "3:A".into(),
            instance_id: OWN.into(),
            reason: FenceClaimReason::Promote,
            fork: FenceFork {
                parent_epoch: "3:A".into(),
                parent_seq: 40,
            },
        }
    }

    fn client(port: u16) -> CloudClient {
        CloudClient::new(&CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap())
            .unwrap()
    }

    #[tokio::test]
    async fn a_won_claim_is_noted_and_sent_with_the_token() {
        let (port, seen) = serve(200, r#"{"epoch":"4:HUBB"}"#.into()).await;
        let (_dir, _path, writer) = writer().await;
        let out = claim_fence(&client(port), "hub_x.y", &writer, &request())
            .await
            .unwrap();
        assert_eq!(
            out,
            Claim::Won {
                epoch: "4:HUBB".into()
            }
        );
        assert_eq!(writer.max_seen_counter().await.unwrap(), 4);
        let raw = seen.lock().unwrap()[0].to_ascii_lowercase();
        assert!(raw.starts_with("post /v1/fence/claim "), "{raw}");
        assert!(raw.contains("authorization: bearer hub_x.y"), "{raw}");
        let body: serde_json::Value =
            serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["reason"], "promote");
        assert_eq!(body["fork"]["parent_seq"], 40);
    }

    #[tokio::test]
    async fn a_fenced_claim_notes_the_counter_the_cloud_named() {
        let reply = r#"{"code":"FENCED","message":"A newer Hub has taken over this branch (epoch 7:HUBZ)."}"#;
        let (port, _) = serve(409, reply.into()).await;
        let (_dir, _path, writer) = writer().await;
        let out = claim_fence(&client(port), "hub_x.y", &writer, &request())
            .await
            .unwrap();
        assert_eq!(
            out,
            Claim::Fenced {
                current: Some("7:HUBZ".into())
            }
        );
        assert_eq!(writer.max_seen_counter().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn a_fenced_claim_that_names_nothing_notes_nothing() {
        let reply = r#"{"code":"FENCED","message":"No Hub has reported to this branch yet."}"#;
        let (port, _) = serve(409, reply.into()).await;
        let (_dir, _path, writer) = writer().await;
        let out = claim_fence(&client(port), "hub_x.y", &writer, &request())
            .await
            .unwrap();
        assert_eq!(out, Claim::Fenced { current: None });
        assert_eq!(writer.max_seen_counter().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn other_failures_come_back_and_note_nothing() {
        let reply = r#"{"code":"RATE_LIMITED","message":"slow down"}"#;
        let (port, _) = serve(429, reply.into()).await;
        let (_dir, _path, writer) = writer().await;
        let out = claim_fence(&client(port), "hub_x.y", &writer, &request()).await;
        assert!(matches!(
            out,
            Err(CloudFailure::Refused { status: 429, .. })
        ));
        assert_eq!(writer.max_seen_counter().await.unwrap(), 0);
    }
}

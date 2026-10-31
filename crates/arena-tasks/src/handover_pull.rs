//! The Windows Hub pulls the branch's final snapshot from the Android Hub (Story 10.17).
//!
//! [`HandoverPuller::join`] runs the whole exchange for "Waiting for your Android Hub": claim the
//! code at the tablet's address, pull the snapshot over TLS with the transfer token, check its
//! hash, size, branch id, `seq` and header, and acknowledge it. The Android Hub's certificate is
//! self-signed, so the channel is not trusted by a certificate: both sides instead prove they know
//! the code *for the certificate each saw* ([`arena_core::handover::handover_proof`]), and every
//! later connection must present the certificate the claim was made on. Importing the file is
//! Story 10.18.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_core::handover::{
    Handover, HandoverJoinControl, JoinFuture, PROOF_CLAIM, PROOF_REPLY, handover_proof,
};
use arena_core::{HubStatus, SnapshotReader};
use arena_proto::{
    DEFAULT_WEB_PORT, ErrorCode, ExportHeader, HANDOVER_ACK_PATH, HANDOVER_CLAIM_PATH,
    HANDOVER_PURPOSE, HANDOVER_SNAPSHOT_PATH, HandoverAck, HandoverClaim, HandoverClaimed,
    HandoverJoinRequest, HandoverRefused, HandoverSnapshotFacts, HandoverView, UiError,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STEP_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_HEAD: usize = 16 * 1024;
const MAX_JSON: usize = 64 * 1024;

fn err(code: ErrorCode, message: impl Into<String>) -> UiError {
    UiError {
        code,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

fn field_err(field: &str, message: &str) -> UiError {
    UiError {
        field: Some(field.into()),
        ..err(ErrorCode::ValidationFailed, message)
    }
}

fn unreachable_tablet(why: &str) -> UiError {
    tracing::warn!(why, "handover: the tablet could not be reached");
    err(
        ErrorCode::NoHub,
        "The PC could not reach the tablet. Check that both are on the same network, or use a file.",
    )
}

fn not_a_hub() -> UiError {
    err(
        ErrorCode::NoHub,
        "The tablet did not answer as an Arena 360 Hub.",
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Accepts the certificate the Android Hub presents and remembers its fingerprint; the proofs
/// bind the code to it.
#[derive(Debug)]
struct Seen {
    provider: Arc<CryptoProvider>,
    fingerprint: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for Seen {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if let Ok(mut seen) = self.fingerprint.lock() {
            *seen = Some(hex(&Sha256::digest(end_entity.as_ref())));
        }
        Ok(ServerCertVerified::assertion())
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

/// `host` or `host:port` (also `[v6]:port`) of the Android Hub's `:7401` listener.
fn split_address(address: &str) -> Result<(String, u16), UiError> {
    let bad = || field_err("address", "Type the tablet's address, like 192.168.1.20.");
    let a = address.trim();
    if a.is_empty() || a.contains(['/', ' ', '@']) {
        return Err(bad());
    }
    if let Some(rest) = a.strip_prefix('[') {
        let (host, tail) = rest.split_once(']').ok_or_else(bad)?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse::<u16>().map_err(|_| bad())?,
            None if tail.is_empty() => DEFAULT_WEB_PORT,
            None => return Err(bad()),
        };
        return Ok((host.to_owned(), port));
    }
    if a.matches(':').count() > 1 {
        // A bare IPv6 address.
        return a
            .parse::<IpAddr>()
            .map(|_| (a.to_owned(), DEFAULT_WEB_PORT))
            .map_err(|_| bad());
    }
    match a.split_once(':') {
        Some((host, port)) if !host.is_empty() => {
            Ok((host.to_owned(), port.parse::<u16>().map_err(|_| bad())?))
        }
        Some(_) => Err(bad()),
        None => Ok((a.to_owned(), DEFAULT_WEB_PORT)),
    }
}

/// A TLS connection to the Android Hub and the fingerprint of the certificate it presented.
struct Conn {
    stream: TlsStream<TcpStream>,
    fingerprint: String,
}

async fn connect(host: &str, port: u16) -> Result<Conn, UiError> {
    let fingerprint: Arc<Mutex<Option<String>>> = Arc::default();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| err(ErrorCode::Internal, e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Seen {
            provider,
            fingerprint: Arc::clone(&fingerprint),
        }))
        .with_no_client_auth();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let name = match bare.parse::<IpAddr>() {
        Ok(ip) => ServerName::IpAddress(ip.into()),
        Err(_) => ServerName::try_from(bare.to_owned())
            .map_err(|_| field_err("address", "That address is not valid."))?,
    };
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((bare, port)))
        .await
        .map_err(|_| unreachable_tablet("connect timed out"))?
        .map_err(|e| unreachable_tablet(&e.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let stream = timeout(
        CONNECT_TIMEOUT,
        TlsConnector::from(Arc::new(config)).connect(name, tcp),
    )
    .await
    .map_err(|_| unreachable_tablet("tls timed out"))?
    .map_err(|e| unreachable_tablet(&e.to_string()))?;
    let fingerprint = fingerprint
        .lock()
        .ok()
        .and_then(|f| f.clone())
        .ok_or_else(|| unreachable_tablet("no certificate"))?;
    Ok(Conn {
        stream,
        fingerprint,
    })
}

/// One answered request: the status, the declared length and what of the body came with the head.
struct Reply {
    status: u16,
    length: Option<u64>,
    first: Vec<u8>,
    stream: TlsStream<TcpStream>,
}

/// What a request carries besides its line: the bearer token and a JSON body.
#[derive(Default)]
struct Extras<'a> {
    token: Option<&'a str>,
    body: Option<&'a [u8]>,
}

async fn exchange(
    conn: Conn,
    (host, port): (&str, u16),
    method: &str,
    path: &str,
    extras: Extras<'_>,
) -> Result<Reply, UiError> {
    let mut stream = conn.stream;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if let Some(token) = extras.token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(body) = extras.body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    let sent = async {
        stream.write_all(head.as_bytes()).await?;
        if let Some(body) = extras.body {
            stream.write_all(body).await?;
        }
        stream.flush().await
    };
    timeout(STEP_TIMEOUT, sent)
        .await
        .map_err(|_| unreachable_tablet("write timed out"))?
        .map_err(|e| unreachable_tablet(&e.to_string()))?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(not_a_hub());
        }
        let n = timeout(STEP_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| unreachable_tablet("read timed out"))?
            .map_err(|e| unreachable_tablet(&e.to_string()))?;
        if n == 0 {
            return Err(unreachable_tablet("closed before the answer"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(not_a_hub)?;
    let length = text.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| v.trim().parse::<u64>().ok())
            .flatten()
    });
    Ok(Reply {
        status,
        length,
        first: buf[end..].to_vec(),
        stream,
    })
}

/// The whole body of a small JSON answer.
async fn json_body(reply: &mut Reply) -> Result<Vec<u8>, UiError> {
    let mut body = std::mem::take(&mut reply.first);
    let mut chunk = [0u8; 4096];
    while body.len() <= MAX_JSON {
        let n = timeout(STEP_TIMEOUT, reply.stream.read(&mut chunk))
            .await
            .map_err(|_| unreachable_tablet("read timed out"))?
            .unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

/// What the Android Hub refused with, as the UI says it.
async fn refused(mut reply: Reply) -> UiError {
    let body = json_body(&mut reply).await.unwrap_or_default();
    if let Ok(r) = serde_json::from_slice::<HandoverRefused>(&body) {
        let message = if r.code_replaced {
            "Too many wrong codes. The tablet now shows a new code.".to_owned()
        } else {
            match r.code {
                ErrorCode::PairCodeInvalid => "That code is not right.".to_owned(),
                ErrorCode::PairCodeExpired => {
                    "That code has expired. Confirm the move on the tablet again.".to_owned()
                }
                ErrorCode::HubHandingOver => "The tablet is already handing over. Use a file, or cancel the move on the tablet.".to_owned(),
                ErrorCode::RateLimited => {
                    "Too many attempts. Try again in a little while.".to_owned()
                }
                _ => r.message,
            }
        };
        return err(r.code, message);
    }
    if let Ok(e) = serde_json::from_slice::<UiError>(&body) {
        return e;
    }
    not_a_hub()
}

/// Pulls the final snapshot of a Hub move (Story 10.17).
pub struct HandoverPuller {
    handover: Arc<Handover>,
    reader: Arc<SnapshotReader>,
    status: watch::Receiver<HubStatus>,
    hub_name: String,
    /// Where the received snapshot is kept until Story 10.18 imports it.
    dir: PathBuf,
}

impl HandoverPuller {
    pub fn new(
        handover: Arc<Handover>,
        reader: Arc<SnapshotReader>,
        status: watch::Receiver<HubStatus>,
        hub_name: String,
        dir: PathBuf,
    ) -> Self {
        Self {
            handover,
            reader,
            status,
            hub_name,
            dir,
        }
    }

    async fn run(&self, req: HandoverJoinRequest) -> Result<HandoverView, UiError> {
        let branch_name = self.status.borrow().migration_wait.clone().ok_or_else(|| {
            err(
                ErrorCode::StepNotReady,
                "This PC is not waiting for an Android Hub.",
            )
        })?;
        if let Some(view) = self.handover.view().await {
            return Ok(view);
        }
        let (host, port) = split_address(&req.address)?;
        let at = (host.as_str(), port);
        let code = req.code.trim().to_owned();
        let reader = Arc::clone(&self.reader);
        let branch_id = tokio::task::spawn_blocking(move || reader.hub_branch_id())
            .await
            .map_err(|_| err(ErrorCode::Internal, "the identity read failed"))?
            .map_err(|e| err(e.code, e.message))?
            .ok_or_else(|| err(ErrorCode::StepNotReady, "This PC is not paired yet."))?;

        // 1. Claim the code on the certificate the tablet presents now.
        let conn = connect(&host, port).await?;
        let fingerprint = conn.fingerprint.clone();
        let claim = HandoverClaim {
            hub_name: self.hub_name.clone(),
            proof: handover_proof(&code, &fingerprint, PROOF_CLAIM, &self.hub_name),
        };
        let body = serde_json::to_vec(&claim).unwrap_or_default();
        let mut reply = exchange(
            conn,
            at,
            "POST",
            HANDOVER_CLAIM_PATH,
            Extras {
                body: Some(&body),
                ..Extras::default()
            },
        )
        .await?;
        if reply.status != 200 {
            return Err(refused(reply).await);
        }
        let claimed: HandoverClaimed =
            serde_json::from_slice(&json_body(&mut reply).await?).map_err(|_| not_a_hub())?;
        // The tablet must know the code too, for this certificate.
        let reply_proof = handover_proof(&code, &fingerprint, PROOF_REPLY, &claimed.transfer_token);
        if claimed.fingerprint != fingerprint || claimed.proof != reply_proof {
            tracing::warn!("handover: the tablet's answer did not prove the code");
            return Err(err(
                ErrorCode::Forbidden,
                "That is not the tablet that showed the code.",
            ));
        }
        let facts = claimed.snapshot.clone();
        if facts.branch_id != branch_id {
            return Err(err(
                ErrorCode::RestoreWrongBranch,
                "That tablet belongs to another branch.",
            ));
        }
        let token = claimed.transfer_token.as_str();

        // 2. Pull the snapshot on the same certificate.
        let conn = connect(&host, port).await?;
        if conn.fingerprint != fingerprint {
            return Err(changed());
        }
        let reply = exchange(
            conn,
            at,
            "GET",
            HANDOVER_SNAPSHOT_PATH,
            Extras {
                token: Some(token),
                ..Extras::default()
            },
        )
        .await?;
        if reply.status != 200 {
            return Err(refused(reply).await);
        }
        // The hash names the file: only a SHA-256 in hex may.
        if facts.sha256.len() != 64 || !facts.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(
                ErrorCode::RestoreCorrupt,
                "The snapshot from the tablet does not match what it said it sent (hash).",
            ));
        }
        let file_name = format!("handover-{}.a360", &facts.sha256[..16]);
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            err(
                ErrorCode::Internal,
                format!("could not make the folder: {e}"),
            )
        })?;
        let target = self.dir.join(&file_name);
        let part = self.dir.join(format!(".{file_name}.part"));
        if let Err(e) = receive(reply, &facts, &part, &target).await {
            let _ = std::fs::remove_file(&part);
            let _ = std::fs::remove_file(&target);
            return Err(e);
        }

        // 3. Say it arrived and matched, then remember it.
        // The tablet answers the same ask twice the same way, so a lost answer is asked again. The
        // checked file stays when it is still unknown whether the tablet stepped down: it may be
        // the only copy.
        let mut acknowledged = acknowledge(&host, port, &fingerprint, token, &facts).await;
        for _ in 0..2 {
            if acknowledged.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            acknowledged = acknowledge(&host, port, &fingerprint, token, &facts).await;
        }
        acknowledged?;
        tracing::info!(
            last_seq = facts.last_seq,
            "handover: the snapshot was received"
        );
        self.handover
            .record_received(target, file_name, facts, &branch_name)
            .await
            .map_err(|e| err(e.code, e.message))
    }
}

fn changed() -> UiError {
    err(
        ErrorCode::Forbidden,
        "The tablet's certificate changed during the move.",
    )
}

async fn acknowledge(
    host: &str,
    port: u16,
    fingerprint: &str,
    token: &str,
    facts: &HandoverSnapshotFacts,
) -> Result<(), UiError> {
    let conn = connect(host, port).await?;
    if conn.fingerprint != fingerprint {
        return Err(changed());
    }
    let ack = serde_json::to_vec(&HandoverAck {
        sha256: facts.sha256.clone(),
        last_seq: facts.last_seq,
    })
    .unwrap_or_default();
    let reply = exchange(
        conn,
        (host, port),
        "POST",
        HANDOVER_ACK_PATH,
        Extras {
            token: Some(token),
            body: Some(&ack),
        },
    )
    .await?;
    if reply.status == 200 {
        Ok(())
    } else {
        Err(refused(reply).await)
    }
}

/// Streams the body into `part`, checks it and moves it to `target`: size and SHA-256 as the claim
/// stated, then the file's own header (`migrate`, the branch, the epoch and `seq`).
async fn receive(
    mut reply: Reply,
    facts: &HandoverSnapshotFacts,
    part: &Path,
    target: &Path,
) -> Result<(), UiError> {
    let bad = |what: &str| {
        err(
            ErrorCode::RestoreCorrupt,
            format!("The snapshot from the tablet did not check out ({what}). Use a file instead."),
        )
    };
    let disk = |e: std::io::Error| {
        err(
            ErrorCode::Internal,
            format!("could not write the snapshot: {e}"),
        )
    };
    if reply.length != Some(facts.bytes) {
        return Err(bad("size"));
    }
    let mut file = tokio::fs::File::create(part).await.map_err(disk)?;
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    let mut chunk = vec![0u8; 64 * 1024];
    let mut data = std::mem::take(&mut reply.first);
    loop {
        if data.is_empty() {
            let n = timeout(STEP_TIMEOUT, reply.stream.read(&mut chunk))
                .await
                .map_err(|_| unreachable_tablet("read timed out"))?
                .map_err(|e| unreachable_tablet(&e.to_string()))?;
            if n == 0 {
                break;
            }
            data = chunk[..n].to_vec();
        }
        got += data.len() as u64;
        if got > facts.bytes {
            return Err(bad("size"));
        }
        hasher.update(&data);
        file.write_all(&data).await.map_err(disk)?;
        data.clear();
    }
    file.sync_all().await.map_err(disk)?;
    drop(file);
    if got != facts.bytes {
        return Err(bad("size"));
    }
    if hex(&hasher.finalize()) != facts.sha256 {
        return Err(bad("hash"));
    }
    let path = part.to_owned();
    let header = tokio::task::spawn_blocking(move || {
        ExportHeader::read(&mut std::fs::File::open(path)?).map_err(std::io::Error::other)
    })
    .await
    .map_err(|_| err(ErrorCode::Internal, "the check failed"))?
    .map_err(|_| bad("header"))?;
    if header.purpose.as_deref() != Some(HANDOVER_PURPOSE) {
        return Err(bad("purpose"));
    }
    if header.branch_id != facts.branch_id {
        return Err(bad("branch"));
    }
    if header.last_seq != facts.last_seq || header.epoch != facts.epoch {
        return Err(bad("seq"));
    }
    std::fs::rename(part, target).map_err(|e| {
        err(
            ErrorCode::Internal,
            format!("could not keep the snapshot: {e}"),
        )
    })
}

impl HandoverJoinControl for HandoverPuller {
    fn join(&self, req: HandoverJoinRequest) -> JoinFuture<'_> {
        Box::pin(self.run(req))
    }
}

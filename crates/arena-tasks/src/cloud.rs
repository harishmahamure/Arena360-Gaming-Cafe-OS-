use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arena_core::{Clock, HubPairer, HubStatus, PairFuture, PairGate, WriterHandle};
use arena_proto::{
    AppError, CATALOG_MANIFEST_PATH, CatalogManifest, CommandResult, CommandResultAck,
    CommandsPage, ErrorCode, FenceClaimRequest, FenceClaimResponse, HeartbeatRequest,
    HeartbeatResponse, HubPairRequest, HubPairResponse, HubPlatform, HubRole, KekEscrowRequest,
    KekEscrowResponse, KekFetchRequest, KekFetchResponse, KekRequest, KekRequestResponse,
    MIGRATION_CLAIM_PATH, MIGRATION_IMPORTED_PATH, MigrationClaimRequest, MigrationClaimResponse,
    MigrationImportedRequest, NotifyRequest, NotifyResponse, PresignRequest, PresignResponse,
    RestorePlan, RestorePlanRequest, SETUP_MAX_BYTES, SetupTemplateOutcome, SetupTemplateStored,
    UiError, normalize_pairing_code, valid_pairing_code,
};
use arena_store::HubIdentity;
use sha2::{Digest, Sha256};

use crate::connectivity::CloudHost;
use crate::keystore::{Keystore, mint_kek};

pub const CLOUD_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a media download may go without receiving a byte.
pub const MEDIA_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long one presigned PUT of a change-log batch may take in all.
pub const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RETRY_AFTER_S: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudEndpoint {
    pub host: CloudHost,
    pub plain_http: bool,
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl CloudEndpoint {
    pub fn parse(s: &str, allow_plain_loopback: bool) -> Result<Self, String> {
        let s = s.trim().trim_end_matches('/');
        if let Some(rest) = s.strip_prefix("http://") {
            let host = CloudHost::parse(rest)?;
            if !is_loopback(&host.host) {
                return Err(format!(
                    "--cloud-host {s:?}: plain http:// is only for a loopback cloud"
                ));
            }
            if !allow_plain_loopback {
                return Err(format!(
                    "--cloud-host {s:?}: plain http:// needs a dev-fixtures build"
                ));
            }
            return Ok(Self {
                host,
                plain_http: true,
            });
        }
        let host = match s.strip_prefix("https://") {
            Some(rest) if !rest.contains(':') || rest.ends_with(']') => {
                CloudHost::parse(&format!("{rest}:443"))?
            }
            Some(rest) => CloudHost::parse(rest)?,
            None => CloudHost::parse(s)?,
        };
        Ok(Self {
            host,
            plain_http: false,
        })
    }

    pub fn base_url(&self) -> String {
        let scheme = if self.plain_http { "http" } else { "https" };
        let host = if self.host.host.contains(':') {
            format!("[{}]", self.host.host)
        } else {
            self.host.host.clone()
        };
        format!("{scheme}://{host}:{}", self.host.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudFailure {
    Refused {
        status: u16,
        error: AppError,
        retry_after_s: Option<u32>,
    },
    Unreachable(String),
}

/// Why a presigned download (Story 9.12) did not produce a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectFailure {
    /// No answer, the stream broke, or the file could not be written: try again.
    Unreachable(String),
    /// The bucket answered this status (a redirect is refused and lands here too). 403 is an
    /// expired link; 404 and 410 are a missing object.
    Status(u16),
}

/// Why a media download did not produce a verified file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaFailure {
    /// No answer, or the stream broke (the WAN is down).
    Unreachable(String),
    /// The CDN answered with this status (a redirect is refused and lands here too).
    Refused(u16),
    /// The body is not the file the manifest describes.
    Mismatch(&'static str),
}

#[derive(Debug, Clone)]
pub struct CloudClient {
    http: reqwest::Client,
    /// For media downloads: no total timeout (a video is large), a read timeout instead.
    media: reqwest::Client,
    /// For presigned PUTs of upload files: a total timeout long enough for a slow WAN.
    upload: reqwest::Client,
    base: String,
}

impl CloudClient {
    pub fn new(endpoint: &CloudEndpoint) -> Result<Self, String> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let builder = |tls: rustls::ClientConfig| {
            reqwest::Client::builder()
                .tls_backend_preconfigured(tls)
                .https_only(!endpoint.plain_http)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("arena-hub/", env!("CARGO_PKG_VERSION")))
        };
        let http = builder(tls.clone())
            .timeout(CLOUD_TIMEOUT)
            .build()
            .map_err(|e| e.without_url().to_string())?;
        let upload = builder(tls.clone())
            .connect_timeout(CLOUD_TIMEOUT)
            .timeout(UPLOAD_TIMEOUT)
            .build()
            .map_err(|e| e.without_url().to_string())?;
        let media = builder(tls)
            .connect_timeout(CLOUD_TIMEOUT)
            .read_timeout(MEDIA_READ_TIMEOUT)
            .build()
            .map_err(|e| e.without_url().to_string())?;
        Ok(Self {
            http,
            media,
            upload,
            base: endpoint.base_url(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub async fn pair(&self, req: &HubPairRequest) -> Result<HubPairResponse, CloudFailure> {
        self.post("/v1/pair", None, req).await
    }

    pub async fn heartbeat(
        &self,
        token: &str,
        req: &HeartbeatRequest,
    ) -> Result<HeartbeatResponse, CloudFailure> {
        self.post("/v1/heartbeat", Some(token), req).await
    }

    /// A standby Hub refreshes its own licence (Story 9.20).
    pub async fn standby_licence(
        &self,
        token: &str,
        req: &arena_proto::StandbyLicenceRequest,
    ) -> Result<arena_proto::StandbyLicenceResponse, CloudFailure> {
        let path = format!("/v1{}", arena_proto::STANDBY_LICENCE_PATH);
        self.post(&path, Some(token), req).await
    }

    pub async fn commands(
        &self,
        token: &str,
        after: i64,
        epoch: &str,
    ) -> Result<CommandsPage, CloudFailure> {
        let call = self
            .http
            .get(format!(
                "{}/v1/commands?after={after}&epoch={epoch}",
                self.base
            ))
            .bearer_auth(token);
        self.send(call).await
    }

    pub async fn command_result(
        &self,
        token: &str,
        id: i64,
        req: &CommandResult,
    ) -> Result<CommandResultAck, CloudFailure> {
        self.post(&format!("/v1/commands/{id}/result"), Some(token), req)
            .await
    }

    /// Keeps `file` (the text of the branch's setup file) as the branch's cloud template
    /// (Story 5.37).
    pub async fn put_setup_template(
        &self,
        token: &str,
        branch_id: &str,
        file: String,
    ) -> Result<SetupTemplateStored, CloudFailure> {
        let call = self
            .http
            .put(format!(
                "{}/v1/branches/{branch_id}/setup-template",
                self.base
            ))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .bearer_auth(token)
            .body(file);
        self.send(call).await
    }

    /// Asks the cloud to send one player message, once per `outbox_id` (Story 8.28).
    pub async fn notify(
        &self,
        token: &str,
        req: &NotifyRequest,
    ) -> Result<NotifyResponse, CloudFailure> {
        self.post("/v1/notify", Some(token), req).await
    }

    /// Claims the next epoch on the branch fence by compare-and-set (Story 9.3).
    pub async fn fence_claim(
        &self,
        token: &str,
        req: &FenceClaimRequest,
    ) -> Result<FenceClaimResponse, CloudFailure> {
        self.post("/v1/fence/claim", Some(token), req).await
    }

    /// The Windows Hub of a Hub move claims the epoch after the Android Hub's (Story 10.18).
    pub async fn migration_claim(
        &self,
        token: &str,
        req: &MigrationClaimRequest,
    ) -> Result<MigrationClaimResponse, CloudFailure> {
        self.post(MIGRATION_CLAIM_PATH, Some(token), req).await
    }

    /// The Windows Hub says it serves the imported branch (Story 10.18).
    pub async fn migration_imported(
        &self,
        token: &str,
        req: &MigrationImportedRequest,
    ) -> Result<MigrationClaimResponse, CloudFailure> {
        self.post(MIGRATION_IMPORTED_PATH, Some(token), req).await
    }

    /// Leaves the branch key, wrapped for the escrow public key, with the cloud (Story 9.2).
    pub async fn kek_escrow(
        &self,
        token: &str,
        req: &KekEscrowRequest,
    ) -> Result<KekEscrowResponse, CloudFailure> {
        self.post("/v1/kek/escrow", Some(token), req).await
    }

    /// Asks the owner to release the branch key to this replacement Hub (Story 9.9).
    pub async fn kek_request(
        &self,
        token: &str,
        req: &KekRequest,
    ) -> Result<KekRequestResponse, CloudFailure> {
        self.post("/v1/kek/request", Some(token), req).await
    }

    /// Asks whether the owner decided the key request, and takes the re-wrapped key once it is
    /// approved (Story 9.10).
    pub async fn kek_fetch(
        &self,
        token: &str,
        req: &KekFetchRequest,
    ) -> Result<KekFetchResponse, CloudFailure> {
        self.post("/v1/kek/fetch", Some(token), req).await
    }

    /// Asks for read links to the branch's cloud backup (Story 9.12).
    pub async fn restore_plan(
        &self,
        token: &str,
        req: &RestorePlanRequest,
    ) -> Result<RestorePlan, CloudFailure> {
        self.post("/v1/restore/plan", Some(token), req).await
    }

    /// Streams the object a presigned `url` leads to into `dest` (created or truncated) and
    /// returns its size (Story 9.12). The link is the proof, so no token is sent and redirects are
    /// refused. A dropped stream is a failure; `dest` then holds a partial file.
    pub async fn download_object(&self, url: &str, dest: &Path) -> Result<u64, ObjectFailure> {
        use tokio::io::AsyncWriteExt;
        let mut res = self
            .media
            .get(url)
            .send()
            .await
            .map_err(|e| ObjectFailure::Unreachable(e.without_url().to_string()))?;
        let status = res.status();
        if !status.is_success() {
            return Err(ObjectFailure::Status(status.as_u16()));
        }
        let io = |what: &str, e: std::io::Error| ObjectFailure::Unreachable(format!("{what}: {e}"));
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| io("create the file", e))?;
        let mut size = 0u64;
        while let Some(chunk) = res
            .chunk()
            .await
            .map_err(|e| ObjectFailure::Unreachable(e.without_url().to_string()))?
        {
            size += chunk.len() as u64;
            file.write_all(&chunk)
                .await
                .map_err(|e| io("write the file", e))?;
        }
        file.sync_all().await.map_err(|e| io("write the file", e))?;
        Ok(size)
    }

    /// Asks for short-lived upload links under the branch's prefix of the data bucket (Story 8.1).
    pub async fn presign(
        &self,
        token: &str,
        req: &PresignRequest,
    ) -> Result<PresignResponse, CloudFailure> {
        self.post("/v1/presign", Some(token), req).await
    }

    /// PUTs `body` to a presigned `url` (Story 8.2). The link is the proof, so no token is sent
    /// and redirects are refused. Anything but a 2xx answer is a failure.
    pub async fn put_object(&self, url: &str, body: Vec<u8>) -> Result<(), CloudFailure> {
        let res = self
            .upload
            .put(url)
            .body(body)
            .send()
            .await
            .map_err(|e| CloudFailure::Unreachable(e.without_url().to_string()))?;
        let status = res.status();
        if status.is_success() {
            return Ok(());
        }
        let retry_after_s = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u32>().ok());
        Err(match status.as_u16() {
            429 => CloudFailure::Refused {
                status: 429,
                error: AppError::new(ErrorCode::RateLimited, "Too many requests."),
                retry_after_s,
            },
            code => CloudFailure::Unreachable(format!("the bucket answered {code}")),
        })
    }

    /// Downloads the setup file a signed link leads to (Story 5.38). The link is the proof, so
    /// no token is sent; the answer is at most `SETUP_MAX_BYTES`.
    pub async fn download_setup_template(&self, path: &str) -> Result<Vec<u8>, CloudFailure> {
        if !path.starts_with("/v1/setup-template/") {
            return Err(CloudFailure::Unreachable("unexpected link".into()));
        }
        let res = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .map_err(|e| CloudFailure::Unreachable(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        if res
            .content_length()
            .is_some_and(|n| n > SETUP_MAX_BYTES as u64)
        {
            return Err(CloudFailure::Unreachable("the file is too large".into()));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| CloudFailure::Unreachable(e.without_url().to_string()))?;
        if status == 200 {
            return if bytes.len() > SETUP_MAX_BYTES {
                Err(CloudFailure::Unreachable("the file is too large".into()))
            } else {
                Ok(bytes.to_vec())
            };
        }
        match serde_json::from_slice::<AppError>(&bytes) {
            Ok(error) => Err(CloudFailure::Refused {
                status,
                error,
                retry_after_s: None,
            }),
            Err(_) => Err(CloudFailure::Unreachable(format!(
                "status {status} without an error body"
            ))),
        }
    }

    /// Tells the cloud how the Hub got on with a setup template link (Story 5.38).
    pub async fn setup_template_outcome(
        &self,
        token: &str,
        branch_id: &str,
        outcome: &SetupTemplateOutcome,
    ) -> Result<(), CloudFailure> {
        let _: serde_json::Value = self
            .post(
                &format!("/v1/branches/{branch_id}/setup-template/outcome"),
                Some(token),
                outcome,
            )
            .await?;
        Ok(())
    }

    /// The games published or withdrawn since catalogue version `since` (Story 6.1), or `None`
    /// when the cloud answers 304: nothing changed.
    pub async fn catalog_manifest(
        &self,
        token: &str,
        since: u64,
    ) -> Result<Option<CatalogManifest>, CloudFailure> {
        let call = self
            .http
            .get(format!(
                "{}/v1{CATALOG_MANIFEST_PATH}?since={since}",
                self.base
            ))
            .bearer_auth(token);
        self.send_maybe(call).await
    }

    /// Streams the media file at `url` into `part`, counting its bytes and hashing it on the
    /// way, and keeps it only when its size and SHA-256 are the manifest's. The URL is a CDN
    /// link: no token is sent, redirects are refused, and https is required unless the cloud
    /// endpoint is plain-http loopback. On any failure `part` is removed.
    pub async fn fetch_media(
        &self,
        url: &str,
        part: &Path,
        bytes: u64,
        sha256: &str,
    ) -> Result<(), MediaFailure> {
        let result = self.stream_media(url, part, bytes, sha256).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(part).await;
        }
        result
    }

    async fn stream_media(
        &self,
        url: &str,
        part: &Path,
        bytes: u64,
        sha256: &str,
    ) -> Result<(), MediaFailure> {
        use tokio::io::AsyncWriteExt;
        let unreachable =
            |e: reqwest::Error| MediaFailure::Unreachable(e.without_url().to_string());
        let mut res = self.media.get(url).send().await.map_err(unreachable)?;
        let status = res.status().as_u16();
        if status != 200 {
            return Err(MediaFailure::Refused(status));
        }
        if res.content_length().is_some_and(|n| n != bytes) {
            return Err(MediaFailure::Mismatch("the size differs from the manifest"));
        }
        let mut file = tokio::fs::File::create(part).await.map_err(|e| {
            MediaFailure::Unreachable(format!("cannot write {}: {e}", part.display()))
        })?;
        let mut hasher = Sha256::new();
        let mut total: u64 = 0;
        while let Some(chunk) = res.chunk().await.map_err(unreachable)? {
            total += chunk.len() as u64;
            if total > bytes {
                return Err(MediaFailure::Mismatch("longer than the manifest says"));
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| MediaFailure::Unreachable(format!("cannot write the file: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| MediaFailure::Unreachable(format!("cannot write the file: {e}")))?;
        file.sync_all()
            .await
            .map_err(|e| MediaFailure::Unreachable(format!("cannot write the file: {e}")))?;
        drop(file);
        if total != bytes {
            return Err(MediaFailure::Mismatch("shorter than the manifest says"));
        }
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != sha256 {
            return Err(MediaFailure::Mismatch(
                "the SHA-256 differs from the manifest",
            ));
        }
        Ok(())
    }

    async fn post<Q: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
        req: &Q,
    ) -> Result<R, CloudFailure> {
        let body = serde_json::to_vec(req)
            .map_err(|e| CloudFailure::Unreachable(format!("encode: {e}")))?;
        let mut call = self
            .http
            .post(format!("{}{path}", self.base))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(token) = bearer {
            call = call.bearer_auth(token);
        }
        self.send(call).await
    }

    async fn send<R: serde::de::DeserializeOwned>(
        &self,
        call: reqwest::RequestBuilder,
    ) -> Result<R, CloudFailure> {
        self.send_maybe(call)
            .await?
            .ok_or_else(|| CloudFailure::Unreachable("status 304 without an error body".into()))
    }

    /// Like [`send`](Self::send), but a 304 is `Ok(None)`.
    async fn send_maybe<R: serde::de::DeserializeOwned>(
        &self,
        call: reqwest::RequestBuilder,
    ) -> Result<Option<R>, CloudFailure> {
        let res = call
            .send()
            .await
            .map_err(|e| CloudFailure::Unreachable(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        if status == 304 {
            return Ok(None);
        }
        let retry_after_s = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u32>().ok());
        let bytes = res
            .bytes()
            .await
            .map_err(|e| CloudFailure::Unreachable(e.without_url().to_string()))?;
        if status == 200 {
            return serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| CloudFailure::Unreachable(format!("unreadable answer: {e}")));
        }
        match serde_json::from_slice::<AppError>(&bytes) {
            Ok(error) => Err(CloudFailure::Refused {
                status,
                error,
                retry_after_s,
            }),
            Err(_) if status == 429 => Err(CloudFailure::Refused {
                status,
                error: AppError::new(ErrorCode::RateLimited, "Too many requests."),
                retry_after_s,
            }),
            Err(_) => Err(CloudFailure::Unreachable(format!(
                "status {status} without an error body"
            ))),
        }
    }
}

fn ui_error(code: ErrorCode, message: &str) -> UiError {
    UiError {
        code,
        message: message.into(),
        field: None,
        retry_after_s: None,
    }
}

pub fn invalid_code() -> UiError {
    ui_error(
        ErrorCode::PairCodeInvalid,
        "That code didn't work. Get a new code from your browser.",
    )
}

pub fn unreachable() -> UiError {
    ui_error(
        ErrorCode::CloudUnreachable,
        "Can't reach Arena 360 — check the internet and try again",
    )
}

fn internal(detail: &str) -> UiError {
    tracing::warn!(detail = %detail, "hub pairing failed");
    ui_error(
        ErrorCode::Internal,
        "the hub could not complete this request",
    )
}

pub fn map_failure(f: CloudFailure) -> UiError {
    match f {
        CloudFailure::Unreachable(detail) => {
            tracing::warn!(detail = %detail, "hub pairing: cloud unreachable");
            unreachable()
        }
        CloudFailure::Refused {
            status,
            error,
            retry_after_s,
        } => match error.code {
            ErrorCode::PairCodeInvalid | ErrorCode::PairCodeExpired => invalid_code(),
            ErrorCode::FeatureNotEntitled => UiError {
                code: ErrorCode::FeatureNotEntitled,
                message: error.message,
                field: error.field.or_else(|| Some("platform".into())),
                retry_after_s: None,
            },
            ErrorCode::RateLimited => UiError {
                code: ErrorCode::RateLimited,
                message: "Too many tries. Try again later.".into(),
                field: None,
                retry_after_s: Some(retry_after_s.unwrap_or(DEFAULT_RETRY_AFTER_S).max(1)),
            },
            _ if status >= 500 => {
                tracing::warn!(
                    status,
                    code = error.code.as_str(),
                    "hub pairing: cloud failed"
                );
                unreachable()
            }
            other => internal(&format!(
                "cloud refused the pairing with {}",
                other.as_str()
            )),
        },
    }
}

pub struct CloudPairer {
    cloud: CloudClient,
    writer: Option<WriterHandle>,
    keystore: Arc<dyn Keystore>,
    gate: PairGate,
    clock: Arc<dyn Clock>,
    platform: HubPlatform,
    hub_instance_id: String,
    version: String,
    /// This PC's name, sent when it pairs so a Hub move can show it on the Android Hub.
    hub_name: Option<String>,
    kek_store: Option<Arc<dyn Keystore>>,
    templates: Option<Arc<crate::SetupTemplateInbox>>,
    /// Where a replacement Hub keeps its one-time key, and the status it turns to "waiting".
    restore: Option<(Arc<dyn Keystore>, tokio::sync::watch::Sender<HubStatus>)>,
    busy: tokio::sync::Mutex<()>,
}

impl CloudPairer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cloud: CloudClient,
        writer: Option<WriterHandle>,
        keystore: Arc<dyn Keystore>,
        gate: PairGate,
        clock: Arc<dyn Clock>,
        platform: HubPlatform,
        hub_instance_id: String,
        version: String,
    ) -> Self {
        Self {
            cloud,
            writer,
            keystore,
            gate,
            clock,
            platform,
            hub_instance_id,
            version,
            hub_name: None,
            kek_store: None,
            templates: None,
            restore: None,
            busy: tokio::sync::Mutex::new(()),
        }
    }

    /// The PC's own name, sent with the pairing request (Story 10.16).
    pub fn with_hub_name(mut self, name: String) -> Self {
        self.hub_name = Some(name);
        self
    }

    /// Pairing also writes a fresh branch key here (the Android Hub, whose keystore holds the
    /// token and the key together). Without it the key is minted lazily on first use.
    pub fn with_kek_store(mut self, kek_store: Arc<dyn Keystore>) -> Self {
        self.kek_store = Some(kek_store);
        self
    }

    /// A branch with history pairs this Hub as a replacement (Story 9.9): its one-time key goes
    /// to `restore_key` and `status` turns to "Restore this branch". No branch key is made.
    pub fn with_restore(
        mut self,
        restore_key: Arc<dyn Keystore>,
        status: tokio::sync::watch::Sender<HubStatus>,
    ) -> Self {
        self.restore = Some((restore_key, status));
        self
    }

    /// Where the setup template link of `branch.bootstrap` is left for the Hub to apply
    /// (Story 5.38).
    pub fn with_templates(mut self, inbox: Arc<crate::SetupTemplateInbox>) -> Self {
        self.templates = Some(inbox);
        self
    }

    async fn write_secret(&self, store: Arc<dyn Keystore>, secret: Vec<u8>) -> Result<(), UiError> {
        tokio::task::spawn_blocking(move || store.put(&secret))
            .await
            .map_err(|e| internal(&format!("keystore task: {e}")))?
            .map_err(|e| internal(&format!("keystore write: {e}")))
    }

    /// The branch has history: this Hub keeps the token and a one-time key, never makes a branch
    /// key and applies no bootstrap, then waits to be restored.
    async fn pair_as_replacement(
        &self,
        identity: HubIdentity,
        paired: HubPairResponse,
    ) -> Result<(), UiError> {
        let Some((restore_key, status)) = &self.restore else {
            return Err(internal("this hub cannot be paired as a replacement"));
        };
        let store = Arc::clone(restore_key);
        tokio::task::spawn_blocking(move || crate::restore_public_key(store.as_ref()))
            .await
            .map_err(|e| internal(&format!("keystore task: {e}")))?
            .map_err(|e| internal(&format!("keystore write: {e}")))?;
        self.write_secret(
            Arc::clone(&self.keystore),
            paired.token.clone().into_bytes(),
        )
        .await?;
        let writer = self
            .writer
            .clone()
            .ok_or_else(|| internal("no writer for the pairing"))?;
        let written = writer
            .record_replacement_pairing(identity, paired.licence, &paired.branch_name)
            .await
            .map_err(|e| internal(&format!("{}: {}", e.code.as_str(), e.message)))?;
        if !written {
            return Err(ui_error(
                ErrorCode::AlreadySet,
                "this hub is already paired",
            ));
        }
        status.send_modify(|s| s.restore_pending = Some(paired.branch_name.clone()));
        self.gate.mark_paired();
        tracing::info!(
            hub_id = %paired.hub_id,
            branch_id = %paired.branch_id,
            tier = paired.tier.as_str(),
            "hub paired as a replacement; waiting to be restored"
        );
        Ok(())
    }

    /// Pairs the Windows Hub of a Hub move (Story 10.16): the token and licence are kept, no
    /// bootstrap is applied and no branch key is made (Story 10.18 receives it). The Hub opens in
    /// "Waiting for your Android Hub" and serves nothing.
    async fn pair_for_migration(
        &self,
        identity: HubIdentity,
        paired: HubPairResponse,
    ) -> Result<(), UiError> {
        let Some((_, status)) = &self.restore else {
            return Err(internal("this hub cannot be paired for a move"));
        };
        let writer = self
            .writer
            .clone()
            .ok_or_else(|| internal("no writer for the pairing"))?;
        self.write_secret(
            Arc::clone(&self.keystore),
            paired.token.clone().into_bytes(),
        )
        .await?;
        let written = writer
            .record_migration_pairing(identity, paired.licence, &paired.branch_name)
            .await
            .map_err(|e| internal(&format!("{}: {}", e.code.as_str(), e.message)))?;
        if !written {
            return Err(ui_error(
                ErrorCode::AlreadySet,
                "this hub is already paired",
            ));
        }
        status.send_modify(|s| s.migration_wait = Some(paired.branch_name.clone()));
        self.gate.mark_paired();
        tracing::info!(
            hub_id = %paired.hub_id,
            branch_id = %paired.branch_id,
            tier = paired.tier.as_str(),
            "hub paired for a move; waiting for the Android Hub"
        );
        Ok(())
    }

    async fn run(&self, raw: &str) -> Result<(), UiError> {
        let _one_at_a_time = self.busy.lock().await;
        if self.gate.is_paired() {
            return Err(ui_error(
                ErrorCode::AlreadySet,
                "this hub is already paired",
            ));
        }
        let code = normalize_pairing_code(raw);
        if !valid_pairing_code(&code) {
            return Err(invalid_code());
        }
        let Some(writer) = self.writer.clone() else {
            return Err(ui_error(
                ErrorCode::NotPrimary,
                "this hub is not the primary writer",
            ));
        };
        let paired = self
            .cloud
            .pair(&HubPairRequest {
                code,
                platform: self.platform,
                hub_instance_id: self.hub_instance_id.clone(),
                version: self.version.clone(),
                // This install pairs as the main Hub; the standby install arrives with the
                // standby Hub itself (Story 9.19).
                role: HubRole::Main,
                hub_name: self.hub_name.clone(),
            })
            .await
            .map_err(map_failure)?;
        let identity = HubIdentity {
            tenant_id: paired.tenant_id.clone(),
            branch_id: paired.branch_id.clone(),
            hub_id: paired.hub_id.clone(),
            tier: paired.tier,
            paired_at: self.clock.now_ms(),
        };
        if self.gate.needs_repair() {
            // The key that wrapped the secrets is lost. The branch data stays as it is: only the
            // identity and licence change, then the token and a fresh branch key are wrapped.
            if !writer
                .record_repairing(identity, paired.licence.clone())
                .await
                .map_err(|e| internal(&format!("{}: {}", e.code.as_str(), e.message)))?
            {
                tracing::warn!("re-pair refused: the code belongs to another branch");
                return Err(invalid_code());
            }
            self.write_secret(
                Arc::clone(&self.keystore),
                paired.token.clone().into_bytes(),
            )
            .await?;
            // A Hub waiting to be restored makes no branch key (Story 9.9).
            let waiting = self
                .restore
                .as_ref()
                .is_some_and(|(_, status)| status.borrow().restore_pending.is_some());
            if let Some(kek) = self.kek_store.as_ref().filter(|_| !waiting) {
                let fresh = mint_kek().map_err(|e| internal(&format!("branch key: {e}")))?;
                self.write_secret(Arc::clone(kek), fresh.to_vec()).await?;
            }
            self.gate.mark_paired();
            tracing::info!(
                hub_id = %paired.hub_id,
                tier = paired.tier.as_str(),
                platform = self.platform.as_str(),
                "hub paired again after its keys were lost"
            );
            return Ok(());
        }
        if paired.restore_pending {
            return self.pair_as_replacement(identity, paired).await;
        }
        if paired.migration {
            return self.pair_for_migration(identity, paired).await;
        }
        if paired.bootstrap.is_none() {
            return Err(internal("the pair response has no bootstrap"));
        }
        self.write_secret(
            Arc::clone(&self.keystore),
            paired.token.clone().into_bytes(),
        )
        .await?;
        if let Some(kek) = &self.kek_store {
            let fresh = mint_kek().map_err(|e| internal(&format!("branch key: {e}")))?;
            self.write_secret(Arc::clone(kek), fresh.to_vec()).await?;
        }
        let template = paired.bootstrap.as_ref().and_then(|b| b.template.clone());
        let written = writer
            .record_pairing(identity, paired.licence, paired.bootstrap)
            .await
            .map_err(|e| match e.code {
                ErrorCode::AlreadyBootstrapped => ui_error(e.code, &e.message),
                _ => internal(&format!("{}: {}", e.code.as_str(), e.message)),
            })?;
        if !written {
            return Err(ui_error(
                ErrorCode::AlreadySet,
                "this hub is already paired",
            ));
        }
        if let (Some(inbox), Some(link)) = (&self.templates, template) {
            inbox.offer(link);
        }
        self.gate.mark_paired();
        tracing::info!(
            hub_id = %paired.hub_id,
            tenant_id = %paired.tenant_id,
            branch_id = %paired.branch_id,
            tier = paired.tier.as_str(),
            platform = self.platform.as_str(),
            "hub paired with the cloud"
        );
        Ok(())
    }
}

impl HubPairer for CloudPairer {
    fn pair<'a>(&'a self, code: &'a str) -> PairFuture<'a> {
        Box::pin(self.run(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_default_to_https_and_plain_http_is_dev_loopback_only() {
        let prod = CloudEndpoint::parse("api.arena360.cloud:443", false).unwrap();
        assert_eq!(prod.base_url(), "https://api.arena360.cloud:443");
        assert_eq!(
            CloudEndpoint::parse("https://api.staging.arena360.cloud", false)
                .unwrap()
                .base_url(),
            "https://api.staging.arena360.cloud:443"
        );
        let dev = CloudEndpoint::parse("http://127.0.0.1:8080", true).unwrap();
        assert!(dev.plain_http);
        assert_eq!(dev.base_url(), "http://127.0.0.1:8080");
        assert_eq!(dev.host.port, 8080);
        assert!(CloudEndpoint::parse("http://127.0.0.1:8080", false).is_err());
        assert!(CloudEndpoint::parse("http://api.arena360.cloud:80", true).is_err());
        assert!(CloudEndpoint::parse("http://10.0.0.5:8080", true).is_err());
        assert!(CloudEndpoint::parse("http://localhost:8080", true).is_ok());
    }

    fn refused(code: ErrorCode, status: u16, retry: Option<u32>) -> CloudFailure {
        CloudFailure::Refused {
            status,
            error: AppError::new(code, "cloud says"),
            retry_after_s: retry,
        }
    }

    #[test]
    fn cloud_answers_map_to_what_the_hub_ui_shows() {
        assert_eq!(
            map_failure(refused(ErrorCode::PairCodeInvalid, 401, None)).code,
            ErrorCode::PairCodeInvalid
        );
        let expired = map_failure(refused(ErrorCode::PairCodeExpired, 401, None));
        assert_eq!(expired, invalid_code());
        let entitled = map_failure(refused(ErrorCode::FeatureNotEntitled, 403, None));
        assert_eq!(entitled.code, ErrorCode::FeatureNotEntitled);
        assert_eq!(entitled.field.as_deref(), Some("platform"));
        let limited = map_failure(refused(ErrorCode::RateLimited, 429, Some(120)));
        assert_eq!(
            (limited.code, limited.retry_after_s),
            (ErrorCode::RateLimited, Some(120))
        );
        assert_eq!(
            map_failure(refused(ErrorCode::RateLimited, 429, None)).retry_after_s,
            Some(60)
        );
        assert_eq!(
            map_failure(CloudFailure::Unreachable("dns".into())).code,
            ErrorCode::CloudUnreachable
        );
        assert_eq!(
            map_failure(refused(ErrorCode::Internal, 500, None)).code,
            ErrorCode::CloudUnreachable
        );
        assert_eq!(
            map_failure(refused(ErrorCode::ValidationFailed, 422, None)).code,
            ErrorCode::Internal
        );
    }

    #[tokio::test]
    async fn a_closed_port_is_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let client = CloudClient::new(
            &CloudEndpoint::parse(&format!("http://127.0.0.1:{port}"), true).unwrap(),
        )
        .unwrap();
        let err = client
            .pair(&HubPairRequest {
                code: "7K2M9PQR".into(),
                platform: HubPlatform::Android,
                hub_instance_id: "i".into(),
                version: "0".into(),
                role: HubRole::Main,
                hub_name: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, CloudFailure::Unreachable(_)), "{err:?}");
        assert_eq!(map_failure(err).code, ErrorCode::CloudUnreachable);
    }
}

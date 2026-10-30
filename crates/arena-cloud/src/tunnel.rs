//! The Cloudflare Tunnel of a branch with the `tunnel` licence flag (Story 8.39): a
//! [`TunnelProvider`] creates and removes the named tunnel, its DNS name and its Access
//! application, and hands out the connector token. It is either the recorded [`FakeTunnels`]
//! (dev and tests) or [`CloudflareTunnels`], which calls the Cloudflare API. Every operation is
//! idempotent (lookups by name before creates, a missing object on delete is success), so a
//! heartbeat that failed half way simply repeats. Errors carry a stable code, never a token, a
//! credential or a response body.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::time::Duration;

use arena_proto::TUNNEL_ORIGIN;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::config::get_env;

/// The Cloudflare API token (a secret: never logged). Needs Tunnel, Access and DNS edit rights.
pub const CF_API_TOKEN_ENV: &str = "ARENA_CF_API_TOKEN";
/// The Cloudflare account that owns the tunnels and Access applications.
pub const CF_ACCOUNT_ID_ENV: &str = "ARENA_CF_ACCOUNT_ID";
/// The zone of `arena360.cloud`, where each tunnel hostname gets its CNAME.
pub const CF_ZONE_ID_ENV: &str = "ARENA_CF_ZONE_ID";
/// Optional: the reusable Access policy attached to every branch application.
pub const CF_ACCESS_POLICY_ID_ENV: &str = "ARENA_CF_ACCESS_POLICY_ID";
/// Optional: the API base, for a mock server (default the production API).
pub const CF_API_BASE_ENV: &str = "ARENA_CF_API_BASE";

const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4";
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a signed-in Access session lasts.
const ACCESS_SESSION: &str = "24h";

/// What the provider made for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisioned {
    pub tunnel_id: String,
    pub access_app_id: String,
}

/// Why a provider call failed: a stable code and no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelError(pub String);

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TunnelError {}

/// An operation of the recorded fake that a test can make fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeOp {
    Ensure,
    Credential,
    Delete,
}

#[derive(Default)]
struct FakeState {
    /// Branch id -> what exists for it.
    live: BTreeMap<String, Provisioned>,
    created: u32,
    deleted: u32,
    credentials: u32,
    failing: BTreeMap<&'static str, u32>,
}

/// The recorded fake: keeps the tunnels it "created" in memory, hands out numbered credentials
/// and fails an operation a set number of times when a test asks. Nothing leaves the process.
#[derive(Default)]
pub struct FakeTunnels {
    state: Mutex<FakeState>,
}

impl FakeTunnels {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn op_name(op: FakeOp) -> &'static str {
        match op {
            FakeOp::Ensure => "ensure",
            FakeOp::Credential => "credential",
            FakeOp::Delete => "delete",
        }
    }

    /// Makes the next `times` calls of `op` fail.
    pub fn fail(&self, op: FakeOp, times: u32) {
        self.with(|s| {
            s.failing.insert(Self::op_name(op), times);
        });
    }

    fn check(&self, op: FakeOp) -> Result<(), TunnelError> {
        self.with(|s| match s.failing.get_mut(Self::op_name(op)) {
            Some(n) if *n > 0 => {
                *n -= 1;
                Err(TunnelError(format!("FAKE_{}_FAILED", Self::op_name(op))))
            }
            _ => Ok(()),
        })
    }

    /// The branches that have a tunnel now.
    pub fn live(&self) -> Vec<String> {
        self.with(|s| s.live.keys().cloned().collect())
    }

    /// What exists for `branch_id`.
    pub fn provisioned(&self, branch_id: &str) -> Option<Provisioned> {
        self.with(|s| s.live.get(branch_id).cloned())
    }

    /// Tunnels created so far (a repeat of `ensure` creates nothing).
    pub fn created(&self) -> u32 {
        self.with(|s| s.created)
    }

    /// Tunnels deleted so far.
    pub fn deleted(&self) -> u32 {
        self.with(|s| s.deleted)
    }

    /// Credentials handed out so far.
    pub fn credentials_issued(&self) -> u32 {
        self.with(|s| s.credentials)
    }

    fn ensure(&self, branch_id: &str) -> Result<Provisioned, TunnelError> {
        self.check(FakeOp::Ensure)?;
        Ok(self.with(|s| {
            if let Some(p) = s.live.get(branch_id) {
                return p.clone();
            }
            let p = Provisioned {
                tunnel_id: format!("tunnel-{branch_id}"),
                access_app_id: format!("app-{branch_id}"),
            };
            s.live.insert(branch_id.to_owned(), p.clone());
            s.created += 1;
            p
        }))
    }

    fn credential(&self, tunnel_id: &str) -> Result<String, TunnelError> {
        self.check(FakeOp::Credential)?;
        Ok(self.with(|s| {
            s.credentials += 1;
            format!("fake-credential-{tunnel_id}-{}", s.credentials)
        }))
    }

    fn delete(&self, branch_id: &str) -> Result<(), TunnelError> {
        self.check(FakeOp::Delete)?;
        self.with(|s| {
            if s.live.remove(branch_id).is_some() {
                s.deleted += 1;
            }
        });
        Ok(())
    }
}

/// The Cloudflare API client.
pub struct CloudflareTunnels {
    client: reqwest::Client,
    base: String,
    token: String,
    account: String,
    zone: String,
    policy: Option<String>,
}

impl std::fmt::Debug for CloudflareTunnels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareTunnels")
            .field("account", &self.account)
            .field("zone", &self.zone)
            .field("token", &"[redacted]")
            .finish_non_exhaustive()
    }
}

fn valid_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn tunnel_name(branch_id: &str) -> String {
    format!("arena-{branch_id}")
}

impl CloudflareTunnels {
    pub fn new(
        token: &str,
        account: &str,
        zone: &str,
        policy: Option<&str>,
        base: Option<&str>,
    ) -> Result<Self, String> {
        if token.is_empty() || token.chars().any(char::is_whitespace) {
            return Err("the API token is empty or has whitespace".to_owned());
        }
        if !valid_id(account) {
            return Err("the account id is not an id".to_owned());
        }
        if !valid_id(zone) {
            return Err("the zone id is not an id".to_owned());
        }
        if policy.is_some_and(|p| !valid_id(p)) {
            return Err("the policy id is not an id".to_owned());
        }
        Ok(Self {
            client: crate::alert::client_builder(CALL_TIMEOUT)?
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.without_url().to_string())?,
            base: base
                .unwrap_or(DEFAULT_API_BASE)
                .trim_end_matches('/')
                .to_owned(),
            token: token.to_owned(),
            account: account.to_owned(),
            zone: zone.to_owned(),
            policy: policy.map(str::to_owned),
        })
    }

    /// One API call; the `result` of a successful answer, or `None` for a 404.
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<Option<Value>, TunnelError> {
        let whole = format!("{}{path}", self.base);
        let url = if query.is_empty() {
            reqwest::Url::parse(&whole)
        } else {
            reqwest::Url::parse_with_params(&whole, query)
        }
        .map_err(|_| TunnelError("CF_BAD_URL".to_owned()))?;
        let mut req = self.client.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            req = req
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let res = req.send().await.map_err(|e| {
            TunnelError(if e.is_timeout() {
                "CF_TIMEOUT".to_owned()
            } else {
                "CF_UNREACHABLE".to_owned()
            })
        })?;
        let status = res.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        let json: Value = match res.bytes().await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };
        if !(200..300).contains(&status) || json.get("success") == Some(&Value::Bool(false)) {
            let code = json
                .pointer("/errors/0/code")
                .and_then(Value::as_i64)
                .map_or(String::new(), |c| format!("_{c}"));
            return Err(TunnelError(format!("CF_HTTP_{status}{code}")));
        }
        Ok(Some(json.get("result").cloned().unwrap_or(Value::Null)))
    }

    async fn find_id(
        &self,
        path: &str,
        query: &[(&str, &str)],
        matches: impl Fn(&Value) -> bool,
    ) -> Result<Option<String>, TunnelError> {
        let found = self.call(reqwest::Method::GET, path, query, None).await?;
        Ok(found
            .as_ref()
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find(|v| matches(v)))
            .and_then(|v| v.get("id")?.as_str().map(str::to_owned)))
    }

    async fn ensure(&self, branch_id: &str, hostname: &str) -> Result<Provisioned, TunnelError> {
        let a = &self.account;
        let name = tunnel_name(branch_id);
        let tunnel_id = match self
            .find_id(
                &format!("/accounts/{a}/cfd_tunnel"),
                &[("name", &name), ("is_deleted", "false")],
                |v| v.get("name").and_then(Value::as_str) == Some(&name),
            )
            .await?
        {
            Some(id) => id,
            None => {
                let mut secret = [0u8; 32];
                getrandom::fill(&mut secret)
                    .map_err(|_| TunnelError("CF_RNG_FAILED".to_owned()))?;
                let made = self
                    .call(
                        reqwest::Method::POST,
                        &format!("/accounts/{a}/cfd_tunnel"),
                        &[],
                        Some(json!({
                            "name": name,
                            "tunnel_secret": STANDARD.encode(secret),
                            "config_src": "cloudflare",
                        })),
                    )
                    .await?;
                made.as_ref()
                    .and_then(|v| v.get("id")?.as_str().map(str::to_owned))
                    .ok_or_else(|| TunnelError("CF_BAD_ANSWER".to_owned()))?
            }
        };
        // The Access application comes first, so the name is never routed without its gate.
        let access_app_id = match self
            .find_id(
                &format!("/accounts/{a}/access/apps"),
                &[("domain", hostname)],
                |v| v.get("domain").and_then(Value::as_str) == Some(hostname),
            )
            .await?
        {
            Some(id) => id,
            None => {
                let mut app = json!({
                    "name": format!("Arena 360 {branch_id}"),
                    "domain": hostname,
                    "type": "self_hosted",
                    "session_duration": ACCESS_SESSION,
                });
                if let Some(policy) = &self.policy {
                    app["policies"] = json!([{"id": policy}]);
                }
                let made = self
                    .call(
                        reqwest::Method::POST,
                        &format!("/accounts/{a}/access/apps"),
                        &[],
                        Some(app),
                    )
                    .await?;
                made.as_ref()
                    .and_then(|v| v.get("id")?.as_str().map(str::to_owned))
                    .ok_or_else(|| TunnelError("CF_BAD_ANSWER".to_owned()))?
            }
        };
        // Only the Hub's tunnel listener is reachable through the tunnel; the rest is 404.
        self.call(
            reqwest::Method::PUT,
            &format!("/accounts/{a}/cfd_tunnel/{tunnel_id}/configurations"),
            &[],
            Some(json!({"config": {"ingress": [
                {"hostname": hostname, "service": TUNNEL_ORIGIN},
                {"service": "http_status:404"},
            ]}})),
        )
        .await?
        .ok_or_else(not_found)?;
        let z = &self.zone;
        let record = json!({
            "type": "CNAME",
            "name": hostname,
            "content": format!("{tunnel_id}.cfargotunnel.com"),
            "proxied": true,
        });
        match self
            .find_id(
                &format!("/zones/{z}/dns_records"),
                &[("type", "CNAME"), ("name", hostname)],
                |_| true,
            )
            .await?
        {
            Some(rid) => {
                self.call(
                    reqwest::Method::PUT,
                    &format!("/zones/{z}/dns_records/{rid}"),
                    &[],
                    Some(record),
                )
                .await?
                .ok_or_else(not_found)?;
            }
            None => {
                self.call(
                    reqwest::Method::POST,
                    &format!("/zones/{z}/dns_records"),
                    &[],
                    Some(record),
                )
                .await?
                .ok_or_else(not_found)?;
            }
        }
        Ok(Provisioned {
            tunnel_id,
            access_app_id,
        })
    }

    async fn credential(&self, tunnel_id: &str) -> Result<String, TunnelError> {
        let a = &self.account;
        let token = self
            .call(
                reqwest::Method::GET,
                &format!("/accounts/{a}/cfd_tunnel/{tunnel_id}/token"),
                &[],
                None,
            )
            .await?;
        token
            .as_ref()
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| TunnelError("CF_BAD_ANSWER".to_owned()))
    }

    async fn delete(&self, hostname: &str, p: &Provisioned) -> Result<(), TunnelError> {
        let a = &self.account;
        let z = &self.zone;
        if !valid_id(&p.access_app_id) || !valid_id(&p.tunnel_id) {
            return Err(TunnelError("CF_BAD_ID".to_owned()));
        }
        // The name stops routing first, then its gate goes, then the tunnel.
        if let Some(rid) = self
            .find_id(
                &format!("/zones/{z}/dns_records"),
                &[("type", "CNAME"), ("name", hostname)],
                |_| true,
            )
            .await?
        {
            self.call(
                reqwest::Method::DELETE,
                &format!("/zones/{z}/dns_records/{rid}"),
                &[],
                None,
            )
            .await?;
        }
        self.call(
            reqwest::Method::DELETE,
            &format!("/accounts/{a}/access/apps/{}", p.access_app_id),
            &[],
            None,
        )
        .await?;
        // A tunnel with live connections cannot be deleted: drop them first.
        self.call(
            reqwest::Method::DELETE,
            &format!("/accounts/{a}/cfd_tunnel/{}/connections", p.tunnel_id),
            &[],
            None,
        )
        .await?;
        self.call(
            reqwest::Method::DELETE,
            &format!("/accounts/{a}/cfd_tunnel/{}", p.tunnel_id),
            &[],
            None,
        )
        .await?;
        Ok(())
    }
}

fn not_found() -> TunnelError {
    TunnelError("CF_NOT_FOUND".to_owned())
}

/// The configured provider.
pub enum TunnelProvider {
    Fake(FakeTunnels),
    Cloudflare(CloudflareTunnels),
}

impl TunnelProvider {
    /// The recorded fake, when that is the provider (tests and `--dev`).
    pub fn fake(&self) -> Option<&FakeTunnels> {
        match self {
            Self::Fake(f) => Some(f),
            Self::Cloudflare(_) => None,
        }
    }

    /// Makes sure the branch's tunnel, its DNS name (ingress: only the Hub's tunnel listener)
    /// and its Access application exist; a repeat finds them and creates nothing.
    pub async fn ensure(
        &self,
        branch_id: &str,
        hostname: &str,
    ) -> Result<Provisioned, TunnelError> {
        match self {
            Self::Fake(f) => f.ensure(branch_id),
            Self::Cloudflare(c) => c.ensure(branch_id, hostname).await,
        }
    }

    /// The connector token of a tunnel. Never log it.
    pub async fn credential(&self, tunnel_id: &str) -> Result<String, TunnelError> {
        match self {
            Self::Fake(f) => f.credential(tunnel_id),
            Self::Cloudflare(c) => c.credential(tunnel_id).await,
        }
    }

    /// Deletes the Access application, the DNS name and the tunnel. What is already gone is
    /// success.
    pub async fn delete(
        &self,
        branch_id: &str,
        hostname: &str,
        what: &Provisioned,
    ) -> Result<(), TunnelError> {
        match self {
            Self::Fake(f) => f.delete(branch_id),
            Self::Cloudflare(c) => c.delete(hostname, what).await,
        }
    }

    /// The provider `ARENA_CF_*` describe; `None` without a token (no tunnels are made, and a
    /// heartbeat answers `off`). Errors name the variable, never its value.
    pub fn from_env(env: &impl Fn(&str) -> Option<OsString>) -> Result<Option<Self>, String> {
        let Some(token) = get_env(env, CF_API_TOKEN_ENV).filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        let need = |name: &str| {
            get_env(env, name)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{name} is not set ({CF_API_TOKEN_ENV} is)"))
        };
        let account = need(CF_ACCOUNT_ID_ENV)?;
        let zone = need(CF_ZONE_ID_ENV)?;
        let policy = get_env(env, CF_ACCESS_POLICY_ID_ENV).filter(|v| !v.is_empty());
        let base = get_env(env, CF_API_BASE_ENV).filter(|v| !v.is_empty());
        CloudflareTunnels::new(
            token.trim(),
            account.trim(),
            zone.trim(),
            policy.as_deref().map(str::trim),
            base.as_deref(),
        )
        .map(|c| Some(Self::Cloudflare(c)))
        .map_err(|e| format!("{CF_API_TOKEN_ENV} / {CF_ACCOUNT_ID_ENV} / {CF_ZONE_ID_ENV}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| map.get(k).map(OsString::from)
    }

    #[tokio::test]
    async fn the_fake_creates_once_hands_out_fresh_credentials_and_deletes() {
        let p = TunnelProvider::Fake(FakeTunnels::new());
        let a = p.ensure("B1", "B1.tunnel.arena360.cloud").await.unwrap();
        let b = p.ensure("B1", "B1.tunnel.arena360.cloud").await.unwrap();
        assert_eq!(a, b);
        let fake = p.fake().unwrap();
        assert_eq!(fake.created(), 1);
        let c1 = p.credential(&a.tunnel_id).await.unwrap();
        let c2 = p.credential(&a.tunnel_id).await.unwrap();
        assert_ne!(c1, c2);
        p.delete("B1", "h", &a).await.unwrap();
        p.delete("B1", "h", &a).await.unwrap();
        assert_eq!((fake.deleted(), fake.live()), (1, Vec::<String>::new()));
    }

    #[tokio::test]
    async fn the_fake_fails_an_operation_a_set_number_of_times() {
        let p = TunnelProvider::Fake(FakeTunnels::new());
        p.fake().unwrap().fail(FakeOp::Ensure, 1);
        assert!(p.ensure("B1", "h").await.is_err());
        assert!(p.ensure("B1", "h").await.is_ok());
    }

    #[test]
    fn the_provider_needs_a_token_and_names_what_is_missing() {
        assert!(TunnelProvider::from_env(&env_of(&[])).unwrap().is_none());
        let e = TunnelProvider::from_env(&env_of(&[(CF_API_TOKEN_ENV, "tok")]))
            .err()
            .unwrap();
        assert!(e.contains(CF_ACCOUNT_ID_ENV), "{e}");
        let ok = TunnelProvider::from_env(&env_of(&[
            (CF_API_TOKEN_ENV, "tok"),
            (CF_ACCOUNT_ID_ENV, "acc123"),
            (CF_ZONE_ID_ENV, "zone456"),
        ]))
        .unwrap();
        assert!(matches!(ok, Some(TunnelProvider::Cloudflare(_))));
        let bad = TunnelProvider::from_env(&env_of(&[
            (CF_API_TOKEN_ENV, "tok"),
            (CF_ACCOUNT_ID_ENV, "../x"),
            (CF_ZONE_ID_ENV, "zone456"),
        ]))
        .err()
        .unwrap();
        assert!(!bad.contains("../x"), "{bad}");
    }

    /// A mock Cloudflare API on a loopback port: records `METHOD path?query body` per call and
    /// answers the way the real API does for a branch with nothing yet (or, with `exists`, with
    /// everything in place).
    async fn mock_api(exists: bool, fail_with: Option<u16>) -> (String, Arc<Mutex<Vec<String>>>) {
        use axum::extract::Request;
        use axum::response::IntoResponse;
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = Arc::clone(&log);
        let app = axum::Router::new().fallback(move |req: Request| {
            let seen = Arc::clone(&seen);
            async move {
                let (parts, body) = req.into_parts();
                let bytes = axum::body::to_bytes(body, 1 << 16).await.unwrap();
                let line = format!(
                    "{} {} {}",
                    parts.method,
                    parts.uri,
                    String::from_utf8_lossy(&bytes)
                );
                assert_eq!(
                    parts.headers.get("authorization").and_then(|v| v.to_str().ok()),
                    Some("Bearer sekret-token")
                );
                seen.lock().unwrap().push(line);
                if let Some(code) = fail_with {
                    return (
                        axum::http::StatusCode::from_u16(code).unwrap(),
                        r#"{"success":false,"errors":[{"code":10000,"message":"sekret-token leaked"}]}"#,
                    )
                        .into_response();
                }
                let path = parts.uri.path().to_owned();
                let result = match (parts.method.as_str(), path.as_str()) {
                    ("GET", "/accounts/acc/cfd_tunnel") if exists => {
                        r#"[{"id":"t-1","name":"arena-B1"}]"#
                    }
                    ("GET", "/zones/zone/dns_records") if exists => r#"[{"id":"r-1"}]"#,
                    ("GET", "/accounts/acc/access/apps") if exists => {
                        r#"[{"id":"app-1","domain":"B1.tunnel.arena360.cloud"}]"#
                    }
                    ("GET", "/accounts/acc/cfd_tunnel/t-1/token") => r#""the-connector-token""#,
                    ("GET", _) => "[]",
                    ("POST", "/accounts/acc/cfd_tunnel") => r#"{"id":"t-1"}"#,
                    ("POST", "/accounts/acc/access/apps") => r#"{"id":"app-1"}"#,
                    _ => "{}",
                };
                (
                    axum::http::StatusCode::OK,
                    format!(r#"{{"success":true,"result":{result}}}"#),
                )
                    .into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        (base, log)
    }

    fn api(base: &str) -> TunnelProvider {
        TunnelProvider::Cloudflare(
            CloudflareTunnels::new("sekret-token", "acc", "zone", Some("pol-1"), Some(base))
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn cloudflare_creates_the_tunnel_its_ingress_dns_name_and_access_app() {
        let (base, log) = mock_api(false, None).await;
        let p = api(&base);
        let made = p.ensure("B1", "B1.tunnel.arena360.cloud").await.unwrap();
        assert_eq!(
            made,
            Provisioned {
                tunnel_id: "t-1".into(),
                access_app_id: "app-1".into()
            }
        );
        let log = log.lock().unwrap().clone();
        let starts: Vec<&str> = log
            .iter()
            .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>())
            .map(|v| if v.len() == 2 { v[0] } else { "" })
            .collect();
        assert_eq!(
            starts,
            ["GET", "POST", "GET", "POST", "PUT", "GET", "POST"],
            "{log:?}"
        );
        assert!(log[0].contains("/accounts/acc/cfd_tunnel?name=arena-B1&is_deleted=false"));
        assert!(
            log[1].contains("\"config_src\":\"cloudflare\"") && log[1].contains("tunnel_secret")
        );
        // The gate exists before the name is routed.
        assert!(log[3].contains("self_hosted") && log[3].contains("pol-1"));
        // Only the Hub's tunnel listener is forwarded to; everything else is 404.
        assert!(
            log[4].contains("/cfd_tunnel/t-1/configurations"),
            "{}",
            log[4]
        );
        assert!(log[4].contains("\"hostname\":\"B1.tunnel.arena360.cloud\""));
        assert!(log[4].contains("\"service\":\"http://127.0.0.1:7403\""));
        assert!(log[4].contains("http_status:404"));
        assert!(log[6].contains("t-1.cfargotunnel.com") && log[6].contains("CNAME"));
        assert_eq!(p.credential("t-1").await.unwrap(), "the-connector-token");
    }

    #[tokio::test]
    async fn cloudflare_finds_what_exists_and_creates_nothing_twice() {
        let (base, log) = mock_api(true, None).await;
        let made = api(&base)
            .ensure("B1", "B1.tunnel.arena360.cloud")
            .await
            .unwrap();
        assert_eq!(made.tunnel_id, "t-1");
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter()
                .all(|l| !l.starts_with("POST /accounts/acc/cfd_tunnel")
                    && !l.starts_with("POST /accounts/acc/access/apps")
                    && !l.starts_with("POST /zones")),
            "{log:?}"
        );
        assert!(
            log.iter()
                .any(|l| l.starts_with("PUT /zones/zone/dns_records/r-1"))
        );
    }

    #[tokio::test]
    async fn cloudflare_deletes_the_dns_name_the_app_the_connections_and_the_tunnel() {
        let (base, log) = mock_api(true, None).await;
        let what = Provisioned {
            tunnel_id: "t-1".into(),
            access_app_id: "app-1".into(),
        };
        api(&base)
            .delete("B1", "B1.tunnel.arena360.cloud", &what)
            .await
            .unwrap();
        let log = log.lock().unwrap().clone();
        let deletes: Vec<String> = log
            .iter()
            .filter(|l| l.starts_with("DELETE"))
            .map(|l| l.trim().to_owned())
            .collect();
        assert_eq!(
            deletes,
            [
                "DELETE /zones/zone/dns_records/r-1",
                "DELETE /accounts/acc/access/apps/app-1",
                "DELETE /accounts/acc/cfd_tunnel/t-1/connections",
                "DELETE /accounts/acc/cfd_tunnel/t-1",
            ]
        );
    }

    #[tokio::test]
    async fn a_delete_retried_after_a_partial_one_finishes_on_the_missing_objects() {
        // Everything already gone: each call answers 404, which is success for a delete, and the
        // tunnel delete is still sent.
        let (base, log) = mock_api(false, Some(404)).await;
        let what = Provisioned {
            tunnel_id: "t-1".into(),
            access_app_id: "app-1".into(),
        };
        api(&base)
            .delete("B1", "B1.tunnel.arena360.cloud", &what)
            .await
            .unwrap();
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter()
                .any(|l| l.trim() == "DELETE /accounts/acc/cfd_tunnel/t-1"),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn a_write_that_finds_its_object_missing_is_an_error_not_a_success() {
        let (base, _log) = mock_api(false, Some(404)).await;
        let e = api(&base)
            .ensure("B1", "B1.tunnel.arena360.cloud")
            .await
            .unwrap_err();
        assert!(e.0.starts_with("CF_"), "{e}");
    }

    #[tokio::test]
    async fn a_cloudflare_error_carries_a_code_and_never_the_answer_or_the_token() {
        let (base, _log) = mock_api(false, Some(403)).await;
        let e = api(&base)
            .ensure("B1", "B1.tunnel.arena360.cloud")
            .await
            .unwrap_err();
        assert_eq!(e.0, "CF_HTTP_403_10000");
        assert!(!e.to_string().contains("sekret"));
        let unreachable = api("http://127.0.0.1:9")
            .credential("t-1")
            .await
            .unwrap_err();
        assert_eq!(unreachable.0, "CF_UNREACHABLE");
    }

    #[test]
    fn the_client_never_shows_its_token() {
        let c = CloudflareTunnels::new("sekret-token", "acc", "zone", None, None).unwrap();
        assert!(!format!("{c:?}").contains("sekret-token"));
    }
}

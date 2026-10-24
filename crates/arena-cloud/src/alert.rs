//! The failure alert: a Slack-compatible incoming webhook (`{"text": …}`), which Discord also
//! accepts on its `/slack` URL suffix.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use crate::config::get_env;

/// The webhook URL (a secret: never logged).
pub const WEBHOOK_ENV: &str = "ARENA_ALERT_WEBHOOK_URL";
/// The environment name an alert carries (`staging`, `production`).
pub const ENV_NAME_ENV: &str = "ARENA_ENV";
/// Optional dead-man's-switch URL, GET after each successful backup (never logged).
pub const HEARTBEAT_ENV: &str = "ARENA_BACKUP_HEARTBEAT_URL";
/// How long one alert POST (or heartbeat GET) may take.
pub const ALERT_TIMEOUT: Duration = Duration::from_secs(10);

/// The webhook URL, when set.
pub fn webhook_from_env(env: &impl Fn(&str) -> Option<OsString>) -> Option<String> {
    get_env(env, WEBHOOK_ENV).filter(|v| !v.is_empty())
}

/// The heartbeat URL, when set.
pub fn heartbeat_from_env(env: &impl Fn(&str) -> Option<OsString>) -> Option<String> {
    get_env(env, HEARTBEAT_ENV).filter(|v| !v.is_empty())
}

/// The environment name, or `unknown`.
pub fn env_name_from_env(env: &impl Fn(&str) -> Option<OsString>) -> String {
    get_env(env, ENV_NAME_ENV)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The alert text: environment, task and error code only (never a secret or error detail).
pub fn alert_text(env_name: &str, task: &str, code: &str) -> String {
    format!("arena-cloud {env_name}: task {task} failed with {code}. See the job's logs.")
}

/// An HTTP client on the workspace rustls (ring) with the Mozilla roots.
fn client(timeout: Duration) -> Result<reqwest::Client, String> {
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
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .timeout(timeout)
        .build()
        .map_err(|e| e.without_url().to_string())
}

/// POSTs `{"text": text}` to `url` within `timeout`. A non-2xx answer is an error. The error
/// never contains the URL.
pub async fn post(url: &str, text: &str, timeout: Duration) -> Result<(), String> {
    let body = serde_json::json!({ "text": text }).to_string();
    let res = client(timeout)?
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("the webhook answered {}", res.status().as_u16()))
    }
}

/// GETs `url` within `timeout` (a heartbeat ping). A non-2xx answer is an error. The error
/// never contains the URL.
pub async fn ping(url: &str, timeout: Duration) -> Result<(), String> {
    let res = client(timeout)?
        .get(url)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("the heartbeat answered {}", res.status().as_u16()))
    }
}

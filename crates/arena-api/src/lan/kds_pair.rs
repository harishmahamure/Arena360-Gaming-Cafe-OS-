//! Kitchen display pairing and the plain-HTTP side of `/kitchen` (Story 7.6).
//!
//! * `POST /kitchen/pair {code, name?}` takes the 6-digit code a Manager issued from Settings →
//!   Kitchen, hashes a fresh token secret off the async runtime, submits `kds.pair` as the system
//!   actor and answers the `kds` token once. The code is held in memory by a second instance of
//!   [`Pairing`](crate::pairing::Pairing): 10 minutes, 5 wrong tries kill it, more than 20
//!   attempts per IP per hour are `RATE_LIMITED`. A failed pairing (a fifth display, a lapse) gives
//!   the code back; a used or expired code is not renewed.
//! * `GET /kitchen/status` says whether the branch has the kitchen flag, so the page can show
//!   "available on Basic" on a Free branch.
//! * `GET /kitchen` and `GET /kitchen/<asset>` serve the built Kitchen display app from
//!   `LanHub.kitchen_dir`, traversal-safe; without the directory a built-in page does the same
//!   pairing in plain JS.
//!
//! A code and a token never appear in a URL, a log line or an audit row.

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use arena_core::devices::{format_token, hash_token_secret, new_paired_token_secret};
use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, FLAG_KITCHEN, Intent, IntentKind, IntentPayload,
    KDS_DEFAULT_NAME, KEY_LICENCE_FLAGS, KdsPair, KdsPairRequest, KdsPairResponse, KdsStatus,
    PAIR_SYSTEM_ACTOR, has_flag, is_display_name,
};

use super::LanHub;
use super::pair::Answer;
use crate::pairing::rate_limited;

/// The largest asset the Hub serves from the Kitchen directory.
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

fn json<T: serde::Serialize>(status: &str, body: &T) -> Answer {
    Answer {
        status: status.to_owned(),
        body: serde_json::to_string(body).unwrap_or_default(),
        retry_after_s: None,
    }
}

fn failed(e: AppError) -> Answer {
    let code = crate::ui_auth::status_of(e.code);
    let status = format!(
        "{} {}",
        code.as_u16(),
        code.canonical_reason().unwrap_or("Error")
    );
    let e = if e.code == ErrorCode::Internal {
        tracing::warn!(detail = %e.message, "kitchen display pairing failed");
        AppError::new(ErrorCode::Internal, "the hub could not pair this display")
    } else {
        e
    };
    json(&status, &e)
}

/// `403 FORBIDDEN` as an `AppError` body: any plain-HTTP path the Hub does not answer.
pub(crate) fn forbidden() -> Answer {
    failed(AppError::new(
        ErrorCode::Forbidden,
        "this address is not served here",
    ))
}

fn internal(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

/// Whether the branch has the `kitchen` flag now (read from the Hub's own database).
async fn kitchen_available(hub: &LanHub) -> Result<bool, AppError> {
    let dir = Arc::clone(&hub.devices);
    let keys = vec![KEY_LICENCE_FLAGS.to_owned()];
    let read = tokio::task::spawn_blocking(move || dir.settings(&keys))
        .await
        .map_err(|e| internal(format!("flag read: {e}")))??;
    Ok(read
        .iter()
        .any(|s| s.key == KEY_LICENCE_FLAGS && has_flag(&s.value, FLAG_KITCHEN)))
}

/// `GET /kitchen/status`.
pub(crate) async fn status(hub: &Arc<LanHub>) -> Answer {
    let available = match kitchen_available(hub).await {
        Ok(a) => a,
        Err(e) => return failed(e),
    };
    let branch_name = match hub.info.as_ref() {
        Some(info) => {
            let reader = Arc::clone(&info.reader);
            match tokio::task::spawn_blocking(move || reader.branch()).await {
                Ok(Ok(Some(b))) => b.name,
                _ => String::new(),
            }
        }
        None => String::new(),
    };
    json(
        "200 OK",
        &KdsStatus {
            available,
            branch_name,
        },
    )
}

/// `POST /kitchen/pair`.
pub(crate) async fn pair(hub: &Arc<LanHub>, peer: SocketAddr, body: &[u8]) -> Answer {
    match pair_inner(hub, peer, body).await {
        Ok(ok) => json("200 OK", &ok),
        Err(Refusal::Error(e)) => failed(e),
        Err(Refusal::Limited(seconds)) => Answer {
            retry_after_s: Some(seconds),
            ..failed(rate_limited())
        },
    }
}

enum Refusal {
    Error(AppError),
    Limited(u32),
}

impl From<AppError> for Refusal {
    fn from(e: AppError) -> Self {
        Self::Error(e)
    }
}

/// The name a display takes: what the tablet sent, trimmed, or "Kitchen".
fn display_name(requested: Option<&str>) -> Result<String, AppError> {
    let name = requested
        .map(|n| n.chars().filter(|c| !c.is_control()).collect::<String>())
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| KDS_DEFAULT_NAME.to_owned());
    if is_display_name(&name) {
        Ok(name)
    } else {
        Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "name",
            "the name is 1 to 40 characters",
        ))
    }
}

async fn pair_inner(
    hub: &Arc<LanHub>,
    peer: SocketAddr,
    body: &[u8],
) -> Result<KdsPairResponse, Refusal> {
    let Some(pairing) = hub.kds_pairing.clone() else {
        return Err(AppError::new(ErrorCode::NotFound, "pairing is not available").into());
    };
    pairing.attempt(peer.ip()).map_err(Refusal::Limited)?;
    let req: KdsPairRequest = serde_json::from_slice(body).map_err(|_| {
        AppError::new(
            ErrorCode::BadFrame,
            "the body must be a kitchen pairing request",
        )
    })?;
    let name = display_name(req.name.as_deref())?;
    let writer = match hub.writer.clone() {
        Some(w) if hub.is_primary() => w,
        _ => {
            return Err(AppError::new(ErrorCode::NotPrimary, "this hub is not the primary").into());
        }
    };
    // A Free branch is refused before the code is looked at.
    if !kitchen_available(hub).await? {
        return Err(AppError::new(ErrorCode::TierLocked, "available on Basic and above").into());
    }
    // A wrong, used or expired code costs a try and no hashing.
    let taken = pairing.take(&req.code)?;
    let secret = new_paired_token_secret()?;
    let hashed = {
        let secret = secret.clone();
        tokio::task::spawn_blocking(move || hash_token_secret(&secret)).await
    };
    let hash = match hashed {
        Ok(Ok(hash)) => hash,
        Ok(Err(e)) => {
            pairing.restore(taken);
            return Err(e.into());
        }
        Err(e) => {
            pairing.restore(taken);
            return Err(internal(format!("hash task: {e}")).into());
        }
    };
    let created_by = taken.owner().to_owned();
    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KdsPair,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: hub.clock.now_ms(),
        base_rev: None,
        actor: Actor {
            kind: ActorKind::System,
            id: PAIR_SYSTEM_ACTOR.into(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KdsPair(KdsPair {
            name: name.clone(),
            token_hash: hash,
            created_by: created_by.clone(),
        }),
    };
    let applied = match writer.submit_intent(intent).await {
        Ok(applied) => applied,
        Err(e) => {
            if e.code != ErrorCode::HubSlow {
                pairing.restore(taken);
            }
            return Err(e.into());
        }
    };
    pairing.used(taken);
    let display_id = applied
        .changes
        .iter()
        .find(|c| c.tbl == "kitchen_displays")
        .map(|c| c.row_id.clone())
        .ok_or_else(|| internal("kds.pair wrote no display row"))?;
    tracing::info!(
        display_id = %display_id,
        peer = %peer.ip(),
        created_by = %created_by,
        "kitchen display paired"
    );
    Ok(KdsPairResponse {
        token: format_token(&display_id, &secret),
        display_id,
        name,
    })
}

/// The bearer token of a request head, if it carries an `Authorization: Bearer` header
/// (`Some("")` for a header that is not a usable bearer).
pub(crate) fn bearer_of(head: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(head).ok()?;
    for line in text.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("authorization") {
            let value = value.trim();
            let token = value
                .get(..7)
                .filter(|p| p.eq_ignore_ascii_case("bearer "))
                .map(|_| value[7..].trim())
                .unwrap_or("");
            return Some(token.to_owned());
        }
    }
    None
}

/// Whether a request to `/kitchen*` may go on: it carries no bearer token, or a valid `kds` one.
/// A paired-device token, any other token or a revoked display is `FORBIDDEN`.
pub(crate) async fn bearer_allowed(hub: &LanHub, peer: SocketAddr, head: &[u8]) -> bool {
    let Some(token) = bearer_of(head) else {
        return true;
    };
    let now = tokio::time::Instant::now();
    if hub.throttle.is_blocked(peer.ip(), now) {
        return false;
    }
    let Ok(_permit) = Arc::clone(&hub.verify_permits).acquire_owned().await else {
        return false;
    };
    let dir = Arc::clone(&hub.devices);
    let verified = tokio::task::spawn_blocking(move || {
        dir.verify(arena_proto::ClientClass::Kds, &token).is_ok()
    })
    .await
    .unwrap_or(false);
    if !verified {
        hub.throttle.record_failure(peer.ip(), now);
    }
    verified
}

/// A file the Kitchen directory serves.
pub(crate) struct Asset {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "webmanifest") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("wav") => "audio/wav",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The file under `root` that `rel` (the part of the path after `/kitchen/`) names, or `None`
/// when `rel` could leave `root`: only plain names, no `..`, no separators of the other kind,
/// no percent-escapes, no drive letters.
pub(crate) fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.len() > 512 {
        return None;
    }
    let mut out = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':', '%', '\0'])
            || part.chars().any(char::is_control)
        {
            return None;
        }
        // A component that is anything but a plain name (a prefix, a root) never joins.
        if !matches!(
            Path::new(part).components().next(),
            Some(Component::Normal(_))
        ) {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

async fn read_asset(path: &Path) -> Option<Asset> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    if !meta.is_file() || meta.len() > MAX_ASSET_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    Some(Asset {
        content_type: content_type(path),
        bytes,
    })
}

/// What `GET /kitchen` or `GET /kitchen/<rel>` answers with: an asset, the built-in page, or
/// nothing (404).
pub(crate) async fn serve(hub: &LanHub, rel: Option<&str>) -> Option<Asset> {
    let dir = hub.kitchen_dir.as_deref();
    match (dir, rel) {
        (Some(dir), Some(rel)) => {
            let path = safe_join(dir, rel)?;
            if let Some(asset) = read_asset(&path).await {
                return Some(asset);
            }
            // An address with no extension is a client-side route of the app: the app's page.
            if Path::new(rel).extension().is_none() {
                return read_asset(&dir.join("index.html")).await;
            }
            None
        }
        (Some(dir), None) => match read_asset(&dir.join("index.html")).await {
            Some(asset) => Some(asset),
            None => Some(builtin()),
        },
        (None, None) => Some(builtin()),
        (None, Some(_)) => None,
    }
}

fn builtin() -> Asset {
    Asset {
        content_type: "text/html; charset=utf-8",
        bytes: BUILTIN_PAGE.as_bytes().to_vec(),
    }
}

/// The page the Hub serves at `/kitchen` when no Kitchen build is installed: the same pairing
/// flow in plain JS (a 6-digit field, a name field, the Free message, the stored token, a `kds`
/// hello and the return to pairing on `TOKEN_REVOKED`). The board arrives with the built app.
const BUILTIN_PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Arena 360 Kitchen</title>
<style>
body{margin:0;font:18px system-ui,sans-serif;background:#10131a;color:#f2f4f8;display:grid;place-items:center;min-height:100vh}
main{width:min(26rem,92vw);display:grid;gap:1rem}
h1{font-size:1.5rem;margin:0}
label{display:grid;gap:.4rem}
input,button{font:inherit;min-height:44px;padding:.5rem .75rem;border-radius:.5rem;border:1px solid #4a5468;background:#1b2130;color:inherit}
button{background:#3d7bff;border-color:#3d7bff;font-weight:600}
p{margin:0}
.error{color:#ffb4a8}
[hidden]{display:none}
</style></head>
<body><main>
<h1>Kitchen display</h1>
<p id="lock" hidden>Kitchen display is available on Basic. Ask the owner to upgrade.</p>
<form id="pair" hidden>
<label>Pairing code<input id="code" inputmode="numeric" autocomplete="off" maxlength="6" pattern="[0-9]{6}" required></label>
<label>Display name<input id="name" value="Kitchen" maxlength="40"></label>
<button type="submit">Pair this display</button>
<p id="error" class="error" role="alert"></p>
</form>
<p id="state" hidden></p>
</main>
<script>
(function(){
var KEY='arena.kitchen.token',$=function(i){return document.getElementById(i)};
var WRONG="That code didn't work. Get a new one from the counter.";
var LIMIT="This branch already has 4 kitchen displays \u2014 remove one at the counter.";
function show(which){['lock','pair','state'].forEach(function(i){$(i).hidden=i!==which})}
function pairing(msg){localStorage.removeItem(KEY);show('pair');$('error').textContent=msg||''}
function connect(){
  var token=localStorage.getItem(KEY);if(!token)return;
  var ws=new WebSocket('ws://'+location.host+'/ws'),ok=false;
  ws.onopen=function(){ws.send(JSON.stringify({v:1,rid:'h1',kind:'hello',credential:token,class:'kds',versions:{app:'0.1.0'},capabilities:[]}))};
  ws.onmessage=function(e){var f=JSON.parse(e.data);
    if(f.kind==='hello.ok'){ok=true;show('state');$('state').textContent='Connected'}
    else if(f.kind==='ping'){ws.send(JSON.stringify({v:1,rid:f.rid,kind:'pong'}))}
    else if(f.kind==='error'&&f.error&&(f.error.code==='TOKEN_REVOKED'||f.error.code==='TOKEN_INVALID')){ws.onclose=null;pairing('')}};
  ws.onclose=function(){if(localStorage.getItem(KEY)){if(ok){show('state');$('state').textContent='Reconnecting\u2026'}setTimeout(connect,ok?2000:5000)}};
}
$('pair').onsubmit=function(ev){ev.preventDefault();$('error').textContent='';
  fetch('/kitchen/pair',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({code:$('code').value,name:$('name').value})})
  .then(function(r){return r.json().then(function(b){return{ok:r.ok,b:b}})})
  .then(function(r){
    if(r.ok){localStorage.setItem(KEY,r.b.token);connect();return}
    $('code').value='';
    var c=r.b&&r.b.code;
    if(c==='TIER_LOCKED'){show('lock')}
    else if(c==='DEVICE_LIMIT'){$('error').textContent=LIMIT}
    else if(c==='RATE_LIMITED'){$('error').textContent='Too many tries. Wait a while and try again.'}
    else{$('error').textContent=WRONG}})
  .catch(function(){$('error').textContent='The Hub could not be reached.'})};
fetch('/kitchen/status').then(function(r){return r.json()}).then(function(s){
  if(!s.available&&!localStorage.getItem(KEY)){show('lock');return}
  if(localStorage.getItem(KEY)){show('state');$('state').textContent='Connecting\u2026';connect()}else{show('pair')}
}).catch(function(){show('pair')});
})();
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chime_is_served_as_audio() {
        assert_eq!(
            content_type(Path::new("sounds/new-ticket.wav")),
            "audio/wav"
        );
    }

    #[test]
    fn safe_join_keeps_every_path_inside_the_root() {
        let root = Path::new("/srv/kitchen");
        assert_eq!(
            safe_join(root, "_app/immutable/a.js"),
            Some(root.join("_app").join("immutable").join("a.js"))
        );
        for bad in [
            "",
            "..",
            "../x",
            "a/../x",
            "a//b",
            "a/./b",
            "/etc/passwd",
            "a\\b",
            "C:/x",
            "a%2e%2e/b",
            "a\0b",
            "a/b/",
        ] {
            assert_eq!(safe_join(root, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_bearer_header_is_read_case_insensitively() {
        let head = b"GET /kitchen HTTP/1.1\r\nHost: h\r\nauthorization: bearer abc.def\r\n\r\n";
        assert_eq!(bearer_of(head).as_deref(), Some("abc.def"));
        assert_eq!(bearer_of(b"GET /kitchen HTTP/1.1\r\nHost: h\r\n\r\n"), None);
        let basic = b"GET /kitchen HTTP/1.1\r\nAuthorization: Basic xyz\r\n\r\n";
        assert_eq!(bearer_of(basic).as_deref(), Some(""));
    }

    #[test]
    fn a_display_name_defaults_to_kitchen_and_is_trimmed() {
        assert_eq!(display_name(None).unwrap(), "Kitchen");
        assert_eq!(display_name(Some("   ")).unwrap(), "Kitchen");
        assert_eq!(display_name(Some("  Pass \n")).unwrap(), "Pass");
        assert!(display_name(Some(&"x".repeat(41))).is_err());
    }

    #[test]
    fn the_built_in_page_carries_the_exact_messages() {
        assert!(BUILTIN_PAGE.contains(arena_proto::KDS_CODE_WRONG));
        assert!(
            BUILTIN_PAGE
                .contains("Kitchen display is available on Basic. Ask the owner to upgrade.")
        );
        assert!(BUILTIN_PAGE.contains(
            "This branch already has 4 kitchen displays \\u2014 remove one at the counter."
        ));
    }
}

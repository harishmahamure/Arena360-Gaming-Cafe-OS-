//! The Epic 1 flows, end to end against a running Hub (loopback HTTP) and a console-mode kiosk
//! service (its app channel, standing in for the app). Every flow either succeeds or returns
//! the reason it did not; the [`Ledger`] counts the intents the driver caused, so a later check
//! can prove none was lost or applied twice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arena_proto::{
    Actor, ActorKind, Intent, IntentKind, IntentPayload, KioskHubState, SettingsSet,
};
use serde_json::{Value, json};

use crate::http::Hub;
use crate::ipc::Kiosk;
use crate::table;

/// The seeded staff (`arena-hub dev seed`): login, password, PIN.
pub const MANAGER: (&str, &str, &str) = ("manager", "dev-manager-pass", "2222");
pub const COUNTER: (&str, &str, &str) = ("counter", "dev-counter-pass", "3333");
/// The setting the settings and elevation flows edit.
pub const FOOTER: &str = "receipt.footer";
/// How long the driver waits for anything asynchronous (a kiosk replay, a reconnect).
const WAIT: Duration = Duration::from_secs(30);

/// The intents the driver caused, by kind.
pub type Ledger = BTreeMap<String, u64>;

/// What a run needs.
pub struct Ctx {
    pub hub: Hub,
    pub hub_db: PathBuf,
    pub kiosk: Arc<Kiosk>,
    pub device_id: String,
    pub ledger: Ledger,
}

fn note(ledger: &mut Ledger, kind: IntentKind) {
    *ledger.entry(kind.as_str().to_owned()).or_default() += 1;
}

fn step(name: &str) {
    println!("arena-e2e: {name}");
}

/// Polls `cond` every 100 ms until it holds or `WAIT` passes.
async fn eventually<F, Fut>(what: &str, mut cond: F) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, String>>,
{
    let deadline = Instant::now() + WAIT;
    loop {
        match cond().await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) if Instant::now() >= deadline => return Err(format!("{what}: {e}")),
            Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn expect(res: &crate::http::Res, status: u16, what: &str) -> Result<(), String> {
    if res.status == status {
        Ok(())
    } else {
        Err(format!(
            "{what}: HTTP {} (wanted {status}): {}",
            res.status, res.body
        ))
    }
}

fn ulid() -> String {
    ulid::Ulid::generate().to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A `settings.set` for `/ui/intent` (the Hub replaces the actor with the session's).
fn settings_set(key: &str, value: &str, base_rev: i64, ticket: Option<&str>) -> Value {
    let intent = Intent {
        intent_id: ulid(),
        kind: IntentKind::SettingsSet,
        idempotency_key: ulid(),
        issued_at: now_ms(),
        base_rev: Some(base_rev),
        actor: Actor {
            kind: ActorKind::Staff,
            id: "e2e".into(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::SettingsSet(SettingsSet {
            key: key.into(),
            value: value.into(),
        }),
    };
    let mut body = json!({ "intent": intent });
    if let Some(t) = ticket {
        body["elevation_ticket"] = json!(t);
    }
    body
}

impl Ctx {
    /// Waits for the Hub to be primary and the kiosk connected (its app channel open).
    pub async fn connect(port: u16, hub_data: &Path, kiosk_data: &Path) -> Result<Self, String> {
        let hub = Hub { port };
        eventually("the hub as primary", || {
            let hub = hub.clone();
            async move {
                let r = hub.get("/healthz", None).await?;
                Ok(r.body["role"] == "primary")
            }
        })
        .await?;
        let kiosk = Kiosk::connect(kiosk_data, WAIT).await?;
        let device_id = kiosk_device_id(kiosk_data)?;
        Ok(Self {
            hub,
            hub_db: hub_data.join("branch.db"),
            kiosk,
            device_id,
            ledger: Ledger::new(),
        })
    }

    fn intents(&self) -> Result<BTreeMap<String, u64>, String> {
        Ok(table::intents_by_kind(&table::read(&self.hub_db)?))
    }

    async fn sign_in(&self, (login, password, _): (&str, &str, &str)) -> Result<String, String> {
        let r = self
            .hub
            .post(
                "/ui/auth/sign-in",
                None,
                &json!({"login": login, "password": password}),
            )
            .await?;
        expect(&r, 200, &format!("sign-in {login}"))?;
        r.body["token"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "sign-in: no token".into())
    }

    async fn rev(&self, token: &str, key: &str) -> Result<i64, String> {
        let r = self.hub.get("/ui/settings", Some(token)).await?;
        expect(&r, 200, "GET /ui/settings")?;
        r.body
            .as_array()
            .and_then(|a| a.iter().find(|s| s["key"] == key))
            .and_then(|s| s["rev"].as_i64())
            .ok_or_else(|| format!("no setting {key}"))
    }

    async fn set(&mut self, token: &str, key: &str, value: &str) -> Result<(), String> {
        let rev = self.rev(token, key).await?;
        let r = self
            .hub
            .post(
                "/ui/intent",
                Some(token),
                &settings_set(key, value, rev, None),
            )
            .await?;
        expect(&r, 200, &format!("settings.set {key}"))?;
        note(&mut self.ledger, IntentKind::SettingsSet);
        Ok(())
    }

    async fn dev_status(&self) -> Result<Value, String> {
        self.kiosk.request("dev.status", json!({})).await
    }

    async fn wait_kiosk(&self, state: KioskHubState, queue_empty: bool) -> Result<(), String> {
        let want = serde_json::to_value(state).unwrap_or_default();
        eventually(&format!("the kiosk link {want}"), || async {
            let s = self.dev_status().await?;
            Ok(s["hub_state"] == want && (!queue_empty || s["queued"] == 0))
        })
        .await
    }

    async fn wait_applied(&self, kind: IntentKind, count: u64) -> Result<(), String> {
        eventually(&format!("{count} {} at the hub", kind.as_str()), || async {
            Ok(self.intents()?.get(kind.as_str()).copied().unwrap_or(0) >= count)
        })
        .await
    }

    /// The `elevated_by_mode` of the newest `device.admin_exit` change.
    fn last_exit_mode(&self) -> Result<Option<String>, String> {
        Ok(table::read(&self.hub_db)?
            .into_iter()
            .rfind(|c| c.intent_kind == "device.admin_exit")
            .and_then(|c| c.elevated_by_mode))
    }

    /// One pass of every flow. `n` (from 1) makes each pass's values and paths distinct.
    pub async fn iteration(&mut self, n: u64) -> Result<(), String> {
        let start = self.intents()?;
        let count = |k: IntentKind| start.get(k.as_str()).copied().unwrap_or(0);

        step(&format!("[{n}] staff sign-in"));
        let manager = self.sign_in(MANAGER).await?;
        let counter = self.sign_in(COUNTER).await?;
        let me = self.hub.get("/ui/me", Some(&counter)).await?;
        expect(&me, 200, "GET /ui/me")?;
        if me.body["role_name"] != "Counter" {
            return Err(format!("the counter signed in as {}", me.body));
        }

        step(&format!("[{n}] branch settings save"));
        self.set(&manager, FOOTER, &format!("e2e footer {n}"))
            .await?;

        step(&format!("[{n}] PIN elevation"));
        let rev = self.rev(&counter, FOOTER).await?;
        let value = format!("e2e footer {n} (counter)");
        let r = self
            .hub
            .post(
                "/ui/intent",
                Some(&counter),
                &settings_set(FOOTER, &value, rev, None),
            )
            .await?;
        expect(&r, 403, "a counter's settings.set without a PIN")?;
        if !matches!(r.code(), Some("PIN_REQUIRED" | "PERMISSION_DENIED")) {
            return Err(format!(
                "a counter's settings.set was refused with {:?}",
                r.code()
            ));
        }
        let r = self
            .hub
            .post(
                "/ui/elevate",
                Some(&counter),
                &json!({"items": [{"intent_kind": "settings.set", "key": FOOTER}], "pin": MANAGER.2}),
            )
            .await?;
        expect(&r, 200, "POST /ui/elevate")?;
        let ticket = r.body["ticket"]
            .as_str()
            .ok_or("elevate: no ticket")?
            .to_owned();
        let r = self
            .hub
            .post(
                "/ui/intent",
                Some(&counter),
                &settings_set(FOOTER, &value, rev, Some(&ticket)),
            )
            .await?;
        expect(&r, 200, "the elevated settings.set")?;
        note(&mut self.ledger, IntentKind::SettingsSet);

        step(&format!("[{n}] idle lock and unlock"));
        let r = self
            .hub
            .post("/ui/lock", Some(&counter), &json!({}))
            .await?;
        expect(&r, 204, "POST /ui/lock")?;
        let rev = self.rev(&counter, FOOTER).await?;
        let r = self
            .hub
            .post(
                "/ui/intent",
                Some(&counter),
                &settings_set(FOOTER, "locked", rev, None),
            )
            .await?;
        expect(&r, 423, "an intent while locked")?;
        let r = self
            .hub
            .post("/ui/unlock", Some(&counter), &json!({"pin": COUNTER.2}))
            .await?;
        expect(&r, 200, "POST /ui/unlock")?;
        if r.body["switched"] != false {
            return Err(format!("unlock switched the member: {}", r.body));
        }

        step(&format!("[{n}] audit list"));
        let r = self
            .hub
            .get("/ui/audit?module=settings&limit=100", Some(&manager))
            .await?;
        expect(&r, 200, "GET /ui/audit")?;
        let items = r.body["items"].as_array().cloned().unwrap_or_default();
        let elevated = items
            .iter()
            .any(|i| i["intent_kind"] == "settings.set" && i["elevated_by"].is_object());
        if !elevated {
            return Err(format!(
                "the audit list lacks the elevated change: {}",
                r.body
            ));
        }

        step(&format!("[{n}] kiosk connection and online status"));
        self.wait_kiosk(KioskHubState::Connected, false).await?;
        let device = self.device_id.clone();
        eventually("the kiosk online on the floor", || async {
            let r = self.hub.get("/ui/snapshot", None).await?;
            Ok(r.body["devices"].as_array().is_some_and(|d| {
                d.iter()
                    .any(|d| d["device_id"] == device && d["online"] == true)
            }))
        })
        .await?;

        step(&format!("[{n}] high-contrast toggle"));
        let hc = arena_proto::hc_key(&self.device_id);
        self.set(&manager, &hc, "true").await?;
        self.kiosk.wait_theme(true, WAIT).await?;
        self.set(&manager, &hc, "false").await?;
        self.kiosk.wait_theme(false, WAIT).await?;

        step(&format!("[{n}] process block"));
        let path = format!(r"C:\Users\player\Downloads\arena-e2e-{n}.exe");
        self.kiosk
            .request("dev.spawn_process", json!({ "path": path }))
            .await?;
        note(&mut self.ledger, IntentKind::KioskProcessBlocked);
        self.wait_applied(
            IntentKind::KioskProcessBlocked,
            count(IntentKind::KioskProcessBlocked) + 1,
        )
        .await?;

        step(&format!("[{n}] admin exit through the hub"));
        eventually("the kiosk's cached verifier", || async {
            Ok(self.dev_status().await?["verifier_version"].is_i64())
        })
        .await?;
        self.kiosk.admin_exit(MANAGER.2).await?;
        self.kiosk.request("dev.relock", json!({})).await?;
        note(&mut self.ledger, IntentKind::DeviceAdminExit);
        note(&mut self.ledger, IntentKind::DeviceAdminRelock);
        self.wait_applied(
            IntentKind::DeviceAdminRelock,
            count(IntentKind::DeviceAdminRelock) + 1,
        )
        .await?;
        let mode = self.last_exit_mode()?;
        if mode.as_deref() != Some("hub") {
            return Err(format!("the online exit was recorded as {mode:?}"));
        }

        step(&format!("[{n}] admin exit with the cached verifier"));
        self.kiosk
            .request("dev.hub_link", json!({"online": false}))
            .await?;
        self.wait_kiosk(KioskHubState::Reconnecting, false).await?;
        self.kiosk.admin_exit(MANAGER.2).await?;
        self.kiosk.request("dev.relock", json!({})).await?;
        note(&mut self.ledger, IntentKind::DeviceAdminExit);
        note(&mut self.ledger, IntentKind::DeviceAdminRelock);
        self.kiosk
            .request("dev.hub_link", json!({"online": true}))
            .await?;
        self.wait_kiosk(KioskHubState::Connected, true).await?;
        self.wait_applied(
            IntentKind::DeviceAdminRelock,
            count(IntentKind::DeviceAdminRelock) + 2,
        )
        .await?;
        let mode = self.last_exit_mode()?;
        if mode.as_deref() != Some("cached_verifier") {
            return Err(format!("the offline exit was recorded as {mode:?}"));
        }

        step(&format!("[{n}] sign-out"));
        for token in [&manager, &counter] {
            let r = self
                .hub
                .post("/ui/auth/sign-out", Some(token), &json!({}))
                .await?;
            expect(&r, 204, "sign-out")?;
        }
        Ok(())
    }
}

/// The provisioned kiosk's device id (`<kiosk data>/config.json`).
fn kiosk_device_id(kiosk_data: &Path) -> Result<String, String> {
    let path = kiosk_data.join("config.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    v["device_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no device_id", path.display()))
}

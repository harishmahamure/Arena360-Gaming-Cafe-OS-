//! Story 4.17: a fleet of 20 fake kiosks and a Hub UI drive a real `arena-hub` with the mixed
//! start / pause / resume / extend / transfer / end / settle load at 5 intents/s, through public
//! APIs only (loopback `/ui/*`, the pinned-TLS LAN protocol), and write one JSON report that
//! `scripts/android-farm.mjs verify` checks against `deploy/android-farm/farm.json`.
//!
//! Two phases, so the farm job can kill the Hub between them:
//! - `load`: seed a floor, connect the kiosks, run the load (and a timer probe every
//!   `FLEET_TIMER_EVERY_S`), watch every committed intent arrive once on the Hub UI stream and
//!   once on every kiosk, then write `report.json` and `acks.json` into `FLEET_OUT`.
//! - `verify`: after the Hub was killed and restarted, resend every acknowledged intent with its
//!   idempotency key (a lost commit would write new rows), compare the live sessions, reconnect
//!   every kiosk and wait for the probe session in flight at the kill to time out.
//!
//! Without `ARENA_FLEET_UI` the test starts a local Hub, runs `load`, kills the Hub, restarts it
//! and runs `verify` (a short run on every CI push). With it, the Hub is on the device: the farm
//! forwards the Hub's health port to `ARENA_FLEET_UI` (the `/ui` guard needs a loopback Host),
//! points `ARENA_FLEET_LAN` at the device's Wi-Fi address and sets `FLEET_PHASE`.
//!
//! Knobs: `FLEET_SECONDS` (6), `FLEET_KIOSKS` (20), `FLEET_DEVICES` (20), `FLEET_RATE` (5),
//! `FLEET_TIMER_EVERY_S` (0 = no probe), `FLEET_TIMER_MINUTES` (2), `FLEET_OUT`.
#![cfg(feature = "dev-fixtures")]

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arena_core::devices::{format_token, hash_token_secret, new_token_secret};
use arena_kiosk_core::HubClient;
use arena_proto::{
    Actor, ActorKind, ClientClass, DeviceKind, DeviceRegister, ExtendSource, GroupUpsert, Hello,
    HubEvent, Intent, IntentPayload, PauseReason, PaymentMethod, PaymentRow, RateUpsert,
    SessionEnd, SessionExtend, SessionMode, SessionPause, SessionResume, SessionSettle,
    SessionStart, SessionTransfer, Versions, WalletTopup,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const REFUSALS: [&str; 7] = [
    "STALE",
    "PC_BUSY",
    "VALIDATION_FAILED",
    "NOT_FOUND",
    "INSUFFICIENT_BALANCE",
    "PAYMENT_SHORT",
    "PAST_CLOSING",
];

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

fn p95(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples
        .get((samples.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .unwrap_or_default()
}

// ---------------------------------------------------------------- loopback HTTP

fn http(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> Option<(u16, Value)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let (head, payload) = out.split_once("\r\n\r\n")?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, serde_json::from_str(payload).unwrap_or(Value::Null)))
}

#[derive(Clone)]
struct Ui {
    port: u16,
    token: String,
}

impl Ui {
    async fn call(&self, method: &'static str, path: &str, body: Option<Value>) -> (u16, Value) {
        let (port, token, path) = (self.port, self.token.clone(), path.to_owned());
        tokio::task::spawn_blocking(move || {
            http(port, method, &path, Some(&token), body.as_ref())
                .unwrap_or_else(|| panic!("{method} {path} got no answer"))
        })
        .await
        .unwrap()
    }

    /// Submits `intent`; `Ok` is the `applied` summary, `Err` the error code.
    async fn submit(&self, intent: &Intent) -> Result<Value, String> {
        let (status, body) = self
            .call("POST", "/ui/intent", Some(json!({ "intent": intent })))
            .await;
        if status == 200 {
            Ok(body.get("applied").cloned().unwrap_or(body))
        } else {
            let code = body["code"].as_str().unwrap_or("?");
            Err(format!("{code} {}", body["message"].as_str().unwrap_or(""))
                .trim()
                .to_owned())
        }
    }

    async fn must(&self, payload: IntentPayload) -> Value {
        match self.submit(&intent(payload, None)).await {
            Ok(applied) => applied,
            Err(code) => panic!("setup intent refused: {code}"),
        }
    }
}

fn intent(payload: IntentPayload, base_rev: Option<i64>) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: payload.kind(),
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms(),
        base_rev,
        actor: Actor {
            kind: ActorKind::Staff,
            id: "fleet".into(),
        },
        elevated_by: None,
        reason: None,
        payload,
    }
}

fn first_row(applied: &Value) -> String {
    applied["changes"][0]["row_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no change row in {applied}"))
        .to_owned()
}

fn seqs(applied: &Value) -> Vec<i64> {
    applied["changes"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["seq"].as_i64()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- the Hub under test

struct Running(Option<Child>);

impl Running {
    fn kill(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.kill();
    }
}

#[derive(Clone)]
struct Target {
    ui_port: u16,
    lan: String,
}

fn start_local(dir: &Path, ports: (u16, u16, u16)) -> Running {
    let hub = Command::new(env!("CARGO_BIN_EXE_arena-hub"))
        .arg("--data-dir")
        .arg(dir)
        .args(["--platform", "android"])
        .args(["--health-port", &ports.0.to_string()])
        .args(["--lan-port", &ports.1.to_string()])
        .args(["--web-port", &ports.2.to_string()])
        .spawn()
        .unwrap();
    Running(Some(hub))
}

async fn wait_primary(port: u16) -> String {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let p = port;
        let body = tokio::task::spawn_blocking(move || http(p, "GET", "/healthz", None, None))
            .await
            .unwrap()
            .map(|(_, b)| b)
            .filter(|b| b["role"] == "primary");
        if let Some(b) = body {
            return b["tls_fingerprint"].as_str().unwrap().to_owned();
        }
        assert!(Instant::now() < deadline, "the hub never became primary");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn sign_in(port: u16) -> Ui {
    let (status, body) = tokio::task::spawn_blocking(move || {
        http(
            port,
            "POST",
            "/ui/auth/sign-in",
            None,
            Some(&json!({"login": "owner", "password": "dev-owner-pass"})),
        )
        .expect("sign-in got no answer")
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    Ui {
        port,
        token: body["token"].as_str().unwrap().to_owned(),
    }
}

// ---------------------------------------------------------------- the floor and the kiosks

#[derive(Serialize, Deserialize, Clone)]
struct Floor {
    group_id: String,
    devices: Vec<String>,
    /// `(device id, token)` of each fake kiosk.
    kiosks: Vec<(String, String)>,
    player_id: String,
    probe_player_id: String,
    probe_device: String,
}

async fn seed_floor(ui: &Ui, devices: usize, kiosks: usize) -> Floor {
    // The seed's own "Dev Kiosk" counts against the Free device cap (20); the fleet needs all 20.
    let snap = snapshot(ui).await;
    for d in snap["devices"].as_array().into_iter().flatten() {
        if d["label"] == "Dev Kiosk" {
            let id = d["device_id"].as_str().unwrap().to_owned();
            let mut revoke = intent(
                IntentPayload::DeviceRevoke(arena_proto::DeviceRevoke { device_id: id }),
                None,
            );
            revoke.reason = Some(arena_proto::ReasonCode::Replaced);
            ui.submit(&revoke).await.expect("revoke the seed's kiosk");
        }
    }
    let group_id = first_row(
        &ui.must(IntentPayload::GroupUpsert(GroupUpsert {
            id: None,
            name: "PCs".into(),
            group_kind: "pc".into(),
            colour: "teal".into(),
            sort_order: 1,
        }))
        .await,
    );
    ui.must(IntentPayload::RateUpsert(RateUpsert {
        group_id: group_id.clone(),
        price_per_hour_minor: 12_000,
    }))
    .await;
    let mut ids = Vec::new();
    let mut tokens = Vec::new();
    for n in 0..devices {
        let secret = new_token_secret().unwrap();
        let mut reg = DeviceRegister::new(
            ClientClass::Paired,
            format!("PC {n}"),
            hash_token_secret(&secret).unwrap(),
        );
        reg.device_kind = DeviceKind::Pc;
        reg.group_id = Some(group_id.clone());
        let id = first_row(&ui.must(IntentPayload::DeviceRegister(reg)).await);
        if n < kiosks {
            tokens.push((id.clone(), format_token(&id, &secret)));
        }
        ids.push(id);
    }
    // One device is kept out of the random mix for the timer probes.
    let probe_device = ids.pop().expect("at least one device");

    if env_u64("FLEET_TIMER_EVERY_S", 0) > 0 {
        // A probe session must be able to time out in minutes, not after the 30-minute minimum.
        set_setting(ui, arena_proto::KEY_BILLING_MINIMUM_MIN, "1").await;
        set_setting(ui, arena_proto::KEY_BILLING_ROUNDING_MIN, "1").await;
    }
    let player_id = create_player(ui, "loader", "+919000000001").await;
    topup(ui, &player_id, 5_000_000).await;
    let probe_player_id = create_player(ui, "prober", "+919000000002").await;
    Floor {
        group_id,
        devices: ids,
        kiosks: tokens,
        player_id,
        probe_player_id,
        probe_device,
    }
}

/// `settings.set` of one key with the revision the Hub holds now.
async fn set_setting(ui: &Ui, key: &str, value: &str) {
    let (status, body) = ui.call("GET", "/ui/settings", None).await;
    assert_eq!(status, 200, "{body}");
    let rev = body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no setting {key}"))["rev"]
        .as_i64();
    let set = intent(
        IntentPayload::SettingsSet(arena_proto::SettingsSet {
            key: key.into(),
            value: value.into(),
        }),
        rev,
    );
    ui.submit(&set).await.expect("settings.set");
}

async fn create_player(ui: &Ui, username: &str, phone: &str) -> String {
    let (status, body) = ui
        .call(
            "POST",
            "/ui/players",
            Some(json!({
                "form": {
                    "username": username, "password": "secret1", "phone": phone,
                    "name": "Asha", "email": "", "dob": "", "notes": "", "tags": [],
                    "consent": true, "policy_version": "1"
                },
                "idempotency_key": ulid::Ulid::generate().to_string(),
            })),
        )
        .await;
    assert_eq!(status, 200, "create the {username} profile: {body}");
    body["player_id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| first_row(body.get("applied").unwrap_or(&body)))
}

async fn topup(ui: &Ui, player_id: &str, amount_minor: i64) {
    ui.must(IntentPayload::WalletTopup(WalletTopup {
        player_id: player_id.to_owned(),
        amount_minor,
        payments: vec![PaymentRow {
            method: PaymentMethod::Cash,
            amount_minor,
            reference: None,
        }],
    }))
    .await;
}

type Seen = Arc<Mutex<HashMap<String, Vec<Instant>>>>;

fn record(seen: &Seen, id: String) {
    seen.lock()
        .unwrap()
        .entry(id)
        .or_default()
        .push(Instant::now());
}

struct Fleet {
    seen: Vec<Seen>,
    clients: Vec<HubClient>,
    drops: Arc<Mutex<usize>>,
}

async fn connect_kiosks(target: &Target, fingerprint: &str, floor: &Floor) -> Fleet {
    let url = format!("wss://{}/", target.lan);
    let mut fleet = Fleet {
        seen: Vec::new(),
        clients: Vec::new(),
        drops: Arc::default(),
    };
    for (_, token) in &floor.kiosks {
        let hello = Hello {
            credential: token.clone(),
            class: ClientClass::Paired,
            versions: Versions {
                app: env!("CARGO_PKG_VERSION").to_owned(),
            },
            capabilities: vec![],
        };
        let client = HubClient::connect(&url, fingerprint, hello)
            .await
            .expect("a fake kiosk pairs with its token");
        assert!(client.hello_ok().primary);
        let seen: Seen = Arc::default();
        let (sink, drops) = (Arc::clone(&seen), Arc::clone(&fleet.drops));
        let mut events = client.events().expect("the kiosk's event stream");
        tokio::spawn(async move {
            while let Some(event) = events.next_event().await {
                if let HubEvent::Applied(a) = event {
                    record(&sink, a.intent_id);
                }
            }
            *drops.lock().unwrap() += 1;
        });
        fleet.seen.push(seen);
        fleet.clients.push(client);
    }
    fleet
}

/// The Hub UI's `/ui/events` stream: every `applied` event's intent id.
fn watch_ui_events(port: u16, token: String, seen: Seen) {
    std::thread::spawn(move || {
        let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
            return;
        };
        let req = format!("GET /ui/events?token={token} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        if s.write_all(req.as_bytes()).is_err() {
            return;
        }
        let mut reader = BufReader::new(s);
        let mut line = String::new();
        let mut event = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let l = line.trim_end();
            if let Some(name) = l.strip_prefix("event:") {
                event = name.trim().to_owned();
            } else if let Some(data) = l.strip_prefix("data:")
                && event == "applied"
                && let Some(id) = serde_json::from_str::<Value>(data.trim())
                    .ok()
                    .and_then(|v| v["intent_id"].as_str().map(str::to_owned))
            {
                record(&seen, id);
            }
            line.clear();
        }
    });
}

// ---------------------------------------------------------------- the load

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn live(state: &str) -> bool {
    matches!(state, "active" | "paused" | "grace")
}

/// Picks the next realistic intent from the Hub's snapshot.
fn next_intent(snapshot: &Value, floor: &Floor, rng: &mut Rng) -> Option<Intent> {
    let sessions: Vec<&Value> = snapshot["sessions"]
        .as_array()?
        .iter()
        .filter(|s| s["device_id"] != floor.probe_device.as_str())
        .collect();
    let running: Vec<&Value> = sessions
        .iter()
        .copied()
        .filter(|s| live(s["state"].as_str().unwrap_or("")))
        .collect();
    let busy: HashSet<&str> = running
        .iter()
        .filter_map(|s| s["device_id"].as_str())
        .collect();
    let free: Vec<&String> = floor
        .devices
        .iter()
        .filter(|d| !busy.contains(d.as_str()))
        .collect();
    let roll = rng.below(100);
    if roll < 30 || running.is_empty() {
        let device = (*free.get(rng.below(free.len()))?).clone();
        let wallet = rng.below(4) == 0;
        return Some(intent(
            IntentPayload::SessionStart(SessionStart {
                group_with: None,
                device_id: device,
                mode: if wallet {
                    SessionMode::Wallet
                } else {
                    SessionMode::Postpaid
                },
                player_id: wallet.then(|| floor.player_id.clone()),
                pack_id: None,
                plan_id: None,
            }),
            None,
        ));
    }
    let s = running[rng.below(running.len())];
    let id = s["id"].as_str()?.to_owned();
    let rev = s["rev"].as_i64();
    let paused = s["state"] == "paused";
    let wallet = s["mode"] == "wallet";
    Some(match roll {
        30..=44 if paused => intent(
            IntentPayload::SessionResume(SessionResume { session_id: id }),
            rev,
        ),
        30..=44 => intent(
            IntentPayload::SessionPause(SessionPause {
                session_id: id,
                reason: PauseReason::ALL
                    .iter()
                    .copied()
                    .find(|r| *r != PauseReason::Other)
                    .unwrap(),
                note: None,
            }),
            rev,
        ),
        45..=54 if wallet => intent(
            IntentPayload::SessionExtend(SessionExtend {
                session_id: id,
                minutes: arena_proto::EXTEND_OPTIONS_MIN[0],
                source: ExtendSource::Wallet,
                pack_id: None,
                credit_override: None,
            }),
            None,
        ),
        55..=64 if !free.is_empty() => intent(
            IntentPayload::SessionTransfer(SessionTransfer {
                session_id: id,
                device_id: free[rng.below(free.len())].clone(),
            }),
            rev,
        ),
        70..=89 => {
            let bills = snapshot["bills"].as_array()?;
            let bill = bills.iter().find(|b| {
                b["order_id"].is_string()
                    && b["remaining_minor"].as_i64().unwrap_or(0) > 0
                    && b["device_id"] != floor.probe_device.as_str()
            });
            match bill {
                Some(b) => intent(
                    IntentPayload::SessionSettle(SessionSettle {
                        order_id: b["order_id"].as_str()?.to_owned(),
                        credit_override: None,
                        payments: vec![PaymentRow {
                            method: PaymentMethod::Cash,
                            amount_minor: b["remaining_minor"].as_i64()?,
                            reference: None,
                        }],
                    }),
                    b["order_rev"].as_i64(),
                ),
                None => intent(
                    IntentPayload::SessionEnd(SessionEnd { session_id: id }),
                    rev,
                ),
            }
        }
        _ => intent(
            IntentPayload::SessionEnd(SessionEnd { session_id: id }),
            rev,
        ),
    })
}

#[derive(Serialize, Deserialize)]
struct Ack {
    intent: Intent,
    changes: Value,
}

#[derive(Serialize, Deserialize, Clone)]
struct Probe {
    session_id: String,
    started_s: f64,
    /// Seconds from the start of the load to the time-up.
    due_s: f64,
    ends_at: i64,
    #[serde(default)]
    late_ms: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct Acks {
    floor: Floor,
    acks: Vec<Ack>,
    live_sessions: Vec<String>,
    probes: Vec<Probe>,
}

async fn snapshot(ui: &Ui) -> Value {
    let (status, body) = ui.call("GET", "/ui/snapshot", None).await;
    assert_eq!(status, 200, "{body}");
    body
}

/// Starts a 2-minute wallet session on the probe device and returns when it should time out.
async fn start_probe(ui: &Ui, floor: &Floor, minutes: u64, started_s: f64) -> Option<Probe> {
    // The probe has its own player, topped up for exactly `minutes` at the fixture rate, so the
    // wallet session times out on its own. A probe that cannot start (the device is still busy)
    // is skipped, never an error.
    topup(ui, &floor.probe_player_id, (minutes * 200) as i64).await;
    let started = ui
        .submit(&intent(
            IntentPayload::SessionStart(SessionStart {
                group_with: None,
                device_id: floor.probe_device.clone(),
                mode: SessionMode::Wallet,
                player_id: Some(floor.probe_player_id.clone()),
                pack_id: None,
                plan_id: None,
            }),
            None,
        ))
        .await
        .map_err(|e| println!("probe start refused: {e}"))
        .ok()?;
    let session_id = started["changes"]
        .as_array()?
        .iter()
        .find(|c| c["tbl"] == "sessions")?["row_id"]
        .as_str()?
        .to_owned();
    let snap = snapshot(ui).await;
    let s = snap["sessions"]
        .as_array()?
        .iter()
        .find(|s| s["id"] == session_id.as_str())?;
    let ends_at = s["ends_at"]
        .as_i64()
        .unwrap_or(now_ms() + (minutes * 60_000) as i64);
    Some(Probe {
        session_id,
        started_s,
        due_s: started_s + (ends_at - now_ms()) as f64 / 1000.0,
        ends_at,
        late_ms: None,
    })
}

/// Waits until the probe session has left `active` and returns how late that was.
async fn probe_lateness(ui: &Ui, probe: &Probe, patience: Duration) -> Option<i64> {
    let deadline = Instant::now() + patience;
    loop {
        let snap = snapshot(ui).await;
        let state = snap["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["id"] == probe.session_id.as_str())
            .and_then(|s| s["state"].as_str().map(str::to_owned));
        if state.as_deref().is_some_and(|s| s != "active") {
            return Some((now_ms() - probe.ends_at).max(0));
        }
        if Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("FLEET_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("arena-fleet-{}", std::process::id()))
        });
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn load(target: &Target, ui: &Ui, out: &Path) {
    let seconds = env_u64("FLEET_SECONDS", 6);
    let kiosks = env_u64("FLEET_KIOSKS", 20) as usize;
    let devices = env_u64("FLEET_DEVICES", 20) as usize;
    let rate = env_u64("FLEET_RATE", 5).max(1);
    let timer_every = env_u64("FLEET_TIMER_EVERY_S", 0);
    let timer_minutes = env_u64("FLEET_TIMER_MINUTES", 2);

    let fingerprint = {
        let p = target.ui_port;
        tokio::task::spawn_blocking(move || http(p, "GET", "/healthz", None, None))
            .await
            .unwrap()
            .expect("healthz")
            .1["tls_fingerprint"]
            .as_str()
            .expect("tls_fingerprint")
            .to_owned()
    };
    let floor = seed_floor(ui, devices, kiosks).await;
    let fleet = connect_kiosks(target, &fingerprint, &floor).await;
    let ui_seen: Seen = Arc::default();
    watch_ui_events(ui.port, ui.token.clone(), Arc::clone(&ui_seen));
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut apply = Vec::new();
    let mut acks: Vec<Ack> = Vec::new();
    let mut sent_at: HashMap<String, Instant> = HashMap::new();
    let (mut refused, mut resent, mut sent_count) = (0usize, 0usize, 0u64);
    let mut probes: Vec<(Probe, Arc<Mutex<Option<i64>>>)> = Vec::new();
    let mut next_probe = if timer_every > 0 { 0 } else { u64::MAX };
    let mut snap = snapshot(ui).await;
    let mut snap_at = Instant::now();
    let begin = Instant::now();
    let mut due = begin;
    while begin.elapsed() < Duration::from_secs(seconds) {
        if begin.elapsed().as_secs() >= next_probe {
            next_probe = begin.elapsed().as_secs() + timer_every;
            if let Some(p) =
                start_probe(ui, &floor, timer_minutes, begin.elapsed().as_secs_f64()).await
            {
                // Lateness is read the moment the session leaves `active`, not at the end.
                let slot: Arc<Mutex<Option<i64>>> = Arc::default();
                let (watcher, watched, out) = (ui.clone(), p.clone(), Arc::clone(&slot));
                tokio::spawn(async move {
                    let patience = Duration::from_secs(seconds + 3_600);
                    *out.lock().unwrap() = probe_lateness(&watcher, &watched, patience).await;
                });
                probes.push((p, slot));
            }
        }
        if snap_at.elapsed() > Duration::from_millis(250) {
            snap = snapshot(ui).await;
            snap_at = Instant::now();
        }
        let Some(next) = next_intent(&snap, &floor, &mut rng) else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        let t = Instant::now();
        let out = ui.submit(&next).await;
        apply.push(t.elapsed());
        sent_count += 1;
        match out {
            Ok(applied) => {
                sent_at.insert(next.intent_id.clone(), t);
                if sent_count.is_multiple_of(7) {
                    let again = ui
                        .submit(&next)
                        .await
                        .expect("a resent key returns its first outcome");
                    assert_eq!(again, applied, "resent key rewrote");
                    resent += 1;
                }
                acks.push(Ack {
                    intent: next,
                    changes: applied,
                });
                snap_at = Instant::now() - Duration::from_secs(1);
            }
            Err(code) => {
                assert!(
                    REFUSALS.iter().any(|r| code.starts_with(r)),
                    "unexpected refusal {code}"
                );
                refused += 1;
            }
        }
        due += Duration::from_micros(1_000_000 / rate);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            tokio::time::sleep(wait).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(1_500)).await;

    // Nothing lost, nothing twice, on the Hub UI and on every kiosk.
    let (mut lost, mut duplicated) = (0usize, 0usize);
    let mut floor_update = Vec::new();
    let mut kiosk_update = Vec::new();
    let mut all_seqs: Vec<i64> = Vec::new();
    for a in &acks {
        let at = sent_at[&a.intent.intent_id];
        all_seqs.extend(seqs(&a.changes));
        let ui_hits = ui_seen.lock().unwrap().get(&a.intent.intent_id).cloned();
        match ui_hits.as_deref() {
            None | Some([]) => lost += 1,
            Some([one]) => floor_update.push(one.saturating_duration_since(at)),
            Some(_) => duplicated += 1,
        }
        for seen in &fleet.seen {
            match seen
                .lock()
                .unwrap()
                .get(&a.intent.intent_id)
                .map(Vec::as_slice)
            {
                None | Some([]) => lost += 1,
                Some([one]) => kiosk_update.push(one.saturating_duration_since(at)),
                Some(_) => duplicated += 1,
            }
        }
    }
    let unique: HashSet<i64> = all_seqs.iter().copied().collect();
    assert_eq!(
        unique.len(),
        all_seqs.len(),
        "two intents share a change row"
    );
    let kiosk_drops =
        *fleet.drops.lock().unwrap() + fleet.clients.iter().filter(|c| c.is_closed()).count();

    // A probe due within the next half minute is awaited; the rest are checked after the restart.
    let probes: Vec<Probe> = {
        let mut done = Vec::new();
        for (mut p, slot) in probes {
            let deadline = Instant::now()
                + Duration::from_millis((p.ends_at - now_ms()).clamp(0, 30_000) as u64 + 15_000);
            while slot.lock().unwrap().is_none() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            p.late_ms = *slot.lock().unwrap();
            done.push(p);
        }
        done
    };

    let live_sessions: Vec<String> = snapshot(ui).await["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| live(s["state"].as_str().unwrap_or("")))
        .filter_map(|s| s["id"].as_str().map(str::to_owned))
        .collect();

    let op = |samples: &mut Vec<Duration>| {
        json!({
            "p95_ms": ms(p95(samples)),
            "max_ms": ms(samples.iter().copied().max().unwrap_or_default()),
            "count": samples.len(),
        })
    };
    let report = json!({
        "schema": 1,
        "run": { "seconds": seconds, "kiosks": kiosks, "devices": devices, "rate": rate,
                 "intents": apply.len(), "refused": refused, "resent": resent },
        "ops": {
            "apply": op(&mut apply),
            "floor_update": op(&mut floor_update),
            "kiosk_update": op(&mut kiosk_update),
        },
        "lost": lost,
        "duplicated": duplicated,
        "kiosk_drops": kiosk_drops,
        "timer_probes": probes.iter().map(|p| json!({
            "at_s": p.started_s, "due_s": p.due_s, "ends_at": p.ends_at, "late_ms": p.late_ms,
        })).collect::<Vec<_>>(),
    });
    println!("fleet load: {report}");
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("acks.json"),
        serde_json::to_string(&Acks {
            floor,
            acks,
            live_sessions,
            probes,
        })
        .unwrap(),
    )
    .unwrap();
}

/// After a kill and restart: every committed change is present, the live sessions are the same,
/// the kiosks sync again and a session that was timing out at the kill still times out.
async fn verify(target: &Target, ui: &Ui, out: &Path) {
    let saved: Acks =
        serde_json::from_slice(&std::fs::read(out.join("acks.json")).expect("acks.json")).unwrap();
    let fingerprint = {
        let p = target.ui_port;
        tokio::task::spawn_blocking(move || http(p, "GET", "/healthz", None, None))
            .await
            .unwrap()
            .expect("healthz")
            .1["tls_fingerprint"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let fleet = connect_kiosks(target, &fingerprint, &saved.floor).await;
    // The kiosks sync again first: a start on an offline PC is refused before its key is looked up.
    let mut missing = 0usize;
    for a in &saved.acks {
        let again = ui.submit(&a.intent).await;
        if again.as_ref().ok() != Some(&a.changes) {
            if missing < 3 {
                println!(
                    "restart mismatch {}: was {} now {again:?}",
                    a.intent.kind.as_str(),
                    a.changes
                );
            }
            missing += 1;
        }
    }
    let live_after: HashSet<String> = snapshot(ui).await["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| live(s["state"].as_str().unwrap_or("")))
        .filter_map(|s| s["id"].as_str().map(str::to_owned))
        .collect();
    let live_before: HashSet<String> = saved.live_sessions.iter().cloned().collect();
    let sessions_changed = live_before.symmetric_difference(&live_after).count();

    let mut timers_late = 0usize;
    for p in saved.probes.iter().filter(|p| p.late_ms.is_none()) {
        let wait_ms = (p.ends_at - now_ms()).max(0) as u64 + 15_000;
        if probe_lateness(ui, p, Duration::from_millis(wait_ms))
            .await
            .is_none()
        {
            timers_late += 1;
        }
    }
    let report = json!({
        "schema": 1,
        "restart": {
            "acknowledged": saved.acks.len(),
            "missing": missing,
            "sessions_changed": sessions_changed,
            "kiosks_resynced": fleet.clients.len(),
            "kiosks_expected": saved.floor.kiosks.len(),
            "timers_not_rebuilt": timers_late,
        },
    });
    println!("fleet restart: {report}");
    std::fs::write(
        out.join("restart.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fleet_of_kiosks_holds_the_budgets_and_survives_a_kill() {
    let out = out_dir();
    if let Ok(port) = std::env::var("ARENA_FLEET_UI") {
        let target = Target {
            ui_port: port.parse().expect("ARENA_FLEET_UI is a port"),
            lan: std::env::var("ARENA_FLEET_LAN").expect("ARENA_FLEET_LAN is host:port"),
        };
        wait_primary(target.ui_port).await;
        let ui = sign_in(target.ui_port).await;
        match std::env::var("FLEET_PHASE").as_deref() {
            Ok("verify") => verify(&target, &ui, &out).await,
            _ => load(&target, &ui, &out).await,
        }
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let seeded = Command::new(env!("CARGO_BIN_EXE_arena-hub"))
        .args(["dev", "seed", "--data-dir"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(seeded.status.code(), Some(0), "{seeded:?}");
    let ports = (free_port(), free_port(), free_port());
    let target = Target {
        ui_port: ports.0,
        lan: format!("127.0.0.1:{}", ports.1),
    };
    let mut hub = start_local(dir.path(), ports);
    wait_primary(target.ui_port).await;
    let ui = sign_in(target.ui_port).await;
    load(&target, &ui, &out).await;

    hub.kill();
    let _hub = start_local(dir.path(), ports);
    wait_primary(target.ui_port).await;
    let ui = sign_in(target.ui_port).await;
    verify(&target, &ui, &out).await;
    let restart: Value =
        serde_json::from_slice(&std::fs::read(out.join("restart.json")).unwrap()).unwrap();
    assert_eq!(restart["restart"]["missing"], 0, "{restart}");
    assert_eq!(restart["restart"]["sessions_changed"], 0, "{restart}");
    let report: Value =
        serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["lost"], 0, "{report}");
    assert_eq!(report["duplicated"], 0, "{report}");
    assert_eq!(report["kiosk_drops"], 0, "{report}");
}

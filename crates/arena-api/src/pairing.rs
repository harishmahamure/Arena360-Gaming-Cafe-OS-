use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use arena_core::Clock;
use arena_proto::{
    AppError, ErrorCode, KioskPairingCode, PAIR_CODE_DIGITS, PAIR_CODE_TTL_MS,
    PAIR_PIN_MAX_FAILURES, PAIR_PIN_PAUSE_S, PAIR_PIN_WINDOW_MS,
};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

pub const RENEW_TICK: Duration = Duration::from_millis(500);
pub const MAX_WRONG_CODES: u32 = 10;
pub const MAX_PIN_PAUSE_S: u32 = 15 * 60;
pub const PIN_STRIKES_RESET_MS: i64 = 60 * 60 * 1000;
pub const GLOBAL_PIN_FAILURES: usize = 20;
pub const GLOBAL_PIN_PAUSE_S: u32 = 5 * 60;
pub const MAX_PEERS: usize = 4096;

#[derive(Clone)]
pub struct CodeChanged {
    pub owner: Arc<str>,
    pub code: Option<KioskPairingCode>,
}

impl CodeChanged {
    pub fn replaced(&self) -> bool {
        self.code.is_none()
    }

    pub fn to_json(&self) -> String {
        match &self.code {
            Some(code) => serde_json::to_string(code).unwrap_or_default(),
            None => serde_json::json!({ "replaced": true }).to_string(),
        }
    }
}

impl std::fmt::Debug for CodeChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeChanged")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

struct Live {
    hash: [u8; 32],
    expires_at: i64,
    owner: Arc<str>,
    wrong: u32,
}

pub struct Taken {
    live: Live,
}

#[derive(Default)]
struct PinPeer {
    failures: Vec<i64>,
    in_flight: u32,
    paused_until: Option<i64>,
    strikes: u32,
    last_failure: Option<i64>,
}

impl PinPeer {
    fn idle(&self, now: i64) -> bool {
        self.in_flight == 0
            && self.paused_until.is_none_or(|t| t <= now)
            && self
                .last_failure
                .is_none_or(|t| now - t >= PIN_STRIKES_RESET_MS)
    }
}

#[derive(Default)]
struct PinState {
    peers: HashMap<IpAddr, PinPeer>,
    global: Vec<i64>,
    global_paused_until: Option<i64>,
}

type OwnerCheck = Box<dyn Fn(&str) -> bool + Send + Sync>;

pub struct Pairing {
    key: [u8; 32],
    clock: Arc<dyn Clock>,
    live: Mutex<Option<Live>>,
    pins: Mutex<PinState>,
    changes: Mutex<Option<mpsc::Sender<CodeChanged>>>,
    owner_alive: OnceLock<OwnerCheck>,
    renewer: OnceLock<()>,
    tick: Duration,
    max_peers: usize,
}

impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing").finish_non_exhaustive()
    }
}

fn internal(message: &str) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

pub fn code_invalid() -> AppError {
    AppError::new(ErrorCode::PairCodeInvalid, "that pairing code is not valid")
}

pub fn pin_paused() -> AppError {
    AppError::new(
        ErrorCode::PairPinPaused,
        "too many wrong PINs; try again later",
    )
}

fn random_code() -> Result<String, AppError> {
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let bytes = arena_core::devices::random_bytes(4)?;
        let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if n < LIMIT {
            return Ok(format!(
                "{:0width$}",
                n % 1_000_000,
                width = PAIR_CODE_DIGITS
            ));
        }
    }
}

fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn peer_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let bits = u128::from(v6) & !((1u128 << 64) - 1);
                IpAddr::V6(bits.into())
            }
        },
    }
}

fn pause_for(strikes: u32) -> u32 {
    let doubled = u64::from(PAIR_PIN_PAUSE_S) << strikes.min(16);
    u32::try_from(doubled.min(u64::from(MAX_PIN_PAUSE_S))).unwrap_or(MAX_PIN_PAUSE_S)
}

fn retry_after(until: i64, now: i64) -> u32 {
    u32::try_from((until - now + 999) / 1000)
        .unwrap_or(PAIR_PIN_PAUSE_S)
        .max(1)
}

impl Pairing {
    pub fn new(clock: Arc<dyn Clock>) -> Result<Self, AppError> {
        let mut key = [0u8; 32];
        key.copy_from_slice(&arena_core::devices::random_bytes(32)?);
        Ok(Self {
            key,
            clock,
            live: Mutex::new(None),
            pins: Mutex::new(PinState::default()),
            changes: Mutex::new(None),
            owner_alive: OnceLock::new(),
            renewer: OnceLock::new(),
            tick: RENEW_TICK,
            max_peers: MAX_PEERS,
        })
    }

    pub fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    pub fn with_max_peers(mut self, max: usize) -> Self {
        self.max_peers = max;
        self
    }

    pub fn set_owner_check(&self, check: impl Fn(&str) -> bool + Send + Sync + 'static) {
        let _ = self.owner_alive.set(Box::new(check));
    }

    pub fn subscribe(&self) -> mpsc::Receiver<CodeChanged> {
        let (tx, rx) = mpsc::channel(16);
        if let Ok(mut slot) = self.changes.lock() {
            *slot = Some(tx);
        }
        rx
    }

    fn announce(&self, change: CodeChanged) {
        if let Ok(slot) = self.changes.lock()
            && let Some(tx) = slot.as_ref()
        {
            let _ = tx.try_send(change);
        }
    }

    fn hash(&self, code: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.key);
        h.update(code.as_bytes());
        h.finalize().into()
    }

    fn mint(&self, owner: Arc<str>) -> Result<(Live, KioskPairingCode), AppError> {
        let code = random_code()?;
        let expires_at = self.clock.now_ms() + PAIR_CODE_TTL_MS;
        Ok((
            Live {
                hash: self.hash(&code),
                expires_at,
                owner,
                wrong: 0,
            },
            KioskPairingCode { code, expires_at },
        ))
    }

    fn owner_alive(&self, owner: &str) -> bool {
        self.owner_alive.get().is_some_and(|check| check(owner))
    }

    fn renew_into(&self, live: &mut Option<Live>, owner: Arc<str>, why: &'static str) {
        if !self.owner_alive(&owner) {
            tracing::info!(
                reason = why,
                "kiosk pairing code not renewed: the panel is closed"
            );
            return;
        }
        match self.mint(Arc::clone(&owner)) {
            Ok((next, code)) => {
                *live = Some(next);
                tracing::info!(
                    reason = why,
                    expires_at = code.expires_at,
                    "kiosk pairing code renewed"
                );
                self.announce(CodeChanged {
                    owner,
                    code: Some(code),
                });
            }
            Err(e) => tracing::warn!(code = e.code.as_str(), "no fresh pairing code"),
        }
    }

    pub fn issue(self: &Arc<Self>, owner: &str) -> Result<KioskPairingCode, AppError> {
        self.start_renewer();
        let (next, code) = self.mint(owner.into())?;
        let mut live = self
            .live
            .lock()
            .map_err(|_| internal("pairing lock poisoned"))?;
        if let Some(old) = live.as_ref()
            && &*old.owner != owner
        {
            tracing::info!("kiosk pairing moved to another manager's panel");
            self.announce(CodeChanged {
                owner: Arc::clone(&old.owner),
                code: None,
            });
        }
        *live = Some(next);
        tracing::info!(expires_at = code.expires_at, "kiosk pairing code issued");
        Ok(code)
    }

    pub fn discard(&self, owner: &str) {
        if let Ok(mut live) = self.live.lock()
            && live.as_ref().is_some_and(|l| &*l.owner == owner)
        {
            *live = None;
            tracing::info!("kiosk pairing code discarded");
        }
    }

    pub fn has_live_code(&self) -> bool {
        let now = self.clock.now_ms();
        self.live
            .lock()
            .ok()
            .is_some_and(|l| l.as_ref().is_some_and(|l| l.expires_at > now))
    }

    pub fn take(&self, code: &str) -> Result<Taken, AppError> {
        let now = self.clock.now_ms();
        let presented = self.hash(code);
        let mut live = self
            .live
            .lock()
            .map_err(|_| internal("pairing lock poisoned"))?;
        let Some(current) = live.as_mut() else {
            return Err(code_invalid());
        };
        if current.expires_at <= now {
            return Err(code_invalid());
        }
        if !arena_proto::is_pair_code(code) || !same(&current.hash, &presented) {
            current.wrong += 1;
            if current.wrong >= MAX_WRONG_CODES {
                let owner = Arc::clone(&current.owner);
                *live = None;
                tracing::warn!(
                    wrong = MAX_WRONG_CODES,
                    "too many wrong pairing codes; replacing the code"
                );
                self.renew_into(&mut live, owner, "wrong_codes");
            }
            return Err(code_invalid());
        }
        let taken = live.take().map(|live| Taken { live });
        taken.ok_or_else(code_invalid)
    }

    pub fn restore(&self, taken: Taken) {
        if let Ok(mut live) = self.live.lock()
            && live.is_none()
        {
            *live = Some(taken.live);
            tracing::info!("kiosk pairing failed; the code stays valid");
        }
    }

    pub fn used(&self, taken: Taken) {
        tracing::info!("kiosk pairing code used");
        if let Ok(mut live) = self.live.lock()
            && live.is_none()
        {
            self.renew_into(&mut live, taken.live.owner, "used");
        }
    }

    pub fn consume(&self, code: &str) -> Result<(), AppError> {
        let taken = self.take(code)?;
        self.used(taken);
        Ok(())
    }

    fn renew_expired(&self) {
        let now = self.clock.now_ms();
        let Ok(mut live) = self.live.lock() else {
            return;
        };
        let Some(current) = live.as_ref().filter(|l| l.expires_at <= now) else {
            return;
        };
        let owner = Arc::clone(&current.owner);
        *live = None;
        self.renew_into(&mut live, owner, "expired");
    }

    fn start_renewer(self: &Arc<Self>) {
        if self.renewer.set(()).is_err() {
            return;
        }
        let weak = Arc::downgrade(self);
        let tick = self.tick;
        tokio::spawn(async move {
            let mut every = tokio::time::interval(tick);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                let Some(me) = weak.upgrade() else {
                    return;
                };
                me.renew_expired();
            }
        });
    }

    pub fn pin_begin(&self, ip: IpAddr) -> Result<(), u32> {
        let now = self.clock.now_ms();
        let key = peer_key(ip);
        let Ok(mut state) = self.pins.lock() else {
            return Err(PAIR_PIN_PAUSE_S);
        };
        if let Some(until) = state.global_paused_until {
            if until > now {
                return Err(retry_after(until, now));
            }
            state.global_paused_until = None;
        }
        if !state.peers.contains_key(&key) && state.peers.len() >= self.max_peers {
            state.peers.retain(|_, p| !p.idle(now));
            if state.peers.len() >= self.max_peers {
                tracing::warn!("too many peers trying pairing PINs; refusing new ones");
                return Err(PAIR_PIN_PAUSE_S);
            }
        }
        let peer = state.peers.entry(key).or_default();
        if peer
            .last_failure
            .is_some_and(|t| now - t >= PIN_STRIKES_RESET_MS)
        {
            peer.strikes = 0;
            peer.failures.clear();
            peer.last_failure = None;
        }
        if let Some(until) = peer.paused_until {
            if until > now {
                return Err(retry_after(until, now));
            }
            peer.paused_until = None;
        }
        peer.failures.retain(|t| now - t < PAIR_PIN_WINDOW_MS);
        if peer.failures.len() + peer.in_flight as usize >= PAIR_PIN_MAX_FAILURES as usize {
            return Err(PAIR_PIN_PAUSE_S);
        }
        peer.in_flight += 1;
        Ok(())
    }

    pub fn pin_finish(&self, ip: IpAddr, outcome: PinOutcome) -> Option<u32> {
        let now = self.clock.now_ms();
        let key = peer_key(ip);
        let mut state = self.pins.lock().ok()?;
        if outcome == PinOutcome::Wrong {
            state.global.retain(|t| now - t < PAIR_PIN_WINDOW_MS);
            state.global.push(now);
        }
        let global_paused = if state.global.len() >= GLOBAL_PIN_FAILURES {
            state.global.clear();
            let until = now + i64::from(GLOBAL_PIN_PAUSE_S) * 1000;
            state.global_paused_until = Some(until);
            tracing::warn!(
                "too many wrong pairing PINs on this LAN; pausing the PIN option for 5 min"
            );
            Some(retry_after(until, now))
        } else {
            None
        };
        let peer = state.peers.entry(key).or_default();
        peer.in_flight = peer.in_flight.saturating_sub(1);
        let peer_paused = match outcome {
            PinOutcome::Matched => {
                *peer = PinPeer {
                    in_flight: peer.in_flight,
                    ..PinPeer::default()
                };
                None
            }
            PinOutcome::Unchecked => None,
            PinOutcome::Wrong => {
                peer.failures.retain(|t| now - t < PAIR_PIN_WINDOW_MS);
                peer.failures.push(now);
                peer.last_failure = Some(now);
                if peer.failures.len() >= PAIR_PIN_MAX_FAILURES as usize {
                    peer.failures.clear();
                    let seconds = pause_for(peer.strikes);
                    peer.strikes += 1;
                    let until = now + i64::from(seconds) * 1000;
                    peer.paused_until = Some(until);
                    tracing::warn!(peer = %key, seconds, "too many wrong pairing PINs; pausing the PIN option");
                    Some(retry_after(until, now))
                } else {
                    None
                }
            }
        };
        match (peer_paused, global_paused) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinOutcome {
    Matched,
    Wrong,
    Unchecked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_core::ManualClock;

    const T0: i64 = 1_800_000_000_000;

    fn pairing() -> (Arc<ManualClock>, Arc<Pairing>) {
        let clock = Arc::new(ManualClock::new(T0));
        let p = Pairing::new(clock.clone())
            .unwrap()
            .with_tick(Duration::from_millis(10));
        p.set_owner_check(|_| true);
        (clock, Arc::new(p))
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn wrong_pins(p: &Pairing, from: IpAddr, n: usize) -> Option<u32> {
        let mut last = None;
        for _ in 0..n {
            p.pin_begin(from).unwrap();
            last = p.pin_finish(from, PinOutcome::Wrong);
        }
        last
    }

    #[tokio::test]
    async fn a_code_is_six_digits_for_ten_minutes_single_use_and_renewed_for_the_panel() {
        let (_clock, p) = pairing();
        let mut changes = p.subscribe();
        let first = p.issue("tok").unwrap();
        assert!(arena_proto::is_pair_code(&first.code));
        assert_eq!(first.expires_at, T0 + PAIR_CODE_TTL_MS);
        assert_eq!(
            p.consume("000000x").unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
        p.consume(&first.code).unwrap();
        assert_eq!(
            p.consume(&first.code).unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
        let next = changes.try_recv().unwrap();
        assert_eq!(&*next.owner, "tok");
        let next = next.code.unwrap();
        assert_ne!(next.code, first.code);
        p.consume(&next.code).unwrap();
    }

    #[tokio::test]
    async fn a_new_request_replaces_the_old_code_and_closing_discards_it() {
        let (_clock, p) = pairing();
        let mut old = p.issue("tok").unwrap();
        let mut new = p.issue("tok").unwrap();
        while new.code == old.code {
            old = new;
            new = p.issue("tok").unwrap();
        }
        assert_eq!(
            p.consume(&old.code).unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
        p.discard("someone-else");
        assert!(p.has_live_code());
        p.discard("tok");
        assert!(!p.has_live_code());
        assert_eq!(
            p.consume(&new.code).unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
    }

    #[tokio::test]
    async fn another_managers_request_tells_the_first_panel_it_was_replaced() {
        let (_clock, p) = pairing();
        let mut changes = p.subscribe();
        p.issue("first").unwrap();
        p.issue("first").unwrap();
        assert!(changes.try_recv().is_err());
        p.issue("second").unwrap();
        let moved = changes.try_recv().unwrap();
        assert_eq!(&*moved.owner, "first");
        assert!(moved.replaced());
        assert_eq!(moved.to_json(), r#"{"replaced":true}"#);
    }

    #[tokio::test]
    async fn an_expired_code_is_refused_and_an_open_panel_gets_a_fresh_one() {
        let (clock, p) = pairing();
        let mut changes = p.subscribe();
        let code = p.issue("tok").unwrap();
        clock.advance(PAIR_CODE_TTL_MS);
        assert_eq!(
            p.consume(&code.code).unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
        let renewed = tokio::time::timeout(Duration::from_secs(5), changes.recv())
            .await
            .unwrap()
            .unwrap()
            .code
            .unwrap();
        assert_eq!(renewed.expires_at, T0 + 2 * PAIR_CODE_TTL_MS);
        p.consume(&renewed.code).unwrap();
    }

    #[tokio::test]
    async fn a_closed_or_unwired_panel_gets_no_renewal() {
        for check in [Some(false), None] {
            let clock = Arc::new(ManualClock::new(T0));
            let p = Arc::new(Pairing::new(clock.clone()).unwrap());
            if let Some(alive) = check {
                p.set_owner_check(move |_| alive);
            }
            let mut changes = p.subscribe();
            let code = p.issue("tok").unwrap();
            clock.advance(PAIR_CODE_TTL_MS + 1);
            p.renew_expired();
            assert!(changes.try_recv().is_err());
            assert!(!p.has_live_code());
            assert!(p.consume(&code.code).is_err());
        }
    }

    #[tokio::test]
    async fn ten_wrong_codes_replace_the_live_code() {
        let (_clock, p) = pairing();
        let mut changes = p.subscribe();
        let code = p.issue("tok").unwrap().code;
        let wrong = if code == "000000" { "000001" } else { "000000" };
        for _ in 0..MAX_WRONG_CODES {
            assert!(p.take(wrong).is_err());
        }
        let fresh = changes.try_recv().unwrap().code.unwrap();
        assert_ne!(fresh.code, code);
        assert_eq!(
            p.consume(&code).unwrap_err().code,
            ErrorCode::PairCodeInvalid
        );
        p.consume(&fresh.code).unwrap();
    }

    #[tokio::test]
    async fn a_failed_pairing_restores_the_code() {
        let (_clock, p) = pairing();
        let mut changes = p.subscribe();
        let code = p.issue("tok").unwrap().code;
        let taken = p.take(&code).unwrap();
        assert!(p.take(&code).is_err());
        p.restore(taken);
        p.consume(&code).unwrap();
        assert!(changes.try_recv().unwrap().code.is_some());
    }

    #[tokio::test]
    async fn two_uses_of_one_code_at_once_let_exactly_one_through() {
        let (_clock, p) = pairing();
        let code = p.issue("tok").unwrap().code;
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let (p, code) = (Arc::clone(&p), code.clone());
            tasks.push(tokio::spawn(async move { p.consume(&code).is_ok() }));
        }
        let mut ok = 0;
        for t in tasks {
            ok += usize::from(t.await.unwrap());
        }
        assert_eq!(ok, 1);
    }

    #[tokio::test]
    async fn the_pause_doubles_per_peer_up_to_fifteen_minutes_and_a_success_resets_it() {
        let (clock, p) = pairing();
        let peer = ip("10.0.0.5");
        let mut expected = Vec::new();
        let mut got = Vec::new();
        for strike in 0..6u32 {
            got.push(wrong_pins(&p, peer, 5).unwrap());
            expected.push(pause_for(strike));
            assert!(p.pin_begin(peer).is_err());
            clock.advance(i64::from(*got.last().unwrap()) * 1000);
        }
        assert_eq!(got, [60, 120, 240, 480, 900, 900]);
        assert_eq!(got, expected);
        p.pin_begin(peer).unwrap();
        p.pin_finish(peer, PinOutcome::Matched);
        assert_eq!(wrong_pins(&p, peer, 5), Some(60));
    }

    #[tokio::test]
    async fn the_strike_count_is_forgotten_after_an_hour_without_failures() {
        let (clock, p) = pairing();
        let peer = ip("10.0.0.5");
        assert_eq!(wrong_pins(&p, peer, 5), Some(60));
        clock.advance(60_000);
        assert_eq!(wrong_pins(&p, peer, 5), Some(120));
        clock.advance(PIN_STRIKES_RESET_MS);
        assert_eq!(wrong_pins(&p, peer, 5), Some(60));
    }

    #[tokio::test]
    async fn wrong_pins_older_than_ten_minutes_do_not_count() {
        let (clock, p) = pairing();
        let peer = ip("10.0.0.5");
        assert_eq!(wrong_pins(&p, peer, 4), None);
        clock.advance(PAIR_PIN_WINDOW_MS);
        assert_eq!(wrong_pins(&p, peer, 1), None);
    }

    #[tokio::test]
    async fn ipv6_peers_share_a_budget_per_64() {
        let (_clock, p) = pairing();
        for i in 0..4 {
            assert_eq!(wrong_pins(&p, ip(&format!("fd00:1:2:3::{i:x}")), 1), None);
        }
        assert_eq!(wrong_pins(&p, ip("fd00:1:2:3:ffff::9"), 1), Some(60));
        assert!(p.pin_begin(ip("fd00:1:2:3::77")).is_err());
        p.pin_begin(ip("fd00:1:2:4::1")).unwrap();
        assert_eq!(peer_key(ip("::ffff:10.0.0.1")), ip("10.0.0.1"));
    }

    #[tokio::test]
    async fn twenty_wrong_pins_across_peers_pause_every_peer_for_five_minutes() {
        let (clock, p) = pairing();
        for i in 0..4 {
            assert_eq!(wrong_pins(&p, ip(&format!("10.0.1.{i}")), 4), None);
        }
        assert_eq!(wrong_pins(&p, ip("10.0.1.4"), 4), Some(GLOBAL_PIN_PAUSE_S));
        assert!(p.pin_begin(ip("10.0.2.1")).is_err());
        clock.advance(i64::from(GLOBAL_PIN_PAUSE_S) * 1000 - 1000);
        assert_eq!(p.pin_begin(ip("10.0.2.1")), Err(1));
        clock.advance(1000);
        p.pin_begin(ip("10.0.2.1")).unwrap();
    }

    #[tokio::test]
    async fn a_full_peer_map_refuses_new_peers_until_one_can_be_evicted() {
        let clock = Arc::new(ManualClock::new(T0));
        let p = Pairing::new(clock.clone()).unwrap().with_max_peers(2);
        wrong_pins(&p, ip("10.0.0.1"), 1);
        wrong_pins(&p, ip("10.0.0.2"), 1);
        assert_eq!(p.pin_begin(ip("10.0.0.3")), Err(PAIR_PIN_PAUSE_S));
        clock.advance(PIN_STRIKES_RESET_MS);
        p.pin_begin(ip("10.0.0.3")).unwrap();
    }
}

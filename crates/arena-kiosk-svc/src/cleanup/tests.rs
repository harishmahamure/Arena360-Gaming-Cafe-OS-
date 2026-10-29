//! The cleanup pipeline on the fake host and the fake process table (the service-level flow
//! against a real Hub is in `tests/runtime.rs`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{
    IntentKind, IntentPayload, KEY_KIOSK_VOLUME_DEFAULT, KioskLogLevel, LockReason, PcAction,
    ProcessMode, SessionScreen,
};
use tokio::sync::{Notify, broadcast, watch};

use super::fake::FakeHost;
use super::*;
use crate::clock::SystemClock;
use crate::paths::DataDir;
use crate::procs::fake::{FakeProc, FakeSystem};
use crate::procs::{EnforceOptions, ProcessApi, ProcessLists};
use crate::runtime::{Ctx, Timings};
use crate::secret::SecretStore;

const PLAYER_SID: &str = "S-1-5-21-1-2-3-1001";
const INSTALL: &str = r"C:\Program Files\Arena 360 Kiosk";
const APP: &str = r"C:\Program Files\Arena 360 Kiosk\arena-kiosk-ui.exe";
const SVC: &str = r"C:\Program Files\Arena 360 Kiosk\arena-kiosk-svc.exe";

struct NoSecret;
impl SecretStore for NoSecret {
    fn save(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn load(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

fn ctx_in(dir: &std::path::Path, retry: Duration) -> Arc<Ctx> {
    let data = DataDir(dir.to_path_buf());
    let queue = QueueDb::open(&data.queue()).unwrap();
    let pc = crate::pc::PcState::load(&queue);
    let cleanup = CleanupState::load(&queue);
    // Report mode (the default): the enforcer itself kills nothing, so every program that
    // dies, dies in the cleanup.
    let (lists, _) = watch::channel(Arc::new(ProcessLists::defaults()));
    let (toasts, _) = broadcast::channel(16);
    let (hub_state, _) = watch::channel(arena_proto::KioskHubState::Reconnecting);
    Arc::new(Ctx {
        data,
        secrets: Box::new(NoSecret),
        clock: Arc::new(SystemClock),
        timings: Timings {
            cleanup_retry: retry,
            ..Timings::default()
        },
        queue: Mutex::new(queue),
        queued: Notify::new(),
        hub_state,
        app_state: Mutex::new(None),
        lists,
        theme: watch::channel(false).0,
        attract: watch::channel(crate::attract::Shown::default()).0,
        toasts,
        launcher: Arc::new(crate::launcher::NoopLauncher),
        admin: crate::admin_exit::AdminState::load(Box::new(NoSecret)),
        link_hold: watch::channel(false).0,
        discovery: crate::discovery::Discovery::system(),
        provisioned: Notify::new(),
        test_session: watch::channel(None).0,
        pc,
        closer: Mutex::new(None),
        cleanup,
    })
}

struct T {
    dir: tempfile::TempDir,
    ctx: Arc<Ctx>,
    host: Arc<FakeHost>,
    stop: watch::Sender<bool>,
}

fn proc(path: &str, parent: Option<u32>) -> FakeProc {
    FakeProc {
        session_id: 1,
        user_sid: PLAYER_SID.into(),
        path: path.into(),
        parent,
    }
}

fn sys() -> Arc<FakeSystem> {
    let sys = Arc::new(FakeSystem::default());
    sys.set_player(crate::config::DEFAULT_PLAYER_USER, PLAYER_SID, 1);
    sys
}

/// Starts the worker on the fake host and the process enforcer on the fake table.
async fn start_in(dir: tempfile::TempDir, retry: Duration, sys: Arc<FakeSystem>) -> T {
    start_with(dir, retry, sys, Pipeline::standard()).await
}

async fn start_with(
    dir: tempfile::TempDir,
    retry: Duration,
    sys: Arc<FakeSystem>,
    pipeline: Pipeline,
) -> T {
    let ctx = ctx_in(dir.path(), retry);
    let host = Arc::new(FakeHost::default());
    let (stop, stop_rx) = watch::channel(false);
    enable(&ctx);
    tokio::spawn(crate::procs::run(
        Arc::clone(&ctx),
        ProcessApi::of(Arc::clone(&sys)),
        EnforceOptions {
            install_dir: Some(INSTALL.into()),
            app_image: Some(APP.into()),
            app_config: None,
            own_images: vec![SVC.into()],
            system_root: r"C:\Windows".into(),
        },
        stop_rx.clone(),
    ));
    tokio::spawn(run(
        Arc::clone(&ctx),
        Arc::clone(&host) as Arc<dyn CleanupHost>,
        pipeline,
        stop_rx,
    ));
    eventually("the closer", || ctx.closer.lock().unwrap().is_some()).await;
    T {
        dir,
        ctx,
        host,
        stop,
    }
}

async fn start() -> T {
    start_in(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        sys(),
    )
    .await
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

impl T {
    fn reason(&self) -> Option<LockReason> {
        self.ctx.pc.screen.borrow().reason
    }

    fn locked(&self) -> bool {
        self.ctx.pc.screen.borrow().locked
    }

    fn lock(&self, reason: LockReason) {
        crate::pc::set_locked(&self.ctx, true, Some(reason));
    }

    fn command(&self, id: &str, action: PcAction) {
        crate::pc::apply(&self.ctx, id, action, None, None);
    }

    /// A session starts on this PC (the Hub's `session.started`).
    fn start_session(&self) {
        crate::session::sync(
            &self.ctx,
            SessionScreen {
                session_id: "S1".into(),
                device_name: "PC 1".into(),
                elapsed_ms: 0,
                lock_after_ms: 3_600_000,
                time_up_mode: "clean_up".into(),
                grace_ms: 0,
                ordered_items: 0,
                username: None,
                guest_no: None,
                remaining_ms: None,
                balance_minor: None,
                wallet_left_minor: None,
                plan_name: None,
            },
            true,
        );
    }

    fn intents(&self) -> Vec<arena_proto::Intent> {
        self.ctx
            .queue
            .lock()
            .unwrap()
            .entries()
            .unwrap()
            .into_iter()
            .map(|q| q.intent)
            .collect()
    }

    fn failures(&self) -> Vec<(String, String)> {
        self.intents()
            .into_iter()
            .filter_map(|i| match i.payload {
                IntentPayload::KioskCleanupFailed(f) => Some((f.step, f.detail)),
                _ => None,
            })
            .collect()
    }

    fn done(&self) -> usize {
        self.intents()
            .iter()
            .filter(|i| {
                matches!(&i.payload, IntentPayload::KioskLog(l)
                    if l.code == DONE_CODE && l.level == KioskLogLevel::Info)
            })
            .count()
    }

    fn kv(&self, key: &str) -> Option<String> {
        kv_get(&self.ctx, key)
    }

    async fn held(&self) {
        eventually("the hold", || self.kv(HOLD_KEY).is_some()).await;
    }

    async fn shows(&self, reason: LockReason) {
        eventually("the lock", || {
            self.locked() && self.reason() == Some(reason)
        })
        .await;
    }

    fn stop(&self) {
        let _ = self.stop.send(true);
        let _ = &self.dir;
    }
}

fn session_ends(t: &T, reason: LockReason) {
    t.start_session();
    t.lock(reason);
}

#[test]
fn the_standard_pipeline_names_its_steps_in_order() {
    assert_eq!(
        Pipeline::standard().names(),
        [
            "programs",
            "signout_steam",
            "signout_epic",
            "signout_riot",
            "signout_discord",
            "reset_chrome",
            "reset_edge",
            "reset_firefox",
            "credentials",
            "clipboard",
            "temp_files",
            "volume"
        ]
    );
}

#[tokio::test]
async fn a_staff_end_cleans_every_program_before_the_bill_lock_shows() {
    let sys = sys();
    // The kiosk app and its WebView2, the service, a protected image and another user's
    // process stay; the player's own programs go.
    sys.add_running(10, proc(APP, None));
    sys.add_running(
        11,
        proc(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\1\msedgewebview2.exe",
            Some(10),
        ),
    );
    sys.add_running(12, proc(SVC, None));
    sys.add_running(13, proc(r"C:\Windows\System32\ctfmon.exe", None));
    sys.add_running(
        14,
        FakeProc {
            user_sid: "S-1-5-18".into(),
            ..proc(r"C:\Windows\System32\csrss.exe", None)
        },
    );
    sys.add_running(20, proc(r"C:\Games\Game\game.exe", Some(10)));
    sys.add_running(
        21,
        proc(r"C:\Users\player\AppData\Local\Discord\Discord.exe", None),
    );
    sys.add_running(
        22,
        proc(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\1\msedgewebview2.exe",
            Some(20),
        ),
    );
    let t = start_in(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        Arc::clone(&sys),
    )
    .await;
    assert_eq!(t.ctx.lists.borrow().mode, ProcessMode::Report);
    t.start_session();
    t.lock(LockReason::PayAtCounter);
    // At once the opaque cleanup lock covers the screen, not the bill lock.
    assert!(t.locked());
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    t.shows(LockReason::PayAtCounter).await;
    for pid in [20, 21, 22] {
        assert!(!sys.alive(pid), "pid {pid} is still running");
    }
    for pid in [10, 11, 12, 13, 14] {
        assert!(sys.alive(pid), "pid {pid} was ended");
    }
    assert_eq!(
        t.host.calls(),
        ["credentials", "clipboard", "temp_files", "volume=50"]
    );
    assert_eq!(t.done(), 1);
    assert!(t.failures().is_empty());
    assert!(!t.ctx.cleanup.dirty());
    assert_eq!(t.kv(PENDING_KEY).as_deref(), Some("0"));
    assert_eq!(t.kv(AFTER_KEY), None);
    assert_eq!(t.kv(HOLD_KEY), None);
    t.stop();
}

#[tokio::test]
async fn the_volume_is_the_branch_default_once_synced() {
    let t = start().await;
    t.ctx
        .queue
        .lock()
        .unwrap()
        .kv_set(KEY_KIOSK_VOLUME_DEFAULT, "30", 4)
        .unwrap();
    session_ends(&t, LockReason::EndedByStaff);
    t.shows(LockReason::EndedByStaff).await;
    assert_eq!(t.host.volume(), Some(30));
    t.stop();
}

#[tokio::test]
async fn a_local_and_a_hub_time_up_clean_once() {
    let t = start().await;
    t.start_session();
    t.lock(LockReason::TimesUp);
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    // The Hub's lock arrives while the pipeline runs: no second run, same lock afterwards.
    t.lock(LockReason::TimesUp);
    t.shows(LockReason::TimesUp).await;
    // And after it: nothing is dirty, the lock shows as it is.
    t.lock(LockReason::TimesUp);
    assert_eq!(t.reason(), Some(LockReason::TimesUp));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(t.host.calls().len(), 4);
    assert_eq!(t.done(), 1);
    t.stop();
}

#[tokio::test]
async fn staff_handles_grace_end_does_not_clean_until_staff_end() {
    let t = start().await;
    t.start_session();
    t.lock(LockReason::TimesUpSeeCounter);
    assert_eq!(t.reason(), Some(LockReason::TimesUpSeeCounter));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(t.host.calls().is_empty());
    assert!(t.ctx.cleanup.dirty());
    t.lock(LockReason::PayAtCounter);
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    t.shows(LockReason::PayAtCounter).await;
    assert_eq!(t.host.calls().len(), 4);
    t.stop();
}

#[tokio::test]
async fn other_locks_and_clean_pcs_are_untouched() {
    let t = start().await;
    // Staff lock a PC that had no session: nothing to clean.
    t.lock(LockReason::LockedByStaff);
    assert_eq!(t.reason(), Some(LockReason::LockedByStaff));
    // A session ends while nobody started one since the last cleanup: nothing is dirty.
    t.lock(LockReason::PayAtCounter);
    assert_eq!(t.reason(), Some(LockReason::PayAtCounter));
    // A lock for a pause during a session is not a session end.
    t.start_session();
    t.lock(LockReason::Paused);
    assert_eq!(t.reason(), Some(LockReason::Paused));
    assert!(t.host.calls().is_empty());
    t.stop();
}

#[tokio::test]
async fn a_step_that_fails_once_is_retried_and_nothing_is_reported() {
    let t = start().await;
    t.host.fail("temp_files", 1);
    session_ends(&t, LockReason::PayAtCounter);
    t.shows(LockReason::PayAtCounter).await;
    assert_eq!(
        t.host.calls(),
        [
            "credentials",
            "clipboard",
            "temp_files",
            "temp_files",
            "volume=50"
        ]
    );
    assert!(t.failures().is_empty());
    assert_eq!(t.done(), 1);
    t.stop();
}

#[tokio::test]
async fn a_step_that_fails_twice_is_reported_the_others_run_and_the_lock_holds() {
    let sys = sys();
    sys.add_running(20, proc(r"C:\Games\stuck.exe", None));
    sys.fail_kill(20);
    let t = start_in(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        Arc::clone(&sys),
    )
    .await;
    t.host.fail("temp_files", 9);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    let mut failures = t.failures();
    failures.sort();
    assert_eq!(
        failures,
        [
            (
                "programs".to_owned(),
                "1 program would not close".to_owned()
            ),
            (
                "temp_files".to_owned(),
                "fake: temp_files failed".to_owned()
            ),
        ]
    );
    // The steps after the failures still ran.
    assert_eq!(
        t.host.calls(),
        [
            "credentials",
            "clipboard",
            "temp_files",
            "temp_files",
            "volume=50"
        ]
    );
    // The screen holds the cleanup lock, with no result and no bill lock.
    assert!(t.locked());
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    assert_eq!(t.done(), 0);
    assert!(t.ctx.cleanup.dirty());
    // Every failure intent is a device intent a kiosk may send.
    for i in t.intents() {
        assert_eq!(i.actor.kind, arena_proto::ActorKind::Device);
        assert!(!i.idempotency_key.is_empty());
        if i.kind == IntentKind::KioskCleanupFailed {
            assert!(arena_proto::class_scope(arena_proto::ClientClass::Paired).contains(&i.kind));
        }
    }
    t.stop();
}

#[tokio::test]
async fn retry_cleanup_runs_the_pipeline_again_and_shows_the_remembered_lock() {
    let t = start().await;
    // The first run fails both tries; the retry command's run passes.
    t.host.fail("temp_files", 2);
    session_ends(&t, LockReason::EndedByStaff);
    t.held().await;
    assert_eq!(t.failures().len(), 1);
    t.command("R1", PcAction::CleanupRetry);
    t.shows(LockReason::EndedByStaff).await;
    assert_eq!(t.done(), 1);
    assert_eq!(t.kv(HOLD_KEY), None);
    assert!(!t.ctx.cleanup.dirty());
    assert_eq!(t.failures().len(), 1);
    // The same command id again is acknowledged only.
    t.command("R1", PcAction::CleanupRetry);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(t.done(), 1);
    t.stop();
}

#[tokio::test]
async fn a_retry_that_fails_holds_again_and_reports_again() {
    let t = start().await;
    t.host.fail("volume", 4);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    assert_eq!(t.failures().len(), 1);
    t.command("R1", PcAction::CleanupRetry);
    eventually("the second report", || t.failures().len() == 2).await;
    t.held().await;
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    assert_eq!(t.done(), 0);
    t.stop();
}

#[tokio::test]
async fn checked_releases_the_hold_without_a_run() {
    let t = start().await;
    t.host.fail("clipboard", 9);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    let calls = t.host.calls().len();
    t.command("C1", PcAction::CleanupChecked);
    assert!(t.locked());
    assert_eq!(t.reason(), Some(LockReason::PayAtCounter));
    assert_eq!(t.kv(HOLD_KEY), None);
    assert_eq!(t.kv(AFTER_KEY), None);
    assert!(!t.ctx.cleanup.dirty());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(t.host.calls().len(), calls, "no run on Checked");
    assert_eq!(t.done(), 0);
    // Nothing held now: both commands are ignored.
    t.command("C2", PcAction::CleanupChecked);
    t.command("R2", PcAction::CleanupRetry);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(t.host.calls().len(), calls);
    t.stop();
}

#[tokio::test]
async fn a_session_that_starts_meanwhile_cancels_the_deferred_lock() {
    // A held cleanup: an Undo of the End restarts the session and unlocks.
    let t = start().await;
    t.host.fail("volume", 9);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    t.start_session();
    assert!(!t.locked());
    assert_eq!(t.reason(), None);
    assert_eq!(t.kv(AFTER_KEY), None);
    assert_eq!(t.kv(HOLD_KEY), None);
    assert!(t.ctx.cleanup.dirty(), "the restored session is dirty again");
    t.stop();

    // A cleanup still running: the rest of the pipeline is dropped.
    let t = start_in(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(300),
        sys(),
    )
    .await;
    t.host.fail("clipboard", 1);
    session_ends(&t, LockReason::PayAtCounter);
    eventually("the first try", || !t.host.calls().is_empty()).await;
    t.start_session();
    assert!(!t.locked());
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(!t.locked(), "the deferred lock never shows");
    assert!(t.host.calls().iter().all(|c| !c.starts_with("volume")));
    assert_eq!(t.done(), 0);
    t.stop();
}

#[tokio::test]
async fn a_restart_mid_clean_runs_it_again_and_a_held_one_stays_held() {
    // Mid-clean: pending and the remembered lock are in the kv, the screen was locked.
    let dir = tempfile::tempdir().unwrap();
    {
        let q = QueueDb::open(&DataDir(dir.path().to_path_buf()).queue()).unwrap();
        q.kv_set(PENDING_KEY, "1", 0).unwrap();
        q.kv_set(AFTER_KEY, "pay_at_counter", 0).unwrap();
        q.kv_set("pc.locked", "1", 0).unwrap();
    }
    let t = start_in(dir, Duration::from_millis(10), sys()).await;
    t.shows(LockReason::PayAtCounter).await;
    assert_eq!(t.host.calls().len(), 4);
    assert_eq!(t.done(), 1);
    t.stop();

    // Held when the service stopped: the lock comes back and nothing runs.
    let dir = tempfile::tempdir().unwrap();
    {
        let q = QueueDb::open(&DataDir(dir.path().to_path_buf()).queue()).unwrap();
        q.kv_set(PENDING_KEY, "1", 0).unwrap();
        q.kv_set(AFTER_KEY, "times_up", 0).unwrap();
        q.kv_set(HOLD_KEY, "1", 0).unwrap();
        q.kv_set("pc.locked", "1", 0).unwrap();
    }
    let t = start_in(dir, Duration::from_millis(10), sys()).await;
    eventually("the cleanup lock", || {
        t.reason() == Some(LockReason::PreparingPc)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(t.host.calls().is_empty());
    t.command("C1", PcAction::CleanupChecked);
    assert_eq!(t.reason(), Some(LockReason::TimesUp));
    t.stop();

    // A session running when the service stopped is not cleaned by the restart.
    let dir = tempfile::tempdir().unwrap();
    {
        let q = QueueDb::open(&DataDir(dir.path().to_path_buf()).queue()).unwrap();
        q.kv_set(PENDING_KEY, "1", 0).unwrap();
    }
    let t = start_in(dir, Duration::from_millis(10), sys()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(t.host.calls().is_empty());
    assert!(!t.locked());
    t.stop();
}

#[tokio::test]
async fn without_a_worker_a_session_end_only_locks() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_in(dir.path(), Duration::from_millis(10));
    mark_dirty(&ctx);
    crate::pc::set_locked(&ctx, true, Some(LockReason::PayAtCounter));
    assert_eq!(
        ctx.pc.screen.borrow().reason,
        Some(LockReason::PayAtCounter)
    );
}

#[test]
fn a_cleared_temp_folder_keeps_the_folder_and_loses_everything_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("Temp");
    std::fs::create_dir_all(temp.join("a").join("b")).unwrap();
    std::fs::write(temp.join("a").join("b").join("x.tmp"), "x").unwrap();
    std::fs::write(temp.join("y.tmp"), "y").unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "keep").unwrap();
    clear_dir_contents(&temp).unwrap();
    assert!(temp.is_dir());
    assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
    assert!(outside.join("keep.txt").exists());
    // A folder that is not there is clean.
    clear_dir_contents(&dir.path().join("missing")).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_in_the_temp_folder_is_removed_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("Temp");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "keep").unwrap();
    std::os::unix::fs::symlink(&outside, temp.join("link")).unwrap();
    clear_dir_contents(&temp).unwrap();
    assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
    assert!(outside.join("keep.txt").exists());
}

#[test]
fn a_reported_detail_is_one_short_clean_line() {
    assert_eq!(clean_detail("a\nb\u{7}c"), "abc");
    assert_eq!(
        clean_detail(&"x".repeat(MAX_CLEANUP_DETAIL_CHARS + 50))
            .chars()
            .count(),
        MAX_CLEANUP_DETAIL_CHARS
    );
    assert_eq!(reason_key(Some(LockReason::TimesUp)), "times_up");
    assert_eq!(parse_reason("times_up"), Some(LockReason::TimesUp));
    assert_eq!(parse_reason("none"), None);
    assert_eq!(reason_key(None), "none");
}

// ---- launcher, Discord and browser sign-out (Story 6.12) ----

const STEAM_EXE: &str = r"C:\Program Files (x86)\Steam\steam.exe";

fn write(path: &std::path::Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"secret").unwrap();
}

/// A player profile and a Steam folder with a login in every place the sign-out clears, and
/// files of each kind that must stay.
fn logins(t: &T) -> (std::path::PathBuf, std::path::PathBuf) {
    let profile = t.dir.path().join("profile");
    let steam = t.dir.path().join("Steam");
    let (local, roaming) = (
        profile.join("AppData/Local"),
        profile.join("AppData/Roaming"),
    );
    for f in [
        steam.join("config/loginusers.vdf"),
        steam.join("config/config.vdf"),
        steam.join("ssfn123456"),
        local.join("Steam/htmlcache/Cookies"),
        local.join("EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini"),
        local.join("EpicGamesLauncher/Saved/webcache_4147/Cookies"),
        local.join("Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml"),
        roaming.join("discord/Local Storage/leveldb/000003.log"),
        roaming.join("discordptb/Local Storage/leveldb/000003.log"),
        local.join("Google/Chrome/User Data/Default/Login Data"),
        local.join("Microsoft/Edge/User Data/Default/Login Data"),
        roaming.join("Mozilla/Firefox/profiles.ini"),
        roaming.join("Mozilla/Firefox/Profiles/abc.default/logins.json"),
        local.join("Mozilla/Firefox/Profiles/abc.default/cache2/entries/1"),
    ] {
        write(&f);
    }
    for f in [
        steam.join("steam.exe"),
        steam.join("steamapps/common/Game/game.exe"),
        local.join("EpicGamesLauncher/Saved/keep.txt"),
        local.join("Unrelated/keep.txt"),
        roaming.join("Unrelated/keep.txt"),
    ] {
        write(&f);
    }
    t.host.set_profile(profile.clone());
    t.host.set_steam(steam.clone());
    (profile, steam)
}

#[tokio::test]
async fn the_standard_cleanup_signs_every_launcher_and_browser_out() {
    let sys = sys();
    for (pid, image) in [
        (30, STEAM_EXE),
        (
            31,
            r"C:\Users\player\AppData\Local\Discord\app-1\Discord.exe",
        ),
        (32, r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
    ] {
        sys.add_running(pid, proc(image, None));
    }
    let t = start_in(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        Arc::clone(&sys),
    )
    .await;
    let (profile, steam) = logins(&t);
    session_ends(&t, LockReason::PayAtCounter);
    t.shows(LockReason::PayAtCounter).await;
    assert!(t.failures().is_empty(), "{:?}", t.failures());
    assert_eq!(t.done(), 1);
    for pid in [30, 31, 32] {
        assert!(!sys.alive(pid));
    }
    let (local, roaming) = (
        profile.join("AppData/Local"),
        profile.join("AppData/Roaming"),
    );
    for gone in [
        steam.join("config/loginusers.vdf"),
        steam.join("config/config.vdf"),
        steam.join("ssfn123456"),
        local.join("Steam/htmlcache"),
        local.join("EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini"),
        local.join("EpicGamesLauncher/Saved/webcache_4147"),
        local.join("Riot Games/Riot Client/Data"),
        roaming.join("discord"),
        roaming.join("discordptb"),
        local.join("Google/Chrome/User Data"),
        local.join("Microsoft/Edge/User Data"),
        roaming.join("Mozilla/Firefox"),
        local.join("Mozilla/Firefox"),
    ] {
        assert!(!gone.exists(), "{} is still there", gone.display());
    }
    for kept in [
        steam.join("steam.exe"),
        steam.join("steamapps/common/Game/game.exe"),
        local.join("EpicGamesLauncher/Saved/keep.txt"),
        local.join("Unrelated/keep.txt"),
        roaming.join("Unrelated/keep.txt"),
    ] {
        assert!(kept.exists(), "{} was removed", kept.display());
    }
    assert_eq!(
        t.host.calls(),
        [
            "steam_login",
            "credentials",
            "clipboard",
            "temp_files",
            "volume=50"
        ]
    );
    t.stop();
}

#[tokio::test]
async fn a_launcher_still_running_is_shut_down_before_its_files_are_removed() {
    let sys = sys();
    sys.add_running(30, proc(STEAM_EXE, None));
    sys.add_running(31, proc(r"C:\Games\other.exe", None));
    // Only the sign-out step runs: it has to end Steam itself, and only Steam.
    let t = start_with(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        Arc::clone(&sys),
        Pipeline::of(vec![Arc::new(signout::SignOut(signout::Target::Steam))]),
    )
    .await;
    let (_, steam) = logins(&t);
    session_ends(&t, LockReason::EndedByStaff);
    t.shows(LockReason::EndedByStaff).await;
    assert!(!sys.alive(30));
    assert!(sys.alive(31));
    assert_eq!(sys.killed(), [30]);
    assert!(!steam.join("config/loginusers.vdf").exists());
    t.stop();
}

#[tokio::test]
async fn a_launcher_that_will_not_exit_keeps_its_files_and_is_named_in_the_report() {
    let sys = sys();
    sys.add_running(30, proc(STEAM_EXE, None));
    sys.fail_kill(30);
    let t = start_with(
        tempfile::tempdir().unwrap(),
        Duration::from_millis(10),
        Arc::clone(&sys),
        Pipeline::of(vec![
            Arc::new(signout::SignOut(signout::Target::Steam)),
            Arc::new(signout::SignOut(signout::Target::Discord)),
        ]),
    )
    .await;
    let (profile, steam) = logins(&t);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    assert_eq!(
        t.failures(),
        [(
            "signout_steam".to_owned(),
            "Steam is still running".to_owned()
        )]
    );
    // Its files stay until it has exited; the next target still ran.
    assert!(steam.join("config/loginusers.vdf").exists());
    assert!(!profile.join("AppData/Roaming/discord").exists());
    assert_eq!(t.reason(), Some(LockReason::PreparingPc));
    assert!(t.host.calls().is_empty());
    t.stop();
}

#[tokio::test]
async fn a_sign_out_that_fails_after_its_retry_names_the_step_and_the_others_run() {
    let t = start().await;
    t.host.fail("steam_login", 9);
    t.host.fail("credentials", 9);
    logins(&t);
    session_ends(&t, LockReason::PayAtCounter);
    t.held().await;
    let mut failures = t.failures();
    failures.sort();
    assert_eq!(
        failures,
        [
            (
                "credentials".to_owned(),
                "fake: credentials failed".to_owned()
            ),
            (
                "signout_steam".to_owned(),
                "Steam: fake: steam_login failed".to_owned()
            ),
        ]
    );
    assert_eq!(t.done(), 0);
    t.stop();
}

#[tokio::test]
async fn launchers_and_browsers_that_are_not_installed_are_skipped_without_error() {
    let t = start().await;
    // A profile with none of them, and no Steam.
    t.host.set_profile(t.dir.path().join("empty-profile"));
    std::fs::create_dir_all(t.dir.path().join("empty-profile")).unwrap();
    session_ends(&t, LockReason::PayAtCounter);
    t.shows(LockReason::PayAtCounter).await;
    assert!(t.failures().is_empty());
    assert_eq!(t.done(), 1);
    t.stop();
}

#[tokio::test]
async fn a_browser_is_reset_to_the_branch_template_when_there_is_one() {
    let t = start().await;
    let (profile, _) = logins(&t);
    let templates = t.dir.path().join("templates");
    write(&templates.join("chrome/Local/Google/Chrome/User Data/First Run"));
    write(&templates.join("chrome/Local/Google/Chrome/User Data/Default/Preferences"));
    t.host.set_templates(templates);
    session_ends(&t, LockReason::PayAtCounter);
    t.shows(LockReason::PayAtCounter).await;
    assert!(t.failures().is_empty(), "{:?}", t.failures());
    let chrome = profile.join("AppData/Local/Google/Chrome/User Data");
    assert!(chrome.join("First Run").exists());
    assert!(chrome.join("Default/Preferences").exists());
    assert!(!chrome.join("Default/Login Data").exists());
    // Edge has no template folder: it is simply cleared.
    assert!(
        !profile
            .join("AppData/Local/Microsoft/Edge/User Data")
            .exists()
    );
    t.stop();
}

// ---- Story 6.15: the volume slider ----

fn bare_ctx() -> (tempfile::TempDir, Arc<Ctx>) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_in(dir.path(), Duration::from_millis(5));
    (dir, ctx)
}

#[tokio::test]
async fn the_volume_request_reads_the_default_then_sets_and_remembers_the_level() {
    let (_dir, ctx) = bare_ctx();
    let host = Arc::new(FakeHost::default());
    ctx.cleanup.volume.attach(host.clone());
    let read = crate::volume::request(&ctx, serde_json::Value::Null)
        .await
        .unwrap();
    assert_eq!(read, serde_json::json!({ "percent": DEFAULT_VOLUME }));
    assert_eq!(host.volume(), None, "reading changes nothing");
    ctx.queue
        .lock()
        .unwrap()
        .kv_set(KEY_KIOSK_VOLUME_DEFAULT, "30", 4)
        .unwrap();
    assert_eq!(crate::volume::level(&ctx), 30);
    let set = crate::volume::request(&ctx, serde_json::json!({ "percent": 65 }))
        .await
        .unwrap();
    assert_eq!(set, serde_json::json!({ "percent": 65 }));
    assert_eq!(host.volume(), Some(65));
    assert_eq!(crate::volume::level(&ctx), 65);
}

#[tokio::test]
async fn the_volume_request_refuses_an_out_of_range_level_and_a_pc_with_no_control() {
    let (_dir, ctx) = bare_ctx();
    let none = crate::volume::request(&ctx, serde_json::json!({ "percent": 40 }))
        .await
        .unwrap_err();
    assert_eq!(none.code, arena_proto::ErrorCode::Unsupported);
    let host = Arc::new(FakeHost::default());
    ctx.cleanup.volume.attach(host.clone());
    let high = crate::volume::request(&ctx, serde_json::json!({ "percent": 101 }))
        .await
        .unwrap_err();
    assert_eq!(high.code, arena_proto::ErrorCode::ValidationFailed);
    let bad = crate::volume::request(&ctx, serde_json::json!({ "percent": "loud" }))
        .await
        .unwrap_err();
    assert_eq!(bad.code, arena_proto::ErrorCode::BadFrame);
    assert_eq!(host.volume(), None);
}

#[tokio::test]
async fn a_failed_volume_change_is_an_error_and_keeps_the_old_level() {
    let (_dir, ctx) = bare_ctx();
    let host = Arc::new(FakeHost::default());
    ctx.cleanup.volume.attach(host.clone());
    crate::volume::set(&ctx, 20).await.unwrap();
    host.fail(STEP_VOLUME, 1);
    let e = crate::volume::set(&ctx, 80).await.unwrap_err();
    assert_eq!(e.code, arena_proto::ErrorCode::Internal);
    assert_eq!(crate::volume::level(&ctx), 20);
}

#[tokio::test]
async fn a_new_session_starts_at_the_branch_default_whatever_the_last_player_set() {
    let (_dir, ctx) = bare_ctx();
    let host = Arc::new(FakeHost::default());
    ctx.cleanup.volume.attach(host.clone());
    ctx.queue
        .lock()
        .unwrap()
        .kv_set(KEY_KIOSK_VOLUME_DEFAULT, "35", 4)
        .unwrap();
    crate::volume::set(&ctx, 90).await.unwrap();
    crate::volume::reset_for_session(&ctx);
    for _ in 0..200 {
        if host.volume() == Some(35) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(host.volume(), Some(35));
    assert_eq!(crate::volume::level(&ctx), 35);
}

// ---- Story 6.16: staff messages and the bell ----

fn session_screen() -> SessionScreen {
    SessionScreen {
        session_id: "S1".into(),
        device_name: "PC 1".into(),
        elapsed_ms: 0,
        lock_after_ms: 3_600_000,
        time_up_mode: "clean_up".into(),
        grace_ms: 0,
        ordered_items: 0,
        username: None,
        guest_no: None,
        remaining_ms: None,
        balance_minor: None,
        wallet_left_minor: None,
        plan_name: None,
    }
}

fn message(ctx: &Ctx, cmd: &str, text: &str) {
    let step = crate::pc::apply(ctx, cmd, PcAction::Message, Some(text.into()), None);
    assert_eq!(step, crate::pc::Step::Ack);
}

fn texts(ctx: &Ctx) -> Vec<String> {
    ctx.pc.staff_notes().into_iter().map(|n| n.text).collect()
}

#[tokio::test]
async fn a_message_with_no_session_on_screen_is_acknowledged_but_not_kept() {
    let (_dir, ctx) = bare_ctx();
    let mut live = ctx.pc.notes.subscribe();
    message(&ctx, "M1", "Closing at 11");
    assert!(texts(&ctx).is_empty());
    assert!(live.try_recv().is_err(), "nothing is raised for it");
}

#[tokio::test]
async fn a_message_in_a_session_is_kept_raised_once_and_read_on_request() {
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, session_screen(), true);
    let mut live = ctx.pc.notes.subscribe();
    message(&ctx, "M1", "  Closing at 11 ");
    message(&ctx, "M1", "  Closing at 11 ");
    message(&ctx, "M2", "Your order is ready");
    assert_eq!(texts(&ctx), ["Closing at 11", "Your order is ready"]);
    let first = live.try_recv().unwrap();
    assert_eq!((first.text.as_str(), first.read), ("Closing at 11", false));
    assert_eq!(live.try_recv().unwrap().text, "Your order is ready");
    assert!(live.try_recv().is_err(), "a repeated cmd_id shows once");
    let read = crate::app::handle_request(&ctx, "messages_read", serde_json::Value::Null);
    assert!(read.is_ok());
    assert!(ctx.pc.staff_notes().iter().all(|n| n.read));
}

#[tokio::test]
async fn the_end_of_a_session_clears_its_messages_and_the_next_player_starts_empty() {
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, session_screen(), true);
    message(&ctx, "M1", "one");
    message(&ctx, "M2", "two");
    message(&ctx, "M3", "three");
    assert_eq!(ctx.pc.staff_notes().len(), 3);
    crate::pc::set_locked(&ctx, true, Some(LockReason::PayAtCounter));
    assert!(ctx.pc.staff_notes().is_empty());
    crate::session::sync(&ctx, session_screen(), true);
    assert!(ctx.pc.staff_notes().is_empty());
    message(&ctx, "M4", "welcome");
    assert_eq!(texts(&ctx), ["welcome"]);
}

#[tokio::test]
async fn a_staff_lock_keeps_the_messages_of_the_session() {
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, session_screen(), true);
    message(&ctx, "M1", "one");
    crate::pc::set_locked(&ctx, true, Some(LockReason::LockedByStaff));
    assert_eq!(texts(&ctx), ["one"]);
}

// ---- Story 6.17: the account menu's lock and log out ----

fn player_screen() -> SessionScreen {
    SessionScreen {
        username: Some("asha".into()),
        ..session_screen()
    }
}

#[tokio::test]
async fn only_a_signed_in_players_session_can_be_locked_and_time_and_messages_go_on() {
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, session_screen(), true);
    assert!(crate::player_session::lock(&ctx).is_err(), "a guest");
    assert!(!ctx.pc.screen.borrow().locked);

    crate::session::sync(&ctx, player_screen(), true);
    message(&ctx, "M1", "one");
    crate::player_session::lock(&ctx).unwrap();
    {
        let s = ctx.pc.screen.borrow();
        assert!(s.locked);
        assert_eq!(s.reason, Some(LockReason::LockedByPlayer));
        assert!(s.session.is_some(), "the session is still on screen");
    }
    assert_eq!(texts(&ctx), ["one"], "the messages stay");
    // Already locked: a second lock is refused, and staff Unlock opens the screen.
    assert!(crate::player_session::lock(&ctx).is_err());
    let step = crate::pc::apply(&ctx, "U1", PcAction::Unlock, None, None);
    assert_eq!(step, crate::pc::Step::Ack);
    let s = ctx.pc.screen.borrow();
    assert!(!s.locked && s.reason.is_none() && s.session.is_some());
}

#[tokio::test]
async fn a_password_opens_only_the_players_own_lock_and_needs_the_hub() {
    use arena_kiosk_core::ipc::SecretPin;
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, player_screen(), true);
    // Not locked, or locked by staff: no password is asked of the Hub.
    let e = crate::player_session::unlock(&ctx, SecretPin::new("pw"))
        .await
        .unwrap_err();
    assert_eq!(e.code, arena_proto::ErrorCode::ValidationFailed);
    crate::pc::set_locked(&ctx, true, Some(LockReason::LockedByStaff));
    let e = crate::player_session::unlock(&ctx, SecretPin::new("pw"))
        .await
        .unwrap_err();
    assert_eq!(e.code, arena_proto::ErrorCode::ValidationFailed);
    // The player's own lock with the Hub unreachable stays locked.
    crate::pc::set_locked(&ctx, false, None);
    crate::player_session::lock(&ctx).unwrap();
    let e = crate::player_session::unlock(&ctx, SecretPin::new("pw"))
        .await
        .unwrap_err();
    assert_eq!(e.code, arena_proto::ErrorCode::HubSlow);
    assert!(ctx.pc.screen.borrow().locked);
}

#[tokio::test]
async fn a_player_who_logged_out_is_taken_back_to_attract_with_no_lock() {
    let (_dir, ctx) = bare_ctx();
    crate::session::sync(&ctx, player_screen(), true);
    message(&ctx, "M1", "one");
    crate::pc::set_locked(&ctx, true, Some(LockReason::LoggedOut));
    let s = ctx.pc.screen.borrow();
    assert!(!s.locked, "no lock text: the attract screen shows");
    assert!(s.reason.is_none() && s.session.is_none());
    assert!(ctx.pc.staff_notes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_warning_mark_passes_on_the_kiosk_clock_with_no_hub() {
    let (_dir, ctx) = bare_ctx();
    let mut s = session_screen();
    s.remaining_ms = Some(11 * 60_000);
    s.lock_after_ms = 11 * 60_000;
    crate::session::sync(&ctx, s, true);
    let (stop, stop_rx) = watch::channel(false);
    let worker = tokio::spawn(crate::session::run(Arc::clone(&ctx), stop_rx));
    let mark = || {
        ctx.pc
            .screen
            .borrow()
            .session
            .as_ref()
            .and_then(|v| v.warning_min)
    };
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(mark(), None, "11 min left: no mark yet");
    tokio::time::sleep(Duration::from_secs(40)).await;
    assert_eq!(mark(), Some(10));
    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
    assert_eq!(mark(), Some(5));
    tokio::time::sleep(Duration::from_secs(4 * 60)).await;
    assert_eq!(mark(), Some(1));
    let _ = stop.send(true);
    let _ = worker.await;
}

fn cache_setting(ctx: &Ctx, key: &str, value: &str, rev: i64) {
    ctx.queue
        .lock()
        .unwrap()
        .kv_set(key, value, rev)
        .expect("the setting is cached");
}

#[tokio::test(start_paused = true)]
async fn the_branch_marks_replace_ten_five_and_one_on_the_kiosk_clock() {
    let (_dir, ctx) = bare_ctx();
    cache_setting(&ctx, "billing.warn_marks", "20,3", 1);
    let mut s = session_screen();
    s.remaining_ms = Some(25 * 60_000);
    s.lock_after_ms = 25 * 60_000;
    crate::session::sync(&ctx, s, true);
    let (stop, stop_rx) = watch::channel(false);
    let worker = tokio::spawn(crate::session::run(Arc::clone(&ctx), stop_rx));
    let mark = || {
        ctx.pc
            .screen
            .borrow()
            .session
            .as_ref()
            .and_then(|v| v.warning_min)
    };
    tokio::time::sleep(Duration::from_secs(4 * 60)).await;
    assert_eq!(mark(), None, "21 min left: the 20-min mark is ahead");
    tokio::time::sleep(Duration::from_secs(2 * 60)).await;
    assert_eq!(mark(), Some(20));
    // No 10 or 5 on this branch: the next mark is 3.
    tokio::time::sleep(Duration::from_secs(15 * 60)).await;
    assert_eq!(mark(), Some(20), "4 min left");
    tokio::time::sleep(Duration::from_secs(90)).await;
    assert_eq!(mark(), Some(3));
    let _ = stop.send(true);
    let _ = worker.await;
}

#[tokio::test]
async fn a_wallet_session_at_or_below_the_branch_amount_reports_what_is_left() {
    let (_dir, ctx) = bare_ctx();
    let low = || {
        ctx.pc
            .screen
            .borrow()
            .session
            .as_ref()
            .and_then(|v| v.low_balance_minor)
    };
    let tick = |left: Option<i64>| {
        let mut s = session_screen();
        s.wallet_left_minor = left;
        crate::session::sync(&ctx, s, false);
    };
    crate::session::sync(&ctx, session_screen(), true);
    // The default amount is 50.00 (5000 minor units).
    tick(Some(5_001));
    assert_eq!(low(), None);
    tick(Some(5_000));
    assert_eq!(low(), Some(5_000));
    tick(Some(4_000));
    assert_eq!(low(), Some(4_000));
    // A top-up lifts it above the amount again.
    tick(Some(9_000));
    assert_eq!(low(), None);
    // Not a wallet session: nothing to warn about.
    tick(None);
    assert_eq!(low(), None);
    // The branch's own amount; and 0 turns the warning off.
    cache_setting(&ctx, "billing.low_balance_minor", "10000", 1);
    tick(Some(9_000));
    assert_eq!(low(), Some(9_000));
    cache_setting(&ctx, "billing.low_balance_minor", "0", 2);
    tick(Some(100));
    assert_eq!(low(), None);
}

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
        Pipeline::standard(),
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
        ["programs", "clipboard", "temp_files", "volume"]
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
    assert_eq!(t.host.calls(), ["clipboard", "temp_files", "volume=50"]);
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
    assert_eq!(t.host.calls().len(), 3);
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
    assert_eq!(t.host.calls().len(), 3);
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
        ["clipboard", "temp_files", "temp_files", "volume=50"]
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
        ["clipboard", "temp_files", "temp_files", "volume=50"]
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
    assert_eq!(t.host.calls().len(), 3);
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

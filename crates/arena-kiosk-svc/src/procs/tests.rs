//! The enforcer's rules on the fake process table, driven through `Inner` directly (the
//! service-level flows against a real Hub are in `tests/runtime.rs`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arena_proto::{
    IntentPayload, KEY_PROCESS_ALLOW, KEY_PROCESS_BLOCK, KEY_PROCESS_MODE, KioskLogLevel,
    ProcessBlockReason, ProcessMode, ProcessRule, SettingEntry,
};
use tokio::sync::{Notify, broadcast, mpsc, watch};

use super::fake::{FakeProc, FakeSystem};
use super::*;
use crate::clock::SystemClock;
use crate::paths::DataDir;
use crate::queue::QueueDb;
use crate::runtime::{Ctx, Timings};
use crate::secret::SecretStore;

const PLAYER_SID: &str = "S-1-5-21-1-2-3-1001";
const SYSTEM_SID: &str = "S-1-5-18";
const INSTALL: &str = r"C:\Program Files\Arena 360 Kiosk";
const APP: &str = r"C:\Program Files\Arena 360 Kiosk\arena-kiosk-ui.exe";

struct NoSecret;
impl SecretStore for NoSecret {
    fn save(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn load(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

struct T {
    _dir: tempfile::TempDir,
    ctx: Arc<Ctx>,
    sys: Arc<FakeSystem>,
    inner: Inner,
}

fn ctx_in(dir: &std::path::Path, lists: ProcessLists) -> Arc<Ctx> {
    let data = DataDir(dir.to_path_buf());
    let queue = QueueDb::open(&data.queue()).unwrap();
    let (lists, _) = watch::channel(Arc::new(lists));
    let (toasts, _) = broadcast::channel(16);
    let (hub_state, _) = watch::channel(arena_proto::KioskHubState::Reconnecting);
    Arc::new(Ctx {
        data,
        secrets: Box::new(NoSecret),
        clock: Arc::new(SystemClock),
        timings: Timings::default(),
        queue: Mutex::new(queue),
        queued: Notify::new(),
        hub_state,
        app_state: Mutex::new(None),
        lists,
        toasts,
        launcher: Arc::new(crate::launcher::NoopLauncher),
        admin: crate::admin_exit::AdminState::load(Box::new(NoSecret)),
    })
}

fn enforcing() -> ProcessLists {
    ProcessLists {
        mode: ProcessMode::Enforce,
        ..ProcessLists::defaults()
    }
}

fn setup(lists: ProcessLists) -> T {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_in(dir.path(), lists);
    let sys = Arc::new(FakeSystem::default());
    sys.set_player(crate::config::DEFAULT_PLAYER_USER, PLAYER_SID, 1);
    let inner = Inner::new(
        Arc::clone(&ctx),
        ProcessApi::of(Arc::clone(&sys)),
        EnforceOptions {
            install_dir: Some(INSTALL.into()),
            app_image: Some(APP.into()),
            app_config: None,
            own_images: vec![format!(r"{INSTALL}\arena-kiosk-svc.exe").into()],
            system_root: r"C:\Windows".into(),
        },
    );
    T {
        _dir: dir,
        ctx,
        sys,
        inner,
    }
}

fn player(path: &str, parent: Option<u32>) -> FakeProc {
    FakeProc {
        session_id: 1,
        user_sid: PLAYER_SID.into(),
        path: path.into(),
        parent,
    }
}

fn session() -> PlayerSession {
    PlayerSession {
        sid: PLAYER_SID.into(),
        session_id: 1,
    }
}

impl T {
    fn lists(&self) -> Arc<ProcessLists> {
        Arc::clone(&self.ctx.lists.borrow())
    }

    fn queued(&self) -> Vec<arena_proto::Intent> {
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

    fn blocked(&self) -> Vec<KioskProcessBlocked> {
        self.queued()
            .into_iter()
            .filter_map(|i| match i.payload {
                IntentPayload::KioskProcessBlocked(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    fn logs(&self, code: &str) -> Vec<arena_proto::KioskLog> {
        self.queued()
            .into_iter()
            .filter_map(|i| match i.payload {
                IntentPayload::KioskLog(l) if l.code == code => Some(l),
                _ => None,
            })
            .collect()
    }

    /// One creation event for `pid`, checked as the event path does.
    fn event(&self, pid: u32, parent: Option<u32>) -> Option<Hit> {
        self.inner.note_parent(pid, parent);
        let ev = ProcStart {
            pid,
            parent_pid: parent,
            at: Instant::now(),
        };
        self.inner.check(pid, &session(), &self.lists(), Some(&ev))
    }
}

#[test]
fn report_mode_never_terminates_and_reports_each_process_once() {
    let t = setup(ProcessLists::defaults());
    assert_eq!(
        t.lists().mode,
        ProcessMode::Report,
        "no mode from the hub: report only"
    );
    t.sys
        .add_running(10, player(r"C:\Windows\System32\cmd.exe", None));
    t.sys.add_running(11, player(r"C:\Games\x.exe", None));
    let hits = t.inner.sweep();
    assert_eq!(hits.len(), 2);
    assert!(
        hits.iter()
            .all(|h| h.mode == ProcessMode::Report && !h.toast)
    );
    for h in &hits {
        t.inner.report(h);
    }
    // And through the event path.
    t.sys.spawn(12, player(r"C:\Temp\y.exe", Some(10)));
    let ev = t.event(12, Some(10)).unwrap();
    assert_eq!(ev.mode, ProcessMode::Report);
    assert!(!ev.toast);
    t.inner.report(&ev);
    assert_eq!(t.sys.kill_attempts(), 0);
    assert!(t.sys.alive(10) && t.sys.alive(11) && t.sys.alive(12));
    let mut toasts = t.ctx.toasts.subscribe();
    // Later sweeps report nothing new for processes already reported.
    assert!(t.inner.sweep().is_empty());
    assert!(toasts.try_recv().is_err());
    let blocked = t.blocked();
    assert_eq!(blocked.len(), 3);
    assert!(blocked.iter().all(|b| b.mode == ProcessMode::Report));
    assert_eq!(blocked[0].reason, ProcessBlockReason::Blocked);
    // Switched to enforce, the same processes are killed.
    t.ctx.lists.send_replace(Arc::new(enforcing()));
    let mut killed: Vec<u32> = t.inner.sweep().iter().map(|h| h.pid).collect();
    killed.sort_unstable();
    assert_eq!(killed, [10, 11, 12]);
}

#[test]
fn only_the_kiosk_apps_webview2_descendants_are_exempt() {
    let t = setup(enforcing());
    // The app (started by the service, as SYSTEM), WebView2 at an unexpected unsigned path,
    // its renderer, and an unrelated WebView2 at the same path.
    t.sys.add_running(
        1,
        FakeProc {
            user_sid: SYSTEM_SID.into(),
            ..player(r"C:\Windows\System32\services.exe", None)
        },
    );
    t.sys.add_running(10, player(APP, Some(1)));
    t.sys
        .add_running(11, player(r"D:\odd\msedgewebview2.exe", Some(10)));
    t.sys
        .add_running(12, player(r"D:\odd\msedgewebview2.exe", Some(11)));
    t.sys
        .add_running(13, player(r"D:\odd\msedgewebview2.exe", Some(1)));
    // Other programs the app (the player's shell) or its WebView2 starts are decided normally.
    t.sys
        .add_running(15, player(r"C:\Windows\System32\cmd.exe", Some(10)));
    t.sys
        .add_running(16, player(r"C:\Temp\helper.exe", Some(11)));
    let mut killed: Vec<u32> = t.inner.sweep().iter().map(|h| h.pid).collect();
    killed.sort_unstable();
    assert_eq!(killed, [13, 15, 16]);
    // Through creation events too: a WebView2 child is exempt (any case), cmd.exe is blocked.
    t.sys
        .spawn(14, player(r"D:\odd\MSEdgeWebView2.EXE", Some(12)));
    assert_eq!(t.event(14, Some(12)), None);
    assert!(t.sys.alive(14));
    t.sys
        .spawn(17, player(r"C:\Windows\System32\cmd.exe", Some(10)));
    let hit = t.event(17, Some(10)).unwrap();
    assert_eq!(hit.reason, ProcessBlockReason::Blocked);
    assert!(!t.sys.alive(17));
    // Once the app is gone, its WebView2 orphans are no longer exempt.
    t.sys
        .add_running(10, player(r"C:\Temp\impostor.exe", Some(1)));
    let mut killed: Vec<u32> = t.inner.sweep().iter().map(|h| h.pid).collect();
    killed.sort_unstable();
    assert_eq!(killed, [10, 11, 12, 14]);
}

#[test]
fn a_signer_scoped_block_rule_goes_through_the_signature() {
    let rule = |signer: Option<&str>| ProcessRule {
        path_glob: r"C:\Games\*.exe".into(),
        signer: signer.map(str::to_owned),
    };
    let t = setup(ProcessLists {
        allow: vec![rule(None)],
        block: vec![rule(Some("Bad Corp"))],
        mode: ProcessMode::Enforce,
    });
    t.sys.sign(r"C:\Games\bad.exe", "Bad Corp", true);
    t.sys.sign(r"C:\Games\good.exe", "Good Corp", true);
    t.sys.sign(r"C:\Games\forged.exe", "Bad Corp", false);
    let lists = t.lists();
    let d = |p: &str| t.inner.decision(p, &lists);
    assert_eq!(
        d(r"C:\Games\bad.exe"),
        Decision::Kill(ProcessBlockReason::Blocked)
    );
    assert_eq!(d(r"C:\Games\good.exe"), Decision::Allow);
    assert_eq!(
        d(r"C:\Games\forged.exe"),
        Decision::Allow,
        "an invalid signature is no signer"
    );
    assert_eq!(d(r"C:\Games\unsigned.exe"), Decision::Allow);
    assert_eq!(t.sys.verifies(), 4);
    // Cached: deciding again verifies nothing.
    assert_eq!(
        d(r"C:\Games\bad.exe"),
        Decision::Kill(ProcessBlockReason::Blocked)
    );
    assert_eq!(t.sys.verifies(), 4);
    // Without a signer-scoped block rule, an unsigned allow match needs no signature at all.
    let plain = ProcessLists {
        block: Vec::new(),
        ..(*lists).clone()
    };
    assert_eq!(
        t.inner.decision(r"C:\Games\new.exe", &plain),
        Decision::Allow
    );
    assert_eq!(t.sys.verifies(), 4);
}

#[test]
fn the_system_placeholder_expands_from_systemroot() {
    let t = setup(enforcing());
    t.sys.sign(
        r"C:\Windows\System32\conhost.exe",
        "Microsoft Windows",
        true,
    );
    let lists = t.lists();
    assert_eq!(
        t.inner.decision(r"C:\Windows\System32\conhost.exe", &lists),
        Decision::Allow
    );
}

#[test]
fn signatures_are_verified_once_per_key_under_concurrency() {
    let t = Arc::new(setup(enforcing()));
    t.sys.slow_verify(Duration::from_millis(150));
    let path = r"C:\Windows\System32\conhost.exe";
    t.sys.sign(path, "Microsoft Windows", true);
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let t = Arc::clone(&t);
            std::thread::spawn(move || t.inner.signature(path))
        })
        .collect();
    for th in threads {
        assert!(th.join().unwrap().valid);
    }
    assert_eq!(t.sys.verifies(), 1);
}

#[test]
fn an_older_rev_or_an_invalid_value_keeps_the_newer_list_in_ctx_and_kv() {
    let t = setup(enforcing());
    let games = r#"[{"path_glob": "C:\\Games\\*.exe"}]"#;
    let entry = |key: &str, value: &str, rev: i64| SettingEntry {
        key: key.into(),
        value: value.into(),
        rev,
    };
    let mut revs = AppliedRevs::default();
    apply_settings(&t.ctx, &[entry(KEY_PROCESS_ALLOW, games, 5)], &mut revs);
    assert_eq!(t.lists().allow.len(), 1);
    let kv = || {
        t.ctx
            .queue
            .lock()
            .unwrap()
            .kv_get(KEY_PROCESS_ALLOW)
            .unwrap()
    };
    assert_eq!(kv(), Some((games.to_owned(), 5)));
    // Older: ignored, on this connection and on the next (revs seeded from the cache).
    let old = r#"[{"path_glob": "C:\\Old\\*.exe"}]"#;
    apply_settings(&t.ctx, &[entry(KEY_PROCESS_ALLOW, old, 4)], &mut revs);
    let mut next = AppliedRevs::from_cache(&t.ctx.queue.lock().unwrap());
    assert_eq!(next.get(KEY_PROCESS_ALLOW), Some(5));
    apply_settings(&t.ctx, &[entry(KEY_PROCESS_ALLOW, old, 3)], &mut next);
    assert_eq!(t.lists().allow[0].path_glob, r"C:\Games\*.exe");
    assert_eq!(kv(), Some((games.to_owned(), 5)));
    // Invalid (an empty allow-list, a catch-all block rule, a bad mode): refused, kept.
    apply_settings(
        &t.ctx,
        &[
            entry(KEY_PROCESS_ALLOW, "[]", 6),
            entry(KEY_PROCESS_BLOCK, r#"[{"path_glob": "*.exe"}]"#, 6),
            entry(KEY_PROCESS_MODE, "off", 6),
        ],
        &mut next,
    );
    assert_eq!(t.lists().allow[0].path_glob, r"C:\Games\*.exe");
    assert_eq!(t.lists().block, ProcessLists::defaults().block);
    assert_eq!(t.lists().mode, ProcessMode::Enforce);
    assert_eq!(kv(), Some((games.to_owned(), 5)));
    assert_eq!(
        t.ctx
            .queue
            .lock()
            .unwrap()
            .kv_get(KEY_PROCESS_MODE)
            .unwrap(),
        None
    );
    // The mode applies and is cached like the lists.
    apply_settings(&t.ctx, &[entry(KEY_PROCESS_MODE, "report", 2)], &mut next);
    assert_eq!(t.lists().mode, ProcessMode::Report);
    assert_eq!(
        t.ctx
            .queue
            .lock()
            .unwrap()
            .kv_get(KEY_PROCESS_MODE)
            .unwrap(),
        Some(("report".to_owned(), 2))
    );
}

#[test]
fn a_corrupt_cache_falls_back_to_the_defaults_and_report_mode() {
    let dir = tempfile::tempdir().unwrap();
    let q = QueueDb::open(&dir.path().join("queue.db")).unwrap();
    assert_eq!(ProcessLists::load(&q), ProcessLists::defaults());
    q.kv_set(KEY_PROCESS_ALLOW, "{not json", 3).unwrap();
    q.kv_set(KEY_PROCESS_BLOCK, r#"[{"path_glob": "*"}]"#, 3)
        .unwrap();
    q.kv_set(KEY_PROCESS_MODE, "sometimes", 3).unwrap();
    let loaded = ProcessLists::load(&q);
    assert_eq!(loaded, ProcessLists::defaults());
    assert_eq!(loaded.mode, ProcessMode::Report);
    q.kv_set(KEY_PROCESS_MODE, "enforce", 4).unwrap();
    assert_eq!(ProcessLists::load(&q).mode, ProcessMode::Enforce);
}

#[test]
fn reports_are_capped_per_minute_with_one_summary() {
    let t = setup(enforcing());
    for i in 0..35u32 {
        t.inner.report(&Hit {
            pid: 100 + i,
            path: format!(r"C:\Games\g{i}.exe"),
            reason: ProcessBlockReason::NotAllowed,
            mode: ProcessMode::Enforce,
            toast: false,
        });
    }
    assert_eq!(t.blocked().len(), REPORT_CAP as usize);
    t.inner.flush_summary();
    assert!(
        t.logs(SUMMARY_CODE).is_empty(),
        "not before the minute is over"
    );
    // The minute passes.
    {
        let mut r = lock(&t.inner.reported);
        r.cap.window = r.cap.window.and_then(|w| w.checked_sub(REPORT_WINDOW));
    }
    t.inner.flush_summary();
    t.inner.flush_summary();
    let summary = t.logs(SUMMARY_CODE);
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].level, KioskLogLevel::Warn);
    assert_eq!(summary[0].detail, "5 more");
}

#[test]
fn the_report_cap_counts_by_window() {
    let mut cap = ReportCap::default();
    let t0 = Instant::now();
    for _ in 0..REPORT_CAP {
        assert_eq!(cap.admit(t0), (true, None));
    }
    assert_eq!(cap.admit(t0 + Duration::from_secs(30)), (false, None));
    assert_eq!(cap.admit(t0 + Duration::from_secs(59)), (false, None));
    assert_eq!(cap.admit(t0 + REPORT_WINDOW), (true, Some(2)));
    assert_eq!(cap.roll(t0 + REPORT_WINDOW * 3), None, "nothing held back");
}

#[test]
fn a_long_path_is_reported_shortened_from_the_left() {
    let t = setup(enforcing());
    let path = format!(r"C:\{}\game.exe", "d".repeat(2000));
    t.inner.report(&Hit {
        pid: 1,
        path,
        reason: ProcessBlockReason::NotAllowed,
        mode: ProcessMode::Enforce,
        toast: false,
    });
    let b = &t.blocked()[0];
    b.validate().unwrap();
    assert!(b.path.starts_with('…') && b.path.ends_with(r"\game.exe"));
    let detail = b.detail();
    assert_eq!(
        detail.chars().count(),
        arena_proto::MAX_KIOSK_LOG_DETAIL_CHARS
    );
    assert!(
        detail.ends_with(&b.path),
        "the stored detail keeps the reported path whole"
    );
}

#[test]
fn unreadable_processes_are_left_alone_and_warned_once() {
    let t = setup(enforcing());
    t.sys
        .add_running(20, player(r"C:\Windows\System32\cmd.exe", None));
    t.sys.make_unreadable(20, Some(1), "token");
    assert!(t.inner.sweep().is_empty());
    assert!(t.inner.sweep().is_empty());
    assert!(t.sys.alive(20));
    assert_eq!(t.sys.kill_attempts(), 0);
    assert_eq!(lock(&t.inner.warned_unreadable).0.len(), 1);
    // Another session's unreadable process is not even warned about.
    t.sys.add_running(21, player(r"C:\x.exe", None));
    t.sys.make_unreadable(21, Some(0), "path");
    t.inner.sweep();
    assert_eq!(lock(&t.inner.warned_unreadable).0.len(), 1);
}

#[test]
fn a_failed_kill_is_warned_and_reported_once() {
    let t = setup(enforcing());
    t.sys
        .add_running(30, player(r"C:\Windows\System32\cmd.exe", None));
    t.sys.fail_kill(30);
    assert!(t.inner.sweep().is_empty());
    assert!(t.inner.sweep().is_empty());
    assert_eq!(t.sys.kill_attempts(), 2, "still retried");
    let failed = t.logs(KILL_FAILED_CODE);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level, KioskLogLevel::Error);
    assert!(failed[0].detail.contains("cmd.exe"));
    assert!(
        t.blocked().is_empty(),
        "nothing was killed, nothing is reported as blocked"
    );
}

#[test]
fn toasts_only_for_visible_or_player_started_launches() {
    let t = setup(enforcing());
    t.sys.add_running(
        2,
        FakeProc {
            user_sid: SYSTEM_SID.into(),
            ..player(r"C:\Windows\System32\svchost.exe", None)
        },
    );
    t.sys
        .add_running(3, player(r"C:\Program Files\Steam\steam.exe", None));
    // Started by the player's session: toast.
    t.sys
        .spawn(40, player(r"C:\Windows\System32\cmd.exe", Some(3)));
    assert!(t.event(40, Some(3)).unwrap().toast);
    // Started by a system service in the background: no toast.
    t.sys
        .spawn(41, player(r"C:\Windows\System32\cmd.exe", Some(2)));
    assert!(!t.event(41, Some(2)).unwrap().toast);
    // Unless it put up a window.
    t.sys
        .spawn(42, player(r"C:\Windows\System32\cmd.exe", Some(2)));
    t.sys.show_window(42);
    assert!(t.event(42, Some(2)).unwrap().toast);
    // A sweep never toasts.
    t.sys
        .add_running(43, player(r"C:\Windows\System32\cmd.exe", Some(3)));
    t.sys.show_window(43);
    let hits = t.inner.sweep();
    assert_eq!(hits.len(), 2, "{hits:?}"); // steam.exe (not allowed) and 43
    assert!(hits.iter().all(|h| !h.toast));
    let mut toasts = t.ctx.toasts.subscribe();
    for h in &hits {
        t.inner.report(h);
    }
    assert!(toasts.try_recv().is_err());
}

#[test]
fn a_full_event_queue_asks_for_a_sweep() {
    let (tx, mut rx) = mpsc::channel(2);
    let sweep = Arc::new(Notify::new());
    let sink = EventSink::new(tx, Arc::clone(&sweep));
    let ev = |pid| ProcStart {
        pid,
        parent_pid: None,
        at: Instant::now(),
    };
    assert!(sink.send(ev(1)) && sink.send(ev(2)));
    assert!(sink.send(ev(3)), "full: dropped, but the watch goes on");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(1), sweep.notified())
            .await
            .expect("a sweep was requested");
    });
    assert_eq!(rx.try_recv().unwrap().pid, 1);
    assert_eq!(rx.try_recv().unwrap().pid, 2);
    assert!(rx.try_recv().is_err());
    drop(rx);
    assert!(!sink.send(ev(4)));
    assert!(sink.is_closed());
}

#[test]
fn the_app_path_is_re_read_from_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_in(dir.path(), enforcing());
    let sys = Arc::new(FakeSystem::default());
    sys.set_player(crate::config::DEFAULT_PLAYER_USER, PLAYER_SID, 1);
    let config = dir.path().join("config.json");
    let write = |app: &str| {
        crate::config::KioskConfig {
            hub_addr: "127.0.0.1:7400".into(),
            hub_fingerprint: "a".repeat(64),
            device_id: "01JDEVICEAAAAAAAAAAAAAAAAA".into(),
            player_user: crate::config::DEFAULT_PLAYER_USER.into(),
            app_path: Some(app.into()),
        }
        .save(&config)
        .unwrap();
    };
    write(r"C:\Kiosk\old.exe");
    let inner = Inner::new(
        Arc::clone(&ctx),
        ProcessApi::of(Arc::clone(&sys)),
        EnforceOptions {
            app_config: Some(config.clone()),
            system_root: r"C:\Windows".into(),
            ..EnforceOptions::default()
        },
    );
    sys.add_running(50, player(r"C:\Kiosk\new.exe", None));
    write(r"C:\Kiosk\new.exe");
    assert!(
        inner.sweep().is_empty(),
        "the newly configured app is never killed"
    );
    assert!(sys.alive(50));
}

// ---- Story 1.20: the admin exit suspends enforcement, and its end closes Explorer ----

#[test]
fn an_admin_exit_suspends_enforcement_and_its_end_closes_explorer() {
    let t = setup(enforcing());
    t.sys
        .add_running(50, player(r"C:\Windows\explorer.exe", None));
    t.sys
        .add_running(51, player(r"C:\Windows\System32\cmd.exe", Some(50)));
    t.ctx
        .admin
        .active
        .send_replace(Some(arena_kiosk_core::admin_exit::ActiveExit::start(
            "E", 0,
        )));
    assert!(
        t.inner.sweep().is_empty(),
        "nothing is killed during an exit"
    );
    assert!(t.sys.killed().is_empty());
    t.ctx.admin.active.send_replace(None);
    // The end: Explorer closed (whatever the mode), then the sweep takes the rest.
    assert_eq!(t.inner.close_named(EXPLORER_IMAGE), 1);
    assert_eq!(t.sys.killed(), vec![50]);
    let hits = t.inner.sweep();
    assert_eq!(hits.iter().map(|h| h.pid).collect::<Vec<_>>(), vec![51]);
}

#[test]
fn closing_explorer_never_touches_other_sessions_or_report_mode_rules() {
    let t = setup(ProcessLists::defaults());
    t.sys.add_running(
        60,
        FakeProc {
            session_id: 0,
            user_sid: "S-1-5-18".into(),
            path: r"C:\Windows\explorer.exe".into(),
            parent: None,
        },
    );
    t.sys
        .add_running(61, player(r"C:\Windows\explorer.exe", None));
    assert_eq!(
        t.inner.close_named(EXPLORER_IMAGE),
        1,
        "report mode still closes it"
    );
    assert_eq!(t.sys.killed(), vec![61]);
}

//! The Games tab on the fake host (the round trip through the app's IPC is in the Playwright
//! suite, the real Hub's side in `arena-core`).

use std::sync::{Arc, Mutex};

use arena_proto::{
    ErrorCode, IntentKind, IntentPayload, KioskInstall, KioskLibraryEntry, LockReason, ProcessRule,
    library_rules,
};
use serde_json::json;
use tokio::sync::{Notify, broadcast, watch};

use super::fake::FakeGames;
use super::*;
use crate::clock::ManualClock;
use crate::paths::DataDir;
use crate::pc::{Screen, ScreenSession};
use crate::queue::QueueDb;
use crate::runtime::{Ctx, Timings};
use crate::secret::SecretStore;

pub(crate) const ME: &str = "DEVICE-ME";
/// The wall clock the tests start at.
const NOW_MS: i64 = 1_800_000_000_000;

struct NoSecret;
impl SecretStore for NoSecret {
    fn save(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn load(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

pub(crate) fn entry(id: &str, name: &str, exe: &str) -> KioskLibraryEntry {
    KioskLibraryEntry {
        id: id.into(),
        kind: "game".into(),
        name: name.into(),
        launcher: "none".into(),
        executable_path: exe.into(),
        launch_uri: String::new(),
        launch_args: String::new(),
        category: String::new(),
        tags: vec![],
        age_rating: String::new(),
        catalog_id: None,
        installs: vec![KioskInstall {
            device_id: ME.into(),
            path: exe.into(),
        }],
    }
}

fn session() -> ScreenSession {
    let now = tokio::time::Instant::now();
    ScreenSession {
        device_name: "PC 1".into(),
        started: now,
        warning_min: None,
        lock_at: now + std::time::Duration::from_secs(3600),
        grace_ms: 0,
        ordered_items: 0,
        unserved_items: 0,
        username: Some("kai".into()),
        guest_no: None,
        ends_at: None,
        balance_minor: None,
        plan_name: None,
        low_balance_minor: None,
        hold: false,
    }
}

pub(crate) struct T {
    _dir: tempfile::TempDir,
    pub(crate) ctx: Arc<Ctx>,
    pub(crate) host: Arc<FakeGames>,
    /// The wall clock the service reads (it moves only when a test advances it).
    pub(crate) clock: Arc<ManualClock>,
}

/// A service with a session on screen, a host and every library path allowed.
pub(crate) fn t(entries: &[KioskLibraryEntry]) -> T {
    let (dir, ctx, clock) = bare(entries);
    let host = Arc::new(FakeGames::default());
    ctx.games.set_host(host.clone());
    T {
        _dir: dir,
        ctx,
        host,
        clock,
    }
}

/// [`t`] without a game host.
fn bare(entries: &[KioskLibraryEntry]) -> (tempfile::TempDir, Arc<Ctx>, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(NOW_MS));
    let dir = tempfile::tempdir().unwrap();
    let data = DataDir(dir.path().to_path_buf());
    let queue = QueueDb::open(&data.queue()).unwrap();
    let pc = crate::pc::PcState::load(&queue);
    let cleanup = crate::cleanup::CleanupState::load(&queue);
    let mut lists = crate::procs::ProcessLists::defaults();
    for e in entries {
        lists
            .allow
            .extend(library_rules(&e.executable_path, &e.launcher));
    }
    let (toasts, _) = broadcast::channel(16);
    let (hub_state, _) = watch::channel(arena_proto::KioskHubState::Reconnecting);
    let ctx = Arc::new(Ctx {
        data,
        secrets: Box::new(NoSecret),
        clock: clock.clone(),
        timings: Timings::default(),
        queue: Mutex::new(queue),
        queued: Notify::new(),
        hub_state,
        app_state: Mutex::new(None),
        lists: watch::channel(Arc::new(lists)).0,
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
        freezer: Mutex::new(None),
        cleanup,
        games: GameState::default(),
    });
    ctx.pc.screen.send_replace(Screen {
        locked: false,
        reason: None,
        session: Some(session()),
    });
    (dir, ctx, clock)
}

/// The kinds of the intents waiting for the Hub, oldest first.
pub(crate) fn queued(t: &T) -> Vec<(IntentKind, Option<IntentPayload>)> {
    let queue = t.ctx.queue.lock().unwrap();
    queue
        .entries()
        .unwrap()
        .into_iter()
        .map(|q| (q.intent.kind, Some(q.intent.payload)))
        .collect()
}

/// Waits until `n` intents wait for the Hub (the launch record follows the game's first window).
async fn wait_queued(t: &T, n: usize) {
    for _ in 0..300 {
        if queued(t).len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{} intents queued, wanted {n}", queued(t).len());
}

async fn go(t: &T, entries: &[KioskLibraryEntry], id: &str) -> serde_json::Value {
    launch_in(&t.ctx, ME, entries.to_vec(), id.into())
        .await
        .unwrap()
}

#[test]
fn the_tab_lists_games_only_sorted_by_name_with_their_running_flag() {
    let mut app = entry("APP", "Zed Editor", r"C:\Apps\zed.exe");
    app.kind = "app".into();
    let entries = vec![
        entry("B", "valorant", r"C:\G\v.exe"),
        app,
        entry("A", "Apex", r"C:\G\a.exe"),
        entry("C", "Apex", r"C:\G\a2.exe"),
    ];
    let cards = cards(entries, "game", |id| id == "B");
    let names: Vec<_> = cards
        .iter()
        .map(|c| (c.name.as_str(), c.id.as_str(), c.running))
        .collect();
    assert_eq!(
        names,
        [
            ("Apex", "A", false),
            ("Apex", "C", false),
            ("valorant", "B", true)
        ]
    );
}

#[test]
fn a_card_carries_the_category_and_tags_the_filter_rail_lists() {
    let mut e = entry("A", "Apex", r"C:\G\a.exe");
    e.category = "Shooter".into();
    e.tags = vec!["fps".into(), "co-op".into()];
    let cards = cards(vec![e], "game", |_| false);
    assert_eq!(cards[0].category, "Shooter");
    assert_eq!(cards[0].tags, ["fps", "co-op"]);
    let body = serde_json::to_value(&cards[0]).unwrap();
    assert_eq!(body["category"], "Shooter");
    assert_eq!(body["tags"], serde_json::json!(["fps", "co-op"]));
}

#[test]
fn a_card_carries_the_launcher_and_age_rating_the_game_details_show() {
    let mut e = entry("A", "Apex", r"C:Ga.exe");
    e.launcher = "epic".into();
    e.age_rating = "16+".into();
    let body = serde_json::to_value(&cards(vec![e], "game", |_| false)[0]).unwrap();
    assert_eq!(body["launcher"], "epic");
    assert_eq!(body["age_rating"], "16+");
}

#[test]
fn a_game_starts_from_its_install_here_else_its_path_else_its_link() {
    let mut e = entry("G", "G", r"C:\Games\elsewhere.exe");
    e.installs = vec![
        KioskInstall {
            device_id: "OTHER".into(),
            path: r"D:\other.exe".into(),
        },
        KioskInstall {
            device_id: ME.into(),
            path: r"E:\mine.exe".into(),
        },
    ];
    e.launch_args = "-windowed".into();
    let spec = spec_of(&e, ME, |_| false).unwrap();
    assert_eq!(spec.exe.as_deref(), Some(r"E:\mine.exe"));
    assert_eq!(spec.args, "-windowed");

    // No install here: the configured path, when it exists on this PC.
    e.installs.clear();
    assert_eq!(spec_of(&e, ME, |_| false), None);
    let spec = spec_of(&e, ME, |p| p == r"C:\Games\elsewhere.exe").unwrap();
    assert_eq!(spec.exe.as_deref(), Some(r"C:\Games\elsewhere.exe"));

    // Neither: the link.
    e.executable_path.clear();
    e.launch_uri = "steam://rungameid/730".into();
    let spec = spec_of(&e, ME, |_| false).unwrap();
    assert_eq!(spec.uri.as_deref(), Some("steam://rungameid/730"));
    assert_eq!(spec.exe, None);

    // A link that is not one, or that could break out of a quoted argument, never starts.
    for bad in ["notalink", "steam://x\"y", "steam://\u{7}"] {
        e.launch_uri = bad.into();
        assert_eq!(spec_of(&e, ME, |_| false), None, "{bad:?}");
    }
}

#[test]
fn the_allow_list_decides_before_anything_starts() {
    let e = entry("G", "G", r"C:\Games\g.exe");
    let spec = spec_of(&e, ME, |_| false).unwrap();
    let mut lists = crate::procs::ProcessLists::defaults();
    assert!(!allowed(&lists, &e, &spec), "the defaults allow no game");
    lists
        .allow
        .extend(library_rules(&e.executable_path, "none"));
    assert!(allowed(&lists, &e, &spec));
    // A block rule wins over the allow rule.
    lists.block.push(ProcessRule {
        path_glob: r"C:\Games\g.exe".into(),
        signer: None,
    });
    assert!(!allowed(&lists, &e, &spec));

    // A link starts the launcher, so the launcher's install rules must be allowed.
    let mut steam = entry("S", "S", "");
    steam.launcher = "steam".into();
    steam.installs.clear();
    steam.launch_uri = "steam://rungameid/730".into();
    let spec = spec_of(&steam, ME, |_| false).unwrap();
    let mut lists = crate::procs::ProcessLists::defaults();
    assert!(!allowed(&lists, &steam, &spec));
    lists.allow.extend(library_rules("", "steam"));
    assert!(allowed(&lists, &steam, &spec));
    steam.launcher = "none".into();
    assert!(!allowed(&lists, &steam, &spec), "no launcher, no link");
}

#[tokio::test]
async fn a_launch_starts_the_game_and_queues_game_launched_for_the_hub() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    assert!(!t.ctx.games.any_running());

    let out = go(&t, &entries, "G1").await;
    assert_eq!(out, json!({"outcome": "started"}));
    assert_eq!(t.host.started().len(), 1);
    assert_eq!(t.host.started()[0].exe.as_deref(), Some(r"C:\Games\v.exe"));
    assert!(t.ctx.games.any_running());
    t.host.show_window(t.host.alive_pids()[0]);
    wait_queued(&t, 1).await;

    let q = queued(&t);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, IntentKind::KioskGameLaunched);
    let Some(IntentPayload::KioskGameLaunched(p)) = &q[0].1 else {
        panic!("a game.launched payload");
    };
    assert_eq!((p.game_id.as_str(), p.name.as_str()), ("G1", "Valorant"));
    p.validate().unwrap();
    assert!(
        p.launch_ms.is_some_and(|ms| ms < 5_000),
        "{:?}",
        p.launch_ms
    );
}

#[tokio::test]
async fn a_second_launch_brings_the_running_game_forward_instead_of_starting_it_again() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    let pid = t.host.alive_pids()[0];
    t.host.show_window(pid);
    wait_queued(&t, 1).await;

    let out = go(&t, &entries, "G1").await;
    assert_eq!(out, json!({"outcome": "already_running", "pid": pid}));
    assert_eq!(t.host.started().len(), 1, "no second copy");
    assert_eq!(queued(&t).len(), 1, "no second game.launched");

    let list = list_of(&t.ctx, entries.clone());
    assert_eq!(list["running"], json!(true));
    assert_eq!(list["games"][0]["running"], json!(true));

    // Once it exits the signal drops and the next launch starts it again.
    t.host.exit(pid);
    let list = list_of(&t.ctx, entries.clone());
    assert_eq!(list["running"], json!(false));
    assert_eq!(list["games"][0]["running"], json!(false));
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert_eq!(t.host.started().len(), 2);
}

#[tokio::test]
async fn the_running_signal_stays_on_until_the_last_game_exits() {
    let entries = vec![
        entry("G1", "One", r"C:\Games\1.exe"),
        entry("G2", "Two", r"C:\Games\2.exe"),
    ];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    go(&t, &entries, "G2").await;
    let pids = t.host.alive_pids();
    assert_eq!(pids.len(), 2);
    t.host.exit(pids[0]);
    assert!(t.ctx.games.any_running());
    t.host.exit(pids[1]);
    assert!(!t.ctx.games.any_running());
}

#[tokio::test]
async fn a_game_that_is_not_in_the_library_or_not_allowed_or_will_not_start_is_logged_and_failed() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);

    // Not in this PC's library.
    assert_eq!(go(&t, &entries, "NOPE").await, json!({"outcome": "failed"}));
    // An app is not a game.
    let mut app = entry("APP", "Editor", r"C:\Apps\e.exe");
    app.kind = "app".into();
    let with_app = vec![entries[0].clone(), app];
    assert_eq!(go(&t, &with_app, "APP").await["outcome"], "failed");
    // Not allowed: the library entry's rule is not in the allow-list.
    let other = vec![entry("G2", "Other", r"C:\Games\other.exe")];
    assert_eq!(go(&t, &other, "G2").await["outcome"], "failed");
    // The executable fails to start.
    t.host
        .fail_with(Some("CreateProcessAsUserW: file not found"));
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "failed");

    assert!(t.host.started().is_empty());
    assert!(!t.ctx.games.any_running());
    let q = queued(&t);
    assert_eq!(q.len(), 4, "one kiosk.log each, no game.launched");
    let details: Vec<String> = q
        .into_iter()
        .map(|(kind, payload)| {
            assert_eq!(kind, IntentKind::KioskLog);
            let Some(IntentPayload::KioskLog(log)) = payload else {
                panic!("a kiosk.log payload");
            };
            assert_eq!(log.code, FAILED_CODE);
            assert_eq!(log.level, arena_proto::KioskLogLevel::Error);
            log.validate().unwrap();
            log.detail
        })
        .collect();
    assert_eq!(details[0], "NOPE: not in this PC's library");
    assert_eq!(details[1], "APP: not in this PC's library");
    assert_eq!(details[2], "Other: not allowed on this PC");
    assert_eq!(details[3], "Valorant: CreateProcessAsUserW: file not found");
}

#[tokio::test]
async fn nothing_starts_without_a_session_on_screen_or_while_locked() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    t.ctx.pc.screen.send_modify(|s| {
        s.locked = true;
        s.reason = Some(LockReason::TimesUp);
    });
    let err = launch(&t.ctx, json!({"id": "G1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(
        list(&t.ctx).unwrap(),
        json!({"games": [], "running": false})
    );

    t.ctx.pc.screen.send_replace(Screen::default());
    let err = launch(&t.ctx, json!({"id": "G1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(t.host.started().is_empty());
}

#[tokio::test]
async fn a_request_without_an_id_or_a_host_is_refused() {
    let t = t(&[]);
    for body in [json!({}), json!({"id": ""}), json!({"id": 5})] {
        let err = launch(&t.ctx, body).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadFrame);
    }
    // A service without a game host (a non-Windows console run) cannot start games.
    let (_dir, ctx, _) = bare(&[]);
    let err = launch(&ctx, json!({"id": "G1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
}

// ---- Story 6.25: the booster and the launch time ----

use arena_proto::BoosterPriority;

use crate::procs::{CloseReport, CloseScope};

fn set_setting(t: &T, key: &str, value: &str) {
    t.ctx.queue.lock().unwrap().kv_set(key, value, 1).unwrap();
}

/// The launch times of the `game.launched` records waiting for the Hub, oldest first.
fn launches(t: &T) -> Vec<Option<u32>> {
    queued(t)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Some(IntentPayload::KioskGameLaunched(g)) => Some(g.launch_ms),
            _ => None,
        })
        .collect()
}

/// The codes of the `kiosk.log` events waiting for the Hub, oldest first.
fn log_codes(t: &T) -> Vec<String> {
    queued(t)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Some(IntentPayload::KioskLog(l)) => Some(l.code),
            _ => None,
        })
        .collect()
}

/// Makes the process enforcer's closer record the image names it is asked to close, and how
/// many games had started when it was asked.
/// The image names a closer was asked for, and how many games had started by then.
type Closed = Arc<Mutex<Vec<(Vec<String>, usize)>>>;

fn recording_closer(t: &T, failed: usize) -> Closed {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (log, host) = (Arc::clone(&seen), Arc::clone(&t.host));
    *t.ctx.closer.lock().unwrap() = Some(Arc::new(move |scope| {
        let CloseScope::Images(names) = scope else {
            panic!("the booster closes by image name");
        };
        log.lock().unwrap().push((
            names.iter().map(|n| (*n).to_owned()).collect(),
            host.started().len(),
        ));
        Ok(CloseReport { closed: 1, failed })
    }));
    seen
}

#[tokio::test]
async fn the_listed_programs_close_before_the_game_starts_and_a_stubborn_one_does_not_stop_it() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    set_setting(&t, "kiosk.booster_close", "discord.exe, OneDrive.exe");
    let seen = recording_closer(&t, 0);
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert_eq!(
        *seen.lock().unwrap(),
        [(vec!["discord.exe".to_owned(), "OneDrive.exe".to_owned()], 0)],
        "closed first, with nothing started yet"
    );
    assert!(log_codes(&t).is_empty());

    // A program that refuses to close: logged for the Hub, the game still starts.
    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_close", "discord.exe");
    recording_closer(&t, 2);
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert_eq!(t.host.started().len(), 1);
    assert_eq!(log_codes(&t), [BOOSTER_CODE]);

    // No process control running: logged too, the game still starts.
    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_close", "discord.exe");
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert_eq!(log_codes(&t), [BOOSTER_CODE]);

    // An empty list closes nothing.
    let t = self::t(&entries);
    let seen = recording_closer(&t, 0);
    go(&t, &entries, "G1").await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_game_gets_the_branch_priority_above_normal_by_default() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    let pid = t.host.alive_pids()[0];
    assert_eq!(t.host.priorities(), [(pid, BoosterPriority::AboveNormal)]);

    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_priority", "high");
    go(&t, &entries, "G1").await;
    assert_eq!(t.host.priorities()[0].1, BoosterPriority::High);

    // Normal is the priority a process starts with: nothing to set.
    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_priority", "normal");
    go(&t, &entries, "G1").await;
    assert!(t.host.priorities().is_empty());

    // A priority that will not change is logged and the game keeps running.
    let t = self::t(&entries);
    t.host
        .fail_priority_with(Some("SetPriorityClass: access denied"));
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert!(t.ctx.games.any_running());
    assert_eq!(log_codes(&t), [BOOSTER_CODE]);
}

#[tokio::test]
async fn the_power_plan_is_raised_for_play_and_put_back_at_session_end() {
    let entries = vec![
        entry("G1", "One", r"C:\Games\1.exe"),
        entry("G2", "Two", r"C:\Games\2.exe"),
    ];
    let t = t(&entries);
    // Off by default: the plan is left alone and nothing is remembered.
    go(&t, &entries, "G1").await;
    assert!(t.host.plan_sets().is_empty());
    assert_eq!(kv_get(&t.ctx, POWER_RESTORE_KEY), None);
    assert_eq!(restore_power_plan(&t.ctx), Ok(()));

    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_power_plan", "true");
    go(&t, &entries, "G1").await;
    assert_eq!(t.host.plan(), HIGH_PERFORMANCE_PLAN);
    assert_eq!(
        kv_get(&t.ctx, POWER_RESTORE_KEY).as_deref(),
        Some(fake::BALANCED_PLAN)
    );
    // A second game in the same session keeps the first plan as the one to go back to.
    go(&t, &entries, "G2").await;
    assert_eq!(t.host.plan_sets(), [HIGH_PERFORMANCE_PLAN]);
    assert_eq!(
        kv_get(&t.ctx, POWER_RESTORE_KEY).as_deref(),
        Some(fake::BALANCED_PLAN)
    );

    // The session-end cleanup's step puts the original back, once.
    let step = crate::cleanup::PowerPlan;
    let host = crate::cleanup::fake::FakeHost::default();
    let env = crate::cleanup::StepEnv {
        ctx: &t.ctx,
        host: &host,
    };
    assert_eq!(crate::cleanup::CleanupStep::name(&step), "power_plan");
    crate::cleanup::CleanupStep::run(&step, &env).unwrap();
    assert_eq!(t.host.plan(), fake::BALANCED_PLAN);
    assert_eq!(kv_get(&t.ctx, POWER_RESTORE_KEY), None);
    crate::cleanup::CleanupStep::run(&step, &env).unwrap();
    assert_eq!(t.host.plan_sets().len(), 2, "nothing left to restore");
}

#[tokio::test]
async fn a_plan_that_cannot_change_is_logged_remembers_nothing_and_a_high_plan_is_left_alone() {
    let entries = vec![entry("G1", "One", r"C:\Games\1.exe")];
    let t = t(&entries);
    set_setting(&t, "kiosk.booster_power_plan", "true");
    t.host.fail_plan_with(Some("powercfg failed"));
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    assert_eq!(log_codes(&t), [BOOSTER_CODE]);
    assert_eq!(kv_get(&t.ctx, POWER_RESTORE_KEY), None);

    // Already on High performance: nothing to change, nothing to restore.
    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_power_plan", "true");
    t.host.set_plan(&HIGH_PERFORMANCE_PLAN.to_uppercase());
    go(&t, &entries, "G1").await;
    assert!(t.host.plan_sets().is_empty());
    assert_eq!(kv_get(&t.ctx, POWER_RESTORE_KEY), None);

    // A restore that fails keeps the memory for the next cleanup.
    let t = self::t(&entries);
    set_setting(&t, "kiosk.booster_power_plan", "true");
    go(&t, &entries, "G1").await;
    t.host.fail_plan_with(Some("powercfg failed"));
    assert!(restore_power_plan(&t.ctx).is_err());
    assert_eq!(
        kv_get(&t.ctx, POWER_RESTORE_KEY).as_deref(),
        Some(fake::BALANCED_PLAN)
    );
    t.host.fail_plan_with(None);
    assert_eq!(restore_power_plan(&t.ctx), Ok(()));
}

#[tokio::test(start_paused = true)]
async fn the_launch_time_is_the_wait_for_the_first_window_and_none_after_three_minutes_or_an_exit()
{
    let host = fake::FakeGames::default();
    let spec = LaunchSpec {
        exe: Some(r"C:\g.exe".into()),
        args: String::new(),
        uri: None,
    };
    let pid = host.start(&spec).unwrap();
    let poll = Duration::from_millis(250);
    let timeout = Duration::from_millis(u64::from(LAUNCH_WINDOW_TIMEOUT_MS));

    // The window is already up: measured at once.
    host.show_window(pid);
    let now = tokio::time::Instant::now();
    assert_eq!(
        first_window_ms(&host, pid, now, poll, timeout).await,
        Some(0)
    );

    // The click was 14.2 s ago and the window shows now.
    let clicked = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_millis(14_200)).await;
    assert_eq!(
        first_window_ms(&host, pid, clicked, poll, timeout).await,
        Some(14_200)
    );

    // No window ever: nothing after the timeout.
    let quiet = host.start(&spec).unwrap();
    let clicked = tokio::time::Instant::now();
    assert_eq!(
        first_window_ms(&host, quiet, clicked, poll, timeout).await,
        None
    );
    assert!(clicked.elapsed() >= timeout);

    // Exits first: nothing, without waiting out the timeout.
    let gone = host.start(&spec).unwrap();
    host.exit(gone);
    let clicked = tokio::time::Instant::now();
    assert_eq!(
        first_window_ms(&host, gone, clicked, poll, timeout).await,
        None
    );
    assert!(clicked.elapsed() < timeout);
}

#[tokio::test]
async fn a_game_that_shows_no_window_is_recorded_with_no_launch_time_and_no_error() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    // Nothing is queued while the game is still starting up.
    assert!(launches(&t).is_empty());
    t.host.exit(t.host.alive_pids()[0]);
    wait_queued(&t, 1).await;
    assert_eq!(launches(&t), [None]);
    assert!(
        log_codes(&t).is_empty(),
        "no error for a game with no window"
    );
}

// ---- Story 6.27: play hours ----

fn plays(t: &T) -> Vec<arena_proto::KioskGamePlayRecorded> {
    queued(t)
        .into_iter()
        .filter_map(|(_, payload)| match payload {
            Some(IntentPayload::KioskGamePlayRecorded(p)) => Some(p),
            _ => None,
        })
        .collect()
}

const MIN: i64 = 60_000;

#[tokio::test]
async fn a_game_that_exits_after_25_minutes_queues_one_play_and_is_no_longer_tracked() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    let pid = t.host.alive_pids()[0];
    t.clock.advance(25 * MIN);
    // Still running: nothing yet.
    assert_eq!(reap(&t.ctx), 1);
    assert!(plays(&t).is_empty());

    t.host.exit(pid);
    assert_eq!(reap(&t.ctx), 0);
    let play = plays(&t);
    assert_eq!(play.len(), 1);
    assert_eq!(
        play[0],
        arena_proto::KioskGamePlayRecorded {
            game_id: "G1".into(),
            started_at: NOW_MS,
            ended_at: NOW_MS + 25 * MIN,
            minutes: 25,
        }
    );
    play[0].validate().unwrap();
    // A device intent stamped when it ended, so the offline replay delivers it once.
    let intent = t
        .ctx
        .queue
        .lock()
        .unwrap()
        .entries()
        .unwrap()
        .pop()
        .unwrap()
        .intent;
    assert_eq!(intent.kind, IntentKind::KioskGamePlayRecorded);
    assert_eq!(intent.issued_at, NOW_MS + 25 * MIN);
    assert_eq!(intent.actor.kind, arena_proto::ActorKind::Device);

    // Written once: the game is no longer tracked, however often it is checked.
    assert_eq!(reap(&t.ctx), 0);
    assert_eq!(list_of(&t.ctx, entries.clone())["running"], json!(false));
    assert_eq!(plays(&t).len(), 1);

    // Played again: a new play from the new start.
    t.clock.advance(5 * MIN);
    go(&t, &entries, "G1").await;
    t.clock.advance(MIN);
    t.host.exit(t.host.alive_pids()[0]);
    reap(&t.ctx);
    let play = plays(&t);
    assert_eq!(play.len(), 2);
    assert_eq!(play[1].started_at, NOW_MS + 30 * MIN);
    assert_eq!(play[1].minutes, 1);
}

#[tokio::test]
async fn a_relaunch_right_after_an_exit_records_the_first_play_first() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    t.clock.advance(10 * MIN);
    t.host.exit(t.host.alive_pids()[0]);
    // No sweep ran in between: the launch itself notices the exit.
    assert_eq!(go(&t, &entries, "G1").await["outcome"], "started");
    let play = plays(&t);
    assert_eq!(play.len(), 1);
    assert_eq!(play[0].minutes, 10);
    assert!(t.ctx.games.any_running());
}

#[tokio::test]
async fn the_session_ending_records_a_running_game_now_and_its_later_exit_records_nothing() {
    let entries = vec![
        entry("G1", "One", r"C:\Games\1.exe"),
        entry("G2", "Two", r"C:\Games\2.exe"),
    ];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    t.clock.advance(7 * MIN);
    go(&t, &entries, "G2").await;
    let pids = t.host.alive_pids();
    // G2 exits a minute later and is noticed then: it ends there, not when the session does.
    t.clock.advance(MIN);
    t.host.exit(pids[1]);
    assert_eq!(reap(&t.ctx), 1);
    t.clock.advance(2 * MIN);

    record_running(&t.ctx);
    let play = plays(&t);
    let got: Vec<_> = play
        .iter()
        .map(|p| (p.game_id.as_str(), p.started_at, p.ended_at, p.minutes))
        .collect();
    assert_eq!(
        got,
        [
            ("G2", NOW_MS + 7 * MIN, NOW_MS + 8 * MIN, 1),
            ("G1", NOW_MS, NOW_MS + 10 * MIN, 10),
        ]
    );
    // The programs are ended afterwards; the exit records nothing.
    t.host.exit(pids[0]);
    t.clock.advance(MIN);
    assert_eq!(reap(&t.ctx), 0);
    record_running(&t.ctx);
    assert_eq!(plays(&t).len(), 2);
    assert!(!t.ctx.games.any_running());
}

#[test]
fn a_play_is_whole_minutes_to_the_nearest_and_always_one_the_hub_accepts() {
    let at = |span_ms: i64| play_of("G", 1_000_000, 1_000_000 + span_ms);
    for (span, minutes) in [
        (0, 0),
        (20_000, 0),
        (29_999, 0),
        (30_000, 1),
        (89_999, 1),
        (90_000, 2),
        (25 * MIN, 25),
    ] {
        let p = at(span);
        assert_eq!(p.minutes, minutes, "{span} ms");
        p.validate().unwrap();
    }
    // A clock that went back is a zero-minute play, not an invalid one.
    let back = play_of("G", 5_000_000, 4_000_000);
    assert_eq!(
        (back.started_at, back.ended_at, back.minutes),
        (4_000_000, 4_000_000, 0)
    );
    back.validate().unwrap();
    // A game left running for weeks is capped at the Hub's limit, its start pulled forward.
    let long = play_of("G", 1_000_000, 1_000_000 + 40 * 24 * 60 * MIN);
    assert_eq!(long.minutes, MAX_PLAY_MINUTES);
    assert_eq!(long.ended_at - long.started_at, MAX_PLAY_MINUTES * MIN);
    long.validate().unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_run_loop_records_an_exited_game_by_itself_within_one_poll() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    let pid = t.host.alive_pids()[0];
    t.host.show_window(pid);
    let (stop, stop_rx) = watch::channel(false);
    let worker = tokio::spawn(run(Arc::clone(&t.ctx), stop_rx));

    // Running: the loop records nothing.
    tokio::time::sleep(POLL * 2).await;
    assert!(plays(&t).is_empty());

    // The process exits; no list, launch or reap call follows. The loop notices on its own.
    t.clock.advance(4 * MIN);
    t.host.exit(pid);
    tokio::time::sleep(POLL * 2).await;
    let play = plays(&t);
    assert_eq!(play.len(), 1);
    assert_eq!((play[0].game_id.as_str(), play[0].minutes), ("G1", 4));
    // And only once, however long the loop keeps running.
    tokio::time::sleep(POLL * 5).await;
    assert_eq!(plays(&t).len(), 1);
    assert!(!t.ctx.games.any_running());

    stop.send(true).unwrap();
    worker.await.unwrap();
}

// ---- Story 6.28: the Apps tab ----

fn app_entry(id: &str, name: &str, exe: &str) -> KioskLibraryEntry {
    let mut e = entry(id, name, exe);
    e.kind = "app".into();
    e
}

async fn go_app(t: &T, entries: &[KioskLibraryEntry], id: &str) -> serde_json::Value {
    launch_app_in(&t.ctx, ME, entries.to_vec(), id.into())
        .await
        .unwrap()
}

/// The `(level, code, detail)` of the `kiosk.log` events waiting for the Hub, oldest first.
fn logs(t: &T) -> Vec<(arena_proto::KioskLogLevel, String, String)> {
    queued(t)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Some(IntentPayload::KioskLog(l)) => Some((l.level, l.code, l.detail)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_apps_list_holds_only_apps_sorted_by_name_with_their_open_flag() {
    let entries = vec![
        app_entry("B", "discord", r"C:\A\d.exe"),
        entry("G", "Apex", r"C:\G\a.exe"),
        app_entry("A", "Chrome", r"C:\A\c.exe"),
    ];
    let t = t(&entries);
    let list = apps_of(&t.ctx, entries.clone());
    let names: Vec<_> = list["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].as_str().unwrap(), c["running"].as_bool().unwrap()))
        .collect();
    assert_eq!(names, [("Chrome", false), ("discord", false)]);
    // The games list is unchanged by apps.
    let games = list_of(&t.ctx, entries);
    assert_eq!(games["games"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_app_starts_after_the_allow_list_and_is_logged_as_an_app_launch_not_a_play() {
    let entries = vec![app_entry("APP1", "Discord", r"C:\Apps\discord.exe")];
    let t = t(&entries);

    let out = go_app(&t, &entries, "APP1").await;
    assert_eq!(out, json!({"outcome": "started"}));
    assert_eq!(t.host.started().len(), 1);
    assert_eq!(
        t.host.started()[0].exe.as_deref(),
        Some(r"C:\Apps\discord.exe")
    );
    // Not a game: no running signal, no game.launched, no booster step.
    assert!(!t.ctx.games.any_running());
    let list = list_of(&t.ctx, vec![]);
    assert_eq!(list["running"], json!(false));
    assert!(t.host.priorities().is_empty());
    assert!(t.host.plan_sets().is_empty());
    let q = queued(&t);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, IntentKind::KioskLog);
    assert_eq!(
        logs(&t),
        [(
            arena_proto::KioskLogLevel::Info,
            APP_LAUNCHED_CODE.to_owned(),
            "Discord (APP1)".to_owned()
        )]
    );
    // It shows as open in the list.
    assert_eq!(apps_of(&t.ctx, entries.clone())["apps"][0]["running"], true);

    // Ending it and the session end record no play.
    t.host.exit(t.host.alive_pids()[0]);
    reap(&t.ctx);
    record_running(&t.ctx);
    assert_eq!(queued(&t).len(), 1);
    assert!(plays(&t).is_empty());
}

#[tokio::test]
async fn an_open_app_is_brought_forward_and_nothing_is_queued() {
    let entries = vec![app_entry("APP1", "Discord", r"C:\Apps\discord.exe")];
    let t = t(&entries);
    go_app(&t, &entries, "APP1").await;
    let pid = t.host.alive_pids()[0];

    let out = go_app(&t, &entries, "APP1").await;
    assert_eq!(out, json!({"outcome": "already_running", "pid": pid}));
    assert_eq!(t.host.started().len(), 1, "no second copy");
    assert_eq!(queued(&t).len(), 1, "no second app_launched");

    // Once it exits the next click starts it again.
    t.host.exit(pid);
    assert_eq!(
        go_app(&t, &entries, "APP1").await,
        json!({"outcome": "started"})
    );
    assert_eq!(t.host.started().len(), 2);
}

#[tokio::test]
async fn an_app_that_is_not_here_not_allowed_or_will_not_start_is_logged_and_failed() {
    let entries = vec![
        app_entry("APP1", "Discord", r"C:\Apps\discord.exe"),
        entry("G1", "Valorant", r"C:\Games\v.exe"),
    ];
    let t = t(&entries);

    // Not in this PC's apps.
    assert_eq!(
        go_app(&t, &entries, "NOPE").await,
        json!({"outcome": "failed"})
    );
    // A game id is not an app.
    assert_eq!(go_app(&t, &entries, "G1").await["outcome"], "failed");
    // Not allowed: the entry's rule is not in the allow-list.
    let other = vec![app_entry("APP2", "Other", r"C:\Apps\other.exe")];
    assert_eq!(go_app(&t, &other, "APP2").await["outcome"], "failed");
    // The executable fails to start.
    t.host.fail_with(Some("CreateProcessAsUserW: not found"));
    assert_eq!(go_app(&t, &entries, "APP1").await["outcome"], "failed");

    assert!(t.host.started().is_empty());
    let l = logs(&t);
    assert_eq!(l.len(), 4);
    for (level, code, _) in &l {
        assert_eq!(*level, arena_proto::KioskLogLevel::Error);
        assert_eq!(code, FAILED_CODE);
    }
    assert_eq!(l[0].2, "NOPE: not in this PC's apps");
    assert_eq!(l[1].2, "G1: not in this PC's apps");
    assert_eq!(l[2].2, "Other: not allowed on this PC");
    assert_eq!(l[3].2, "Discord: CreateProcessAsUserW: not found");
    assert!(
        queued(&t).iter().all(|(k, _)| *k == IntentKind::KioskLog),
        "no game.launched"
    );
}

#[tokio::test]
async fn the_apps_tab_is_empty_and_app_launch_refused_without_a_session_or_while_locked() {
    let entries = vec![app_entry("APP1", "Discord", r"C:\Apps\discord.exe")];
    let t = t(&entries);
    t.ctx.pc.screen.send_modify(|s| {
        s.locked = true;
        s.reason = Some(LockReason::TimesUp);
    });
    assert_eq!(list_apps(&t.ctx).unwrap(), json!({"apps": []}));
    let err = launch_app(&t.ctx, json!({"id": "APP1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);

    t.ctx.pc.screen.send_replace(Screen::default());
    assert_eq!(list_apps(&t.ctx).unwrap(), json!({"apps": []}));
    let err = launch_app(&t.ctx, json!({"id": "APP1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(t.host.started().is_empty());

    // No id: a bad frame; no host: unsupported.
    for body in [json!({}), json!({"id": ""}), json!({"id": 5})] {
        let err = launch_app(&t.ctx, body).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadFrame);
    }
    let (_dir, ctx, _) = bare(&[]);
    let err = launch_app(&ctx, json!({"id": "APP1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
}

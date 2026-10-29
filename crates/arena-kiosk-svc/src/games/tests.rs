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
use crate::clock::SystemClock;
use crate::paths::DataDir;
use crate::pc::{Screen, ScreenSession};
use crate::queue::QueueDb;
use crate::runtime::{Ctx, Timings};
use crate::secret::SecretStore;

const ME: &str = "DEVICE-ME";

struct NoSecret;
impl SecretStore for NoSecret {
    fn save(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn load(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

fn entry(id: &str, name: &str, exe: &str) -> KioskLibraryEntry {
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
        username: Some("kai".into()),
        guest_no: None,
        ends_at: None,
        balance_minor: None,
        plan_name: None,
        low_balance_minor: None,
        hold: false,
    }
}

struct T {
    _dir: tempfile::TempDir,
    ctx: Arc<Ctx>,
    host: Arc<FakeGames>,
}

/// A service with a session on screen, a host and every library path allowed.
fn t(entries: &[KioskLibraryEntry]) -> T {
    let (dir, ctx) = bare(entries);
    let host = Arc::new(FakeGames::default());
    ctx.games.set_host(host.clone());
    T {
        _dir: dir,
        ctx,
        host,
    }
}

/// [`t`] without a game host.
fn bare(entries: &[KioskLibraryEntry]) -> (tempfile::TempDir, Arc<Ctx>) {
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
        clock: Arc::new(SystemClock),
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
    (dir, ctx)
}

/// The kinds of the intents waiting for the Hub, oldest first.
fn queued(t: &T) -> Vec<(IntentKind, Option<IntentPayload>)> {
    let queue = t.ctx.queue.lock().unwrap();
    queue
        .entries()
        .unwrap()
        .into_iter()
        .map(|q| (q.intent.kind, Some(q.intent.payload)))
        .collect()
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
    let cards = cards(entries, |id| id == "B");
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
    let cards = cards(vec![e], |_| false);
    assert_eq!(cards[0].category, "Shooter");
    assert_eq!(cards[0].tags, ["fps", "co-op"]);
    let body = serde_json::to_value(&cards[0]).unwrap();
    assert_eq!(body["category"], "Shooter");
    assert_eq!(body["tags"], serde_json::json!(["fps", "co-op"]));
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

    let q = queued(&t);
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].0, IntentKind::KioskGameLaunched);
    let Some(IntentPayload::KioskGameLaunched(p)) = &q[0].1 else {
        panic!("a game.launched payload");
    };
    assert_eq!((p.game_id.as_str(), p.name.as_str()), ("G1", "Valorant"));
    p.validate().unwrap();
}

#[tokio::test]
async fn a_second_launch_brings_the_running_game_forward_instead_of_starting_it_again() {
    let entries = vec![entry("G1", "Valorant", r"C:\Games\v.exe")];
    let t = t(&entries);
    go(&t, &entries, "G1").await;
    let pid = t.host.alive_pids()[0];

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
    let (_dir, ctx) = bare(&[]);
    let err = launch(&ctx, json!({"id": "G1"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
}

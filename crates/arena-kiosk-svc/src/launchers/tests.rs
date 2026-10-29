//! The launcher cards on the fake host (the round trip through the app's IPC is in the
//! Playwright suite, the Hub's record in `arena-core`).

use arena_proto::{ErrorCode, IntentKind, IntentPayload, ScanLauncher};
use serde_json::json;

use super::*;
use crate::games::tests::{ME, T, entry, queued, t};
use crate::pc::Screen;

const STEAM_EXE: &str = r"C:\Program Files (x86)\Steam\steam.exe";
const EPIC_EXE: &str =
    r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe";
const RIOT_EXE: &str = r"C:\Riot Games\Riot Client\RiotClientServices.exe";

/// A service whose allow-list holds the Steam, Epic and Riot install rules.
fn allowing() -> T {
    let mut games = Vec::new();
    for launcher in ["steam", "epic", "riot"] {
        let mut e = entry(launcher, launcher, r"C:\Games\x.exe");
        e.launcher = launcher.into();
        games.push(e);
    }
    t(&games)
}

fn here(paths: &[&str]) -> impl Fn(&str) -> bool {
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    move |p| paths.iter().any(|x| x == p)
}

async fn go(t: &T, id: &str, paths: &[&str]) -> Value {
    let host = t.ctx.games.host().unwrap();
    open_in(&t.ctx, ME, id, &host, here(paths)).await.unwrap()
}

fn log_details(t: &T) -> Vec<String> {
    queued(t)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Some(IntentPayload::KioskLog(l)) if l.code == FAILED_CODE => Some(l.detail),
            _ => None,
        })
        .collect()
}

fn opened(t: &T) -> Vec<String> {
    queued(t)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Some(IntentPayload::KioskLauncherOpened(o)) => Some(o.launcher),
            _ => None,
        })
        .collect()
}

fn scan_of(launcher: &str, path: &str) -> Vec<ScanLauncher> {
    vec![ScanLauncher {
        launcher: launcher.into(),
        path: path.into(),
    }]
}

#[test]
fn a_launcher_is_found_where_the_scan_saw_it_else_at_its_usual_place() {
    let steam = def_of("steam").unwrap();
    let scan = scan_of("steam", r"D:\Steam\");
    assert_eq!(
        exe_of(steam, &scan, here(&[r"D:\Steam\steam.exe", STEAM_EXE])).as_deref(),
        Some(r"D:\Steam\steam.exe"),
        "the scanned folder first"
    );
    assert_eq!(
        exe_of(steam, &scan, here(&[STEAM_EXE])).as_deref(),
        Some(STEAM_EXE)
    );
    assert_eq!(exe_of(steam, &[], here(&[])), None);
    // The scan of another launcher says nothing about Steam.
    let riot_scan = scan_of("riot", r"D:\Riot Games\Riot Client");
    assert_eq!(exe_of(steam, &riot_scan, here(&[])), None);
    let riot = def_of("riot").unwrap();
    assert_eq!(
        exe_of(
            riot,
            &riot_scan,
            here(&[r"D:\Riot Games\Riot Client\RiotClientServices.exe"])
        )
        .as_deref(),
        Some(r"D:\Riot Games\Riot Client\RiotClientServices.exe")
    );
    let epic = def_of("epic").unwrap();
    let epic_scan = scan_of("epic", r"C:\Program Files (x86)\Epic Games\Launcher");
    assert_eq!(
        exe_of(epic, &epic_scan, here(&[EPIC_EXE])).as_deref(),
        Some(EPIC_EXE)
    );
    assert!(def_of("battlenet").is_none() && def_of("").is_none());
}

#[test]
fn the_row_lists_only_installed_launchers_in_order_with_their_open_flag() {
    let t = allowing();
    assert_eq!(
        list(&t.ctx, here(&[])).unwrap(),
        json!({"launchers": []}),
        "nothing installed, nothing shown"
    );
    let both = list(&t.ctx, here(&[RIOT_EXE, STEAM_EXE])).unwrap();
    assert_eq!(
        both,
        json!({"launchers": [
            {"id": "steam", "name": "Steam", "running": false},
            {"id": "riot", "name": "Riot Client", "running": false},
        ]})
    );
    // A launcher open outside this service shows as open.
    t.host.show_process("steam.exe", 77);
    let open = list(&t.ctx, here(&[STEAM_EXE])).unwrap();
    assert_eq!(open["launchers"][0]["running"], json!(true));
    // The scan's record is read back.
    t.ctx
        .queue
        .lock()
        .unwrap()
        .kv_set(
            SCAN_KEY,
            r#"[{"launcher":"epic","path":"E:\\Epic\\Launcher"}]"#,
            0,
        )
        .unwrap();
    assert_eq!(scanned(&t.ctx).len(), 1);
    let epic = list(
        &t.ctx,
        here(&[r"E:\Epic\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"]),
    )
    .unwrap();
    assert_eq!(epic["launchers"][0]["id"], json!("epic"));
}

#[test]
fn a_damaged_scan_record_lists_the_usual_places_only() {
    let t = allowing();
    t.ctx
        .queue
        .lock()
        .unwrap()
        .kv_set(SCAN_KEY, "not json", 0)
        .unwrap();
    assert!(scanned(&t.ctx).is_empty());
    assert_eq!(
        list(&t.ctx, here(&[STEAM_EXE])).unwrap()["launchers"][0]["id"],
        json!("steam")
    );
}

#[test]
fn nothing_is_listed_without_a_session_on_screen_or_while_locked() {
    let t = allowing();
    let (session, reason) = {
        let screen = t.ctx.pc.screen.borrow();
        (screen.session.clone(), screen.reason.clone())
    };
    t.ctx.pc.screen.send_replace(Screen {
        locked: true,
        reason: reason.clone(),
        session: session.clone(),
    });
    assert_eq!(
        list(&t.ctx, here(&[STEAM_EXE])).unwrap(),
        json!({"launchers": []})
    );
    t.ctx.pc.screen.send_replace(Screen {
        locked: false,
        reason,
        session: None,
    });
    assert_eq!(
        list(&t.ctx, here(&[STEAM_EXE])).unwrap(),
        json!({"launchers": []})
    );
}

#[tokio::test]
async fn opening_a_launcher_starts_only_its_executable_and_queues_launcher_opened() {
    let t = allowing();
    let out = go(&t, "steam", &[STEAM_EXE]).await;
    assert_eq!(out, json!({"outcome": "started"}));
    let started = t.host.started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].exe.as_deref(), Some(STEAM_EXE));
    assert!(started[0].args.is_empty() && started[0].uri.is_none());
    assert_eq!(opened(&t), vec!["steam".to_owned()]);
    let kinds: Vec<IntentKind> = queued(&t).into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, vec![IntentKind::KioskLauncherOpened]);
    // An open launcher is not a game running: the media keeps playing behind the tab.
    assert!(!t.ctx.games.any_running());
}

#[tokio::test]
async fn a_launcher_that_is_already_open_is_brought_forward_not_started_again() {
    let t = allowing();
    go(&t, "steam", &[STEAM_EXE]).await;
    let pid = t.host.alive_pids()[0];
    let out = go(&t, "steam", &[STEAM_EXE]).await;
    assert_eq!(out, json!({"outcome": "already_running", "pid": pid}));
    assert_eq!(t.host.started().len(), 1, "no second copy");
    assert_eq!(opened(&t).len(), 1, "no second launcher.opened");

    // Once it exits, the next card press opens it again.
    t.host.exit(pid);
    assert_eq!(go(&t, "steam", &[STEAM_EXE]).await["outcome"], "started");
    assert_eq!(opened(&t).len(), 2);

    // One open outside this service (a game started through its link) counts too.
    t.host.show_process("RiotClientServices.exe", 4242);
    let out = go(&t, "riot", &[RIOT_EXE]).await;
    assert_eq!(out, json!({"outcome": "already_running", "pid": 4242}));
    assert_eq!(t.host.started().len(), 2);
}

#[tokio::test]
async fn a_launcher_that_is_missing_or_not_allowed_or_will_not_start_is_logged_and_failed() {
    let t = allowing();
    let failed = json!({"outcome": "failed"});
    // Not installed.
    assert_eq!(go(&t, "epic", &[]).await, failed);
    // Not a launcher at all.
    assert_eq!(go(&t, "discord", &[]).await, failed);
    // Not on the allow-list: Steam in another folder is found but not allowed.
    let scan = r#"[{"launcher":"steam","path":"D:\\Steam"}]"#;
    t.ctx
        .queue
        .lock()
        .unwrap()
        .kv_set(SCAN_KEY, scan, 0)
        .unwrap();
    assert_eq!(go(&t, "steam", &[r"D:\Steam\steam.exe"]).await, failed);
    // Will not start.
    t.host.fail_with(Some("CreateProcessAsUserW: denied"));
    assert_eq!(go(&t, "riot", &[RIOT_EXE]).await, failed);
    assert!(t.host.started().is_empty());
    assert!(
        opened(&t).is_empty(),
        "a failed open is not recorded as opened"
    );
    let details = log_details(&t);
    assert_eq!(details.len(), 4);
    assert!(details[0].contains("Epic Games") && details[0].contains("not installed"));
    assert!(details[2].contains("not allowed"));
    assert!(details[3].contains("denied"));
    // The one that failed is not stuck: it opens once the cause is gone.
    t.host.fail_with(None);
    assert_eq!(go(&t, "riot", &[RIOT_EXE]).await["outcome"], "started");
}

#[tokio::test]
async fn nothing_opens_without_a_session_or_an_id() {
    let t = allowing();
    let err = open(&t.ctx, json!({})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::BadFrame);
    let err = open(&t.ctx, json!({"id": ""})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::BadFrame);
    let (session, reason) = {
        let screen = t.ctx.pc.screen.borrow();
        (screen.session.clone(), screen.reason.clone())
    };
    t.ctx.pc.screen.send_replace(Screen {
        locked: true,
        reason,
        session,
    });
    let err = open(&t.ctx, json!({"id": "steam"})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(t.host.started().is_empty());
}

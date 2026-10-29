//! The PC settings panel on the fake PC. The Hub's side (what is saved, a guest's `saved:false`)
//! is `crates/arena-api/tests/lan.rs`; here the Hub is unreachable, which is the "change stays
//! applied for the session" row of the story.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{
    ErrorCode, KEY_DISPLAY_MODES, LockReason, PcSettings, PcSettingsPanel, SessionScreen,
};
use serde_json::json;
use tokio::sync::{Notify, broadcast, watch};

use super::fake::FakeHost;
use super::*;
use crate::clock::SystemClock;
use crate::paths::DataDir;
use crate::procs::ProcessLists;
use crate::queue::QueueDb;
use crate::runtime::{Ctx, Timings};
use crate::secret::SecretStore;

struct NoSecret;
impl SecretStore for NoSecret {
    fn save(&self, _: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
    fn load(&self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

fn ctx() -> (tempfile::TempDir, Arc<Ctx>) {
    let dir = tempfile::tempdir().unwrap();
    let data = DataDir(dir.path().to_path_buf());
    let queue = QueueDb::open(&data.queue()).unwrap();
    let pc = crate::pc::PcState::load(&queue);
    let cleanup = crate::cleanup::CleanupState::load(&queue);
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
        lists: watch::channel(Arc::new(ProcessLists::defaults())).0,
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
        games: crate::games::GameState::default(),
        pc_settings: PcSettingsState::default(),
    });
    (dir, ctx)
}

/// A context with the fake PC attached and a session on screen.
fn playing() -> (tempfile::TempDir, Arc<Ctx>, Arc<FakeHost>) {
    let (dir, ctx) = ctx();
    let host = Arc::new(FakeHost::default());
    ctx.pc_settings.set_host(host.clone());
    crate::session::sync(
        &ctx,
        SessionScreen {
            session_id: "S1".into(),
            device_name: "PC 1".into(),
            elapsed_ms: 0,
            lock_after_ms: 3_600_000,
            time_up_mode: "clean_up".into(),
            grace_ms: 0,
            ordered_items: 0,
            orders: Vec::new(),
            username: Some("asha".into()),
            guest_no: None,
            remaining_ms: None,
            balance_minor: None,
            wallet_left_minor: None,
            plan_name: None,
        },
        true,
    );
    (dir, ctx, host)
}

fn allow(ctx: &Ctx, modes: &str) {
    ctx.queue
        .lock()
        .unwrap()
        .kv_set(KEY_DISPLAY_MODES, modes, 2)
        .unwrap();
}

async fn panel_of(ctx: &Arc<Ctx>) -> PcSettingsPanel {
    serde_json::from_value(view(ctx).await.unwrap()).unwrap()
}

async fn change_err(ctx: &Arc<Ctx>, body: serde_json::Value) -> arena_proto::AppError {
    change(ctx, body).await.unwrap_err()
}

#[tokio::test]
async fn the_panel_offers_the_branch_modes_this_monitor_supports() {
    let (_dir, ctx, _host) = playing();
    // Nothing allowed: this PC's own mode only, even though the monitor does more.
    let p = panel_of(&ctx).await;
    assert_eq!(p.display_modes, ["1920x1080@60"]);
    assert_eq!(p.display_mode, "1920x1080@60");
    assert_eq!(p.mouse_speed, 10);
    assert_eq!(p.audio_outputs, ["Speakers", "Headset"]);
    assert_eq!(p.audio_output.as_deref(), Some("Speakers"));
    assert_eq!(p.keyboard_layouts.len(), 2);
    assert_eq!(p.keyboard_layout.as_deref(), Some("04090409"));
    assert_eq!(p.revert_ms, None);
    // Allowed and supported, in the branch's order; 3840x2160 is allowed but unsupported.
    allow(&ctx, "2560x1440@60, 3840x2160@60, 1920x1080@144");
    assert_eq!(
        panel_of(&ctx).await.display_modes,
        ["2560x1440@60", "1920x1080@144"]
    );
}

#[tokio::test]
async fn a_pc_with_no_control_refuses_the_panel() {
    let (_dir, ctx) = ctx();
    assert_eq!(view(&ctx).await.unwrap_err().code, ErrorCode::Unsupported);
    let e = change_err(&ctx, json!({ "mouse_speed": 5 })).await;
    assert_eq!(e.code, ErrorCode::Unsupported);
    assert_eq!(keep(&ctx).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn a_change_is_applied_at_once_and_stays_when_the_hub_is_unreachable() {
    let (_dir, ctx, host) = playing();
    // The Hub link is down in this test: the answer is HUB_SLOW, the change is still applied.
    let e = change_err(&ctx, json!({ "mouse_speed": 14 })).await;
    assert_eq!(e.code, ErrorCode::HubSlow);
    assert_eq!(host.pc().mouse_speed, 14);
    assert_eq!(panel_of(&ctx).await.mouse_speed, 14);
    let e = change_err(&ctx, json!({ "audio_output": "Headset" })).await;
    assert_eq!(e.code, ErrorCode::HubSlow);
    assert_eq!(host.pc().audio_output.as_deref(), Some("Headset"));
    let e = change_err(&ctx, json!({ "keyboard_layout": "04070407" })).await;
    assert_eq!(e.code, ErrorCode::HubSlow);
    assert_eq!(host.pc().keyboard_layout.as_deref(), Some("04070407"));
    assert_eq!(
        host.calls(),
        ["mouse=14", "audio=Headset", "layout=04070407"]
    );
}

#[tokio::test]
async fn a_bad_or_impossible_value_changes_nothing() {
    let (_dir, ctx, host) = playing();
    allow(&ctx, "1920x1080@144, 3840x2160@60");
    for (body, field) in [
        (json!({ "mouse_speed": 0 }), "mouse_speed"),
        (json!({ "mouse_speed": 21 }), "mouse_speed"),
        (json!({ "display_mode": "huge" }), "display_mode"),
        // Supported but not allowed, and allowed but not supported.
        (json!({ "display_mode": "2560x1440@60" }), "display_mode"),
        (json!({ "display_mode": "3840x2160@60" }), "display_mode"),
        (
            json!({ "audio_output": "Monitor speakers" }),
            "audio_output",
        ),
        (json!({ "keyboard_layout": "0000040C" }), "keyboard_layout"),
        (json!({}), "settings"),
    ] {
        let e = change_err(&ctx, body.clone()).await;
        assert_eq!(e.code, ErrorCode::ValidationFailed, "{body}");
        assert_eq!(e.field.as_deref(), Some(field), "{body}");
    }
    let e = change_err(&ctx, json!({ "mouse_speed": "fast" })).await;
    assert_eq!(e.code, ErrorCode::BadFrame);
    assert!(host.calls().is_empty());
    assert!(!ctx.pc_settings.pending());
}

#[tokio::test]
async fn there_is_nothing_to_change_without_a_session() {
    let (_dir, ctx) = ctx();
    ctx.pc_settings.set_host(Arc::new(FakeHost::default()));
    let e = change_err(&ctx, json!({ "mouse_speed": 5 })).await;
    assert_eq!(e.code, ErrorCode::ValidationFailed);
}

#[tokio::test(start_paused = true)]
async fn a_display_change_not_kept_goes_back_and_nothing_is_saved() {
    let (_dir, ctx, host) = playing();
    allow(&ctx, "1920x1080@60, 1920x1080@144");
    let answer = change(&ctx, json!({ "display_mode": "1920x1080@144" }))
        .await
        .unwrap();
    // Applied, waiting for "Keep this", not saved.
    assert_eq!(answer, json!({ "saved": false, "revert_ms": 15_000 }));
    assert_eq!(host.pc().display_mode, "1920x1080@144");
    assert!(ctx.pc_settings.pending());
    let p = panel_of(&ctx).await;
    assert_eq!(p.display_mode, "1920x1080@144");
    assert!(p.revert_ms.is_some_and(|ms| ms <= 15_000 && ms > 14_000));
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert_eq!(host.pc().display_mode, "1920x1080@144");
    tokio::time::sleep(Duration::from_secs(2)).await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
        if !ctx.pc_settings.pending() {
            break;
        }
    }
    assert_eq!(host.pc().display_mode, "1920x1080@60");
    assert!(!ctx.pc_settings.pending());
    assert_eq!(
        host.calls(),
        ["display=1920x1080@144", "display=1920x1080@60"]
    );
    // Nothing is left to keep.
    assert_eq!(keep(&ctx).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test(start_paused = true)]
async fn a_kept_display_change_stays_and_a_second_change_goes_back_to_the_first_mode() {
    let (_dir, ctx, host) = playing();
    allow(&ctx, "1920x1080@60, 1920x1080@144, 2560x1440@60");
    change(&ctx, json!({ "display_mode": "1920x1080@144" }))
        .await
        .unwrap();
    // A second pick before answering still goes back to where the PC started.
    change(&ctx, json!({ "display_mode": "2560x1440@60" }))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(16)).await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
        if host.pc().display_mode == "1920x1080@60" {
            break;
        }
    }
    assert_eq!(host.pc().display_mode, "1920x1080@60");

    // Kept: the timer does nothing (the Hub is down here, so the save is HUB_SLOW, but the mode
    // stays for the session).
    change(&ctx, json!({ "display_mode": "1920x1080@144" }))
        .await
        .unwrap();
    let e = keep(&ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::HubSlow);
    assert!(!ctx.pc_settings.pending());
    tokio::time::sleep(Duration::from_secs(30)).await;
    tokio::task::yield_now().await;
    assert_eq!(host.pc().display_mode, "1920x1080@144");
}

#[tokio::test]
async fn picking_the_current_mode_waits_for_nothing() {
    let (_dir, ctx, host) = playing();
    let answer = change(&ctx, json!({ "display_mode": "1920x1080@60" }))
        .await
        .unwrap();
    assert_eq!(answer, json!({ "saved": false }));
    assert!(host.calls().is_empty());
    assert!(!ctx.pc_settings.pending());
}

#[tokio::test]
async fn saved_settings_are_applied_one_by_one_and_an_unhonourable_one_is_skipped() {
    let (_dir, ctx, host) = playing();
    allow(&ctx, "1920x1080@60, 1920x1080@144");
    apply_saved(
        &ctx,
        PcSettings {
            mouse_speed: Some(17),
            // Not on the branch's list.
            display_mode: Some("2560x1440@60".into()),
            // This PC has no such device.
            audio_output: Some("Studio monitors".into()),
            keyboard_layout: Some("04070407".into()),
        },
    )
    .await;
    let pc = host.pc();
    assert_eq!(pc.mouse_speed, 17);
    assert_eq!(pc.display_mode, "1920x1080@60");
    assert_eq!(pc.audio_output.as_deref(), Some("Speakers"));
    assert_eq!(pc.keyboard_layout.as_deref(), Some("04070407"));
    assert_eq!(host.calls(), ["mouse=17", "layout=04070407"]);
    // An allowed, supported mode is applied at session start with no countdown.
    apply_saved(
        &ctx,
        PcSettings {
            display_mode: Some("1920x1080@144".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(host.pc().display_mode, "1920x1080@144");
    assert!(!ctx.pc_settings.pending());
    // The PC's own values were recorded first, once.
    let base = super::baseline(&ctx).unwrap();
    assert_eq!(
        (base.mouse_speed, base.display_mode.as_deref()),
        (Some(10), Some("1920x1080@60"))
    );
}

#[tokio::test]
async fn a_failing_saved_setting_does_not_stop_the_others() {
    let (_dir, ctx, host) = playing();
    host.fail("mouse", true);
    apply_saved(
        &ctx,
        PcSettings {
            mouse_speed: Some(3),
            keyboard_layout: Some("04070407".into()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(host.pc().keyboard_layout.as_deref(), Some("04070407"));
    assert_eq!(host.pc().mouse_speed, 10);
}

#[tokio::test]
async fn the_cleanup_step_puts_the_baseline_back_once() {
    let (_dir, ctx, host) = playing();
    // Nothing changed, nothing to put back.
    restore(&ctx).unwrap();
    assert!(host.calls().is_empty());

    allow(&ctx, "1920x1080@60, 1920x1080@144");
    let _ = change(&ctx, json!({ "mouse_speed": 20 })).await;
    let _ = change(&ctx, json!({ "audio_output": "Headset" })).await;
    let _ = change(&ctx, json!({ "keyboard_layout": "04070407" })).await;
    let _ = change(&ctx, json!({ "display_mode": "1920x1080@144" })).await;
    assert!(ctx.pc_settings.pending());
    // A change made later in the session does not move the baseline.
    let _ = change(&ctx, json!({ "mouse_speed": 2 })).await;

    restore(&ctx).unwrap();
    let pc = host.pc();
    assert_eq!(
        (pc.mouse_speed, pc.display_mode.as_str()),
        (10, "1920x1080@60")
    );
    assert_eq!(pc.audio_output.as_deref(), Some("Speakers"));
    assert_eq!(pc.keyboard_layout.as_deref(), Some("04090409"));
    assert!(
        !ctx.pc_settings.pending(),
        "no countdown outlives the session"
    );
    // Done: the baseline is forgotten, a second run touches nothing.
    let calls = host.calls().len();
    restore(&ctx).unwrap();
    assert_eq!(host.calls().len(), calls);
    assert!(super::baseline(&ctx).is_none());
}

#[tokio::test]
async fn a_setting_that_will_not_go_back_fails_the_step_and_a_retry_finishes_it() {
    let (_dir, ctx, host) = playing();
    let _ = change(&ctx, json!({ "mouse_speed": 20 })).await;
    let _ = change(&ctx, json!({ "keyboard_layout": "04070407" })).await;
    host.fail("mouse", true);
    let e = restore(&ctx).unwrap_err();
    assert_eq!(e, "1 PC setting would not go back");
    // The layout went back; the baseline stays for the retry.
    assert_eq!(host.pc().keyboard_layout.as_deref(), Some("04090409"));
    assert!(super::baseline(&ctx).is_some());
    host.fail("mouse", false);
    restore(&ctx).unwrap();
    assert_eq!(host.pc().mouse_speed, 10);
    assert!(super::baseline(&ctx).is_none());
}

#[tokio::test]
async fn the_baseline_survives_a_service_restart() {
    let (dir, ctx, host) = playing();
    let _ = change(&ctx, json!({ "mouse_speed": 20 })).await;
    // A restarted service has the same queue.db and a fresh memory.
    ctx.pc_settings.lock().baseline = None;
    assert_eq!(super::baseline(&ctx).unwrap().mouse_speed, Some(10));
    restore(&ctx).unwrap();
    assert_eq!(host.pc().mouse_speed, 10);
    drop(dir);
}

#[tokio::test]
async fn the_screen_lock_reason_is_not_needed_to_restore() {
    // restore() depends on neither the lock nor a session: a cleanup after the session is over
    // still finds the baseline.
    let (_dir, ctx, host) = playing();
    let _ = change(&ctx, json!({ "mouse_speed": 18 })).await;
    crate::pc::set_locked(&ctx, true, Some(LockReason::LoggedOut));
    restore(&ctx).unwrap();
    assert_eq!(host.pc().mouse_speed, 10);
}

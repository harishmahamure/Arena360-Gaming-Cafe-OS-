//! The intents the kiosk creates. Each gets its ULID ids and `issued_at` here, when the event
//! happens, so a replay is the very same intent and the Hub's dedupe applies it once.

use arena_proto::{
    Actor, ActorKind, Intent, IntentKind, IntentPayload, KioskLog, KioskLogLevel,
    KioskProcessBlocked, KioskScanReported,
};

/// A `kiosk.log` intent from `device_id`, issued at `now_ms`. (The Hub replaces the actor with
/// the session's device anyway.)
pub fn kiosk_log(
    device_id: &str,
    now_ms: i64,
    level: KioskLogLevel,
    code: &str,
    detail: &str,
) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskLog,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskLog(KioskLog {
            level,
            code: code.to_owned(),
            detail: detail.to_owned(),
        }),
    }
}

/// A `kiosk.scan_reported` intent from `device_id`, minted at `now_ms` (when the scan finished),
/// so the offline replay delivers it exactly once.
pub fn kiosk_scan_reported(device_id: &str, now_ms: i64, report: KioskScanReported) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskScanReported,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskScanReported(report),
    }
}

/// A `kiosk.process_blocked` intent from `device_id`, minted at `now_ms` (the moment of the
/// kill), so the offline replay delivers it exactly once.
pub fn kiosk_process_blocked(device_id: &str, now_ms: i64, blocked: KioskProcessBlocked) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskProcessBlocked,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskProcessBlocked(blocked),
    }
}

/// A `device.admin_exit` intent from `device_id`, minted at `now_ms` (the moment the exit was
/// granted), elevated by the manager whose PIN was checked (`mode`: at the Hub, or offline
/// against the cached verifier of `verifier_version`).
#[allow(clippy::too_many_arguments)]
pub fn device_admin_exit(
    device_id: &str,
    now_ms: i64,
    exit_id: &str,
    mode: arena_proto::AdminExitMode,
    staff_id: &str,
    elevation: arena_proto::ElevationMode,
    verifier_version: i64,
) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::DeviceAdminExit,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: Some(arena_proto::ElevatedBy {
            kind: arena_proto::ElevationKind::Pin,
            id: staff_id.to_owned(),
            mode: elevation,
            verifier_version,
        }),
        reason: None,
        payload: IntentPayload::DeviceAdminExit(arena_proto::DeviceAdminExit {
            exit_id: exit_id.to_owned(),
            mode,
        }),
    }
}

pub fn device_test_session_locked(device_id: &str, now_ms: i64) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::DeviceTestSessionCompleted,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::DeviceTestSessionCompleted(
            arena_proto::DeviceTestSessionCompleted {
                device_id: device_id.to_owned(),
                outcome: arena_proto::TestSessionOutcome::Locked,
            },
        ),
    }
}

/// A `device.admin_relock` intent from `device_id`, minted at `now_ms` (when it re-locked).
pub fn device_admin_relock(device_id: &str, now_ms: i64, exit_id: &str) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::DeviceAdminRelock,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::DeviceAdminRelock(arena_proto::DeviceAdminRelock {
            exit_id: exit_id.to_owned(),
        }),
    }
}

/// A `kiosk.cleanup_failed` intent from `device_id`, minted at `now_ms` (when the step failed
/// its retry), so the offline replay delivers it exactly once.
pub fn kiosk_cleanup_failed(
    device_id: &str,
    now_ms: i64,
    failed: arena_proto::KioskCleanupFailed,
) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskCleanupFailed,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskCleanupFailed(failed),
    }
}

/// A `launcher.opened` intent from `device_id`, minted at `now_ms` (when the player opened it), so
/// the offline replay delivers it exactly once. The Hub names the player and session itself.
pub fn launcher_opened(device_id: &str, now_ms: i64, launcher: &str) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskLauncherOpened,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskLauncherOpened(arena_proto::KioskLauncherOpened {
            launcher: launcher.to_owned(),
        }),
    }
}

/// A `game.launched` intent from `device_id`, minted at `now_ms` (when the game started), so the
/// offline replay delivers it exactly once. The Hub names the player and session itself.
pub fn game_launched(
    device_id: &str,
    now_ms: i64,
    launched: arena_proto::KioskGameLaunched,
) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskGameLaunched,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskGameLaunched(launched),
    }
}

/// A `game.play_recorded` intent from `device_id`, minted at `now_ms` (when the game stopped), so
/// the offline replay delivers it exactly once. The Hub names the player and session itself.
pub fn game_play_recorded(
    device_id: &str,
    now_ms: i64,
    play: arena_proto::KioskGamePlayRecorded,
) -> Intent {
    Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::KioskGamePlayRecorded,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now_ms,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Device,
            id: device_id.to_owned(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::KioskGamePlayRecorded(play),
    }
}

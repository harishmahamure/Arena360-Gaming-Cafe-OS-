//! The intents the kiosk creates. Each gets its ULID ids and `issued_at` here, when the event
//! happens, so a replay is the very same intent and the Hub's dedupe applies it once.

use arena_proto::{Actor, ActorKind, Intent, IntentKind, IntentPayload, KioskLog, KioskLogLevel};

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

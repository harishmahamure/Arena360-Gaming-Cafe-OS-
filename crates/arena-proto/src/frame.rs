//! The LAN protocol envelope: every WebSocket text frame is UTF-8 JSON
//! `{v: 1, kind, rid, ...body}`.
//!
//! - Unknown fields are ignored.
//! - An unparseable frame fails `BAD_FRAME`; an unknown `kind` or a `v` other than 1 fails
//!   `UNSUPPORTED`. Both answer with the frame's `rid` when it could be read, and neither
//!   closes the session.
//! - The first frame of a session must be [`Hello`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::admin::{AdminPinOk, AdminUnlockAck, ManagerVerifier, VerifyAdminPin};
use crate::error::{AppError, ErrorCode};
use crate::intent::Intent;
use crate::pc_command::{LockReason, PcAck, PcAction};
use crate::scope::ClientClass;
use crate::session::SessionScreen;
use crate::ui_auth::SettingEntry;

/// The only protocol version.
pub const PROTOCOL_VERSION: u32 = 1;
/// Longest accepted `rid`, in bytes.
pub const MAX_RID_LEN: usize = 128;

/// One frame: the envelope plus its kind-tagged body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Frame {
    /// Always [`PROTOCOL_VERSION`].
    pub v: u32,
    /// The request id: a reply carries the rid of its request; a push carries a Hub-minted one.
    pub rid: String,
    #[serde(flatten)]
    pub body: FrameBody,
}

/// A frame's body, tagged by `kind`.
// Frames are short-lived and decoded one at a time; boxing the intent would only add an
// allocation per submit.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum FrameBody {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "hello.ok")]
    HelloOk(HelloOk),
    #[serde(rename = "intent.submit")]
    IntentSubmit(IntentSubmit),
    #[serde(rename = "intent.result")]
    IntentResult(IntentResult),
    #[serde(rename = "event")]
    Event(EventFrame),
    #[serde(rename = "error")]
    Error(ErrorFrame),
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "settings.get")]
    SettingsGet(SettingsGet),
    #[serde(rename = "settings.values")]
    SettingsValues(SettingsValues),
    #[serde(rename = "kiosk.verify_admin_pin")]
    VerifyAdminPin(VerifyAdminPin),
    #[serde(rename = "kiosk.admin_pin_ok")]
    AdminPinOk(AdminPinOk),
    #[serde(rename = "kiosk.admin_unlock_ack")]
    AdminUnlockAck(AdminUnlockAck),
    #[serde(rename = "kiosk.pc_ack")]
    PcAck(PcAck),
    /// The Hub's empty reply to a request that has nothing to return.
    #[serde(rename = "ack")]
    Ack,
}

/// Every known `kind`.
pub const FRAME_KINDS: [&str; 15] = [
    "hello",
    "hello.ok",
    "intent.submit",
    "intent.result",
    "event",
    "error",
    "ping",
    "pong",
    "settings.get",
    "settings.values",
    "kiosk.verify_admin_pin",
    "kiosk.admin_pin_ok",
    "kiosk.admin_unlock_ack",
    "kiosk.pc_ack",
    "ack",
];

impl FrameBody {
    /// The `kind` this body is sent as.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello(_) => "hello",
            Self::HelloOk(_) => "hello.ok",
            Self::IntentSubmit(_) => "intent.submit",
            Self::IntentResult(_) => "intent.result",
            Self::Event(_) => "event",
            Self::Error(_) => "error",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::SettingsGet(_) => "settings.get",
            Self::SettingsValues(_) => "settings.values",
            Self::VerifyAdminPin(_) => "kiosk.verify_admin_pin",
            Self::AdminPinOk(_) => "kiosk.admin_pin_ok",
            Self::AdminUnlockAck(_) => "kiosk.admin_unlock_ack",
            Self::PcAck(_) => "kiosk.pc_ack",
            Self::Ack => "ack",
        }
    }
}

/// App versions, as `x.y.z`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Versions {
    pub app: String,
}

/// The mandatory first frame of every session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Hello {
    /// The device token (`paired`, `kds`); ignored for `phone`.
    #[serde(default)]
    pub credential: String,
    pub class: ClientClass,
    pub versions: Versions,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// The Hub's answer to an accepted [`Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct HelloOk {
    pub class: ClientClass,
    /// The authenticated device (`paired`, `kds`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_id: Option<String>,
    /// The Hub's own version.
    pub versions: Versions,
    /// Whether this Hub is the branch's primary writer; a follower refuses every intent with
    /// `NOT_PRIMARY`.
    pub primary: bool,
}

/// Asks the Hub to apply an intent. The Hub overwrites `intent.actor` with the session's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct IntentSubmit {
    pub intent: Intent,
}

/// What a written row's change was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ChangeOpKind {
    Insert,
    Update,
    Delete,
}

/// One change row written by an intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ChangeSummary {
    pub epoch: String,
    #[ts(type = "number")]
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
    pub op: ChangeOpKind,
}

/// A committed intent and the change rows it wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AppliedSummary {
    pub intent_id: String,
    pub changes: Vec<ChangeSummary>,
}

/// The reply to a successful `intent.submit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct IntentResult {
    pub applied: AppliedSummary,
}

/// Most keys one `settings.get` may ask for.
pub const MAX_SETTINGS_GET_KEYS: usize = 32;

/// Asks the Hub for the current value of branch settings. Paired sessions only, and only
/// `kiosk.*` keys (`FORBIDDEN` otherwise); the reply is [`SettingsValues`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettingsGet {
    pub keys: Vec<String>,
}

/// Branch setting values: the reply to `settings.get` (keys the branch lacks are left out) and
/// the body of the pushed `settings.changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SettingsValues {
    pub values: Vec<SettingEntry>,
}

/// A state change pushed by the Hub. Clients never poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum HubEvent {
    /// An intent was committed (by any client or by the Hub itself).
    #[serde(rename = "applied")]
    Applied(AppliedSummary),
    /// An intent wrote `kiosk.*` settings: their values now (paired sessions only).
    #[serde(rename = "settings.changed")]
    SettingsChanged(SettingsValues),
    /// The manager verifier for offline admin exits (paired sessions only): on hello and
    /// whenever it changes.
    #[serde(rename = "verifier")]
    Verifier { verifier: ManagerVerifier },
    /// A manager unlocked this kiosk remotely: grant the exit and answer
    /// `kiosk.admin_unlock_ack` (to that device's paired sessions only).
    #[serde(rename = "admin_unlock")]
    AdminUnlock { exit_id: String },
    #[serde(rename = "pc_command")]
    PcCommand {
        cmd_id: String,
        action: PcAction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        reason: Option<LockReason>,
    },
    #[serde(rename = "session.started")]
    SessionStarted {
        cmd_id: String,
        session: SessionScreen,
    },
    #[serde(rename = "session.tick")]
    SessionTick(SessionScreen),
    /// 10, 5 or 1 minute remain on the device's session (not persisted, not resent).
    #[serde(rename = "session.warning")]
    SessionWarning {
        session_id: String,
        minutes_left: u8,
    },
    #[serde(rename = "catalog.changed")]
    CatalogChanged { products: Vec<crate::CatalogEntry> },
    #[serde(rename = "test_session")]
    TestSession {
        #[ts(type = "number")]
        lock_after_ms: u64,
    },
    /// What this device's idle screen shows: branch, this PC's rate and the pricing rule in
    /// effect, and the library's curated videos (Story 6.7). Pushed to one device on every
    /// fan-out tick (rules start and end by the clock, not by an intent).
    #[serde(rename = "attract")]
    Attract(Box<crate::AttractView>),
}

/// The body of an `event` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EventFrame {
    pub event: HubEvent,
}

/// The body of an `error` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ErrorFrame {
    pub error: AppError,
}

impl Frame {
    /// A frame of the current protocol version.
    pub fn new(rid: impl Into<String>, body: FrameBody) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            rid: rid.into(),
            body,
        }
    }

    /// An `error` frame.
    pub fn error(rid: impl Into<String>, error: AppError) -> Self {
        Self::new(rid, FrameBody::Error(ErrorFrame { error }))
    }

    /// The frame as JSON text.
    pub fn to_text(&self) -> String {
        // Every field is a string, number, bool, list or map: serialization cannot fail.
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"v":1,"kind":"error","rid":"","error":{"code":"INTERNAL","message":"encode"}}"#
                .to_owned()
        })
    }
}

/// A frame that could not be accepted: the error to answer with, and the `rid` to answer on
/// (empty when the frame carried none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    pub rid: String,
    pub error: AppError,
}

impl FrameError {
    fn new(rid: &str, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            rid: rid.to_owned(),
            error: AppError::new(code, message),
        }
    }

    /// The `error` frame answering this failure.
    pub fn to_frame(&self) -> Frame {
        Frame::error(self.rid.clone(), self.error.clone())
    }
}

/// The envelope of a frame that passed [`check_envelope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope<'a> {
    pub rid: &'a str,
    pub kind: &'a str,
}

/// Parses frame text into JSON (`BAD_FRAME` when it is not a JSON object).
pub fn parse_json(text: &str) -> Result<Value, FrameError> {
    match serde_json::from_str::<Value>(text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err(FrameError::new(
            "",
            ErrorCode::BadFrame,
            "a frame must be a JSON object",
        )),
        Err(_) => Err(FrameError::new("", ErrorCode::BadFrame, "malformed JSON")),
    }
}

/// Checks `rid`, `v` and `kind`: a missing or ill-typed one is `BAD_FRAME`, a `v` other than
/// 1 or an unknown `kind` is `UNSUPPORTED`.
pub fn check_envelope(value: &Value) -> Result<Envelope<'_>, FrameError> {
    let rid = match value.get("rid") {
        Some(Value::String(r)) if r.len() <= MAX_RID_LEN => r.as_str(),
        _ => {
            return Err(FrameError::new(
                "",
                ErrorCode::BadFrame,
                "rid must be a string of at most 128 bytes",
            ));
        }
    };
    match value.get("v").map(Value::as_u64) {
        Some(Some(v)) if v == u64::from(PROTOCOL_VERSION) => {}
        Some(Some(v)) => {
            return Err(FrameError::new(
                rid,
                ErrorCode::Unsupported,
                format!("protocol version {v} is not supported (only 1)"),
            ));
        }
        _ => {
            return Err(FrameError::new(
                rid,
                ErrorCode::BadFrame,
                "v must be a non-negative integer",
            ));
        }
    }
    let kind = match value.get("kind") {
        Some(Value::String(k)) => k.as_str(),
        _ => return Err(FrameError::new(rid, ErrorCode::BadFrame, "kind is missing")),
    };
    if !FRAME_KINDS.contains(&kind) {
        return Err(FrameError::new(
            rid,
            ErrorCode::Unsupported,
            format!("unsupported kind {kind:?}"),
        ));
    }
    Ok(Envelope { rid, kind })
}

/// Parses a JSON frame that passed [`check_envelope`] into its typed form (`BAD_FRAME` when
/// the body does not fit its kind).
pub fn parse_frame_value(value: Value) -> Result<Frame, FrameError> {
    let rid = check_envelope(&value)?.rid.to_owned();
    serde_json::from_value::<Frame>(value).map_err(|e| FrameError {
        rid,
        error: AppError::new(ErrorCode::BadFrame, format!("malformed frame body: {e}")),
    })
}

/// Parses frame text: [`parse_json`], [`check_envelope`], then [`parse_frame_value`].
pub fn parse_frame(text: &str) -> Result<Frame, FrameError> {
    parse_frame_value(parse_json(text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hello() -> Frame {
        Frame::new(
            "1",
            FrameBody::Hello(Hello {
                credential: "tok".into(),
                class: ClientClass::Paired,
                versions: Versions {
                    app: "0.1.0".into(),
                },
                capabilities: vec!["events".into()],
            }),
        )
    }

    #[test]
    fn hello_round_trips_flat() {
        let f = hello();
        let v: Value = serde_json::from_str(&f.to_text()).unwrap();
        assert_eq!(
            v,
            json!({"v": 1, "kind": "hello", "rid": "1", "credential": "tok", "class": "paired",
                   "versions": {"app": "0.1.0"}, "capabilities": ["events"]})
        );
        assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text = r#"{"v":1,"kind":"hello","rid":"1","credential":"tok","class":"paired",
                       "versions":{"app":"0.1.0","extra":true},"capabilities":["events"],"x":1}"#;
        assert_eq!(parse_frame(text).unwrap(), hello());
        let ping = parse_frame(r#"{"v":1,"kind":"ping","rid":"p","x":1}"#).unwrap();
        assert_eq!(ping, Frame::new("p", FrameBody::Ping));
        assert_eq!(
            serde_json::to_value(&ping).unwrap(),
            json!({"v": 1, "kind": "ping", "rid": "p"})
        );
    }

    #[test]
    fn envelope_errors() {
        let err = |t: &str| parse_frame(t).unwrap_err();
        let e = err("{not json");
        assert_eq!((e.rid.as_str(), e.error.code), ("", ErrorCode::BadFrame));
        assert_eq!(err("[1]").error.code, ErrorCode::BadFrame);
        let e = err(r#"{"v":1,"kind":"foo","rid":"7"}"#);
        assert_eq!(
            (e.rid.as_str(), e.error.code),
            ("7", ErrorCode::Unsupported)
        );
        let e = err(r#"{"v":2,"kind":"ping","rid":"8"}"#);
        assert_eq!(
            (e.rid.as_str(), e.error.code),
            ("8", ErrorCode::Unsupported)
        );
        assert_eq!(
            err(r#"{"kind":"ping","rid":"9"}"#).error.code,
            ErrorCode::BadFrame
        );
        assert_eq!(
            err(r#"{"v":1,"kind":"ping"}"#).error.code,
            ErrorCode::BadFrame
        );
        assert_eq!(err(r#"{"v":1,"rid":"1"}"#).error.code, ErrorCode::BadFrame);
        let e = err(r#"{"v":1,"kind":"hello","rid":"3","class":"robot"}"#);
        assert_eq!((e.rid.as_str(), e.error.code), ("3", ErrorCode::BadFrame));
        let long = format!(r#"{{"v":1,"kind":"ping","rid":"{}"}}"#, "r".repeat(129));
        assert_eq!(err(&long).error.code, ErrorCode::BadFrame);
    }

    #[test]
    fn settings_frames_round_trip() {
        let get = parse_frame(
            r#"{"v":1,"kind":"settings.get","rid":"s1","keys":["kiosk.process_allow"]}"#,
        )
        .unwrap();
        assert_eq!(
            get.body,
            FrameBody::SettingsGet(SettingsGet {
                keys: vec!["kiosk.process_allow".into()]
            })
        );
        let values = SettingsValues {
            values: vec![SettingEntry {
                key: "kiosk.process_allow".into(),
                value: "[]".into(),
                rev: 2,
            }],
        };
        let reply = Frame::new("s1", FrameBody::SettingsValues(values.clone()));
        assert_eq!(parse_frame(&reply.to_text()).unwrap(), reply);
        let ev = Frame::new(
            "e",
            FrameBody::Event(EventFrame {
                event: HubEvent::SettingsChanged(values),
            }),
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["event"]["kind"], "settings.changed");
        assert_eq!(v["event"]["values"][0]["rev"], 2);
        assert_eq!(parse_frame(&ev.to_text()).unwrap(), ev);
    }

    #[test]
    fn admin_exit_frames_round_trip() {
        let verify =
            parse_frame(r#"{"v":1,"kind":"kiosk.verify_admin_pin","rid":"a","pin_proof":"2222"}"#)
                .unwrap();
        assert!(matches!(verify.body, FrameBody::VerifyAdminPin(_)));
        assert!(!format!("{verify:?}").contains("2222"));
        for body in [
            FrameBody::AdminPinOk(AdminPinOk {
                staff_id: "S".into(),
                verifier_version: 3,
            }),
            FrameBody::AdminUnlockAck(AdminUnlockAck {
                exit_id: "E".into(),
            }),
            FrameBody::Ack,
        ] {
            let f = Frame::new("r", body);
            assert_eq!(parse_frame(&f.to_text()).unwrap(), f);
        }
        let ev = Frame::new(
            "e",
            FrameBody::Event(EventFrame {
                event: HubEvent::Verifier {
                    verifier: ManagerVerifier {
                        version: 4,
                        entries: vec![],
                    },
                },
            }),
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["event"]["kind"], "verifier");
        assert_eq!(v["event"]["verifier"]["version"], 4);
        assert_eq!(parse_frame(&ev.to_text()).unwrap(), ev);
        let unlock = Frame::new(
            "e",
            FrameBody::Event(EventFrame {
                event: HubEvent::AdminUnlock {
                    exit_id: "X".into(),
                },
            }),
        );
        let v = serde_json::to_value(&unlock).unwrap();
        assert_eq!(
            v["event"],
            serde_json::json!({"kind": "admin_unlock", "exit_id": "X"})
        );
    }

    #[test]
    fn session_events_carry_the_screen_and_a_lock_its_reason() {
        let screen = SessionScreen {
            session_id: "S".into(),
            device_name: "PC 1".into(),
            elapsed_ms: 5,
            lock_after_ms: 9,
            time_up_mode: "clean_up".into(),
            grace_ms: 0,
            ordered_items: 0,
        };
        let tick = serde_json::to_value(HubEvent::SessionTick(screen.clone())).unwrap();
        assert_eq!(tick["kind"], "session.tick");
        assert_eq!(tick["lock_after_ms"], 9);
        let started = HubEvent::SessionStarted {
            cmd_id: "C".into(),
            session: screen,
        };
        let v = serde_json::to_value(&started).unwrap();
        assert_eq!(v["kind"], "session.started");
        assert_eq!(v["session"]["device_name"], "PC 1");
        assert_eq!(serde_json::from_value::<HubEvent>(v).unwrap(), started);
        let lock = HubEvent::PcCommand {
            cmd_id: "C".into(),
            action: PcAction::Lock,
            text: None,
            reason: Some(LockReason::PayAtCounter),
        };
        assert_eq!(
            serde_json::to_value(&lock).unwrap()["reason"],
            "pay_at_counter"
        );
    }

    #[test]
    fn pc_command_event_and_ack_round_trip() {
        let ev = Frame::new(
            "e",
            FrameBody::Event(EventFrame {
                event: HubEvent::PcCommand {
                    cmd_id: "C".into(),
                    action: PcAction::Message,
                    text: Some("hi".into()),
                    reason: None,
                },
            }),
        );
        assert_eq!(
            serde_json::to_value(&ev).unwrap()["event"],
            json!({"kind": "pc_command", "cmd_id": "C", "action": "message", "text": "hi"})
        );
        assert_eq!(parse_frame(&ev.to_text()).unwrap(), ev);
        let ack = Frame::new("r", FrameBody::PcAck(PcAck { cmd_id: "C".into() }));
        assert_eq!(parse_frame(&ack.to_text()).unwrap(), ack);
    }

    #[test]
    fn test_session_event_round_trips() {
        let ev = Frame::new(
            "e",
            FrameBody::Event(EventFrame {
                event: HubEvent::TestSession {
                    lock_after_ms: 120_000,
                },
            }),
        );
        assert_eq!(
            serde_json::to_value(&ev).unwrap()["event"],
            serde_json::json!({"kind": "test_session", "lock_after_ms": 120000})
        );
        assert_eq!(parse_frame(&ev.to_text()).unwrap(), ev);
    }

    #[test]
    fn error_and_event_frames_nest_their_bodies() {
        let f = Frame::error("5", AppError::new(ErrorCode::TokenInvalid, "no"));
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            json!({"v": 1, "kind": "error", "rid": "5",
                   "error": {"code": "TOKEN_INVALID", "message": "no"}})
        );
        let ev = Frame::new(
            "e1",
            FrameBody::Event(EventFrame {
                event: HubEvent::Applied(AppliedSummary {
                    intent_id: "I".into(),
                    changes: vec![ChangeSummary {
                        epoch: "1:H".into(),
                        seq: 3,
                        tbl: "devices".into(),
                        row_id: "D".into(),
                        op: ChangeOpKind::Update,
                    }],
                }),
            }),
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "event");
        assert_eq!(v["event"]["kind"], "applied");
        assert_eq!(v["event"]["changes"][0]["op"], "update");
        assert_eq!(parse_frame(&ev.to_text()).unwrap(), ev);
        assert_eq!(ev.body.kind(), "event");
    }
}

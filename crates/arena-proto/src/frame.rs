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

use crate::error::{AppError, ErrorCode};
use crate::intent::Intent;
use crate::scope::ClientClass;

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
}

/// Every known `kind`.
pub const FRAME_KINDS: [&str; 8] = [
    "hello",
    "hello.ok",
    "intent.submit",
    "intent.result",
    "event",
    "error",
    "ping",
    "pong",
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

/// A state change pushed by the Hub. Clients never poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum HubEvent {
    /// An intent was committed (by any client or by the Hub itself).
    #[serde(rename = "applied")]
    Applied(AppliedSummary),
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

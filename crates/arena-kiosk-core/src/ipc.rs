//! The local channel between the Kiosk service (`arena-kiosk-svc`, privileged) and the Kiosk
//! app (`arena-kiosk-ui`, the unprivileged player session): one JSON object per line.
//!
//! The service sends `ping`, `hub_status`, `restore_state`, `toast`, `admin_exit_started`,
//! `admin_exit_ended`, `test_session_started`, `test_session_ended`, `pc_screen` and `reply`; the app sends `pong`, `app_state`, `request` and
//! `admin_exit` (a manager PIN, answered with `reply`; the [`SecretPin`] never prints). The app never talks to the Hub: everything it needs from the Hub
//! goes through a `request` the service answers. A line longer than [`MAX_LINE_BYTES`] or that
//! does not parse is an error the reader skips (the connection stays up).

use arena_proto::{AppError, DiscoveredHub, KioskHubState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// The longest line either side accepts, newline included.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The Windows named pipe the service listens on (the name is the same on every branch).
pub const PIPE_NAME: &str = r"\\.\pipe\arena-kiosk-v1";
/// The Unix socket file name (non-Windows dev and test builds), inside [`SOCKET_DIR`].
pub const SOCKET_FILE: &str = "kiosk.sock";
/// The 0700 directory inside the data directory that holds the socket.
pub const SOCKET_DIR: &str = "ipc";

/// The dev socket under `data_dir`: `<data>/ipc/kiosk.sock`.
pub fn socket_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join(SOCKET_DIR).join(SOCKET_FILE)
}

/// The dev data directory (non-Windows builds): `$ARENA_KIOSK_DATA_DIR`, else
/// `~/.arena360/kiosk`. The service and the app both find the socket from it.
pub fn dev_data_dir(env: impl Fn(&str) -> Option<String>) -> std::path::PathBuf {
    if let Some(dir) = env("ARENA_KIOSK_DATA_DIR").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    let home = env("HOME").unwrap_or_else(|| ".".into());
    std::path::PathBuf::from(home)
        .join(".arena360")
        .join("kiosk")
}

/// The catalogue key of the toast shown after the service killed a disallowed program.
pub const TOAST_APP_BLOCKED: &str = "player.app.blocked";

/// A PIN on its way from the app to the service: it never shows in `Debug` output, and the
/// service clears it right after checking it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretPin(String);

impl SecretPin {
    pub fn new(pin: impl Into<String>) -> Self {
        Self(pin.into())
    }

    /// The PIN, for the one check that needs it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Overwrites and drops the PIN.
    pub fn clear(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.iter_mut().for_each(|b| *b = 0);
        std::hint::black_box(&bytes);
    }
}

impl Drop for SecretPin {
    fn drop(&mut self) {
        self.clear();
    }
}

impl std::fmt::Debug for SecretPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretPin([redacted])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcSession {
    pub device_name: String,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_min: Option<u8>,
    /// Milliseconds until the grace after the end time begins (`None` without a grace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_in_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub grace_ms: u64,
    /// How many items were placed on the session's bill (shown as "Ordered: n items").
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ordered_items: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One IPC message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMsg {
    /// Service → app, every 2 s: the app answers `pong` with the same `n`.
    Ping {
        n: u64,
    },
    /// App → service.
    Pong {
        n: u64,
    },
    /// Service → app: the Hub link changed (also sent once on every new connection).
    HubStatus {
        state: KioskHubState,
    },
    /// Service → app: whether this kiosk uses the high-contrast theme (its
    /// `display.hc.<device_id>` setting). Sent on every new connection, right after
    /// `hub_status`, and whenever it changes.
    Theme {
        hc: bool,
    },
    /// App → service: the app's restorable state (a JSON value the UI owns), sent whenever it
    /// changes.
    AppState {
        json: Value,
    },
    /// Service → app, on connect: the last `app_state` the service saw, to restore after a
    /// restart.
    RestoreState {
        json: Value,
    },
    /// App → service: a request (`kind`, e.g. `log`) the service answers with `reply`.
    Request {
        rid: String,
        kind: String,
        #[serde(default)]
        body: Value,
    },
    /// Service → app: show the player the catalogue line `key` as a toast that stays until
    /// dismissed (the same key again does not stack a second one).
    Toast {
        key: String,
    },
    /// App → service: a manager PIN for an admin exit (the hidden Ctrl+Alt+Shift+A panel). The
    /// answer is `reply` `rid`: `{until}` when granted, else `PIN_REJECTED` or `AUTH_LOCKED`
    /// with `{retry_after_s}`.
    AdminExit {
        rid: String,
        pin: SecretPin,
    },
    /// Service → app: an admin exit runs until `until` (UTC ms): drop topmost and the keyboard
    /// lockdown and show the countdown badge. Also sent on every connection while one runs.
    AdminExitStarted {
        until: i64,
    },
    /// Service → app: the exit is over: cover every screen and lock down again.
    AdminExitEnded,
    TestSessionStarted {
        remaining_ms: u64,
    },
    TestSessionEnded,
    PcScreen {
        locked: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<arena_proto::LockReason>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<PcSession>,
    },
    #[serde(rename = "discovery.found")]
    DiscoveryFound {
        search: u64,
        hubs: Vec<DiscoveredHub>,
    },
    #[serde(rename = "discovery.done")]
    DiscoveryDone {
        search: u64,
        hubs: Vec<DiscoveredHub>,
    },
    /// Service → app: the answer to `request` `rid`: `body` on success, `error` otherwise.
    Reply {
        rid: String,
        #[serde(default)]
        body: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<AppError>,
    },
}

/// Why a line could not be read.
#[derive(Debug)]
pub enum IpcError {
    /// The peer closed the connection (or an I/O error ended it).
    Closed,
    /// A line over [`MAX_LINE_BYTES`] (the rest of it was discarded).
    TooLong,
    /// The line is not one [`IpcMsg`].
    Bad(String),
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("the ipc connection is closed"),
            Self::TooLong => write!(f, "an ipc line is longer than {MAX_LINE_BYTES} bytes"),
            Self::Bad(m) => write!(f, "a bad ipc line: {m}"),
        }
    }
}

impl std::error::Error for IpcError {}

impl IpcMsg {
    /// The message as one line, newline included.
    pub fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        s
    }

    /// Parses one line (with or without its newline).
    pub fn parse(line: &str) -> Result<Self, IpcError> {
        serde_json::from_str(line.trim_end_matches(['\r', '\n']))
            .map_err(|e| IpcError::Bad(e.to_string()))
    }
}

/// Reads the next message. An over-long or malformed line is an error that leaves the stream
/// positioned at the next line, so the caller may skip it and read on.
pub async fn read_msg<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<IpcMsg, IpcError> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = reader.fill_buf().await.map_err(|_| IpcError::Closed)?;
        if chunk.is_empty() {
            return Err(IpcError::Closed);
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > MAX_LINE_BYTES {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        reader.consume(take);
        if done {
            break;
        }
    }
    if too_long {
        return Err(IpcError::TooLong);
    }
    let text = std::str::from_utf8(&buf).map_err(|e| IpcError::Bad(e.to_string()))?;
    IpcMsg::parse(text)
}

/// Writes one message and flushes it.
pub async fn write_msg<W: AsyncWrite + Unpin>(writer: &mut W, msg: &IpcMsg) -> std::io::Result<()> {
    writer.write_all(msg.to_line().as_bytes()).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[test]
    fn messages_are_tagged_json_lines() {
        let line = IpcMsg::HubStatus {
            state: KioskHubState::Reconnecting,
        }
        .to_line();
        assert_eq!(
            line,
            "{\"type\":\"hub_status\",\"state\":\"reconnecting\"}\n"
        );
        assert_eq!(
            IpcMsg::parse("{\"type\":\"ping\",\"n\":3}").unwrap(),
            IpcMsg::Ping { n: 3 }
        );
        let state = IpcMsg::AppState {
            json: serde_json::json!({"route": "/play"}),
        };
        assert_eq!(IpcMsg::parse(&state.to_line()).unwrap(), state);
        let reply = IpcMsg::Reply {
            rid: "1".into(),
            body: Value::Null,
            error: None,
        };
        assert_eq!(
            reply.to_line(),
            "{\"type\":\"reply\",\"rid\":\"1\",\"body\":null}\n"
        );
        // Unknown fields are ignored; unknown types are not.
        assert_eq!(
            IpcMsg::parse("{\"type\":\"pong\",\"n\":1,\"x\":2}").unwrap(),
            IpcMsg::Pong { n: 1 }
        );
        assert!(IpcMsg::parse("{\"type\":\"shell\"}").is_err());
        assert_eq!(
            IpcMsg::Toast {
                key: TOAST_APP_BLOCKED.into()
            }
            .to_line(),
            "{\"type\":\"toast\",\"key\":\"player.app.blocked\"}\n"
        );
        assert_eq!(
            IpcMsg::Theme { hc: true }.to_line(),
            "{\"type\":\"theme\",\"hc\":true}\n"
        );
        assert_eq!(
            IpcMsg::parse("{\"type\":\"theme\",\"hc\":false}").unwrap(),
            IpcMsg::Theme { hc: false }
        );
    }

    #[test]
    fn admin_exit_messages_never_print_the_pin() {
        let msg = IpcMsg::AdminExit {
            rid: "a".into(),
            pin: SecretPin::new("2222"),
        };
        assert!(!format!("{msg:?}").contains("2222"));
        assert_eq!(
            msg.to_line(),
            "{\"type\":\"admin_exit\",\"rid\":\"a\",\"pin\":\"2222\"}\n"
        );
        assert_eq!(IpcMsg::parse(&msg.to_line()).unwrap(), msg);
        assert_eq!(
            IpcMsg::AdminExitStarted { until: 5 }.to_line(),
            "{\"type\":\"admin_exit_started\",\"until\":5}\n"
        );
        assert_eq!(
            IpcMsg::AdminExitEnded.to_line(),
            "{\"type\":\"admin_exit_ended\"}\n"
        );
        let mut pin = SecretPin::new("1234");
        pin.clear();
        assert_eq!(pin.expose(), "");
    }

    #[test]
    fn test_session_messages_are_tagged_lines() {
        assert_eq!(
            IpcMsg::TestSessionStarted {
                remaining_ms: 120_000
            }
            .to_line(),
            "{\"type\":\"test_session_started\",\"remaining_ms\":120000}\n"
        );
        assert_eq!(
            IpcMsg::parse("{\"type\":\"test_session_ended\"}").unwrap(),
            IpcMsg::TestSessionEnded
        );
    }

    #[test]
    fn the_pc_screen_omits_an_absent_message() {
        assert_eq!(
            IpcMsg::PcScreen {
                locked: true,
                reason: None,
                message: None,
                session: None
            }
            .to_line(),
            "{\"type\":\"pc_screen\",\"locked\":true}\n"
        );
        let shown = IpcMsg::PcScreen {
            locked: false,
            reason: Some(arena_proto::LockReason::PayAtCounter),
            message: Some("hi".into()),
            session: Some(PcSession {
                device_name: "PC 1".into(),
                elapsed_ms: 7,
                warning_min: Some(5),
                grace_in_ms: Some(3),
                grace_ms: 4,
                ordered_items: 2,
            }),
        };
        assert_eq!(IpcMsg::parse(&shown.to_line()).unwrap(), shown);
    }

    #[test]
    fn discovery_events_are_dotted() {
        let hub = DiscoveredHub {
            addr: "10.0.0.2:7400".into(),
            branch_name: "B".into(),
            hub_name: "H".into(),
            fingerprint: "ab".repeat(32),
            protocol_version: 1,
            compatible: true,
            web_port: 7401,
        };
        let found = IpcMsg::DiscoveryFound {
            search: 3,
            hubs: vec![hub.clone()],
        };
        let v: Value = serde_json::from_str(&found.to_line()).unwrap();
        assert_eq!(v["type"], "discovery.found");
        assert_eq!(v["hubs"][0]["addr"], "10.0.0.2:7400");
        assert_eq!(v["search"], 3);
        assert_eq!(IpcMsg::parse(&found.to_line()).unwrap(), found);
        assert_eq!(
            IpcMsg::DiscoveryDone {
                search: 1,
                hubs: vec![]
            }
            .to_line(),
            "{\"type\":\"discovery.done\",\"search\":1,\"hubs\":[]}\n"
        );
    }

    #[tokio::test]
    async fn the_reader_skips_bad_and_over_long_lines() {
        let long = "x".repeat(MAX_LINE_BYTES + 10);
        let input = format!(
            "{{\"type\":\"ping\",\"n\":1}}\nnot json\n{long}\n{{\"type\":\"pong\",\"n\":2}}\n"
        );
        let mut r = BufReader::with_capacity(1024, input.as_bytes());
        assert_eq!(read_msg(&mut r).await.unwrap(), IpcMsg::Ping { n: 1 });
        assert!(matches!(read_msg(&mut r).await, Err(IpcError::Bad(_))));
        assert!(matches!(read_msg(&mut r).await, Err(IpcError::TooLong)));
        assert_eq!(read_msg(&mut r).await.unwrap(), IpcMsg::Pong { n: 2 });
        assert!(matches!(read_msg(&mut r).await, Err(IpcError::Closed)));
    }

    #[tokio::test]
    async fn write_then_read_round_trips() {
        let (a, b) = tokio::io::duplex(4096);
        let (_ra, mut wa) = tokio::io::split(a);
        let (rb, _wb) = tokio::io::split(b);
        let msg = IpcMsg::RestoreState {
            json: serde_json::json!({"route": "/"}),
        };
        write_msg(&mut wa, &msg).await.unwrap();
        let mut rb = BufReader::new(rb);
        assert_eq!(read_msg(&mut rb).await.unwrap(), msg);
    }
}

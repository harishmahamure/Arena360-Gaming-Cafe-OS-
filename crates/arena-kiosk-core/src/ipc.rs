//! The local channel between the Kiosk service (`arena-kiosk-svc`, privileged) and the Kiosk
//! app (`arena-kiosk-ui`, the unprivileged player session): one JSON object per line.
//!
//! The service sends `ping`, `hub_status`, `restore_state`, `toast` and `reply`; the app sends
//! `pong`, `app_state` and `request`. The app never talks to the Hub: everything it needs from the Hub
//! goes through a `request` the service answers. A line longer than [`MAX_LINE_BYTES`] or that
//! does not parse is an error the reader skips (the connection stays up).

use arena_proto::{AppError, KioskHubState};
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

/// One IPC message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMsg {
    /// Service → app, every 2 s: the app answers `pong` with the same `n`.
    Ping { n: u64 },
    /// App → service.
    Pong { n: u64 },
    /// Service → app: the Hub link changed (also sent once on every new connection).
    HubStatus { state: KioskHubState },
    /// App → service: the app's restorable state (a JSON value the UI owns), sent whenever it
    /// changes.
    AppState { json: Value },
    /// Service → app, on connect: the last `app_state` the service saw, to restore after a
    /// restart.
    RestoreState { json: Value },
    /// App → service: a request (`kind`, e.g. `log`) the service answers with `reply`.
    Request {
        rid: String,
        kind: String,
        #[serde(default)]
        body: Value,
    },
    /// Service → app: show the player the catalogue line `key` as a toast that stays until
    /// dismissed (the same key again does not stack a second one).
    Toast { key: String },
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

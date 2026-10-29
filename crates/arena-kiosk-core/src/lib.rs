//! `arena-kiosk-core`: the platform-neutral kiosk logic shared by the Kiosk service and the
//! Kiosk app: the pinned-TLS Hub client, the reconnect back-off, the service ↔ app IPC
//! messages, the app watchdog, the offline queue's ordering rules, the process allow/block
//! decision and the Windows lockdown's pure parts (the registry plan, the keyboard filter, the
//! monitor union). It depends only on `arena-proto` among the workspace crates.

pub mod admin_exit;
pub mod backoff;
pub mod client;
pub mod hud;
pub mod ipc;
pub mod keys;
pub mod lockdown;
pub mod monitors;
pub mod pair;
pub mod procpolicy;
pub mod queue;
pub mod watchdog;

pub use backoff::Backoff;
pub use client::{
    ClientError, Closed, DEFAULT_PORT, EventStream, HubClient, HubStream, IDLE_TIMEOUT,
    PING_INTERVAL, REQUEST_TIMEOUT, capture_fingerprint, connect_ws, hex_lower, parse_fingerprint,
    pinned_config,
};
pub use ipc::{IpcError, IpcMsg, MAX_APP_LINE_BYTES, read_msg, read_msg_limited, write_msg};
pub use pair::{PAIR_TIMEOUT, PairError, pair};
pub use queue::{QUEUE_TTL_MS, ReplayStep};
pub use watchdog::{Watchdog, WatchdogAction};

//! `arena-kiosk-core`: the platform-neutral kiosk logic shared by the Kiosk service and the
//! Kiosk app: the pinned-TLS Hub client, the reconnect back-off, the service ↔ app IPC
//! messages, the app watchdog, the offline queue's ordering rules and the Windows lockdown's
//! pure parts (the registry plan, the keyboard filter, the monitor union). It depends only on
//! `arena-proto` among the workspace crates.

pub mod backoff;
pub mod client;
pub mod ipc;
pub mod keys;
pub mod lockdown;
pub mod monitors;
pub mod queue;
pub mod watchdog;

pub use backoff::Backoff;
pub use client::{
    ClientError, Closed, DEFAULT_PORT, EventStream, HubClient, HubStream, IDLE_TIMEOUT,
    PING_INTERVAL, REQUEST_TIMEOUT, connect_ws, hex_lower, parse_fingerprint, pinned_config,
};
pub use ipc::{IpcError, IpcMsg, read_msg, write_msg};
pub use queue::{QUEUE_TTL_MS, ReplayStep};
pub use watchdog::{Watchdog, WatchdogAction};

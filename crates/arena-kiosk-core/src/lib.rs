//! `arena-kiosk-core`: the shared Rust Hub client used by the Kiosk service (and any other
//! paired Rust app). It depends only on `arena-proto` among the workspace crates.

pub mod client;

pub use client::{
    ClientError, Closed, DEFAULT_PORT, EventStream, HubClient, HubStream, IDLE_TIMEOUT,
    PING_INTERVAL, REQUEST_TIMEOUT, connect_ws, parse_fingerprint, pinned_config,
};

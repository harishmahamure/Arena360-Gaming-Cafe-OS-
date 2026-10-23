//! `arena-tasks`: the Hub's background tasks.

pub mod lease;

pub use lease::{LEASE_TTL_MS, LeaseTask, RENEW_INTERVAL, Tick};

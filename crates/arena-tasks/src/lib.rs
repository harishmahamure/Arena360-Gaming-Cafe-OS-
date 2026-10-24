//! `arena-tasks`: the Hub's background tasks.

pub mod connectivity;
pub mod housekeeping;
pub mod lease;

pub use connectivity::{
    CloudHost, ConnectivityTask, DEFAULT_CLOUD_HOST, DEFAULT_INTERNET_ENDPOINTS, NetProber,
    PROBE_INTERVAL, PROBE_TIMEOUT, Prober, decide, default_internet_endpoints,
    parse_internet_endpoints,
};
pub use housekeeping::{
    DEDUPE_RETENTION_MS, HOUSEKEEPING_INTERVAL, Housekeeping, HousekeepingTask,
};
pub use lease::{LEASE_TTL_MS, LeaseTask, RENEW_INTERVAL, Tick};

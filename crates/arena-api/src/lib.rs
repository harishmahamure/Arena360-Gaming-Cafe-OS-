//! `arena-api`: the Hub's network edges. This story serves only the loopback `/healthz`.

pub mod health;

pub use health::{DEFAULT_HEALTH_PORT, bind_health, router, serve_health};

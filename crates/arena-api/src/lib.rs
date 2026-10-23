//! `arena-api`: the Hub's network edges: the loopback `/healthz` and `/ui/snapshot`, and the LAN
//! protocol server ([`lan`]).

pub mod health;
pub mod lan;

pub use health::{
    DEFAULT_HEALTH_PORT, UI_ALLOWED_ORIGINS, bind_health, router, serve_health, serve_router,
    ui_router,
};

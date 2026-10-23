//! `arena-api`: the Hub's network edges: the loopback `/healthz`, `/ui/snapshot` and the staff
//! auth routes ([`ui_auth`]), and the LAN
//! protocol server ([`lan`]).

pub mod health;
pub mod lan;
pub mod ui_auth;

pub use health::{
    DEFAULT_HEALTH_PORT, UI_ALLOWED_ORIGINS, bind_health, router, serve_health, serve_router,
    ui_router,
};
pub use ui_auth::{UiAuth, ui_auth_router};

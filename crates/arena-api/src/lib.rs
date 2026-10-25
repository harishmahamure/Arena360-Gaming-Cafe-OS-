//! `arena-api`: the Hub's network edges: the loopback `/healthz`, `/ui/snapshot`, the staff
//! auth routes ([`ui_auth`]), the audit viewer's routes ([`ui_audit`]) and the `/ui/events`
//! push stream ([`ui_events`]), and the LAN protocol server ([`lan`]).

pub mod health;
pub mod lan;
pub mod pairing;
pub mod ui_audit;
pub mod ui_auth;
pub mod ui_events;
mod ui_pairing;
mod ui_printer;

pub use health::{
    DEFAULT_HEALTH_PORT, UI_ALLOWED_ORIGINS, bind_health, router, serve_health, serve_router,
    ui_router,
};
pub use ui_auth::{UiAuth, ui_auth_router};

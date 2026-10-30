//! `arena-api`: the Hub's network edges: the loopback `/healthz`, `/ui/snapshot`, the staff
//! auth routes ([`ui_auth`]), the audit viewer's routes ([`ui_audit`]) and the `/ui/events`
//! push stream ([`ui_events`]), and the LAN protocol server ([`lan`]).

pub mod health;
pub mod lan;
pub mod pairing;
mod ui_ads;
pub mod ui_audit;
pub mod ui_auth;
mod ui_backup;
#[cfg(test)]
mod ui_backup_tests;
mod ui_coupons;
pub mod ui_events;
mod ui_import;
mod ui_library;
#[cfg(test)]
mod ui_library_tests;
pub mod ui_pair;
mod ui_pairing;
#[cfg(test)]
mod ui_player_tests;
mod ui_printer;
mod ui_refund;
mod ui_report;
#[cfg(test)]
mod ui_setup_tests;
mod ui_shift;
mod ui_tunnel;

pub use health::{
    DEFAULT_HEALTH_PORT, UI_ALLOWED_ORIGINS, bind_health, router, serve_health, serve_router,
    ui_router,
};
pub use ui_auth::{UiAuth, ui_auth_router};
pub use ui_pair::ui_pair_router;

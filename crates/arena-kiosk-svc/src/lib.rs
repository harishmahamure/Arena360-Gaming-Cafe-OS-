//! `arena-kiosk-svc`: the Kiosk's privileged half, a LocalSystem Windows service.
//!
//! It holds the device token (DPAPI, [`secret`]), the pinned-TLS Hub connection with jittered
//! reconnect ([`hub_link`]) and the offline intent queue ([`queue`]). The unprivileged Kiosk app
//! (`arena-kiosk-ui`, the player's session) talks only to this service, over an ACL'd named pipe
//! ([`ipc`]); the service pings it and restarts it when it stops answering ([`app`],
//! [`launcher`]). `lockdown apply|restore` ([`lockdown`]) locks the player account down for
//! the installer. [`procs`] kills programs the branch's process lists do not allow in the
//! player's session. Off Windows the same logic runs in console mode over a Unix socket, for dev
//! and CI.

pub mod admin_exit;
pub mod app;
pub mod cli;
pub mod clock;
pub mod config;
#[cfg(feature = "dev-fixtures")]
pub mod dev;
pub mod host;
pub mod hub_link;
pub mod intents;
pub mod ipc;
pub mod launcher;
pub mod lockdown;
pub mod logs;
pub mod paths;
pub mod procs;
pub mod queue;
pub mod runtime;
pub mod secret;
#[cfg(windows)]
pub mod service;
#[cfg(windows)]
pub mod winsec;

pub use runtime::{Ctx, Runtime, Timings};

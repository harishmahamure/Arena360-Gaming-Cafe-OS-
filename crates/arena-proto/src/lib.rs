//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

mod actor;
mod error;
mod intent;
mod permission;

pub use actor::{Actor, ActorKind, ElevatedBy, ElevationMode};
pub use error::{AppError, ErrorCode};
pub use intent::{BranchBootstrap, Intent, IntentKind, IntentPayload, SettingsSet};
pub use permission::Permission;

/// Returns this crate's name.
pub fn crate_name() -> &'static str {
    "arena-proto"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-proto");
    }
}

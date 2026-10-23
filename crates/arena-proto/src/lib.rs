//! `arena-proto`: protocol types shared by the Hub, the Kiosk and every UI surface.
//!
//! TypeScript bindings are generated from these types with ts-rs (`pnpm gen:types`) into
//! `packages/ui/src/generated`.

mod actor;
mod error;
mod frame;
mod intent;
mod permission;
mod scope;
mod snapshot;

pub use actor::{Actor, ActorKind, ElevatedBy, ElevationMode};
pub use error::{AppError, ErrorCode};
pub use frame::{
    AppliedSummary, ChangeOpKind, ChangeSummary, Envelope, ErrorFrame, EventFrame, FRAME_KINDS,
    Frame, FrameBody, FrameError, Hello, HelloOk, HubEvent, IntentResult, IntentSubmit,
    MAX_RID_LEN, PROTOCOL_VERSION, Versions, check_envelope, parse_frame, parse_frame_value,
    parse_json,
};
pub use intent::{
    BranchBootstrap, DeviceRegister, DeviceRevoke, DeviceVersionReported, Intent, IntentCategory,
    IntentKind, IntentPayload, SettingsSet,
};
pub use permission::Permission;
pub use scope::{ClientClass, class_scope, in_scope};
pub use snapshot::{HubSnapshot, SnapshotBranch, SnapshotDevice};

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

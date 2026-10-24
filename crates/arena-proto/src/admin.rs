//! Kiosk admin exit (Story 1.20): a manager drops a locked-down kiosk to the desktop for
//! [`ADMIN_EXIT_MS`], then it re-locks. Three ways in, each audited as a `device_admin_exits`
//! row: a manager PIN at the PC checked by the Hub (`hub`), the same PIN checked offline against
//! the cached [`ManagerVerifier`] (`cached_verifier`), or a remote unlock from the Hub floor
//! (`remote`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};

/// How long an admin exit lasts before the kiosk re-locks: 15 minutes.
pub const ADMIN_EXIT_MS: i64 = 15 * 60 * 1000;

/// The `kiosk.log` code of a failed admin-exit attempt (never the PIN).
pub const ADMIN_EXIT_FAILED_CODE: &str = "admin_exit_failed";

/// Why an admin exit was flagged for review: the offline verifier the kiosk used was older than
/// the one in effect when the exit happened.
pub const FLAG_STALE_VERIFIER: &str = "stale_verifier";

/// Most verifier entries the kiosk checks a PIN against (like the Hub's PIN candidates).
pub const MAX_VERIFIER_ENTRIES: usize = 50;

/// How an admin exit was granted (the `device_admin_exits.mode` column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum AdminExitMode {
    /// A manager PIN at the PC, checked live by the Hub.
    Hub,
    /// A manager PIN at the PC, checked offline against the cached verifier.
    CachedVerifier,
    /// A remote unlock from the Hub floor.
    Remote,
}

impl AdminExitMode {
    /// The stored and wire form, e.g. `cached_verifier`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::CachedVerifier => "cached_verifier",
            Self::Remote => "remote",
        }
    }

    /// The mode with this stored form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Hub, Self::CachedVerifier, Self::Remote]
            .into_iter()
            .find(|m| m.as_str() == s)
    }
}

/// The payload of `device.admin_exit` (device actor, elevated by a manager's PIN): the kiosk
/// granted an admin exit at `issued_at`. `exit_id` is a ULID the kiosk minted at that moment;
/// `mode` is `hub` or `cached_verifier` and matches `elevated_by.mode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceAdminExit {
    pub exit_id: String,
    pub mode: AdminExitMode,
}

/// The payload of `device.admin_relock` (device actor): the kiosk re-locked after the exit
/// `exit_id` (at the intent's `issued_at`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceAdminRelock {
    pub exit_id: String,
}

/// The payload of `device.admin_unlock` (staff with `device.remote`, or elevated to it): unlock
/// the paired kiosk `device_id` remotely. The Hub mints the exit and pushes it to the kiosk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceAdminUnlock {
    pub device_id: String,
}

/// One staff member whose PIN may unlock a kiosk offline: the argon2id PHC string of the PIN
/// (never the PIN) and its `pin_verifier_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct VerifierEntry {
    pub staff_id: String,
    pub pin_hash: String,
    #[ts(type = "number")]
    pub pin_verifier_version: i64,
}

/// What the Hub pushes to paired sessions so a kiosk can check a manager PIN while the Hub is
/// unreachable: the active staff whose role holds `device.remote` and who have a PIN (at most
/// [`MAX_VERIFIER_ENTRIES`]). `version` rises on every change to that set, their PINs or their
/// active state; a kiosk keeps the highest it has seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ManagerVerifier {
    #[ts(type = "number")]
    pub version: i64,
    pub entries: Vec<VerifierEntry>,
}

/// `kiosk.verify_admin_pin` (paired kiosk → Hub): check a manager PIN for an admin exit. The
/// LAN link is pinned TLS; the PIN is checked with argon2id and never stored or logged. The
/// reply is [`AdminPinOk`], or `PIN_REJECTED` / `AUTH_LOCKED`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct VerifyAdminPin {
    /// The PIN itself (see the README's note on the LAN link).
    pub pin_proof: String,
}

impl std::fmt::Debug for VerifyAdminPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifyAdminPin")
            .field("pin_proof", &"[redacted]")
            .finish()
    }
}

/// The Hub's answer to an accepted [`VerifyAdminPin`]: whose PIN it was and the verifier
/// version in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdminPinOk {
    pub staff_id: String,
    #[ts(type = "number")]
    pub verifier_version: i64,
}

/// `kiosk.admin_unlock_ack` (paired kiosk → Hub): the kiosk granted the remote unlock
/// `exit_id`. The Hub answers `ack` and tells the Hub UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdminUnlockAck {
    pub exit_id: String,
}

/// The Hub UI's `device.admin_unlock_ack` event: the kiosk `device_id` granted `exit_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct AdminUnlockAckEvent {
    pub exit_id: String,
    pub device_id: String,
}

/// Whether `s` is a ULID (26 Crockford base32 characters).
fn is_ulid_text(s: &str) -> bool {
    s.len() == 26
        && s.bytes().all(|b| {
            b.is_ascii_digit()
                || (b.is_ascii_uppercase() && !matches!(b, b'I' | b'L' | b'O' | b'U'))
        })
        && s.as_bytes()[0] <= b'7'
}

/// Checks an exit id: a ULID (`VALIDATION_FAILED` on `exit_id` otherwise).
pub fn validate_exit_id(exit_id: &str) -> Result<(), AppError> {
    if is_ulid_text(exit_id) {
        Ok(())
    } else {
        Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "exit_id",
            "exit_id must be a ULID",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip() {
        for m in [
            AdminExitMode::Hub,
            AdminExitMode::CachedVerifier,
            AdminExitMode::Remote,
        ] {
            assert_eq!(
                serde_json::to_string(&m).unwrap(),
                format!("\"{}\"", m.as_str())
            );
            assert_eq!(AdminExitMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(AdminExitMode::parse("x"), None);
        assert_eq!(ADMIN_EXIT_MS, 900_000);
    }

    #[test]
    fn the_pin_never_shows_in_debug() {
        let v = VerifyAdminPin {
            pin_proof: "2222".into(),
        };
        assert!(!format!("{v:?}").contains("2222"));
    }

    #[test]
    fn exit_ids_are_ulids() {
        assert!(validate_exit_id("01JAAAAAAAAAAAAAAAAAAAAAAA").is_ok());
        for bad in [
            "",
            "x",
            "01jaaaaaaaaaaaaaaaaaaaaaaa",
            "81JAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(validate_exit_id(bad).is_err(), "{bad}");
        }
    }
}

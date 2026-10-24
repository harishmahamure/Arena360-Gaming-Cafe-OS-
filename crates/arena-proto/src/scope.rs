//! LAN client classes and the intent kinds each class may submit.
//!
//! A request outside a session's class scope fails `FORBIDDEN`. No browser class (`kds`,
//! `phone`) may ever reach a staff, money or device intent.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::intent::IntentKind;

/// The kind of client on the other end of a LAN session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ClientClass {
    /// A paired app (Kiosk, Hub UI on another machine): device token, TLS only.
    Paired,
    /// A kitchen display in a browser: device token, `:7401/ws` only.
    Kds,
    /// A guest phone in a browser: anonymous until per-console tokens arrive, `:7401/ws` only.
    Phone,
}

impl ClientClass {
    /// The wire and stored form, e.g. `kds`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paired => "paired",
            Self::Kds => "kds",
            Self::Phone => "phone",
        }
    }

    /// The class with this wire form, if any.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Paired, Self::Kds, Self::Phone]
            .into_iter()
            .find(|c| c.as_str() == s)
    }

    /// Whether this class authenticates with a device token from the `devices` table.
    pub fn uses_device_token(self) -> bool {
        matches!(self, Self::Paired | Self::Kds)
    }
}

/// Paired apps (the Kiosk service): kiosk events and its own admin exits.
const PAIRED_SCOPE: &[IntentKind] = &[
    IntentKind::KioskLog,
    IntentKind::KioskProcessBlocked,
    IntentKind::DeviceAdminExit,
    IntentKind::DeviceAdminRelock,
];
/// Kitchen intents only (none exist yet).
const KDS_SCOPE: &[IntentKind] = &[];
const PHONE_SCOPE: &[IntentKind] = &[IntentKind::ConsolePlanRequested];

/// The intent kinds a session of `class` may submit.
pub fn class_scope(class: ClientClass) -> &'static [IntentKind] {
    match class {
        ClientClass::Paired => PAIRED_SCOPE,
        ClientClass::Kds => KDS_SCOPE,
        ClientClass::Phone => PHONE_SCOPE,
    }
}

/// Whether a session of `class` may submit `kind`.
pub fn in_scope(class: ClientClass, kind: IntentKind) -> bool {
    class_scope(class).contains(&kind)
}

/// The setting-key prefix a paired session may read with `settings.get` (and is pushed in
/// `settings.changed`). Browser classes read no settings at all.
pub const PAIRED_SETTINGS_PREFIX: &str = "kiosk.";

/// Whether a session of `class` may read the branch setting `key`: only paired apps, and only
/// `kiosk.*` keys.
pub fn may_read_setting(class: ClientClass, key: &str) -> bool {
    class == ClientClass::Paired && key.starts_with(PAIRED_SETTINGS_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::IntentCategory;

    #[test]
    fn no_browser_scope_reaches_staff_money_or_device() {
        for class in [ClientClass::Kds, ClientClass::Phone] {
            for kind in class_scope(class) {
                assert!(
                    !matches!(
                        kind.category(),
                        IntentCategory::Staff | IntentCategory::Money | IntentCategory::Device
                    ),
                    "{} may submit {}",
                    class.as_str(),
                    kind.as_str()
                );
            }
        }
    }

    #[test]
    fn kds_is_kitchen_only_and_phone_is_plan_requests_only() {
        assert!(
            class_scope(ClientClass::Kds)
                .iter()
                .all(|k| k.category() == IntentCategory::Kitchen)
        );
        assert_eq!(
            class_scope(ClientClass::Phone),
            &[IntentKind::ConsolePlanRequested]
        );
        assert_eq!(
            class_scope(ClientClass::Paired),
            &[
                IntentKind::KioskLog,
                IntentKind::KioskProcessBlocked,
                IntentKind::DeviceAdminExit,
                IntentKind::DeviceAdminRelock
            ]
        );
        assert!(!in_scope(
            ClientClass::Paired,
            IntentKind::DeviceAdminUnlock
        ));
        for kind in IntentKind::ALL {
            assert_eq!(
                in_scope(ClientClass::Phone, kind),
                kind == IntentKind::ConsolePlanRequested
            );
        }
    }

    #[test]
    fn only_paired_sessions_read_settings_and_only_kiosk_keys() {
        assert!(may_read_setting(ClientClass::Paired, "kiosk.process_allow"));
        assert!(may_read_setting(ClientClass::Paired, "kiosk.process_block"));
        assert!(!may_read_setting(ClientClass::Paired, "branch.currency"));
        assert!(!may_read_setting(ClientClass::Paired, "kiosk"));
        assert!(!may_read_setting(
            ClientClass::Paired,
            "Kiosk.process_allow"
        ));
        for class in [ClientClass::Kds, ClientClass::Phone] {
            assert!(!may_read_setting(class, "kiosk.process_allow"));
        }
    }

    #[test]
    fn classes_round_trip() {
        for class in [ClientClass::Paired, ClientClass::Kds, ClientClass::Phone] {
            assert_eq!(
                serde_json::to_string(&class).unwrap(),
                format!("\"{}\"", class.as_str())
            );
            assert_eq!(ClientClass::parse(class.as_str()), Some(class));
        }
        assert!(ClientClass::Kds.uses_device_token());
        assert!(!ClientClass::Phone.uses_device_token());
    }
}

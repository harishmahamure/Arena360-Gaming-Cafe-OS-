//! The permission catalogue and what each intent requires.
//!
//! Every [`IntentKind`](crate::IntentKind) maps to exactly one [`Requirement`]: a catalogue
//! [`Permission`] held through a role, or one of the implicit principals (`System`, `Device`,
//! `Phone`) that no role can grant. Custom roles may only use catalogue permissions.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A catalogue permission, serialized as its dotted `domain.verb` form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum Permission {
    #[serde(rename = "player.manage")]
    PlayerManage,
    #[serde(rename = "player.view")]
    PlayerView,
    #[serde(rename = "wallet.topup")]
    WalletTopup,
    #[serde(rename = "credit.manage")]
    CreditManage,
    #[serde(rename = "credit.collect")]
    CreditCollect,
    #[serde(rename = "session.start")]
    SessionStart,
    #[serde(rename = "session.pause")]
    SessionPause,
    #[serde(rename = "session.extend")]
    SessionExtend,
    #[serde(rename = "session.transfer")]
    SessionTransfer,
    #[serde(rename = "session.end")]
    SessionEnd,
    #[serde(rename = "session.force_end")]
    SessionForceEnd,
    #[serde(rename = "booking.manage")]
    BookingManage,
    #[serde(rename = "pos.sell")]
    PosSell,
    #[serde(rename = "pos.void")]
    PosVoid,
    #[serde(rename = "refund.issue")]
    RefundIssue,
    #[serde(rename = "discount.manual")]
    DiscountManual,
    #[serde(rename = "kitchen.use")]
    KitchenUse,
    #[serde(rename = "stock.in")]
    StockIn,
    #[serde(rename = "stock.adjust")]
    StockAdjust,
    #[serde(rename = "stock.override")]
    StockOverride,
    #[serde(rename = "catalog.edit")]
    CatalogEdit,
    #[serde(rename = "shift.open")]
    ShiftOpen,
    #[serde(rename = "shift.close")]
    ShiftClose,
    #[serde(rename = "shift.override_variance")]
    ShiftOverrideVariance,
    #[serde(rename = "cash.manage")]
    CashManage,
    /// Register and configure devices.
    #[serde(rename = "device.manage")]
    DeviceManage,
    /// Act on a paired device remotely (revoke it, unlock it).
    #[serde(rename = "device.remote")]
    DeviceRemote,
    /// Pair a new device with the branch.
    #[serde(rename = "device.pair")]
    DevicePair,
    #[serde(rename = "device.screen")]
    DeviceScreen,
    #[serde(rename = "user.manage")]
    UserManage,
    #[serde(rename = "settings.edit")]
    SettingsEdit,
    #[serde(rename = "backup.export")]
    BackupExport,
    #[serde(rename = "ops.promote")]
    OpsPromote,
    #[serde(rename = "ops.restore")]
    OpsRestore,
    #[serde(rename = "report.view")]
    ReportView,
    #[serde(rename = "audit.view")]
    AuditView,
}

impl Permission {
    /// Every catalogue permission, grouped by module.
    pub const ALL: [Permission; 36] = [
        Self::PlayerManage,
        Self::PlayerView,
        Self::WalletTopup,
        Self::CreditManage,
        Self::CreditCollect,
        Self::SessionStart,
        Self::SessionPause,
        Self::SessionExtend,
        Self::SessionTransfer,
        Self::SessionEnd,
        Self::SessionForceEnd,
        Self::BookingManage,
        Self::PosSell,
        Self::PosVoid,
        Self::RefundIssue,
        Self::DiscountManual,
        Self::KitchenUse,
        Self::StockIn,
        Self::StockAdjust,
        Self::StockOverride,
        Self::CatalogEdit,
        Self::ShiftOpen,
        Self::ShiftClose,
        Self::ShiftOverrideVariance,
        Self::CashManage,
        Self::DeviceManage,
        Self::DeviceRemote,
        Self::DevicePair,
        Self::DeviceScreen,
        Self::UserManage,
        Self::SettingsEdit,
        Self::BackupExport,
        Self::OpsPromote,
        Self::OpsRestore,
        Self::ReportView,
        Self::AuditView,
    ];

    /// The wire and stored form, e.g. `device.remote`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlayerManage => "player.manage",
            Self::PlayerView => "player.view",
            Self::WalletTopup => "wallet.topup",
            Self::CreditManage => "credit.manage",
            Self::CreditCollect => "credit.collect",
            Self::SessionStart => "session.start",
            Self::SessionPause => "session.pause",
            Self::SessionExtend => "session.extend",
            Self::SessionTransfer => "session.transfer",
            Self::SessionEnd => "session.end",
            Self::SessionForceEnd => "session.force_end",
            Self::BookingManage => "booking.manage",
            Self::PosSell => "pos.sell",
            Self::PosVoid => "pos.void",
            Self::RefundIssue => "refund.issue",
            Self::DiscountManual => "discount.manual",
            Self::KitchenUse => "kitchen.use",
            Self::StockIn => "stock.in",
            Self::StockAdjust => "stock.adjust",
            Self::StockOverride => "stock.override",
            Self::CatalogEdit => "catalog.edit",
            Self::ShiftOpen => "shift.open",
            Self::ShiftClose => "shift.close",
            Self::ShiftOverrideVariance => "shift.override_variance",
            Self::CashManage => "cash.manage",
            Self::DeviceManage => "device.manage",
            Self::DeviceRemote => "device.remote",
            Self::DevicePair => "device.pair",
            Self::DeviceScreen => "device.screen",
            Self::UserManage => "user.manage",
            Self::SettingsEdit => "settings.edit",
            Self::BackupExport => "backup.export",
            Self::OpsPromote => "ops.promote",
            Self::OpsRestore => "ops.restore",
            Self::ReportView => "report.view",
            Self::AuditView => "audit.view",
        }
    }

    /// The read permissions a manager PIN may grant for a while ("read elevation", Story 1.15):
    /// `POST /ui/elevate` with a `{permission}` item issues a read ticket for one of these.
    pub const READ_ELEVATION: [Permission; 3] =
        [Self::AuditView, Self::ReportView, Self::PlayerView];

    /// Whether a read elevation may grant this permission ([`Self::READ_ELEVATION`]).
    pub fn is_read_elevation(self) -> bool {
        Self::READ_ELEVATION.contains(&self)
    }

    /// The permission with this wire form, if it is in the catalogue.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// The module this permission is grouped under (for role editors).
    pub fn module(self) -> &'static str {
        match self {
            Self::PlayerManage | Self::PlayerView => "player",
            Self::WalletTopup | Self::CreditManage | Self::CreditCollect => "wallet",
            Self::SessionStart
            | Self::SessionPause
            | Self::SessionExtend
            | Self::SessionTransfer
            | Self::SessionEnd
            | Self::SessionForceEnd => "session",
            Self::BookingManage => "booking",
            Self::PosSell | Self::PosVoid | Self::RefundIssue | Self::DiscountManual => "pos",
            Self::KitchenUse => "kitchen",
            Self::StockIn | Self::StockAdjust | Self::StockOverride => "stock",
            Self::CatalogEdit => "catalog",
            Self::ShiftOpen | Self::ShiftClose | Self::ShiftOverrideVariance | Self::CashManage => {
                "shift"
            }
            Self::DeviceManage | Self::DeviceRemote | Self::DevicePair | Self::DeviceScreen => {
                "device"
            }
            Self::UserManage => "users",
            Self::SettingsEdit => "settings",
            Self::BackupExport | Self::OpsPromote | Self::OpsRestore => "ops",
            Self::ReportView | Self::AuditView => "views",
        }
    }
}

/// What an intent kind requires of its actor. Exactly one per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Requirement {
    /// A staff actor whose role allows (and does not deny) the permission, or an owner.
    Perm(Permission),
    /// The Hub itself (`actor.kind = system`). Implicit: no role grants it.
    System,
    /// A paired or kitchen device (`actor.kind = device`). Implicit.
    Device,
    /// A guest phone. Implicit; no phone actor exists yet, so it is refused.
    Phone,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn permissions_serialize_as_domain_verb_and_parse_back() {
        let mut seen = HashSet::new();
        for p in Permission::ALL {
            assert!(seen.insert(p), "{p:?} listed twice");
            assert_eq!(
                serde_json::to_string(&p).unwrap(),
                format!("\"{}\"", p.as_str())
            );
            assert_eq!(Permission::parse(p.as_str()), Some(p));
            let (domain, verb) = p.as_str().split_once('.').unwrap();
            assert!(!domain.is_empty() && !verb.is_empty());
            assert!(!p.module().is_empty());
        }
        assert_eq!(Permission::parse("nope.x"), None);
        assert_eq!(Permission::DeviceRemote.module(), "device");
        assert_eq!(Permission::CreditCollect.module(), "wallet");
        assert_eq!(Permission::AuditView.module(), "views");
    }

    #[test]
    fn only_view_permissions_take_a_read_elevation() {
        let reads: Vec<_> = Permission::ALL
            .into_iter()
            .filter(|p| p.is_read_elevation())
            .collect();
        assert_eq!(
            reads,
            vec![
                Permission::PlayerView,
                Permission::ReportView,
                Permission::AuditView
            ]
        );
        assert!(!Permission::SettingsEdit.is_read_elevation());
    }
}

//! Branch rows, settings and the append-only `changes` log, plus the generic row access the
//! change log and the replayer need. Only `apply()` (in `arena-core`) and the replayer write
//! these tables.

use std::collections::BTreeMap;

use arena_proto::{AppError, ErrorCode};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db_error;

/// One column value of a row, as carried in a change's MessagePack map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

/// A full row: column name to value.
pub type Row = BTreeMap<String, RowValue>;

/// A table the change log may carry (and the replayer may write).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Branch,
    Settings,
    Devices,
    Roles,
    RolePerms,
    Staff,
    ReadGrants,
    DeviceEvents,
    DeviceAdminExits,
    DevicesMeta,
    DeviceGroups,
    FloorZones,
    Products,
    BundleComponents,
    Stock,
    StockLots,
    StockMoves,
    StockHolds,
    Rates,
    Players,
    Consents,
    Sessions,
    SessionSegments,
    Orders,
    OrderLines,
    Payments,
    Wallets,
    WalletTxns,
    CreditBalances,
    Plans,
    PlanGroups,
    PlayerPacks,
    Alerts,
    PricingRules,
    GroupBills,
    Shifts,
    CashEvents,
    InvoiceSequences,
    NotifyOutbox,
    Coupons,
    CouponTargets,
    Refunds,
    LibraryEntries,
    KioskScans,
    GameMatches,
    LibraryInstalls,
    LibraryDismissals,
    GamePlays,
}

impl Table {
    /// Every change-log table.
    pub const ALL: [Table; 48] = [
        Table::Branch,
        Table::Settings,
        Table::Devices,
        Table::Roles,
        Table::RolePerms,
        Table::Staff,
        Table::ReadGrants,
        Table::DeviceEvents,
        Table::DeviceAdminExits,
        Table::DevicesMeta,
        Table::DeviceGroups,
        Table::FloorZones,
        Table::Products,
        Table::BundleComponents,
        Table::Stock,
        Table::StockLots,
        Table::StockMoves,
        Table::StockHolds,
        Table::Rates,
        Table::Players,
        Table::Consents,
        Table::Sessions,
        Table::SessionSegments,
        Table::Orders,
        Table::OrderLines,
        Table::Payments,
        Table::Wallets,
        Table::WalletTxns,
        Table::CreditBalances,
        Table::Plans,
        Table::PlanGroups,
        Table::PlayerPacks,
        Table::Alerts,
        Table::PricingRules,
        Table::GroupBills,
        Table::Shifts,
        Table::CashEvents,
        Table::InvoiceSequences,
        Table::NotifyOutbox,
        Table::Coupons,
        Table::CouponTargets,
        Table::Refunds,
        Table::LibraryEntries,
        Table::KioskScans,
        Table::GameMatches,
        Table::LibraryInstalls,
        Table::LibraryDismissals,
        Table::GamePlays,
    ];

    /// The SQL table name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Settings => "settings",
            Self::Devices => "devices",
            Self::Roles => "roles",
            Self::RolePerms => "role_perms",
            Self::Staff => "staff",
            Self::ReadGrants => "read_grants",
            Self::DeviceEvents => "device_events",
            Self::DeviceAdminExits => "device_admin_exits",
            Self::DevicesMeta => "devices_meta",
            Self::DeviceGroups => "device_groups",
            Self::FloorZones => "floor_zones",
            Self::Products => "products",
            Self::BundleComponents => "bundle_components",
            Self::Stock => "stock",
            Self::StockLots => "stock_lots",
            Self::StockMoves => "stock_moves",
            Self::StockHolds => "stock_holds",
            Self::Rates => "rates",
            Self::Players => "players",
            Self::Consents => "consents",
            Self::Sessions => "sessions",
            Self::SessionSegments => "session_segments",
            Self::Orders => "orders",
            Self::OrderLines => "order_lines",
            Self::Payments => "payments",
            Self::Wallets => "wallets",
            Self::WalletTxns => "wallet_txns",
            Self::CreditBalances => "credit_balances",
            Self::Plans => "plans",
            Self::PlanGroups => "plan_groups",
            Self::PlayerPacks => "player_packs",
            Self::Alerts => "alerts",
            Self::PricingRules => "pricing_rules",
            Self::GroupBills => "group_bills",
            Self::Shifts => "shifts",
            Self::CashEvents => "cash_events",
            Self::InvoiceSequences => "invoice_sequences",
            Self::NotifyOutbox => "notify_outbox",
            Self::Coupons => "coupons",
            Self::CouponTargets => "coupon_targets",
            Self::Refunds => "refunds",
            Self::LibraryEntries => "library_entries",
            Self::KioskScans => "kiosk_scans",
            Self::GameMatches => "game_matches",
            Self::LibraryInstalls => "library_installs",
            Self::LibraryDismissals => "library_dismissals",
            Self::GamePlays => "game_plays",
        }
    }

    /// The primary-key columns, in order.
    pub fn primary_key(self) -> &'static [&'static str] {
        match self {
            Self::Branch => &["branch_id"],
            Self::Settings => &["branch_id", "key"],
            Self::Devices => &["device_id"],
            Self::Roles => &["role_id"],
            Self::RolePerms => &["role_id", "permission"],
            Self::Staff => &["staff_id"],
            Self::ReadGrants => &["grant_id"],
            Self::DeviceEvents => &["event_id"],
            Self::DeviceAdminExits => &["exit_id"],
            Self::DevicesMeta | Self::KioskScans | Self::GameMatches => &["device_id"],
            Self::LibraryInstalls => &["entry_id", "device_id"],
            Self::LibraryDismissals => &["key"],
            Self::DeviceGroups => &["group_id"],
            Self::FloorZones => &["zone_id"],
            Self::Products => &["product_id"],
            Self::BundleComponents => &["bundle_id", "component_id"],
            Self::Stock => &["product_id"],
            Self::StockLots => &["lot_id"],
            Self::StockMoves => &["move_id"],
            Self::Rates => &["rate_id"],
            Self::Players => &["player_id"],
            Self::Wallets | Self::CreditBalances => &["player_id"],
            Self::PlayerPacks
            | Self::Alerts
            | Self::GroupBills
            | Self::Shifts
            | Self::CashEvents
            | Self::StockHolds
            | Self::NotifyOutbox
            | Self::Refunds
            | Self::LibraryEntries
            | Self::GamePlays => &["id"],
            Self::Plans => &["plan_id"],
            Self::PricingRules => &["rule_id"],
            Self::Coupons => &["coupon_id"],
            Self::CouponTargets => &["coupon_id", "target_kind", "target_id"],
            Self::PlanGroups => &["plan_id", "group_id"],
            Self::InvoiceSequences => &["fy", "segment"],
            Self::Consents => &["consent_id"],
            Self::Sessions
            | Self::SessionSegments
            | Self::Orders
            | Self::OrderLines
            | Self::Payments
            | Self::WalletTxns => &["id"],
        }
    }

    /// Whether the table has a `created_at` column (stamped on insert).
    pub fn has_created_at(self) -> bool {
        match self {
            Self::Branch
            | Self::Devices
            | Self::Roles
            | Self::Staff
            | Self::ReadGrants
            | Self::DeviceEvents
            | Self::DeviceAdminExits
            | Self::DevicesMeta
            | Self::DeviceGroups
            | Self::FloorZones
            | Self::Products
            | Self::Stock
            | Self::StockLots
            | Self::StockMoves
            | Self::StockHolds
            | Self::Rates
            | Self::Players
            | Self::Consents
            | Self::Sessions
            | Self::SessionSegments
            | Self::Orders
            | Self::OrderLines
            | Self::Payments
            | Self::Wallets
            | Self::WalletTxns
            | Self::CreditBalances
            | Self::PlayerPacks
            | Self::Alerts
            | Self::PricingRules
            | Self::GroupBills
            | Self::Shifts
            | Self::CashEvents
            | Self::InvoiceSequences
            | Self::NotifyOutbox
            | Self::Coupons
            | Self::Refunds
            | Self::LibraryEntries
            | Self::KioskScans
            | Self::GameMatches
            | Self::LibraryInstalls
            | Self::LibraryDismissals
            | Self::GamePlays
            | Self::Plans => true,
            Self::Settings
            | Self::RolePerms
            | Self::BundleComponents
            | Self::PlanGroups
            | Self::CouponTargets => false,
        }
    }

    /// Whether the table has `rev` and `updated_at` (stamped on every write). `role_perms`
    /// rows are versioned through their role's `rev`.
    pub fn is_versioned(self) -> bool {
        !matches!(
            self,
            Self::RolePerms | Self::BundleComponents | Self::PlanGroups | Self::CouponTargets
        )
    }

    /// The table with this SQL name, if it is a change-log table.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// The primary key of one written row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Branch {
        branch_id: String,
    },
    Settings {
        branch_id: String,
        key: String,
    },
    Devices {
        device_id: String,
    },
    Roles {
        role_id: String,
    },
    RolePerms {
        role_id: String,
        permission: String,
    },
    Staff {
        staff_id: String,
    },
    ReadGrants {
        grant_id: String,
    },
    DeviceEvents {
        event_id: String,
    },
    DeviceAdminExits {
        exit_id: String,
    },
    DevicesMeta {
        device_id: String,
    },
    DeviceGroups {
        group_id: String,
    },
    FloorZones {
        zone_id: String,
    },
    Products {
        product_id: String,
    },
    BundleComponents {
        bundle_id: String,
        component_id: String,
    },
    Stock {
        product_id: String,
    },
    StockLots {
        lot_id: String,
    },
    StockMoves {
        move_id: String,
    },
    Rates {
        rate_id: String,
    },
    Players {
        player_id: String,
    },
    Consents {
        consent_id: String,
    },
    Sessions {
        id: String,
    },
    SessionSegments {
        id: String,
    },
    Orders {
        id: String,
    },
    OrderLines {
        id: String,
    },
    Payments {
        id: String,
    },
    Wallets {
        player_id: String,
    },
    CreditBalances {
        player_id: String,
    },
    WalletTxns {
        id: String,
    },
    Plans {
        plan_id: String,
    },
    PlanGroups {
        plan_id: String,
        group_id: String,
    },
    PricingRules {
        rule_id: String,
    },
    PlayerPacks {
        id: String,
    },
    Alerts {
        id: String,
    },
    GroupBills {
        id: String,
    },
    Shifts {
        id: String,
    },
    StockHolds {
        id: String,
    },
    CashEvents {
        id: String,
    },
    InvoiceSequences {
        fy: String,
        segment: String,
    },
    NotifyOutbox {
        id: String,
    },
    Refunds {
        id: String,
    },
    LibraryEntries {
        id: String,
    },
    KioskScans {
        device_id: String,
    },
    GameMatches {
        device_id: String,
    },
    LibraryInstalls {
        entry_id: String,
        device_id: String,
    },
    GamePlays {
        id: String,
    },
    LibraryDismissals {
        key: String,
    },
    Coupons {
        coupon_id: String,
    },
    CouponTargets {
        coupon_id: String,
        target_kind: String,
        target_id: String,
    },
}

impl RowKey {
    pub fn table(&self) -> Table {
        match self {
            Self::Branch { .. } => Table::Branch,
            Self::Settings { .. } => Table::Settings,
            Self::Devices { .. } => Table::Devices,
            Self::Roles { .. } => Table::Roles,
            Self::RolePerms { .. } => Table::RolePerms,
            Self::Staff { .. } => Table::Staff,
            Self::ReadGrants { .. } => Table::ReadGrants,
            Self::DeviceEvents { .. } => Table::DeviceEvents,
            Self::DeviceAdminExits { .. } => Table::DeviceAdminExits,
            Self::DevicesMeta { .. } => Table::DevicesMeta,
            Self::DeviceGroups { .. } => Table::DeviceGroups,
            Self::FloorZones { .. } => Table::FloorZones,
            Self::Products { .. } => Table::Products,
            Self::BundleComponents { .. } => Table::BundleComponents,
            Self::Stock { .. } => Table::Stock,
            Self::StockLots { .. } => Table::StockLots,
            Self::StockMoves { .. } => Table::StockMoves,
            Self::StockHolds { .. } => Table::StockHolds,
            Self::Rates { .. } => Table::Rates,
            Self::Plans { .. } => Table::Plans,
            Self::PlanGroups { .. } => Table::PlanGroups,
            Self::PricingRules { .. } => Table::PricingRules,
            Self::PlayerPacks { .. } => Table::PlayerPacks,
            Self::Alerts { .. } => Table::Alerts,
            Self::GroupBills { .. } => Table::GroupBills,
            Self::Shifts { .. } => Table::Shifts,
            Self::CashEvents { .. } => Table::CashEvents,
            Self::InvoiceSequences { .. } => Table::InvoiceSequences,
            Self::NotifyOutbox { .. } => Table::NotifyOutbox,
            Self::Refunds { .. } => Table::Refunds,
            Self::LibraryEntries { .. } => Table::LibraryEntries,
            Self::KioskScans { .. } => Table::KioskScans,
            Self::GameMatches { .. } => Table::GameMatches,
            Self::LibraryInstalls { .. } => Table::LibraryInstalls,
            Self::LibraryDismissals { .. } => Table::LibraryDismissals,
            Self::GamePlays { .. } => Table::GamePlays,
            Self::Coupons { .. } => Table::Coupons,
            Self::CouponTargets { .. } => Table::CouponTargets,
            Self::Players { .. } => Table::Players,
            Self::Consents { .. } => Table::Consents,
            Self::Sessions { .. } => Table::Sessions,
            Self::SessionSegments { .. } => Table::SessionSegments,
            Self::Orders { .. } => Table::Orders,
            Self::OrderLines { .. } => Table::OrderLines,
            Self::Payments { .. } => Table::Payments,
            Self::Wallets { .. } => Table::Wallets,
            Self::WalletTxns { .. } => Table::WalletTxns,
            Self::CreditBalances { .. } => Table::CreditBalances,
        }
    }

    /// The `changes.row_id` form: the key columns joined by `/`.
    pub fn row_id(&self) -> String {
        match self {
            Self::Branch { branch_id } => branch_id.clone(),
            Self::Settings { branch_id, key } => format!("{branch_id}/{key}"),
            Self::Devices { device_id } => device_id.clone(),
            Self::Roles { role_id } => role_id.clone(),
            Self::RolePerms {
                role_id,
                permission,
            } => format!("{role_id}/{permission}"),
            Self::Staff { staff_id } => staff_id.clone(),
            Self::ReadGrants { grant_id } => grant_id.clone(),
            Self::DeviceEvents { event_id } => event_id.clone(),
            Self::DeviceAdminExits { exit_id } => exit_id.clone(),
            Self::DevicesMeta { device_id }
            | Self::KioskScans { device_id }
            | Self::GameMatches { device_id } => device_id.clone(),
            Self::LibraryInstalls {
                entry_id,
                device_id,
            } => format!("{entry_id}/{device_id}"),
            Self::LibraryDismissals { key } => key.clone(),
            Self::GamePlays { id } => id.clone(),
            Self::DeviceGroups { group_id } => group_id.clone(),
            Self::FloorZones { zone_id } => zone_id.clone(),
            Self::Products { product_id } => product_id.clone(),
            Self::BundleComponents {
                bundle_id,
                component_id,
            } => format!("{bundle_id}/{component_id}"),
            Self::Stock { product_id } => product_id.clone(),
            Self::StockLots { lot_id } => lot_id.clone(),
            Self::StockMoves { move_id } => move_id.clone(),
            Self::Rates { rate_id } => rate_id.clone(),
            Self::Plans { plan_id } => plan_id.clone(),
            Self::PlanGroups { plan_id, group_id } => format!("{plan_id}/{group_id}"),
            Self::InvoiceSequences { fy, segment } => format!("{fy}/{segment}"),
            Self::PricingRules { rule_id } => rule_id.clone(),
            Self::Coupons { coupon_id } => coupon_id.clone(),
            Self::CouponTargets {
                coupon_id,
                target_kind,
                target_id,
            } => format!("{coupon_id}/{target_kind}/{target_id}"),
            Self::Players { player_id } => player_id.clone(),
            Self::Wallets { player_id } | Self::CreditBalances { player_id } => player_id.clone(),
            Self::Consents { consent_id } => consent_id.clone(),
            Self::Sessions { id }
            | Self::SessionSegments { id }
            | Self::Orders { id }
            | Self::OrderLines { id }
            | Self::PlayerPacks { id }
            | Self::Alerts { id }
            | Self::GroupBills { id }
            | Self::Shifts { id }
            | Self::CashEvents { id }
            | Self::StockHolds { id }
            | Self::NotifyOutbox { id }
            | Self::Refunds { id }
            | Self::LibraryEntries { id }
            | Self::Payments { id }
            | Self::WalletTxns { id } => id.clone(),
        }
    }

    /// Rebuilds the key of a `table` row from its key columns (`None` if one is missing or
    /// not text).
    pub fn from_row(table: Table, row: &Row) -> Option<Self> {
        let text = |c: &str| match row.get(c) {
            Some(RowValue::Text(t)) => Some(t.clone()),
            _ => None,
        };
        Some(match table {
            Table::Branch => Self::Branch {
                branch_id: text("branch_id")?,
            },
            Table::Settings => Self::Settings {
                branch_id: text("branch_id")?,
                key: text("key")?,
            },
            Table::Devices => Self::Devices {
                device_id: text("device_id")?,
            },
            Table::Roles => Self::Roles {
                role_id: text("role_id")?,
            },
            Table::RolePerms => Self::RolePerms {
                role_id: text("role_id")?,
                permission: text("permission")?,
            },
            Table::Staff => Self::Staff {
                staff_id: text("staff_id")?,
            },
            Table::ReadGrants => Self::ReadGrants {
                grant_id: text("grant_id")?,
            },
            Table::DeviceEvents => Self::DeviceEvents {
                event_id: text("event_id")?,
            },
            Table::DeviceAdminExits => Self::DeviceAdminExits {
                exit_id: text("exit_id")?,
            },
            Table::DevicesMeta => Self::DevicesMeta {
                device_id: text("device_id")?,
            },
            Table::DeviceGroups => Self::DeviceGroups {
                group_id: text("group_id")?,
            },
            Table::FloorZones => Self::FloorZones {
                zone_id: text("zone_id")?,
            },
            Table::Products => Self::Products {
                product_id: text("product_id")?,
            },
            Table::BundleComponents => Self::BundleComponents {
                bundle_id: text("bundle_id")?,
                component_id: text("component_id")?,
            },
            Table::Stock => Self::Stock {
                product_id: text("product_id")?,
            },
            Table::StockLots => Self::StockLots {
                lot_id: text("lot_id")?,
            },
            Table::StockMoves => Self::StockMoves {
                move_id: text("move_id")?,
            },
            Table::Rates => Self::Rates {
                rate_id: text("rate_id")?,
            },
            Table::Plans => Self::Plans {
                plan_id: text("plan_id")?,
            },
            Table::PlanGroups => Self::PlanGroups {
                plan_id: text("plan_id")?,
                group_id: text("group_id")?,
            },
            Table::PricingRules => Self::PricingRules {
                rule_id: text("rule_id")?,
            },
            Table::Players => Self::Players {
                player_id: text("player_id")?,
            },
            Table::Consents => Self::Consents {
                consent_id: text("consent_id")?,
            },
            Table::PlayerPacks => Self::PlayerPacks { id: text("id")? },
            Table::Alerts => Self::Alerts { id: text("id")? },
            Table::GroupBills => Self::GroupBills { id: text("id")? },
            Table::Shifts => Self::Shifts { id: text("id")? },
            Table::CashEvents => Self::CashEvents { id: text("id")? },
            Table::InvoiceSequences => Self::InvoiceSequences {
                fy: text("fy")?,
                segment: text("segment")?,
            },
            Table::StockHolds => Self::StockHolds { id: text("id")? },
            Table::NotifyOutbox => Self::NotifyOutbox { id: text("id")? },
            Table::Refunds => Self::Refunds { id: text("id")? },
            Table::LibraryEntries => Self::LibraryEntries { id: text("id")? },
            Table::KioskScans => Self::KioskScans {
                device_id: text("device_id")?,
            },
            Table::GameMatches => Self::GameMatches {
                device_id: text("device_id")?,
            },
            Table::LibraryInstalls => Self::LibraryInstalls {
                entry_id: text("entry_id")?,
                device_id: text("device_id")?,
            },
            Table::LibraryDismissals => Self::LibraryDismissals { key: text("key")? },
            Table::GamePlays => Self::GamePlays { id: text("id")? },
            Table::Coupons => Self::Coupons {
                coupon_id: text("coupon_id")?,
            },
            Table::CouponTargets => Self::CouponTargets {
                coupon_id: text("coupon_id")?,
                target_kind: text("target_kind")?,
                target_id: text("target_id")?,
            },
            Table::Sessions => Self::Sessions { id: text("id")? },
            Table::SessionSegments => Self::SessionSegments { id: text("id")? },
            Table::Orders => Self::Orders { id: text("id")? },
            Table::OrderLines => Self::OrderLines { id: text("id")? },
            Table::Payments => Self::Payments { id: text("id")? },
            Table::Wallets => Self::Wallets {
                player_id: text("player_id")?,
            },
            Table::WalletTxns => Self::WalletTxns { id: text("id")? },
            Table::CreditBalances => Self::CreditBalances {
                player_id: text("player_id")?,
            },
        })
    }

    fn values(&self) -> Vec<&str> {
        match self {
            Self::Branch { branch_id } => vec![branch_id],
            Self::Settings { branch_id, key } => vec![branch_id, key],
            Self::Devices { device_id } => vec![device_id],
            Self::Roles { role_id } => vec![role_id],
            Self::RolePerms {
                role_id,
                permission,
            } => vec![role_id, permission],
            Self::Staff { staff_id } => vec![staff_id],
            Self::ReadGrants { grant_id } => vec![grant_id],
            Self::DeviceEvents { event_id } => vec![event_id],
            Self::DeviceAdminExits { exit_id } => vec![exit_id],
            Self::DevicesMeta { device_id }
            | Self::KioskScans { device_id }
            | Self::GameMatches { device_id } => vec![device_id],
            Self::LibraryInstalls {
                entry_id,
                device_id,
            } => vec![entry_id, device_id],
            Self::LibraryDismissals { key } => vec![key],
            Self::GamePlays { id } => vec![id],
            Self::DeviceGroups { group_id } => vec![group_id],
            Self::FloorZones { zone_id } => vec![zone_id],
            Self::Products { product_id } => vec![product_id],
            Self::BundleComponents {
                bundle_id,
                component_id,
            } => vec![bundle_id, component_id],
            Self::Stock { product_id } => vec![product_id],
            Self::StockLots { lot_id } => vec![lot_id],
            Self::StockMoves { move_id } => vec![move_id],
            Self::Rates { rate_id } => vec![rate_id],
            Self::Plans { plan_id } => vec![plan_id],
            Self::PlanGroups { plan_id, group_id } => vec![plan_id, group_id],
            Self::InvoiceSequences { fy, segment } => vec![fy, segment],
            Self::PricingRules { rule_id } => vec![rule_id],
            Self::Coupons { coupon_id } => vec![coupon_id],
            Self::CouponTargets {
                coupon_id,
                target_kind,
                target_id,
            } => vec![coupon_id, target_kind, target_id],
            Self::Players { player_id } => vec![player_id],
            Self::Wallets { player_id } | Self::CreditBalances { player_id } => vec![player_id],
            Self::Consents { consent_id } => vec![consent_id],
            Self::Sessions { id }
            | Self::SessionSegments { id }
            | Self::Orders { id }
            | Self::OrderLines { id }
            | Self::PlayerPacks { id }
            | Self::Alerts { id }
            | Self::GroupBills { id }
            | Self::Shifts { id }
            | Self::CashEvents { id }
            | Self::StockHolds { id }
            | Self::NotifyOutbox { id }
            | Self::Refunds { id }
            | Self::LibraryEntries { id }
            | Self::Payments { id }
            | Self::WalletTxns { id } => vec![id],
        }
    }
}

/// The kind of write a change records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeOp {
    Insert,
    Update,
    Delete,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "insert" => Some(Self::Insert),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// One row of the `changes` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRecord {
    pub epoch: String,
    pub seq: i64,
    pub tbl: String,
    pub row_id: String,
    pub op: ChangeOp,
    /// The full row after the write, as a MessagePack map.
    pub row: Vec<u8>,
    pub intent_id: String,
    pub intent_kind: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub elevated_by_kind: Option<String>,
    pub elevated_by_id: Option<String>,
    pub elevated_by_mode: Option<String>,
    pub elevated_by_verifier_version: Option<i64>,
    pub at: i64,
    /// The business day (`YYYY-MM-DD`) the change belongs to, under the branch locale then in
    /// effect; `None` before the branch exists and on rows written before migration 0006.
    pub business_day: Option<String>,
    /// The financial year of `business_day`, e.g. `FY2026-27`.
    pub fy: Option<String>,
    /// The intent's reason code (e.g. `stolen`), for kinds that require one (migration 0007).
    pub reason: Option<String>,
}

/// A new branch row (`rev` and timestamps are set by the stamp step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranch<'a> {
    pub branch_id: &'a str,
    pub name: &'a str,
    pub branch_code: &'a str,
    pub currency: &'a str,
    pub tz: &'a str,
    pub day_start_min: u16,
    pub fy_start_month: u8,
}

/// The branch's id, if it has been bootstrapped (a database holds one branch).
pub fn current_branch_id(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT branch_id FROM branch LIMIT 1", [], |r| r.get(0))
        .optional()
        .map_err(db_error)
}

/// The branch's display fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummary {
    pub branch_id: String,
    pub name: String,
    pub branch_code: String,
    pub code_locked: bool,
}

/// The branch's display fields, if it has been bootstrapped (a database holds one branch).
pub fn find_branch_summary(conn: &Connection) -> Result<Option<BranchSummary>, AppError> {
    conn.query_row(
        "SELECT branch_id, COALESCE((SELECT value FROM settings WHERE key = 'branch.name'), name),
                COALESCE((SELECT value FROM settings WHERE key = 'branch.code'), branch_code),
                COALESCE((SELECT value <> '' FROM settings WHERE key = 'branch.code_locked_at'), 0)
         FROM branch LIMIT 1",
        [],
        |r| {
            Ok(BranchSummary {
                branch_id: r.get(0)?,
                name: r.get(1)?,
                branch_code: r.get(2)?,
                code_locked: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// The locale columns of the branch row: the bootstrap values (the `branch.*` settings are
/// authoritative once they exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchLocale {
    pub currency: String,
    pub tz: String,
    pub day_start_min: u16,
    pub fy_start_month: u8,
}

/// The branch's locale columns, if it has been bootstrapped.
pub fn find_branch_locale(conn: &Connection) -> Result<Option<BranchLocale>, AppError> {
    conn.query_row(
        "SELECT currency, tz, day_start_min, fy_start_month FROM branch LIMIT 1",
        [],
        |r| {
            Ok(BranchLocale {
                currency: r.get(0)?,
                tz: r.get(1)?,
                day_start_min: r.get(2)?,
                fy_start_month: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// Whether a branch row exists.
pub fn branch_exists(conn: &Connection) -> Result<bool, AppError> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM branch)", [], |r| r.get(0))
        .map_err(db_error)
}

/// Whether any change-log table or `changes` itself has a row.
pub fn has_branch_data(conn: &Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM branch) OR EXISTS (SELECT 1 FROM settings)
             OR EXISTS (SELECT 1 FROM devices) OR EXISTS (SELECT 1 FROM roles)
             OR EXISTS (SELECT 1 FROM role_perms) OR EXISTS (SELECT 1 FROM staff)
             OR EXISTS (SELECT 1 FROM read_grants) OR EXISTS (SELECT 1 FROM device_events)
             OR EXISTS (SELECT 1 FROM device_admin_exits)
             OR EXISTS (SELECT 1 FROM devices_meta)
             OR EXISTS (SELECT 1 FROM device_groups)
             OR EXISTS (SELECT 1 FROM changes)",
        [],
        |r| r.get(0),
    )
    .map_err(db_error)
}

/// Inserts the branch row with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_branch(conn: &Connection, b: &NewBranch<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO branch (branch_id, name, branch_code, currency, tz, day_start_min,
                             fy_start_month, rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
        params![
            b.branch_id,
            b.name,
            b.branch_code,
            b.currency,
            b.tz,
            b.day_start_min,
            b.fy_start_month
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Inserts one setting with `rev = 0` and a zero timestamp, for the stamp step to set.
pub fn insert_setting(
    conn: &Connection,
    branch_id: &str,
    key: &str,
    value: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO settings (branch_id, key, value, rev, updated_at) VALUES (?1, ?2, ?3, 0, 0)",
        params![branch_id, key, value],
    )
    .map_err(db_error)?;
    Ok(())
}

/// A granted read elevation (`audit.read_granted`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReadGrant<'a> {
    pub grant_id: &'a str,
    pub branch_id: &'a str,
    pub permission: &'a str,
    pub requester_staff_id: &'a str,
    pub approver_staff_id: &'a str,
    pub verifier_version: i64,
}

/// Inserts a read-grant row with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_read_grant(conn: &Connection, g: &NewReadGrant<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO read_grants (grant_id, branch_id, permission, requester_staff_id,
                                  approver_staff_id, verifier_version, rev, created_at,
                                  updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0)",
        params![
            g.grant_id,
            g.branch_id,
            g.permission,
            g.requester_staff_id,
            g.approver_staff_id,
            g.verifier_version
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// A kiosk event (`kiosk.log`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeviceEvent<'a> {
    pub event_id: &'a str,
    pub branch_id: &'a str,
    pub device_id: &'a str,
    pub at: i64,
    pub level: &'a str,
    pub code: &'a str,
    pub detail: &'a str,
}

/// Inserts a device-event row with `rev = 0` and zero timestamps, for the stamp step to set.
pub fn insert_device_event(conn: &Connection, e: &NewDeviceEvent<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO device_events (event_id, branch_id, device_id, at, level, code, detail,
                                    rev, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
        params![
            e.event_id,
            e.branch_id,
            e.device_id,
            e.at,
            e.level,
            e.code,
            e.detail
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// One existing setting: its key, `rev`, value and any deferred change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRef {
    pub branch_id: String,
    pub key: String,
    pub rev: i64,
    pub value: String,
    /// The value in effect before `effective_from` (a deferred change), if any.
    pub prior_value: Option<String>,
    /// The business day (`YYYY-MM-DD`) from which `value` applies, if deferred.
    pub effective_from: Option<String>,
}

/// The branch's setting `key`, if it exists. A database holds one branch, so the key alone
/// identifies the row.
pub fn find_setting(conn: &Connection, key: &str) -> Result<Option<SettingRef>, AppError> {
    conn.query_row(
        "SELECT s.branch_id, s.key, s.rev, s.value, s.prior_value, s.effective_from
         FROM settings s
         JOIN branch b ON b.branch_id = s.branch_id
         WHERE s.key = ?1",
        [key],
        |r| {
            Ok(SettingRef {
                branch_id: r.get(0)?,
                key: r.get(1)?,
                rev: r.get(2)?,
                value: r.get(3)?,
                prior_value: r.get(4)?,
                effective_from: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

/// One setting as the Hub UI reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub rev: i64,
    pub prior_value: Option<String>,
    pub effective_from: Option<String>,
}

/// Every setting of the branch, ordered by key (empty before bootstrap).
pub fn list_settings(conn: &Connection) -> Result<Vec<SettingRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.key, s.value, s.rev, s.prior_value, s.effective_from FROM settings s
             JOIN branch b ON b.branch_id = s.branch_id ORDER BY s.key",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SettingRow {
                key: r.get(0)?,
                value: r.get(1)?,
                rev: r.get(2)?,
                prior_value: r.get(3)?,
                effective_from: r.get(4)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// Sets one setting's `value` and clears any deferred change (the stamp step then bumps `rev`
/// and `updated_at`). Fails `INTERNAL` if the row does not exist.
pub fn update_setting_value(
    conn: &Connection,
    branch_id: &str,
    key: &str,
    value: &str,
) -> Result<(), AppError> {
    update_setting(conn, branch_id, key, value, None, None)
}

/// Sets one setting's `value`, `prior_value` and `effective_from` (both `None` for a change
/// that applies at once). Fails `INTERNAL` if the row does not exist.
pub fn update_setting(
    conn: &Connection,
    branch_id: &str,
    key: &str,
    value: &str,
    prior_value: Option<&str>,
    effective_from: Option<&str>,
) -> Result<(), AppError> {
    let n = conn
        .execute(
            "UPDATE settings SET value = ?3, prior_value = ?4, effective_from = ?5
             WHERE branch_id = ?1 AND key = ?2",
            params![branch_id, key, value, prior_value, effective_from],
        )
        .map_err(db_error)?;
    if n != 1 {
        return Err(AppError::new(
            ErrorCode::Internal,
            format!("no settings row {branch_id}/{key}"),
        ));
    }
    Ok(())
}

fn key_where(table: Table) -> String {
    table
        .primary_key()
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Stamps a written row: `rev + 1` and `updated_at = now`, plus `created_at = now` on an insert
/// into a table that has it. A table without `rev` ([`Table::is_versioned`]) is not stamped.
/// Fails `INTERNAL` if the row does not exist.
pub fn stamp_row(
    conn: &Connection,
    key: &RowKey,
    now_ms: i64,
    inserted: bool,
) -> Result<(), AppError> {
    let table = key.table();
    if !table.is_versioned() {
        return Ok(());
    }
    let now_param = table.primary_key().len() + 1;
    let created = if inserted && table.has_created_at() {
        format!(", created_at = ?{now_param}")
    } else {
        String::new()
    };
    let sql = format!(
        "UPDATE \"{}\" SET rev = rev + 1, updated_at = ?{now_param}{created} WHERE {}",
        table.name(),
        key_where(table)
    );
    let mut values: Vec<Value> = key
        .values()
        .into_iter()
        .map(|v| Value::Text(v.to_owned()))
        .collect();
    values.push(Value::Integer(now_ms));
    let changed = conn
        .execute(&sql, params_from_iter(values))
        .map_err(db_error)?;
    if changed != 1 {
        return Err(AppError::new(
            ErrorCode::Internal,
            format!("stamp: no {} row {}", table.name(), key.row_id()),
        ));
    }
    Ok(())
}

fn row_from(r: &rusqlite::Row<'_>, names: &[String]) -> rusqlite::Result<Row> {
    let mut row = Row::new();
    for (i, name) in names.iter().enumerate() {
        let v = match r.get_ref(i)? {
            ValueRef::Null => RowValue::Null,
            ValueRef::Integer(n) => RowValue::Integer(n),
            ValueRef::Real(f) => RowValue::Real(f),
            ValueRef::Text(t) => RowValue::Text(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(_) => {
                return Err(rusqlite::Error::InvalidColumnType(
                    i,
                    name.clone(),
                    rusqlite::types::Type::Blob,
                ));
            }
        };
        row.insert(name.clone(), v);
    }
    Ok(row)
}

/// Reads the full row at `key`.
pub fn read_row(conn: &Connection, key: &RowKey) -> Result<Row, AppError> {
    let table = key.table();
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {}",
        table.name(),
        key_where(table)
    );
    let mut stmt = conn.prepare(&sql).map_err(db_error)?;
    let names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    stmt.query_row(params_from_iter(key.values()), |r| row_from(r, &names))
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                format!("no {} row {}", table.name(), key.row_id()),
            )
        })
}

/// Every row of `table`, ordered by its primary key.
pub fn dump_table(conn: &Connection, table: Table) -> Result<Vec<Row>, AppError> {
    let sql = format!(
        "SELECT * FROM \"{}\" ORDER BY {}",
        table.name(),
        table
            .primary_key()
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut stmt = conn.prepare(&sql).map_err(db_error)?;
    let names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let rows = stmt
        .query_map([], |r| row_from(r, &names))
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

/// The next `seq` for `epoch`: one more than the highest so far (1 for a new epoch).
pub fn next_change_seq(conn: &Connection, epoch: &str) -> Result<i64, AppError> {
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM changes WHERE epoch = ?1",
            [epoch],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    Ok(max + 1)
}

/// Appends one change row.
pub fn insert_change(conn: &Connection, c: &ChangeRecord) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO changes (epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
                              actor_kind, actor_id, elevated_by_kind, elevated_by_id,
                              elevated_by_mode, elevated_by_verifier_version, at,
                              business_day, fy, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18)",
        params![
            c.epoch,
            c.seq,
            c.tbl,
            c.row_id,
            c.op.as_str(),
            c.row,
            c.intent_id,
            c.intent_kind,
            c.actor_kind,
            c.actor_id,
            c.elevated_by_kind,
            c.elevated_by_id,
            c.elevated_by_mode,
            c.elevated_by_verifier_version,
            c.at,
            c.business_day,
            c.fy,
            c.reason
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// The columns [`change_from`] reads, in order.
pub(crate) const CHANGE_COLUMNS: &str = "epoch, seq, tbl, row_id, op, row, intent_id, intent_kind,
    actor_kind, actor_id, elevated_by_kind, elevated_by_id, elevated_by_mode,
    elevated_by_verifier_version, at, business_day, fy, reason";

/// One `changes` row selected as [`CHANGE_COLUMNS`].
pub(crate) fn change_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChangeRecord> {
    let op: String = r.get(4)?;
    Ok(ChangeRecord {
        epoch: r.get(0)?,
        seq: r.get(1)?,
        tbl: r.get(2)?,
        row_id: r.get(3)?,
        op: ChangeOp::parse(&op).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("unknown op {op}").into(),
            )
        })?,
        row: r.get(5)?,
        intent_id: r.get(6)?,
        intent_kind: r.get(7)?,
        actor_kind: r.get(8)?,
        actor_id: r.get(9)?,
        elevated_by_kind: r.get(10)?,
        elevated_by_id: r.get(11)?,
        elevated_by_mode: r.get(12)?,
        elevated_by_verifier_version: r.get(13)?,
        at: r.get(14)?,
        business_day: r.get(15)?,
        fy: r.get(16)?,
        reason: r.get(17)?,
    })
}

/// Every change row, ordered by epoch (numeric counter, then the epoch text) and `seq`.
pub fn read_changes(conn: &Connection) -> Result<Vec<ChangeRecord>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT epoch, seq, tbl, row_id, op, row, intent_id, intent_kind, actor_kind,
                    actor_id, elevated_by_kind, elevated_by_id, elevated_by_mode,
                    elevated_by_verifier_version, at, business_day, fy, reason
             FROM changes
             ORDER BY CAST(substr(epoch, 1, instr(epoch, ':') - 1) AS INTEGER), epoch, seq",
        )
        .map_err(db_error)?;
    let rows = stmt.query_map([], change_from).map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn table_columns(conn: &Connection, table: Table) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1)")
        .map_err(db_error)?;
    let cols = stmt
        .query_map([table.name()], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    cols.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn to_sql(v: &RowValue) -> Value {
    match v {
        RowValue::Null => Value::Null,
        RowValue::Integer(n) => Value::Integer(*n),
        RowValue::Real(f) => Value::Real(*f),
        RowValue::Text(t) => Value::Text(t.clone()),
    }
}

fn checked_columns(conn: &Connection, table: Table, row: &Row) -> Result<(), AppError> {
    let known = table_columns(conn, table)?;
    if let Some(bad) = row.keys().find(|c| !known.contains(c)) {
        return Err(AppError::new(
            ErrorCode::Internal,
            format!("{} has no column {bad:?}", table.name()),
        ));
    }
    if let Some(missing) = table.primary_key().iter().find(|c| !row.contains_key(**c)) {
        return Err(AppError::new(
            ErrorCode::Internal,
            format!("{} row lacks key column {missing}", table.name()),
        ));
    }
    Ok(())
}

/// Inserts `row` into `table`, or updates the existing row with the same primary key. Every
/// column name is checked against the table's schema.
pub fn upsert_row(conn: &Connection, table: Table, row: &Row) -> Result<(), AppError> {
    checked_columns(conn, table, row)?;
    let cols: Vec<&String> = row.keys().collect();
    let quoted = |c: &str| format!("\"{c}\"");
    let pk = table.primary_key();
    let updates: Vec<String> = cols
        .iter()
        .filter(|c| !pk.contains(&c.as_str()))
        .map(|c| format!("{0} = excluded.{0}", quoted(c)))
        .collect();
    let conflict = if updates.is_empty() {
        "DO NOTHING".to_owned()
    } else {
        format!("DO UPDATE SET {}", updates.join(", "))
    };
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) {}",
        table.name(),
        cols.iter()
            .map(|c| quoted(c))
            .collect::<Vec<_>>()
            .join(", "),
        (1..=cols.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", "),
        pk.iter().map(|c| quoted(c)).collect::<Vec<_>>().join(", "),
        conflict
    );
    conn.execute(&sql, params_from_iter(row.values().map(to_sql)))
        .map_err(db_error)?;
    Ok(())
}

/// Deletes the row of `table` whose primary key matches `row`'s key columns. Returns the
/// number of rows deleted (0 or 1).
pub fn delete_row(conn: &Connection, table: Table, row: &Row) -> Result<usize, AppError> {
    checked_columns(conn, table, row)?;
    let sql = format!(
        "DELETE FROM \"{}\" WHERE {}",
        table.name(),
        key_where(table)
    );
    let values: Vec<Value> = table
        .primary_key()
        .iter()
        .map(|c| to_sql(&row[*c]))
        .collect();
    conn.execute(&sql, params_from_iter(values))
        .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("branch.db"), 0).unwrap();
        (dir, store)
    }

    fn change(epoch: &str, seq: i64) -> ChangeRecord {
        ChangeRecord {
            epoch: epoch.into(),
            seq,
            tbl: "branch".into(),
            row_id: "B".into(),
            op: ChangeOp::Insert,
            row: vec![0x80],
            intent_id: "I".into(),
            intent_kind: "branch.bootstrap".into(),
            actor_kind: "system".into(),
            actor_id: "test".into(),
            elevated_by_kind: None,
            elevated_by_id: None,
            elevated_by_mode: None,
            elevated_by_verifier_version: None,
            at: 5,
            business_day: Some("2027-01-01".into()),
            fy: Some("FY2026-27".into()),
            reason: None,
        }
    }

    #[test]
    fn changes_is_append_only() {
        let (_dir, mut store) = store();
        store
            .write_tx(|tx| insert_change(tx, &change("1:A", 1)))
            .unwrap();
        for sql in [
            "UPDATE changes SET at = 6",
            // No retroactive restamping either.
            "UPDATE changes SET business_day = '2027-01-02'",
            "DELETE FROM changes",
            "DELETE FROM changes WHERE seq = 1",
        ] {
            let err = store
                .write_tx(|tx| tx.execute(sql, []).map_err(db_error))
                .unwrap_err();
            assert!(
                err.message.contains("changes is append-only"),
                "{sql}: {err}"
            );
        }
        assert_eq!(store.changes().unwrap(), vec![change("1:A", 1)]);
    }

    #[test]
    fn change_checks_reject_unknown_enums() {
        let (_dir, mut store) = store();
        let bad_actor = ChangeRecord {
            actor_kind: "robot".into(),
            ..change("1:A", 1)
        };
        assert!(store.write_tx(|tx| insert_change(tx, &bad_actor)).is_err());
        let bad_mode = ChangeRecord {
            elevated_by_mode: Some("phone".into()),
            ..change("1:A", 1)
        };
        assert!(store.write_tx(|tx| insert_change(tx, &bad_mode)).is_err());
        // Since 0007 the elevation kind is how it was proven, not an actor kind.
        for kind in ["staff", "owner", "robot"] {
            let bad_kind = ChangeRecord {
                elevated_by_kind: Some(kind.into()),
                elevated_by_id: Some("mgr".into()),
                elevated_by_mode: Some("hub".into()),
                elevated_by_verifier_version: Some(1),
                ..change("1:A", 1)
            };
            assert!(
                store.write_tx(|tx| insert_change(tx, &bad_kind)).is_err(),
                "{kind}"
            );
        }
        let mfa = ChangeRecord {
            elevated_by_kind: Some("mfa".into()),
            elevated_by_id: Some("owner".into()),
            elevated_by_mode: Some("hub".into()),
            elevated_by_verifier_version: Some(1),
            ..change("1:A", 1)
        };
        store.write_tx(|tx| insert_change(tx, &mfa)).unwrap();
    }

    #[test]
    fn elevated_by_columns_round_trip_and_are_all_or_nothing() {
        let (_dir, mut store) = store();
        let elevated = ChangeRecord {
            elevated_by_kind: Some("pin".into()),
            elevated_by_id: Some("mgr".into()),
            elevated_by_mode: Some("cached_verifier".into()),
            elevated_by_verifier_version: Some(3),
            ..change("1:A", 1)
        };
        store.write_tx(|tx| insert_change(tx, &elevated)).unwrap();
        assert_eq!(store.changes().unwrap(), vec![elevated.clone()]);
        let partial = ChangeRecord {
            seq: 2,
            elevated_by_verifier_version: None,
            ..elevated
        };
        assert!(store.write_tx(|tx| insert_change(tx, &partial)).is_err());
    }

    #[test]
    fn changes_order_by_numeric_epoch_counter() {
        let (_dir, mut store) = store();
        store
            .write_tx(|tx| {
                insert_change(tx, &change("10:A", 1))?;
                insert_change(tx, &change("9:Z", 2))?;
                insert_change(tx, &change("9:Z", 1))?;
                insert_change(tx, &change("10:A", 2))
            })
            .unwrap();
        let order: Vec<(String, i64)> = store
            .changes()
            .unwrap()
            .into_iter()
            .map(|c| (c.epoch, c.seq))
            .collect();
        assert_eq!(
            order,
            [("9:Z", 1), ("9:Z", 2), ("10:A", 1), ("10:A", 2)]
                .map(|(e, s)| (e.to_owned(), s))
                .to_vec()
        );
    }

    #[test]
    fn next_seq_is_per_epoch() {
        let (_dir, mut store) = store();
        store
            .write_tx(|tx| {
                assert_eq!(next_change_seq(tx, "1:A")?, 1);
                insert_change(tx, &change("1:A", 1))?;
                insert_change(tx, &change("1:A", 2))?;
                assert_eq!(next_change_seq(tx, "1:A")?, 3);
                assert_eq!(next_change_seq(tx, "2:B")?, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn insert_stamp_read_upsert_and_delete() {
        let (_dir, mut store) = store();
        let key = RowKey::Settings {
            branch_id: "B".into(),
            key: "k".into(),
        };
        let row = store
            .write_tx(|tx| {
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "N",
                        branch_code: "C",
                        currency: "INR",
                        tz: "Asia/Kolkata",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                let bkey = RowKey::Branch {
                    branch_id: "B".into(),
                };
                stamp_row(tx, &bkey, 42, true)?;
                let b = read_row(tx, &bkey)?;
                assert_eq!(b["rev"], RowValue::Integer(1));
                assert_eq!(b["created_at"], RowValue::Integer(42));
                insert_setting(tx, "B", "k", "v")?;
                stamp_row(tx, &key, 43, true)?;
                stamp_row(tx, &key, 44, false)?;
                read_row(tx, &key)
            })
            .unwrap();
        assert_eq!(row["rev"], RowValue::Integer(2));
        assert_eq!(row["updated_at"], RowValue::Integer(44));
        assert_eq!(key.row_id(), "B/k");

        let mut edited = row.clone();
        edited.insert("value".into(), RowValue::Text("w".into()));
        store
            .write_tx(|tx| upsert_row(tx, Table::Settings, &edited))
            .unwrap();
        assert_eq!(store.rows(Table::Settings).unwrap(), vec![edited.clone()]);

        let mut evil = edited.clone();
        evil.insert("x\"; DROP TABLE branch; --".into(), RowValue::Null);
        assert!(
            store
                .write_tx(|tx| upsert_row(tx, Table::Settings, &evil))
                .is_err()
        );

        assert_eq!(
            store
                .write_tx(|tx| delete_row(tx, Table::Settings, &edited))
                .unwrap(),
            1
        );
        assert!(store.rows(Table::Settings).unwrap().is_empty());
        assert_eq!(
            store
                .write_tx(|tx| delete_row(tx, Table::Settings, &edited))
                .unwrap(),
            0
        );
        assert_eq!(
            RowKey::from_row(Table::Settings, &edited),
            Some(key.clone())
        );
        assert!(store.write_tx(|tx| has_branch_data(tx)).unwrap());
        assert_eq!(store.rows(Table::Branch).unwrap().len(), 1);
    }

    #[test]
    fn find_and_update_a_setting() {
        let (_dir, mut store) = store();
        store
            .write_tx(|tx| {
                assert_eq!(find_setting(tx, "k")?, None);
                insert_branch(
                    tx,
                    &NewBranch {
                        branch_id: "B",
                        name: "N",
                        branch_code: "C",
                        currency: "INR",
                        tz: "UTC",
                        day_start_min: 0,
                        fy_start_month: 1,
                    },
                )?;
                insert_setting(tx, "B", "k", "v")?;
                let found = find_setting(tx, "k")?.unwrap();
                assert_eq!((found.branch_id.as_str(), found.rev), ("B", 0));
                update_setting_value(tx, "B", "k", "w")?;
                let row = read_row(
                    tx,
                    &RowKey::Settings {
                        branch_id: "B".into(),
                        key: "k".into(),
                    },
                )?;
                assert_eq!(row["value"], RowValue::Text("w".into()));
                assert!(update_setting_value(tx, "B", "nope", "x").is_err());
                update_setting(tx, "B", "k", "z", Some("w"), Some("2027-01-02"))?;
                let found = find_setting(tx, "k")?.unwrap();
                assert_eq!(found.value, "z");
                assert_eq!(found.prior_value.as_deref(), Some("w"));
                assert_eq!(found.effective_from.as_deref(), Some("2027-01-02"));
                let listed = list_settings(tx)?;
                assert_eq!(listed[0].prior_value.as_deref(), Some("w"));
                update_setting_value(tx, "B", "k", "y")?;
                let found = find_setting(tx, "k")?.unwrap();
                assert_eq!((found.prior_value, found.effective_from), (None, None));
                assert_eq!(
                    find_branch_locale(tx)?,
                    Some(BranchLocale {
                        currency: "INR".into(),
                        tz: "UTC".into(),
                        day_start_min: 0,
                        fy_start_month: 1,
                    })
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn stamping_a_missing_row_fails() {
        let (_dir, mut store) = store();
        let err = store
            .write_tx(|tx| {
                stamp_row(
                    tx,
                    &RowKey::Branch {
                        branch_id: "nope".into(),
                    },
                    1,
                    true,
                )
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
    }

    #[test]
    fn table_allow_list() {
        assert_eq!(Table::from_name("branch"), Some(Table::Branch));
        assert_eq!(Table::from_name("settings"), Some(Table::Settings));
        assert_eq!(Table::from_name("lease"), None);
        assert_eq!(Table::from_name("changes"), None);
    }
}

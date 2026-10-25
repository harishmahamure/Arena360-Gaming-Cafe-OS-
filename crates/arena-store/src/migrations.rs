//! Embedded, numbered, forward-only migrations.

/// One migration: its number and its SQL.
pub(crate) struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// Every migration, in order. Append new files here; never edit or reorder applied ones.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_branch_settings_changes.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../migrations/0003_applied_intents.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("../migrations/0004_devices.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("../migrations/0005_staff_roles.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("../migrations/0006_locale.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("../migrations/0007_elevation_kind.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("../migrations/0008_audit_indexes.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("../migrations/0009_device_events.sql"),
    },
    Migration {
        version: 10,
        sql: include_str!("../migrations/0010_admin_exit.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("../migrations/0011_devices_meta.sql"),
    },
    Migration {
        version: 12,
        sql: include_str!("../migrations/0012_hub_identity.sql"),
    },
    Migration {
        version: 13,
        sql: include_str!("../migrations/0013_device_groups.sql"),
    },
    Migration {
        version: 14,
        sql: include_str!("../migrations/0014_manual_devices.sql"),
    },
    Migration {
        version: 15,
        sql: include_str!("../migrations/0015_trusted_clock.sql"),
    },
    Migration {
        version: 16,
        sql: include_str!("../migrations/0016_floor_zones.sql"),
    },
    Migration {
        version: 17,
        sql: include_str!("../migrations/0017_products.sql"),
    },
    Migration {
        version: 18,
        sql: include_str!("../migrations/0018_staff_setup_code.sql"),
    },
];

/// The schema version this binary writes.
pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].version;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version as usize, i + 1);
        }
    }
}

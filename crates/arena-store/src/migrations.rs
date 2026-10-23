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

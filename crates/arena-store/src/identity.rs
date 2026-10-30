use arena_proto::{AppError, Tier};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db_error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubIdentity {
    pub tenant_id: String,
    pub branch_id: String,
    pub hub_id: String,
    pub tier: Tier,
    pub paired_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedLicence {
    pub jwt: String,
    pub received_at: i64,
}

pub fn find_hub_identity(conn: &Connection) -> Result<Option<HubIdentity>, AppError> {
    let row = conn
        .query_row(
            "SELECT tenant_id, branch_id, hub_id, tier, paired_at FROM hub_identity WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some((tenant_id, branch_id, hub_id, tier, paired_at)) = row else {
        return Ok(None);
    };
    let tier = Tier::parse(&tier).ok_or_else(|| {
        AppError::new(
            arena_proto::ErrorCode::Internal,
            format!("hub_identity has an unknown tier {tier:?}"),
        )
    })?;
    Ok(Some(HubIdentity {
        tenant_id,
        branch_id,
        hub_id,
        tier,
        paired_at,
    }))
}

pub fn insert_hub_identity(conn: &Connection, id: &HubIdentity) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "INSERT INTO hub_identity (id, tenant_id, branch_id, hub_id, tier, paired_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5) ON CONFLICT (id) DO NOTHING",
            params![
                id.tenant_id,
                id.branch_id,
                id.hub_id,
                id.tier.as_str(),
                id.paired_at
            ],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// Replaces the Hub's identity after a re-pair (the Keystore key was lost): the new hub id, tier
/// and paired time. Only for the same tenant and branch; `false` (nothing changed) when there is
/// no identity or the new code belongs to another branch.
pub fn replace_hub_identity(conn: &Connection, id: &HubIdentity) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE hub_identity SET hub_id = ?1, tier = ?2, paired_at = ?3
             WHERE id = 1 AND tenant_id = ?4 AND branch_id = ?5",
            params![
                id.hub_id,
                id.tier.as_str(),
                id.paired_at,
                id.tenant_id,
                id.branch_id
            ],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

pub fn find_cached_licence(conn: &Connection) -> Result<Option<CachedLicence>, AppError> {
    conn.query_row(
        "SELECT jwt, received_at FROM licence_cache WHERE id = 1",
        [],
        |r| {
            Ok(CachedLicence {
                jwt: r.get(0)?,
                received_at: r.get(1)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn store_licence(conn: &Connection, jwt: &str, received_at: i64) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO licence_cache (id, jwt, received_at) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET jwt = excluded.jwt, received_at = excluded.received_at",
        params![jwt, received_at],
    )
    .map_err(db_error)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedClockRow {
    pub cloud_ms: i64,
    pub mono_ms: i64,
    pub wall_delta_ms: i64,
}

pub fn find_trusted_clock(conn: &Connection) -> Result<Option<TrustedClockRow>, AppError> {
    conn.query_row(
        "SELECT cloud_ms, mono_ms, wall_delta_ms FROM trusted_clock WHERE id = 1",
        [],
        |r| {
            Ok(TrustedClockRow {
                cloud_ms: r.get(0)?,
                mono_ms: r.get(1)?,
                wall_delta_ms: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}

pub fn store_trusted_clock(conn: &Connection, c: &TrustedClockRow) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO trusted_clock (id, cloud_ms, mono_ms, wall_delta_ms) VALUES (1, ?1, ?2, ?3)
         ON CONFLICT (id) DO UPDATE SET cloud_ms = excluded.cloud_ms, mono_ms = excluded.mono_ms,
                                        wall_delta_ms = excluded.wall_delta_ms",
        params![c.cloud_ms, c.mono_ms, c.wall_delta_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(branch: &str, hub: &str, tier: Tier) -> HubIdentity {
        HubIdentity {
            tenant_id: "T".into(),
            branch_id: branch.into(),
            hub_id: hub.into(),
            tier,
            paired_at: 5,
        }
    }

    #[test]
    fn a_re_pair_replaces_the_hub_of_the_same_branch_only() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/0012_hub_identity.sql"))
            .unwrap();
        let first = identity("B", "H1", Tier::Free);
        assert!(!replace_hub_identity(&conn, &first).unwrap(), "no identity");
        assert!(insert_hub_identity(&conn, &first).unwrap());
        let mut second = identity("B", "H2", Tier::Basic);
        second.paired_at = 9;
        assert!(replace_hub_identity(&conn, &second).unwrap());
        assert_eq!(find_hub_identity(&conn).unwrap(), Some(second));
        assert!(!replace_hub_identity(&conn, &identity("OTHER", "H3", Tier::Free)).unwrap());
        assert_eq!(find_hub_identity(&conn).unwrap().unwrap().hub_id, "H2");
    }
}

/// A Hub that replaces another of a branch with history and waits to be restored (Story 9.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePending {
    /// The branch name the "Restore this branch" screen shows.
    pub branch_name: String,
}

/// Marks the paired Hub as waiting to be restored. `false` when the Hub has no identity.
pub fn mark_restore_pending(conn: &Connection, branch_name: &str) -> Result<bool, AppError> {
    let n = conn
        .execute(
            "UPDATE hub_identity SET restore_pending = 1, restore_branch_name = ?1 WHERE id = 1",
            params![branch_name],
        )
        .map_err(db_error)?;
    Ok(n == 1)
}

/// The restore the Hub is waiting for, if any.
pub fn find_restore_pending(conn: &Connection) -> Result<Option<RestorePending>, AppError> {
    conn.query_row(
        "SELECT restore_branch_name FROM hub_identity WHERE id = 1 AND restore_pending = 1",
        [],
        |r| {
            Ok(RestorePending {
                branch_name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            })
        },
    )
    .optional()
    .map_err(db_error)
}

#[cfg(test)]
mod restore_pending_tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE hub_identity (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL,
               branch_id TEXT NOT NULL, hub_id TEXT NOT NULL, tier TEXT NOT NULL,
               paired_at INTEGER NOT NULL, restore_pending INTEGER NOT NULL DEFAULT 0,
               restore_branch_name TEXT NULL);",
        )
        .unwrap();
        c
    }

    #[test]
    fn a_hub_without_identity_cannot_wait_and_one_with_it_remembers_the_name() {
        let c = conn();
        assert!(!mark_restore_pending(&c, "Level Up").unwrap());
        assert_eq!(find_restore_pending(&c).unwrap(), None);
        let id = HubIdentity {
            tenant_id: "T".into(),
            branch_id: "B".into(),
            hub_id: "H".into(),
            tier: Tier::Basic,
            paired_at: 1,
        };
        assert!(insert_hub_identity(&c, &id).unwrap());
        assert_eq!(find_restore_pending(&c).unwrap(), None);
        assert!(mark_restore_pending(&c, "Level Up").unwrap());
        assert_eq!(
            find_restore_pending(&c).unwrap(),
            Some(RestorePending {
                branch_name: "Level Up".into()
            })
        );
    }
}

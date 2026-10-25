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

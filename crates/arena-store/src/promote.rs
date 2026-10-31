//! What promoting a standby to main needs from the database (Story 9.21, AD-11, AD-24).
//!
//! [`Store::promote_basis`] says where the standby's copy of the main Hub's log ends, which is the
//! fork the new epoch declares. [`Store::promote_to_main`] then does the whole database part in
//! one transaction: it re-applies every erasure, takes the lease for a new epoch, records the fork
//! and marks the kitchen displays that must be paired again. The standby keeps its own identity,
//! licence and caches (Story 9.19), so nothing else changes hands.

use arena_proto::{AppError, ErrorCode, PAIR_AGAIN_TOKEN, epoch_parts};
use rusqlite::{Connection, params};

use crate::upload::BatchFork;
use crate::{
    LeaseRow, META_HUB_INSTANCE_ID, Store, db_error, delete_player_key, erase_player, get_meta,
    log_tip, raise_max_seen_counter, read_lease, read_max_seen_counter, set_meta, write_lease,
    write_lease_fork,
};

/// The `hub_meta` key holding the schema version of the main Hub, as it last stated it.
pub const META_MAIN_SCHEMA: &str = "standby.main_schema";

/// Where the standby stands, before it promotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteBasis {
    /// The epoch of the standby's newest change row (empty when it holds none), which is the
    /// epoch of the main Hub it followed.
    pub old_epoch: String,
    /// That row's `seq`: the last row of the old epoch the new epoch keeps.
    pub fork_seq: i64,
    /// This Hub's own `hub_instance_id`, which the new epoch is minted for.
    pub instance_id: String,
    /// The highest epoch counter this Hub has seen, in its lease row or the log.
    pub max_seen: i64,
    /// The schema version of this binary's database.
    pub schema_version: u32,
    /// The main Hub's schema version as it last stated it.
    pub main_schema: Option<u32>,
}

/// What the promote is told.
#[derive(Debug, Clone)]
pub struct PromoteInput<'a> {
    pub now_ms: i64,
    /// How long the new lease holds before the lease task must renew it.
    pub lease_ttl_ms: i64,
    /// The epoch the cloud issued for this Hub, `<counter>:<instance>`; `None` when it did not
    /// answer in time, which makes the new epoch provisional.
    pub claimed_epoch: Option<&'a str>,
}

/// What the promote did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteOutcome {
    pub old_epoch: String,
    pub fork_seq: i64,
    pub new_epoch: String,
    pub counter: i64,
    pub provisional: bool,
    /// Erased players whose erasure was applied again.
    pub erasures_reapplied: i64,
    /// Kitchen displays now marked to be paired again.
    pub kitchen_displays_to_repair: i64,
}

fn epoch_counter(epoch: &str) -> i64 {
    epoch_parts(epoch).map_or(0, |(counter, _)| counter)
}

/// Every erased player and when, from the player rows and from the `player.erased` change rows
/// (the log is the record of an erasure; the row is its effect).
fn erasures(conn: &Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT player_id, erased_at FROM players WHERE erased_at IS NOT NULL
             UNION
             SELECT row_id, at FROM changes
              WHERE tbl = 'players' AND intent_kind = 'player.erased'
                AND row_id IN (SELECT player_id FROM players)
             ORDER BY 1",
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(db_error)?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for row in rows {
        let (id, at) = row.map_err(db_error)?;
        // A player in both lists keeps the row's own time.
        if out.last().is_none_or(|(last, _)| *last != id) {
            out.push((id, at));
        }
    }
    Ok(out)
}

impl Store {
    /// Remembers the schema version the main Hub stated (`ready` frame), or a lower bound of it
    /// when it refused this standby as behind.
    pub fn record_main_schema(&mut self, schema: u32, only_raise: bool) -> Result<(), AppError> {
        self.write_tx(|tx| {
            let held = get_meta(tx, META_MAIN_SCHEMA)?.and_then(|v| v.parse::<u32>().ok());
            if only_raise && held.is_some_and(|h| h >= schema) {
                return Ok(());
            }
            set_meta(tx, META_MAIN_SCHEMA, &schema.to_string())
        })
    }

    /// The schema version the main Hub last stated, if this standby ever heard one.
    pub fn main_schema(&self) -> Result<Option<u32>, AppError> {
        Ok(self
            .meta(META_MAIN_SCHEMA)?
            .and_then(|v| v.parse::<u32>().ok()))
    }

    /// Where this standby's copy of the main Hub's log ends and who it is. Fails `INTERNAL`
    /// when the Hub has no instance id (it never opened as a Hub).
    pub fn promote_basis(&self) -> Result<PromoteBasis, AppError> {
        let (old_epoch, fork_seq) = log_tip(&self.conn)?.unwrap_or((String::new(), 0));
        let instance_id = get_meta(&self.conn, META_HUB_INSTANCE_ID)?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "this Hub has no instance id"))?;
        let max_seen = [
            epoch_counter(&old_epoch),
            read_lease(&self.conn)?.map_or(0, |l| l.counter),
            read_max_seen_counter(&self.conn)?,
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        Ok(PromoteBasis {
            old_epoch,
            fork_seq,
            instance_id,
            max_seen,
            schema_version: self.schema_version()?,
            main_schema: self.main_schema()?,
        })
    }

    /// Makes this Hub the writer of a new epoch, in one transaction: every erasure is applied
    /// again, the lease is taken for the new epoch (provisional unless the cloud issued it), the
    /// fork `{old epoch, last replicated seq}` is recorded for the first upload batch, the
    /// highest counter seen is raised and the kitchen displays inherited from the main Hub are
    /// marked to be paired again. Fails `SCHEMA_BEHIND` when this build's schema is below the
    /// main Hub's, and `INTERNAL` when the claimed epoch is not for this Hub or is not above the
    /// epochs it has seen.
    pub fn promote_to_main(
        &mut self,
        input: &PromoteInput<'_>,
    ) -> Result<PromoteOutcome, AppError> {
        let basis = self.promote_basis()?;
        if basis.main_schema.is_some_and(|m| basis.schema_version < m) {
            return Err(AppError::new(
                ErrorCode::SchemaBehind,
                "update this PC before it can take over from the main hub",
            ));
        }
        self.write_tx(|tx| {
            let mut reapplied = 0_i64;
            for (player_id, erased_at) in erasures(tx)? {
                erase_player(tx, &player_id, erased_at)?;
                delete_player_key(tx, &player_id)?;
                reapplied += 1;
            }

            let (counter, provisional) = match input.claimed_epoch {
                Some(epoch) => {
                    let (counter, instance) = epoch_parts(epoch).ok_or_else(|| {
                        AppError::new(ErrorCode::Internal, "the cloud issued a malformed epoch")
                    })?;
                    if instance != basis.instance_id || counter <= basis.max_seen {
                        return Err(AppError::new(
                            ErrorCode::Internal,
                            "the cloud issued an epoch this Hub cannot hold",
                        ));
                    }
                    (counter, false)
                }
                None => (basis.max_seen + 1, true),
            };
            write_lease(
                tx,
                &LeaseRow {
                    counter,
                    hub_instance_id: basis.instance_id.clone(),
                    provisional,
                    renewed_at: input.now_ms,
                    expires_at: input.now_ms + input.lease_ttl_ms,
                },
            )?;
            let fork = (!basis.old_epoch.is_empty()).then(|| BatchFork {
                parent_epoch: basis.old_epoch.clone(),
                parent_seq: basis.fork_seq,
            });
            write_lease_fork(tx, fork.as_ref())?;
            raise_max_seen_counter(tx, counter)?;

            // The displays hold tokens the main Hub issued and their tablets keep the main
            // Hub's address, so none of them can follow: each is paired again with a fresh code.
            let repaired = tx
                .execute(
                    "UPDATE kitchen_displays SET token_hash = ?1
                      WHERE revoked_at IS NULL AND token_hash <> ?1",
                    params![PAIR_AGAIN_TOKEN],
                )
                .map_err(db_error)?;
            let repair: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM kitchen_displays
                      WHERE revoked_at IS NULL AND token_hash = ?1",
                    params![PAIR_AGAIN_TOKEN],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            debug_assert!(repaired as i64 <= repair);

            Ok(PromoteOutcome {
                old_epoch: basis.old_epoch.clone(),
                fork_seq: basis.fork_seq,
                new_epoch: format!("{counter}:{}", basis.instance_id),
                counter,
                provisional,
                erasures_reapplied: reapplied,
                kitchen_displays_to_repair: repair,
            })
        })
    }
}

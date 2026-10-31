//! The move of a branch from an Android Hub to a Windows Hub (Story 10.16).
//!
//! `hub_migrations` holds one live row per branch. The owner starts it (a `migrate` pairing code is
//! written beside it), a fresh Windows Hub pairs with the code and takes the standby slot as a Hub
//! marked by `hubs.migration_id`, and the Android Hub is told at its next heartbeat. The owner can
//! cancel until the handover (Story 10.17) begins. Nothing here touches the branch's main Hub, its
//! token or its fence.

use super::hubs::PairOutcome;
use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_text, col_text, text};

/// `hub_migrations.state`. The states past `AwaitingHandover` belong to Stories 10.17-10.19.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    /// The owner started the move; no Windows Hub has paired with the code yet.
    AwaitingWindows,
    /// The Windows Hub paired; the Android Hub is asked to hand over.
    AwaitingHandover,
    HandingOver,
    HandedOver,
    Imported,
    Done,
    Cancelled,
}

impl MigrationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingWindows => "awaiting_windows",
            Self::AwaitingHandover => "awaiting_handover",
            Self::HandingOver => "handing_over",
            Self::HandedOver => "handed_over",
            Self::Imported => "imported",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::AwaitingWindows,
            Self::AwaitingHandover,
            Self::HandingOver,
            Self::HandedOver,
            Self::Imported,
            Self::Done,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubMigration {
    pub id: String,
    pub branch_id: String,
    pub from_hub_id: String,
    pub to_hub_id: Option<String>,
    pub target_hub_name: Option<String>,
    pub state: MigrationState,
    pub created_at: i64,
    pub updated_at: i64,
}

const MIGRATION_COLUMNS: &str =
    "id, branch_id, from_hub_id, to_hub_id, target_hub_name, state, created_at, updated_at";

fn migration_from(row: &[SqlValue]) -> Result<HubMigration, ControlError> {
    let state = col_text(row, 5)?;
    Ok(HubMigration {
        id: col_text(row, 0)?,
        branch_id: col_text(row, 1)?,
        from_hub_id: col_text(row, 2)?,
        to_hub_id: col_opt_text(row, 3)?,
        target_hub_name: col_opt_text(row, 4)?,
        state: MigrationState::parse(&state)
            .ok_or_else(|| ControlError::Db(format!("unknown migration state {state:?}")))?,
        created_at: col_int(row, 6)?,
        updated_at: col_int(row, 7)?,
    })
}

/// A move to start, or the fresh code of one that is still waiting for its Windows Hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMigration<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    /// The branch's Android main Hub.
    pub from_hub_id: &'a str,
    pub code_id: &'a str,
    pub code_hash: &'a str,
    pub now: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStart {
    /// A new move began; its code is written.
    Started,
    /// The move was still waiting for its Windows Hub: its earlier code is replaced.
    CodeReissued,
    /// The Windows Hub already paired (or the move is further on): no new code.
    InProgress(HubMigration),
    /// The drawn code was already taken; draw another.
    HashTaken,
    /// The Android Hub is no longer the branch's live main Hub.
    HubChanged,
}

/// The Windows Hub of a move, pairing with its `migrate` code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMigrationHub<'a> {
    pub id: &'a str,
    pub branch_id: &'a str,
    pub hub_instance_id: &'a str,
    pub version: &'a str,
    pub token_hash: &'a str,
    pub code_hash: &'a str,
    /// The PC's name, shown to staff on the Android Hub.
    pub target_hub_name: &'a str,
    pub now: i64,
}

const INSERT_MIGRATE_CODE: &str = "INSERT INTO pairing_codes (id, branch_id, code_hash, \
     tier_at_issue, created_at, expires_at, used_at, invalidated_at, role, purpose) \
     SELECT ?, ?, ?, COALESCE((SELECT CASE WHEN e.until IS NOT NULL AND ? > e.until \
     THEN 'free' ELSE e.tier END FROM entitlements e WHERE e.branch_id = ?), 'free'), \
     ?, ?, NULL, NULL, 'main', 'migrate' \
     WHERE NOT EXISTS (SELECT 1 FROM pairing_codes WHERE code_hash = ?)";

const INVALIDATE_MIGRATE_CODES: &str = "UPDATE pairing_codes SET invalidated_at = ? \
     WHERE branch_id = ? AND purpose = 'migrate' AND used_at IS NULL AND invalidated_at IS NULL";

impl ControlDb {
    /// The branch's move that is not over (`done` or `cancelled`), if any.
    pub async fn live_hub_migration(
        &self,
        branch_id: &str,
    ) -> Result<Option<HubMigration>, ControlError> {
        let sql = format!(
            "SELECT {MIGRATION_COLUMNS} FROM hub_migrations WHERE branch_id = ? \
             AND state NOT IN ('done', 'cancelled')"
        );
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| migration_from(r)).transpose()
    }

    /// The move whose Android Hub is `from_hub_id`, if it is not over.
    pub async fn live_migration_of_hub(
        &self,
        from_hub_id: &str,
    ) -> Result<Option<HubMigration>, ControlError> {
        let sql = format!(
            "SELECT {MIGRATION_COLUMNS} FROM hub_migrations WHERE from_hub_id = ? \
             AND state NOT IN ('done', 'cancelled')"
        );
        let rows = self.query(&sql, vec![text(from_hub_id)]).await?;
        rows.first().map(|r| migration_from(r)).transpose()
    }

    /// The branch's unused, unexpired `migrate` code.
    pub async fn active_migration_code(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<i64>, ControlError> {
        let rows = self
            .query(
                "SELECT expires_at FROM pairing_codes WHERE branch_id = ? AND purpose = 'migrate' \
                 AND used_at IS NULL AND invalidated_at IS NULL AND expires_at > ? \
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                vec![text(branch_id), SqlValue::Integer(now)],
            )
            .await?;
        rows.first().map(|r| col_int(r, 0)).transpose()
    }

    /// Starts the move of `branch_id` off its Android Hub, or replaces the code of one still
    /// waiting for its Windows Hub. The caller has checked the plan and that the Android Hub is
    /// the branch's main Hub; this re-checks the Hub in the same transaction.
    pub async fn start_hub_migration(
        &self,
        m: &NewMigration<'_>,
    ) -> Result<MigrationStart, ControlError> {
        let now = SqlValue::Integer(m.now);
        let existing = self.live_hub_migration(m.branch_id).await?;
        if let Some(live) = &existing
            && live.state != MigrationState::AwaitingWindows
        {
            return Ok(MigrationStart::InProgress(live.clone()));
        }
        let mut stmts = Vec::new();
        if existing.is_none() {
            stmts.push(
                Stmt::new(
                    "INSERT INTO hub_migrations (id, branch_id, from_hub_id, to_hub_id, \
                     target_hub_name, state, created_at, updated_at) \
                     SELECT ?, ?, ?, NULL, NULL, 'awaiting_windows', ?, ? \
                     WHERE EXISTS (SELECT 1 FROM hubs WHERE id = ? AND branch_id = ? \
                     AND role = 'main' AND platform = 'android' AND revoked_at IS NULL)",
                    vec![
                        text(m.id),
                        text(m.branch_id),
                        text(m.from_hub_id),
                        now.clone(),
                        now.clone(),
                        text(m.from_hub_id),
                        text(m.branch_id),
                    ],
                )
                .expect(1),
            );
        }
        stmts.push(Stmt::new(
            INVALIDATE_MIGRATE_CODES,
            vec![now.clone(), text(m.branch_id)],
        ));
        stmts.push(
            Stmt::new(
                INSERT_MIGRATE_CODE,
                vec![
                    text(m.code_id),
                    text(m.branch_id),
                    text(m.code_hash),
                    now.clone(),
                    text(m.branch_id),
                    now,
                    SqlValue::Integer(m.expires_at),
                    text(m.code_hash),
                ],
            )
            .expect(1),
        );
        if self.transact(stmts).await? {
            return Ok(if existing.is_none() {
                MigrationStart::Started
            } else {
                MigrationStart::CodeReissued
            });
        }
        if self.pairing_code_by_hash(m.code_hash).await?.is_some() {
            return Ok(MigrationStart::HashTaken);
        }
        Ok(MigrationStart::HubChanged)
    }

    /// The Windows Hub pairs with the move's code: the code is spent, the Hub is written beside
    /// the branch's main Hub (its token is not touched) and the move waits for the handover.
    /// `CodeRefused` when the code is no longer good for a Windows Hub on a paid plan, or the move
    /// is no longer waiting for one.
    pub async fn pair_migration_hub(
        &self,
        h: &NewMigrationHub<'_>,
    ) -> Result<PairOutcome, ControlError> {
        let now = SqlValue::Integer(h.now);
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "UPDATE pairing_codes SET used_at = ? WHERE code_hash = ? AND branch_id = ? \
                     AND purpose = 'migrate' AND used_at IS NULL AND invalidated_at IS NULL \
                     AND expires_at > ? AND COALESCE((SELECT CASE \
                     WHEN e.until IS NOT NULL AND ? > e.until THEN 'free' ELSE e.tier END \
                     FROM entitlements e WHERE e.branch_id = ?), 'free') <> 'free'",
                    vec![
                        now.clone(),
                        text(h.code_hash),
                        text(h.branch_id),
                        now.clone(),
                        now.clone(),
                        text(h.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, \
                     token_hash, paired_at, last_seen_at, revoked_at, role, migration_id) \
                     SELECT ?, ?, 'windows', ?, ?, ?, ?, NULL, NULL, 'standby', m.id \
                     FROM hub_migrations m WHERE m.branch_id = ? AND m.state = 'awaiting_windows'",
                    vec![
                        text(h.id),
                        text(h.branch_id),
                        text(h.hub_instance_id),
                        text(h.version),
                        text(h.token_hash),
                        now.clone(),
                        text(h.branch_id),
                    ],
                )
                .expect(1),
                Stmt::new(
                    "UPDATE hub_migrations SET state = 'awaiting_handover', to_hub_id = ?, \
                     target_hub_name = ?, updated_at = ? \
                     WHERE branch_id = ? AND state = 'awaiting_windows'",
                    vec![text(h.id), text(h.target_hub_name), now, text(h.branch_id)],
                )
                .expect(1),
            ])
            .await?;
        Ok(match rows {
            Some(_) => PairOutcome::Paired {
                revoked: Vec::new(),
            },
            None => PairOutcome::CodeRefused,
        })
    }

    /// The Android Hub says the Windows Hub holds the final snapshot (Story 10.17): the move goes
    /// from `awaiting_handover` to `handed_over`. `false` when `from_hub_id` has no such move (it
    /// was cancelled, or already reported), so a late report changes nothing.
    pub async fn mark_migration_handed_over(
        &self,
        from_hub_id: &str,
        migration_id: &str,
        now: i64,
    ) -> Result<bool, ControlError> {
        self.transact(vec![
            Stmt::new(
                "UPDATE hub_migrations SET state = 'handed_over', updated_at = ?                  WHERE id = ? AND from_hub_id = ? AND state = 'awaiting_handover'",
                vec![SqlValue::Integer(now), text(migration_id), text(from_hub_id)],
            )
            .expect(1),
        ])
        .await
    }

    /// The owner cancels the move before the handover: the move is `cancelled`, the Windows Hub's
    /// token is revoked and a waiting code is invalidated. `None` when there was no move to cancel
    /// (none, already over, or the handover has begun).
    pub async fn cancel_hub_migration(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<HubMigration>, ControlError> {
        let Some(live) = self.live_hub_migration(branch_id).await? else {
            return Ok(None);
        };
        if !matches!(
            live.state,
            MigrationState::AwaitingWindows | MigrationState::AwaitingHandover
        ) {
            return Ok(None);
        }
        let now = SqlValue::Integer(now);
        let done = self
            .transact(vec![
                Stmt::new(
                    "UPDATE hub_migrations SET state = 'cancelled', updated_at = ? \
                     WHERE id = ? AND state IN ('awaiting_windows', 'awaiting_handover')",
                    vec![now.clone(), text(&live.id)],
                )
                .expect(1),
                Stmt::new(
                    "UPDATE hubs SET revoked_at = ? WHERE migration_id = ? AND revoked_at IS NULL",
                    vec![now.clone(), text(&live.id)],
                ),
                Stmt::new(INVALIDATE_MIGRATE_CODES, vec![now, text(branch_id)]),
            ])
            .await?;
        Ok(done.then_some(HubMigration {
            state: MigrationState::Cancelled,
            ..live
        }))
    }
}

//! A replacement Hub's request for the branch key (Stories 9.9 and 9.10, AD-24): `key_requests` in
//! `gz_control`, and the test that tells a replacement Hub from a first one.
//!
//! The table holds the one-time public key, where the request stands and, once the owner approved,
//! the branch key re-wrapped for that public key (an age file only the waiting Hub can open). The
//! blob is handed out once ([`ControlDb::fetch_key_release`]) and cleared as it is. Nothing here
//! reads the escrowed key: that is `ControlDb::latest_kek_escrow`.

use arena_proto::{KEY_REQUEST_EXPIRY_MS, KeyRequestState};

use super::admin_audit::insert_stmt;
use super::commands::{NewCommand, command_from};
use super::{
    CommandRow, ControlDb, ControlError, NewAdminAudit, SqlValue, Stmt, col_int, col_opt_int,
    col_opt_text, col_text, text,
};

/// Why a decision on a key request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// No such request on the branch.
    NotFound,
    /// The request is decided already (denied, expired, or approved when the owner denies).
    NotPending,
    /// The request's public key was already released (this request or another instance's).
    AlreadyReleased,
    /// The Hub instance that asked has no live token any more.
    HubRevoked,
}

/// What a decision did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDecisionOutcome {
    /// Decided now: the `applied` command row.
    Applied(CommandRow),
    /// The post was already taken: its first row, nothing changed.
    Replayed(CommandRow),
    Refused(KeyRefusal),
}

/// What a Hub's fetch of its request found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyFetch {
    /// The Hub made no request.
    NoRequest,
    /// Pending, denied or expired: no blob.
    State(KeyRequestState),
    /// Approved and not fetched before: the blob, handed out now and never again.
    Released(String),
    /// Approved and fetched already.
    AlreadyFetched,
}

/// An owner's decision to write (one transaction: the command row, the request and the audit row).
#[derive(Debug, Clone)]
pub struct KeyDecision<'a> {
    /// The `applied` row to write (`kind` `backup.restore`).
    pub command: &'a NewCommand<'a>,
    pub request_id: &'a str,
    pub approve: bool,
    /// The re-wrapped key; present exactly when `approve`.
    pub wrapped_kek: Option<&'a str>,
    pub audit: &'a NewAdminAudit<'a>,
}

impl ControlDb {
    /// Whether the branch has history that a Hub paired as `hub_instance_id` must be restored
    /// from: another Hub instance of it checked in at least once, or the owner replaced a Hub
    /// (`hub.revoke`). The same instance pairing again (a retry, or after its keystore was lost)
    /// is not a replacement.
    pub async fn branch_has_history(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
    ) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT EXISTS (SELECT 1 FROM hubs WHERE branch_id = ?1 \
                    AND hub_instance_id <> ?2 AND last_seen_at IS NOT NULL) \
                 OR (EXISTS (SELECT 1 FROM hubs WHERE branch_id = ?1 AND hub_instance_id <> ?2) \
                    AND EXISTS (SELECT 1 FROM commands WHERE branch_id = ?1 \
                    AND kind = 'hub.revoke' AND status = 'applied'))",
                vec![text(branch_id), text(hub_instance_id)],
            )
            .await?;
        Ok(rows
            .first()
            .and_then(|r| r.first())
            .is_some_and(|v| matches!(v, SqlValue::Integer(1))))
    }

    /// Writes the request of `hub_instance_id` or refreshes it: one row per Hub instance. Only a
    /// `pending` row changes (a new key, name and time); an approved, denied or expired one stays
    /// as it is. Returns the state the row is in afterwards.
    pub async fn put_key_request(
        &self,
        id: &str,
        branch_id: &str,
        hub_instance_id: &str,
        public_key: &str,
        pc_name: Option<&str>,
        now: i64,
    ) -> Result<KeyRequestState, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "INSERT INTO key_requests (id, branch_id, hub_instance_id, public_key, state, \
                     requested_at, pc_name) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6) \
                     ON CONFLICT (branch_id, hub_instance_id) DO UPDATE SET \
                        public_key = excluded.public_key, requested_at = excluded.requested_at, \
                        pc_name = excluded.pc_name \
                     WHERE key_requests.state = 'pending'",
                    vec![
                        text(id),
                        text(branch_id),
                        text(hub_instance_id),
                        text(public_key),
                        SqlValue::Integer(now),
                        pc_name.map_or(SqlValue::Null, text),
                    ],
                ),
                Stmt::new(
                    "SELECT state FROM key_requests WHERE branch_id = ? AND hub_instance_id = ?",
                    vec![text(branch_id), text(hub_instance_id)],
                )
                .fetch(),
            ])
            .await?
            .ok_or_else(|| ControlError::Db("the key request was refused".to_owned()))?;
        let state = rows
            .get(1)
            .and_then(|r| r.first())
            .map(|row| col_text(row, 0))
            .transpose()?;
        state
            .as_deref()
            .and_then(KeyRequestState::parse)
            .ok_or_else(|| ControlError::Db("the key request has no state".to_owned()))
    }
}

/// One row of `key_requests`, without the wrapped key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequestRow {
    pub id: String,
    pub branch_id: String,
    pub hub_instance_id: String,
    pub public_key: String,
    pub state: KeyRequestState,
    pub requested_at: i64,
    pub decided_at: Option<i64>,
    pub decided_by: Option<String>,
    pub fetched_at: Option<i64>,
    pub pc_name: Option<String>,
}

fn request_from(r: &[SqlValue]) -> Result<KeyRequestRow, ControlError> {
    Ok(KeyRequestRow {
        id: col_text(r, 0)?,
        branch_id: col_text(r, 1)?,
        hub_instance_id: col_text(r, 2)?,
        public_key: col_text(r, 3)?,
        state: KeyRequestState::parse(&col_text(r, 4)?)
            .ok_or_else(|| ControlError::Db("key_requests has an unknown state".to_owned()))?,
        requested_at: col_int(r, 5)?,
        decided_at: col_opt_int(r, 6)?,
        decided_by: col_opt_text(r, 7)?,
        fetched_at: col_opt_int(r, 8)?,
        pc_name: col_opt_text(r, 9)?,
    })
}

impl ControlDb {
    /// Whether the owner released the branch key to `hub_instance_id` and that Hub took it
    /// (Story 9.12: the cloud backup is handed out only then).
    pub async fn key_released_to(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
    ) -> Result<bool, ControlError> {
        Ok(self
            .key_request(branch_id, hub_instance_id)
            .await?
            .is_some_and(|r| r.state == KeyRequestState::Approved && r.fetched_at.is_some()))
    }

    /// The request of `hub_instance_id` for `branch_id`, if it made one.
    pub async fn key_request(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
    ) -> Result<Option<KeyRequestRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, branch_id, hub_instance_id, public_key, state, requested_at, \
                 decided_at, decided_by, fetched_at, pc_name FROM key_requests \
                 WHERE branch_id = ? AND hub_instance_id = ?",
                vec![text(branch_id), text(hub_instance_id)],
            )
            .await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// The request `id` of `branch_id`.
    pub async fn key_request_by_id(
        &self,
        branch_id: &str,
        id: &str,
    ) -> Result<Option<KeyRequestRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, branch_id, hub_instance_id, public_key, state, requested_at, \
                 decided_at, decided_by, fetched_at, pc_name FROM key_requests \
                 WHERE branch_id = ? AND id = ?",
                vec![text(branch_id), text(id)],
            )
            .await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// The newest unexpired `pending` request of a Hub instance that still has a live token: what
    /// the owner app offers to release the key to.
    pub async fn pending_key_request(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<Option<KeyRequestRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, branch_id, hub_instance_id, public_key, state, requested_at, \
                 decided_at, decided_by, fetched_at, pc_name FROM key_requests k \
                 WHERE k.branch_id = ? AND k.state = 'pending' AND k.requested_at >= ? \
                 AND EXISTS (SELECT 1 FROM hubs h WHERE h.branch_id = k.branch_id \
                    AND h.hub_instance_id = k.hub_instance_id AND h.revoked_at IS NULL) \
                 ORDER BY k.requested_at DESC, k.id DESC LIMIT 1",
                vec![
                    text(branch_id),
                    SqlValue::Integer(now - KEY_REQUEST_EXPIRY_MS),
                ],
            )
            .await?;
        rows.first().map(|r| request_from(r)).transpose()
    }

    /// Flips every `pending` request of the branch older than 7 days to `expired`, one
    /// `admin_audit` row each in the same transaction. Returns how many it flipped.
    pub async fn expire_key_requests(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<u64, ControlError> {
        let cutoff = now - KEY_REQUEST_EXPIRY_MS;
        let due = self
            .query(
                "SELECT k.id, k.hub_instance_id, t.owner_id FROM key_requests k \
                 JOIN branches b ON b.id = k.branch_id JOIN tenants t ON t.id = b.tenant_id \
                 WHERE k.branch_id = ? AND k.state = 'pending' AND k.requested_at < ?",
                vec![text(branch_id), SqlValue::Integer(cutoff)],
            )
            .await?;
        let mut flipped = 0;
        for row in &due {
            let (id, instance, owner) = (col_text(row, 0)?, col_text(row, 1)?, col_text(row, 2)?);
            let summary = serde_json::json!({
                "outcome": "expired",
                "request_id": id,
                "hub_instance_id": instance,
                "owner_id": owner,
            })
            .to_string();
            let audit_id = format!("kreq-expired-{id}");
            let audit = NewAdminAudit {
                id: &audit_id,
                at: now,
                actor_kind: "system",
                actor_id: &owner,
                action: "kek.request.expire",
                target_kind: "branch",
                target_id: branch_id,
                reason: None,
                summary: &summary,
                request_id: &id,
            };
            let done = self
                .transact(vec![
                    Stmt::new(
                        "UPDATE key_requests SET state = 'expired', decided_at = ? \
                         WHERE id = ? AND state = 'pending' AND requested_at < ?",
                        vec![SqlValue::Integer(now), text(&id), SqlValue::Integer(cutoff)],
                    )
                    .expect(1),
                    insert_stmt(&audit),
                ])
                .await?;
            if done {
                flipped += 1;
            }
        }
        Ok(flipped)
    }

    /// Whether the owner may decide request `request_id` of `branch_id` as `approve` says: the
    /// request, or why not.
    pub async fn checked_key_request(
        &self,
        branch_id: &str,
        request_id: &str,
        approve: bool,
    ) -> Result<Result<KeyRequestRow, KeyRefusal>, ControlError> {
        let Some(row) = self.key_request_by_id(branch_id, request_id).await? else {
            return Ok(Err(KeyRefusal::NotFound));
        };
        if row.state != KeyRequestState::Pending {
            let released = approve && row.state == KeyRequestState::Approved;
            return Ok(Err(if released {
                KeyRefusal::AlreadyReleased
            } else {
                KeyRefusal::NotPending
            }));
        }
        if !approve {
            return Ok(Ok(row));
        }
        let rows = self
            .query(
                "SELECT EXISTS (SELECT 1 FROM hubs WHERE branch_id = ?1 AND hub_instance_id = ?2 \
                    AND revoked_at IS NULL), \
                 EXISTS (SELECT 1 FROM key_requests WHERE branch_id = ?1 AND public_key = ?3 \
                    AND state = 'approved')",
                vec![
                    text(branch_id),
                    text(&row.hub_instance_id),
                    text(&row.public_key),
                ],
            )
            .await?;
        let flag = |i: usize| {
            rows.first()
                .and_then(|r| r.get(i))
                .is_some_and(|v| matches!(v, SqlValue::Integer(1)))
        };
        Ok(if !flag(0) {
            Err(KeyRefusal::HubRevoked)
        } else if flag(1) {
            Err(KeyRefusal::AlreadyReleased)
        } else {
            Ok(row)
        })
    }

    /// Records the owner's decision: in one transaction the `applied` command row, the request's
    /// new state (and, on approval, the re-wrapped key) and one `admin_audit` row. The same
    /// idempotency key again finds its first row and changes nothing. A request that cannot be
    /// decided (any more) is refused and nothing is written.
    pub async fn apply_key_release(
        &self,
        d: &KeyDecision<'_>,
    ) -> Result<KeyDecisionOutcome, ControlError> {
        let c = d.command;
        let insert = Stmt::new(
            "INSERT INTO commands (branch_id, idempotency_key, kind, schema_version, payload, \
             issued_by, status, created_at, resolved_at, group_id, elevated_by) \
             VALUES (?, ?, ?, ?, ?, ?, 'applied', ?, ?, NULL, ?) \
             ON CONFLICT (branch_id, idempotency_key) DO NOTHING",
            vec![
                text(c.branch_id),
                text(c.idempotency_key),
                text(c.kind),
                SqlValue::Integer(i64::from(c.schema_version)),
                text(c.payload),
                text(c.issued_by),
                SqlValue::Integer(c.now),
                SqlValue::Integer(c.now),
                c.elevated_by.map_or(SqlValue::Null, text),
            ],
        )
        .expect(1);
        let decide = match (d.approve, d.wrapped_kek) {
            (true, Some(wrapped)) => Stmt::new(
                "UPDATE key_requests SET state = 'approved', decided_at = ?, decided_by = ?, \
                 wrapped_kek = ? WHERE id = ? AND branch_id = ? AND state = 'pending' \
                 AND EXISTS (SELECT 1 FROM hubs h WHERE h.branch_id = key_requests.branch_id \
                    AND h.hub_instance_id = key_requests.hub_instance_id AND h.revoked_at IS NULL) \
                 AND NOT EXISTS (SELECT 1 FROM key_requests o \
                    WHERE o.branch_id = key_requests.branch_id \
                    AND o.public_key = key_requests.public_key AND o.state = 'approved')",
                vec![
                    SqlValue::Integer(c.now),
                    text(c.issued_by),
                    text(wrapped),
                    text(d.request_id),
                    text(c.branch_id),
                ],
            ),
            (false, None) => Stmt::new(
                "UPDATE key_requests SET state = 'denied', decided_at = ?, decided_by = ? \
                 WHERE id = ? AND branch_id = ? AND state = 'pending'",
                vec![
                    SqlValue::Integer(c.now),
                    text(c.issued_by),
                    text(d.request_id),
                    text(c.branch_id),
                ],
            ),
            _ => {
                return Err(ControlError::Db(
                    "a key release carries the wrapped key exactly when it approves".to_owned(),
                ));
            }
        }
        .expect(1);
        let done = self
            .transact(vec![insert, decide, insert_stmt(d.audit)])
            .await?;
        let existing = self
            .query(
                "SELECT id, branch_id, idempotency_key, kind, schema_version, payload, \
                 issued_by, status, error, applied_seq, created_at, resolved_at, elevated_by \
                 FROM commands WHERE branch_id = ? AND idempotency_key = ?",
                vec![text(c.branch_id), text(c.idempotency_key)],
            )
            .await?
            .first()
            .map(|r| command_from(r))
            .transpose()?;
        if done {
            return existing
                .map(KeyDecisionOutcome::Applied)
                .ok_or_else(|| ControlError::Db("the decision's command row is missing".into()));
        }
        if let Some(row) = existing {
            return Ok(KeyDecisionOutcome::Replayed(row));
        }
        Ok(KeyDecisionOutcome::Refused(
            match self
                .checked_key_request(c.branch_id, d.request_id, d.approve)
                .await?
            {
                Err(why) => why,
                // Decided by someone else between the check and the write.
                Ok(_) => KeyRefusal::NotPending,
            },
        ))
    }

    /// The Hub's fetch of its decided request. An approved request not fetched before hands out
    /// its re-wrapped key, and in the same transaction sets `fetched_at` and clears the blob; the
    /// request is never handed out twice.
    pub async fn fetch_key_release(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
        now: i64,
    ) -> Result<KeyFetch, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "SELECT wrapped_kek FROM key_requests WHERE branch_id = ?1 \
                     AND hub_instance_id = ?2 AND state = 'approved' AND fetched_at IS NULL \
                     AND wrapped_kek IS NOT NULL",
                    vec![text(branch_id), text(hub_instance_id)],
                )
                .fetch(),
                Stmt::new(
                    "UPDATE key_requests SET fetched_at = ?3, wrapped_kek = NULL \
                     WHERE branch_id = ?1 AND hub_instance_id = ?2 AND state = 'approved' \
                     AND fetched_at IS NULL AND wrapped_kek IS NOT NULL",
                    vec![
                        text(branch_id),
                        text(hub_instance_id),
                        SqlValue::Integer(now),
                    ],
                ),
            ])
            .await?;
        let blob = rows
            .as_ref()
            .and_then(|r| r.first())
            .and_then(|r| r.first())
            .map(|row| col_text(row, 0))
            .transpose()?;
        if let Some(blob) = blob {
            return Ok(KeyFetch::Released(blob));
        }
        Ok(match self.key_request(branch_id, hub_instance_id).await? {
            None => KeyFetch::NoRequest,
            Some(r) if r.state == KeyRequestState::Approved => KeyFetch::AlreadyFetched,
            Some(r) => KeyFetch::State(r.state),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;

    async fn db() -> (ControlDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = ControlDb::open_local(&dir.path().join("gz_control.db"))
            .await
            .unwrap();
        db.migrate(1).await.unwrap();
        db.transact(vec![
            Stmt::new(
                "INSERT INTO owners (id, name, phone_e164, email, cafe_name, city, mfa_state, \n                 created_at) VALUES ('O1', 'n', '+911', 'e', 'c', 'x', 'enrolled', 1)",
                vec![],
            ),
            Stmt::new("INSERT INTO tenants (id, owner_id, name, created_at) VALUES ('T1', 'O1', 'c', 1)", vec![]),
            Stmt::new(
                "INSERT INTO branches (id, tenant_id, name, city, country, currency, timezone, \n                 branch_code, created_at, updated_at) \n                 VALUES ('B1', 'T1', 'b', 'x', 'IN', 'INR', 'Asia/Kolkata', 'LVL', 1, 1)",
                vec![],
            ),
        ])
        .await
        .unwrap();
        (db, dir)
    }

    async fn put(db: &ControlDb, id: &str, instance: &str, key: &str, now: i64) {
        db.put_key_request(id, "B1", instance, key, Some("FRONT-DESK"), now)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_key_counts_as_released_only_once_it_was_approved_and_taken() {
        let (db, _dir) = db().await;
        assert!(!db.key_released_to("B1", "I1").await.unwrap(), "no request");
        put(&db, "R1", "I1", "age1a", 10).await;
        assert!(!db.key_released_to("B1", "I1").await.unwrap(), "pending");
        db.transact(vec![
            Stmt::new(
                "UPDATE key_requests SET state = 'approved', decided_at = 30, decided_by = 'O1' \n                 WHERE branch_id = 'B1'",
                vec![],
            )
            .expect(1),
        ])
        .await
        .unwrap();
        assert!(
            !db.key_released_to("B1", "I1").await.unwrap(),
            "not fetched"
        );
        db.transact(vec![
            Stmt::new(
                "UPDATE key_requests SET fetched_at = 40 WHERE branch_id = 'B1'",
                vec![],
            )
            .expect(1),
        ])
        .await
        .unwrap();
        assert!(db.key_released_to("B1", "I1").await.unwrap());
        assert!(
            !db.key_released_to("B1", "I2").await.unwrap(),
            "another instance"
        );
    }

    #[tokio::test]
    async fn only_a_pending_request_is_refreshed_and_a_decided_one_stays() {
        let (db, _dir) = db().await;
        let put = |id: &'static str, key: &'static str, now: i64| {
            let db = &db;
            async move {
                db.put_key_request(id, "B1", "I1", key, None, now)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(put("R1", "age1a", 10).await, KeyRequestState::Pending);
        assert_eq!(put("R2", "age1b", 20).await, KeyRequestState::Pending);
        let row = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(
            (row.id.as_str(), row.public_key.as_str(), row.requested_at),
            ("R1", "age1b", 20)
        );

        db.transact(vec![
            Stmt::new(
                "UPDATE key_requests SET state = 'approved', decided_at = 30, decided_by = 'O1' \
                 WHERE branch_id = 'B1'",
                vec![],
            )
            .expect(1),
        ])
        .await
        .unwrap();
        assert_eq!(put("R3", "age1c", 40).await, KeyRequestState::Approved);
        let row = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(
            (row.public_key.as_str(), row.requested_at, row.decided_at),
            ("age1b", 20, Some(30))
        );
        assert!(db.key_request("B1", "I2").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn another_hub_instance_has_its_own_request() {
        let (db, _dir) = db().await;
        put(&db, "R1", "I1", "age1a", 10).await;
        put(&db, "R2", "I2", "age1b", 11).await;
        let row = db.key_request("B1", "I2").await.unwrap().unwrap();
        assert_eq!(
            (row.id.as_str(), row.pc_name.as_deref()),
            ("R2", Some("FRONT-DESK"))
        );
    }

    async fn hub(db: &ControlDb, id: &str, instance: &str, last_seen: Option<i64>, revoked: bool) {
        db.transact(vec![Stmt::new(
            "INSERT INTO hubs (id, branch_id, platform, hub_instance_id, version, token_hash, \
             paired_at, last_seen_at, revoked_at) VALUES (?, 'B1', 'windows', ?, '1', 'h', 1, ?, ?)",
            vec![
                text(id),
                text(instance),
                last_seen.map_or(SqlValue::Null, SqlValue::Integer),
                if revoked {
                    SqlValue::Integer(2)
                } else {
                    SqlValue::Null
                },
            ],
        )
        .expect(1)])
        .await
        .unwrap();
    }

    async fn revoke_command(db: &ControlDb, status: &str) {
        db.transact(vec![
            Stmt::new(
                "INSERT INTO commands (branch_id, idempotency_key, kind, schema_version, payload, \
             issued_by, status, created_at) VALUES ('B1', 'k', 'hub.revoke', 1, '{}', 'O1', ?, 1)",
                vec![text(status)],
            )
            .expect(1),
        ])
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn history_is_another_instance_that_checked_in_or_a_revoked_hub() {
        let (db, _dir) = db().await;
        assert!(!db.branch_has_history("B1", "I2").await.unwrap(), "no hub");

        // The same instance, even one that beat, pairing again is not a replacement.
        hub(&db, "H1", "I1", Some(5), false).await;
        assert!(!db.branch_has_history("B1", "I1").await.unwrap());
        // Another PC pairing while the first Hub has checked in is.
        assert!(db.branch_has_history("B1", "I2").await.unwrap());
    }

    #[tokio::test]
    async fn a_hub_that_never_checked_in_is_history_only_after_an_applied_revoke() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", None, false).await;
        assert!(!db.branch_has_history("B1", "I2").await.unwrap(), "a retry");
        revoke_command(&db, "pending").await;
        assert!(!db.branch_has_history("B1", "I2").await.unwrap());
        db.transact(vec![
            Stmt::new("UPDATE commands SET status = 'applied'", vec![]).expect(1),
        ])
        .await
        .unwrap();
        assert!(db.branch_has_history("B1", "I2").await.unwrap());
        assert!(!db.branch_has_history("B1", "I1").await.unwrap());
    }

    fn cmd<'a>(key: &'a str, payload: &'a str, now: i64) -> NewCommand<'a> {
        NewCommand {
            branch_id: "B1",
            idempotency_key: key,
            kind: "backup.restore",
            schema_version: 1,
            payload,
            issued_by: "O1",
            group_id: None,
            elevated_by: Some("O1"),
            now,
        }
    }

    fn audit<'a>(id: &'a str, now: i64) -> NewAdminAudit<'a> {
        NewAdminAudit {
            id,
            at: now,
            actor_kind: "system",
            actor_id: "O1",
            action: "kek.release",
            target_kind: "branch",
            target_id: "B1",
            reason: None,
            summary: "{}",
            request_id: "REQ",
        }
    }

    async fn decide(
        db: &ControlDb,
        key: &str,
        request_id: &str,
        approve: bool,
        now: i64,
    ) -> KeyDecisionOutcome {
        let c = cmd(key, "{}", now);
        let audit_id = format!("A-{key}");
        let a = audit(&audit_id, now);
        db.apply_key_release(&KeyDecision {
            command: &c,
            request_id,
            approve,
            wrapped_kek: approve.then_some("V3JhcHBlZA=="),
            audit: &a,
        })
        .await
        .unwrap()
    }

    async fn audit_count(db: &ControlDb) -> usize {
        db.admin_audit_rows().await.unwrap().len()
    }

    #[tokio::test]
    async fn an_approval_decides_the_row_writes_the_command_and_one_audit_row_together() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", Some(5), false).await;
        put(&db, "R1", "I1", "age1a", 10).await;

        let KeyDecisionOutcome::Applied(row) = decide(&db, "K1", "R1", true, 20).await else {
            panic!("not applied");
        };
        assert_eq!(row.kind, "backup.restore");
        assert_eq!(row.elevated_by.as_deref(), Some("O1"));
        let req = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(req.state, KeyRequestState::Approved);
        assert_eq!(
            (req.decided_at, req.decided_by.as_deref()),
            (Some(20), Some("O1"))
        );
        assert_eq!(audit_count(&db).await, 1);

        // The same post again finds its first row and writes nothing.
        assert!(matches!(
            decide(&db, "K1", "R1", true, 30).await,
            KeyDecisionOutcome::Replayed(r) if r.id == row.id
        ));
        assert_eq!(audit_count(&db).await, 1);
        // Another post for the approved request: its key is out.
        assert_eq!(
            decide(&db, "K2", "R1", true, 40).await,
            KeyDecisionOutcome::Refused(KeyRefusal::AlreadyReleased)
        );
        assert_eq!(
            decide(&db, "K3", "R1", false, 40).await,
            KeyDecisionOutcome::Refused(KeyRefusal::NotPending)
        );
        assert_eq!(audit_count(&db).await, 1);
        assert!(
            db.command(row.id + 1).await.unwrap().is_none(),
            "no stray row"
        );
    }

    #[tokio::test]
    async fn a_denial_decides_the_row_and_hands_out_nothing() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", Some(5), false).await;
        put(&db, "R1", "I1", "age1a", 10).await;
        assert!(matches!(
            decide(&db, "K1", "R1", false, 20).await,
            KeyDecisionOutcome::Applied(_)
        ));
        let req = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(req.state, KeyRequestState::Denied);
        assert_eq!(audit_count(&db).await, 1);
        assert_eq!(
            db.fetch_key_release("B1", "I1", 25).await.unwrap(),
            KeyFetch::State(KeyRequestState::Denied)
        );
        assert_eq!(
            decide(&db, "K2", "R1", true, 30).await,
            KeyDecisionOutcome::Refused(KeyRefusal::NotPending)
        );
    }

    #[tokio::test]
    async fn the_same_public_key_is_released_once_per_branch_and_a_revoked_hub_gets_none() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", Some(5), false).await;
        put(&db, "R1", "I1", "age1same", 10).await;
        put(&db, "R2", "I2", "age1same", 11).await;
        assert!(matches!(
            decide(&db, "K1", "R1", true, 20).await,
            KeyDecisionOutcome::Applied(_)
        ));
        // The owner replaced the Hub again; the new instance asked with the same one-time key.
        db.transact(vec![
            Stmt::new("UPDATE hubs SET revoked_at = 25 WHERE id = 'H1'", vec![]).expect(1),
        ])
        .await
        .unwrap();
        hub(&db, "H2", "I2", Some(5), false).await;
        assert_eq!(
            decide(&db, "K2", "R2", true, 30).await,
            KeyDecisionOutcome::Refused(KeyRefusal::AlreadyReleased)
        );
        assert_eq!(
            db.key_request("B1", "I2").await.unwrap().unwrap().state,
            KeyRequestState::Pending
        );

        hub(&db, "H3", "I3", Some(5), true).await;
        put(&db, "R3", "I3", "age1other", 12).await;
        assert_eq!(
            decide(&db, "K3", "R3", true, 40).await,
            KeyDecisionOutcome::Refused(KeyRefusal::HubRevoked)
        );
        assert_eq!(
            decide(&db, "K4", "NOPE", true, 40).await,
            KeyDecisionOutcome::Refused(KeyRefusal::NotFound)
        );
        assert_eq!(audit_count(&db).await, 1, "refusals write no audit row");
    }

    #[tokio::test]
    async fn the_blob_is_handed_out_once_and_cleared() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", Some(5), false).await;
        assert_eq!(
            db.fetch_key_release("B1", "I1", 5).await.unwrap(),
            KeyFetch::NoRequest
        );
        put(&db, "R1", "I1", "age1a", 10).await;
        assert_eq!(
            db.fetch_key_release("B1", "I1", 15).await.unwrap(),
            KeyFetch::State(KeyRequestState::Pending)
        );
        decide(&db, "K1", "R1", true, 20).await;
        assert_eq!(
            db.fetch_key_release("B1", "I1", 25).await.unwrap(),
            KeyFetch::Released("V3JhcHBlZA==".to_owned())
        );
        assert_eq!(
            db.fetch_key_release("B1", "I1", 26).await.unwrap(),
            KeyFetch::AlreadyFetched
        );
        let rows = db
            .query("SELECT fetched_at, wrapped_kek FROM key_requests", vec![])
            .await
            .unwrap();
        assert_eq!(rows[0][0], SqlValue::Integer(25));
        assert_eq!(rows[0][1], SqlValue::Null);
    }

    #[tokio::test]
    async fn a_request_pending_past_seven_days_expires_with_one_audit_row() {
        let (db, _dir) = db().await;
        put(&db, "R1", "I1", "age1a", 10).await;
        put(&db, "R2", "I2", "age1b", 8 * DAY).await;
        assert_eq!(db.expire_key_requests("B1", 7 * DAY).await.unwrap(), 0);
        let now = 7 * DAY + 11;
        assert_eq!(db.expire_key_requests("B1", now).await.unwrap(), 1);
        assert_eq!(db.expire_key_requests("B1", now).await.unwrap(), 0);
        let first = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(first.state, KeyRequestState::Expired);
        assert_eq!(
            db.key_request("B1", "I2").await.unwrap().unwrap().state,
            KeyRequestState::Pending
        );
        let rows = db.admin_audit_rows().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].actor_kind.as_str(), rows[0].actor_id.as_str()),
            ("system", "O1")
        );
        assert_eq!(rows[0].action, "kek.request.expire");
        assert!(rows[0].summary.contains("I1") && rows[0].summary.contains("expired"));
        assert_eq!(
            decide(&db, "K1", "R1", true, now).await,
            KeyDecisionOutcome::Refused(KeyRefusal::NotPending)
        );
        assert_eq!(
            db.fetch_key_release("B1", "I1", now).await.unwrap(),
            KeyFetch::State(KeyRequestState::Expired)
        );
    }

    #[tokio::test]
    async fn the_owner_is_offered_the_newest_unexpired_pending_request_of_a_live_hub() {
        let (db, _dir) = db().await;
        hub(&db, "H1", "I1", Some(5), true).await;
        hub(&db, "H2", "I2", Some(5), false).await;
        put(&db, "R1", "I1", "age1a", 100).await;
        assert!(db.pending_key_request("B1", 200).await.unwrap().is_none());
        put(&db, "R2", "I2", "age1b", 150).await;
        let row = db.pending_key_request("B1", 200).await.unwrap().unwrap();
        assert_eq!(row.id, "R2");
        assert!(
            db.pending_key_request("B1", 150 + 7 * DAY + 1)
                .await
                .unwrap()
                .is_none()
        );
    }
}

//! A replacement Hub's request for the branch key (Story 9.9, AD-24): `key_requests` in
//! `gz_control`, and the test that tells a replacement Hub from a first one.
//!
//! The table holds the one-time public key and where the request stands, nothing secret. Story 9.10
//! reads it to show the owner the pending request, and decides, expires and replay-checks it.

use arena_proto::KeyRequestState;

use super::{
    ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_opt_text, col_text, text,
};

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
    /// `pending` row changes (a new key and time); an approved, denied or expired one stays as it
    /// is. Returns the state the row is in afterwards.
    pub async fn put_key_request(
        &self,
        id: &str,
        branch_id: &str,
        hub_instance_id: &str,
        public_key: &str,
        now: i64,
    ) -> Result<KeyRequestState, ControlError> {
        let rows = self
            .transact_rows(vec![
                Stmt::new(
                    "INSERT INTO key_requests (id, branch_id, hub_instance_id, public_key, state, \
                     requested_at) VALUES (?1, ?2, ?3, ?4, 'pending', ?5) \
                     ON CONFLICT (branch_id, hub_instance_id) DO UPDATE SET \
                        public_key = excluded.public_key, requested_at = excluded.requested_at \
                     WHERE key_requests.state = 'pending'",
                    vec![
                        text(id),
                        text(branch_id),
                        text(hub_instance_id),
                        text(public_key),
                        SqlValue::Integer(now),
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

/// One row of `key_requests`.
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
}

impl ControlDb {
    /// The request of `hub_instance_id` for `branch_id`, if it made one.
    pub async fn key_request(
        &self,
        branch_id: &str,
        hub_instance_id: &str,
    ) -> Result<Option<KeyRequestRow>, ControlError> {
        let rows = self
            .query(
                "SELECT id, branch_id, hub_instance_id, public_key, state, requested_at, \
                 decided_at, decided_by FROM key_requests \
                 WHERE branch_id = ? AND hub_instance_id = ?",
                vec![text(branch_id), text(hub_instance_id)],
            )
            .await?;
        rows.first()
            .map(|r| {
                Ok(KeyRequestRow {
                    id: col_text(r, 0)?,
                    branch_id: col_text(r, 1)?,
                    hub_instance_id: col_text(r, 2)?,
                    public_key: col_text(r, 3)?,
                    state: KeyRequestState::parse(&col_text(r, 4)?).ok_or_else(|| {
                        ControlError::Db("key_requests has an unknown state".to_owned())
                    })?,
                    requested_at: col_int(r, 5)?,
                    decided_at: col_opt_int(r, 6)?,
                    decided_by: col_opt_text(r, 7)?,
                })
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn only_a_pending_request_is_refreshed_and_a_decided_one_stays() {
        let (db, _dir) = db().await;
        let put = |id: &'static str, key: &'static str, now: i64| {
            let db = &db;
            async move { db.put_key_request(id, "B1", "I1", key, now).await.unwrap() }
        };
        assert_eq!(put("R1", "age1a", 10).await, KeyRequestState::Pending);
        assert_eq!(put("R2", "age1b", 20).await, KeyRequestState::Pending);
        let row = db.key_request("B1", "I1").await.unwrap().unwrap();
        assert_eq!(
            (row.id.as_str(), row.public_key.as_str(), row.requested_at),
            ("R1", "age1b", 20)
        );

        // Story 9.10 decides: the Hub asking again changes nothing.
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
        db.put_key_request("R1", "B1", "I1", "age1a", 10)
            .await
            .unwrap();
        db.put_key_request("R2", "B1", "I2", "age1b", 11)
            .await
            .unwrap();
        assert_eq!(db.key_request("B1", "I2").await.unwrap().unwrap().id, "R2");
    }
}

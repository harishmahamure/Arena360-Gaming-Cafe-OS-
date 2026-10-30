//! Owner alerts, web push subscriptions and the owner's alert settings (Story 8.33). The
//! `owner_alert_deliveries` row is the once-only guard of a push: it is claimed before the push
//! is sent. WhatsApp is guarded by the send ledger (`send_ledger.rs`), not by this table.

use super::{ControlDb, ControlError, SqlValue, Stmt, col_int, col_opt_int, col_text, text};

/// Alerts older than this are deleted on each raise and never listed (90 days).
pub const OWNER_ALERT_RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// An alert about to be stored. `params` is a JSON object of display parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOwnerAlert<'a> {
    pub id: &'a str,
    pub tenant_id: &'a str,
    pub branch_id: &'a str,
    pub kind: &'a str,
    pub params: &'a str,
    pub now: i64,
}

/// A stored alert with its branch's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerAlertRow {
    pub id: String,
    pub tenant_id: String,
    pub branch_id: String,
    pub branch_name: String,
    pub kind: String,
    pub params: String,
    pub created_at: i64,
}

/// A browser an owner turned push on in. The endpoint, keys and agent are never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct PushSubscriptionRow {
    pub id: String,
    pub owner_user_id: String,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub user_agent: String,
    pub created_at: i64,
    pub last_success_at: Option<i64>,
}

impl std::fmt::Debug for PushSubscriptionRow {
    /// Ids and times only: the endpoint and keys are secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushSubscriptionRow")
            .field("id", &self.id)
            .field("owner_user_id", &self.owner_user_id)
            .field("created_at", &self.created_at)
            .field("last_success_at", &self.last_success_at)
            .finish_non_exhaustive()
    }
}

/// A subscription to store (or refresh, when its endpoint is already known).
#[derive(Clone, PartialEq, Eq)]
pub struct UpsertSubscription<'a> {
    pub id: &'a str,
    pub owner_user_id: &'a str,
    pub endpoint: &'a str,
    pub p256dh: &'a str,
    pub auth: &'a str,
    pub user_agent: &'a str,
    pub now: i64,
    /// A new endpoint is refused when the owner already has this many.
    pub max_per_owner: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertedSubscription {
    Saved,
    /// The owner is at the limit and the endpoint is new.
    TooMany,
}

const ALERT_COLUMNS: &str =
    "a.id, a.tenant_id, a.branch_id, b.name, a.kind, a.params, a.created_at";

fn alert_from(r: &[SqlValue]) -> Result<OwnerAlertRow, ControlError> {
    Ok(OwnerAlertRow {
        id: col_text(r, 0)?,
        tenant_id: col_text(r, 1)?,
        branch_id: col_text(r, 2)?,
        branch_name: col_text(r, 3)?,
        kind: col_text(r, 4)?,
        params: col_text(r, 5)?,
        created_at: col_int(r, 6)?,
    })
}

const SUBSCRIPTION_COLUMNS: &str =
    "id, owner_user_id, endpoint, p256dh, auth, user_agent, created_at, last_success_at";

fn subscription_from(r: &[SqlValue]) -> Result<PushSubscriptionRow, ControlError> {
    Ok(PushSubscriptionRow {
        id: col_text(r, 0)?,
        owner_user_id: col_text(r, 1)?,
        endpoint: col_text(r, 2)?,
        p256dh: col_text(r, 3)?,
        auth: col_text(r, 4)?,
        user_agent: col_text(r, 5)?,
        created_at: col_int(r, 6)?,
        last_success_at: col_opt_int(r, 7)?,
    })
}

impl ControlDb {
    /// Stores `new` unless an alert with its id exists. Answers whether it was stored.
    pub async fn owner_alert_insert(&self, new: &NewOwnerAlert<'_>) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "INSERT INTO owner_alerts (id, tenant_id, branch_id, kind, params, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO NOTHING",
                vec![
                    text(new.id),
                    text(new.tenant_id),
                    text(new.branch_id),
                    text(new.kind),
                    text(new.params),
                    SqlValue::Integer(new.now),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    /// The alert with `id`, whatever tenant it belongs to.
    pub async fn owner_alert(&self, id: &str) -> Result<Option<OwnerAlertRow>, ControlError> {
        let rows = self
            .query(
                &format!(
                    "SELECT {ALERT_COLUMNS} FROM owner_alerts a \
                     JOIN branches b ON b.id = a.branch_id WHERE a.id = ?"
                ),
                vec![text(id)],
            )
            .await?;
        rows.first().map(|r| alert_from(r)).transpose()
    }

    /// The tenant's alerts created at or after `since`, newest first. `before` is the
    /// `(created_at, id)` of the last alert of the previous page.
    pub async fn owner_alerts(
        &self,
        tenant_id: &str,
        since: i64,
        before: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<OwnerAlertRow>, ControlError> {
        let mut sql = format!(
            "SELECT {ALERT_COLUMNS} FROM owner_alerts a \
             JOIN branches b ON b.id = a.branch_id \
             WHERE a.tenant_id = ? AND a.created_at >= ?"
        );
        let mut args = vec![text(tenant_id), SqlValue::Integer(since)];
        if let Some((at, id)) = before {
            sql.push_str(" AND (a.created_at < ? OR (a.created_at = ? AND a.id < ?))");
            args.push(SqlValue::Integer(at));
            args.push(SqlValue::Integer(at));
            args.push(text(id));
        }
        sql.push_str(" ORDER BY a.created_at DESC, a.id DESC LIMIT ?");
        args.push(SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        let rows = self.query(&sql, args).await?;
        rows.iter().map(|r| alert_from(r)).collect()
    }

    /// Deletes every alert created before `cutoff`, with its delivery rows.
    pub async fn owner_alerts_prune(&self, cutoff: i64) -> Result<(), ControlError> {
        let at = SqlValue::Integer(cutoff);
        self.transact(vec![
            Stmt::new(
                "DELETE FROM owner_alert_deliveries WHERE alert_id IN \
                 (SELECT id FROM owner_alerts WHERE created_at < ?)",
                vec![at.clone()],
            ),
            Stmt::new("DELETE FROM owner_alerts WHERE created_at < ?", vec![at]),
        ])
        .await?;
        Ok(())
    }

    /// Claims the push of `alert_id` to `target` (a subscription id). Only the caller that gets
    /// `true` may send it.
    pub async fn owner_alert_claim(
        &self,
        alert_id: &str,
        target: &str,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "INSERT INTO owner_alert_deliveries (alert_id, target) VALUES (?, ?) \
                 ON CONFLICT (alert_id, target) DO NOTHING",
                vec![text(alert_id), text(target)],
            )
            .await?;
        Ok(n == 1)
    }

    /// Stores a subscription, or refreshes the one with the same endpoint (its owner, keys and
    /// agent). A new endpoint is refused once the owner has `max_per_owner`.
    pub async fn push_subscription_upsert(
        &self,
        s: &UpsertSubscription<'_>,
    ) -> Result<UpsertedSubscription, ControlError> {
        let n = self
            .execute(
                "INSERT INTO push_subscriptions \
                 (id, owner_user_id, endpoint, p256dh, auth, user_agent, created_at) \
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 \
                 WHERE EXISTS (SELECT 1 FROM push_subscriptions WHERE endpoint = ?3) \
                    OR (SELECT COUNT(*) FROM push_subscriptions WHERE owner_user_id = ?2) < ?8 \
                 ON CONFLICT (endpoint) DO UPDATE SET owner_user_id = excluded.owner_user_id, \
                    p256dh = excluded.p256dh, auth = excluded.auth, \
                    user_agent = excluded.user_agent",
                vec![
                    text(s.id),
                    text(s.owner_user_id),
                    text(s.endpoint),
                    text(s.p256dh),
                    text(s.auth),
                    text(s.user_agent),
                    SqlValue::Integer(s.now),
                    SqlValue::Integer(i64::from(s.max_per_owner)),
                ],
            )
            .await?;
        Ok(if n == 0 {
            UpsertedSubscription::TooMany
        } else {
            UpsertedSubscription::Saved
        })
    }

    /// Removes the owner's subscription with `endpoint`. Answers whether there was one.
    pub async fn push_subscription_remove(
        &self,
        owner_id: &str,
        endpoint: &str,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "DELETE FROM push_subscriptions WHERE owner_user_id = ? AND endpoint = ?",
                vec![text(owner_id), text(endpoint)],
            )
            .await?;
        Ok(n == 1)
    }

    /// Removes a subscription the push service said is gone.
    pub async fn push_subscription_delete(&self, id: &str) -> Result<(), ControlError> {
        self.execute(
            "DELETE FROM push_subscriptions WHERE id = ?",
            vec![text(id)],
        )
        .await?;
        Ok(())
    }

    /// The owner's subscriptions, oldest first.
    pub async fn push_subscriptions(
        &self,
        owner_id: &str,
    ) -> Result<Vec<PushSubscriptionRow>, ControlError> {
        let rows = self
            .query(
                &format!(
                    "SELECT {SUBSCRIPTION_COLUMNS} FROM push_subscriptions \
                     WHERE owner_user_id = ? ORDER BY created_at, id"
                ),
                vec![text(owner_id)],
            )
            .await?;
        rows.iter().map(|r| subscription_from(r)).collect()
    }

    /// How many subscriptions the owner has.
    pub async fn push_subscription_count(&self, owner_id: &str) -> Result<u32, ControlError> {
        let rows = self
            .query(
                "SELECT COUNT(*) FROM push_subscriptions WHERE owner_user_id = ?",
                vec![text(owner_id)],
            )
            .await?;
        let n = rows
            .first()
            .map(|r| col_int(r, 0))
            .transpose()?
            .unwrap_or(0);
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// Records a push the service accepted.
    pub async fn push_subscription_succeeded(
        &self,
        id: &str,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE push_subscriptions SET last_success_at = ? WHERE id = ?",
            vec![SqlValue::Integer(now), text(id)],
        )
        .await?;
        Ok(())
    }

    /// Whether the owner's alerts also go out on WhatsApp (on until switched off).
    pub async fn owner_alert_whatsapp(&self, owner_id: &str) -> Result<bool, ControlError> {
        let rows = self
            .query(
                "SELECT whatsapp_enabled FROM owner_alert_settings WHERE owner_id = ?",
                vec![text(owner_id)],
            )
            .await?;
        Ok(rows
            .first()
            .map(|r| col_int(r, 0))
            .transpose()?
            .is_none_or(|v| v != 0))
    }

    pub async fn set_owner_alert_whatsapp(
        &self,
        owner_id: &str,
        enabled: bool,
    ) -> Result<(), ControlError> {
        self.execute(
            "INSERT INTO owner_alert_settings (owner_id, whatsapp_enabled) VALUES (?, ?) \
             ON CONFLICT (owner_id) DO UPDATE SET whatsapp_enabled = excluded.whatsapp_enabled",
            vec![text(owner_id), SqlValue::Integer(i64::from(enabled))],
        )
        .await?;
        Ok(())
    }
}

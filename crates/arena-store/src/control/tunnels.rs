//! The cloud's record of a branch's Cloudflare Tunnel (Story 8.39): which tunnel and Access
//! application exist, whether the credential was handed to the Hub, and whether the row is being
//! deleted. The credential is never stored here.

use super::{ControlDb, ControlError, SqlValue, col_int, col_opt_int, col_text, text};

/// Where a tunnel row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelRowState {
    /// The tunnel and its Access application exist.
    Ready,
    /// The flag went away: the provider is to remove both, then the row goes.
    Deleting,
}

impl TunnelRowState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Deleting => "deleting",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Ready, Self::Deleting]
            .into_iter()
            .find(|v| v.as_str() == s)
    }
}

/// One branch's tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTunnel {
    pub branch_id: String,
    pub hostname: String,
    pub tunnel_id: String,
    pub access_app_id: String,
    pub state: TunnelRowState,
    /// When the credential was last handed to the Hub (cloud ms); `None` until then.
    pub delivered_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A tunnel to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranchTunnel<'a> {
    pub branch_id: &'a str,
    pub hostname: &'a str,
    pub tunnel_id: &'a str,
    pub access_app_id: &'a str,
    pub now: i64,
}

const COLUMNS: &str = "branch_id, hostname, tunnel_id, access_app_id, state, delivered_at, \
                       created_at, updated_at";

fn tunnel_from(r: &[SqlValue]) -> Result<BranchTunnel, ControlError> {
    let state = col_text(r, 4)?;
    Ok(BranchTunnel {
        branch_id: col_text(r, 0)?,
        hostname: col_text(r, 1)?,
        tunnel_id: col_text(r, 2)?,
        access_app_id: col_text(r, 3)?,
        state: TunnelRowState::parse(&state)
            .ok_or_else(|| ControlError::Db(format!("unknown tunnel state {state:?}")))?,
        delivered_at: col_opt_int(r, 5)?,
        created_at: col_int(r, 6)?,
        updated_at: col_int(r, 7)?,
    })
}

impl ControlDb {
    /// The tunnel recorded for `branch_id`.
    pub async fn branch_tunnel(
        &self,
        branch_id: &str,
    ) -> Result<Option<BranchTunnel>, ControlError> {
        let sql = format!("SELECT {COLUMNS} FROM branch_tunnels WHERE branch_id = ?");
        let rows = self.query(&sql, vec![text(branch_id)]).await?;
        rows.first().map(|r| tunnel_from(r)).transpose()
    }

    /// Records a tunnel as `ready` and not yet delivered. An existing row of the branch is
    /// replaced (a retry after a failed delivery).
    pub async fn insert_branch_tunnel(&self, t: &NewBranchTunnel<'_>) -> Result<(), ControlError> {
        self.execute(
            "INSERT INTO branch_tunnels (branch_id, hostname, tunnel_id, access_app_id, state, \
                delivered_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 'ready', NULL, ?5, ?5) \
             ON CONFLICT (branch_id) DO UPDATE SET hostname = excluded.hostname, \
                tunnel_id = excluded.tunnel_id, access_app_id = excluded.access_app_id, \
                state = 'ready', delivered_at = NULL, updated_at = excluded.updated_at",
            vec![
                text(t.branch_id),
                text(t.hostname),
                text(t.tunnel_id),
                text(t.access_app_id),
                SqlValue::Integer(t.now),
            ],
        )
        .await?;
        Ok(())
    }

    /// Notes that the credential was handed to the Hub at `now`.
    pub async fn mark_tunnel_delivered(
        &self,
        branch_id: &str,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE branch_tunnels SET delivered_at = ?, updated_at = ? WHERE branch_id = ?",
            vec![
                SqlValue::Integer(now),
                SqlValue::Integer(now),
                text(branch_id),
            ],
        )
        .await?;
        Ok(())
    }

    /// Moves the row to `state`.
    pub async fn set_tunnel_state(
        &self,
        branch_id: &str,
        state: TunnelRowState,
        now: i64,
    ) -> Result<(), ControlError> {
        self.execute(
            "UPDATE branch_tunnels SET state = ?, updated_at = ? WHERE branch_id = ?",
            vec![
                text(state.as_str()),
                SqlValue::Integer(now),
                text(branch_id),
            ],
        )
        .await?;
        Ok(())
    }

    /// Removes the row (after the provider deleted the tunnel and the Access application).
    pub async fn delete_branch_tunnel(&self, branch_id: &str) -> Result<(), ControlError> {
        self.execute(
            "DELETE FROM branch_tunnels WHERE branch_id = ?",
            vec![text(branch_id)],
        )
        .await?;
        Ok(())
    }
}

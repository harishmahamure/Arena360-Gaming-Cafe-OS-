//! `owners`: café owners. A row exists only after a correct sign-up code.

use arena_proto::MfaState;

use super::{ControlDb, ControlError, SqlValue, col_int, col_text, text};

/// An owner row.
#[derive(Clone, PartialEq, Eq)]
pub struct Owner {
    pub id: String,
    pub name: String,
    pub phone_e164: String,
    pub email: String,
    pub cafe_name: String,
    pub city: String,
    pub mfa_state: MfaState,
    pub created_at: i64,
}

impl std::fmt::Debug for Owner {
    /// Ids and state only: never the personal details.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owner")
            .field("id", &self.id)
            .field("mfa_state", &self.mfa_state)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

/// The details an owner signs up with.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NewOwner {
    pub name: String,
    pub phone_e164: String,
    pub email: String,
    pub cafe_name: String,
    pub city: String,
}

impl std::fmt::Debug for NewOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewOwner").finish_non_exhaustive()
    }
}

pub(super) const OWNER_COLUMNS: &str =
    "id, name, phone_e164, email, cafe_name, city, mfa_state, created_at";

/// An owner from a row of [`OWNER_COLUMNS`] starting at column `at`.
pub(super) fn owner_from(row: &[SqlValue], at: usize) -> Result<Owner, ControlError> {
    let state = col_text(row, at + 6)?;
    Ok(Owner {
        id: col_text(row, at)?,
        name: col_text(row, at + 1)?,
        phone_e164: col_text(row, at + 2)?,
        email: col_text(row, at + 3)?,
        cafe_name: col_text(row, at + 4)?,
        city: col_text(row, at + 5)?,
        mfa_state: MfaState::parse(&state)
            .ok_or_else(|| ControlError::Db(format!("unknown mfa_state {state:?}")))?,
        created_at: col_int(row, at + 7)?,
    })
}

impl ControlDb {
    /// The owner with this E.164 phone.
    pub async fn owner_by_phone(&self, phone_e164: &str) -> Result<Option<Owner>, ControlError> {
        let rows = self
            .query(
                "SELECT id, name, phone_e164, email, cafe_name, city, mfa_state, created_at \
                 FROM owners WHERE phone_e164 = ?",
                vec![text(phone_e164)],
            )
            .await?;
        rows.first().map(|r| owner_from(r, 0)).transpose()
    }

    /// The owner with this id.
    pub async fn owner_by_id(&self, id: &str) -> Result<Option<Owner>, ControlError> {
        let rows = self
            .query(
                "SELECT id, name, phone_e164, email, cafe_name, city, mfa_state, created_at \
                 FROM owners WHERE id = ?",
                vec![text(id)],
            )
            .await?;
        rows.first().map(|r| owner_from(r, 0)).transpose()
    }

    /// How many owners exist.
    pub async fn owner_count(&self) -> Result<u64, ControlError> {
        let rows = self.query("SELECT COUNT(*) FROM owners", vec![]).await?;
        let n = rows
            .first()
            .map(|r| col_int(r, 0))
            .transpose()?
            .unwrap_or(0);
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Sets an owner's second-factor state; `false` when there is no such owner.
    pub async fn set_owner_mfa_state(
        &self,
        id: &str,
        state: MfaState,
    ) -> Result<bool, ControlError> {
        let n = self
            .execute(
                "UPDATE owners SET mfa_state = ? WHERE id = ?",
                vec![text(state.as_str()), text(id)],
            )
            .await?;
        Ok(n == 1)
    }
}

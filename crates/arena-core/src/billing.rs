use arena_proto::{AppError, BillingRules};
use arena_store::{StoreTx, list_settings};

pub fn read(tx: &StoreTx<'_>) -> Result<BillingRules, AppError> {
    let rows = list_settings(tx)?;
    Ok(BillingRules::from_lookup(|key| {
        rows.iter().find(|r| r.key == key).map(|r| r.value.clone())
    }))
}

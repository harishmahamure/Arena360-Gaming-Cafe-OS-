use arena_proto::{AppError, KEY_DEVICE_CAP, KEY_LICENCE_FLAGS, KEY_USER_CAP, Tier};
use arena_store::{StoreTx, find_setting};

pub fn limit(tx: &StoreTx<'_>, name: &str) -> Result<Option<i64>, AppError> {
    let (key, free) = match name {
        "devices" => (KEY_DEVICE_CAP, Tier::Free.defaults().limits.devices),
        "users" => (KEY_USER_CAP, Tier::Free.defaults().limits.users),
        _ => return Ok(None),
    };
    Ok(Some(
        find_setting(tx, key)?
            .and_then(|s| s.value.parse().ok())
            .unwrap_or(i64::from(free)),
    ))
}

pub fn has_flag(tx: &StoreTx<'_>, name: &str) -> Result<bool, AppError> {
    Ok(find_setting(tx, KEY_LICENCE_FLAGS)?.is_some_and(|s| arena_proto::has_flag(&s.value, name)))
}

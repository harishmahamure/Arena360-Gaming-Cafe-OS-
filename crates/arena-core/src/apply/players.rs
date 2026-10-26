use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, IntentPayload, PlayerCreate, valid_policy_version,
    valid_tags, valid_username,
};
use arena_store::{
    ChangeOp, PlayerRow, RowKey, StoreTx, insert_consent, insert_player, insert_player_key,
    phone_taken, username_taken,
};

use super::mutate::Written;
use crate::audit::base64url_decode;

const MAX_ENC_CHARS: usize = 4096;
const MIN_ENC_BYTES: usize = 28;

#[derive(Default)]
pub(crate) struct PlayerTarget {
    username_taken: bool,
    phone_taken: bool,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<PlayerTarget, AppError> {
    let IntentPayload::PlayerCreate(c) = p else {
        return Ok(PlayerTarget::default());
    };
    Ok(PlayerTarget {
        username_taken: username_taken(tx, &c.username)?,
        phone_taken: phone_taken(tx, &c.phone_lookup)?,
    })
}

fn sealed(s: &str) -> bool {
    s.len() <= MAX_ENC_CHARS && base64url_decode(s).is_some_and(|b| b.len() >= MIN_ENC_BYTES)
}

pub(super) fn validate(p: &PlayerCreate, t: &PlayerTarget) -> Result<(), AppError> {
    if !super::is_ulid(&p.player_id) {
        return Err(invalid("player_id", "player_id must be a ULID"));
    }
    if !valid_username(&p.username) {
        return Err(invalid("username", "username is not valid"));
    }
    if !p.password_hash.starts_with("$argon2id$") {
        return Err(invalid(
            "password_hash",
            "password must be an argon2id hash",
        ));
    }
    let lookup = &p.phone_lookup;
    if lookup.len() != 64
        || !lookup
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid(
            "phone_lookup",
            "phone_lookup must be a SHA-256 hex digest",
        ));
    }
    let required = [
        ("name_enc", Some(&p.name_enc)),
        ("phone_enc", Some(&p.phone_enc)),
        ("wrapped_key", Some(&p.wrapped_key)),
        ("email_enc", p.email_enc.as_ref()),
        ("dob_enc", p.dob_enc.as_ref()),
        ("notes_enc", p.notes_enc.as_ref()),
    ];
    for (field, value) in required {
        if value.is_some_and(|v| !sealed(v)) {
            return Err(invalid(field, "must be sealed ciphertext"));
        }
    }
    if !valid_tags(&p.tags) {
        return Err(invalid("tags", "up to 10 tags of up to 24 characters each"));
    }
    if !valid_policy_version(&p.policy_version) {
        return Err(invalid("policy_version", "policy version is not valid"));
    }
    if t.username_taken {
        return Err(AppError::with_field(
            ErrorCode::UsernameTaken,
            "username",
            "that username is taken",
        ));
    }
    if t.phone_taken {
        return Err(AppError::with_field(
            ErrorCode::PhoneTaken,
            "phone",
            "that phone number already has a profile",
        ));
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    p: &PlayerCreate,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let tags = serde_json::to_string(&p.tags)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("encode tags: {e}")))?;
    insert_player(
        tx,
        &PlayerRow {
            player_id: p.player_id.clone(),
            username: p.username.clone(),
            password_hash: p.password_hash.clone(),
            phone_lookup: p.phone_lookup.clone(),
            name_enc: p.name_enc.clone(),
            phone_enc: p.phone_enc.clone(),
            email_enc: p.email_enc.clone(),
            dob_enc: p.dob_enc.clone(),
            notes_enc: p.notes_enc.clone(),
            tags,
        },
    )?;
    let consent_id = ulid::Ulid::generate().to_string();
    insert_consent(
        tx,
        &consent_id,
        &p.player_id,
        CONSENT_TRANSACTIONAL,
        &p.policy_version,
        p.consent,
        now_ms,
    )?;
    insert_player_key(tx, &p.player_id, &p.wrapped_key)?;
    Ok(vec![
        Written {
            key: RowKey::Players {
                player_id: p.player_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        },
        Written {
            key: RowKey::Consents { consent_id },
            op: ChangeOp::Insert,
            before: None,
        },
    ])
}

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, Intent, IntentPayload, PlayerCreate,
    PlayerPasswordChange, PlayerPasswordReset, PlayerUpdate, valid_policy_version, valid_tags,
    valid_username,
};
use arena_store::{
    ChangeOp, PlayerRow, RowKey, StoreTx, find_player, insert_consent, insert_player,
    insert_player_key, insert_wallet, latest_consent, phone_holder, set_player_password,
    update_player, username_taken,
};

use super::mutate::Written;
use crate::audit::base64url_decode;

const MAX_ENC_CHARS: usize = 4096;
const MIN_ENC_BYTES: usize = 28;

pub(crate) struct Existing {
    rev: i64,
    consent: Option<bool>,
    row: PlayerRow,
}

#[derive(Default)]
pub(crate) struct PlayerTarget {
    username_taken: bool,
    phone_holder: Option<String>,
    existing: Option<Existing>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub(super) fn load_create(tx: &StoreTx<'_>, c: &PlayerCreate) -> Result<PlayerTarget, AppError> {
    Ok(PlayerTarget {
        username_taken: username_taken(tx, &c.username)?,
        phone_holder: phone_holder(tx, &c.phone_lookup)?,
        existing: None,
    })
}

pub(super) fn load(tx: &StoreTx<'_>, p: &IntentPayload) -> Result<PlayerTarget, AppError> {
    let id = match p {
        IntentPayload::PlayerCreate(c) => return load_create(tx, c),
        IntentPayload::PlayerUpdate(u) => &u.player_id,
        IntentPayload::PlayerPasswordReset(r) => &r.player_id,
        IntentPayload::PlayerPasswordChange(c) => &c.player_id,
        _ => return Ok(PlayerTarget::default()),
    };
    let existing = match find_player(tx, id)? {
        Some((row, rev, _)) => Some(Existing {
            rev,
            consent: latest_consent(tx, id, CONSENT_TRANSACTIONAL)?.map(|c| c.0),
            row,
        }),
        None => None,
    };
    let phone_holder = match p {
        IntentPayload::PlayerUpdate(u) => phone_holder(tx, &u.phone_lookup)?,
        _ => None,
    };
    Ok(PlayerTarget {
        username_taken: false,
        phone_holder,
        existing,
    })
}

pub(super) fn base_rev(intent: &Intent, t: &PlayerTarget) -> Result<(), AppError> {
    if !matches!(intent.payload, IntentPayload::PlayerUpdate(_)) {
        return Ok(());
    }
    match (t.existing.as_ref(), intent.base_rev) {
        (Some(_), None) => Err(invalid("base_rev", "editing needs base_rev")),
        (Some(e), Some(base)) if e.rev != base => Err(AppError::with_field(
            ErrorCode::Stale,
            "base_rev",
            "this changed since it was read",
        )),
        _ => Ok(()),
    }
}

fn sealed(s: &str) -> bool {
    s.len() <= MAX_ENC_CHARS && base64url_decode(s).is_some_and(|b| b.len() >= MIN_ENC_BYTES)
}

fn check_fields(
    lookup: &str,
    sealed_fields: &[(&str, Option<&String>)],
    tags: &[String],
    policy_version: &str,
) -> Result<(), AppError> {
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
    for (field, value) in sealed_fields {
        if value.is_some_and(|v| !sealed(v)) {
            return Err(invalid(field, "must be sealed ciphertext"));
        }
    }
    if !valid_tags(tags) {
        return Err(invalid("tags", "up to 10 tags of up to 24 characters each"));
    }
    if !valid_policy_version(policy_version) {
        return Err(invalid("policy_version", "policy version is not valid"));
    }
    Ok(())
}

fn phone_taken_error() -> AppError {
    AppError::with_field(
        ErrorCode::PhoneTaken,
        "phone",
        "that phone number already has a profile",
    )
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
    check_fields(
        &p.phone_lookup,
        &[
            ("name_enc", Some(&p.name_enc)),
            ("phone_enc", Some(&p.phone_enc)),
            ("wrapped_key", Some(&p.wrapped_key)),
            ("email_enc", p.email_enc.as_ref()),
            ("dob_enc", p.dob_enc.as_ref()),
            ("notes_enc", p.notes_enc.as_ref()),
        ],
        &p.tags,
        &p.policy_version,
    )?;
    if t.username_taken {
        return Err(AppError::with_field(
            ErrorCode::UsernameTaken,
            "username",
            "that username is taken",
        ));
    }
    if t.phone_holder.is_some() {
        return Err(phone_taken_error());
    }
    Ok(())
}

fn existing(t: &PlayerTarget) -> Result<&Existing, AppError> {
    t.existing
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no such player"))
}

pub(super) fn validate_update(p: &PlayerUpdate, t: &PlayerTarget) -> Result<(), AppError> {
    existing(t)?;
    check_fields(
        &p.phone_lookup,
        &[
            ("name_enc", Some(&p.name_enc)),
            ("phone_enc", Some(&p.phone_enc)),
            ("email_enc", p.email_enc.as_ref()),
            ("dob_enc", p.dob_enc.as_ref()),
            ("notes_enc", p.notes_enc.as_ref()),
        ],
        &p.tags,
        &p.policy_version,
    )?;
    if t.phone_holder.as_ref().is_some_and(|h| *h != p.player_id) {
        return Err(phone_taken_error());
    }
    Ok(())
}

pub(super) fn validate_reset(p: &PlayerPasswordReset, t: &PlayerTarget) -> Result<(), AppError> {
    check_hash(&p.password_hash, t)
}

pub(super) fn validate_change(p: &PlayerPasswordChange, t: &PlayerTarget) -> Result<(), AppError> {
    check_hash(&p.password_hash, t)
}

fn check_hash(password_hash: &str, t: &PlayerTarget) -> Result<(), AppError> {
    existing(t)?;
    if !password_hash.starts_with("$argon2id$") {
        return Err(invalid(
            "password_hash",
            "password must be an argon2id hash",
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
    let consent_id = format!("{}:{CONSENT_TRANSACTIONAL}", p.player_id);
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
    insert_wallet(tx, &p.player_id)?;
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
        Written {
            key: RowKey::Wallets {
                player_id: p.player_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        },
    ])
}

pub(super) fn mutate_update(
    tx: &StoreTx<'_>,
    p: &PlayerUpdate,
    t: &PlayerTarget,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let e = existing(t)?;
    let tags = serde_json::to_string(&p.tags)
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("encode tags: {e}")))?;
    update_player(
        tx,
        &PlayerRow {
            phone_lookup: p.phone_lookup.clone(),
            name_enc: p.name_enc.clone(),
            phone_enc: p.phone_enc.clone(),
            email_enc: p.email_enc.clone(),
            dob_enc: p.dob_enc.clone(),
            notes_enc: p.notes_enc.clone(),
            tags,
            ..e.row.clone()
        },
    )?;
    let mut written = vec![Written {
        key: RowKey::Players {
            player_id: p.player_id.clone(),
        },
        op: ChangeOp::Update,
        before: None,
    }];
    if e.consent.unwrap_or(false) != p.consent {
        let consent_id = format!("{}:{CONSENT_TRANSACTIONAL}:{}", p.player_id, e.rev + 1);
        insert_consent(
            tx,
            &consent_id,
            &p.player_id,
            CONSENT_TRANSACTIONAL,
            &p.policy_version,
            p.consent,
            now_ms,
        )?;
        written.push(Written {
            key: RowKey::Consents { consent_id },
            op: ChangeOp::Insert,
            before: None,
        });
    }
    Ok(written)
}

/// Stores a new password hash: a staff reset (Story 3.8) or the player's own change (Story 6.34).
pub(super) fn mutate_reset(
    tx: &StoreTx<'_>,
    player_id: &str,
    password_hash: &str,
) -> Result<Vec<Written>, AppError> {
    set_player_password(tx, player_id, password_hash)?;
    Ok(vec![Written {
        key: RowKey::Players {
            player_id: player_id.to_owned(),
        },
        op: ChangeOp::Update,
        before: None,
    }])
}

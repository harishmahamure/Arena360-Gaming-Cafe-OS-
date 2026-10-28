//! `import.players` (Story 5.34): a chunk of up to 500 new players from a player CSV. Each gets
//! a profile with its personal data sealed, consent recorded as not granted, no password, a
//! wallet, an `import` ledger row for an opening balance (no cash event, no invoice) and a credit
//! account when a limit is set. All rows apply or none do, and the chunk's idempotency key makes a
//! retry a no-op.

use std::collections::HashSet;

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, CREDIT_LIMIT_MAX_MINOR, ErrorCode, IMPORT_CHUNK_ROWS,
    IMPORT_CREDIT_REMINDER_DAYS, IMPORT_NO_PASSWORD, ImportPlayerRow, ImportPlayers, Intent,
    WALLET_TOPUP_MAX_MINOR, valid_policy_version, valid_username,
};
use arena_store::{
    ChangeOp, PlayerRow, RowKey, StoreTx, adjust_wallet, insert_consent, insert_credit,
    insert_import_wallet_txn, insert_player, insert_player_key, insert_wallet, phone_holder,
    username_taken,
};

use super::mutate::Written;
use crate::audit::base64url_decode;

const MAX_ENC_CHARS: usize = 4096;
const MIN_ENC_BYTES: usize = 28;

/// Which rows of the chunk collide with a profile the Hub already holds.
#[derive(Default)]
pub(crate) struct PlayersTarget {
    taken_phone: Vec<bool>,
    taken_username: Vec<bool>,
}

fn invalid(row: usize, field: &str, message: &str) -> AppError {
    AppError::with_field(
        ErrorCode::ValidationFailed,
        format!("rows[{row}].{field}"),
        message,
    )
}

pub(super) fn load(tx: &StoreTx<'_>, p: &ImportPlayers) -> Result<PlayersTarget, AppError> {
    let mut t = PlayersTarget::default();
    for row in p.rows.iter().take(IMPORT_CHUNK_ROWS) {
        t.taken_phone
            .push(phone_holder(tx, &row.phone_lookup)?.is_some());
        t.taken_username.push(username_taken(tx, &row.username)?);
    }
    Ok(t)
}

fn sealed(s: &str) -> bool {
    s.len() <= MAX_ENC_CHARS && base64url_decode(s).is_some_and(|b| b.len() >= MIN_ENC_BYTES)
}

fn check_row(i: usize, row: &ImportPlayerRow) -> Result<(), AppError> {
    if !super::is_ulid(&row.player_id) {
        return Err(invalid(i, "player_id", "player_id must be a ULID"));
    }
    if !valid_username(&row.username) {
        return Err(invalid(i, "username", "username is not valid"));
    }
    let lookup = &row.phone_lookup;
    if lookup.len() != 64
        || !lookup
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid(
            i,
            "phone_lookup",
            "phone_lookup must be a SHA-256 hex digest",
        ));
    }
    for (field, value) in [
        ("name_enc", &row.name_enc),
        ("phone_enc", &row.phone_enc),
        ("wrapped_key", &row.wrapped_key),
    ] {
        if !sealed(value) {
            return Err(invalid(i, field, "must be sealed ciphertext"));
        }
    }
    if !valid_policy_version(&row.policy_version) {
        return Err(invalid(i, "policy_version", "policy version is not valid"));
    }
    if !(0..=WALLET_TOPUP_MAX_MINOR).contains(&row.balance_minor) {
        return Err(invalid(i, "balance_minor", "the balance is out of range"));
    }
    if !(0..=CREDIT_LIMIT_MAX_MINOR).contains(&row.credit_limit_minor) {
        return Err(invalid(
            i,
            "credit_limit_minor",
            "the credit limit is out of range",
        ));
    }
    Ok(())
}

pub(super) fn validate(p: &ImportPlayers, t: &PlayersTarget) -> Result<(), AppError> {
    if p.rows.is_empty() || p.rows.len() > IMPORT_CHUNK_ROWS {
        return Err(AppError::with_field(
            ErrorCode::ValidationFailed,
            "rows",
            "an import chunk carries 1 to 500 rows",
        ));
    }
    let mut ids = HashSet::new();
    let mut phones = HashSet::new();
    let mut names = HashSet::new();
    for (i, row) in p.rows.iter().enumerate() {
        check_row(i, row)?;
        if t.taken_username.get(i).copied().unwrap_or(true) || !names.insert(&row.username) {
            return Err(AppError::with_field(
                ErrorCode::UsernameTaken,
                format!("rows[{i}].username"),
                "that username is taken",
            ));
        }
        if t.taken_phone.get(i).copied().unwrap_or(true) || !phones.insert(&row.phone_lookup) {
            return Err(AppError::with_field(
                ErrorCode::PhoneTaken,
                format!("rows[{i}].phone"),
                "that phone number already has a profile",
            ));
        }
        if !ids.insert(&row.player_id) {
            return Err(invalid(i, "player_id", "player_id is repeated"));
        }
    }
    Ok(())
}

pub(super) fn mutate(
    tx: &StoreTx<'_>,
    intent: &Intent,
    p: &ImportPlayers,
    now_ms: i64,
) -> Result<Vec<Written>, AppError> {
    let (business_day, fy) = crate::locale::stamp(tx, now_ms)?.ok_or_else(|| {
        AppError::new(ErrorCode::Internal, "the branch has not been bootstrapped")
    })?;
    let mut out = Vec::new();
    for row in &p.rows {
        insert_player(
            tx,
            &PlayerRow {
                player_id: row.player_id.clone(),
                username: row.username.clone(),
                password_hash: IMPORT_NO_PASSWORD.to_owned(),
                phone_lookup: row.phone_lookup.clone(),
                name_enc: row.name_enc.clone(),
                phone_enc: row.phone_enc.clone(),
                email_enc: None,
                dob_enc: None,
                notes_enc: None,
                tags: "[]".to_owned(),
            },
        )?;
        out.push(Written {
            key: RowKey::Players {
                player_id: row.player_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        });
        let consent_id = format!("{}:{CONSENT_TRANSACTIONAL}", row.player_id);
        insert_consent(
            tx,
            &consent_id,
            &row.player_id,
            CONSENT_TRANSACTIONAL,
            &row.policy_version,
            false,
            now_ms,
        )?;
        out.push(Written {
            key: RowKey::Consents { consent_id },
            op: ChangeOp::Insert,
            before: None,
        });
        insert_player_key(tx, &row.player_id, &row.wrapped_key)?;
        insert_wallet(tx, &row.player_id)?;
        if row.balance_minor > 0 {
            adjust_wallet(tx, &row.player_id, row.balance_minor)?;
        }
        out.push(Written {
            key: RowKey::Wallets {
                player_id: row.player_id.clone(),
            },
            op: ChangeOp::Insert,
            before: None,
        });
        if row.balance_minor > 0 {
            let id = ulid::Ulid::generate().to_string();
            insert_import_wallet_txn(
                tx,
                &id,
                &row.player_id,
                row.balance_minor,
                &intent.actor.id,
                &business_day,
                &fy,
            )?;
            out.push(Written {
                key: RowKey::WalletTxns { id },
                op: ChangeOp::Insert,
                before: None,
            });
        }
        if row.credit_limit_minor > 0 {
            insert_credit(
                tx,
                &row.player_id,
                row.credit_limit_minor,
                &IMPORT_CREDIT_REMINDER_DAYS,
            )?;
            out.push(Written {
                key: RowKey::CreditBalances {
                    player_id: row.player_id.clone(),
                },
                op: ChangeOp::Insert,
                before: None,
            });
        }
    }
    Ok(out)
}

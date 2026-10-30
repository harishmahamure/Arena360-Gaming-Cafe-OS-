//! The player key backup (Story 9.2, AD-24): the whole `player_keys` table sealed with the branch
//! key, as the one object `keys/player-keys.bin`.
//!
//! `A360KEYS` ++ format version (1 byte) ++ a fresh 96-bit nonce ++ AES-256-GCM of a JSON list of
//! `{player_id, dek_wrapped}`, with the first nine bytes as the additional data. Every data key in
//! it is already wrapped by the branch key; sealing the table again keeps the player ids unreadable
//! to anyone without that key. An erased player's key is not in the table, so it is not in the
//! object: restoring from it cannot bring them back.

use arena_proto::{AppError, ErrorCode};
use arena_store::PlayerKeyRow;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::players::KEK_LEN;

/// What every key backup starts with.
pub const KEY_BACKUP_MAGIC: &[u8; 8] = b"A360KEYS";
/// The format version byte.
pub const KEY_BACKUP_VERSION: u8 = 1;
const HEADER_LEN: usize = KEY_BACKUP_MAGIC.len() + 1;

#[derive(Serialize, Deserialize)]
struct Table {
    keys: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    player_id: String,
    dek_wrapped: String,
}

fn bad(message: &str) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

fn header() -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..KEY_BACKUP_MAGIC.len()].copy_from_slice(KEY_BACKUP_MAGIC);
    h[KEY_BACKUP_MAGIC.len()] = KEY_BACKUP_VERSION;
    h
}

fn aead_key(kek: &[u8; KEK_LEN]) -> Result<LessSafeKey, AppError> {
    UnboundKey::new(&AES_256_GCM, kek)
        .map(LessSafeKey::new)
        .map_err(|_| bad("the branch key is not usable"))
}

/// Seals the key table for the cloud.
pub fn seal_key_table(kek: &[u8; KEK_LEN], rows: &[PlayerKeyRow]) -> Result<Vec<u8>, AppError> {
    let table = Table {
        keys: rows
            .iter()
            .map(|r| Entry {
                player_id: r.player_id.clone(),
                dek_wrapped: r.dek_wrapped.clone(),
            })
            .collect(),
    };
    let mut body =
        serde_json::to_vec(&table).map_err(|_| bad("the key table cannot be encoded"))?;
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| bad("the system random source failed"))?;
    let head = header();
    aead_key(kek)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(head),
            &mut body,
        )
        .map_err(|_| bad("the key table cannot be sealed"))?;
    let mut out = Vec::with_capacity(HEADER_LEN + NONCE_LEN + body.len());
    out.extend_from_slice(&head);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Opens a key backup with the branch key it was sealed with. Fails for any other key and for a
/// changed byte.
pub fn open_key_table(kek: &[u8; KEK_LEN], bytes: &[u8]) -> Result<Vec<PlayerKeyRow>, AppError> {
    let head = header();
    if bytes.len() < HEADER_LEN + NONCE_LEN || bytes[..HEADER_LEN] != head {
        return Err(bad("this is not a player key backup"));
    }
    let nonce: [u8; NONCE_LEN] = bytes[HEADER_LEN..HEADER_LEN + NONCE_LEN]
        .try_into()
        .map_err(|_| bad("this is not a player key backup"))?;
    let mut body = bytes[HEADER_LEN + NONCE_LEN..].to_vec();
    let plain = aead_key(kek)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(head),
            &mut body,
        )
        .map_err(|_| bad("the player key backup does not open with this branch key"))?;
    let table: Table =
        serde_json::from_slice(plain).map_err(|_| bad("the player key backup is damaged"))?;
    Ok(table
        .keys
        .into_iter()
        .map(|e| PlayerKeyRow {
            player_id: e.player_id,
            dek_wrapped: e.dek_wrapped,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEK: [u8; KEK_LEN] = [7; KEK_LEN];

    fn rows() -> Vec<PlayerKeyRow> {
        vec![
            PlayerKeyRow {
                player_id: "P1".into(),
                dek_wrapped: "wrapped-one".into(),
            },
            PlayerKeyRow {
                player_id: "P2".into(),
                dek_wrapped: "wrapped-two".into(),
            },
        ]
    }

    #[test]
    fn the_table_round_trips_and_hides_its_contents() {
        let sealed = seal_key_table(&KEK, &rows()).unwrap();
        assert!(sealed.starts_with(KEY_BACKUP_MAGIC));
        let text = String::from_utf8_lossy(&sealed);
        assert!(!text.contains("P1") && !text.contains("wrapped-one"));
        assert_eq!(open_key_table(&KEK, &sealed).unwrap(), rows());
    }

    #[test]
    fn an_empty_table_is_a_valid_backup_and_each_seal_uses_a_new_nonce() {
        let a = seal_key_table(&KEK, &[]).unwrap();
        assert_eq!(open_key_table(&KEK, &a).unwrap(), vec![]);
        assert_ne!(a, seal_key_table(&KEK, &[]).unwrap());
    }

    #[test]
    fn another_key_a_changed_byte_and_a_foreign_file_do_not_open() {
        let sealed = seal_key_table(&KEK, &rows()).unwrap();
        assert!(open_key_table(&[8; KEK_LEN], &sealed).is_err());
        let mut flipped = sealed.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert!(open_key_table(&KEK, &flipped).is_err());
        let mut versioned = sealed;
        versioned[KEY_BACKUP_MAGIC.len()] = 2;
        assert!(open_key_table(&KEK, &versioned).is_err());
        assert!(open_key_table(&KEK, b"not a backup").is_err());
    }
}

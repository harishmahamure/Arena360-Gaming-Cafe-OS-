use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, PlayerConsent, PlayerCreate, PlayerForm, PlayerHit,
    PlayerProfile, normalize_phone, normalize_username,
};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use crate::SnapshotReader;
use crate::audit::base64url_decode;
use crate::staff::base64url;

pub const KEK_LEN: usize = 32;
const LOOKUP_LABEL: &[u8] = b"arena.phone-lookup.v1";

pub trait Kek: Send + Sync {
    fn kek(&self) -> Result<[u8; KEK_LEN], AppError>;
}

fn internal(message: &str) -> AppError {
    AppError::new(ErrorCode::Internal, message)
}

fn gcm(key: &[u8]) -> Result<LessSafeKey, AppError> {
    UnboundKey::new(&AES_256_GCM, key)
        .map(LessSafeKey::new)
        .map_err(|_| internal("bad key length"))
}

fn encrypt(key: &[u8], aad: &str, plain: &[u8]) -> Result<String, AppError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| internal("the system random source failed"))?;
    let mut buf = plain.to_vec();
    gcm(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad.as_bytes()),
            &mut buf,
        )
        .map_err(|_| internal("encryption failed"))?;
    let mut out = nonce.to_vec();
    out.extend(buf);
    Ok(base64url(&out))
}

fn decrypt(key: &[u8], aad: &str, text: &str) -> Result<Vec<u8>, AppError> {
    let mut bytes = base64url_decode(text).ok_or_else(|| internal("bad ciphertext"))?;
    if bytes.len() < NONCE_LEN {
        return Err(internal("bad ciphertext"));
    }
    let mut body = bytes.split_off(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(&bytes).map_err(|_| internal("bad nonce"))?;
    let plain = gcm(key)?
        .open_in_place(nonce, Aad::from(aad.as_bytes()), &mut body)
        .map_err(|_| internal("decryption failed"))?;
    Ok(plain.to_vec())
}

pub fn phone_lookup(kek: &[u8; KEK_LEN], phone: &str) -> String {
    let derived = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, kek), LOOKUP_LABEL);
    hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref()),
        normalize_phone(phone).as_bytes(),
    )
    .as_ref()
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

fn dek_aad(player_id: &str) -> String {
    format!("{player_id}:dek")
}

fn field_aad(player_id: &str, field: &str) -> String {
    format!("{player_id}:{field}")
}

pub fn seal(
    kek: &[u8; KEK_LEN],
    form: &PlayerForm,
    player_id: &str,
    password_hash: String,
) -> Result<PlayerCreate, AppError> {
    let mut dek = [0u8; KEK_LEN];
    SystemRandom::new()
        .fill(&mut dek)
        .map_err(|_| internal("the system random source failed"))?;
    let field =
        |name: &str, value: &str| encrypt(&dek, &field_aad(player_id, name), value.as_bytes());
    let optional = |name: &str, value: &str| {
        let value = value.trim();
        (!value.is_empty()).then(|| field(name, value)).transpose()
    };
    let phone = normalize_phone(&form.phone);
    Ok(PlayerCreate {
        player_id: player_id.to_owned(),
        username: normalize_username(&form.username),
        password_hash,
        phone_lookup: phone_lookup(kek, &phone),
        name_enc: field("name", form.name.trim())?,
        phone_enc: field("phone", &phone)?,
        email_enc: optional("email", &form.email)?,
        dob_enc: optional("dob", &form.dob)?,
        notes_enc: optional("notes", &form.notes)?,
        tags: form
            .tags
            .iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect(),
        wrapped_key: encrypt(kek, &dek_aad(player_id), &dek)?,
        consent: form.consent,
        policy_version: form.policy_version.clone(),
    })
}

pub fn open_field(
    kek: &[u8; KEK_LEN],
    player_id: &str,
    wrapped_key: &str,
    field: &str,
    enc: &str,
) -> Result<String, AppError> {
    let dek = decrypt(kek, &dek_aad(player_id), wrapped_key)?;
    let plain = decrypt(&dek, &field_aad(player_id, field), enc)?;
    String::from_utf8(plain).map_err(|_| internal("bad plaintext"))
}

pub const SEARCH_LIMIT: usize = 8;

struct Entry {
    hit: PlayerHit,
    digits: String,
    folded: String,
}

#[derive(Default)]
pub struct PlayerIndex {
    last: i64,
    entries: Vec<Entry>,
}

impl PlayerIndex {
    fn sync(&mut self, reader: &SnapshotReader, kek: &[u8; KEK_LEN]) -> Result<(), AppError> {
        for (rowid, row, wrapped) in reader.player_rows_after(self.last)? {
            self.last = rowid;
            let open = |field, enc: &str| open_field(kek, &row.player_id, &wrapped, field, enc);
            let (Ok(name), Ok(phone)) =
                (open("name", &row.name_enc), open("phone", &row.phone_enc))
            else {
                tracing::warn!(player_id = %row.player_id, "player index: skipping an unreadable row");
                continue;
            };
            self.entries.push(Entry {
                digits: phone.chars().filter(char::is_ascii_digit).collect(),
                folded: name.to_lowercase(),
                hit: PlayerHit {
                    tags: serde_json::from_str(&row.tags).unwrap_or_default(),
                    player_id: row.player_id,
                    name,
                    username: row.username,
                    phone,
                },
            });
        }
        Ok(())
    }

    pub fn search(
        &mut self,
        reader: &SnapshotReader,
        kek: &[u8; KEK_LEN],
        query: &str,
    ) -> Result<Vec<PlayerHit>, AppError> {
        self.sync(reader, kek)?;
        let q: String = query.chars().filter(char::is_ascii_digit).collect();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut found: Vec<_> = self
            .entries
            .iter()
            .filter_map(|e| e.digits.find(&q).map(|at| (at, &e.folded, &e.hit)))
            .collect();
        found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        Ok(found
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(_, _, h)| h.clone())
            .collect())
    }

    pub fn profile(
        &mut self,
        reader: &SnapshotReader,
        kek: &[u8; KEK_LEN],
        player_id: &str,
    ) -> Result<PlayerProfile, AppError> {
        self.sync(reader, kek)?;
        let hit = self
            .entries
            .iter()
            .find(|e| e.hit.player_id == player_id)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no such player"))?
            .hit
            .clone();
        let consent = reader
            .latest_consent(player_id, CONSENT_TRANSACTIONAL)?
            .map(|(granted, policy_version)| PlayerConsent {
                granted,
                policy_version,
            });
        Ok(PlayerProfile {
            player: hit,
            consent,
        })
    }
}

pub fn utc_day(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.date_naive().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEK: [u8; KEK_LEN] = [7; KEK_LEN];

    fn form() -> PlayerForm {
        PlayerForm {
            username: "Asha".into(),
            password: "pw-plain-1".into(),
            phone: "+91 98765-43210".into(),
            name: "Asha Rao".into(),
            email: "asha@example.com".into(),
            dob: "2000-01-02".into(),
            notes: "likes FPS".into(),
            tags: vec![" vip ".into(), " ".into()],
            consent: true,
            policy_version: "1".into(),
        }
    }

    #[test]
    fn sealed_fields_open_with_the_key_and_hide_the_plaintext() {
        let p = seal(&KEK, &form(), "P1", "$argon2id$x".into()).unwrap();
        let json = serde_json::to_string(&p).unwrap();
        for plain in [
            "Asha Rao",
            "9876543210",
            "asha@example.com",
            "2000-01-02",
            "FPS",
            "pw-plain",
        ] {
            assert!(!json.contains(plain), "{plain}");
        }
        assert_eq!(
            (p.username.as_str(), p.tags.as_slice()),
            ("asha", ["vip".to_owned()].as_slice())
        );
        let open = |f: &str, e: &str| open_field(&KEK, "P1", &p.wrapped_key, f, e).unwrap();
        assert_eq!(open("name", &p.name_enc), "Asha Rao");
        assert_eq!(open("phone", &p.phone_enc), "+919876543210");
        assert_eq!(
            open("email", p.email_enc.as_deref().unwrap()),
            "asha@example.com"
        );
        assert_eq!(open("dob", p.dob_enc.as_deref().unwrap()), "2000-01-02");
        assert_eq!(open("notes", p.notes_enc.as_deref().unwrap()), "likes FPS");
    }

    #[test]
    fn ciphertext_is_bound_to_the_key_player_and_field() {
        let p = seal(&KEK, &form(), "P1", "h".into()).unwrap();
        assert!(open_field(&[8; KEK_LEN], "P1", &p.wrapped_key, "name", &p.name_enc).is_err());
        assert!(open_field(&KEK, "P2", &p.wrapped_key, "name", &p.name_enc).is_err());
        assert!(open_field(&KEK, "P1", &p.wrapped_key, "phone", &p.name_enc).is_err());
        let other = seal(&KEK, &form(), "P1", "h".into()).unwrap();
        assert_ne!(p.name_enc, other.name_enc);
    }

    #[test]
    fn optional_fields_stay_absent_when_empty() {
        let mut f = form();
        (f.email, f.dob, f.notes) = (String::new(), " ".into(), String::new());
        let p = seal(&KEK, &f, "P1", "h".into()).unwrap();
        assert_eq!((p.email_enc, p.dob_enc, p.notes_enc), (None, None, None));
    }

    #[test]
    fn the_phone_lookup_ignores_formatting_and_depends_on_the_key() {
        let a = phone_lookup(&KEK, "+91 98765-43210");
        assert_eq!(a, phone_lookup(&KEK, "+919876543210"));
        assert_ne!(a, phone_lookup(&[8; KEK_LEN], "+919876543210"));
        assert_eq!(a.len(), 64);
    }

    fn seeded(n: usize) -> (tempfile::TempDir, SnapshotReader) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch.db");
        let mut store = arena_store::Store::open(&path, 0).unwrap();
        store
            .write_tx(|tx| {
                for i in 0..n {
                    let id = format!("P{i:05}");
                    let mut f = form();
                    f.name = format!("Player {}", n - i);
                    f.phone = format!("+91 9{:09}", i * 7919 % 1_000_000_000);
                    let p = seal(&KEK, &f, &id, "h".into()).unwrap();
                    arena_store::insert_player(
                        tx,
                        &arena_store::PlayerRow {
                            player_id: id.clone(),
                            username: format!("user{i}"),
                            password_hash: p.password_hash,
                            phone_lookup: p.phone_lookup,
                            name_enc: p.name_enc,
                            phone_enc: p.phone_enc,
                            email_enc: None,
                            dob_enc: None,
                            notes_enc: None,
                            tags: "[\"vip\"]".into(),
                        },
                    )?;
                    arena_store::insert_player_key(tx, &id, &p.wrapped_key)?;
                }
                Ok(())
            })
            .unwrap();
        let reader = SnapshotReader::open(&path).unwrap();
        (dir, reader)
    }

    #[test]
    fn search_matches_digits_best_first_and_syncs_new_rows() {
        let (_dir, reader) = seeded(30);
        let mut idx = PlayerIndex::default();
        assert!(idx.search(&reader, &KEK, "abc").unwrap().is_empty());
        let all = idx.search(&reader, &KEK, "+91 9").unwrap();
        assert_eq!(all.len(), SEARCH_LIMIT);
        assert!(all.iter().all(|h| h.phone.starts_with("+919")));
        let one = idx.search(&reader, &KEK, "91-9").unwrap();
        assert_eq!(one[0].tags, ["vip"]);
        let p = idx.profile(&reader, &KEK, "P00003").unwrap();
        assert_eq!((p.player.username.as_str(), p.consent), ("user3", None));
        assert_eq!(
            idx.profile(&reader, &KEK, "nope").unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn three_thousand_players_answer_in_under_100_ms() {
        let (_dir, reader) = seeded(3000);
        let mut idx = PlayerIndex::default();
        idx.search(&reader, &KEK, "9").unwrap();
        let start = std::time::Instant::now();
        let hits = idx.search(&reader, &KEK, "9876").unwrap();
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
        assert!(hits.len() <= SEARCH_LIMIT);
        assert_eq!(idx.entries.len(), 3000);
    }

    #[test]
    fn days_are_utc_dates() {
        assert_eq!(utc_day(1_800_000_000_000), "2027-01-15");
    }
}

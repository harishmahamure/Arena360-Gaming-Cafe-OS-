use std::collections::{BTreeMap, HashMap};

use arena_proto::{
    AppError, CONSENT_TRANSACTIONAL, ErrorCode, ImportPlayerRow, PlayerConsent, PlayerCreate,
    PlayerEditForm, PlayerForm, PlayerHit, PlayerProfile, PlayerUpdate, normalize_phone,
    normalize_username,
};
use arena_store::PlayerRow;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use crate::SnapshotReader;
use crate::audit::base64url_decode;
use crate::staff::base64url;

pub const KEK_LEN: usize = 32;
const LOOKUP_LABEL: &[u8] = b"arena.phone-lookup.v1";
/// The policy version recorded for imported players, whose consent is not granted.
pub const IMPORT_POLICY_VERSION: &str = "import";

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

struct Plain<'a> {
    phone: &'a str,
    name: &'a str,
    email: &'a str,
    dob: &'a str,
    notes: &'a str,
    tags: &'a [String],
    consent: bool,
    policy_version: &'a str,
}

fn seal_details(
    kek: &[u8; KEK_LEN],
    dek: &[u8],
    player_id: &str,
    d: &Plain,
) -> Result<PlayerUpdate, AppError> {
    let field =
        |name: &str, value: &str| encrypt(dek, &field_aad(player_id, name), value.as_bytes());
    let optional = |name: &str, value: &str| {
        let value = value.trim();
        (!value.is_empty()).then(|| field(name, value)).transpose()
    };
    let phone = normalize_phone(d.phone);
    Ok(PlayerUpdate {
        player_id: player_id.to_owned(),
        phone_lookup: phone_lookup(kek, &phone),
        name_enc: field("name", d.name.trim())?,
        phone_enc: field("phone", &phone)?,
        email_enc: optional("email", d.email)?,
        dob_enc: optional("dob", d.dob)?,
        notes_enc: optional("notes", d.notes)?,
        tags: d
            .tags
            .iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect(),
        consent: d.consent,
        policy_version: d.policy_version.to_owned(),
    })
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
    let u = seal_details(
        kek,
        &dek,
        player_id,
        &Plain {
            phone: &form.phone,
            name: &form.name,
            email: &form.email,
            dob: &form.dob,
            notes: &form.notes,
            tags: &form.tags,
            consent: form.consent,
            policy_version: &form.policy_version,
        },
    )?;
    Ok(PlayerCreate {
        player_id: u.player_id,
        username: normalize_username(&form.username),
        password_hash,
        phone_lookup: u.phone_lookup,
        name_enc: u.name_enc,
        phone_enc: u.phone_enc,
        email_enc: u.email_enc,
        dob_enc: u.dob_enc,
        notes_enc: u.notes_enc,
        tags: u.tags,
        wrapped_key: encrypt(kek, &dek_aad(player_id), &dek)?,
        consent: u.consent,
        policy_version: u.policy_version,
    })
}

/// Seals one imported player (Story 5.34): name and phone as ciphertext under a fresh key
/// wrapped by `kek`, exactly as [`seal`] does for a profile made at the counter, but with no
/// password to hash and no optional fields.
pub fn seal_import(
    kek: &[u8; KEK_LEN],
    player_id: &str,
    username: &str,
    phone: &str,
    name: &str,
    balance_minor: i64,
    credit_limit_minor: i64,
) -> Result<ImportPlayerRow, AppError> {
    let mut dek = [0u8; KEK_LEN];
    SystemRandom::new()
        .fill(&mut dek)
        .map_err(|_| internal("the system random source failed"))?;
    let phone = normalize_phone(phone);
    Ok(ImportPlayerRow {
        player_id: player_id.to_owned(),
        username: username.to_owned(),
        phone_lookup: phone_lookup(kek, &phone),
        name_enc: encrypt(&dek, &field_aad(player_id, "name"), name.trim().as_bytes())?,
        phone_enc: encrypt(&dek, &field_aad(player_id, "phone"), phone.as_bytes())?,
        wrapped_key: encrypt(kek, &dek_aad(player_id), &dek)?,
        policy_version: IMPORT_POLICY_VERSION.to_owned(),
        balance_minor,
        credit_limit_minor,
    })
}

pub fn reseal(
    kek: &[u8; KEK_LEN],
    form: &PlayerEditForm,
    wrapped_key: &str,
) -> Result<PlayerUpdate, AppError> {
    let dek = decrypt(kek, &dek_aad(&form.player_id), wrapped_key)?;
    seal_details(
        kek,
        &dek,
        &form.player_id,
        &Plain {
            phone: &form.phone,
            name: &form.name,
            email: &form.email,
            dob: &form.dob,
            notes: &form.notes,
            tags: &form.tags,
            consent: form.consent,
            policy_version: &form.policy_version,
        },
    )
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

/// Whether `password` is the one `hash` was made from (Story 6.8). Only an argon2id PHC string
/// verifies: an imported player's placeholder (`!`) or any other value never does. When there is
/// nothing to verify against, a dummy check keeps the time alike. Blocking (argon2id).
pub fn check_password(password: &str, hash: Option<&str>) -> bool {
    match hash {
        Some(h) if h.starts_with("$argon2id$") => crate::staff::verify_secret(password, h),
        _ => {
            crate::staff::burn_check(password);
            false
        }
    }
}

/// The player's name as they gave it (opened with `kek`).
pub fn open_name(
    kek: &[u8; KEK_LEN],
    player_id: &str,
    wrapped_key: &str,
    name_enc: &str,
) -> Result<String, AppError> {
    open_field(kek, player_id, wrapped_key, "name", name_enc)
}

pub const SEARCH_LIMIT: usize = 8;

struct Entry {
    hit: PlayerHit,
    digits: String,
    folded: String,
}

#[derive(Default)]
pub struct PlayerIndex {
    seen: HashMap<String, i64>,
    entries: BTreeMap<String, Entry>,
}

fn hit(kek: &[u8; KEK_LEN], row: PlayerRow, wrapped: &str) -> Result<PlayerHit, AppError> {
    let open = |field, enc: &str| open_field(kek, &row.player_id, wrapped, field, enc);
    Ok(PlayerHit {
        name: open("name", &row.name_enc)?,
        phone: open("phone", &row.phone_enc)?,
        tags: serde_json::from_str(&row.tags).unwrap_or_default(),
        player_id: row.player_id,
        username: row.username,
        outstanding_minor: 0,
    })
}

impl PlayerIndex {
    fn sync(&mut self, reader: &SnapshotReader, kek: &[u8; KEK_LEN]) -> Result<(), AppError> {
        for (id, rev) in reader.player_revs()? {
            if self.seen.get(&id) == Some(&rev) {
                continue;
            }
            let Some((row, rev, wrapped)) = reader.player(&id)? else {
                continue;
            };
            self.seen.insert(id.clone(), rev);
            self.entries.remove(&id);
            let Ok(hit) = hit(kek, row, &wrapped) else {
                tracing::warn!(player_id = %id, "player index: skipping an unreadable row");
                continue;
            };
            self.entries.insert(
                id,
                Entry {
                    digits: hit.phone.chars().filter(char::is_ascii_digit).collect(),
                    folded: hit.name.to_lowercase(),
                    hit,
                },
            );
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
            .values()
            .filter_map(|e| e.digits.find(&q).map(|at| (at, &e.folded, &e.hit)))
            .collect();
        found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let mut hits: Vec<PlayerHit> = found
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(_, _, h)| h.clone())
            .collect();
        for hit in &mut hits {
            hit.outstanding_minor = reader.outstanding(&hit.player_id)?;
        }
        Ok(hits)
    }
}

pub fn profile(
    reader: &SnapshotReader,
    kek: &[u8; KEK_LEN],
    player_id: &str,
) -> Result<PlayerProfile, AppError> {
    let (row, rev, wrapped) = reader
        .player(player_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no such player"))?;
    let optional = |field, enc: &Option<String>| {
        enc.as_deref().map_or(Ok(String::new()), |e| {
            open_field(kek, player_id, &wrapped, field, e)
        })
    };
    let (email, dob, notes) = (
        optional("email", &row.email_enc)?,
        optional("dob", &row.dob_enc)?,
        optional("notes", &row.notes_enc)?,
    );
    let consent = reader
        .latest_consent(player_id, CONSENT_TRANSACTIONAL)?
        .map(|(granted, policy_version)| PlayerConsent {
            granted,
            policy_version,
        });
    Ok(PlayerProfile {
        player: hit(kek, row, &wrapped)?,
        consent,
        rev,
        wallet_minor: reader.wallet_balance(player_id)?,
        email,
        dob,
        notes,
    })
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
    fn only_an_argon2id_hash_of_the_password_verifies() {
        let hash = crate::staff::hash_secret("pw-plain-1").unwrap();
        assert!(check_password("pw-plain-1", Some(&hash)));
        assert!(!check_password("pw-plain-2", Some(&hash)));
        assert!(!check_password("", Some(&hash)));
        // An imported player has the placeholder; a stub, an empty hash and none never verify.
        for bad in [Some("!"), Some(""), Some("plain"), Some("$argon2i$x"), None] {
            assert!(!check_password("!", bad), "{bad:?}");
            assert!(!check_password("pw-plain-1", bad), "{bad:?}");
        }
    }

    #[test]
    fn the_name_opens_with_its_player_and_key() {
        let p = seal(&KEK, &form(), "P1", "h".into()).unwrap();
        assert_eq!(
            open_name(&KEK, "P1", &p.wrapped_key, &p.name_enc).unwrap(),
            "Asha Rao"
        );
        assert!(open_name(&KEK, "P2", &p.wrapped_key, &p.name_enc).is_err());
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
        let p = profile(&reader, &KEK, "P00003").unwrap();
        assert_eq!((p.player.username.as_str(), p.consent), ("user3", None));
        assert_eq!((p.rev, p.email.as_str()), (0, ""));
        assert_eq!(
            profile(&reader, &KEK, "nope").unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_edit_replaces_the_entry_and_the_old_phone_stops_matching() {
        let (dir, reader) = seeded(3);
        let mut idx = PlayerIndex::default();
        assert_eq!(idx.search(&reader, &KEK, "9000007919").unwrap().len(), 1);
        let (_, _, wrapped) = reader.player("P00001").unwrap().unwrap();
        let edit = PlayerEditForm {
            player_id: "P00001".into(),
            base_rev: 0,
            phone: "+91 91111-11111".into(),
            name: "Zed".into(),
            email: "z@example.com".into(),
            dob: String::new(),
            notes: String::new(),
            tags: vec!["new".into()],
            consent: true,
            policy_version: "1".into(),
        };
        let u = reseal(&KEK, &edit, &wrapped).unwrap();
        assert!(!serde_json::to_string(&u).unwrap().contains("Zed"));
        let mut store = arena_store::Store::open(dir.path().join("branch.db"), 0).unwrap();
        store
            .write_tx(|tx| {
                arena_store::update_player(
                    tx,
                    &PlayerRow {
                        player_id: u.player_id.clone(),
                        username: String::new(),
                        password_hash: String::new(),
                        phone_lookup: u.phone_lookup,
                        name_enc: u.name_enc,
                        phone_enc: u.phone_enc,
                        email_enc: u.email_enc,
                        dob_enc: None,
                        notes_enc: None,
                        tags: "[\"new\"]".into(),
                    },
                )?;
                tx.execute("UPDATE players SET rev = rev + 1", [])
                    .map_err(arena_store::db_error)?;
                Ok(())
            })
            .unwrap();
        assert!(idx.search(&reader, &KEK, "9000007919").unwrap().is_empty());
        let found = idx.search(&reader, &KEK, "91111-11111").unwrap();
        assert_eq!((found.len(), found[0].name.as_str()), (1, "Zed"));
        assert_eq!(found[0].tags, ["new"]);
        let p = profile(&reader, &KEK, "P00001").unwrap();
        assert_eq!((p.rev, p.email.as_str()), (1, "z@example.com"));
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

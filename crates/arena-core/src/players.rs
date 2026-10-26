use arena_proto::{
    AppError, ErrorCode, PlayerCreate, PlayerForm, normalize_phone, normalize_username,
};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

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

    #[test]
    fn days_are_utc_dates() {
        assert_eq!(utc_day(1_800_000_000_000), "2027-01-15");
    }
}

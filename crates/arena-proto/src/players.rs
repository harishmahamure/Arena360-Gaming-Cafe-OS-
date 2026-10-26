use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, ErrorCode};
use crate::stock::valid_date;

pub const PLAYER_PASSWORD_MIN_CHARS: usize = 6;
pub const PLAYER_PASSWORD_MAX_CHARS: usize = 128;
pub const PLAYER_USERNAME_MIN_CHARS: usize = 3;
pub const PLAYER_USERNAME_MAX_CHARS: usize = 32;
pub const PLAYER_NAME_MAX_CHARS: usize = 80;
pub const PLAYER_EMAIL_MAX_CHARS: usize = 254;
pub const PLAYER_NOTES_MAX_CHARS: usize = 500;
pub const PLAYER_TAGS_MAX: usize = 10;
pub const PLAYER_TAG_MAX_CHARS: usize = 24;
pub const PLAYER_PHONE_MIN_DIGITS: usize = 8;
pub const PLAYER_PHONE_MAX_DIGITS: usize = 15;
pub const POLICY_VERSION_MAX_CHARS: usize = 16;
pub const CONSENT_TRANSACTIONAL: &str = "transactional";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerForm {
    pub username: String,
    pub password: String,
    pub phone: String,
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub dob: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub consent: bool,
    pub policy_version: String,
}

impl std::fmt::Debug for PlayerForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerForm").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerCreate {
    pub player_id: String,
    pub username: String,
    pub password_hash: String,
    pub phone_lookup: String,
    pub name_enc: String,
    pub phone_enc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub email_enc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dob_enc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notes_enc: Option<String>,
    pub tags: Vec<String>,
    pub wrapped_key: String,
    pub consent: bool,
    pub policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiPlayerRequest {
    pub form: PlayerForm,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

fn invalid(field: &str, message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, field, message)
}

pub fn normalize_username(s: &str) -> String {
    s.trim().to_lowercase()
}

pub fn normalize_phone(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, ' ' | '-')).collect()
}

pub fn valid_username(s: &str) -> bool {
    (PLAYER_USERNAME_MIN_CHARS..=PLAYER_USERNAME_MAX_CHARS).contains(&s.len())
        && s.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

pub fn valid_phone(s: &str) -> bool {
    let digits = s.strip_prefix('+').unwrap_or("");
    (PLAYER_PHONE_MIN_DIGITS..=PLAYER_PHONE_MAX_DIGITS).contains(&digits.len())
        && digits.bytes().all(|c| c.is_ascii_digit())
}

pub fn valid_tags(tags: &[String]) -> bool {
    tags.len() <= PLAYER_TAGS_MAX
        && tags.iter().all(|t| {
            let n = t.chars().count();
            (1..=PLAYER_TAG_MAX_CHARS).contains(&n) && !t.chars().any(char::is_control)
        })
}

pub fn valid_policy_version(s: &str) -> bool {
    (1..=POLICY_VERSION_MAX_CHARS).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
}

fn clean(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(char::is_control)
}

impl PlayerForm {
    pub fn check(&self, today: &str) -> Result<(), AppError> {
        if !valid_username(&normalize_username(&self.username)) {
            return Err(invalid(
                "username",
                "username must be 3-32 characters: letters, digits, dot, dash or underscore",
            ));
        }
        if !(PLAYER_PASSWORD_MIN_CHARS..=PLAYER_PASSWORD_MAX_CHARS)
            .contains(&self.password.chars().count())
        {
            return Err(invalid("password", "password must be 6 to 128 characters"));
        }
        if !valid_phone(&normalize_phone(&self.phone)) {
            return Err(invalid(
                "phone",
                "phone must start with + and have 8 to 15 digits",
            ));
        }
        let name = self.name.trim();
        if name.is_empty() || !clean(name, PLAYER_NAME_MAX_CHARS) {
            return Err(invalid("name", "name must be 1-80 characters"));
        }
        let email = self.email.trim();
        if !email.is_empty()
            && (email.matches('@').count() != 1 || !clean(email, PLAYER_EMAIL_MAX_CHARS))
        {
            return Err(invalid("email", "email must contain one @"));
        }
        let dob = self.dob.trim();
        if !dob.is_empty() && (!valid_date(dob) || dob >= today) {
            return Err(invalid("dob", "date of birth must be a past date"));
        }
        if self.notes.chars().count() > PLAYER_NOTES_MAX_CHARS {
            return Err(invalid("notes", "notes must be up to 500 characters"));
        }
        if !valid_tags(&self.tags) {
            return Err(invalid("tags", "up to 10 tags of up to 24 characters each"));
        }
        if !valid_policy_version(&self.policy_version) {
            return Err(invalid("policy_version", "policy version is not valid"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> PlayerForm {
        PlayerForm {
            username: "Asha_9".into(),
            password: "secret".into(),
            phone: "+91 98765-43210".into(),
            name: "Asha".into(),
            email: String::new(),
            dob: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            consent: true,
            policy_version: "1".into(),
        }
    }

    fn field(f: &PlayerForm) -> Option<String> {
        f.check("2026-10-27").err().and_then(|e| e.field)
    }

    #[test]
    fn a_minimal_form_passes() {
        assert_eq!(field(&form()), None);
    }

    #[test]
    fn each_rule_names_its_field() {
        type Edit = fn(&mut PlayerForm);
        let cases: [(&str, Edit); 11] = [
            ("username", |f| f.username = "ab".into()),
            ("username", |f| f.username = "a b c".into()),
            ("password", |f| f.password = "12345".into()),
            ("password", |f| f.password = "x".repeat(129)),
            ("phone", |f| f.phone = "+91 12".into()),
            ("phone", |f| f.phone = "9876543210".into()),
            ("name", |f| f.name = "  ".into()),
            ("email", |f| f.email = "a@b@c".into()),
            ("dob", |f| f.dob = "2026-10-27".into()),
            ("notes", |f| f.notes = "x".repeat(501)),
            ("tags", |f| f.tags = vec!["t".into(); 11]),
        ];
        for (want, edit) in cases {
            let mut f = form();
            edit(&mut f);
            assert_eq!(field(&f).as_deref(), Some(want), "{want}");
        }
    }

    #[test]
    fn optional_fields_accept_good_values() {
        let mut f = form();
        f.email = "a@b.c".into();
        f.dob = "2000-02-29".into();
        f.tags = vec!["vip".into()];
        assert_eq!(field(&f), None);
        assert_eq!(normalize_phone(&f.phone), "+919876543210");
        assert_eq!(normalize_username(" Asha_9 "), "asha_9");
    }

    #[test]
    fn debug_hides_the_values() {
        assert!(!format!("{:?}", form()).contains("secret"));
    }
}

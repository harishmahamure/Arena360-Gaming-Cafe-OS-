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
/// The longest reason a manager gives for disposing of a wallet balance on an erasure (Story 9.1).
pub const ERASE_NOTE_MAX_CHARS: usize = 120;

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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerEditForm {
    pub player_id: String,
    #[ts(type = "number")]
    pub base_rev: i64,
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

impl std::fmt::Debug for PlayerEditForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerEditForm").finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPasswordForm {
    pub player_id: String,
    pub password: String,
    pub confirm: String,
}

impl std::fmt::Debug for PlayerPasswordForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerPasswordForm").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerUpdate {
    pub player_id: String,
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
    pub consent: bool,
    pub policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPasswordReset {
    pub player_id: String,
    pub password_hash: String,
}

/// A player's own new password hash (Story 6.34): the Hub built it from a password it had just
/// checked against the stored one, so the kiosk never sends a hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerPasswordChange {
    pub player_id: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiPlayerEditRequest {
    pub form: PlayerEditForm,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiPasswordResetRequest {
    pub form: PlayerPasswordForm,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
}

/// What happened to a wallet balance an erasure found (Story 9.1): paid out at the counter, or
/// given up by the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum WalletDisposal {
    Refunded,
    Forfeited,
}

impl WalletDisposal {
    /// The `wallet_txns.reason` of the row that zeroes the wallet.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Refunded => "erase_refund",
            Self::Forfeited => "erase_forfeit",
        }
    }
}

/// A player's personal data erased by crypto-shredding (Story 9.1). It holds the player id, how a
/// wallet balance was disposed of and the manager's reason, never any personal data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerErase {
    pub player_id: String,
    /// Required exactly when the wallet balance is above zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wallet: Option<WalletDisposal>,
    /// The manager's reason for the disposal (1 to 120 characters); absent with no `wallet`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct UiPlayerEraseRequest {
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wallet: Option<WalletDisposal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elevation_ticket: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerHit {
    pub player_id: String,
    pub name: String,
    pub username: String,
    pub phone: String,
    pub tags: Vec<String>,
    #[serde(default)]
    #[ts(type = "number")]
    pub outstanding_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerConsent {
    pub granted: bool,
    pub policy_version: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PlayerProfile {
    pub player: PlayerHit,
    pub consent: Option<PlayerConsent>,
    #[ts(type = "number")]
    pub rev: i64,
    #[serde(default)]
    #[ts(type = "number")]
    pub wallet_minor: i64,
    pub email: String,
    pub dob: String,
    pub notes: String,
}

impl std::fmt::Debug for PlayerProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerProfile").finish_non_exhaustive()
    }
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

fn check_password(password: &str) -> Result<(), AppError> {
    if !(PLAYER_PASSWORD_MIN_CHARS..=PLAYER_PASSWORD_MAX_CHARS).contains(&password.chars().count())
    {
        return Err(invalid("password", "password must be 6 to 128 characters"));
    }
    Ok(())
}

struct Details<'a> {
    phone: &'a str,
    name: &'a str,
    email: &'a str,
    dob: &'a str,
    notes: &'a str,
    tags: &'a [String],
    policy_version: &'a str,
}

impl Details<'_> {
    fn check(&self, today: &str) -> Result<(), AppError> {
        if !valid_phone(&normalize_phone(self.phone)) {
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
        if !valid_tags(self.tags) {
            return Err(invalid("tags", "up to 10 tags of up to 24 characters each"));
        }
        if !valid_policy_version(self.policy_version) {
            return Err(invalid("policy_version", "policy version is not valid"));
        }
        Ok(())
    }
}

impl PlayerForm {
    pub fn check(&self, today: &str) -> Result<(), AppError> {
        if !valid_username(&normalize_username(&self.username)) {
            return Err(invalid(
                "username",
                "username must be 3-32 characters: letters, digits, dot, dash or underscore",
            ));
        }
        check_password(&self.password)?;
        Details {
            phone: &self.phone,
            name: &self.name,
            email: &self.email,
            dob: &self.dob,
            notes: &self.notes,
            tags: &self.tags,
            policy_version: &self.policy_version,
        }
        .check(today)
    }
}

impl PlayerEditForm {
    pub fn check(&self, today: &str) -> Result<(), AppError> {
        Details {
            phone: &self.phone,
            name: &self.name,
            email: &self.email,
            dob: &self.dob,
            notes: &self.notes,
            tags: &self.tags,
            policy_version: &self.policy_version,
        }
        .check(today)
    }
}

impl PlayerPasswordForm {
    pub fn check(&self) -> Result<(), AppError> {
        check_password(&self.password)?;
        if self.password != self.confirm {
            return Err(invalid("confirm", "the two passwords differ"));
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
    fn an_edit_form_and_a_password_form_follow_the_same_rules() {
        let edit = |phone: &str, name: &str| PlayerEditForm {
            player_id: "P".into(),
            base_rev: 1,
            phone: phone.into(),
            name: name.into(),
            email: String::new(),
            dob: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            consent: false,
            policy_version: "1".into(),
        };
        let field = |f: &PlayerEditForm| f.check("2026-10-27").err().and_then(|e| e.field);
        assert_eq!(field(&edit("+91 98765-43210", "Asha")), None);
        assert_eq!(field(&edit("12", "Asha")).as_deref(), Some("phone"));
        assert_eq!(field(&edit("+919876543210", " ")).as_deref(), Some("name"));
        let pw = |a: &str, b: &str| {
            PlayerPasswordForm {
                player_id: "P".into(),
                password: a.into(),
                confirm: b.into(),
            }
            .check()
            .err()
            .and_then(|e| e.field)
        };
        assert_eq!(pw("secret1", "secret1"), None);
        assert_eq!(pw("secret1", "secret2").as_deref(), Some("confirm"));
        assert_eq!(pw("12345", "12345").as_deref(), Some("password"));
    }

    #[test]
    fn debug_hides_the_values() {
        assert!(!format!("{:?}", form()).contains("secret"));
    }
}

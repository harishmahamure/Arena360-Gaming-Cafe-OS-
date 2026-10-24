//! Owner sign-up with a phone one-time code and the owner's browser session, on the cloud API
//! (`/v1/signup/*`, `/v1/session*`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How long a sign-up code works, in seconds.
pub const OTP_TTL_S: u32 = 300;
/// How long after a send the same challenge may send again, in seconds.
pub const OTP_RESEND_AFTER_S: u32 = 30;
/// Wrong codes a challenge takes; the last one voids it.
pub const OTP_MAX_ATTEMPTS: u32 = 5;
/// Digits in a sign-up code.
pub const OTP_DIGITS: usize = 6;
/// Code sends allowed per phone per rolling hour.
pub const OTP_SENDS_PER_HOUR: u32 = 5;

/// `POST /v1/signup/start`: the "Get started" form. `phone` is E.164 (`+919876543210`).
/// `Debug` never prints the values.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignupStart {
    pub name: String,
    pub phone: String,
    pub email: String,
    pub cafe_name: String,
    pub city: String,
}

impl std::fmt::Debug for SignupStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupStart").finish_non_exhaustive()
    }
}

/// `202` from `POST /v1/signup/start` and `POST /v1/signup/resend`: a code was sent. The same
/// whether or not the phone already has an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignupChallenge {
    pub challenge_id: String,
    /// Seconds the code works ([`OTP_TTL_S`]).
    pub expires_in_s: u32,
    /// Seconds until "Resend code" may be used ([`OTP_RESEND_AFTER_S`]).
    pub resend_after_s: u32,
}

/// `POST /v1/signup/resend`: a new code on the same challenge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignupResend {
    pub challenge_id: String,
}

/// `POST /v1/signup/verify`. `Debug` never prints the code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignupVerify {
    pub challenge_id: String,
    pub code: String,
}

impl std::fmt::Debug for SignupVerify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignupVerify")
            .field("challenge_id", &self.challenge_id)
            .field("code", &"<redacted>")
            .finish()
    }
}

/// Where a verified sign-up goes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SignupNext {
    /// Signed in (session cookies set): continue with account setup.
    AccountSetup,
    /// The phone belongs to an owner who already enrolled a second factor: no session; sign
    /// in instead.
    SignIn,
}

/// `200` from `POST /v1/signup/verify`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SignupVerified {
    pub next: SignupNext,
}

/// An owner's second-factor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum MfaState {
    /// Signed up; no password or TOTP yet.
    Pending,
    /// Password and TOTP enrolled.
    Enrolled,
}

impl MfaState {
    /// The stored form (`pending`, `enrolled`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Enrolled => "enrolled",
        }
    }

    /// Parses the stored form.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "enrolled" => Some(Self::Enrolled),
            _ => None,
        }
    }
}

/// The signed-in owner, as `GET /v1/session` shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionOwner {
    pub name: String,
    pub mfa_state: MfaState,
    /// Whether the owner has set a password.
    pub has_password: bool,
    /// Whether the owner confirmed saving their backup codes.
    pub backup_codes_acked: bool,
    pub sms_fallback: bool,
}

/// `200` from `GET /v1/session` and `POST /v1/session/refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SessionInfo {
    pub owner: SessionOwner,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_mfa_state_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&SignupVerified {
                next: SignupNext::AccountSetup
            })
            .unwrap(),
            r#"{"next":"account_setup"}"#
        );
        assert_eq!(
            serde_json::to_string(&SignupNext::SignIn).unwrap(),
            r#""sign_in""#
        );
        for s in [MfaState::Pending, MfaState::Enrolled] {
            assert_eq!(MfaState::parse(s.as_str()), Some(s));
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!("\"{}\"", s.as_str())
            );
        }
    }

    #[test]
    fn debug_hides_personal_data_and_the_code() {
        let start = SignupStart {
            name: "Asha".into(),
            phone: "+919876543210".into(),
            email: "a@b.in".into(),
            cafe_name: "Cafe".into(),
            city: "Pune".into(),
        };
        let d = format!("{start:?}");
        assert!(!d.contains("Asha") && !d.contains("9876543210") && !d.contains("a@b.in"));
        let v = SignupVerify {
            challenge_id: "c".into(),
            code: "123456".into(),
        };
        assert!(!format!("{v:?}").contains("123456"));
    }
}

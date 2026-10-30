use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SIGNIN_MAX_FAILURES: u32 = 5;
pub const SIGNIN_LOCK_S: u32 = 15 * 60;
pub const SIGNIN_CHALLENGE_TTL_S: u32 = 300;
pub const SIGNIN_MAX_ATTEMPTS: u32 = 5;
pub const TRUSTED_DEVICE_TTL_S: u32 = 30 * 24 * 60 * 60;
pub const DEVICE_LABEL_MAX_CHARS: usize = 60;
pub const BACKUP_CODES_LOW: u32 = 3;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct Signin {
    pub login: String,
    pub password: String,
}

impl std::fmt::Debug for Signin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signin").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SigninMethod {
    Totp,
    BackupCode,
    Sms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum SigninNext {
    AccountSetup,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SigninReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next: Option<SigninNext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub challenge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub methods: Option<Vec<SigninMethod>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SigninSms {
    pub challenge_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SigninSmsSent {
    pub resend_after_s: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SigninVerify {
    pub challenge_id: String,
    pub method: SigninMethod,
    pub code: String,
    #[serde(default)]
    pub trust_device: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device_label: Option<String>,
}

impl std::fmt::Debug for SigninVerify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigninVerify")
            .field("challenge_id", &self.challenge_id)
            .field("method", &self.method)
            .field("trust_device", &self.trust_device)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SigninVerified {
    pub next: SigninNext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub backup_codes_remaining: Option<u32>,
}

/// How long a step-up proof lets a sensitive owner command through (Story 8.25).
pub const STEPUP_TTL_MS: i64 = 5 * 60 * 1000;

/// `POST /v1/session/stepup`: a second factor for the signed-in session. The answer is 204 and the
/// session holds a proof for [`STEPUP_TTL_MS`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StepupRequest {
    pub method: SigninMethod,
    pub code: String,
}

impl std::fmt::Debug for StepupRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepupRequest")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

/// `POST /v1/session/stepup/sms`: send a text-message code for a step-up. No fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StepupSms {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct StepupSmsSent {
    pub resend_after_s: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SmsFallback {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TrustedDevice {
    pub id: String,
    pub label: String,
    #[ts(type = "number")]
    pub last_seen_at: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    pub current: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names() {
        let challenge = SigninReply {
            next: None,
            challenge_id: Some("c".into()),
            methods: Some(vec![
                SigninMethod::Totp,
                SigninMethod::BackupCode,
                SigninMethod::Sms,
            ]),
        };
        assert_eq!(
            serde_json::to_string(&challenge).unwrap(),
            r#"{"challenge_id":"c","methods":["totp","backup_code","sms"]}"#
        );
        let done = SigninReply {
            next: Some(SigninNext::AccountSetup),
            challenge_id: None,
            methods: None,
        };
        assert_eq!(
            serde_json::to_string(&done).unwrap(),
            r#"{"next":"account_setup"}"#
        );
        let v: SigninVerify =
            serde_json::from_str(r#"{"challenge_id":"c","method":"totp","code":"123456"}"#)
                .unwrap();
        assert!(!v.trust_device && v.device_label.is_none());
    }

    #[test]
    fn debug_hides_the_login_password_and_code() {
        let s = format!(
            "{:?}",
            Signin {
                login: "+919876543210".into(),
                password: "hunter2-hunter2".into(),
            }
        );
        assert!(!s.contains("9876543210") && !s.contains("hunter2"));
        let v = format!(
            "{:?}",
            SigninVerify {
                challenge_id: "c".into(),
                method: SigninMethod::BackupCode,
                code: "ABCDE-FGHJK".into(),
                trust_device: true,
                device_label: Some("Asha's phone".into()),
            }
        );
        assert!(!v.contains("ABCDE") && !v.contains("Asha"));
    }
}

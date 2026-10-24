//! The owner's account setup on the cloud API: a password, an authenticator app (TOTP) and
//! backup codes (`/v1/account/*`). `Debug` never prints a password, secret or code.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The shortest password, in Unicode scalar values.
pub const PASSWORD_MIN_CHARS: usize = 10;
/// The longest password, in Unicode scalar values.
pub const PASSWORD_MAX_CHARS: usize = 128;
/// Digits in an authenticator (TOTP) code.
pub const TOTP_DIGITS: usize = 6;
/// The TOTP time step, in seconds.
pub const TOTP_PERIOD_S: u64 = 30;
/// Backup codes issued at once.
pub const BACKUP_CODE_COUNT: usize = 10;
/// Characters in a backup code (shown as `XXXXX-XXXXX`).
pub const BACKUP_CODE_CHARS: usize = 10;

/// `POST /v1/account/password`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SetPassword {
    pub password: String,
}

impl std::fmt::Debug for SetPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetPassword").finish_non_exhaustive()
    }
}

/// `200` from `POST /v1/account/mfa/totp/start`: a new, unconfirmed secret.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TotpStarted {
    /// RFC 4648 base32 without padding.
    pub secret: String,
    /// `otpauth://totp/Arena%20360:<phone>?secret=…&issuer=Arena%20360&…`.
    pub otpauth_uri: String,
    /// The URI as a QR code: an SVG document.
    pub qr_svg: String,
}

impl std::fmt::Debug for TotpStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotpStarted").finish_non_exhaustive()
    }
}

/// `POST /v1/account/mfa/totp/confirm` and `POST /v1/account/mfa/backup-codes/regenerate`: a
/// current authenticator code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct TotpCode {
    pub code: String,
}

impl std::fmt::Debug for TotpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotpCode").finish_non_exhaustive()
    }
}

/// `200` from confirm and regenerate: the new backup codes (`XXXXX-XXXXX`), shown this once.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct BackupCodes {
    pub backup_codes: Vec<String>,
}

impl std::fmt::Debug for BackupCodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupCodes")
            .field("count", &self.backup_codes.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_passwords_secrets_and_codes() {
        let all = [
            format!(
                "{:?}",
                SetPassword {
                    password: "hunter2-hunter2".into()
                }
            ),
            format!(
                "{:?}",
                TotpStarted {
                    secret: "JBSWY3DPEHPK3PXP".into(),
                    otpauth_uri: "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP".into(),
                    qr_svg: "<svg/>".into(),
                }
            ),
            format!(
                "{:?}",
                TotpCode {
                    code: "123456".into()
                }
            ),
            format!(
                "{:?}",
                BackupCodes {
                    backup_codes: vec!["ABCDE-FGHJK".into()]
                }
            ),
        ]
        .join("\n");
        for secret in ["hunter2", "JBSWY3DPEHPK3PXP", "123456", "ABCDE"] {
            assert!(!all.contains(secret), "{all}");
        }
    }

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_string(&BackupCodes {
                backup_codes: vec!["A".into()]
            })
            .unwrap(),
            r#"{"backup_codes":["A"]}"#
        );
    }
}

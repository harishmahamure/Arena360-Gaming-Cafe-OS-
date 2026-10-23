//! The kiosk process allow-list and block-list: the rule type both branch settings hold, the
//! rules a value must follow (the Hub validates `settings.set` with them; the kiosk re-checks
//! what it receives), the default lists every branch starts with, and the
//! `kiosk.process_blocked` intent payload.
//!
//! A rule is `{path_glob, signer?}`. `path_glob` is matched case-insensitively against the
//! normalised full path of a process image: `*` matches any run of characters within one path
//! segment, `?` one character, `**` any run of characters across segments. Two placeholders may
//! start a `path_glob` (and nowhere else): `{install}` stands for the kiosk's own install
//! directory and `{system}` for `%SystemRoot%` (each kiosk expands them). `signer` is an
//! Authenticode subject CN the file's valid signature must carry.
//!
//! - **Allow rules** must name a full path (`X:\…`, `{install}\…` or `{system}\…`): a program
//!   is never allowed by its file name alone. The allow-list may not be empty, and a rule with no
//!   signer may not sit under a user-writable root (`X:\Users\`, `X:\ProgramData\`,
//!   `X:\Temp\`, `{system}\Temp\`, or a wildcard first folder that could be one of them).
//! - **Block rules** may be a bare file name (`cmd.exe`, matched at any path) or a full path, but
//!   never a catch-all (only wildcards, optionally with `.exe`: `*`, `*.exe`, `C:\**`), which
//!   would kill everything including the kiosk.
//! - A `signer` is 1–256 characters, with no control characters and no leading or trailing
//!   space.
//!
//! The third setting, `kiosk.process_mode` (`enforce` | `report`), decides whether the kiosk
//! kills disallowed processes or only reports them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppError, ErrorCode, MAX_KIOSK_LOG_DETAIL_CHARS};

/// The allow-list setting key.
pub const KEY_PROCESS_ALLOW: &str = "kiosk.process_allow";
/// The block-list setting key.
pub const KEY_PROCESS_BLOCK: &str = "kiosk.process_block";
/// The enforcement-mode setting key (`enforce` | `report`).
pub const KEY_PROCESS_MODE: &str = "kiosk.process_mode";
/// The mode `branch.bootstrap` seeds for a new branch.
pub const BOOTSTRAP_PROCESS_MODE: &str = "enforce";
/// The mode `branch.seed_defaults` backfills on a branch that existed before the lists did: an
/// upgraded café only sees reports until an operator switches it to `enforce`.
pub const BACKFILL_PROCESS_MODE: &str = "report";
/// The Authenticode subject CN the Arena 360 binaries are signed with.
pub const ARENA_SIGNER_CN: &str = "Arena 360";
/// The placeholder for the kiosk's install directory in a `path_glob`.
pub const INSTALL_DIR_TOKEN: &str = "{install}";
/// The placeholder for `%SystemRoot%` in a `path_glob`.
pub const SYSTEM_DIR_TOKEN: &str = "{system}";
/// Both placeholders.
pub const PATH_TOKENS: [&str; 2] = [INSTALL_DIR_TOKEN, SYSTEM_DIR_TOKEN];
/// Most rules one list may hold.
pub const MAX_PROCESS_RULES: usize = 500;
/// Longest `path_glob`, in characters.
pub const MAX_PATH_GLOB_CHARS: usize = 520;
/// Longest `signer`, in characters.
pub const MAX_SIGNER_CHARS: usize = 256;
/// Longest list value, in characters (500 rules of about 260 characters each).
pub const MAX_PROCESS_RULES_CHARS: usize = 131_072;

/// One allow or block rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct ProcessRule {
    pub path_glob: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub signer: Option<String>,
}

/// Which list a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessList {
    Allow,
    Block,
}

impl ProcessList {
    /// The list `key` holds, if it is one of the two settings.
    pub fn of_key(key: &str) -> Option<Self> {
        match key {
            KEY_PROCESS_ALLOW => Some(Self::Allow),
            KEY_PROCESS_BLOCK => Some(Self::Block),
            _ => None,
        }
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "value", message)
}

/// The enforcement mode (`kiosk.process_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ProcessMode {
    /// Disallowed processes are killed (and reported).
    #[default]
    Enforce,
    /// Nothing is killed; each disallowed process is only reported.
    Report,
}

impl ProcessMode {
    /// The setting and wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Report => "report",
        }
    }

    /// Whether this is [`ProcessMode::Enforce`] (the payload leaves the default out).
    pub fn is_enforce(&self) -> bool {
        *self == Self::Enforce
    }

    /// Every value the setting takes.
    pub const VALUES: &'static [&'static str] = &["enforce", "report"];
}

/// Parses a `kiosk.process_mode` value. A failure is `VALIDATION_FAILED` on `value`.
pub fn parse_process_mode(value: &str) -> Result<ProcessMode, AppError> {
    match value {
        "enforce" => Ok(ProcessMode::Enforce),
        "report" => Ok(ProcessMode::Report),
        _ => Err(invalid("kiosk.process_mode must be enforce or report")),
    }
}

/// The root prefix of a full-path glob (`C:\`, `{install}\`, `{system}\`), if it has one
/// followed by more.
fn root_of(glob: &str) -> Option<&str> {
    let b = glob.as_bytes();
    if b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        return Some(&glob[..3]);
    }
    PATH_TOKENS.iter().find_map(|t| {
        glob.strip_prefix(t)
            .filter(|rest| rest.len() > 1 && rest.starts_with('\\'))
            .map(|_| &glob[..t.len() + 1])
    })
}

/// Whether `glob` names a full path: a drive (`C:\`) or a placeholder (`{install}\`,
/// `{system}\`), then more.
pub fn is_full_path_glob(glob: &str) -> bool {
    root_of(glob).is_some()
}

/// Whether `glob` holds a placeholder anywhere but as its root (`{install}\…`, `{system}\…`).
fn misplaced_token(glob: &str) -> bool {
    let rooted = root_of(glob).is_some_and(|r| r.starts_with('{'));
    PATH_TOKENS
        .iter()
        .any(|t| glob.match_indices(t).any(|(i, _)| i > 0 || !rooted))
}

/// Whether `glob` (a bare name, or the part after a full path's root) is only wildcards and
/// separators, optionally ending in `.exe`: it matches every program.
pub fn is_catch_all_glob(glob: &str) -> bool {
    let rest = root_of(glob).map_or(glob, |r| &glob[r.len()..]);
    let lower = rest.to_ascii_lowercase();
    let body = lower.strip_suffix(".exe").unwrap_or(&lower);
    !body.is_empty() && body.chars().all(|c| matches!(c, '*' | '?' | '\\'))
}

/// Whether an allow rule's `glob` sits under a root any user can write to (or under a wildcard
/// first folder that could be one): `X:\Users\`, `X:\ProgramData\`, `X:\Temp\`,
/// `{system}\Temp\`.
pub fn is_user_writable_glob(glob: &str) -> bool {
    let Some(root) = root_of(glob) else {
        return false;
    };
    if root.eq_ignore_ascii_case(&format!("{INSTALL_DIR_TOKEN}\\")) {
        return false;
    }
    let first = glob[root.len()..].split('\\').next().unwrap_or("");
    if first.contains(['*', '?']) {
        return true;
    }
    let first = first.to_ascii_lowercase();
    if root.starts_with(SYSTEM_DIR_TOKEN) {
        first == "temp"
    } else {
        matches!(first.as_str(), "users" | "programdata" | "temp")
    }
}

/// Whether `glob` is a bare file name (no directory part).
pub fn is_file_name_glob(glob: &str) -> bool {
    !glob.is_empty() && !glob.contains(['\\', '/', ':'])
}

/// Parses and checks one list value: well-formed JSON, an array of at most
/// [`MAX_PROCESS_RULES`] rules (at least one for the allow-list), each `path_glob` 1–520
/// characters with no control characters and a placeholder only as its prefix (a full path for
/// an allow rule; a full path or a bare file name, but no catch-all, for a block rule), each
/// `signer` 1–256 characters with no control characters and no surrounding space, and no
/// unsigned allow rule under a user-writable root (see the module docs). A failure is
/// `VALIDATION_FAILED` on `value`, naming the rule.
pub fn parse_process_rules(list: ProcessList, value: &str) -> Result<Vec<ProcessRule>, AppError> {
    if value.chars().count() > MAX_PROCESS_RULES_CHARS {
        return Err(invalid(format!(
            "the list must be at most {MAX_PROCESS_RULES_CHARS} characters"
        )));
    }
    let raw: serde_json::Value =
        serde_json::from_str(value).map_err(|e| invalid(format!("the list is not JSON: {e}")))?;
    let Some(items) = raw.as_array() else {
        return Err(invalid("the list must be a JSON array of rules"));
    };
    if list == ProcessList::Allow && items.is_empty() {
        return Err(invalid(
            "the allow-list may not be empty: it would kill every program, the kiosk's own included",
        ));
    }
    if items.len() > MAX_PROCESS_RULES {
        return Err(invalid(format!(
            "the list holds {} rules; at most {MAX_PROCESS_RULES} are allowed",
            items.len()
        )));
    }
    let mut rules = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let rule: ProcessRule = serde_json::from_value(item.clone()).map_err(|e| {
            invalid(format!(
                "rule {i} must be {{path_glob, signer?}} with no other fields: {e}"
            ))
        })?;
        let glob = &rule.path_glob;
        let len = glob.chars().count();
        if len == 0 || len > MAX_PATH_GLOB_CHARS || glob.chars().any(char::is_control) {
            return Err(invalid(format!(
                "rule {i}: path_glob must be 1–{MAX_PATH_GLOB_CHARS} characters with no control characters"
            )));
        }
        if misplaced_token(glob) {
            return Err(invalid(format!(
                "rule {i}: {INSTALL_DIR_TOKEN} and {SYSTEM_DIR_TOKEN} may only start a path_glob"
            )));
        }
        let shape_ok = match list {
            ProcessList::Allow => is_full_path_glob(glob),
            ProcessList::Block => is_full_path_glob(glob) || is_file_name_glob(glob),
        };
        if !shape_ok {
            return Err(invalid(match list {
                ProcessList::Allow => format!(
                    "rule {i}: an allow rule's path_glob must be a full path (C:\\…, {INSTALL_DIR_TOKEN}\\… or {SYSTEM_DIR_TOKEN}\\…)"
                ),
                ProcessList::Block => {
                    format!("rule {i}: a block rule's path_glob must be a full path or a file name")
                }
            }));
        }
        if list == ProcessList::Block && is_catch_all_glob(glob) {
            return Err(invalid(format!(
                "rule {i}: a block rule may not match every program ({glob}); name the programs to block"
            )));
        }
        if let Some(signer) = &rule.signer {
            let len = signer.chars().count();
            if signer.trim().is_empty()
                || signer.trim() != signer
                || len > MAX_SIGNER_CHARS
                || signer.chars().any(char::is_control)
            {
                return Err(invalid(format!(
                    "rule {i}: signer must be 1–{MAX_SIGNER_CHARS} characters with no control characters and no leading or trailing space"
                )));
            }
        } else if list == ProcessList::Allow && is_user_writable_glob(glob) {
            return Err(invalid(format!(
                "rule {i}: an allow rule under a user-writable folder ({glob}) must name a signer"
            )));
        }
        rules.push(rule);
    }
    Ok(rules)
}

/// The allow-list every branch starts with: the kiosk app and service (Arena-signed, in the
/// install directory), the Windows processes a player session needs (signed "Microsoft
/// Windows", under `{system}`) and WebView2 wherever the kiosk's runtime may live (machine-wide
/// Evergreen in either Program Files, per-user Evergreen, or a fixed-version runtime under the
/// install directory; signed "Microsoft Corporation").
pub const DEFAULT_PROCESS_ALLOW: &str = r#"[
  {"path_glob": "{install}\\arena-kiosk-ui.exe", "signer": "Arena 360"},
  {"path_glob": "{install}\\arena-kiosk-svc.exe", "signer": "Arena 360"},
  {"path_glob": "{system}\\System32\\conhost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\dllhost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\ctfmon.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\fontdrvhost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\sihost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\RuntimeBroker.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\smartscreen.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\WerFault.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\taskhostw.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\userinit.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\System32\\backgroundTaskHost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "{system}\\SystemApps\\*\\TextInputHost.exe", "signer": "Microsoft Windows"},
  {"path_glob": "C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\\*\\msedgewebview2.exe", "signer": "Microsoft Corporation"},
  {"path_glob": "C:\\Program Files\\Microsoft\\EdgeWebView\\Application\\*\\msedgewebview2.exe", "signer": "Microsoft Corporation"},
  {"path_glob": "C:\\Users\\*\\AppData\\Local\\Microsoft\\EdgeWebView\\Application\\*\\msedgewebview2.exe", "signer": "Microsoft Corporation"},
  {"path_glob": "{install}\\**\\msedgewebview2.exe", "signer": "Microsoft Corporation"}
]"#;

/// The block-list every branch starts with (file names, matched at any path).
pub const DEFAULT_PROCESS_BLOCK: &str = r#"[
  {"path_glob": "cmd.exe"},
  {"path_glob": "powershell.exe"},
  {"path_glob": "pwsh.exe"},
  {"path_glob": "regedit.exe"},
  {"path_glob": "taskmgr.exe"},
  {"path_glob": "mmc.exe"},
  {"path_glob": "explorer.exe"},
  {"path_glob": "control.exe"},
  {"path_glob": "msconfig.exe"},
  {"path_glob": "wscript.exe"},
  {"path_glob": "cscript.exe"},
  {"path_glob": "mshta.exe"}
]"#;

/// Why the kiosk killed (or, in report mode, would have killed) a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum ProcessBlockReason {
    /// It matched a block rule.
    Blocked,
    /// No allow rule matched its path.
    NotAllowed,
    /// An allow rule matched its path, but its signature is missing, invalid or by another
    /// signer.
    Signature,
}

impl ProcessBlockReason {
    /// The wire and stored form, e.g. `not_allowed`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::NotAllowed => "not_allowed",
            Self::Signature => "signature",
        }
    }
}

/// Longest `exe_name`, in characters.
pub const MAX_EXE_NAME_CHARS: usize = 260;
/// Longest `path`, in characters.
pub const MAX_PROCESS_PATH_CHARS: usize = 1024;

/// The payload of `kiosk.process_blocked`: the kiosk killed a process in the player session
/// (`mode: enforce`, the default, left out on the wire) or, in report mode, found one it would
/// have killed and left running (`mode: report`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KioskProcessBlocked {
    /// The image's file name, e.g. `cmd.exe`.
    pub exe_name: String,
    /// The normalised full path of the image (the kiosk shortens a long one from the left).
    pub path: String,
    pub reason: ProcessBlockReason,
    /// Whether the process was killed (`enforce`) or only reported (`report`).
    #[serde(default, skip_serializing_if = "ProcessMode::is_enforce")]
    #[ts(as = "Option<ProcessMode>", optional)]
    pub mode: ProcessMode,
}

/// The ellipsis a path shortened from the left starts with.
pub const PATH_ELLIPSIS: char = '…';

/// `path` shortened from the left to at most `max` characters: `…` then its last `max - 1`
/// characters (unchanged when it fits; empty when `max` is 0).
pub fn truncate_path_left(path: &str, max: usize) -> String {
    let n = path.chars().count();
    if n <= max {
        return path.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let tail: String = path.chars().skip(n - (max - 1)).collect();
    format!("{PATH_ELLIPSIS}{tail}")
}

impl KioskProcessBlocked {
    /// The payload rules the Hub applies (and the kiosk checks before queueing): `exe_name`
    /// 1–260 and `path` 1–1024 characters, neither with control characters. A failure is
    /// `VALIDATION_FAILED` naming the field.
    pub fn validate(&self) -> Result<(), AppError> {
        let check = |field: &str, v: &str, max: usize| {
            let len = v.chars().count();
            if len == 0 || len > max || v.chars().any(char::is_control) {
                Err(AppError::with_field(
                    ErrorCode::ValidationFailed,
                    field,
                    format!("{field} must be 1–{max} characters with no control characters"),
                ))
            } else {
                Ok(())
            }
        };
        check("exe_name", &self.exe_name, MAX_EXE_NAME_CHARS)?;
        check("path", &self.path, MAX_PROCESS_PATH_CHARS)
    }

    /// The `device_events.detail` text: `cmd.exe (blocked): C:\Windows\System32\cmd.exe`, or
    /// `cmd.exe (blocked, report only): …` in report mode. It is at most
    /// [`MAX_KIOSK_LOG_DETAIL_CHARS`] characters, like a `kiosk.log` detail: a long path is
    /// shortened from the left (`…`), and the file name too if it alone is too long.
    pub fn detail(&self) -> String {
        let mode = match self.mode {
            ProcessMode::Enforce => "",
            ProcessMode::Report => ", report only",
        };
        let max = MAX_KIOSK_LOG_DETAIL_CHARS;
        let head = format!("{} ({}{mode}): ", self.exe_name, self.reason.as_str());
        let head_len = head.chars().count();
        if head_len >= max {
            return truncate_path_left(&head, max);
        }
        format!("{head}{}", truncate_path_left(&self.path, max - head_len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(list: ProcessList, json: &str) -> Result<Vec<ProcessRule>, AppError> {
        parse_process_rules(list, json)
    }

    #[test]
    fn the_defaults_parse_and_hold_the_listed_programs() {
        let allow = rules(ProcessList::Allow, DEFAULT_PROCESS_ALLOW).unwrap();
        assert_eq!(allow.len(), 18);
        assert_eq!(
            allow[0],
            ProcessRule {
                path_glob: r"{install}\arena-kiosk-ui.exe".into(),
                signer: Some(ARENA_SIGNER_CN.into()),
            }
        );
        for name in [
            "conhost.exe",
            "dllhost.exe",
            "ctfmon.exe",
            "fontdrvhost.exe",
            "sihost.exe",
            "RuntimeBroker.exe",
            "smartscreen.exe",
            "WerFault.exe",
            "taskhostw.exe",
            "userinit.exe",
            "backgroundTaskHost.exe",
        ] {
            let want = format!(r"{{system}}\System32\{name}");
            assert!(
                allow.iter().any(
                    |r| r.path_glob == want && r.signer.as_deref() == Some("Microsoft Windows")
                ),
                "{name}"
            );
        }
        // No default rule hard-codes the Windows directory.
        assert!(
            allow
                .iter()
                .all(|r| !r.path_glob.to_ascii_lowercase().contains(r"c:\windows"))
        );
        assert!(allow.iter().any(
            |r| r.path_glob == r"{system}\SystemApps\*\TextInputHost.exe"
                && r.signer.as_deref() == Some("Microsoft Windows")
        ));
        for glob in [
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\*\msedgewebview2.exe",
            r"C:\Program Files\Microsoft\EdgeWebView\Application\*\msedgewebview2.exe",
            r"C:\Users\*\AppData\Local\Microsoft\EdgeWebView\Application\*\msedgewebview2.exe",
            r"{install}\**\msedgewebview2.exe",
        ] {
            assert!(
                allow
                    .iter()
                    .any(|r| r.path_glob == glob
                        && r.signer.as_deref() == Some("Microsoft Corporation")),
                "{glob}"
            );
        }
        let block = rules(ProcessList::Block, DEFAULT_PROCESS_BLOCK).unwrap();
        let names: Vec<&str> = block.iter().map(|r| r.path_glob.as_str()).collect();
        assert_eq!(
            names,
            [
                "cmd.exe",
                "powershell.exe",
                "pwsh.exe",
                "regedit.exe",
                "taskmgr.exe",
                "mmc.exe",
                "explorer.exe",
                "control.exe",
                "msconfig.exe",
                "wscript.exe",
                "cscript.exe",
                "mshta.exe"
            ]
        );
        assert!(block.iter().all(|r| r.signer.is_none()));
        assert!(DEFAULT_PROCESS_ALLOW.len() < MAX_PROCESS_RULES_CHARS);
    }

    #[test]
    fn the_validator_wants_json_arrays_of_at_most_500_well_formed_rules() {
        let err = |list, json: &str| rules(list, json).unwrap_err();
        for bad in ["", "{", "{}", "\"cmd.exe\"", "[1]", "[{\"signer\": \"x\"}]"] {
            let e = err(ProcessList::Block, bad);
            assert_eq!(e.code, ErrorCode::ValidationFailed, "{bad}");
            assert_eq!(e.field.as_deref(), Some("value"));
        }
        // An empty allow-list would kill everything; an empty block-list is fine.
        err(ProcessList::Allow, "[]");
        assert!(rules(ProcessList::Block, "[]").unwrap().is_empty());
        // Unknown fields are refused: a typo must not silently drop the signer.
        err(
            ProcessList::Allow,
            r#"[{"path_glob": "C:\\Games\\x.exe", "signr": "Valve"}]"#,
        );
        let one = r#"{"path_glob": "C:\\Games\\*.exe"}"#;
        let list = |n: usize| format!("[{}]", vec![one; n].join(","));
        assert_eq!(rules(ProcessList::Allow, &list(500)).unwrap().len(), 500);
        assert!(err(ProcessList::Allow, &list(501)).message.contains("501"));
        let huge = format!(
            r#"[{{"path_glob": "C:\\{}"}}]"#,
            "a".repeat(MAX_PROCESS_RULES_CHARS)
        );
        err(ProcessList::Allow, &huge);
    }

    #[test]
    fn allow_rules_need_a_full_path_block_rules_may_be_a_name() {
        let ok = |list, glob: &str| {
            let json = serde_json::json!([{ "path_glob": glob }]).to_string();
            rules(list, &json).is_ok()
        };
        assert!(ok(ProcessList::Allow, r"C:\Games\*.exe"));
        assert!(ok(ProcessList::Allow, r"{install}\arena-kiosk-ui.exe"));
        assert!(ok(ProcessList::Allow, r"{system}\System32\x.exe"));
        assert!(!ok(ProcessList::Allow, r"{system}"));
        // A placeholder only as the prefix.
        assert!(!ok(ProcessList::Allow, r"C:\Kiosk\{install}\x.exe"));
        assert!(!ok(ProcessList::Allow, r"{install}\{system}\x.exe"));
        assert!(!ok(ProcessList::Block, r"{system}x.exe"));
        assert!(ok(ProcessList::Block, r"{system}\System32\cmd.exe"));
        assert!(!ok(ProcessList::Allow, "arena-kiosk-ui.exe"));
        assert!(!ok(ProcessList::Allow, r"*\arena-kiosk-ui.exe"));
        assert!(!ok(ProcessList::Allow, r"C:"));
        assert!(!ok(ProcessList::Allow, r"{install}"));
        assert!(!ok(ProcessList::Allow, r"\\server\share\x.exe"));
        assert!(ok(ProcessList::Block, "cmd.exe"));
        assert!(ok(ProcessList::Block, r"C:\Temp\*.exe"));
        assert!(!ok(ProcessList::Block, r"Temp\x.exe"));
        assert!(!ok(ProcessList::Block, ""));
        assert!(!ok(ProcessList::Block, "cmd\u{7}.exe"));
        assert!(!ok(
            ProcessList::Block,
            &"a".repeat(MAX_PATH_GLOB_CHARS + 1)
        ));
        let signer = |s: &str| {
            let json = serde_json::json!([{ "path_glob": "C:\\x.exe", "signer": s }]).to_string();
            rules(ProcessList::Allow, &json).is_ok()
        };
        assert!(signer("Valve Corp."));
        assert!(!signer(""));
        assert!(!signer("  "));
        assert!(!signer(" Valve Corp."));
        assert!(!signer("Valve Corp. "));
        assert!(!signer(&"s".repeat(MAX_SIGNER_CHARS + 1)));
        assert_eq!(
            ProcessList::of_key(KEY_PROCESS_ALLOW),
            Some(ProcessList::Allow)
        );
        assert_eq!(ProcessList::of_key("branch.currency"), None);
    }

    #[test]
    fn operator_foot_guns_are_refused() {
        let ok = |list, glob: &str, signer: Option<&str>| {
            let json = serde_json::json!([{ "path_glob": glob, "signer": signer }])
                .to_string()
                .replace(r#","signer":null"#, "");
            rules(list, &json).is_ok()
        };
        // Catch-all block rules.
        for glob in [
            "*",
            "*.exe",
            "*.EXE",
            "?*.exe",
            "**",
            r"C:\*",
            r"C:\**\*.exe",
            r"{install}\*",
        ] {
            assert!(!ok(ProcessList::Block, glob, None), "{glob}");
            assert!(!ok(ProcessList::Block, glob, Some("Bad Corp")), "{glob}");
        }
        for glob in [
            "cmd*.exe",
            r"C:\Temp\*.exe",
            r"{system}\System32\cmd.exe",
            "x?.exe",
        ] {
            assert!(ok(ProcessList::Block, glob, None), "{glob}");
        }
        // Unsigned allow rules under user-writable roots.
        for glob in [
            r"C:\Users\*\Downloads\*.exe",
            r"c:\users\player\x.exe",
            r"D:\ProgramData\x\y.exe",
            r"C:\Temp\x.exe",
            r"{system}\Temp\x.exe",
            r"{system}\TEMP\x.exe",
            r"C:\*\x.exe",
            r"C:\U*\x.exe",
        ] {
            assert!(!ok(ProcessList::Allow, glob, None), "{glob}");
            assert!(ok(ProcessList::Allow, glob, Some("Valve Corp.")), "{glob}");
        }
        for glob in [
            r"C:\Games\*.exe",
            r"C:\Program Files\Steam\steam.exe",
            r"{system}\System32\x.exe",
            r"{install}\x.exe",
        ] {
            assert!(ok(ProcessList::Allow, glob, None), "{glob}");
        }
        assert!(is_catch_all_glob("*.exe") && !is_catch_all_glob(".exe"));
        assert!(is_user_writable_glob(r"C:\Users\x.exe"));
        assert!(!is_user_writable_glob("x.exe"));
    }

    #[test]
    fn the_mode_setting_parses() {
        assert_eq!(parse_process_mode("enforce").unwrap(), ProcessMode::Enforce);
        assert_eq!(parse_process_mode("report").unwrap(), ProcessMode::Report);
        for bad in ["", "Enforce", "off", " report"] {
            assert_eq!(
                parse_process_mode(bad).unwrap_err().code,
                ErrorCode::ValidationFailed
            );
        }
        assert_eq!(ProcessMode::VALUES, ["enforce", "report"]);
        assert_eq!(
            parse_process_mode(BOOTSTRAP_PROCESS_MODE).unwrap(),
            ProcessMode::Enforce
        );
        assert_eq!(
            parse_process_mode(BACKFILL_PROCESS_MODE).unwrap(),
            ProcessMode::Report
        );
    }

    #[test]
    fn a_long_detail_is_capped_with_the_path_cut_from_the_left() {
        let path = format!(r"C:\{}\game.exe", "d".repeat(900));
        let p = KioskProcessBlocked {
            exe_name: "game.exe".into(),
            path: path.clone(),
            reason: ProcessBlockReason::NotAllowed,
            mode: ProcessMode::Report,
        };
        p.validate().unwrap();
        let d = p.detail();
        assert_eq!(d.chars().count(), MAX_KIOSK_LOG_DETAIL_CHARS);
        assert!(
            d.starts_with("game.exe (not_allowed, report only): …ddd"),
            "{d}"
        );
        assert!(d.ends_with(r"\game.exe"));
        assert_eq!(truncate_path_left("abcdef", 4), "…def");
        assert_eq!(truncate_path_left("abc", 4), "abc");
        assert_eq!(truncate_path_left("abc", 0), "");
        let long_name = KioskProcessBlocked {
            exe_name: "n".repeat(MAX_EXE_NAME_CHARS),
            ..p
        };
        // Still capped, even though the name alone nearly fills it.
        assert!(long_name.detail().chars().count() <= MAX_KIOSK_LOG_DETAIL_CHARS);
    }

    #[test]
    fn the_mode_is_optional_on_the_wire_and_defaults_to_enforce() {
        let p: KioskProcessBlocked = serde_json::from_str(
            r#"{"exe_name": "cmd.exe", "path": "C:\\x\\cmd.exe", "reason": "blocked"}"#,
        )
        .unwrap();
        assert_eq!(p.mode, ProcessMode::Enforce);
        assert!(serde_json::to_value(&p).unwrap().get("mode").is_none());
        let r = KioskProcessBlocked {
            mode: ProcessMode::Report,
            ..p
        };
        assert_eq!(serde_json::to_value(&r).unwrap()["mode"], "report");
    }

    #[test]
    fn the_blocked_payload_is_checked_and_described() {
        let p = KioskProcessBlocked {
            exe_name: "cmd.exe".into(),
            path: r"C:\Windows\System32\cmd.exe".into(),
            reason: ProcessBlockReason::Blocked,
            mode: ProcessMode::Enforce,
        };
        p.validate().unwrap();
        assert_eq!(
            p.detail(),
            r"cmd.exe (blocked): C:\Windows\System32\cmd.exe"
        );
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["reason"], "blocked");
        for reason in [
            ProcessBlockReason::Blocked,
            ProcessBlockReason::NotAllowed,
            ProcessBlockReason::Signature,
        ] {
            assert_eq!(
                serde_json::to_string(&reason).unwrap(),
                format!("\"{}\"", reason.as_str())
            );
        }
        let bad = KioskProcessBlocked {
            exe_name: String::new(),
            ..p.clone()
        };
        assert_eq!(
            bad.validate().unwrap_err().field.as_deref(),
            Some("exe_name")
        );
        let bad = KioskProcessBlocked {
            path: "x".repeat(MAX_PROCESS_PATH_CHARS + 1),
            ..p
        };
        assert_eq!(bad.validate().unwrap_err().field.as_deref(), Some("path"));
    }
}

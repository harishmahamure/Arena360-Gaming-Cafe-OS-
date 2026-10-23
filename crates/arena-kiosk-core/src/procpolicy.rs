//! The kiosk's process decision, pure and platform-neutral: given a process image's
//! normalised path and its Authenticode result, and the branch's allow-list and block-list,
//! keep it or kill it.
//!
//! - A **block** rule that matches kills (`blocked`). A block rule that is a bare file name
//!   matches the image's file name at any path; one with a path matches the full path. A block
//!   rule naming a signer matches only a valid signature by that signer.
//! - Otherwise the process must match an **allow** rule: the rule's full-path glob matches the
//!   path and, when the rule names a signer, the signature is valid and by exactly that signer.
//!   A path match whose signature fails kills with `signature`; no path match kills with
//!   `not_allowed`. So a renamed blocked program is killed because it is not allowed, and an
//!   allowed name at another path, or unsigned, is killed too. Nothing is ever allowed by its
//!   file name alone.
//!
//! Globs match case-insensitively: `*` is any run of characters within one path segment, `**`
//! any run across segments, `?` one character other than `\`. `{install}` and `{system}` at
//! the start of a rule are replaced by the kiosk's install directory and `%SystemRoot%` with
//! [`expand_dirs`]; a rule still holding a placeholder never matches.

use arena_proto::{
    INSTALL_DIR_TOKEN, PATH_TOKENS, ProcessBlockReason, ProcessRule, SYSTEM_DIR_TOKEN,
};

/// A process as the decision sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    /// The image's full path, normalised with [`normalize_path`].
    pub path: String,
    /// The subject CN of the image's Authenticode signer, when it has a signature.
    pub signer: Option<String>,
    /// Whether that signature verified (`WinVerifyTrust`).
    pub valid_sig: bool,
}

/// What to do with a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Kill(ProcessBlockReason),
}

/// Normalises a Windows image path: `/` becomes `\`, the `\\?\` / `\??\` prefixes are dropped
/// (`\\?\UNC\` becomes `\\`), repeated separators collapse, `.` and `..` segments are resolved
/// and a trailing `\` is dropped. The case is kept (matching ignores it).
pub fn normalize_path(raw: &str) -> String {
    let mut p = raw.trim().replace('/', "\\");
    for prefix in [r"\\?\UNC\", r"\??\UNC\"] {
        if let Some(rest) = p.strip_prefix(prefix) {
            p = format!(r"\\{rest}");
        }
    }
    for prefix in [r"\\?\", r"\??\"] {
        if let Some(rest) = p.strip_prefix(prefix) {
            p = rest.to_owned();
        }
    }
    let unc = p.starts_with(r"\\");
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('\\') {
        match seg {
            "" | "." => {}
            ".." => {
                // Never above the root (the drive, or the UNC server and share).
                let floor = if unc { 2 } else { 1 };
                if parts.len() > floor {
                    parts.pop();
                }
            }
            s => parts.push(s),
        }
    }
    let body = parts.join("\\");
    if unc { format!(r"\\{body}") } else { body }
}

/// The last segment of a normalised path.
pub fn file_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

fn fold(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// Whether `glob` matches all of `text`, ignoring case (`*` within one segment, `**` across
/// segments, `?` one non-separator character).
pub fn glob_match(glob: &str, text: &str) -> bool {
    #[derive(Clone, Copy)]
    enum Tok {
        Lit(char),
        One,
        Star,
        Deep,
    }
    let g = fold(glob);
    let mut toks = Vec::with_capacity(g.len());
    let mut i = 0;
    while i < g.len() {
        match g[i] {
            '*' if g.get(i + 1) == Some(&'*') => {
                while g.get(i) == Some(&'*') {
                    i += 1;
                }
                toks.push(Tok::Deep);
                continue;
            }
            '*' => toks.push(Tok::Star),
            '?' => toks.push(Tok::One),
            c => toks.push(Tok::Lit(c)),
        }
        i += 1;
    }
    let t = fold(text);
    // m[j]: the first `i` glob tokens match the first `j` text characters.
    let mut m = vec![false; t.len() + 1];
    m[0] = true;
    for tok in toks {
        let mut next = vec![false; t.len() + 1];
        match tok {
            Tok::Star | Tok::Deep => {
                next[0] = m[0];
                for j in 1..=t.len() {
                    let may_take = matches!(tok, Tok::Deep) || t[j - 1] != '\\';
                    next[j] = m[j] || (may_take && next[j - 1]);
                }
            }
            Tok::One => {
                for j in 1..=t.len() {
                    next[j] = m[j - 1] && t[j - 1] != '\\';
                }
            }
            Tok::Lit(c) => {
                for j in 1..=t.len() {
                    next[j] = m[j - 1] && t[j - 1] == c;
                }
            }
        }
        m = next;
    }
    m[t.len()]
}

/// The directories the placeholders stand for on this kiosk.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dirs<'a> {
    /// `{install}`: the kiosk's install directory.
    pub install: Option<&'a str>,
    /// `{system}`: `%SystemRoot%`.
    pub system: Option<&'a str>,
}

/// `rules` with a leading `{install}` / `{system}` replaced by its directory (normalised, no
/// trailing `\`). A placeholder with no directory is left as it is, and that rule never
/// matches.
pub fn expand_dirs(rules: &[ProcessRule], dirs: Dirs<'_>) -> Vec<ProcessRule> {
    let norm = |d: Option<&str>| d.map(normalize_path).filter(|d| !d.is_empty());
    let install = norm(dirs.install);
    let system = norm(dirs.system);
    rules
        .iter()
        .map(|r| {
            let mut out = r.clone();
            for (token, dir) in [(INSTALL_DIR_TOKEN, &install), (SYSTEM_DIR_TOKEN, &system)] {
                if let (Some(d), Some(rest)) = (dir, r.path_glob.strip_prefix(token)) {
                    out.path_glob = format!("{d}{rest}");
                }
            }
            out
        })
        .collect()
}

/// `rules` with `{install}` replaced by `install_dir` (see [`expand_dirs`]).
pub fn expand_install(rules: &[ProcessRule], install_dir: Option<&str>) -> Vec<ProcessRule> {
    expand_dirs(
        rules,
        Dirs {
            install: install_dir,
            system: None,
        },
    )
}

fn signed_by(proc: &ProcInfo, signer: &str) -> bool {
    proc.valid_sig && proc.signer.as_deref() == Some(signer)
}

fn is_name_only(glob: &str) -> bool {
    !glob.contains(['\\', '/', ':'])
}

fn path_matches(glob: &str, path: &str) -> bool {
    !PATH_TOKENS.iter().any(|t| glob.contains(t)) && glob_match(&normalize_path(glob), path)
}

/// The decision for `proc` (see the module docs).
pub fn decide(proc: &ProcInfo, allow: &[ProcessRule], block: &[ProcessRule]) -> Decision {
    let path = &proc.path;
    let name = file_name(path);
    let blocked = block.iter().any(|r| {
        let hit = if is_name_only(&r.path_glob) {
            glob_match(&r.path_glob, name)
        } else {
            path_matches(&r.path_glob, path)
        };
        hit && r.signer.as_deref().is_none_or(|s| signed_by(proc, s))
    });
    if blocked {
        return Decision::Kill(ProcessBlockReason::Blocked);
    }
    let mut path_allowed = false;
    for r in allow {
        // A bare name never allows anything.
        if is_name_only(&r.path_glob) || !path_matches(&r.path_glob, path) {
            continue;
        }
        path_allowed = true;
        match &r.signer {
            None => return Decision::Allow,
            Some(s) if signed_by(proc, s) => return Decision::Allow,
            Some(_) => {}
        }
    }
    Decision::Kill(if path_allowed {
        ProcessBlockReason::Signature
    } else {
        ProcessBlockReason::NotAllowed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::{
        ARENA_SIGNER_CN, DEFAULT_PROCESS_ALLOW, DEFAULT_PROCESS_BLOCK, ProcessList,
        parse_process_rules,
    };

    const INSTALL: &str = r"C:\Program Files\Arena 360 Kiosk";
    const SYSTEM: &str = r"C:\Windows";

    fn dirs() -> Dirs<'static> {
        Dirs {
            install: Some(INSTALL),
            system: Some(SYSTEM),
        }
    }

    fn defaults() -> (Vec<ProcessRule>, Vec<ProcessRule>) {
        let allow = parse_process_rules(ProcessList::Allow, DEFAULT_PROCESS_ALLOW).unwrap();
        let block = parse_process_rules(ProcessList::Block, DEFAULT_PROCESS_BLOCK).unwrap();
        (expand_dirs(&allow, dirs()), expand_dirs(&block, dirs()))
    }

    fn proc(path: &str, signer: Option<&str>, valid_sig: bool) -> ProcInfo {
        ProcInfo {
            path: normalize_path(path),
            signer: signer.map(str::to_owned),
            valid_sig,
        }
    }

    fn decide_default(p: &ProcInfo) -> Decision {
        let (allow, block) = defaults();
        decide(p, &allow, &block)
    }

    use ProcessBlockReason::{Blocked, NotAllowed, Signature};

    #[test]
    fn matrix_blocked_name() {
        let p = proc(
            r"C:\Windows\System32\cmd.exe",
            Some("Microsoft Windows"),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Kill(Blocked));
        // Any path and any case.
        let p = proc(r"D:\tools\CMD.EXE", None, false);
        assert_eq!(decide_default(&p), Decision::Kill(Blocked));
    }

    #[test]
    fn matrix_renamed_blocked_is_not_allowed() {
        // Really cmd, renamed: its signature is Microsoft's, but no allow rule has this path.
        let p = proc(
            r"C:\Users\player\Downloads\notepad.exe",
            Some("Microsoft Windows"),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Kill(NotAllowed));
    }

    #[test]
    fn matrix_allowed_and_signed() {
        let p = proc(
            &format!(r"{INSTALL}\arena-kiosk-ui.exe"),
            Some(ARENA_SIGNER_CN),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Allow);
        let p = proc(
            r"c:\windows\system32\CONHOST.exe",
            Some("Microsoft Windows"),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Allow);
        let p = proc(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\130.0.2849.80\msedgewebview2.exe",
            Some("Microsoft Corporation"),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Allow);
    }

    #[test]
    fn matrix_allowed_name_at_the_wrong_path() {
        let p = proc(r"C:\Temp\arena-kiosk-ui.exe", Some(ARENA_SIGNER_CN), true);
        assert_eq!(decide_default(&p), Decision::Kill(NotAllowed));
        // `*` does not cross a separator: a nested folder is not the version folder.
        let p = proc(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\1\x\msedgewebview2.exe",
            Some("Microsoft Corporation"),
            true,
        );
        assert_eq!(decide_default(&p), Decision::Kill(NotAllowed));
    }

    #[test]
    fn matrix_allowed_path_with_a_bad_signature() {
        let path = format!(r"{INSTALL}\arena-kiosk-ui.exe");
        for (signer, valid) in [
            (Some(ARENA_SIGNER_CN), false),
            (None, false),
            (Some("Someone Else"), true),
            (Some("arena 360"), true),
        ] {
            let p = proc(&path, signer, valid);
            assert_eq!(
                decide_default(&p),
                Decision::Kill(Signature),
                "{signer:?} {valid}"
            );
        }
    }

    #[test]
    fn matrix_unknown_program() {
        let p = proc(r"C:\Games\x.exe", None, false);
        assert_eq!(decide_default(&p), Decision::Kill(NotAllowed));
    }

    #[test]
    fn matrix_a_wider_allow_list_lets_the_next_launch_through() {
        let (mut allow, block) = defaults();
        let p = proc(r"C:\Games\x.exe", None, false);
        assert_eq!(decide(&p, &allow, &block), Decision::Kill(NotAllowed));
        allow.extend(
            parse_process_rules(ProcessList::Allow, r#"[{"path_glob": "C:\\Games\\*.exe"}]"#)
                .unwrap(),
        );
        assert_eq!(decide(&p, &allow, &block), Decision::Allow);
        // Still not a blocked name, and still not a subfolder.
        let cmd = proc(r"C:\Games\cmd.exe", None, false);
        assert_eq!(decide(&cmd, &allow, &block), Decision::Kill(Blocked));
        let deeper = proc(r"C:\Games\sub\x.exe", None, false);
        assert_eq!(decide(&deeper, &allow, &block), Decision::Kill(NotAllowed));
    }

    #[test]
    fn a_name_only_allow_rule_allows_nothing() {
        let allow = vec![ProcessRule {
            path_glob: "x.exe".into(),
            signer: None,
        }];
        let p = proc(r"C:\Games\x.exe", None, false);
        assert_eq!(decide(&p, &allow, &[]), Decision::Kill(NotAllowed));
    }

    #[test]
    fn a_signed_block_rule_only_hits_that_signer() {
        let block = vec![ProcessRule {
            path_glob: r"C:\Games\*.exe".into(),
            signer: Some("Bad Corp".into()),
        }];
        let allow = vec![ProcessRule {
            path_glob: r"C:\Games\*.exe".into(),
            signer: None,
        }];
        let bad = proc(r"C:\Games\a.exe", Some("Bad Corp"), true);
        assert_eq!(decide(&bad, &allow, &block), Decision::Kill(Blocked));
        let ok = proc(r"C:\Games\a.exe", Some("Good Corp"), true);
        assert_eq!(decide(&ok, &allow, &block), Decision::Allow);
    }

    #[test]
    fn install_placeholders_expand_or_never_match() {
        let (allow, _) = defaults();
        assert_eq!(
            allow[0].path_glob,
            r"C:\Program Files\Arena 360 Kiosk\arena-kiosk-ui.exe"
        );
        let raw = parse_process_rules(ProcessList::Allow, DEFAULT_PROCESS_ALLOW).unwrap();
        let unexpanded = expand_install(&raw, None);
        let p = proc(r"{install}\arena-kiosk-ui.exe", Some(ARENA_SIGNER_CN), true);
        assert_eq!(decide(&p, &unexpanded, &[]), Decision::Kill(NotAllowed));
        let trailing = expand_install(&raw, Some(r"C:\Kiosk\"));
        assert_eq!(trailing[0].path_glob, r"C:\Kiosk\arena-kiosk-ui.exe");
    }

    #[test]
    fn the_system_placeholder_follows_systemroot() {
        let raw = parse_process_rules(ProcessList::Allow, DEFAULT_PROCESS_ALLOW).unwrap();
        let elsewhere = expand_dirs(
            &raw,
            Dirs {
                install: Some(INSTALL),
                system: Some(r"D:\WINNT"),
            },
        );
        let conhost = |path: &str| proc(path, Some("Microsoft Windows"), true);
        assert_eq!(
            decide(&conhost(r"D:\WINNT\System32\conhost.exe"), &elsewhere, &[]),
            Decision::Allow
        );
        assert_eq!(
            decide(
                &conhost(r"C:\Windows\System32\conhost.exe"),
                &elsewhere,
                &[]
            ),
            Decision::Kill(NotAllowed)
        );
        // Unexpanded, a `{system}` rule never matches.
        let unexpanded = expand_install(&raw, Some(INSTALL));
        assert_eq!(
            decide(
                &conhost(r"C:\Windows\System32\conhost.exe"),
                &unexpanded,
                &[]
            ),
            Decision::Kill(NotAllowed)
        );
    }

    #[test]
    fn the_default_webview2_and_input_rules_cover_every_runtime_location() {
        for path in [
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\130.0.2849.80\msedgewebview2.exe",
            r"C:\Program Files\Microsoft\EdgeWebView\Application\130.0.2849.80\msedgewebview2.exe",
            r"C:\Users\player\AppData\Local\Microsoft\EdgeWebView\Application\130.0.2849.80\msedgewebview2.exe",
            r"C:\Program Files\Arena 360 Kiosk\Microsoft.WebView2.FixedVersionRuntime.130.0.2849.80.x64\msedgewebview2.exe",
        ] {
            let p = proc(path, Some("Microsoft Corporation"), true);
            assert_eq!(decide_default(&p), Decision::Allow, "{path}");
            let unsigned = proc(path, None, false);
            assert_eq!(
                decide_default(&unsigned),
                Decision::Kill(Signature),
                "{path}"
            );
        }
        for path in [
            r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\TextInputHost.exe",
            r"C:\Windows\System32\backgroundTaskHost.exe",
        ] {
            let p = proc(path, Some("Microsoft Windows"), true);
            assert_eq!(decide_default(&p), Decision::Allow, "{path}");
        }
    }

    #[test]
    fn paths_normalise() {
        for (raw, want) in [
            (
                r"\\?\C:\Windows\System32\cmd.exe",
                r"C:\Windows\System32\cmd.exe",
            ),
            (r"\??\C:\Windows\cmd.exe", r"C:\Windows\cmd.exe"),
            ("C:/Games//x.exe", r"C:\Games\x.exe"),
            (r"C:\Games\.\sub\..\x.exe", r"C:\Games\x.exe"),
            (r"C:\..\..\x.exe", r"C:\x.exe"),
            (r"\\?\UNC\srv\share\x.exe", r"\\srv\share\x.exe"),
            (r"\\srv\share\..\..\x.exe", r"\\srv\share\x.exe"),
            (r"C:\Games\", r"C:\Games"),
        ] {
            assert_eq!(normalize_path(raw), want, "{raw}");
        }
        assert_eq!(file_name(r"C:\Games\x.exe"), "x.exe");
        assert_eq!(file_name("x.exe"), "x.exe");
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.exe", "cmd.exe"));
        assert!(glob_match("c?d.EXE", "CMD.exe"));
        assert!(!glob_match("*.exe", r"a\b.exe"));
        assert!(glob_match(r"C:\*\x.exe", r"c:\games\x.exe"));
        assert!(!glob_match(r"C:\*\x.exe", r"C:\a\b\x.exe"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(glob_match("**", "abc"));
        assert!(glob_match("**", r"a\b\c"));
        assert!(glob_match(r"C:\K\**\w.exe", r"C:\K\a\b\w.exe"));
        assert!(glob_match(r"C:\K\**\w.exe", r"C:\K\a\w.exe"));
        assert!(!glob_match(r"C:\K\**\w.exe", r"C:\K\w.exe"));
        assert!(!glob_match(r"C:\K\**\w.exe", r"C:\Other\a\w.exe"));
        assert!(glob_match("a*b*c", "abbbc"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("?", r"\"));
        assert!(!glob_match("", "x"));
        assert!(glob_match("", ""));
    }
}

//! The named pipe's security descriptor, built as SDDL.
//!
//! The DACL is protected (`P`: nothing inherited) and holds exactly two allow ACEs: SYSTEM
//! (`SY`, the service itself) with full access, and the configured player account with
//! [`PLAYER_RIGHTS`]: `FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES`. That is
//! enough to connect, read and write, but it leaves out `FILE_APPEND_DATA` (0x4), which on a
//! pipe is `FILE_CREATE_PIPE_INSTANCE` (`GW`/`FILE_GENERIC_WRITE` would include it), so the
//! player cannot create a rogue instance of the pipe. There is no ACE for Everyone, Users,
//! Authenticated Users, Interactive, Anonymous or Administrators, so every other caller is
//! denied by default.

/// One allow ACE: who and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ace {
    /// An SDDL SID: a well-known alias (`SY`) or a string SID (`S-1-5-21-…`).
    pub sid: String,
    /// SDDL access rights, e.g. `GA` or a hex mask such as `0x12018b`.
    pub rights: &'static str,
}

/// The pipe's DACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeAcl {
    pub aces: Vec<Ace>,
}

/// The player's access mask: `FILE_GENERIC_READ` (0x120089) | `FILE_WRITE_DATA` (0x2) |
/// `FILE_WRITE_ATTRIBUTES` (0x100). No `FILE_CREATE_PIPE_INSTANCE` (0x4).
pub const PLAYER_RIGHTS: u32 = 0x0012_0089 | 0x2 | 0x100;
/// `FILE_CREATE_PIPE_INSTANCE` (= `FILE_APPEND_DATA`).
pub const FILE_CREATE_PIPE_INSTANCE: u32 = 0x4;
const PLAYER_RIGHTS_SDDL: &str = "0x12018b";

/// Groups that must never be allowed on the pipe, as SDDL aliases and as numeric SIDs:
/// Everyone, Users, Authenticated Users, Administrators, Interactive and Anonymous.
pub const FORBIDDEN_SIDS: &[&str] = &[
    "WD",
    "BU",
    "AU",
    "BA",
    "IU",
    "AN",
    "S-1-1-0",
    "S-1-5-32-545",
    "S-1-5-11",
    "S-1-5-32-544",
    "S-1-5-4",
    "S-1-5-7",
];

impl PipeAcl {
    /// SYSTEM plus the player account whose string SID is `player_sid`. A SID that is not of
    /// the `S-1-…` form is refused (an alias such as `WD` would open the pipe to everyone).
    pub fn for_player(player_sid: &str) -> Result<Self, String> {
        let well_formed = player_sid.starts_with("S-1-")
            && player_sid[4..]
                .split('-')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if !well_formed || FORBIDDEN_SIDS.contains(&player_sid) {
            return Err(format!("not an account SID: {player_sid}"));
        }
        Ok(Self {
            aces: vec![
                Ace {
                    sid: "SY".into(),
                    rights: "GA",
                },
                Ace {
                    sid: player_sid.into(),
                    rights: PLAYER_RIGHTS_SDDL,
                },
            ],
        })
    }

    /// SYSTEM only (when the player account cannot be resolved: nothing else may connect).
    pub fn system_only() -> Self {
        Self {
            aces: vec![Ace {
                sid: "SY".into(),
                rights: "GA",
            }],
        }
    }

    /// The SDDL string, e.g. `D:P(A;;GA;;;SY)(A;;0x12018b;;;S-1-5-21-…)`.
    pub fn sddl(&self) -> String {
        let mut s = String::from("D:P");
        for ace in &self.aces {
            s.push_str(&format!("(A;;{};;;{})", ace.rights, ace.sid));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";

    #[test]
    fn only_system_and_the_player_are_allowed() {
        let acl = PipeAcl::for_player(PLAYER).unwrap();
        let sids: Vec<&str> = acl.aces.iter().map(|a| a.sid.as_str()).collect();
        assert_eq!(sids, ["SY", PLAYER]);
        let sddl = acl.sddl();
        assert_eq!(sddl, format!("D:P(A;;GA;;;SY)(A;;0x12018b;;;{PLAYER})"));
        // Protected (no inherited ACEs), allow ACEs only, and no broad group anywhere.
        assert!(sddl.starts_with("D:P("));
        assert_eq!(sddl.matches("(A;").count(), 2);
        assert!(!sddl.contains("(D;"));
        for forbidden in FORBIDDEN_SIDS {
            assert!(!sddl.contains(&format!(";;;{forbidden})")), "{forbidden}");
        }
        // The player can read and write, not create pipe instances.
        let mask = u32::from_str_radix(acl.aces[1].rights.trim_start_matches("0x"), 16).unwrap();
        assert_eq!(mask, PLAYER_RIGHTS);
        assert_eq!(mask & FILE_CREATE_PIPE_INSTANCE, 0);
        assert_eq!(mask & 0x1, 0x1, "FILE_READ_DATA");
        assert_eq!(mask & 0x2, 0x2, "FILE_WRITE_DATA");
    }

    #[test]
    fn aliases_and_junk_are_not_accepted_as_the_player() {
        for bad in FORBIDDEN_SIDS
            .iter()
            .copied()
            .chain(["player", "S-1-", "S-1-5-x", ""])
        {
            assert!(PipeAcl::for_player(bad).is_err(), "{bad}");
        }
        assert_eq!(PipeAcl::system_only().sddl(), "D:P(A;;GA;;;SY)");
    }

    /// The descriptor Windows builds from the SDDL parses (Windows CI only).
    #[cfg(windows)]
    #[test]
    fn windows_accepts_the_sddl() {
        let sd = super::super::pipe::SecurityDescriptor::from_sddl(
            &PipeAcl::for_player(PLAYER).unwrap().sddl(),
        )
        .unwrap();
        drop(sd);
    }
}

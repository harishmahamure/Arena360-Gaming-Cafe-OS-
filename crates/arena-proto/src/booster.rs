//! The game booster (Story 6.25): what a PC frees up when a game starts, and what the Hub shows
//! about how long launches take.
//!
//! Three branch settings, all under `kiosk.` so every paired PC reads them and is pushed their
//! changes:
//!
//! - `kiosk.booster_close`: the programs to close at launch, executable names separated by
//!   commas (empty by default).
//! - `kiosk.booster_priority`: the game process priority, `normal`, `above_normal` (default) or
//!   `high`.
//! - `kiosk.booster_power_plan`: `true` to switch to the High performance power plan during play
//!   (`false` by default).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const KEY_BOOSTER_CLOSE: &str = "kiosk.booster_close";
pub const KEY_BOOSTER_PRIORITY: &str = "kiosk.booster_priority";
pub const KEY_BOOSTER_POWER_PLAN: &str = "kiosk.booster_power_plan";

pub const DEFAULT_BOOSTER_CLOSE: &str = "";
pub const DEFAULT_BOOSTER_PRIORITY: &str = "above_normal";
pub const DEFAULT_BOOSTER_POWER_PLAN: &str = "false";

/// The three booster settings.
pub const KIOSK_BOOSTER_KEYS: [&str; 3] = [
    KEY_BOOSTER_CLOSE,
    KEY_BOOSTER_PRIORITY,
    KEY_BOOSTER_POWER_PLAN,
];

/// The words `kiosk.booster_priority` takes, lowest first.
pub const BOOSTER_PRIORITIES: &[&str] = &["normal", "above_normal", "high"];

/// The most programs the close list holds.
pub const BOOSTER_PROGRAMS_MAX: usize = 50;
/// The longest program name, in characters.
pub const BOOSTER_PROGRAM_NAME_MAX: usize = 64;

/// How long after the click a game may take to show its first window before the launch is
/// recorded with no launch time.
pub const LAUNCH_WINDOW_TIMEOUT_MS: u32 = 180_000;

/// How far back the median launch time looks.
pub const LAUNCH_MEDIAN_DAYS: i64 = 7;

/// The priority a started game gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoosterPriority {
    Normal,
    #[default]
    AboveNormal,
    High,
}

impl BoosterPriority {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "above_normal" => Some(Self::AboveNormal),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::AboveNormal => "above_normal",
            Self::High => "high",
        }
    }
}

/// Why a close list is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoosterProgramsProblem {
    /// A name that is not an executable file name (`something.exe`, no folders).
    Shape,
    /// The same program twice.
    Duplicate,
    /// More than [`BOOSTER_PROGRAMS_MAX`].
    TooMany,
}

impl BoosterProgramsProblem {
    pub fn message(self) -> &'static str {
        match self {
            Self::Shape => "list executable file names such as discord.exe, separated by commas",
            Self::Duplicate => "each program must be listed once",
            Self::TooMany => "list at most 50 programs",
        }
    }
}

fn program_ok(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.len() > ".exe".len()
        && lower.len() <= BOOSTER_PROGRAM_NAME_MAX
        && lower.ends_with(".exe")
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '+' | '(' | ')')
        })
}

/// Reads a close list into its executable names, in the order given. Spaces around a name are
/// tolerated; a name is an `.exe` file name (letters, digits, `. _ - + ( )` and inner spaces,
/// never a folder); none repeats (ignoring case). Empty is an empty list.
pub fn parse_booster_programs(value: &str) -> Result<Vec<String>, BoosterProgramsProblem> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<String> = Vec::new();
    for part in value.split(',') {
        let name = part.trim();
        if !program_ok(name) {
            return Err(BoosterProgramsProblem::Shape);
        }
        if out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return Err(BoosterProgramsProblem::Duplicate);
        }
        out.push(name.to_owned());
    }
    if out.len() > BOOSTER_PROGRAMS_MAX {
        return Err(BoosterProgramsProblem::TooMany);
    }
    Ok(out)
}

/// The booster as a PC applies it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Booster {
    pub close: Vec<String>,
    pub priority: BoosterPriority,
    pub high_performance: bool,
}

/// One PC's launch times as the Hub UI reads them (Story 6.25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceLaunchTimes {
    pub device_id: String,
    /// The PC's label.
    pub name: String,
    /// Its latest launch with a measured time (`null` before there is one).
    pub last: Option<LastLaunch>,
    /// Each game launched here in the window, with the median of its measured times.
    pub games: Vec<GameLaunchMedian>,
}

/// A measured launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LastLaunch {
    pub game_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub launch_ms: i64,
    #[ts(type = "number")]
    pub at: i64,
}

/// One game's median launch time on one PC over the last 7 days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct GameLaunchMedian {
    pub game_id: String,
    pub name: String,
    /// Launches in the window, with or without a measured time.
    #[ts(type = "number")]
    pub launches: i64,
    /// The median of the measured times (`null` when none was measured).
    #[ts(type = "number | null")]
    pub median_ms: Option<i64>,
}

/// The answer of `GET /ui/games/launch-times`: every live paired PC, ordered by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct LaunchTimesView {
    pub devices: Vec<DeviceLaunchTimes>,
}

/// The median of `times` (the mean of the middle two when even, rounded down); `None` when
/// there are none.
pub fn median_ms(times: &mut [i64]) -> Option<i64> {
    if times.is_empty() {
        return None;
    }
    times.sort_unstable();
    let mid = times.len() / 2;
    Some(if times.len() % 2 == 1 {
        times[mid]
    } else {
        (times[mid - 1] + times[mid]) / 2
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_exe_names_separated_by_commas() {
        assert_eq!(parse_booster_programs("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_booster_programs("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_booster_programs("Discord.exe, chrome.exe ,OneDrive.exe").unwrap(),
            ["Discord.exe", "chrome.exe", "OneDrive.exe"]
        );
        for bad in [
            "discord",
            "C:\\x\\discord.exe",
            "a/b.exe",
            "discord.exe,",
            ",discord.exe",
            ".exe",
            "a.exe;b.exe",
            "a\"b.exe",
            " .exe",
            "dis\ncord.exe",
        ] {
            assert_eq!(
                parse_booster_programs(bad),
                Err(BoosterProgramsProblem::Shape),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse_booster_programs("a.exe, A.EXE"),
            Err(BoosterProgramsProblem::Duplicate)
        );
        let many = (0..=BOOSTER_PROGRAMS_MAX)
            .map(|i| format!("p{i}.exe"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_booster_programs(&many),
            Err(BoosterProgramsProblem::TooMany)
        );
        let long = format!("{}.exe", "a".repeat(BOOSTER_PROGRAM_NAME_MAX));
        assert_eq!(
            parse_booster_programs(&long),
            Err(BoosterProgramsProblem::Shape)
        );
    }

    #[test]
    fn priorities_round_trip_and_default_to_above_normal() {
        for word in BOOSTER_PRIORITIES {
            assert_eq!(BoosterPriority::parse(word).unwrap().as_str(), *word);
        }
        assert_eq!(BoosterPriority::parse("realtime"), None);
        assert_eq!(
            BoosterPriority::default().as_str(),
            DEFAULT_BOOSTER_PRIORITY
        );
    }

    #[test]
    fn the_median_is_the_middle_value() {
        assert_eq!(median_ms(&mut []), None);
        assert_eq!(median_ms(&mut [5]), Some(5));
        assert_eq!(median_ms(&mut [9, 1, 5]), Some(5));
        assert_eq!(median_ms(&mut [4, 1, 3, 10]), Some(3));
    }
}

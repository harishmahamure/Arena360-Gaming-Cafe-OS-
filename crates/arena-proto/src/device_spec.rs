use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MIN_RAM_GB: u32 = 3;
pub const MIN_FREE_GB: u32 = 20;
pub const MIN_RAM_BYTES: u64 = 2_500_000_000;
pub const MIN_FREE_BYTES: u64 = MIN_FREE_GB as u64 * 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SpecRequirement {
    Ram,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct SpecFailure {
    pub requirement: SpecRequirement,
    #[ts(type = "number")]
    pub found_bytes: u64,
    pub needed_gb: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct DeviceSpec {
    pub failures: Vec<SpecFailure>,
}

impl DeviceSpec {
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn check_device(ram_bytes: u64, free_bytes: u64) -> DeviceSpec {
    let failures = [
        (SpecRequirement::Ram, ram_bytes, MIN_RAM_BYTES, MIN_RAM_GB),
        (
            SpecRequirement::Storage,
            free_bytes,
            MIN_FREE_BYTES,
            MIN_FREE_GB,
        ),
    ]
    .into_iter()
    .filter(|&(_, found, min, _)| found < min)
    .map(|(requirement, found_bytes, _, needed_gb)| SpecFailure {
        requirement,
        found_bytes,
        needed_gb,
    })
    .collect();
    DeviceSpec { failures }
}

pub fn meminfo_total_bytes(meminfo: &str) -> Option<u64> {
    let rest = meminfo.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    let kb: u64 = rest.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kb.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn a_device_at_the_floor_passes() {
        assert!(check_device(MIN_RAM_BYTES, MIN_FREE_BYTES).ok());
        assert!(check_device(8 * GB, 100 * GB).ok());
    }

    #[test]
    fn one_byte_under_fails_that_requirement_only() {
        let d = check_device(MIN_RAM_BYTES - 1, MIN_FREE_BYTES);
        assert_eq!(
            d.failures,
            [SpecFailure {
                requirement: SpecRequirement::Ram,
                found_bytes: MIN_RAM_BYTES - 1,
                needed_gb: 3
            }]
        );
        let d = check_device(MIN_RAM_BYTES, MIN_FREE_BYTES - 1);
        assert_eq!(d.failures.len(), 1);
        assert_eq!(d.failures[0].requirement, SpecRequirement::Storage);
        assert_eq!(d.failures[0].needed_gb, 20);
    }

    #[test]
    fn both_failing_are_both_named_ram_first() {
        let d = check_device(GB, 14 * GB);
        let names: Vec<_> = d.failures.iter().map(|f| f.requirement).collect();
        assert_eq!(names, [SpecRequirement::Ram, SpecRequirement::Storage]);
        assert_eq!(d.failures[1].found_bytes, 14 * GB);
    }

    #[test]
    fn a_marketed_3_gb_phone_reports_less_than_3_gib_and_passes() {
        assert!(check_device(2_860_000 * 1024, 30 * GB).ok());
    }

    #[test]
    fn meminfo_total_is_read_in_bytes() {
        let text = "MemTotal:        2860000 kB\nMemFree:          100 kB\n";
        assert_eq!(meminfo_total_bytes(text), Some(2_860_000 * 1024));
    }

    #[test]
    fn a_meminfo_without_a_readable_total_is_none() {
        for text in [
            "",
            "MemFree: 1 kB",
            "MemTotal: lots kB",
            "MemTotal: 12",
            "MemTotal: -5 kB",
        ] {
            assert_eq!(meminfo_total_bytes(text), None, "{text}");
        }
    }
}

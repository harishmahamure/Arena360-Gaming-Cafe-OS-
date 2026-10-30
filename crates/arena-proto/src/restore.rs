//! Restoring a branch from a `.a360` export file as a new epoch (Story 9.7).
//!
//! Three steps. System → Backup → "Restore from file" **checks** the file (`POST
//! /ui/backup/restore/check`), then, with the Owner's PIN and the typed branch code, **stages** it
//! and stops the Hub (`POST /ui/backup/restore`). At the next start, before the writer opens, the
//! Hub swaps the database, opens the new epoch and records `ops.restore` ([`OpsRestore`]), which
//! also sets `backup.last_restore`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ExportHeader, RestorePosition};

/// The settings key `ops.restore` writes: the last restore that finished.
pub const KEY_BACKUP_LAST_RESTORE: &str = "backup.last_restore";
/// Before any restore.
pub const DEFAULT_BACKUP_LAST_RESTORE: &str = "";
/// The `actor.id` of the system actor that records a restore when the Owner who confirmed it is
/// not in the restored database (they were added after the file was made).
pub const RESTORE_SYSTEM_ACTOR: &str = "restore";
/// The `devices.token_hash` of a paired device after the branch was rebuilt from the cloud backup
/// (Story 9.13): it is not an Argon2 hash, so nothing verifies against it, and it is not empty,
/// so the device is not mistaken for one added by hand. Staff pair the device again.
pub const PAIR_AGAIN_TOKEN: &str = "!pair-again";
/// How long a replaced database stays in `orphaned/` before the Hub deletes it, in days.
pub const ORPHAN_KEEP_DAYS: u64 = 90;

const MAX_TEXT_CHARS: usize = 100;

/// The payload of `POST /ui/backup/restore/check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreCheckRequest {
    /// The `.a360` file on this PC.
    pub path: String,
    /// The Owner's PIN, when the signed-in member is not the Owner.
    #[serde(default)]
    #[ts(optional)]
    pub pin: Option<String>,
}

/// What a checked file holds and what a restore of it would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreCheck {
    /// The branch the file belongs to (this branch).
    pub branch_name: String,
    pub branch_code: String,
    /// The file's own name.
    pub file_name: String,
    /// The size of the file.
    #[ts(type = "number")]
    pub bytes: i64,
    /// When the export was made, UTC ms.
    #[ts(type = "number")]
    pub exported_at: i64,
    /// The epoch of the last change row in the file.
    pub epoch: String,
    /// That row's `seq`.
    #[ts(type = "number")]
    pub last_seq: i64,
    /// The newest migration the file has (older than this build's is migrated at the restore).
    pub schema_version: u32,
    /// How many change rows of this PC's database are newer than the file and will be set aside.
    #[ts(type = "number")]
    pub rows_set_aside: i64,
    /// The consequences, in words, for the confirmation.
    pub warning: String,
    /// Players erased after the export may come back: Free tier and this PC's database holds no
    /// erasure to re-apply (a new PC).
    pub erased_may_reappear: bool,
}

/// The payload of `POST /ui/backup/restore`: stages the file and restarts the Hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreRequest {
    pub path: String,
    /// The Owner's PIN (the signed-in Owner's own, or an Owner's when another member is signed in).
    pub pin: String,
    /// The branch code, typed.
    pub branch_code: String,
}

/// The answer of `POST /ui/backup/restore`: the file is staged and the Hub is stopping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct RestoreStaged {
    pub restarting: bool,
}

/// `event: epoch.changed` on `GET /ui/events`: the lease epoch of this Hub changed (a restore, a
/// promote or a lease taken over). `epoch` is `<counter>:<instance>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct EpochChangedEvent {
    pub epoch: String,
    #[ts(type = "number")]
    pub counter: i64,
    pub provisional: bool,
}

/// What `ops.restore` records, and (as JSON) the value of `backup.last_restore`: the file's header,
/// the epochs either side of the restore and what was set aside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct OpsRestore {
    /// The file's name as the Owner gave it.
    pub file_name: String,
    /// The header of the file that was restored.
    pub header: ExportHeader,
    /// The epoch of the replaced database's lease (empty when it had none).
    pub old_epoch: String,
    /// The new provisional epoch, `<counter>:<instance>`.
    pub new_epoch: String,
    /// Change rows of the replaced database above the file's last `seq`.
    #[ts(type = "number")]
    pub rows_set_aside: i64,
    /// Players erased in the replaced database and erased again in the restored one.
    #[ts(type = "number")]
    pub erasures_reapplied: i64,
    /// Where the replaced database went, inside `orphaned/`.
    pub orphaned_file: String,
    /// When the swap finished, UTC ms.
    #[ts(type = "number")]
    pub restored_at: i64,
    /// The Owner who confirmed (their display name), set by the Hub when it applies the record.
    #[serde(default)]
    pub by: Option<String>,
    /// What the rebuild used, when the branch was rebuilt from the cloud backup (Story 9.13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cloud: Option<CloudRestoreFacts>,
}

/// What a rebuild from the cloud backup (Story 9.13) replayed, for the audit record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CloudRestoreFacts {
    /// The business day of the snapshot the rebuild started from, if there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snapshot_date: Option<String>,
    /// The first and last change of the batches replayed after the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub first: Option<RestorePosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last: Option<RestorePosition>,
    /// Batches replayed.
    pub batches: u32,
    /// Changes applied from them.
    #[ts(type = "number")]
    pub applied: i64,
    /// Changes of a forked epoch's parent above its fork point, kept out of state.
    #[ts(type = "number")]
    pub orphaned: i64,
    /// Player keys restored from the key backup.
    #[ts(type = "number")]
    pub keys_restored: i64,
}

impl CloudRestoreFacts {
    fn is_consistent(&self) -> bool {
        self.snapshot_date
            .as_deref()
            .is_none_or(|d| !d.is_empty() && text_ok(d))
            && self
                .first
                .iter()
                .chain(self.last.iter())
                .all(|p| !p.epoch.is_empty() && text_ok(&p.epoch) && p.seq >= 0)
            && self.applied >= 0
            && self.orphaned >= 0
            && self.keys_restored >= 0
    }
}

/// `yyyy-mm-dd hh:mm` of `ms` in the branch's zone (UTC for an unknown zone).
pub fn restore_time_text(ms: i64, tz: &str) -> String {
    use chrono::TimeZone;
    let zone = tz.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    zone.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| chrono_tz::UTC.timestamp_opt(0, 0).unwrap())
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// The consequences of a restore, as the confirmation shows them. On Free, when this PC has no
/// erasure to re-apply, it also says players erased after the export may reappear.
pub fn restore_warning(exported_at_ms: i64, tz: &str, erased_may_reappear: bool) -> String {
    let time = restore_time_text(exported_at_ms, tz);
    let mut text = format!(
        "Everything after {time} on this PC will be set aside. Kiosks reconnect by themselves."
    );
    if erased_may_reappear {
        text.push_str(&format!(" Players erased after {time} may reappear."));
    }
    text
}

fn text_ok(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT_CHARS && !s.chars().any(char::is_control)
}

fn plain_file(s: &str) -> bool {
    !s.is_empty() && text_ok(s) && !s.contains(['/', '\\'])
}

impl OpsRestore {
    /// Whether the facts could describe a restore this build performs.
    pub fn is_consistent(&self) -> bool {
        plain_file(&self.file_name)
            && plain_file(&self.orphaned_file)
            && text_ok(&self.old_epoch)
            && !self.new_epoch.is_empty()
            && text_ok(&self.new_epoch)
            && !self.header.branch_id.is_empty()
            && self.header.last_seq >= 0
            && self.header.exported_at > 0
            && text_ok(&self.header.epoch)
            && self.rows_set_aside >= 0
            && self.erasures_reapplied >= 0
            && self.restored_at > 0
            && self.by.as_ref().is_none_or(|b| !b.is_empty() && text_ok(b))
            && self
                .cloud
                .as_ref()
                .is_none_or(CloudRestoreFacts::is_consistent)
    }

    /// The value of `backup.last_restore`, or `None` when it is empty or malformed.
    pub fn parse(value: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(value).ok()?;
        parsed.is_consistent().then_some(parsed)
    }

    /// The JSON stored in `backup.last_restore`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EXPORT_FORMAT_VERSION;

    fn facts() -> OpsRestore {
        OpsRestore {
            file_name: "LVL01-2027-01-15-1330.a360".into(),
            header: ExportHeader {
                format_version: EXPORT_FORMAT_VERSION,
                branch_id: "01JBRANCH".into(),
                branch_code: "LVL01".into(),
                epoch: "3:abc".into(),
                last_seq: 120,
                schema_version: 84,
                exported_at: 1_800_000_000_000,
                sha256: "ab".repeat(32),
            },
            old_epoch: "4:abc".into(),
            new_epoch: "5:abc".into(),
            rows_set_aside: 31,
            erasures_reapplied: 1,
            orphaned_file: "4-abc-20270115T133000.db".into(),
            restored_at: 1_800_000_100_000,
            by: None,
            cloud: None,
        }
    }

    #[test]
    fn the_recorded_facts_round_trip_and_reject_nonsense() {
        let f = facts();
        assert!(f.is_consistent());
        assert_eq!(OpsRestore::parse(&f.to_json()), Some(f.clone()));
        assert_eq!(OpsRestore::parse(""), None);
        for bad in [
            OpsRestore {
                file_name: "../x.a360".into(),
                ..f.clone()
            },
            OpsRestore {
                orphaned_file: "orphaned/x.db".into(),
                ..f.clone()
            },
            OpsRestore {
                new_epoch: String::new(),
                ..f.clone()
            },
            OpsRestore {
                rows_set_aside: -1,
                ..f.clone()
            },
            OpsRestore {
                restored_at: 0,
                ..f.clone()
            },
            OpsRestore {
                by: Some(String::new()),
                ..f.clone()
            },
        ] {
            assert!(!bad.is_consistent(), "{bad:?}");
            assert_eq!(OpsRestore::parse(&bad.to_json()), None);
        }
    }

    #[test]
    fn the_cloud_facts_are_recorded_and_checked() {
        let cloud = CloudRestoreFacts {
            snapshot_date: Some("2027-01-14".into()),
            first: Some(RestorePosition {
                epoch: "3:abc".into(),
                seq: 121,
            }),
            last: Some(RestorePosition {
                epoch: "3:abc".into(),
                seq: 150,
            }),
            batches: 2,
            applied: 30,
            orphaned: 2,
            keys_restored: 4,
        };
        let f = OpsRestore {
            cloud: Some(cloud.clone()),
            ..facts()
        };
        assert!(f.is_consistent());
        assert_eq!(OpsRestore::parse(&f.to_json()), Some(f.clone()));
        for bad in [
            CloudRestoreFacts {
                applied: -1,
                ..cloud.clone()
            },
            CloudRestoreFacts {
                orphaned: -1,
                ..cloud.clone()
            },
            CloudRestoreFacts {
                keys_restored: -1,
                ..cloud.clone()
            },
            CloudRestoreFacts {
                last: Some(RestorePosition {
                    epoch: String::new(),
                    seq: 1,
                }),
                ..cloud.clone()
            },
        ] {
            let bad = OpsRestore {
                cloud: Some(bad),
                ..f.clone()
            };
            assert!(!bad.is_consistent(), "{bad:?}");
            assert_eq!(OpsRestore::parse(&bad.to_json()), None);
        }
    }

    #[test]
    fn the_warning_names_the_export_time_in_the_branch_zone() {
        // 2027-01-15 08:00 UTC is 13:30 in Kolkata.
        let at = 1_800_000_000_000;
        assert_eq!(
            restore_warning(at, "Asia/Kolkata", false),
            "Everything after 2027-01-15 13:30 on this PC will be set aside. Kiosks reconnect by themselves."
        );
        assert!(
            restore_warning(at, "UTC", true)
                .ends_with("Players erased after 2027-01-15 08:00 may reappear.")
        );
    }

    #[test]
    fn a_payload_with_an_unknown_field_is_refused() {
        let mut v = serde_json::to_value(facts()).unwrap();
        v["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<OpsRestore>(v).is_err());
    }
}

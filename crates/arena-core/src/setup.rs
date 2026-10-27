use std::path::Path;
use std::sync::Arc;

use tokio::sync::{broadcast, watch};

use crate::{Applied, HubStatus, SnapshotReader};

use arena_proto::{
    AdConsentState, AdsConsent, AppError, BACKUP_PROBE_PREFIX, BackupDirProblem, ErrorCode,
    KEY_ADS_CONSENT, KEY_BACKUP_DIR, KEY_OPENING_HOURS_SET, KEY_PRINTER_STATUS, KEY_SETUP_LIVE,
    KEY_TEST_SESSION, PrinterState, PrinterStatus, SetupStatus, TestSessionStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubTier {
    Free,
    Paid,
}

pub fn hub_tier() -> HubTier {
    HubTier::Free
}

pub fn setup_status<'a>(
    value: impl Fn(&str) -> Option<&'a str>,
    tier: HubTier,
    products_count: u32,
    groups_without_rate: u32,
    manager_exists: bool,
) -> SetupStatus {
    let ad_consent = match tier {
        HubTier::Paid => AdConsentState::NotNeeded,
        HubTier::Free => match value(KEY_ADS_CONSENT).and_then(AdsConsent::parse) {
            Some(c) if c.given => AdConsentState::Given,
            _ => AdConsentState::Missing,
        },
    };
    SetupStatus {
        opening_hours_set: value(KEY_OPENING_HOURS_SET) == Some("true"),
        backup_location_set: value(KEY_BACKUP_DIR).is_some_and(|v| !v.trim().is_empty()),
        ad_consent,
        printer: value(KEY_PRINTER_STATUS)
            .and_then(PrinterStatus::parse)
            .map_or(PrinterState::Unset, |s| s.state),
        products_count,
        groups_without_rate,
        manager_exists,
        test_session: value(KEY_TEST_SESSION)
            .and_then(TestSessionStatus::parse)
            .map_or_else(Default::default, |s| s.state),
        live: value(KEY_SETUP_LIVE) == Some("true"),
    }
}

pub fn probe_backup_dir(path: &Path) -> Result<(), BackupDirProblem> {
    if !path.is_absolute() {
        return Err(BackupDirProblem::NotFound);
    }
    let meta = std::fs::metadata(path).map_err(|_| BackupDirProblem::NotFound)?;
    if !meta.is_dir() {
        return Err(BackupDirProblem::NotDir);
    }
    let probe = path.join(format!("{BACKUP_PROBE_PREFIX}{}", ulid::Ulid::generate()));
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);
    match created {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(&probe).map_err(|e| {
                tracing::warn!(error = %e, "the backup location probe file could not be removed");
                BackupDirProblem::NotWritable
            })
        }
        Err(_) => Err(BackupDirProblem::NotWritable),
    }
}

pub fn backup_dir_error(problem: BackupDirProblem) -> AppError {
    AppError::with_field(
        ErrorCode::ValidationFailed,
        KEY_BACKUP_DIR,
        problem.as_str(),
    )
}

/// A folder the platform grants access to by a URI rather than a path (the Android folder picker
/// returns `content://` tree URIs). The Android shell supplies one; the Hub never reads a URI
/// itself.
pub trait BackupFolder: Send + Sync {
    /// Creates and deletes a probe file in the folder. Blocking.
    fn probe(&self, location: &str) -> Result<(), BackupDirProblem>;
}

/// Checks a stored backup location: a tree URI through `folder`, anything else as a path.
pub fn probe_location(
    folder: Option<&dyn BackupFolder>,
    location: &str,
) -> Result<(), BackupDirProblem> {
    if arena_proto::is_backup_tree_uri(location) {
        return folder.map_or(Err(BackupDirProblem::NotFound), |f| f.probe(location));
    }
    probe_backup_dir(Path::new(location))
}

/// How long one check of the chosen folder may take before it counts as unavailable.
const BACKUP_WATCH_PROBE: std::time::Duration = std::time::Duration::from_secs(10);

/// Checks the chosen backup folder at start, whenever the backup location changes and then every
/// `every`. While it cannot be written, setup status reports the location as not set (the System
/// screen then offers "Choose again"); the next good check restores it. Runs until aborted.
pub async fn watch_backup_folder(
    reader: Arc<SnapshotReader>,
    events: Option<broadcast::Receiver<Applied>>,
    status: watch::Sender<HubStatus>,
    folder: Arc<dyn BackupFolder>,
    every: std::time::Duration,
) {
    let mut events = events;
    loop {
        let r = Arc::clone(&reader);
        let f = Arc::clone(&folder);
        let job = tokio::task::spawn_blocking(move || match r.setting_value(KEY_BACKUP_DIR) {
            Ok(Some(v)) if !v.trim().is_empty() => probe_location(Some(&*f), &v).is_err(),
            Ok(_) => false,
            // A read failure says nothing about the folder: keep the last answer.
            Err(_) => r.backup_unavailable(),
        });
        let unavailable = match tokio::time::timeout(BACKUP_WATCH_PROBE, job).await {
            Ok(Ok(unavailable)) => unavailable,
            Ok(Err(_)) => false,
            Err(_) => {
                tracing::warn!("the backup folder check timed out");
                true
            }
        };
        if reader.set_backup_unavailable(unavailable) {
            tracing::info!(unavailable, "the backup folder changed availability");
            refresh(&reader, &status).await;
        }
        let wait = tokio::time::sleep(every);
        tokio::pin!(wait);
        loop {
            let Some(rx) = events.as_mut() else {
                (&mut wait).await;
                break;
            };
            tokio::select! {
                () = &mut wait => break,
                got = rx.recv() => match got {
                    Ok(applied) => {
                        if applied.changes.iter().any(|c| c.tbl == "settings") {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => break,
                    Err(broadcast::error::RecvError::Closed) => events = None,
                },
            }
        }
    }
}

async fn refresh(reader: &Arc<SnapshotReader>, status: &watch::Sender<HubStatus>) {
    let r = Arc::clone(reader);
    match tokio::task::spawn_blocking(move || r.setup()).await {
        Ok(Ok(setup)) => {
            status.send_if_modified(|s| {
                if s.setup == setup {
                    return false;
                }
                s.live = setup.is_some_and(|x| x.live);
                s.setup = setup;
                true
            });
        }
        Ok(Err(e)) => {
            tracing::warn!(code = e.code.as_str(), detail = %e.message, "setup status read failed")
        }
        Err(e) => tracing::warn!(detail = %e, "setup status read task failed"),
    }
}

pub async fn publish_setup(
    reader: Arc<SnapshotReader>,
    events: Option<broadcast::Receiver<Applied>>,
    status: watch::Sender<HubStatus>,
) {
    refresh(&reader, &status).await;
    let Some(mut events) = events else {
        return;
    };
    loop {
        match events.recv().await {
            Ok(applied) => {
                if applied
                    .changes
                    .iter()
                    .any(|c| matches!(c.tbl.as_str(), "settings" | "branch" | "products"))
                {
                    refresh(&reader, &status).await;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => refresh(&reader, &status).await,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::DEFAULT_ADS_CONSENT;

    fn status(rows: &[(&str, &str)], tier: HubTier) -> SetupStatus {
        setup_status(
            |k| rows.iter().find(|(key, _)| *key == k).map(|(_, v)| *v),
            tier,
            0,
            0,
            false,
        )
    }

    #[test]
    fn the_live_flag_follows_the_setting() {
        assert!(!status(&[], HubTier::Free).live);
        assert!(status(&[(KEY_SETUP_LIVE, "true")], HubTier::Free).live);
        assert!(!status(&[(KEY_SETUP_LIVE, "false")], HubTier::Free).live);
    }

    #[test]
    fn setup_status_matrix() {
        let defaults = [
            (KEY_OPENING_HOURS_SET, "false"),
            (KEY_BACKUP_DIR, ""),
            (KEY_ADS_CONSENT, DEFAULT_ADS_CONSENT),
        ];
        assert_eq!(
            status(&defaults, HubTier::Free),
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: false,
                ad_consent: AdConsentState::Missing,
                printer: PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
            }
        );
        let given = r#"{"given":true,"at":1,"staff_id":"S"}"#;
        let done = [
            (KEY_OPENING_HOURS_SET, "true"),
            (KEY_BACKUP_DIR, "/mnt/backup"),
            (KEY_ADS_CONSENT, given),
            (
                KEY_PRINTER_STATUS,
                r#"{"state":"skipped","error":null,"at":3}"#,
            ),
        ];
        assert_eq!(
            status(&done, HubTier::Free),
            SetupStatus {
                opening_hours_set: true,
                backup_location_set: true,
                ad_consent: AdConsentState::Given,
                printer: PrinterState::Skipped,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
            }
        );
        assert_eq!(
            status(&defaults, HubTier::Paid).ad_consent,
            AdConsentState::NotNeeded
        );
        assert_eq!(
            status(&done, HubTier::Paid).ad_consent,
            AdConsentState::NotNeeded
        );
        let withdrawn = [(KEY_ADS_CONSENT, r#"{"given":false,"at":2,"staff_id":"S"}"#)];
        assert_eq!(
            status(&withdrawn, HubTier::Free).ad_consent,
            AdConsentState::Missing
        );
        assert_eq!(
            status(&[(KEY_ADS_CONSENT, "junk")], HubTier::Free).ad_consent,
            AdConsentState::Missing
        );
        assert_eq!(
            status(&[], HubTier::Free),
            SetupStatus {
                opening_hours_set: false,
                backup_location_set: false,
                ad_consent: AdConsentState::Missing,
                printer: PrinterState::Unset,
                products_count: 0,
                groups_without_rate: 0,
                manager_exists: false,
                test_session: Default::default(),
                live: false,
            }
        );
        for (value, state) in [
            (r#"{"state":"ok","error":null,"at":1}"#, PrinterState::Ok),
            (
                r#"{"state":"failed","error_kind":"refused","error":null,"at":1}"#,
                PrinterState::Failed,
            ),
            (arena_proto::DEFAULT_PRINTER_STATUS, PrinterState::Unset),
            ("junk", PrinterState::Unset),
        ] {
            assert_eq!(
                status(&[(KEY_PRINTER_STATUS, value)], HubTier::Free).printer,
                state,
                "{value}"
            );
        }
        assert!(!status(&[(KEY_BACKUP_DIR, "  ")], HubTier::Free).backup_location_set);
        assert_eq!(hub_tier(), HubTier::Free);
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(BACKUP_PROBE_PREFIX))
            .collect()
    }

    struct Folder(Result<(), BackupDirProblem>);

    impl BackupFolder for Folder {
        fn probe(&self, _location: &str) -> Result<(), BackupDirProblem> {
            self.0
        }
    }

    #[test]
    fn a_tree_uri_goes_to_the_platform_folder_and_a_path_to_the_disk() {
        let tree = "content://com.android.externalstorage.documents/tree/primary%3ABk";
        assert_eq!(probe_location(Some(&Folder(Ok(()))), tree), Ok(()));
        assert_eq!(
            probe_location(Some(&Folder(Err(BackupDirProblem::NotWritable))), tree),
            Err(BackupDirProblem::NotWritable)
        );
        assert_eq!(probe_location(None, tree), Err(BackupDirProblem::NotFound));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let refusing = Folder(Err(BackupDirProblem::NotFound));
        assert_eq!(probe_location(Some(&refusing), &path), Ok(()));
    }
    #[test]
    fn a_writable_dir_passes_and_the_probe_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(probe_backup_dir(dir.path()), Ok(()));
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn missing_relative_and_file_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_backup_dir(&dir.path().join("nope")),
            Err(BackupDirProblem::NotFound)
        );
        assert_eq!(
            probe_backup_dir(Path::new("relative/dir")),
            Err(BackupDirProblem::NotFound)
        );
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(probe_backup_dir(&file), Err(BackupDirProblem::NotDir));
        let e = backup_dir_error(BackupDirProblem::NotDir);
        assert_eq!(e.code, ErrorCode::ValidationFailed);
        assert_eq!(e.field.as_deref(), Some("system.backup_dir"));
        assert_eq!(e.message, "not_dir");
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_dir_is_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let can_write_anyway = std::fs::write(ro.join("root-check"), b"x").is_ok();
        let result = probe_backup_dir(&ro);
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        if can_write_anyway {
            let sealed = if cfg!(target_os = "linux") {
                "/proc/self"
            } else {
                "/"
            };
            assert_eq!(
                probe_backup_dir(Path::new(sealed)),
                Err(BackupDirProblem::NotWritable)
            );
            return;
        }
        assert_eq!(result, Err(BackupDirProblem::NotWritable));
        assert!(leftovers(&ro).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_deny_write_acl_is_not_writable() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        let denied = Command::new("icacls")
            .arg(&ro)
            .args(["/deny", "*S-1-1-0:(OI)(CI)(W)"])
            .status()
            .is_ok_and(|s| s.success());
        if !denied {
            eprintln!("skipped: icacls could not add a deny-write entry");
            return;
        }
        let result = probe_backup_dir(&ro);
        let _ = Command::new("icacls")
            .arg(&ro)
            .args(["/remove:d", "*S-1-1-0"])
            .status();
        assert_eq!(result, Err(BackupDirProblem::NotWritable));
    }
}
